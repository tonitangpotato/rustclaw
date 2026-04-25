# ISS-019: `/ritual cancel` Does Not Persist Cancellation to State File

**Created**: 2026-04-22
**Resolved**: 2026-04-25
**Priority**: Medium
**Status**: ✅ Resolved — 3-part root fix landed (commits b10c9f9, 6e5782b + gid-rs schema)
**Related**: ISS-016 (main agent ritual awareness — explicitly deferred this fix), ISS-025 (separate no-op-implementation symptom)

---

## Resolution

Three-part root fix:

1. **Part 1 — schema** (gid-rs): Added `status: Option<RitualStatus>` field to `RitualState`. `Active | Cancelled | Done | Failed`. The FSM transition arm `(_, UserCancel)` now sets `status = Some(Cancelled)` and `phase = Cancelled`. Status defaults to `None` on legacy files for backward compat (deserialized as Active by callers).

2. **Part 2 — cancel handler rewire** (rustclaw `b10c9f9`): `/ritual cancel` no longer trusts the spawned EP-action task to call `advance(UserCancel)` itself. It fires the cancellation token (interrupts in-flight work) and then *unconditionally* drives the FSM via `send_event_to(id, UserCancel)`. Single authoritative path; both running and paused branches converge on the same FSM transition. Idempotent.

3. **Part 3 — orphan sweep** (rustclaw `6e5782b`): Daemon startup walks `.gid/rituals/*.json` and rewrites zombies (non-terminal phase + dead `adapter_pid`, OR no pid + stale >24h) as `Cancelled` via `transition(state, UserCancel)`. Result is byte-identical to a user-driven cancel. Dead PID preserved on disk for forensics. 16 unit tests cover dead/live pid, terminal-skip, idempotency, legacy-no-status, no-pid stale, corrupt JSON.

Validated against real zombie `.gid/rituals/r-e4e1f7.json` (Implementing, pid 49668 dead, status field absent) — fixture-perfect match with sweep tests; will be cleaned on next daemon restart.

Tests: 336/336 rustclaw + 48/48 gid-core state machine.

---

## Problem

When a user cancels a ritual (via `/ritual cancel <id>` or equivalent surface), the runtime stops the adapter/worker but **does not write a terminal transition back to the ritual state file**. The state file keeps its last in-flight `phase` (e.g. `Graphing`, `Implementing`) indefinitely, with no `Cancelled`/`Aborted` entry in `transitions[]` and no `status` field change.

From the filesystem's point of view, such a ritual is indistinguishable from one that is still running — it becomes a **zombie state file**.

### Symptoms observed

**2026-04-22 — ISS-024 ritual `r-e9410e` in `/Users/potato/clawd/projects/engram-ai-rust/`:**

- User cancelled the ritual earlier in the day via Telegram in another session.
- State file `.gid/rituals/r-e9410e.json` shows:
  - `phase: "Graphing"`
  - `status: null`
  - `transitions`: `Initializing → Triaging → Planning → Graphing` (no `Cancelled`)
  - `updated_at`: 5+ hours old, no further writes
- No adapter/worker process is alive for this ritual.
- User, main agent, and other surfaces all read this as "ritual still in Graphing phase" and draw wrong conclusions:
  - Main agent initially suspected the ritual was "stuck" and blocking new rituals.
  - ISS-016's active-ritual injection would surface this as a live ritual in the system prompt.
  - Any future "is there an active ritual?" check returns a false positive.

### Why this is a root problem

ISS-016 explicitly acknowledged this gap and deferred it:

> `/ritual cancel` and in-loop ritual actions do not call `invalidate()` directly. The 5s TTL catches them. Not worth wiring through the state machine until/unless it becomes a visible problem.

It has now become a visible problem:

