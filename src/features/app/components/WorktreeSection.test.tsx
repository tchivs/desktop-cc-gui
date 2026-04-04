// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceInfo } from "../../../types";
import { WorktreeSection } from "./WorktreeSection";

const worktree: WorkspaceInfo = {
  id: "wt-1",
  name: "Worktree One",
  path: "/tmp/worktree",
  connected: true,
  kind: "worktree",
  worktree: { branch: "feature/test" },
  settings: { sidebarCollapsed: false },
};

afterEach(() => {
  cleanup();
});

describe("WorktreeSection", () => {
  it("does not render older thread controls for worktrees", () => {
    render(
      <WorktreeSection
        parentWorkspaceId="workspace-1"
        worktrees={[worktree]}
        isSectionCollapsed={false}
        onToggleSectionCollapse={vi.fn()}
        deletingWorktreeIds={new Set()}
        threadsByWorkspace={{ [worktree.id]: [] }}
        threadStatusById={{}}
        runningSessionCountByWorkspaceId={{}}
        recentSessionCountByWorkspaceId={{}}
        threadListLoadingByWorkspace={{ [worktree.id]: false }}
        threadListPagingByWorkspace={{ [worktree.id]: false }}
        threadListCursorByWorkspace={{ [worktree.id]: "cursor" }}
        expandedWorkspaces={new Set()}
        activeWorkspaceId={null}
        activeThreadId={null}
        getThreadRows={() => ({
          pinnedRows: [],
          unpinnedRows: [],
          totalRoots: 0,
          hasMoreRoots: false,
        })}
        getThreadTime={() => null}
        isThreadPinned={() => false}
        isThreadAutoNaming={() => false}
        onToggleThreadPin={vi.fn()}
        getPinTimestamp={() => null}
        onSelectWorkspace={vi.fn()}
        onConnectWorkspace={vi.fn()}
        onToggleWorkspaceCollapse={vi.fn()}
        onSelectThread={vi.fn()}
        onShowThreadMenu={vi.fn()}
        onShowWorktreeMenu={vi.fn()}
        onToggleExpanded={vi.fn()}
        onLoadOlderThreads={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "Search older..." }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Load older..." }),
    ).toBeNull();
  });

  it("toggles the worktree section on double click only", () => {
    const onToggleSectionCollapse = vi.fn();

    const { container } = render(
      <WorktreeSection
        parentWorkspaceId="workspace-1"
        worktrees={[worktree]}
        isSectionCollapsed={false}
        onToggleSectionCollapse={onToggleSectionCollapse}
        deletingWorktreeIds={new Set()}
        threadsByWorkspace={{ [worktree.id]: [] }}
        threadStatusById={{}}
        runningSessionCountByWorkspaceId={{}}
        recentSessionCountByWorkspaceId={{}}
        threadListLoadingByWorkspace={{ [worktree.id]: false }}
        threadListPagingByWorkspace={{ [worktree.id]: false }}
        threadListCursorByWorkspace={{ [worktree.id]: null }}
        expandedWorkspaces={new Set()}
        activeWorkspaceId={null}
        activeThreadId={null}
        getThreadRows={() => ({
          pinnedRows: [],
          unpinnedRows: [],
          totalRoots: 0,
          hasMoreRoots: false,
        })}
        getThreadTime={() => null}
        isThreadPinned={() => false}
        isThreadAutoNaming={() => false}
        onToggleThreadPin={vi.fn()}
        getPinTimestamp={() => null}
        onSelectWorkspace={vi.fn()}
        onConnectWorkspace={vi.fn()}
        onToggleWorkspaceCollapse={vi.fn()}
        onSelectThread={vi.fn()}
        onShowThreadMenu={vi.fn()}
        onShowWorktreeMenu={vi.fn()}
        onToggleExpanded={vi.fn()}
        onLoadOlderThreads={vi.fn()}
      />,
    );

    const header = container.querySelector(".worktree-header") as HTMLButtonElement | null;
    expect(header).toBeTruthy();
    if (!header) {
      throw new Error("Expected worktree header");
    }
    fireEvent.click(header);
    expect(onToggleSectionCollapse).not.toHaveBeenCalled();

    fireEvent.doubleClick(header);
    expect(onToggleSectionCollapse).toHaveBeenCalledWith("workspace-1");
  });

  it("selects worktree on single click and toggles agents on double click", () => {
    const onSelectWorkspace = vi.fn();
    const onToggleWorkspaceCollapse = vi.fn();

    const { container } = render(
      <WorktreeSection
        parentWorkspaceId="workspace-1"
        worktrees={[worktree]}
        isSectionCollapsed={false}
        onToggleSectionCollapse={vi.fn()}
        deletingWorktreeIds={new Set()}
        threadsByWorkspace={{ [worktree.id]: [] }}
        threadStatusById={{}}
        runningSessionCountByWorkspaceId={{}}
        recentSessionCountByWorkspaceId={{}}
        threadListLoadingByWorkspace={{ [worktree.id]: false }}
        threadListPagingByWorkspace={{ [worktree.id]: false }}
        threadListCursorByWorkspace={{ [worktree.id]: null }}
        expandedWorkspaces={new Set()}
        activeWorkspaceId={null}
        activeThreadId={null}
        getThreadRows={() => ({
          pinnedRows: [],
          unpinnedRows: [],
          totalRoots: 0,
          hasMoreRoots: false,
        })}
        getThreadTime={() => null}
        isThreadPinned={() => false}
        isThreadAutoNaming={() => false}
        onToggleThreadPin={vi.fn()}
        getPinTimestamp={() => null}
        onSelectWorkspace={onSelectWorkspace}
        onConnectWorkspace={vi.fn()}
        onToggleWorkspaceCollapse={onToggleWorkspaceCollapse}
        onSelectThread={vi.fn()}
        onShowThreadMenu={vi.fn()}
        onShowWorktreeMenu={vi.fn()}
        onToggleExpanded={vi.fn()}
        onLoadOlderThreads={vi.fn()}
      />,
    );

    const worktreeRow = container.querySelector(".worktree-row") as HTMLElement | null;
    expect(worktreeRow).toBeTruthy();
    if (!worktreeRow) {
      throw new Error("Expected worktree row");
    }

    fireEvent.click(worktreeRow);
    expect(onSelectWorkspace).toHaveBeenCalledWith("wt-1");
    expect(onToggleWorkspaceCollapse).not.toHaveBeenCalled();

    fireEvent.doubleClick(worktreeRow);
    expect(onToggleWorkspaceCollapse).toHaveBeenCalledWith("wt-1", true);
  });
});
