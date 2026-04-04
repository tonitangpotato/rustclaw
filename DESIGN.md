# DESIGN.md — RustClaw Architecture Overview

## Problem Statement

RustClaw is a **Rust-native AI agent framework** that needs to provide a single-binary, low-latency alternative to TypeScript/Node agent frameworks (like OpenClaw). It must support:

- Multi-channel messaging (Telegram, Discord, Slack, Signal, WhatsApp, Matrix)
- Full agentic loop with tool execution (read/write/exec/web/memory/GID)
- Cognitive memory via native Engram integration (ACT-R, Hebbian, Ebbinghaus)
- Multi-agent orchestration (CEO → specialist sub-agents)
- Security-first design (sandbox, safety layer, prompt injection detection)
- Ritual/workflow system via GID integration (phase-scoped tool constraints)

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      main.rs (CLI)                      │
│   Commands: run | chat | config | setup | daemon        │
└────────────────────────┬────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        ▼                ▼                ▼
   ┌─────────┐    ┌───────────┐    ┌───────────┐
   │ Channels │    │ Heartbeat │    │   Cron    │
   │ (6 adapters)│ │ (periodic)│    │ (scheduled)│
   └─────┬───┘    └─────┬─────┘    └─────┬─────┘
         │              │                │
         ▼              ▼                ▼
   ┌─────────────────────────────────────────┐
   │           AgentRunner (agent.rs)        │
   │  ┌─────────┐ ┌──────────┐ ┌──────────┐ │
   │  │ Hooks   │ │ Sessions │ │ Safety   │ │
   │  │ (6 pts) │ │ (SQLite) │ │ Layer    │ │
   │  └────┬────┘ └────┬─────┘ └──────────┘ │
   │       │           │                     │
   │  ┌────▼───────────▼────────────────┐    │
   │  │     Agentic Loop (≤80 turns)    │    │
   │  │  LLM → ToolCalls → Results → ↻ │    │
   │  └────────────┬────────────────────┘    │
   │               │                         │
   │  ┌────────────▼────────────────────┐    │
   │  │  ToolRegistry (tools.rs)        │    │
   │  │  exec, read/write/edit, web,    │    │
   │  │  engram, GID (30), voice, etc.  │    │
   │  └─────────────────────────────────┘    │
   └──────────┬───────────┬──────────────────┘
              │           │
    ┌─────────▼──┐   ┌────▼──────────┐
    │ MemoryMgr  │   │ Orchestrator  │
    │ (engramai) │   │ (CEO → subs)  │
    │ EmotionBus │   │ spawn_agent() │
    └────────────┘   └───────────────┘
