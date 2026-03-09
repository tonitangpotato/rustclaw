# RustClaw Test Results — 2026-03-09

## Environment
- Binary: 23MB release build (arm64 macOS)
- Model: claude-sonnet-4-5 via OAuth
- Bot: @rustblawbot (Telegram)
- Dashboard: http://localhost:8081

## Test Matrix

### Core (All Passed ✅)
| Test | Status | Notes |
|------|--------|-------|
| Telegram polling | ✅ | Messages received, responses sent |
| OAuth authentication | ✅ | Bearer + stealth headers, 200 OK |
| Workspace injection | ✅ | SOUL/USER/MEMORY loaded, 0 tool call |
| Session persistence | ✅ | Multi-turn context maintained (SQLite) |
| Agentic loop | ✅ | 4-turn tool use (read_file error → exec → read_file → respond) |
| Tool: read_file | ✅ | Read rustclaw.yaml successfully |
| Tool: exec | ✅ | Executed shell commands |
| Hooks | ✅ | PromptInjection + SensitiveLeak registered |
| Request timeout | ✅ | 120s timeout prevents hangs |
| Response logging | ✅ | Full token/stop_reason/tool_calls logging |
| Dashboard API | ✅ | /api/status returns correct info |
| Config hot-reload | ✅ | Watcher started on rustclaw.yaml |

### Infrastructure (Verified Running)
| Component | Status | Notes |
|-----------|--------|-------|
| TTS pipeline | ✅ | edge-tts → ffmpeg → ogg works (tested manually) |
| STT code | ✅ | Whisper integration compiled, needs voice message test |
| Dashboard UI | ✅ | Tailwind HTML served on :8081 |
| Memory (Engram) | ✅ | Auto-recall enabled |
| Session DB | ✅ | SQLite initialized |
| Plugin system | ✅ | Ready (0 plugins loaded) |

### Not Yet Tested
| Feature | Reason |
|---------|--------|
| STT (voice message) | Need to send voice note to bot |
| TTS (voice reply) | Need LLM to output VOICE: prefix |
| Multi-LLM (OpenAI/Google) | Need API keys configured |
| FTS5 search | Need search tool invocation |
| Distributed agent bus | Needs multi-node setup |
| Serverless hibernate | Needs idle timeout trigger |
| Orchestrator (CEO) | Disabled in config |
| Browser (CDP) | Need Chrome running |
| GID tasks | Need GID MCP configured |

## Performance
- Cold start: <1 second
- First response: ~13 seconds (Sonnet, 10983 input tokens)
- Tool-use response: ~36 seconds (4 turns, 3 API calls)
- System prompt size: ~11K tokens (workspace injection)

## Bugs Found & Fixed
1. **No request timeout** — reqwest had no timeout, causing silent hangs. Fixed: 120s request + 10s connect timeout.
2. **Missing response logs** — No log after LLM response. Fixed: added token/stop/text logging.
3. **YAML env var expansion** — `${VAR}` not expanded by serde_yaml. Workaround: use env vars or hardcode values.
4. **OAuth token in YAML** — Token written to file got corrupted by regex. Use env var `ANTHROPIC_AUTH_TOKEN` instead.
