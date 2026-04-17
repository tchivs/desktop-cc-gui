// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ConversationItem } from "../../../../types";
import * as diffParser from "../../../../utils/diff";
import { GenericToolBlock } from "./GenericToolBlock";

const askUserItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-1",
  kind: "tool",
  toolType: "toolCall",
  title: "Tool: askuserquestion",
  detail: "{}",
};

const fileChangeItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-2",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: "{}",
  status: "completed",
  changes: [
    { path: "src/App.tsx", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/New.tsx", kind: "added", diff: "@@ -0,0 +1 @@\n+const x = 1;" },
  ],
};

const fileChangeManyItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-2-many",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: "{}",
  status: "completed",
  changes: [
    { path: "src/App.tsx", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/New.tsx", kind: "added", diff: "@@ -0,0 +1 @@\n+const x = 1;" },
    { path: "src/routes/Home.tsx", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/routes/About.tsx", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/lib/api.ts", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/lib/store.ts", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "src/styles/app.css", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
    { path: "package.json", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
  ],
};

const fileChangeWithOutputItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-2-output",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: "{}",
  status: "completed",
  output: "@@ -1 +1 @@\n-old\n+new",
  changes: [
    { path: "src/App.tsx", kind: "modified", diff: "@@ -1 +1 @@\n-old\n+new" },
  ],
};

const fileChangeWithoutInlineDiffItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-2-output-fallback",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: "{}",
  status: "completed",
  output: "@@ -1 +1 @@\n-old\n+new",
  changes: [{ path: "src/App.tsx", kind: "modified" }],
};

const fileChangePathHintCompatItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-2-pathhint-compat",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: JSON.stringify({
    input: {
      file_path: "/repo/src/App.tsx",
      old_string: "const oldValue = 1;",
      new_string: "const newValue = 1;",
    },
  }),
  status: "completed",
  changes: [{ path: "src/App.tsx", kind: "modified" }],
};

const markdownOutputItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-3",
  kind: "tool",
  toolType: "fileChange",
  title: "File changes",
  detail: "{}",
  status: "completed",
  output: "## Summary\n\n| Name | Value |\n| --- | --- |\n| a | b |",
};

const blockedModeItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-4",
  kind: "tool",
  toolType: "modeBlocked",
  title: "Tool: askuserquestion",
  detail: "item/tool/requestUserInput",
  status: "completed",
  output:
    "requestUserInput is blocked while effective_mode=code\n\nSwitch to Plan mode and resend the prompt when user input is needed.",
};

const exitPlanModeItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-5",
  kind: "tool",
  toolType: "toolCall",
  title: "Tool: ExitPlanMode",
  detail:
    "PLAN\n# Plan\n\n## Context\nCreate a few sample files.\n\n## Steps\n- add aaa.txt\n- add bbb.txt\n\nPLANFILEPATH\n/Users/demo/.claude/plans/sample-plan.md",
  status: "completed",
};

const exitPlanModeRawFallbackItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-6",
  kind: "tool",
  toolType: "toolCall",
  title: "Tool: ExitPlanMode",
  detail: "plain exit plan payload without labeled sections",
  status: "completed",
};

const exitPlanModeJsonItem: Extract<ConversationItem, { kind: "tool" }> = {
  id: "tool-7",
  kind: "tool",
  toolType: "toolCall",
  title: "Tool: ExitPlanMode",
  detail: JSON.stringify({
    plan: "# 计划\n\n## Context\n用户要求创建多个简单的文本文件。",
    planFilePath: "/Users/demo/.claude/plans/json-plan.md",
  }),
  output: "Exit plan mode?",
  status: "completed",
};