```

## Key Source Files

| File | Purpose |
|------|---------|
| `src/main.rs` | CLI entry point, wires all subsystems |
| `src/agent.rs` | Core `AgentRunner` — agentic loop, sub-agent spawning |
| `src/config.rs` | YAML config types + auth resolution (API key / OAuth / Keychain) |
| `src/llm.rs` | LLM client abstraction (Anthropic, OpenAI, Google) |
| `src/tools.rs` | `ToolRegistry` — all built-in tools |
| `src/session.rs` | Session management (SQLite + in-memory), summarization, microcompact |
| `src/memory.rs` | Engram memory wrapper (store/recall/consolidate/reflect) |
| `src/hooks.rs` | 6-point hook system (BeforeInbound/BeforeToolCall/BeforeOutbound/etc.) |
| `src/engram_hooks.rs` | Auto-recall and auto-store hooks |
| `src/channels/` | Platform adapters (telegram, discord, slack, signal, whatsapp, matrix) |
| `src/orchestrator.rs` | Multi-agent CEO/specialist orchestration |
| `src/safety.rs` | Prompt injection detection, sensitive leak checks, sanitization |
| `src/sandbox.rs` | WASM/Docker sandbox for tool execution |
| `src/workspace.rs` | Workspace files (SOUL.md, AGENTS.md, etc.) → system prompt |
| `src/cron.rs` | Cron job scheduler (standard expressions + timezone) |
| `src/skills.rs` | Markdown-based skill/workflow definitions |
| `src/ritual_adapter.rs` | Bridge: RustClaw LLM → GID ritual phases |
| `src/events.rs` | Agent event stream (Text/ToolStart/ToolDone/Response) |
| `src/auth_profiles.rs` | Multi-token rotation with cooldown tracking |
| `src/oauth.rs` | macOS Keychain OAuth token management |
| `src/stt.rs` | Whisper.cpp STT (local voice-to-text) |
| `src/tts.rs` | edge-tts TTS (text-to-voice) |
| `src/voice_mode.rs` | Per-chat voice mode toggle |
| `src/reload.rs` | Config hot-reload (FSEvents watcher + SIGHUP) |
| `src/dashboard.rs` | Web dashboard (Axum HTTP server) |

## Key Design Decisions

1. **Single binary** — no IPC, no sidecar processes. Everything compiles into one `rustclaw` binary (~35MB).

2. **Native Engram memory** — uses `engramai` crate directly (not MCP). Recall is ~5ms vs ~200ms for MCP-based memory. Includes ACT-R activation, Hebbian learning, Ebbinghaus decay, and EmotionBus drive alignment.

3. **Auth profile rotation** — multi-token with round-robin (oldest-first), exponential backoff cooldown on 429/529, automatic failover. Supports API keys, OAuth tokens, and macOS Keychain dynamic refresh.

4. **Event-driven agent loop** — `process_message_events()` emits `AgentEvent` variants via `mpsc` channel. Callers can stream (Telegram typing effect) or collect (simple string response).

5. **Context efficiency** — two-layer approach:
   - *Microcompact*: clears old tool result content in-memory (keeps preview)
   - *Persist-to-disk*: large tool results (>30KB) saved to disk, replaced with 2KB preview in context

6. **Ritual/ToolScope enforcement** — two layers:
   - *Layer 1*: tool visibility filtering (LLM doesn't see disallowed tools)
   - *Layer 2*: path + bash policy validation (blocks writes outside scope)

7. **Session persistence** — SQLite-backed with in-memory cache. Supports summarization via separate (cheaper) LLM model.

8. **Sub-agent isolation** — each sub-agent gets its own `Workspace`, `ToolRegistry` (scoped to its worktree), and `LlmClient`. Sessions are namespaced via `agent:{id}:` prefix.

9. **Channel abstraction** — all channels implement the `Channel` trait. Each runs as a separate tokio task with auto-restart on failure.

10. **Config hot-reload** — FSEvents file watcher + SIGHUP listener. Model, temperature, and other config changes apply without restart.

## Skills

RustClaw includes several built-in skills (Markdown-based LLM workflows):

### capture-idea (Priority 50)
- General-purpose idea intake for text, voice, and URLs
- Triggers: "idea:", "想法:", "intake", "记录一下", voice messages, any URL
- Stores to IDEAS.md + engram + daily log

### social-intake (Priority 80)
- **New**: Specialized social media content extraction and archival
- Triggers: URLs from Twitter/X, YouTube, HN, Reddit, 小红书, WeChat, GitHub
- Python engine (`skills/social-intake/intake.py`) handles platform-specific scraping
- Three-layer storage: intake/ (external content archive), IDEAS.md (triggered ideas only), engram (connections)
- Platform detection, deduplication, fallback chains (platform tool → Jina Reader → web_fetch)
- Optional video transcription (yt-dlp + whisper) and subtitle extraction
- See: `.gid/requirements-social-intake.md` and `.gid/design-social-intake.md`

Skills are defined in `skills/{name}/SKILL.md` and automatically loaded by `src/skills.rs`.

## Remaining Roadmap

- [ ] Reply-to-message context (quoted message parsing in Telegram/Discord)
- [ ] Web dashboard enhancements (orchestrator view, agent names)
- [ ] Hot-reload orchestrator config
- [ ] WASM tool sandbox (currently stubbed)
- [ ] Vision model integration for social-intake (direct image OCR)
