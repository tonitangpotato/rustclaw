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
- `gid-core` v0.2.1 — graph-indexed project management

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
- Commits: engramai `50ceb93`, RustClaw `97e5e3e`

### E2E Testing (2026-03-29)
- Comprehensive e2e test in `src/memory.rs` mod `e2e_tests`
- 8 sub-tests covering full memory pipeline
- Total: 130→140 tests across development

## Core Rules

- **NEVER simplify the architecture** — follow the design (potato's explicit rule)
- Use GID for ALL project/task tracking
- **NEVER fabricate numbers** — always compute from data
- Double-write rule: MEMORY.md + daily log + engram for key learnings

*Last updated: 2026-03-29*
