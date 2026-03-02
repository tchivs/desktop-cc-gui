/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { EngineType } from "../../../types";
import { Composer } from "./Composer";

afterEach(() => {
  cleanup();
});

vi.mock("../../../services/dragDrop", () => ({
  subscribeWindowDragDrop: vi.fn(() => () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `tauri://${path}`,
  invoke: vi.fn(async () => null),
}));

vi.mock("../../engine/components/EngineSelector", () => ({
  EngineSelector: () => null,
}));

vi.mock("../../opencode/components/OpenCodeControlPanel", () => ({
  OpenCodeControlPanel: () => null,
}));

vi.mock("../../status-panel/components/StatusPanel", () => ({
  StatusPanel: () => <div data-testid="status-panel" />,
}));

vi.mock("./ChatInputBox/ChatInputBoxAdapter", () => ({
  ChatInputBoxAdapter: ({ showStatusPanelToggle }: { showStatusPanelToggle?: boolean }) => (
    <div
      data-testid="chat-input-box-adapter"
      data-show-status-panel-toggle={String(showStatusPanelToggle)}
    />
  ),
}));

function ComposerHarness({ selectedEngine }: { selectedEngine: EngineType }) {
  return (
    <Composer
      onSend={() => {}}
      onQueue={() => {}}
      onStop={() => {}}
      canStop={false}
      isProcessing={false}
      steerEnabled={false}
      collaborationModes={[]}
      collaborationModesEnabled={true}
      selectedCollaborationModeId={null}
      onSelectCollaborationMode={() => {}}
      selectedEngine={selectedEngine}
      models={[]}
      selectedModelId={null}
      onSelectModel={() => {}}
      reasoningOptions={[]}
      selectedEffort={null}
      onSelectEffort={() => {}}
      reasoningSupported={false}
      accessMode="current"
      onSelectAccessMode={() => {}}
      skills={[]}
      prompts={[]}
      commands={[]}
      files={[]}
      draftText=""
      onDraftChange={() => {}}
      dictationEnabled={false}
      activeWorkspaceId="ws-1"
      activeThreadId="thread-1"
    />
  );
}

describe("Composer status panel toggle visibility", () => {
  it("shows status panel toggle on claude engine", () => {
    render(<ComposerHarness selectedEngine="claude" />);
    expect(screen.getByTestId("status-panel")).not.toBeNull();
    expect(
      screen
        .getByTestId("chat-input-box-adapter")
        .getAttribute("data-show-status-panel-toggle"),
    ).toBe("true");
  });

  it("shows status panel toggle on codex engine", () => {
    render(<ComposerHarness selectedEngine="codex" />);
    expect(screen.getByTestId("status-panel")).not.toBeNull();
    expect(
      screen
        .getByTestId("chat-input-box-adapter")
        .getAttribute("data-show-status-panel-toggle"),
    ).toBe("true");
  });
});
