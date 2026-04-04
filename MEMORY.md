# MEMORY.md - Long-term Memory (RustClaw)

> This is RustClaw's memory — the Rust-native AI agent framework.
> Engram DB at `~/rustclaw/engram-memory.db`.

---

## About potato

- **Name**: potato (oneB)
- 全职程序员, building towards financial freedom
- **Personality**: curious, honest, moves FAST, prefers action over planning
- **Values honesty** over performance. Trusts agent with big ideas.

### Working Style
- potato has ideas → crystallize into designs → iterate fast
- "以后never简化问题" — potato的明确要求
- Prefers deep explanations with concrete examples, not jargon

---

## RustClaw Development History

### Architecture
- **Rust AI agent framework** — full-featured, single binary
- 35MB release binary, 140 tests, 0 warnings
- **Channels**: Telegram (@rustblawbot), Discord, Signal, WhatsApp, Matrix, Slack
- **Memory**: Engram (engramai crate) + GID (gid-core crate) + file-based logs
- **LLM**: Anthropic (Claude), OpenAI, Google — streaming support
- **Orchestrator**: Multi-specialist delegation (coder + researcher)
- **Dashboard**: Web UI at port 8081

### Dependencies (crates.io)
- `engramai` v0.2.2 — neuroscience-grounded memory (ACT-R, Hebbian learning)
- `gid-core` v0.2.1 — graph-indexed code intelligence + task management

### Completed Features (all 13 TODOs done, 2026-03-28)
- ✅ Token Tracking — TokenTracker atomic counters, all providers
- ✅ Heartbeat Channel Routing — non-HEARTBEAT_OK responses auto-send to Telegram
- ✅ Streaming Telegram — typing indicator + streaming output
- ✅ Session Persistence — SQLite-backed conversation history
- ✅ Sub-agent Shared Engram — `for_subagent_with_memory()`
- ✅ Hot-reload Orchestrator — config changes auto-update specialists
- ✅ TTS/STT — built-in VOICE: prefix, OGG output
- ✅ Dashboard Agent Name — reads from IDENTITY.md
- ✅ Dashboard Orchestrator View — /api/tokens + /api/orchestrator
- ✅ Interactive CLI — `rustclaw chat` REPL with /clear
- ✅ Interactive Setup — `rustclaw init` wizard
- ✅ Code Cleanup — dead code removed, 0 warnings

### Cross-Language Drive Alignment (2026-03-29)
- Problem: SOUL.md in Chinese → keyword matching fails for English content
- Solution: `score_alignment_hybrid()` = max(keyword, embedding) in engramai
- DriveEmbeddings pre-computed at startup, threshold 0.3 for cross-language

### Context System Refactor (2026-03-29, biggest change)
- **src/context.rs** — 6 new types: MessageContext, ChatType, QuotedMessage, ChannelCapabilities, RuntimeContext, ProcessedResponse
- **MessageContext** — LLM sees sender name/username, chat type (direct/group), quoted messages
- **ChannelCapabilities** — channels declare what they support (voice, tables, markdown, etc.), LLM adapts output format
- **RuntimeContext** — OS, arch, version, hostname injected into system prompt
- **ProcessedResponse** — unified extraction of VOICE:, NO_REPLY, [[reply_to:N]] from raw LLM output
- **Modular system prompt** — broke monolithic format! into 10 composable sections
- **Yesterday's daily notes** — system prompt now loads yesterday's log too

### Skill System (2026-03-29)
- **Skills auto-loading** — scans `skills/*/SKILL.md`, injects into system prompt
- **Dynamic trigger matching** — YAML frontmatter with triggers, priority, always_load
- **Idea Intake Pipeline** — first skill: processes URLs/ideas into IDEAS.md + engram + GID

### Bug Fixes (2026-03-29)
- **fd leak** — notify kqueue→fsevent, config watcher watches file not directory
- **FTS5 corruption** — rebuilt full-text search index in engram DB
- **block_in_place** — OAuth token refresh in async context panic fix
- **whisper.cpp** — Python whisper→whisper-cli, 3x faster STT (32s→11s)

### Behavior Improvements (2026-03-29)
- Persistent typing indicator (refresh every 4s)
- Unified send_response (voice/text logic consolidated)
- Voice mode toggle per chat
- "Acknowledge before working" rule in system prompt + AGENTS.md

### Test Count: 166 (up from 140)

## Core Rules

- **NEVER simplify the architecture** — follow the design (potato's explicit rule)
- Use GID for code structure analysis, dependency tracking, impact queries, and task management
- **NEVER fabricate numbers** — always compute from data
- Double-write rule: MEMORY.md + daily log + engram for key learnings

### Architecture Notes
- **context.rs** is the new "structured metadata" layer between channels and the agent
- System prompt is modular: context files → skills → channel caps → runtime → behavior rules
- Skills are markdown-based workflows with YAML frontmatter triggers — no Rust code needed

*Last updated: 2026-04-03*

---

## GID Ecosystem (2026-04-02)

### 四个项目定位
- **gid-core** — 图引擎 + 共享类型（事件格式、状态 schema）
- **gid-harness** — AI 自主开发执行引擎 ✅ **已完整实现**
- **gidterm** — TUI surface，纯展示层，读 execution-log.jsonl
- **agentctl** — daemon 进程管家（TUI + Telegram bot，7,001行，38 tests）

### gid-harness ✅ DONE
- **15 个 Rust 源文件，6,881 行代码**
- 路径：`/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/harness/`
- 模块：executor, scheduler, replanner, context, notifier, planner, verifier, topology, worktree, config, types, telemetry, log_reader, execution_state
- 文件系统是 backend：graph.yml + execution-log.jsonl + execution-state.json
- 7-Phase 流程（Phase 1-3 人机协作，Phase 4-7 AI 自动）
- gate:human tag 做审批控制

### 关键架构决策
- 方案 B：harness 独立实现，gidterm 是纯 UI
- 共享协议不共享代码：事件格式和状态 schema 在 gid-core
- 所有 surface（Telegram、gidterm、CLI）读写同一套文件

---

## 产品商业化定位 (2026-04-03 potato 明确)

### 可卖钱的产品
- **xinfluencer** — X/Twitter 影响力增长工具，Rust，6,462 行，13 模块
  - 自用：集成进 RustClaw，Telegram Bot 控制
  - 商业：作为独立 SaaS 产品卖
  - 功能：autopilot, engage, discover, crawler, scoring, brand_audit, graph, monitor
- **Knowledge Compiler** (IDEA-20260403-02) — 知识管理产品化

### 内部工具（不适合直接卖）
- **gid-harness** — AI 开发执行引擎，主要内部使用，作为服务卖比较困难
- **agentctl** — 进程管家，纯运维工具
