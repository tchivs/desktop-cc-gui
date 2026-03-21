import path from "node:path";
import process from "node:process";
import { ESLint } from "eslint";
import ts from "typescript";

const APP_SHELL_FILE = "src/app-shell.tsx";
const SEARCH_SECTION_FILE = "src/app-shell-parts/useAppShellSearchAndComposerSection.ts";
const SECTIONS_FILE = "src/app-shell-parts/useAppShellSections.ts";
const LAYOUT_FILE = "src/app-shell-parts/useAppShellLayoutNodesSection.tsx";
const RENDER_FILE = "src/app-shell-parts/renderAppShell.tsx";

const CONTRACT_FILES = [
  APP_SHELL_FILE,
  SEARCH_SECTION_FILE,
  SECTIONS_FILE,
  LAYOUT_FILE,
  RENDER_FILE,
];

const PARSER_OPTIONS_JSON = JSON.stringify({
  ecmaVersion: 2022,
  sourceType: "module",
  ecmaFeatures: { jsx: true },
});

function toAbsolutePath(relativePath) {
  return path.resolve(process.cwd(), relativePath);
}

function getProgramSourceFile(program, relativePath) {
  const absolutePath = toAbsolutePath(relativePath);
  const sourceFile = program.getSourceFile(absolutePath);
  if (!sourceFile) {
    throw new Error(`Cannot load source file "${relativePath}".`);
  }
  return sourceFile;
}

function visitNode(node, callback) {
  callback(node);
  node.forEachChild((child) => visitNode(child, callback));
}

function getPropertyNameText(name) {
  if (!name) {
    return null;
  }
  if (
    ts.isIdentifier(name) ||
    ts.isStringLiteral(name) ||
    ts.isNumericLiteral(name)
  ) {
    return name.text;
  }
  return null;
}

function collectObjectLiteralOwnKeys(objectLiteral) {
  const keys = new Set();
  for (const property of objectLiteral.properties) {
    if (ts.isShorthandPropertyAssignment(property)) {
      keys.add(property.name.text);
      continue;
    }
    if (
      ts.isPropertyAssignment(property) ||
      ts.isMethodDeclaration(property) ||
      ts.isGetAccessorDeclaration(property) ||
      ts.isSetAccessorDeclaration(property)
    ) {
      const key = getPropertyNameText(property.name);
      if (key) {
        keys.add(key);
      }
    }
  }
  return keys;
}

function findFunctionDeclaration(sourceFile, functionName) {
  for (const statement of sourceFile.statements) {
    if (
      ts.isFunctionDeclaration(statement) &&
      statement.name?.text === functionName
    ) {
      if (!statement.body) {
        throw new Error(`Function "${functionName}" has no body.`);
      }
      return statement;
    }
  }
  throw new Error(`Cannot find function "${functionName}" in ${sourceFile.fileName}.`);
}

function getVariableObjectLiteral(sourceFile, variableName) {
  let result = null;
  visitNode(sourceFile, (node) => {
    if (result) {
      return;
    }
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === variableName &&
      node.initializer &&
      ts.isObjectLiteralExpression(node.initializer)
    ) {
      result = node.initializer;
    }
  });

  if (!result) {
    throw new Error(`Cannot find object literal variable "${variableName}" in ${sourceFile.fileName}.`);
  }
  return result;
}

function getVariableObjectLiteralKeys(sourceFile, variableName) {
  return collectObjectLiteralOwnKeys(
    getVariableObjectLiteral(sourceFile, variableName),
  );
}

function checkObjectLiteralShorthandBindings(sourceFile, variableName, checker) {
  const objectLiteral = getVariableObjectLiteral(sourceFile, variableName);
  const issues = [];

  for (const property of objectLiteral.properties) {
    if (!ts.isShorthandPropertyAssignment(property)) {
      continue;
    }
    const symbol = checker.getSymbolAtLocation(property.name);
    if (!symbol) {
      issues.push(
        `[${variableName}] shorthand "${property.name.text}" has no resolvable symbol.`,
      );
      continue;
    }
    const declarations = symbol.declarations ?? [];
    const hasLocalDeclaration = declarations.some(
      (declaration) => declaration.getSourceFile().fileName === sourceFile.fileName,
    );
    if (!hasLocalDeclaration) {
      issues.push(
        `[${variableName}] shorthand "${property.name.text}" resolves to a non-local symbol (likely global).`,
      );
    }
  }

  return issues;
}

