# RustClaw Batch 3 — Architecture Features Design

## Overview

5 architecture-level features that transform RustClaw from a single-agent bot into a full agent platform.

---

## 11. CEO Multi-Agent Orchestration (GID Integration)

### Core Idea
CEO agent (Clawd) reads GID task graph → finds unblocked tasks → spawns specialist agents → monitors completion → merges results.

### Architecture
```
┌─────────────────────────────────────────────┐
│              CEO Agent (main)               │
│                                             │
│  GID Graph ←→ Task Scheduler ←→ Agent Pool  │
│       ↑              ↓              ↓       │
│  task_update    Priority Q     Specialist    │
│                               Agents (N)    │
└─────────────────┬───────────────────────────┘
                  │
    ┌─────────────┼─────────────┐
    ↓             ↓             ↓
┌─────────┐ ┌─────────┐ ┌─────────┐
│Agent A  │ │Agent B  │ │Agent C  │
│worktree │ │worktree │ │worktree │
│/branch  │ │/branch  │ │/branch  │
└────┬────┘ └────┬────┘ └────┬────┘
     └────────────┼────────────┘
                  ↓
         Engram (shared memory)
```

### Implementation Plan

**Phase 1: GID Client** (`src/gid.rs`)
- Parse `graph.yml` from workspace `.gid/` directory
- Read nodes (tasks) and edges (dependencies)
- Filter for tasks with status "todo" and no unmet dependencies
- Return ordered list of ready tasks

**Phase 2: Task Scheduler** (`src/scheduler.rs`)
- `TaskScheduler` struct with configurable concurrency (max_parallel_agents)
- Poll GID graph on interval (or on-demand)
- For each ready task:
  - Check if success criteria is auto-evaluable
  - If yes: spawn specialist agent autonomously
  - If no: queue for CEO review
- Track running tasks, handle completion/failure

**Phase 3: Agent Pool** 
- Extend existing `SubAgent` from multi-agent support
- Each specialist gets: own workspace (git worktree), own session, own model config
- Agent pool manages lifecycle: spawn, monitor, kill on timeout
- Results flow back to CEO via Engram shared memory

**Phase 4: Git Worktree Integration**
- `git worktree add agents/<id> -b agent/<id>` for each specialist
- On task completion: CEO reviews diff, merges to main
- On failure: `git worktree remove`, discard branch

### Config
```yaml
ceo:
  enabled: true
  gid_path: ".gid/graph.yml"
  max_parallel_agents: 3
  auto_merge: false  # require CEO review before merge
  task_poll_interval: 60  # seconds
```

### Dependencies
- gidterm's graph parsing (can port from Rust)
- git CLI for worktree management
- Existing SubAgent infrastructure

---

## 12. WASM Tool Sandbox

### Core Idea
Execute untrusted tools in a WASM sandbox for security isolation. Tools can't access filesystem, network, or secrets unless explicitly granted.

### Architecture
```
Agent Loop
  ↓ tool_call
Tool Registry
  ↓ check permissions
┌─────────────────┐
│  WASM Runtime    │ ← wasmtime
│  ┌─────────────┐ │
│  │  Tool Code  │ │ ← compiled .wasm
│  │  (sandboxed)│ │
│  └──────┬──────┘ │
│         ↓        │
│  Host Functions  │ ← controlled API surface
│  - fs_read()     │   (allowlist)
│  - http_get()    │
│  - env_get()     │
└─────────────────┘
```

### Implementation Plan

**Phase 1: WASM Runtime** (`src/sandbox/mod.rs`)
- Integrate `wasmtime` crate
- Define host function interface (WASI subset)
- Three permission levels:
  - `ReadOnly`: can read allowed files, no network, no env
  - `WorkspaceWrite`: can read/write workspace, no network
  - `FullAccess`: full WASI (for trusted tools only)

**Phase 2: Tool Compilation Pipeline**
- Tools written in Rust/AssemblyScript → compiled to .wasm
- Tool manifest: `tool.toml` with name, permissions, description
- Tool registry loads .wasm files from `tools/` directory

**Phase 3: Host Functions**
- `fs_read(path) -> bytes` — sandboxed to allowed directories
- `fs_write(path, bytes)` — only in WorkspaceWrite+
- `http_get(url) -> response` — only in FullAccess
- `env_get(key) -> value` — credential injection by host
- `log(msg)` — always allowed

### Config
```yaml
sandbox:
  enabled: true
  default_permission: "read_only"
  tools_dir: "tools/"
  memory_limit_mb: 64
  time_limit_ms: 30000
```

### Dependencies
- `wasmtime` crate (~adds 5MB to binary)
- Tool compilation toolchain (can be external)

---

## 13. Plugin System

### Core Idea
Third-party extensions via Rust traits + dynamic loading. Plugins can add tools, hooks, channels, and memory backends.

### Architecture
```
┌──────────────────────────────────────┐
│            Plugin Registry           │
│                                      │
│  ┌──────────┐  ┌──────────┐         │
│  │ Channel   │  │ Tool     │         │
│  │ Plugins   │  │ Plugins  │         │
│  │ (Discord) │  │ (custom) │         │
│  └──────────┘  └──────────┘         │
│  ┌──────────┐  ┌──────────┐         │
│  │ Hook      │  │ Memory   │         │
│  │ Plugins   │  │ Plugins  │         │
│  │ (logging) │  │ (custom) │         │
│  └──────────┘  └──────────┘         │
└──────────────────────────────────────┘
```

### Implementation Plan