describe("GenericToolBlock", () => {
  afterEach(() => {
    cleanup();
  });

  it("shows plan-mode hint for askuserquestion in code mode", () => {
    render(
      <GenericToolBlock
        item={askUserItem}
        isExpanded={false}
        onToggle={vi.fn()}
        activeCollaborationModeId="code"
      />,
    );

    expect(screen.getByText("This feature requires Plan mode")).toBeTruthy();
  });

  it("hides plan-mode hint when collaboration mode is plan", () => {
    render(
      <GenericToolBlock
        item={askUserItem}
        isExpanded={false}
        onToggle={vi.fn()}
        activeCollaborationModeId="plan"
      />,
    );

    expect(screen.queryByText("This feature requires Plan mode")).toBeNull();
  });

  it("hides plan-mode hint for claude askuserquestion when pending user input exists", () => {
    render(
      <GenericToolBlock
        item={askUserItem}
        isExpanded={false}
        onToggle={vi.fn()}
        activeCollaborationModeId="code"
        activeEngine="claude"
        hasPendingUserInputRequest
      />,
    );

    expect(screen.queryByText("This feature requires Plan mode")).toBeNull();
  });

  it("hides plan-mode hint for claude askuserquestion in history without pending request", () => {
    render(
      <GenericToolBlock
        item={askUserItem}
        isExpanded={false}
        onToggle={vi.fn()}
        activeCollaborationModeId="code"
        activeEngine="claude"
      />,
    );

    expect(screen.queryByText("This feature requires Plan mode")).toBeNull();
  });

  it("shows blocked suggestion for modeBlocked askuserquestion item", () => {
    const view = render(
      <GenericToolBlock
        item={blockedModeItem}
        isExpanded={false}
        onToggle={vi.fn()}
        activeCollaborationModeId="code"
      />,
    );

    expect(screen.getByText("This feature requires Plan mode")).toBeTruthy();
    const header = view.container.querySelector(".task-header");
    expect(header).toBeTruthy();
    if (header) {
      fireEvent.click(header);
    }
    const rawPre = view.container.querySelector(".tool-output-raw-pre");
    expect(rawPre?.textContent ?? "").toContain("Switch to Plan mode");
  });

  it("shows file-change summary and per-file detail stats", () => {
    const view = render(
      <GenericToolBlock
        item={fileChangeItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getAllByText("2 files").length).toBeGreaterThan(0);
    expect(screen.getAllByText("+2").length).toBeGreaterThan(0);
    expect(screen.getAllByText("-1").length).toBeGreaterThan(0);
    expect(view.container.querySelector(".tool-change-metrics")).toBeNull();
    expect(view.container.querySelectorAll(".tool-change-kind-badge.added").length).toBe(1);
    expect(view.container.querySelectorAll(".tool-change-kind-badge.modified").length).toBe(1);
    expect(screen.getByText("-0")).toBeTruthy();
    expect(screen.getAllByText("+1").length).toBeGreaterThan(1);
  });

  it("uses singular file label for collapsed single file changes", () => {
    render(
      <GenericToolBlock
        item={fileChangeWithOutputItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("1 file")).toBeTruthy();
    expect(screen.queryByText("1 files")).toBeNull();
  });

  it("shows each changed file as its own collapsed row without overflow summary", () => {
    const parseDiffSpy = vi.spyOn(diffParser, "parseDiff");
    const view = render(
      <GenericToolBlock
        item={fileChangeManyItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("App.tsx")).toBeTruthy();
    expect(screen.getByText("New.tsx")).toBeTruthy();
    expect(screen.getByText("Home.tsx")).toBeTruthy();
    expect(screen.getByText("About.tsx")).toBeTruthy();
    expect(screen.getByText("api.ts")).toBeTruthy();
    expect(screen.getByText("store.ts")).toBeTruthy();
    expect(screen.getByText("app.css")).toBeTruthy();
    expect(screen.getByText("package.json")).toBeTruthy();
    expect(screen.queryByText(/\+\d+\s+more files/i)).toBeNull();
    expect(view.container.querySelectorAll(".tool-change-stack-entry").length).toBe(8);
    expect(parseDiffSpy).not.toHaveBeenCalled();
    parseDiffSpy.mockRestore();
  });

  it("toggles collapsed multi-file rows independently", () => {
    const view = render(
      <GenericToolBlock
        item={fileChangeManyItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    const headers = Array.from(
      view.container.querySelectorAll(".tool-change-stack-entry .tool-change-stack-header"),
    );
    expect(headers.length).toBe(8);
    expect(view.container.querySelectorAll(".tool-change-stack-entry .task-details").length).toBe(0);

    fireEvent.click(headers[0]!);
    expect(view.container.querySelectorAll(".tool-change-stack-entry .task-details").length).toBe(1);

    fireEvent.click(headers[1]!);
    expect(view.container.querySelectorAll(".tool-change-stack-entry .task-details").length).toBe(2);

    fireEvent.click(headers[0]!);
    expect(view.container.querySelectorAll(".tool-change-stack-entry .task-details").length).toBe(1);
    expect(view.container.querySelectorAll(".tool-change-stack-entry .tool-change-inline-diff").length).toBe(1);
  });

  it("renders each expanded file change inside its own visual card", () => {
    const view = render(
      <GenericToolBlock
        item={fileChangeItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );

    expect(view.container.querySelectorAll(".tool-change-entry").length).toBe(2);
    expect(view.container.querySelector(".tool-change-entry .tool-change-inline-diff")).toBeTruthy();
  });

  it("shows inline diff preview for file changes and hides raw diff output pane", () => {
    render(
      <GenericToolBlock
        item={fileChangeWithOutputItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("App.tsx")).toBeTruthy();
    expect(document.querySelector(".tool-output-raw-pre")).toBeNull();
    expect(screen.getByText("-1 +1")).toBeTruthy();
    expect(document.querySelector(".tool-change-inline-diff")).toBeTruthy();
  });

  it("falls back to output diff stats when change rows omit inline diff", () => {
    render(
      <GenericToolBlock
        item={fileChangeWithoutInlineDiffItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getAllByText("+1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("-1").length).toBeGreaterThan(0);
    expect(document.querySelector(".tool-output-raw-pre")).toBeNull();
    expect(screen.getByText("-1 +1")).toBeTruthy();
  });

  it("matches path hints with absolute/relative compatibility for stats fallback", () => {
    render(
      <GenericToolBlock
        item={fileChangePathHintCompatItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getAllByText("+1").length).toBeGreaterThan(0);
    expect(screen.getAllByText("-1").length).toBeGreaterThan(0);
  });

  it("opens diff path when clicking file-change row link without toggling card", () => {
    const onOpenDiffPath = vi.fn();
    const onToggle = vi.fn();
    render(
      <GenericToolBlock
        item={fileChangeItem}
        isExpanded
        onToggle={onToggle}
        onOpenDiffPath={onOpenDiffPath}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "App.tsx" }));
    expect(onOpenDiffPath).toHaveBeenCalledWith("src/App.tsx");
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("contains diff-route callback errors and keeps card interactive", () => {
    const onOpenDiffPath = vi.fn(() => {
      throw new Error("route failed");
    });
    const onToggle = vi.fn();
    render(
      <GenericToolBlock
        item={fileChangeItem}
        isExpanded
        onToggle={onToggle}
        onOpenDiffPath={onOpenDiffPath}
      />,
    );

    expect(() => fireEvent.click(screen.getByRole("button", { name: "App.tsx" }))).not.toThrow();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it("keeps markdown-like output in raw text mode", () => {
    render(
      <GenericToolBlock
        item={markdownOutputItem}
        isExpanded
        onToggle={vi.fn()}
      />,
    );
    const rawPre = document.querySelector(".tool-output-raw-pre");
    expect(rawPre).toBeTruthy();
    const rawText = rawPre?.textContent ?? "";
    expect(rawText).toContain("## Summary");
    expect(rawText).toContain("| Name | Value |");
  });

  it("renders exit plan mode as a dedicated plan-ready card", () => {
    const view = render(
      <GenericToolBlock
        item={exitPlanModeItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    expect(screen.getByText("Execution Plan Ready")).toBeTruthy();
    expect(screen.queryByText("Plan summary")).toBeNull();
    const header = view.container.querySelector(".tool-exit-plan-card-header");
    expect(header).toBeTruthy();
    if (header) {
      fireEvent.click(header);
    }
    expect(screen.getByText("Plan summary")).toBeTruthy();
    expect(screen.getByText("Execution handoff")).toBeTruthy();
    expect(screen.getByText("Plan file")).toBeTruthy();
    expect(screen.getByText("/Users/demo/.claude/plans/sample-plan.md")).toBeTruthy();
    expect(view.container.querySelector(".tool-exit-plan-card")).toBeTruthy();
  });

  it("falls back to raw output when exit plan payload has no labeled sections", () => {
    const view = render(
      <GenericToolBlock
        item={exitPlanModeRawFallbackItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    const header = view.container.querySelector(".tool-exit-plan-card-header");
    expect(header).toBeTruthy();
    if (header) {
      fireEvent.click(header);
    }
    expect(screen.getByText("Raw output")).toBeTruthy();
    expect(view.container.querySelector(".tool-exit-plan-card-markdown")?.textContent ?? "").toContain(
      "plain exit plan payload without labeled sections",
    );
  });

  it("parses json exit plan payload and renders plan markdown", () => {
    render(
      <GenericToolBlock
        item={exitPlanModeJsonItem}
        isExpanded={false}
        onToggle={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Execution Plan ReadyExit Plan mode" }));
    expect(screen.getByText("计划")).toBeTruthy();
    expect(screen.getByText("用户要求创建多个简单的文本文件。")).toBeTruthy();
    expect(screen.getByText("/Users/demo/.claude/plans/json-plan.md")).toBeTruthy();
    expect(screen.queryByText("Exit plan mode?")).toBeNull();
  });
});
