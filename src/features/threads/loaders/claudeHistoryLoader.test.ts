import { describe, expect, it } from "vitest";
import { parseClaudeHistoryMessages } from "./claudeHistoryLoader";

describe("parseClaudeHistoryMessages", () => {
  it("preserves transcript-style bash output and command metadata", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "tool",
        id: "tool-1",
        tool_name: "bash",
        tool_input: {
          command: "git log --oneline -10",
          description: "查看最近的 git 提交历史",
        },
      },
      {
        kind: "tool",
        id: "tool-1-result",
        toolType: "result",
        text: "",
        tool_output: {
          output: "abc123 first commit\ndef456 second commit\n",
          exit: 0,
        },
      },
    ]);

    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      id: "tool-1",
      kind: "tool",
      toolType: "bash",
      title: "bash",
      status: "completed",
      output: "abc123 first commit\ndef456 second commit\n",
    });
    if (items[0]?.kind === "tool") {
      expect(items[0].detail).toContain("git log --oneline -10");
      expect(items[0].detail).toContain("查看最近的 git 提交历史");
    }
  });

  it("preserves non-command tool input payload for read tools", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "tool",
        id: "tool-read-1",
        tool_name: "read_file",
        tool_input: {
          file_path: "/workspace/README.md",
        },
      },
    ]);

    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      id: "tool-read-1",
      kind: "tool",
      toolType: "read_file",
      title: "read_file",
    });
    if (items[0]?.kind === "tool") {
      expect(items[0].detail).toContain("file_path");
      expect(items[0].detail).toContain("/workspace/README.md");
    }
  });

  it("preserves full command tool_input payload so session activity can read cwd/argv", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "tool",
        id: "tool-bash-1",
        tool_name: "bash",
        tool_input: {
          argv: ["zsh", "-lc", "pnpm vitest"],
          cwd: "/workspace/project",
          description: "run tests",
        },
      },
    ]);

    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      id: "tool-bash-1",
      kind: "tool",
      toolType: "bash",
      title: "bash",
      status: "started",
    });
    if (items[0]?.kind === "tool") {
      expect(items[0].detail).toContain("argv");
      expect(items[0].detail).toContain("/workspace/project");
      expect(items[0].detail).toContain("run tests");
    }
  });

  it("maps AskUserQuestion user answer message into submitted history block", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "tool",
        id: "tool-ask-1",
        tool_name: "AskUserQuestion",
        input: {
          questions: [
            {
              id: "q-0",
              header: "技术偏好",
              question: "你关注哪些方面？",
              options: [
                { label: "代码质量", description: "可维护性" },
                { label: "性能优化", description: "响应速度" },
              ],
            },
          ],
        },
      },
      {
        kind: "message",
        role: "user",
        id: "msg-user-1",
        text: "The user answered the AskUserQuestion: 代码质量, 性能优化. Please continue based on this selection.",
      },
    ]);

    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({
      id: "tool-ask-1",
      kind: "tool",
      toolType: "AskUserQuestion",
      status: "completed",
      output: "代码质量, 性能优化",
    });
    expect(items[1]).toMatchObject({
      id: "request-user-input-submitted-tool-ask-1",
      kind: "tool",
      toolType: "requestUserInputSubmitted",
      status: "completed",
    });
    if (items[1]?.kind === "tool") {
      const parsed = JSON.parse(items[1].detail);
      expect(parsed.schema).toBe("requestUserInputSubmitted/v1");
      expect(parsed.questions[0].question).toBe("你关注哪些方面？");
      expect(parsed.questions[0].selectedOptions).toEqual([
        "代码质量",
        "性能优化",
      ]);
    }
  });

  it("keeps user image attachments for message rows even when text is empty", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "message",
        role: "user",
        id: "msg-user-image-1",
        text: "",
        images: ["data:image/png;base64,AAAA"],
      },
    ]);

    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      id: "msg-user-image-1",
      kind: "message",
      role: "user",
      images: ["data:image/png;base64,AAAA"],
    });
  });

  it("parses legacy single-question AskUserQuestion payloads and answer text variants", () => {
    const items = parseClaudeHistoryMessages([
      {
        kind: "tool",
        id: "tool-ask-legacy-1",
        tool_name: "AskUserQuestion",
        tool_input: {
          header: "项目类型",
          question: "请选择一个项目类型",
          options: [
            { label: "Web应用", description: "浏览器端项目" },
            { label: "CLI工具", description: "命令行项目" },
          ],
        },
      },
      {
        kind: "message",
        role: "user",
        id: "msg-user-legacy-1",
        text: "The user answered the AskUserQuestion: Web应用 Please continue based on this selection.",
      },
    ]);

    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({
      id: "tool-ask-legacy-1",
      kind: "tool",
      status: "completed",
      output: "Web应用",
    });
    expect(items[1]).toMatchObject({
      id: "request-user-input-submitted-tool-ask-legacy-1",
      kind: "tool",
      toolType: "requestUserInputSubmitted",
      status: "completed",
    });
    if (items[1]?.kind === "tool") {
      const parsed = JSON.parse(items[1].detail);
      expect(parsed.questions[0].question).toBe("请选择一个项目类型");
      expect(parsed.questions[0].selectedOptions).toEqual(["Web应用"]);
    }
  });
});
