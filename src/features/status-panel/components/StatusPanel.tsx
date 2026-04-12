import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import ListChecks from "lucide-react/dist/esm/icons/list-checks";
import Bot from "lucide-react/dist/esm/icons/bot";
import FileEdit from "lucide-react/dist/esm/icons/file-edit";
import ListTodo from "lucide-react/dist/esm/icons/list-todo";
import type { ConversationItem } from "../../../types";
import type { TurnPlan } from "../../../types";
import type { TabType } from "../types";
import { useStatusPanelData } from "../hooks/useStatusPanelData";
import { resolvePlanStepStatusForDisplay } from "../../threads/utils/threadNormalize";
import { TodoList } from "./TodoList";
import { SubagentList } from "./SubagentList";
import { FileChangesList } from "./FileChangesList";
import { PlanList } from "./PlanList";

interface StatusPanelProps {
  items: ConversationItem[];
  isProcessing: boolean;
  expanded?: boolean;
  plan?: TurnPlan | null;
  isPlanMode?: boolean;
  isCodexEngine?: boolean;
  onOpenDiffPath?: (path: string) => void;
  variant?: "popover" | "dock";
}

function resolvePreferredTab(
  variant: "popover" | "dock",
  showPlanTab: boolean,
): TabType | null {
  if (variant === "dock") {
    return showPlanTab ? "plan" : "todo";
  }
  return null;
}

