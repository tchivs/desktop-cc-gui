## 0. 实施批次

### Batch A [P0] Phase 1 安全开放

- [x] A.1 [文件:`src/app-shell.tsx`][目标: 去掉 Claude 模式初始化时的强制 `full-access` 覆盖][完成定义: 用户在 Claude 下选择的模式不会在产品层被静默改回全自动]
- [x] A.2 [文件:`src/features/composer/components/ChatInputBox/selectors/ModeSelect.tsx`][目标: 在 Claude 下开放 `plan + bypassPermissions`，继续禁用 `default + acceptEdits`][完成定义: Phase 1 模式可选态与 proposal 一致]
- [x] A.3 [文件:`src/i18n/locales/zh.part2.ts`,`src/i18n/locales/en.part2.ts`][目标: 校准 Claude 模式文案，明确渐进式开放阶段语义][完成定义: 文案不再暗示未开放模式已稳定可用]
- [x] A.4 [文件:`src/features/composer/components/ChatInputBox/ModeSelect.test.tsx`,`src/services/tauri.test.ts`][目标: 补 UI mode -> payload mapping 回归测试][完成定义: Claude `plan` 与 `full-access` 两条链路断言通过]

### Batch B [P0] Claude Approval Bridge

- [x] B.0 [文件:`src-tauri/src/engine/claude.rs`,`src-tauri/src/engine/events.rs`][目标: 在 approval bridge 缺失期间，把已识别的 Claude `AskUserQuestion permission denied` 显式映射到 `collaboration/modeBlocked`][完成定义: 用户不会只看到静默失败，GUI 能给出“先切 Plan mode”的可解释提示]
- [ ] B.1 [文件:`src-tauri/src/engine/claude/event_conversion.rs`,`src-tauri/src/engine/claude.rs`][目标: 识别 Claude 运行时中的 approval-required 事件][完成定义: Claude 需要审批的操作可被 runtime 识别并标准化]
- [ ] B.2 [文件:`src-tauri/src/engine/events.rs`,`src/features/app/hooks/useAppServerEvents.ts`][目标: 将 Claude approval request 映射到现有 approval/request 主链路][完成定义: UI 可收到 Claude 审批请求而不是静默失败]
- [ ] B.3 [文件:`src-tauri/src/codex/mod.rs` 或对应共享响应入口][目标: 复用现有 `respond_to_server_request` 响应 Claude 审批结果][完成定义: accept/decline 能正确回到 Claude 运行时]
- [ ] B.4 [文件:`src/features/threads/hooks/useThreadApprovalEvents.test.tsx`,`src/features/app/hooks/useAppServerEvents.test.tsx`][目标: 补 Claude 审批桥接测试][完成定义: Claude approval request -> toast -> response 路径可回归验证]

### Batch C [P1] Phase 2/3 逐步开放

- [ ] C.1 [依赖:Batch B][文件:`src/features/composer/components/ChatInputBox/selectors/ModeSelect.tsx`][目标: approval bridge 稳定后开放 Claude `default`][完成定义: `default` 在 Claude 下可选且审批请求可达]
- [ ] C.2 [依赖:Batch B][文件:`src-tauri/src/engine/claude.rs`,`src/i18n/locales/zh.part2.ts`,`src/i18n/locales/en.part2.ts`][目标: 校验并对齐 `acceptEdits` 的真实 CLI 语义][完成定义: 文案与 CLI 实际行为一致]
- [ ] C.3 [依赖:C.2][文件:`src/features/composer/components/ChatInputBox/selectors/ModeSelect.tsx`,`src/services/tauri.test.ts`][目标: 在语义确认后开放 Claude `acceptEdits`][完成定义: `acceptEdits` 开放不会破坏现有审批/执行语义]

## 1. 验证门禁

- [x] V.1 `npm run typecheck`
- [x] V.2 `npm run test`
- [x] V.3 `npm run check:runtime-contracts`
- [ ] V.4 `npm run doctor:strict`
- [ ] V.5 Claude 手测矩阵：
  - `plan` 模式发送消息时进入只读执行
  - `full-access` 模式发送消息时不进入审批链
  - `default` 模式在开放后能弹出现有审批 toast
  - `acceptEdits` 模式在开放后满足最终定义的行为语义

## 2. 回滚策略

- [ ] R.1 若 Phase 1 回归异常，回滚到仅 `full-access` 可选
- [ ] R.2 若 Claude approval bridge 不稳定，保持 `default/acceptEdits` 继续禁用
- [ ] R.3 若 `acceptEdits` 语义与 CLI 不一致，延后 Phase 3，不强行开放
