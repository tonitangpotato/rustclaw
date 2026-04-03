# RustClaw — Rust Agent Framework

> A Rust-native agent framework with cognitive memory, multi-agent orchestration, and security-first design.

## Why

OpenClaw (TypeScript) works but:
- No lifecycle hooks → Engram integration required core modifications
- No native multi-agent → workarounds needed for agent swarm
- Growing tech debt from fork divergence (11 commits, 193 files changed)
- No sandbox/safety layer

IronClaw (Rust, by Illia Polosukhin) has great architecture but:
- Requires PostgreSQL + pgvector (heavy)
- NEAR AI auth dependency
- Still v0.16.1 (early)
- Memory system is FTS + vector, no cognitive models

RustClaw = best of both, designed for our needs.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   RustClaw                       │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Channels │  │  Agent   │  │  Multi-Agent  │  │
│  │          │  │  Runner  │  │  Orchestrator │  │
│  │ Telegram │  │          │  │  (CEO pattern)│  │
│  │ Discord  │  │ LLM Call │  │              │  │
│  │ (future) │  │ Tools    │  │  Spawn/Wait  │  │
│  └────┬─────┘  │ Hooks    │  │  Announce    │  │
│       │        └────┬─────┘  └──────┬───────┘  │
│       │             │               │           │
│  ┌────▼─────────────▼───────────────▼────────┐  │
│  │              Core Services                 │  │
│  │                                            │  │
│  │  ┌─────────┐ ┌────────┐ ┌──────────────┐ │  │
│  │  │ Engram  │ │Session │ │  Workspace   │ │  │
│  │  │ Memory  │ │Manager │ │  (files +    │ │  │
│  │  │ (native)│ │(SQLite)│ │  git worktree)│ │  │
│  │  └─────────┘ └────────┘ └──────────────┘ │  │
│  │                                            │  │
│  │  ┌─────────┐ ┌────────┐ ┌──────────────┐ │  │
│  │  │ Safety  │ │ Hooks  │ │    Cron +    │ │  │
│  │  │ Layer   │ │Registry│ │  Heartbeat   │ │  │
│  │  └─────────┘ └────────┘ └──────────────┘ │  │
│  └────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Engram as Native Memory (not plugin)
- engramai crate linked directly (not MCP, not CLI)
- Zero overhead recall/store
- Hooks: `BeforeInbound` → auto-recall, `BeforeOutbound` → auto-store
- Same SQLite DB, seamless migration from OpenClaw

### 2. Hook System (borrowed from IronClaw)
Six lifecycle hooks:
- `BeforeInbound` — before processing user message (→ Engram recall)
- `BeforeToolCall` — before executing a tool
- `BeforeOutbound` — before sending response (→ Engram store)
- `OnSessionStart` — session initialization
- `OnSessionEnd` — cleanup
- `TransformResponse` — modify final response

### 3. Multi-Agent via Git Worktree
- Each agent gets its own git worktree (branch)
- CEO agent (main) spawns specialists
- Specialists work on their own branch
- CEO merges results back to main
- Shared Engram DB with namespace isolation

### 4. Security (borrowed from IronClaw)
- Safety layer: prompt injection detection, secret leak scanning
- Sandbox: exec in isolated environments (Docker optional)
- Credential injection via proxy (never exposed to LLM)

### 5. Workspace Files (compatible with OpenClaw)
- Reads SOUL.md, AGENTS.md, USER.md, TOOLS.md, HEARTBEAT.md, MEMORY.md
- Same format, same directory structure
- Drop-in replacement — point RustClaw at existing workspace

## Inspirations from Existing Frameworks

### IronClaw (Rust, Illia Polosukhin, 5.6k ⭐)
- ✅ **Hook system** — 6 lifecycle points (BeforeInbound, BeforeToolCall, BeforeOutbound, OnSessionStart, OnSessionEnd, TransformResponse)
- ✅ **Safety layer** — prompt injection detection, secret leak scanning, credential detection
- ✅ **Sandbox** — Docker container isolation, network proxy, credential injection
- ✅ **WASM tool runtime** — tools run in WASM sandbox with capability allowlists
- ❌ Requires PostgreSQL + pgvector (heavy)
- ❌ NEAR AI auth dependency