function isTypePosition(node) {
  let current = node.parent;
  while (current) {
    if (ts.isTypeNode(current) || ts.isExpressionWithTypeArguments(current)) {
      return true;
    }
    current = current.parent;
  }
  return false;
}

function isDeclarationIdentifier(node) {
  const parent = node.parent;
  if (!parent) {
    return false;
  }
  if (ts.isBindingElement(parent) && (parent.name === node || parent.propertyName === node)) {
    return true;
  }
  if (ts.isVariableDeclaration(parent) && parent.name === node) {
    return true;
  }
  if (ts.isParameter(parent) && parent.name === node) {
    return true;
  }
  if (ts.isFunctionDeclaration(parent) && parent.name === node) {
    return true;
  }
  if (ts.isFunctionExpression(parent) && parent.name === node) {
    return true;
  }
  if (ts.isClassDeclaration(parent) && parent.name === node) {
    return true;
  }
  if (ts.isImportClause(parent) && parent.name === node) {
    return true;
  }
  if (ts.isImportSpecifier(parent) && (parent.name === node || parent.propertyName === node)) {
    return true;
  }
  if (ts.isNamespaceImport(parent) && parent.name === node) {
    return true;
  }
  return false;
}

function getCtxDestructureKeys(sourceFile, functionName, checker) {
  const fn = findFunctionDeclaration(sourceFile, functionName);
  const allKeys = new Set();
  const bindingSymbols = new Map();
  const bindingNameNodes = new Set();

  visitNode(fn.body, (node) => {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isObjectBindingPattern(node.name) &&
      node.initializer &&
      ts.isIdentifier(node.initializer) &&
      node.initializer.text === "ctx"
    ) {
      for (const element of node.name.elements) {
        if (element.dotDotDotToken) {
          continue;
        }
        if (!ts.isIdentifier(element.name)) {
          continue;
        }
        bindingNameNodes.add(element.name);
        const symbol = checker.getSymbolAtLocation(element.name);
        const key = element.propertyName
          ? getPropertyNameText(element.propertyName)
          : element.name.text;
        if (!key) {
          continue;
        }
        allKeys.add(key);
        if (symbol) {
          bindingSymbols.set(symbol, key);
        }
      }
    }
  });

  if (allKeys.size === 0) {
    throw new Error(`Cannot find "const { ... } = ctx" destructure in ${sourceFile.fileName}.`);
  }

  const usedKeys = new Set();
  visitNode(fn.body, (node) => {
    if (!ts.isIdentifier(node)) {
      return;
    }
    if (bindingNameNodes.has(node)) {
      return;
    }
    if (isDeclarationIdentifier(node) || isTypePosition(node)) {
      return;
    }
    const symbol = checker.getSymbolAtLocation(node);
    if (!symbol) {
      return;
    }
    const key = bindingSymbols.get(symbol);
    if (!key) {
      return;
    }
    usedKeys.add(key);
  });

  // Fallback to all destructured keys if symbol analysis cannot find usages.
  if (usedKeys.size === 0) {
    return allKeys;
  }

  return usedKeys;
}

function getReturnObjectKeys(sourceFile, functionName) {
  const fn = findFunctionDeclaration(sourceFile, functionName);
  const keys = new Set();
  visitNode(fn.body, (node) => {
    if (
      ts.isReturnStatement(node) &&
      node.expression &&
      ts.isObjectLiteralExpression(node.expression)
    ) {
      for (const key of collectObjectLiteralOwnKeys(node.expression)) {
        keys.add(key);
      }
    }
  });

  if (keys.size === 0) {
    throw new Error(`Cannot find object literal return in function "${functionName}" (${sourceFile.fileName}).`);
  }
  return keys;
}

function getFirstCallArgument(sourceFile, calleeName) {
  let result = null;
  visitNode(sourceFile, (node) => {
    if (result) {
      return;
    }
    if (!ts.isCallExpression(node)) {
      return;
    }
    if (ts.isIdentifier(node.expression) && node.expression.text === calleeName) {
      result = node.arguments[0] ?? null;
    }
  });

  if (!result) {
    throw new Error(`Cannot find call "${calleeName}(...)".`);
  }
  return result;
}