1. Zombie state files **pollute the active-ritual view** that ISS-016 is specifically designed to provide. The whole point of ISS-016 is to give the main agent accurate situational awareness — a zombie invalidates that.
2. Humans (and agents) reading state files directly as ground truth are misled.
3. TTL-based cache invalidation (ISS-016's workaround) only affects in-process caches — it does nothing for the persisted file, which is shared across processes, sessions, and days.

The cancellation path must persist a terminal transition, same way successful completion does.

---

## Scope

### In scope

1. **When a ritual is cancelled, write a terminal transition to its state file before tearing down the worker.**
   - `transitions[]` gets a new entry: `{from: <current phase>, to: "Cancelled", event: "user_cancel" | "adapter_shutdown" | ..., timestamp: <now>, reason: <optional>}`
   - `phase` transitions to `Cancelled`
   - `status` is set (e.g. `"cancelled"`) so downstream readers have a single field to check
   - `updated_at` reflects the cancel time
2. **Identify all code paths that end a ritual without successful completion**, not just the `/ritual cancel` command:
   - Explicit user cancel (Telegram / CLI / TUI)
   - Adapter shutdown mid-ritual (daemon restart, SIGTERM)
   - Parent-process death / orphaned ritual
   - Fatal error in adapter loop that isn't retried
3. **Ensure idempotency**: writing `Cancelled` on an already-`Cancelled` or already-`Done` state is a no-op, not a corruption.

### Out of scope

- Resuming a cancelled ritual (that is a separate design question).
- GC / cleanup of old terminal state files (separate operational concern).
- Retroactively fixing existing zombie files (one-off cleanup — handle manually as discovered).

---

## Design

### Where the fix lives

`/ritual cancel` is handled by the ritual runtime in RustClaw (the same module that drives the state machine). The fix is one step inside the cancel path:

```
cancel_ritual(id) {
    1. Load state file
    2. If phase is already terminal (Cancelled/Done/Failed) → return idempotent ok
    3. Append transition { from: current_phase, to: "Cancelled", event: "user_cancel", timestamp: now }
    4. Set phase = "Cancelled", status = "cancelled", updated_at = now
    5. fsync(state_file)           ← critical, must persist before step 6
    6. Signal adapter/worker to stop (existing logic)
    7. Invalidate in-process caches (ISS-016 RitualRegistry)
}
```

The ordering matters: **persist before killing the worker**. If we kill first and crash between, we reproduce the current zombie.

### Adapter-shutdown path

When the daemon shuts down with an in-flight ritual, the adapter should attempt the same transition before exiting:

- On clean shutdown (SIGTERM with grace period): write `Cancelled` with `event: "adapter_shutdown"`.
- On crash / SIGKILL: impossible to write from the dying process. A separate **startup sweep** on next daemon boot scans `.gid/rituals/*.json` and for any non-terminal state whose `adapter_pid` is no longer alive, writes `Cancelled` with `event: "orphaned"`.

### Schema additions

Add a dedicated `status` field at the top level of the state file (currently `null` in active files, never set to terminal values). Agreed values:

- `null` — in flight
- `"done"` — successful completion
- `"cancelled"` — user or system cancellation
- `"failed"` — terminal failure (distinct from suspected-stuck)

This gives readers a single field to branch on without inspecting the transitions array.

---

## Verification

1. **Happy path**: start a ritual, `/ritual cancel <id>`, inspect state file — must have `phase: "Cancelled"`, `status: "cancelled"`, a transition entry with `to: "Cancelled"`.
2. **Idempotency**: cancel twice — second call is a no-op, no duplicate transition, no error.
3. **Adapter crash simulation**: start ritual, SIGKILL the adapter, restart daemon — startup sweep marks the ritual `Cancelled` with `event: "orphaned"`.
4. **ISS-016 interaction**: after cancel, `RitualRegistry` must not report the ritual as active (verify TTL + file state combine correctly).
5. **Regression**: existing "ritual completes successfully" path still writes `Done` transition and `status: "done"`.

---

## References

- ISS-016 (`.gid/issues/ISS-016/ISSUE.md`) — main agent ritual awareness, explicitly deferred this fix.
- ISS-024 ritual `r-e9410e` — concrete zombie instance in `/Users/potato/clawd/projects/engram-ai-rust/.gid/rituals/` (do not delete without user confirmation; it's a reproducer).
- Today's observation (2026-04-22): two ISS-022 rituals running in parallel; the duplicate `r-5ff35a` would need `/ritual cancel` — and without this fix, cancelling it would produce another zombie.