export const StatusPanel = memo(function StatusPanel({
  items,
  isProcessing,
  expanded = true,
  plan = null,
  isPlanMode = false,
  isCodexEngine = false,
  onOpenDiffPath,
  variant = "popover",
}: StatusPanelProps) {
  const { t } = useTranslation();
  const {
    todos,
    subagents,
    fileChanges,
    todoCompleted,
    todoTotal,
    hasInProgressTodo,
    subagentCompleted,
    subagentTotal,
    hasRunningSubagent,
    totalAdditions,
    totalDeletions,
  } = useStatusPanelData(items, { isCodexEngine });

  const showPlanTab = isPlanMode || Boolean(plan);
  const [openTab, setOpenTab] = useState<TabType | null>(() =>
    resolvePreferredTab(variant, showPlanTab),
  );
  const panelRef = useRef<HTMLDivElement>(null);
  const planTotal = plan?.steps.length ?? 0;
  const planCompleted =
    plan?.steps.filter((step) => step.status === "completed").length ?? 0;
  const showInlinePlanTab = variant === "dock" ? showPlanTab : showPlanTab && !isCodexEngine;
  const showPlanTabButton = variant === "dock"
    ? showInlinePlanTab
    : !isCodexEngine && showInlinePlanTab;
  const codexTaskItems = useMemo(() => {
    if (isCodexEngine && plan && plan.steps.length > 0) {
      return plan.steps.map((step) => {
        const statusForDisplay = resolvePlanStepStatusForDisplay(step.status, isProcessing);
        return {
          content: step.step,
          status:
            statusForDisplay === "completed"
              ? ("completed" as const)
              : statusForDisplay === "inProgress"
                ? ("in_progress" as const)
                : ("pending" as const),
        };
      });
    }
    return todos;
  }, [isCodexEngine, isProcessing, plan, todos]);
  const codexTaskCompleted = useMemo(
    () => codexTaskItems.filter((item) => item.status === "completed").length,
    [codexTaskItems],
  );
  const codexTaskTotal = codexTaskItems.length;
  const codexTaskInProgress = codexTaskItems.some((item) => item.status === "in_progress");

  // 点击外部关闭 popover
  useEffect(() => {
    if (variant !== "popover" || !openTab) return;
    function handleClickOutside(event: MouseEvent) {
      if (
        panelRef.current &&
        !panelRef.current.contains(event.target as Node)
      ) {
        setOpenTab(null);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [openTab, variant]);

  useEffect(() => {
    if (variant !== "popover" || !openTab) return;
    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpenTab(null);
      }
    }
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [openTab, variant]);

  useEffect(() => {
    if (variant !== "dock") {
      return;
    }
    if (openTab === "plan" && !showInlinePlanTab) {
      setOpenTab("todo");
      return;
    }
    if (!openTab) {
      setOpenTab(resolvePreferredTab(variant, showInlinePlanTab));
    }
  }, [openTab, showInlinePlanTab, variant]);

  const handleTabClick = useCallback(
    (tab: TabType) => {
      setOpenTab((prev) => {
        if (variant === "dock") {
          return tab;
        }
        return prev === tab ? null : tab;
      });
    },
    [variant],
  );

  if (!expanded) return null;

  const activeTab = variant === "dock"
    ? openTab ?? resolvePreferredTab(variant, showInlinePlanTab)
    : openTab;
  const contentNode = (
    <>
      {activeTab === "todo" && <TodoList todos={isCodexEngine ? codexTaskItems : todos} />}
      {activeTab === "subagent" && <SubagentList subagents={subagents} />}
      {activeTab === "files" && (
        <FileChangesList
          fileChanges={fileChanges}
          totalAdditions={totalAdditions}
          totalDeletions={totalDeletions}
          onOpenDiffPath={onOpenDiffPath}
          onAfterSelect={() => {
            if (variant !== "dock") {
              setOpenTab(null);
            }
          }}
        />
      )}
      {activeTab === "plan" && (
        <PlanList
          plan={plan}
          isPlanMode={isPlanMode}
          isProcessing={isProcessing}
          isCodexEngine={isCodexEngine}
        />
      )}
    </>
  );

  return (
    <div className={`sp-root${variant === "dock" ? " sp-root--dock" : ""}`} ref={panelRef}>
      {variant === "dock" ? (
        <>
          <div className="sp-tabs sp-tabs--dock">
            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "todo" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("todo")}
              >
                <ListChecks size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabTodos")}</span>
                <span className="sp-tab-count">
                  {codexTaskCompleted}/{codexTaskTotal}
                </span>
                {isProcessing && codexTaskInProgress && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "subagent" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("subagent")}
              >
                <Bot size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabAgents")}</span>
                <span className="sp-tab-count">
                  {subagentCompleted}/{subagentTotal}
                </span>
                {isProcessing && hasRunningSubagent && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "files" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("files")}
                aria-expanded={activeTab === "files"}
              >
                <FileEdit size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabEdits")}</span>
                <span className="sp-tab-file-stats">
                  <span className="sp-stat-add">+{totalAdditions}</span>
                  <span className="sp-stat-mod">-{totalDeletions}</span>
                </span>
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "todo" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("todo")}
              >
                <ListChecks size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabTodos")}</span>
                <span className="sp-tab-count">
                  {todoCompleted}/{todoTotal}
                </span>
                {isProcessing && hasInProgressTodo && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "subagent" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("subagent")}
              >
                <Bot size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabSubagents")}</span>
                <span className="sp-tab-count">
                  {subagentCompleted}/{subagentTotal}
                </span>
                {isProcessing && hasRunningSubagent && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "files" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("files")}
                aria-expanded={activeTab === "files"}
              >
                <FileEdit size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabEdits")}</span>
                <span className="sp-tab-file-stats">
                  <span className="sp-stat-add">+{totalAdditions}</span>
                  <span className="sp-stat-mod">-{totalDeletions}</span>
                </span>
              </button>
            )}

            {showPlanTabButton && (
              <button
                type="button"
                className={`sp-tab${activeTab === "plan" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("plan")}
                aria-expanded={activeTab === "plan"}
              >
                <ListTodo size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabPlan")}</span>
                <span className="sp-tab-count">
                  {planCompleted}/{planTotal}
                </span>
                {isProcessing && isPlanMode && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}
          </div>
          <div className="sp-dock-shell">
            <div className="sp-popover-content sp-dock-content">
              {contentNode}
            </div>
          </div>
        </>
      ) : (
        <>
          {openTab && (
            <div className="sp-popover">
              <div className="sp-popover-content">
                {contentNode}
              </div>
            </div>
          )}
          <div className="sp-tabs">
            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "todo" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("todo")}
              >
                <ListChecks size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabTodos")}</span>
                <span className="sp-tab-count">
                  {codexTaskCompleted}/{codexTaskTotal}
                </span>
                {isProcessing && codexTaskInProgress && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "subagent" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("subagent")}
              >
                <Bot size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabAgents")}</span>
                <span className="sp-tab-count">
                  {subagentCompleted}/{subagentTotal}
                </span>
                {isProcessing && hasRunningSubagent && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "files" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("files")}
                aria-expanded={activeTab === "files"}
              >
                <FileEdit size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabEdits")}</span>
                <span className="sp-tab-file-stats">
                  <span className="sp-stat-add">+{totalAdditions}</span>
                  <span className="sp-stat-mod">-{totalDeletions}</span>
                </span>
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "todo" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("todo")}
              >
                <ListChecks size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabTodos")}</span>
                <span className="sp-tab-count">
                  {todoCompleted}/{todoTotal}
                </span>
                {isProcessing && hasInProgressTodo && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "subagent" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("subagent")}
              >
                <Bot size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabSubagents")}</span>
                <span className="sp-tab-count">
                  {subagentCompleted}/{subagentTotal}
                </span>
                {isProcessing && hasRunningSubagent && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}

            {!isCodexEngine && (
              <button
                type="button"
                className={`sp-tab${activeTab === "files" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("files")}
                aria-expanded={activeTab === "files"}
              >
                <FileEdit size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabEdits")}</span>
                <span className="sp-tab-file-stats">
                  <span className="sp-stat-add">+{totalAdditions}</span>
                  <span className="sp-stat-mod">-{totalDeletions}</span>
                </span>
              </button>
            )}

            {showPlanTabButton && (
              <button
                type="button"
                className={`sp-tab${activeTab === "plan" ? " sp-tab-active" : ""}`}
                onClick={() => handleTabClick("plan")}
                aria-expanded={activeTab === "plan"}
              >
                <ListTodo size={14} className="sp-tab-icon" />
                <span className="sp-tab-label">{t("statusPanel.tabPlan")}</span>
                <span className="sp-tab-count">
                  {planCompleted}/{planTotal}
                </span>
                {isProcessing && isPlanMode && (
                  <span className="sp-tab-loading" />
                )}
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
});
