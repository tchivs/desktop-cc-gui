## Context

当前 `Claude Code` 模式在代码层已经具备三段基础能力，但产品层 wiring 仍然不一致：

- UI 层已经存在四种 `PermissionMode`：`default / plan / acceptEdits / bypassPermissions`。
- 前端到后端已经存在 mode mapping：`PermissionMode -> AccessMode`，并在消息发送时透传到 Tauri。
- Claude runtime 已经支持将 `AccessMode` 映射到 Claude CLI 参数：
  - `default` -> `--permission-mode default`
  - `read-only` -> `--permission-mode plan`
  - `current` -> `--permission-mode acceptEdits`
  - `full-access` -> `--dangerously-skip-permissions`

但当前产品层仍有两处强短路：

- `ModeSelect` 对非 `Gemini` provider 默认将除 `bypassPermissions` 外的模式全部禁用。
- `app-shell` 初始化阶段直接将 `accessMode` 强制设置为 `full-access`。

这导致 `Claude Code` 实际只剩“全自动”一条运行路径，UI 已有语义和 runtime 实际行为严重脱节。与此同时，`Claude` 的审批请求事件尚未完整接入现有 approval toast / `respond_to_server_request` 流程，因此如果直接把 `default` 和 `acceptEdits` 一次性打开，用户会进入“需要审批但 UI 不弹框”的失败路径。

本变更涉及 frontend mode UX、thread 启动参数、Rust Claude runtime、approval event bridge、测试与 i18n 文案，属于典型跨层改动，必须先冻结分阶段设计再实施。

## Goals / Non-Goals

**Goals:**

- 让 `Claude Code` 的模式选择恢复为真实运行时输入，不再被产品层强制覆盖。
- 采用渐进式开放而不是一次性放开四种模式。
- Phase 1 先交付可安全验证的 `plan + full-access`。
- 为 Phase 2/3 明确审批桥补齐条件，避免 `default/acceptEdits` 提前裸奔。
- 保持 `Codex / Gemini / OpenCode` 现有模式行为完全不变。

**Non-Goals:**

- 不重做整个 ChatInput 模式选择器视觉设计。
- 不修改 Claude provider / token / 本地配置读写逻辑。
- 不引入第二套审批 UI。
- 不在本轮承诺处理所有 Claude CLI event 差异，只处理和模式开放直接相关的运行时契约。

## Decisions

### Decision 1: 采用三阶段 rollout，而不是一次性开放四种模式

**Decision**

- Phase 1：开放 `plan` 与 `full-access`
- Phase 2：补齐 Claude approval bridge 后开放 `default`
- Phase 3：在 `default` 稳定后开放 `acceptEdits`

**Rationale**

- `plan` 与 `full-access` 不依赖审批事件桥，当前 runtime 已有直接 CLI flag 映射。
- `default` 与 `acceptEdits` 都依赖 Claude 在需要批准时能正确进入现有 approval 流程；当前这条链路未完整验证。
- 先开放低歧义档位，可以尽快让用户摆脱“只有全自动”这一产品硬伤，同时避免把半成品权限模式暴露给外部用户。

**Alternatives considered**

- 一次性开放四种模式：实现最少，但会把 Claude 审批桥缺口直接暴露给用户。
- 继续只保留 `full-access`：风险最低，但产品语义长期失真，且无法支持谨慎模式用户。

### Decision 2: 保留现有 mode enum 和 mapping，不重新命名权限模型

**Decision**

- 继续沿用当前 `PermissionMode` 与 `AccessMode` 对照关系。
- Phase 1 只调整可选态与初始化逻辑，不引入新的 mode ids。

**Rationale**

- 现有 UI、i18n、tests、send message payload 已围绕这套命名建立。
- 重新命名会把一个“能力开放”问题升级成“全链路重构”问题，收益明显不成比例。

**Alternatives considered**

- 新建 Claude 专属 mode enum：更干净，但会造成 provider-specific 条件分叉和额外迁移成本。

### Decision 3: Mode availability 由 provider-specific gating 控制，而不是 runtime silent override

**Decision**

- `ModeSelect` 必须基于 provider + rollout phase 决定哪些模式可选。
- 运行时不得再通过初始化 effect 或发送前静默覆盖用户选择。

**Rationale**

- 用户选择一旦进入 composer，就应该是显式 contract，而不是“看起来可选，实际被偷偷改掉”。
- rollout gating 应在 UI 层和产品规则层显式表达，方便测试与文案说明。

**Alternatives considered**

- 保留 UI 可选但发送前强制兜底：会继续制造“显示值”和“实际值”不一致，难以排查。

### Decision 4: Claude approval bridge 复用现有 approval/request 流程，不新增第二条确认链

**Decision**

- `Claude` 需要批准的操作必须映射成现有 `approval/request` 或 `item/*/requestApproval` 语义。
- 前端仍沿用 `useAppServerEvents -> useThreadApprovalEvents -> ApprovalToasts -> respondToServerRequest` 这一主链路。

