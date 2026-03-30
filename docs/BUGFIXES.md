# RustClaw Bug Fixes & Post-Mortems

## 2026-03-30: Silent Process Exit (ROOT CAUSE FOUND)

**Symptom**: RustClaw process silently exits after running for hours. No panic, no error log. Happened repeatedly over several days.

**Root Cause**: In `src/channels/telegram.rs`, the Telegram polling loop had:
```rust
let body: serde_json::Value = r.json().await?;
```
The `?` operator propagates JSON parse errors up through `run()` → `start()` → `start_gateway()` → `main()`. If Telegram API returns invalid JSON (network glitch, partial response), the entire process exits normally (not a crash — `main` returns `Ok(())`).

**Why no error log**: The error was `Ok(())` from main's perspective — all channel tasks completed, so main returned cleanly. No panic = no panic hook. No error = no error log.

**Fix** (commit `b3941fc`):
1. **Catch JSON parse errors** in polling loop — `match` instead of `?`, log error, sleep 5s, `continue`
2. **Auto-restart channels** in `start_gateway()` — channels run in a `loop {}`, so even if they exit (error or normal), they restart after 5s
3. **Panic hook** added to `main()` — writes to `~/.rustclaw/logs/rustclaw.err` if any future panic occurs

**Prevention**: Any new `?` in the polling loop should be reviewed — errors must not propagate out of the infinite loop.

---

## 2026-03-30: Voice Mode Not Triggering from Voice Messages

**Symptom**: User sends voice message saying "开启voice mode", RustClaw transcribes it correctly but doesn't toggle voice mode. Instead, sends the transcription to the LLM which responds with text "voice mode 已开启" (but doesn't actually enable it).

**Root Cause**: The voice mode toggle detection (`detect_voice_mode_toggle`) was only in the text message path. The `handle_voice_message` path transcribed audio and sent it directly to the agent without checking for toggle commands.

**Fix** (commit `67b172f`): Added `detect_voice_mode_toggle` check in `handle_voice_message` after transcription, before sending to agent. If transcription matches a toggle pattern, toggle voice mode and return early.

---

## 2026-03-30: Unwanted Voice Replies to Voice Messages

**Symptom**: User sends voice message (without requesting voice mode), RustClaw replies with voice instead of text.

**Root Cause**: Two issues:
1. Old `send_response` method in `handle_voice_message` checked for `VOICE:` prefix in LLM output — LLM sometimes added it unprompted
2. System prompt told LLM about `VOICE:` prefix mechanism, encouraging it to use voice for voice message inputs

**Fix** (commit `67b172f`):
- Removed `VOICE:` prefix mechanism entirely
- Voice replies now controlled **only** by per-chat voice mode toggle state
- LLM has no way to force voice — it's a transport decision, not a content decision
- System prompt updated to explain voice mode is framework-controlled

**Design Principle**: LLM decides **content**, framework decides **transport format**.

---

## 2026-03-29: File Descriptor Leak

**Symptom**: fd count grows over time, eventually hitting OS limit.

**Root Cause**: `notify` crate with `macos_kqueue` backend opens 1 fd per watched file. Config watcher was watching entire workspace directory.

**Fix** (commit `ae0a4ef`):
1. Switched to `macos_fsevent` backend in `Cargo.toml`
2. Config watcher only watches the config file itself, not the workspace

**Post-fix**: fd count stable at 35.

---

## 2026-03-29: Engram FTS Index Corruption

**Symptom**: `engram consolidate` fails with "database disk image is malformed".

**Root Cause**: SQLite FTS5 index corruption (exact trigger unknown — possibly concurrent writes or unclean shutdown).

**Fix**: Rebuild FTS index:
```sql
INSERT INTO memories_fts(memories_fts) VALUES('rebuild');
```

**Note**: This has happened multiple times. Consider adding automatic FTS rebuild on startup if consolidate fails.

---

## 2026-03-29: block_in_place Panic

**Symptom**: Panic when OAuth token refresh is called from async context.

**Root Cause**: `runtime.block_on()` called inside an async task — Tokio doesn't allow nested block_on.

**Fix**: Wrapped in `tokio::task::block_in_place()` at `src/memory.rs:37`.
