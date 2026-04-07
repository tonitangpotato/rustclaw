# Issues: RustClaw

> 项目使用过程中发现的 bug、改进点和待办事项。
> 格式: ISS-{NNN} [{type}] [{priority}] [{status}]

---

## ISS-001 [bug] [P0] [open]
**发现日期**: 2026-04-04
**发现者**: potato
**组件**: src/orchestrator.rs, src/session.rs

**描述**:
spawn 的 sub-agent session key 不在主 agent 的 `cancellation_tokens` 里，导致 `/stop` 命令无法 kill sub-agent。

**上下文**:
三层问题：(1) sub-agent 没注册 CancellationToken (2) loop 里不检查 cancellation (3) /stop 不知道子 session key。potato 在使用中等了很久 sub-agent 不停。

**建议方案**:
- 注册 sub-agent 的 CancellationToken 到全局 map
- agentic loop 每轮检查 cancellation
- parent→children 映射，/stop 级联 cancel 所有子 session

**相关**:
- 已指派给 OpenClaw 修复（2026-04-04 daily log）

---

## ISS-002 [improvement] [P1] [open]
**发现日期**: 2026-04-07
**发现者**: potato
**组件**: gid-core/src/code_graph.rs (tree-sitter + name-matching)
**项目**: gid-rs (`/Users/potato/clawd/projects/gid-rs/`)

**描述**:
当前 code_graph.rs (7039 lines) 使用 tree-sitter 提取结构 + name-matching heuristics 生成 call edges。Name-matching 产生 ~28K call edges，包含大量 false positives（例如不同类型的同名方法被错误链接）。

**上下文**:
- Tree-sitter 提供结构化信息（files, classes, functions, imports）
- Name-matching 对 call sites 进行启发式匹配，无类型信息
- 测试目标代码库: claude-code-source-code (1902 files, 512K lines TypeScript)
- 需要 compiler-precise edges 来提高准确率

**建议方案**:
实现 LSP client 增强 call-edge 检测精度：

1. **新模块**: `crates/gid-core/src/lsp_client.rs`
   - 轻量级 LSP client (initialize, definition queries, shutdown)
   - 通过 stdio 启动 language servers: typescript-language-server, rust-analyzer, pyright
   - 发送 textDocument/definition requests 获取精确定义位置

2. **修改 code_graph.rs**:
   - Tree-sitter pass: 提取结构 + call sites
   - LSP pass (optional): 为每个 call site 查询精确定义
   - 用 LSP 结果替换/增强 name-matched edges (confidence 1.0 vs 0.5)

3. **CLI 增强**:
   - 添加 `gid extract --lsp` flag 启用 LSP-enhanced extraction
   - 支持语言选择: `--lsp=ts,rust,python`

4. **实现阶段**:
   - Phase 1: TypeScript support (typescript-language-server)
   - Phase 2: Rust support (rust-analyzer)
   - Phase 3: Python support (pyright)

**预期效果**:
- Precision: >95% (vs ~70% with name-matching)
- 减少 false positive edges >50%
- 性能: <5 分钟处理 claude-code-source-code

**相关**:
- 设计文档: `docs/DESIGN-LSP-CLIENT.md`
- 目标测试代码库: `/Users/potato/clawd/projects/claude-code-source-code`

---

## ISS-003 [improvement] [P1] [open]
**发现日期**: 2026-04-05
**发现者**: RustClaw
**组件**: src/session.rs, agentic loop

**描述**:
Session persist 只在 agentic loop 结束时一次性写入。如果进程中途被 kill（rate limit、crash），整个 session 历史丢失。

**上下文**:
已有分析文档：`/Users/potato/rustclaw/docs/SESSION-PERSIST-PLAN.md`。方案是 incremental persist——在关键节点（tool call 后、每 N turns）做增量写入。

**建议方案**:
- Phase 1: Turn-level persist（每个 user→assistant turn 后写入）
- Phase 2: Tool call persist with debounce
- 修改 src/session.rs + agentic layer

**相关**:
- docs/SESSION-PERSIST-PLAN.md

---

## ISS-004 [bug] [P1] [open]
**发现日期**: 2026-04-01
**发现者**: potato
**组件**: Telegram channel (src/channels/telegram.rs)

**描述**:
语音消息回复时引用的消息（quoted message）有时看不到。potato 引用一条消息发送语音，RustClaw 只收到语音转文字内容，没有 quoted message 信息。

**上下文**:
后续测试发现文字引用是正常的，问题可能只出现在语音+引用的组合情况。需要进一步排查 Telegram API 在语音消息场景下是否传递 reply_to_message。

**建议方案**:
待 debug，检查 Telegram Bot API 语音消息的 reply_to_message 字段是否正确解析。

**相关**:
- 2026-04-01 daily log

---

## ISS-005 [improvement] [P1] [open]
**发现日期**: 2026-04-07
**发现者**: potato + Claude Code
**组件**: src/autopilot.rs, src/channels/telegram.rs

**描述**:
`/autopilot` 不带参数时应自动发现 `tasks/` 目录下最新的任务文件，而不是默认 HEARTBEAT.md。

**上下文**:
任务文件按日期命名存放在 `tasks/` 下（如 `tasks/2026-04-07.md`）。每天新任务创建新文件。autopilot 应自动找到最新的。

**建议方案**:
- 扫描 `tasks/*.md`，从文件名解析日期（`YYYY-MM-DD.md`）
- 选最新的
- `tasks/` 为空则 fall back 到 HEARTBEAT.md
- `/autopilot <file>` 显式指定仍然有效

**文件**: `src/autopilot.rs`, `src/channels/telegram.rs`

---
