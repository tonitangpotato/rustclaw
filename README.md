# 🦀 RustClaw

**Rust-native AI agent framework** with cognitive memory, multi-agent orchestration, and security-first design.

> Single binary. Zero IPC overhead. Native Engram memory.

## Why RustClaw?

| Feature | OpenClaw | IronClaw | RustClaw |
|---|---|---|---|
| Language | TypeScript | Rust | **Rust** |
| Memory | MCP/external | PostgreSQL | **engramai (native)** |
| Multi-agent | Config-based | Not yet | **Hierarchical orchestration + GID code intelligence** |
| Binary size | ~200MB (Node) | ~15MB | **~35MB** |
| Startup | ~2s | ~1s | **<100ms** |
| Memory recall | ~200ms (MCP) | N/A | **~5ms** |

## Architecture

```
┌──────────────────────────────────────┐
│            RustClaw Agent            │
│                                      │
│  Workspace Files (SOUL.md, etc.)     │
│  ↕                                   │
│  Hooks (6 lifecycle points)          │
│  ↕                                   │
│  Agent Loop (LLM + Tools)            │
│  ↕                                   │
│  Engram (native cognitive memory)    │
└──────────────┬───────────────────────┘
               │
    ┌──────────┼──────────┐
    ↓          ↓          ↓
 Telegram    CLI       (future)
```

## Quick Start

```bash
# Build
cargo build --release

# Copy and edit config
cp rustclaw.example.yaml rustclaw.yaml
# Edit rustclaw.yaml with your API key and bot token

# Create workspace files
echo "Be helpful." > SOUL.md

# Run
./target/release/rustclaw run
```

## Workspace Files

RustClaw uses markdown files as its "operating system" — compatible with OpenClaw:

| File | Purpose |
|---|---|
| `SOUL.md` | Agent personality and values |
| `AGENTS.md` | Workspace conventions |
| `USER.md` | Info about the human |
| `TOOLS.md` | Local tool notes |
| `HEARTBEAT.md` | Periodic check tasks |
| `MEMORY.md` | Long-term memory |
| `IDENTITY.md` | Name, emoji, vibe |

## Tools

Built-in tools available to the agent:

- **exec** — Execute shell commands
- **read_file** — Read file contents
- **write_file** — Write files (creates dirs)
- **edit_file** — Surgical text replacement
- **list_dir** — List directory contents
- **web_fetch** — Fetch URL content
- **web_search** — Brave search API
- **engram_recall** / **engram_store** — Cognitive memory recall and storage
- **gid_*** — 30 graph-indexed development tools (code intelligence, task tracking, impact analysis)

## Hooks

6 lifecycle hook points (inspired by IronClaw):

```rust
enum HookPoint {
    BeforeInbound,    // Before processing user message
    BeforeToolCall,   // Before executing a tool
    BeforeOutbound,   // Before sending response
    OnSessionStart,   // New session created
    OnSessionEnd,     // Session ended
    TransformResponse // Transform final response
}
```

## Auth Profile Rotation

Multi-token auth with automatic rotation and cooldown tracking (matches OpenClaw's design):

```
~/.rustclaw/auth-profiles.json    ← credential store (never in rustclaw.yaml)
```

- **Multiple OAuth profiles** per provider with priority ordering
- **Round-robin rotation** — sorted by `lastUsed` (oldest first)
- **Auto-failover** on 429 (rate limit) and 529 (overloaded)
- **Exponential backoff cooldown** — 1min → 5min → 25min → 1h max
- **Automatic cooldown expiry** — error counts reset after cooldown passes
- **Usage stats persisted** to disk after each request

```json
{
  "version": 1,
  "profiles": {
    "anthropic:keychain": { "type": "oauth", "provider": "anthropic", "access": "keychain", "refresh": "keychain", "expires": 9999999999999 },
    "anthropic:default":  { "type": "token", "provider": "anthropic", "token": "sk-ant-..." },
    "anthropic:manual":   { "type": "token", "provider": "anthropic", "token": "sk-ant-..." }
  },
  "order": { "anthropic": ["anthropic:keychain", "anthropic:default", "anthropic:manual"] }
}
```

Special `"access": "keychain"` profile uses dynamic OAuth from macOS Keychain (auto-refresh).

## Memory

Native [engramai](https://crates.io/crates/engramai) integration — neuroscience-grounded cognitive memory:

- **ACT-R** activation model (frequency × recency power law)
- **Hebbian** learning (co-activation links)
- **Ebbinghaus** forgetting curves
- **Hybrid Search** — 15% FTS + 60% embedding + 25% ACT-R
- **LLM Extraction** — Claude Haiku extracts key facts at store time
- **EmpathyBus** — drive alignment, behavior feedback, emotional trends
- **Cross-language alignment** — Chinese SOUL drives align with English content via embeddings
- **Auto-recall** before each LLM call (~5ms)
- **Auto-store** after each LLM response (with fact extraction)
- **Auto-consolidation** every 6 hours
- **Self-reflection** every 24 hours (prune, decay, soul/heartbeat suggestions)
- **Anomaly detection** — monitors engram storage health (BaselineTracker)

### Embedding Pipeline

All embeddings use **nomic-embed-text** (768-dim) via local Ollama:

| Stage | Embedding Use | Cost |
|-------|--------------|------|
| Store | Content → vector for future recall | Local, free |
| Recall | Query → vector → cosine similarity | Local, free |
| Drive Alignment | Content vector ↔ drive vectors → importance boost | Reuses store embedding, zero extra cost |

Drive alignment is **multilingual by design** — embeddings capture semantic meaning across languages. A Chinese SOUL drive ("帮potato实现财务自由") naturally aligns with English content ("trading profit") because the embedding model maps related concepts to nearby vectors regardless of language.

## Roadmap

- [x] Core agent loop with tool execution
- [x] Telegram channel (long polling)
- [x] Native Engram memory (full EmpathyBus integration)
- [x] 6-point hook system
- [x] Auth profile rotation (multi-token, cooldown tracking)
- [x] Cron system (standard cron expressions + timezone)
- [x] Multi-agent orchestration (orchestrator + specialist sub-agents)
- [x] Safety layer (prompt injection detection, sensitive leak check)
- [x] GID integration (30 tools — code intelligence, task tracking, impact analysis)
- [x] EmpathyBus (drive alignment, behavior feedback, emotional trends, self-reflection)
- [x] Cross-language drive alignment (embedding-based)
- [x] Voice I/O — whisper.cpp STT (local, ~3s for 40s audio) + edge-tts TTS (Telegram voice messages)
- [x] Config hot-reload (FSEvents file watcher)
- [x] Stream mode (Telegram typing effect with chunked responses)
- [x] Skill system (markdown-based workflow definitions, auto-loaded from `skills/`)
- [x] Per-chat voice mode toggle
- [ ] SQLite session persistence
- [ ] Reply-to-message context (quoted message parsing)
- [ ] Web dashboard enhancements (orchestrator view, agent name)
- [ ] Hot-reload orchestrator config
- [ ] WASM tool sandbox

## License

**AGPL-3.0** — Free for open source use.

For commercial/proprietary deployments, a separate commercial license is available. See [LICENSE](LICENSE) for details.