### Hermes Agent (Python, Nous Research, 862 ⭐)
- ✅ **Auto skill generation** — agent solves hard problem → automatically writes SKILL.md for future reuse. True procedural memory.
- ✅ **Honcho user modeling** — dialectic user profiling, not just preference storage
- ✅ **Modal/serverless backend** — environment hibernates when idle, wakes on demand. Near-zero cost between sessions.
- ✅ **FTS5 session search** — search past conversations across sessions with LLM summarization
- ✅ **Subagent delegation** — spawn isolated subagents for parallel work
- ❌ Python = slow startup, large dependencies, no single binary
- ❌ No cognitive memory models (no ACT-R, Hebbian, Ebbinghaus)

### OpenClaw (TypeScript, current)
- ✅ Multi-channel (Telegram, Discord, WhatsApp, Signal, Matrix, Slack)
- ✅ Mature session management, compaction
- ✅ Heartbeat + cron system
- ❌ No lifecycle hooks (required core modifications for Engram)
- ❌ No sandbox/safety layer
- ❌ Plugin system limited to channels, not agent lifecycle

### RustClaw Differentiation
| Feature | OpenClaw | IronClaw | Hermes | **RustClaw** |
|---|---|---|---|---|
| Language | TypeScript | Rust | Python | **Rust** |
| Memory | Files only | FTS+pgvector | FTS5+Honcho | **Engram (cognitive)** |
| Hooks | ❌ | ✅ 6 points | ❌ | **✅ 6 points** |
| Safety | ❌ | ✅ Full | Basic | **✅ (from IronClaw)** |
| Auto-skills | ❌ | ❌ | ✅ | **✅ (from Hermes)** |
| Multi-agent | Basic spawn | ❌ | Basic delegate | **✅ CEO pattern + git worktree** |
| Deployment | Single binary (Node) | Single binary | pip install | **Single binary (~15MB)** |
| DB | JSON files | PostgreSQL | SQLite | **SQLite** |

## Core Integration: GID + GIDterm

RustClaw's CEO pattern is powered by potato's existing Rust projects:

### GID — Task Graph (the CEO's brain)
- Graph-indexed task management: nodes = tasks, edges = dependencies
- CEO reads the graph → finds unblocked tasks → assigns to specialists
- Specialists complete work → `gid_task_update` → next tasks unlock
- Current: TypeScript MCP server (`projects/gid/`). Future: Rust native crate.

### GIDterm — Terminal Controller (the CEO's hands)
- Rust binary (`gidterm` v0.5.0, 3,277 lines) at `~/clawd/gidterm/`
- Graph-driven terminal multiplexer: spawns sessions per task
- Already has: `agents.rs` (824 lines), `session.rs`, `workspace.rs`
- Multi-project workspace mode already implemented

### How They Fit Together

```
┌─────────────────────────────────────────────┐
│              RustClaw CEO Agent              │
│                                              │
│  GID Graph ←→ Task Selection ←→ Agent Spawn │
│       ↑              ↓              ↓        │
│  gid_task_update   Priority    Specialist    │
│  (on complete)     Queue       Agents        │
└──────────────────────┬──────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        ↓              ↓              ↓
   ┌─────────┐   ┌─────────┐   ┌─────────┐
   │Visibility│   │ Builder │   │ Trading │
   │  Agent   │   │  Agent  │   │  Agent  │
   │          │   │         │   │         │
   │ worktree │   │worktree │   │worktree │
   │ /branch  │   │/branch  │   │/branch  │
   └────┬─────┘   └────┬────┘   └────┬────┘
        │              │              │
        └──────────────┼──────────────┘
                       ↓
              Engram (shared memory)
```

### autoresearch Pattern (from Karpathy)
- **program.md** = lightweight skill file defining agent behavior
- **Fixed budget** = each specialist gets token/time budget
- **Auto-evaluate** = success criteria checked automatically where possible
- **Git as state** = commit before experiment, revert on failure
- CEO sets checkpoints at key decision points for human review