**Rationale**

- 当前产品已经有一套 approval state、toast、remember rule、accept/decline 处理链。
- 再做一套 Claude 专属确认 UI 会把问题复杂化，并破坏跨引擎交互一致性。

**Alternatives considered**

- Claude 专属 AskUserQuestion 弹窗代替 approval：能短期跑通，但会让“审批”与“普通提问”混在一起，语义不稳。

### Decision 4A: 在 approval bridge 缺失期间，为 Claude denial 提供 modeBlocked 兜底诊断

**Decision**

- 在未确认 Claude CLI 有稳定结构化 approval signal 之前，不提前开放 `default`。
- 若 Claude 在 `code/default` 语义下对 `AskUserQuestion` 直接返回 `permission denied`，runtime 要把它降级映射成现有 `collaboration/modeBlocked` 提示。

**Rationale**

- 当前本地样本已证明 Claude 可能出现 `AskUserQuestion tool permission denied`，而不是 `approval/request`。
- 用户可接受“暂时不开 default”，但不可接受“看起来支持、实际静默失败”。
- 先把 denial 解释清楚，可以降低排障成本，并为后续继续调查真实 approval shape 留出空间。

**Alternatives considered**

- 忽略 denial，仅保留 `TurnError`：用户无法理解为什么没有弹审批提示。
- 伪造 `approval/request` 并开放 `default`：没有可回传的真实 `request_id`，`accept/decline` 语义不成立。

### Decision 5: `acceptEdits` 必须以后验验证为准，而不是按命名直觉开放

**Decision**

- `acceptEdits` 作为 Phase 3 模式，必须先验证 Claude CLI 在当前版本下的真实语义。
- 如果 CLI 的 `acceptEdits` 不满足“文件编辑自动通过，命令仍审批”的预期，则以 CLI 真实语义为准修正文案和产品规则。

**Rationale**

- 现在 UI 文案是按期望语义写的，不代表 Claude CLI 在不同版本下完全等价。
- 该模式最容易出现“自动得太多”或“自动得太少”的误解，必须把文案和真实行为对齐。

**Alternatives considered**

- 直接沿用现有文案开放：风险是用户预期和 CLI 真实行为不一致。

## Risks / Trade-offs

- [Risk] Phase 1 打开 `plan` 后，某些 Claude 会话路径仍可能被遗留逻辑改回 `full-access`
  → Mitigation：补前端 mode mapping 测试、发送 payload 测试、Claude CLI flag 测试，形成三段断言。

- [Risk] Claude approval event 格式与 Codex 不同，直接桥接可能遗漏边界事件
  → Mitigation：先以 `default` 单模式验证 approval path，再推进 `acceptEdits`。

- [Risk] `acceptEdits` 名称和真实 CLI 语义不完全一致，用户会误判风险边界
  → Mitigation：把 Phase 3 设为显式验证门槛，必要时调整文案而不是硬套既有描述。

- [Risk] 修改 ModeSelect provider gating 时波及 Codex/Gemini 现有体验
  → Mitigation：provider-specific tests 必须覆盖 Claude/Codex/Gemini 至少三种路径。

- [Trade-off] 渐进式 rollout 交付节奏更慢
  → 这是有意为之。当前核心不是“尽快展示所有模式”，而是“让每个开放出来的模式都可解释、可验证、可回退”。

## Migration Plan

1. Phase 1
   - 去掉 `app-shell` 中对 `Claude` 的强制 `full-access` 初始化覆盖。
   - 调整 `ModeSelect`，在 Claude 下开放 `plan + bypassPermissions(full-access)`，继续禁用 `default + acceptEdits`。
   - 校准中英文文案，明确这是“渐进式开放”的当前阶段。
   - 补前端与 runtime mapping tests。

2. Phase 2
   - 在 Rust Claude event conversion 层识别权限请求事件。
   - 将其统一映射到现有 approval/request event 流。
   - 打通 accept/decline 响应链。
   - 仅在该链稳定后开放 Claude `default`。

3. Phase 3
   - 复核 `acceptEdits` 在当前 Claude CLI 版本下的真实行为。
   - 补“文件编辑自动通过 / 命令审批保留”的回归测试。
   - 行为和文案对齐后再开放 `acceptEdits`。

4. Rollback
   - 任一阶段如发现审批链不稳定，可回退到上一个 phase 的 mode gating。
   - 最保守回退路径是恢复“仅 `full-access` 可选”，但仅作为短期应急手段。

## Open Questions

- Claude CLI 当前版本的权限请求事件是否已有稳定、可机器识别的结构化信号，还是仍需兼容多种 event shape？
- `acceptEdits` 在当前项目采用的 Claude CLI 版本中，是否严格等价于“自动文件编辑 + 命令审批保留”？
- Phase 2 是否需要为 Claude approval request 增加额外的 `toolName` / `command preview` 规范化，以便沿用现有 allowlist 逻辑？
