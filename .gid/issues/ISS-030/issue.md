---
id: "ISS-030"
title: "Multi-daemon shared workspace ritual race condition"
status: open
priority: P3
created: 2026-04-26
component: "src/ritual.rs, src/workspace.rs"
related: ["ISS-028"]
---
# ISS-030: Multi-daemon shared workspace causes ritual state race + broadcast confusion

**Status:** open
**Severity:** High (data corruption + user-visible confusion)
**Filed:** 2026-04-26
**Project:** rustclaw
**Reporter:** potato (observed broadcast confusion between rustclaw and rustclaw2)

---

## Summary

Multiple rustclaw daemons running against the **same workspace** (`/Users/potato/rustclaw/`) share the ritual state directory (`.gid/rituals/*.json`) but each daemon has its own Telegram bot and `notify` callback. This produces three concrete failure modes:

1. **State file write race** — FSM transitions are file writes with no lock; two daemons advancing the same ritual can clobber each other.
2. **Broadcast misrouting** — when daemon A advances a ritual, only daemon A's bot/chat sees the phase notification. The chat that *started* the ritual (if it was daemon B) sees nothing.
3. **Duplicate broadcast** — for events triggered identically on both daemons (timers, watchers), both bots fire notifications.

## Evidence

Observed concurrently running processes (2026-04-26 15:48):

```
PID    config                       bot_token (last 8)   workspace
34981  rustclaw.yaml                ...iSV_0xQ           /Users/potato/rustclaw
9083   rustclaw-marketing.yaml      (separate)           /Users/potato/rustclaw
9066   rustclaw-2.yaml              ...wilzfm8           /Users/potato/rustclaw
```

All three share `/Users/potato/rustclaw/.gid/rituals/` — 32 JSON files in one directory, no ownership metadata.

User-reported symptom: "ritual 播报在 rustclaw 和 rustclaw2 之间有点混乱" — phase notifications appearing on the wrong bot/chat or being missed entirely.

## Root cause

`src/ritual_runner.rs` (and the registry) treats the workspace's `.gid/rituals/` as if a single process owns it. There is:

- **No file lock** around state JSON read-modify-write
- **No ownership field** in the state file (no `daemon_id` / `bot_token_hash` / `originating_chat_id`)
- **No notification routing** — `notify` callback is per-process, with no way to know "this ritual was started by daemon B, send the broadcast there"

The dedup bug filed as **ISS-028** is a sibling of this issue but distinct: ISS-028 is about a single daemon double-creating rituals. ISS-030 is about multiple daemons stepping on one shared state directory.

## Failure modes (concrete scenarios)

**Scenario A — Lost progress notification:**
1. User on rustclaw2 chat: `/ritual ISS-X`
2. rustclaw2 creates `r-abc.json`, sends "🚀 starting" to rustclaw2 chat
3. rustclaw daemon (different process) sees the new file via its own watcher / next-tick scan, picks it up, advances to design phase, sends "📐 design phase" — **to rustclaw chat, not rustclaw2 chat**
4. User on rustclaw2 sees nothing further, thinks ritual hung

**Scenario B — Write clobber:**
1. Both daemons observe phase=Research is complete
2. Both compute next phase = Design and write JSON simultaneously
3. Whichever finishes second wins; the other's metadata (transition timestamp, observer notes) is lost

**Scenario C — Double-notify:**
1. Phase deadline timer fires on both daemons within the same second
2. Both bots send "⏰ phase X timed out" to their respective chats — user sees the same alert twice across two chats

## Proposed direction (not for this session — punt to fix session)

Three orthogonal options, can combine:

- **A. Daemon ownership** — add `owner_daemon_id` + `originating_chat` fields to ritual state. Only the owner advances/notifies; non-owners ignore the file entirely.
- **B. File locking** — use `fs2::FileExt::try_lock_exclusive` on the JSON during read-modify-write. Prevents Scenario B but not A or C.
- **C. Single-writer mode** — declare one daemon the "ritual coordinator" via config flag; others run read-only and never touch `.gid/rituals/`.

Recommended combo: **A + B**. C is simpler but requires user discipline.

## Out of scope for this issue

- Fixing single-daemon dedup → that's **ISS-028**
- Adding liveness signals → that's **ISS-029**
- Changing how multiple daemons are deployed (separate workspaces vs shared) — that's a config decision

## Related

- ISS-028: duplicate rituals (single-daemon dedup)
- ISS-029: ritual state liveness signal
- ISS-027: ritual observer context injection

## Notes

Filed by RustClaw session 2026-04-26 while focused on engram v3 work. Discovered when potato asked about broadcast confusion between rustclaw and rustclaw2. Not implementing — flagged for a dedicated rustclaw-fix session.