### Task Assignment Flow
```
1. CEO reads GID graph
2. Finds tasks with no unmet dependencies
3. For each ready task:
   a. Has clear success criteria? → spawn specialist autonomously
   b. Needs human judgment? → queue for review, work on other tasks
4. Specialist completes → gid_task_update(done) → unlock dependents
5. CEO merges specialist's branch → repeat
```

## MVP Scope (Week 1-2)

### Must Have (ALL DONE ✅)
- [x] Telegram channel (receive messages, send text/voice, STT via Whisper)
- [x] LLM provider: Anthropic (Claude) via HTTP (OAuth + stealth headers)
- [x] Session management (SQLite)
- [x] Workspace file loading (SOUL.md, AGENTS.md, etc.)
- [x] Engram native integration (auto-recall + auto-store via hooks)
- [x] Hook system (6 hook points)
- [x] Basic tool: exec (shell commands)
- [x] Basic tool: read/write files
- [x] Cron + Heartbeat
- [x] TTS via edge-tts

### Nice to Have (ALL DONE ✅)
- [x] Multi-agent orchestrator (CEO pattern + git worktree)
- [x] Safety layer (IronClaw port: sanitizer, leak detector, policy engine, credential detect — 1,995 lines)
- [x] Web fetch tool
- [x] Browser control (via CDP)
- [x] Multiple LLM providers (OpenAI, Google)
- [x] FTS5 session search (search past conversations — from Hermes)

### Also Done ✅ (originally "Future")
- [x] Auto skill generation (from Hermes)
- [x] Discord, Slack, Signal, WhatsApp, Matrix channels (6 total)
- [x] Distributed agent bus (TCP)
- [x] Serverless hibernate/wake
- [x] Docker sandbox (ephemeral, capabilities mode — see persistent upgrade below)
- [x] GID native task graph
- [x] Credential proxy/injection
- [x] User modeling (Honcho-style)
- [x] Config hot-reload (SIGHUP + file watcher)
- [x] Dashboard (axum web UI on :8081)
- [x] Streaming responses (SSE)

### Future (not yet implemented)
- [ ] WASM tool sandbox (from IronClaw)
- [ ] **Builder specialist → Claude Code subprocess**
  - Builder specialist 不自己写代码，而是启动 Claude Code CLI 作为 PTY 子进程
  - 流程：CEO 分配编码任务 → builder 用 `claude 'task description'` 在目标 workdir 启动 Claude Code → 监控进度 (process poll/log) → 收集结果 → 回报 CEO
  - 优势：Claude Code 有编码专用优化（extended thinking, ripgrep, context management, coding system prompt）
  - 用 git worktree 隔离并行任务，避免冲突
  - 完成后 `openclaw gateway wake` 通知 CEO
  - 参考：OpenClaw coding-agent skill 已验证此模式可行
- [ ] Modal/serverless execution backend (from Hermes)
- [ ] Distributed agents (across machines)
- [ ] **Persistent Docker sandbox** — current Docker sandbox uses ephemeral containers (`--rm` per exec call), which breaks multi-step workflows (no state persistence, can't see workspace files unless mounted). When multi-user or untrusted code execution is needed, upgrade to: persistent container per session, workspace volume binding, `docker exec` for commands, container lifecycle management (create/reuse/destroy). ~500-800 lines. Not needed for single-user personal use — capabilities mode (path whitelist + timeout) is sufficient.

## Crate Dependencies

```toml
[dependencies]
engramai = "0.1"          # Cognitive memory
tokio = { version = "1", features = ["full"] }
axum = "0.8"              # HTTP server (webhook)
reqwest = "0.12"          # HTTP client (LLM APIs, Telegram)
sqlx = { version = "0.8", features = ["sqlite"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"           # Logging
clap = { version = "4", features = ["derive"] }  # CLI
```

## Migration Plan

1. **Phase 1**: Build RustClaw MVP, test with a new Telegram bot
2. **Phase 2**: Point RustClaw at existing workspace (~/clawd)
3. **Phase 3**: Run both in parallel, compare behavior
4. **Phase 4**: Switch primary to RustClaw, keep OpenClaw as fallback
5. **Phase 5**: Retire OpenClaw

## Naming

- Crate: `rustclaw`
- Binary: `rustclaw`
- Repo: `tonitangpotato/rustclaw`

---

*Created: 2026-03-08*
*Status: Design phase*
