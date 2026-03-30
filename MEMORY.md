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

*Last updated: 2026-03-29 (evening refactor)*