function resolveProvidedKeysFromArgument(argumentNode, sourceSetsByIdentifier) {
  if (ts.isIdentifier(argumentNode)) {
    const keys = sourceSetsByIdentifier.get(argumentNode.text);
    if (!keys) {
      throw new Error(`Unknown argument source "${argumentNode.text}".`);
    }
    return { keys: new Set(keys), unresolvedSpreads: [] };
  }

  if (!ts.isObjectLiteralExpression(argumentNode)) {
    throw new Error(`Unsupported argument node kind: ${ts.SyntaxKind[argumentNode.kind]}.`);
  }

  const keys = new Set();
  const unresolvedSpreads = [];

  for (const property of argumentNode.properties) {
    if (ts.isSpreadAssignment(property)) {
      const spreadExpr = property.expression;
      if (ts.isIdentifier(spreadExpr)) {
        const spreadKeys = sourceSetsByIdentifier.get(spreadExpr.text);
        if (!spreadKeys) {
          unresolvedSpreads.push(spreadExpr.text);
          continue;
        }
        for (const key of spreadKeys) {
          keys.add(key);
        }
        continue;
      }
      if (ts.isObjectLiteralExpression(spreadExpr)) {
        const nested = resolveProvidedKeysFromArgument(
          spreadExpr,
          sourceSetsByIdentifier,
        );
        for (const key of nested.keys) {
          keys.add(key);
        }
        unresolvedSpreads.push(...nested.unresolvedSpreads);
        continue;
      }
      unresolvedSpreads.push(spreadExpr.getText());
      continue;
    }

    if (ts.isShorthandPropertyAssignment(property)) {
      keys.add(property.name.text);
      continue;
    }

    if (
      ts.isPropertyAssignment(property) ||
      ts.isMethodDeclaration(property) ||
      ts.isGetAccessorDeclaration(property) ||
      ts.isSetAccessorDeclaration(property)
    ) {
      const key = getPropertyNameText(property.name);
      if (key) {
        keys.add(key);
      }
    }
  }

  return { keys, unresolvedSpreads };
}

function sorted(items) {
  return [...items].sort((a, b) => a.localeCompare(b));
}

async function runNoUndefCheck() {
  const eslint = new ESLint({
    useEslintrc: false,
    overrideConfig: {
      env: {
        browser: true,
        es2021: true,
        node: true,
      },
      parser: "@typescript-eslint/parser",
      parserOptions: JSON.parse(PARSER_OPTIONS_JSON),
      rules: {
        "no-undef": "error",
      },
    },
    errorOnUnmatchedPattern: false,
  });

  const results = await eslint.lintFiles(CONTRACT_FILES);
  const errorCount = results.reduce((total, item) => total + item.errorCount, 0);
  if (errorCount > 0) {
    const formatter = await eslint.loadFormatter("stylish");
    const formatted = formatter.format(results);
    if (formatted) {
      console.error(formatted);
    }
    throw new Error("no-undef check failed.");
  }
}

function checkContract(contract) {
  const { name, requiredKeys, providedKeys, unresolvedSpreads } = contract;
  const issues = [];

  if (unresolvedSpreads.length > 0) {
    issues.push(
      `[${name}] unresolved spread source(s): ${sorted(unresolvedSpreads).join(", ")}`,
    );
  }

  const missingKeys = sorted(
    [...requiredKeys].filter((key) => !providedKeys.has(key)),
  );
  if (missingKeys.length > 0) {
    issues.push(`[${name}] missing ${missingKeys.length} key(s): ${missingKeys.join(", ")}`);
  }

  return issues;
}