**Phase 1: Plugin Traits** (`src/plugin.rs`)
```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn on_load(&mut self, ctx: &PluginContext) -> Result<()>;
    fn on_unload(&mut self) -> Result<()>;
}

pub trait ChannelPlugin: Plugin {
    fn start(&self, runner: Arc<AgentRunner>) -> Result<()>;
}

pub trait ToolPlugin: Plugin {
    fn tools(&self) -> Vec<Box<dyn Tool>>;
}

pub trait HookPlugin: Plugin {
    fn hooks(&self) -> Vec<Box<dyn Hook>>;
}
```

**Phase 2: Static Plugins (features)**
- Each plugin is a Cargo feature flag
- `--features discord,slack` enables those channels
- Compile-time plugin selection (no runtime overhead)

**Phase 3: Dynamic Plugins (future)**
- `libloading` crate for .so/.dylib loading
- Plugin manifest: `plugin.toml`
- Hot-reload support via file watcher

### Config
```yaml
plugins:
  - name: discord
    enabled: true
    config:
      bot_token: "..."
  - name: custom-tool
    path: "plugins/custom-tool.so"
```

---

## 14. Multi-Channel Support

### Core Idea
Abstract the channel layer so adding Discord, Slack, Signal is just implementing a trait.

### Architecture
```rust
#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;
    
    /// Start receiving messages (long-polling, websocket, etc.)
    async fn start(&self, sender: MessageSender) -> Result<()>;
    
    /// Send a text message
    async fn send_text(&self, target: &str, text: &str) -> Result<()>;
    
    /// Send a voice message
    async fn send_voice(&self, target: &str, ogg_path: &str) -> Result<()>;
    
    /// Send a file
    async fn send_file(&self, target: &str, path: &str) -> Result<()>;
    
    /// Edit a sent message
    async fn edit_text(&self, target: &str, msg_id: &str, text: &str) -> Result<()>;
}
```

### Channels to Implement

| Channel | Complexity | Library | Priority |
|---------|-----------|---------|----------|
| Discord | Medium | `serenity` crate | High (agent community) |
| Slack | Medium | `slack-morphism` | Medium (enterprise) |
| Signal | Hard | signal-cli wrapper | Low |
| Matrix | Medium | `matrix-sdk` | Low (open protocol) |
| CLI/stdin | Easy | built-in | High (dev/testing) |

### Implementation Plan

**Phase 1: Channel Trait** (`src/channels/mod.rs`)
- Define `Channel` trait as above
- Refactor Telegram to implement it
- `ChannelRouter` dispatches messages to correct channel

**Phase 2: CLI Channel** (`src/channels/cli.rs`)
- stdin/stdout interactive mode
- Great for development and testing
- No external dependencies

**Phase 3: Discord** (`src/channels/discord.rs`)
- `serenity` crate for Discord API
- Gateway websocket connection
- Slash commands support
- Thread support for long conversations

### Config
```yaml
channels:
  telegram:
    bot_token: "..."
  discord:
    bot_token: "..."
    guild_ids: [123456]
  cli:
    enabled: true  # for dev mode
```

---

## 15. Web Dashboard

### Core Idea
Built-in web UI for monitoring, configuration, and session management. Uses the existing `axum` dependency.

### Architecture
```
┌──────────────────────────────────────┐
│          Web Dashboard               │
│  ┌──────────┐  ┌──────────────────┐  │
│  │ Sessions  │  │ Memory Explorer │  │
│  │ (active,  │  │ (Engram search, │  │
│  │  history) │  │  stats, graph)  │  │
│  ├──────────┤  ├──────────────────┤  │
│  │ Agent     │  │ Config Editor   │  │
│  │ Monitor   │  │ (hot-reload)    │  │
│  ├──────────┤  ├──────────────────┤  │
│  │ Tool Logs │  │ Safety Audit    │  │
│  └──────────┘  └──────────────────┘  │
└──────────────────────────────────────┘
         ↕ REST API (axum)
┌──────────────────────────────────────┐
│         RustClaw Core                │
└──────────────────────────────────────┘
```

### Implementation Plan

**Phase 1: REST API** (`src/web/api.rs`)
- `GET /api/status` — system status, uptime, memory stats
- `GET /api/sessions` — list active sessions
- `GET /api/sessions/:key/messages` — session history
- `POST /api/sessions/:key/message` — inject message
- `GET /api/memory/search?q=...` — search Engram
- `GET /api/memory/stats` — memory statistics
- `GET /api/config` — current config (redacted secrets)
- `PUT /api/config` — update config (hot-reload)

**Phase 2: Static Frontend**
- Single-page app (SPA) served from `web/` directory
- Minimal: HTML + vanilla JS (or htmx for simplicity)
- No build step required (no React/Vite)
- Real-time updates via SSE (Server-Sent Events)

**Phase 3: Advanced UI**
- Session viewer with message bubbles
- Engram memory graph visualization (D3.js)
- Agent status dashboard (CPU, memory, tokens used)
- Config editor with validation

### Config
```yaml
web:
  enabled: true
  port: 8080
  auth:
    type: "basic"  # or "token"
    username: "admin"
    password_hash: "..."
```

### Dependencies
- `axum` (already in deps)
- No additional crates needed for Phase 1-2

---

## Priority & Timeline

| Feature | Effort | Impact | Priority |
|---------|--------|--------|----------|
| 14. Multi-Channel (CLI first) | 1 day | High (dev experience) | P0 |
| 15. Web Dashboard (API only) | 1 day | High (monitoring) | P0 |
| 11. CEO Multi-Agent | 2-3 days | Very High (differentiation) | P1 |
| 13. Plugin System | 1-2 days | Medium (extensibility) | P2 |
| 12. WASM Sandbox | 2-3 days | Medium (security) | P3 |

**Recommended order**: CLI channel → REST API → CEO orchestration → Plugins → WASM

Total estimated effort: **7-10 days** for all 5 features.
