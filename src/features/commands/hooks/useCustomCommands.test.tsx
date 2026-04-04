/** @vitest-environment jsdom */
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getClaudeCommandsList, getOpenCodeCommandsList } from "../../../services/tauri";
import { useCustomCommands } from "./useCustomCommands";

vi.mock("../../../services/tauri", () => ({
  getClaudeCommandsList: vi.fn(),
  getOpenCodeCommandsList: vi.fn(),
}));

describe("useCustomCommands", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("passes workspace id to claude commands and normalizes source", async () => {
    vi.mocked(getClaudeCommandsList).mockResolvedValue([
      {
        name: "/open-spec:apply",
        path: "/repo/.claude/commands/open-spec/apply.md",
        description: "apply change",
        source: "project_claude",
        content: "body",
      },
    ]);

    const { result } = renderHook(() =>
      useCustomCommands({
        activeEngine: "claude",
        workspaceId: "workspace-1",
      }),
    );

    await waitFor(() => {
      expect(result.current.commands).toHaveLength(1);
    });

    expect(getClaudeCommandsList).toHaveBeenCalledWith("workspace-1");
    expect(result.current.commands[0]).toMatchObject({
      name: "open-spec:apply",
      source: "project_claude",
    });
  });

  it("uses opencode command list when active engine is opencode", async () => {
    vi.mocked(getOpenCodeCommandsList).mockResolvedValue([
      {
        name: "status",
        path: "",
        description: "Show status",
        content: "",
      },
    ]);

    const { result } = renderHook(() =>
      useCustomCommands({
        activeEngine: "opencode",
        workspaceId: "workspace-1",
      }),
    );

    await waitFor(() => {
      expect(result.current.commands).toHaveLength(1);
    });

    expect(getClaudeCommandsList).not.toHaveBeenCalled();
    expect(getOpenCodeCommandsList).toHaveBeenCalled();
  });

  it("retries workspace claude command list once when first response is empty", async () => {
    vi.mocked(getClaudeCommandsList)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        {
          name: "/workflow:analyze-with-file",
          path: "/repo/.claude/commands/workflow/analyze-with-file.md",
          description: "analyze with file",
          source: "project_claude",
          content: "body",
        },
      ]);

    const { result } = renderHook(() =>
      useCustomCommands({
        activeEngine: "claude",
        workspaceId: "workspace-1",
      }),
    );

    await waitFor(() => {
      expect(result.current.commands.some((entry) => entry.name === "workflow:analyze-with-file")).toBe(true);
    });

    expect(getClaudeCommandsList).toHaveBeenNthCalledWith(1, "workspace-1");
    expect(getClaudeCommandsList).toHaveBeenNthCalledWith(2, "workspace-1");
  });

  it("falls back to global claude command list when workspace responses stay empty", async () => {
    vi.mocked(getClaudeCommandsList)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([
        {
          name: "/workflow:analyze-with-file",
          path: "/Users/demo/.claude/commands/workflow/analyze-with-file.md",
          description: "analyze with file",
          source: "global_claude",
          content: "body",
        },
      ]);

    const { result } = renderHook(() =>
      useCustomCommands({
        activeEngine: "claude",
        workspaceId: "workspace-1",
      }),
    );

    await waitFor(() => {
      expect(result.current.commands).toHaveLength(1);
    });

    expect(getClaudeCommandsList).toHaveBeenNthCalledWith(1, "workspace-1");
    expect(getClaudeCommandsList).toHaveBeenNthCalledWith(2, "workspace-1");
    expect(getClaudeCommandsList).toHaveBeenNthCalledWith(3, null);
    expect(result.current.commands[0]).toMatchObject({
      name: "workflow:analyze-with-file",
      source: "global_claude",
    });
  });

  it("applies cooldown to avoid repeating empty claude retry burst", async () => {
    const nowSpy = vi.spyOn(Date, "now");
    nowSpy.mockReturnValue(1_000_000);

    vi.mocked(getClaudeCommandsList).mockResolvedValue([]);

    const { result } = renderHook(() =>
      useCustomCommands({
        activeEngine: "claude",
        workspaceId: "workspace-1",
      }),
    );

    await waitFor(() => {
      expect(getClaudeCommandsList).toHaveBeenCalledTimes(3);
    });

    await act(async () => {
      await result.current.refreshCommands();
    });

    await waitFor(() => {
      expect(getClaudeCommandsList).toHaveBeenCalledTimes(4);
    });

    nowSpy.mockReturnValue(1_016_000);
    await act(async () => {
      await result.current.refreshCommands();
    });

    await waitFor(() => {
      expect(getClaudeCommandsList).toHaveBeenCalledTimes(7);
    });

    nowSpy.mockRestore();
  });
});