async function main() {
  await runNoUndefCheck();

  const program = ts.createProgram({
    rootNames: CONTRACT_FILES.map((file) => toAbsolutePath(file)),
    options: {
      target: ts.ScriptTarget.ESNext,
      module: ts.ModuleKind.ESNext,
      jsx: ts.JsxEmit.Preserve,
      skipLibCheck: true,
    },
  });
  const checker = program.getTypeChecker();

  const appShellSource = getProgramSourceFile(program, APP_SHELL_FILE);
  const searchSource = getProgramSourceFile(program, SEARCH_SECTION_FILE);
  const sectionsSource = getProgramSourceFile(program, SECTIONS_FILE);
  const layoutSource = getProgramSourceFile(program, LAYOUT_FILE);
  const renderSource = getProgramSourceFile(program, RENDER_FILE);

  const appShellContextKeys = getVariableObjectLiteralKeys(
    appShellSource,
    "appShellContext",
  );
  const appShellContextShorthandIssues = checkObjectLiteralShorthandBindings(
    appShellSource,
    "appShellContext",
    checker,
  );
  const searchReturnKeys = getReturnObjectKeys(
    searchSource,
    "useAppShellSearchAndComposerSection",
  );
  const sectionsReturnKeys = getReturnObjectKeys(
    sectionsSource,
    "useAppShellSections",
  );
  const layoutReturnKeys = getReturnObjectKeys(
    layoutSource,
    "useAppShellLayoutNodesSection",
  );

  const searchRequiredKeys = getCtxDestructureKeys(
    searchSource,
    "useAppShellSearchAndComposerSection",
    checker,
  );
  const sectionsRequiredKeys = getCtxDestructureKeys(
    sectionsSource,
    "useAppShellSections",
    checker,
  );
  const layoutRequiredKeys = getCtxDestructureKeys(
    layoutSource,
    "useAppShellLayoutNodesSection",
    checker,
  );
  const renderRequiredKeys = getCtxDestructureKeys(
    renderSource,
    "renderAppShell",
    checker,
  );

  const searchProvided = resolveProvidedKeysFromArgument(
    getFirstCallArgument(appShellSource, "useAppShellSearchAndComposerSection"),
    new Map([["appShellContext", appShellContextKeys]]),
  );
  const sectionsProvided = resolveProvidedKeysFromArgument(
    getFirstCallArgument(appShellSource, "useAppShellSections"),
    new Map([
      ["appShellContext", appShellContextKeys],
      ["searchAndComposerSection", searchReturnKeys],
    ]),
  );
  const layoutProvided = resolveProvidedKeysFromArgument(
    getFirstCallArgument(appShellSource, "useAppShellLayoutNodesSection"),
    new Map([
      ["appShellContext", appShellContextKeys],
      ["searchAndComposerSection", searchReturnKeys],
      ["sections", sectionsReturnKeys],
    ]),
  );
  const renderProvided = resolveProvidedKeysFromArgument(
    getFirstCallArgument(appShellSource, "renderAppShell"),
    new Map([
      ["appShellContext", appShellContextKeys],
      ["searchAndComposerSection", searchReturnKeys],
      ["sections", sectionsReturnKeys],
      ["layoutNodes", layoutReturnKeys],
    ]),
  );

  const issues = [
    ...appShellContextShorthandIssues,
    ...checkContract({
      name: "useAppShellSearchAndComposerSection",
      requiredKeys: searchRequiredKeys,
      providedKeys: searchProvided.keys,
      unresolvedSpreads: searchProvided.unresolvedSpreads,
    }),
    ...checkContract({
      name: "useAppShellSections",
      requiredKeys: sectionsRequiredKeys,
      providedKeys: sectionsProvided.keys,
      unresolvedSpreads: sectionsProvided.unresolvedSpreads,
    }),
    ...checkContract({
      name: "useAppShellLayoutNodesSection",
      requiredKeys: layoutRequiredKeys,
      providedKeys: layoutProvided.keys,
      unresolvedSpreads: layoutProvided.unresolvedSpreads,
    }),
    ...checkContract({
      name: "renderAppShell",
      requiredKeys: renderRequiredKeys,
      providedKeys: renderProvided.keys,
      unresolvedSpreads: renderProvided.unresolvedSpreads,
    }),
  ];

  if (issues.length > 0) {
    console.error("check-app-shell-runtime-contract: FAILED");
    for (const issue of issues) {
      console.error(`- ${issue}`);
    }
    process.exit(1);
  }

  console.log("check-app-shell-runtime-contract: OK");
}

try {
  await main();
} catch (error) {
  console.error(
    `check-app-shell-runtime-contract: FAILED\n- ${
      error instanceof Error ? error.message : String(error)
    }`,
  );
  process.exit(1);
}
