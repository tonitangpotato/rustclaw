---
id: "ISS-050"
title: "Ritual silently wedges when save_state hits IO error (e.g. disk full)"
status: open
priority: P1
created: 2026-04-27
component: "crates/gid-core/src/ritual/v2_executor.rs (advance loop) + src/ritual_runner.rs (save_state)"
---
# ISS-050: Ritual silently wedges when save_state hits IO error (e.g. disk full)

**Status**: open
**Severity**: high (silent ritual hang, no user feedback, no retry, indistinguishable from "still working")
**Filed**: 2026-04-27
**Discovered while**: running combined ritual r-950ebf for ISS-028 + ISS-029 — ritual stalled on Verifying for 12+ minutes with zero subprocess activity

## Symptom

Ritual advances normally through Triage → Graph → Implement, then on Verifying:

1. `cargo build && cargo test` runs and **completes** (cargo test exits 101 — failures detected).
2. State machine receives `ShellFailed { exit_code: 101, stderr: "..." }` event.
3. State machine calls `save_state(&path, &new_state)` to persist the transition.
4. `save_state` returns `Err(io::Error { kind: StorageFull, code: 28 })` — disk is 100% full.
5. **Nothing recovers from this error.** No retry. No fallback path. No panic. No user notification.
6. Ritual phase remains `Verifying` on disk (old state). agent2 daemon process stays alive but completely idle (no children, no further log lines).
7. From the user's perspective: ritual is "running" but has been stalled indefinitely. Cannot be distinguished from a healthy long-running phase without log inspection.

## Forensic Evidence (r-950ebf)

From `/Users/potato/.rustclaw/logs/rustclaw2.err`:

```
2026-04-27T01:43:59.136523Z  INFO Advancing ritual ... event=ShellFailed { exit_code: 101, stderr: "FAILED at step: test" }
2026-04-27T01:43:59.136584Z  ERROR Advance after action failed: No space left on device (os error 28)
[12+ minutes of silence — no further log entries for this ritual]
```

State on disk at time of investigation (21:55+, ~12min after the error):

```json
{
  "id": "r-950ebf",
  "phase": "Verifying",   // ← stale; should be Verifying-failed/retrying or Escalated
  "phase_tokens": { "implement": 13569, "graph": 3308, "triage": 1260, "planning": 603 },
  "verify_retries": 0     // ← 0, not incremented; retry path never entered
}
```

`pgrep -P 37314` (agent2 PID) → **no children**. cargo, sh, rustc all gone. Process is alive but completely idle.

`df -h /Users/potato` at time of failure: `/dev/disk3s5 228Gi 228Gi 129Mi 100%` — confirmed disk-full.

## Expected behavior

When `save_state` fails on a state transition, the system MUST do **at least one** of:

1. **Retry with backoff** — IO errors are often transient (especially during load spikes). Try N times before giving up.
2. **Fallback location** — if primary state path is unwritable, write to `/tmp/gid-rituals-fallback/` so state isn't lost.
3. **Promote to Escalated** — if persistence fails irrecoverably, transition the ritual to `Escalated` (terminal) so user is notified and ritual doesn't appear to be running.
4. **Surface to user** — emit a Telegram / channel message: "Ritual r-XXXX failed to persist state: disk full. Manual intervention required."

The current behavior (log ERROR and return without action) is the **worst possible** outcome: ritual appears alive, no error reaches the user, no recovery happens, no resources are released.

## Root cause

Code path (verified 2026-04-26 against `crates/gid-core/src/ritual/v2_executor.rs`):

- `execute_actions` → calls `transition()` → produces new state + actions
- New state is persisted via `save_state(&path, &state)?`
- The `?` operator propagates the error up to the caller (`advance`)
- `advance` logs `ERROR Advance after action failed: ...` and returns `Err(...)`
- **No caller of `advance` handles this error in any meaningful way.** The orchestrator just drops the future. The state file on disk is never updated. The phase remains stuck in whatever state was last successfully persisted.

There is also a related issue in `run_shell` (line 595, `crates/gid-core/src/ritual/v2_executor.rs`):

```rust
match tokio::process::Command::new("sh")
    .arg("-c")
    .arg(command)
    .output()         // ← no timeout wrapper
    .await
```

Even though this is NOT the failure mode for r-950ebf (cargo test did complete), it's the same class of bug — a long-running verify command (e.g. cargo build with deadlocked lock files) would hang indefinitely. **Tracked separately** but should likely be addressed in the same fix pass.

## Reproduction

1. Fill disk to within ~50MB of full.
2. Start any ritual that reaches Verifying.
3. Verify fails or succeeds — doesn't matter; what matters is the state-persistence step.
4. Observe: ritual phase stuck on Verifying, no Telegram notification, no retry, agent process idle.

## Fix proposal (rough — design phase will refine)

Add a small persistence wrapper:

```rust
async fn save_state_with_recovery(
    path: &Path,
    state: &RitualState,
) -> Result<(), PersistenceError> {
    // Try primary path with 3 retries (50ms, 200ms, 1s backoff)
    for attempt in 0..3 {
        match save_state(path, state).await {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 2 => {
                warn!(?e, attempt, "save_state failed, retrying");
                tokio::time::sleep(BACKOFF[attempt]).await;
            }
            Err(e) => {
                // Fallback: write to /tmp so state isn't lost
                let fallback = fallback_path(path);
                if save_state(&fallback, state).await.is_ok() {
                    error!(primary=?path, fallback=?fallback, original_err=?e,
                           "Primary save_state failed, wrote fallback. MANUAL recovery needed.");
                    notify_user_critical("Ritual state persistence degraded: see logs").await;
                    return Err(PersistenceError::FallbackUsed);
                }
                // Both failed → escalate the ritual itself
                return Err(PersistenceError::Unrecoverable(e));
            }
        }
    }
    unreachable!()
}
```

Also: `advance`'s caller in `ritual_runner.rs` must handle `PersistenceError::Unrecoverable` by force-transitioning the in-memory state to `Escalated` and emitting a user-visible notification (Telegram message).

## Relationship to other issues

- **ISS-029 (ritual liveness signal)**: this issue is a *concrete instance* of the wedged-ritual class that ISS-029 wants to detect. ISS-029's heartbeat mechanism would have flagged r-950ebf as `Wedged` after 60s of idle — but ISS-050 is the **root cause** of the wedge, not just observability of it. **Fix both**: ISS-050 prevents this specific wedge, ISS-029 catches future wedges from other causes.
- **ISS-038 (file_snapshot post-condition)**: post-conditions correctly fired during r-950ebf's Implement phase (artifacts: []) but the phase transition still proceeded — separate bug, tracked in ISS-051.

## Verification

- Unit test: simulate `save_state` returning `io::Error::StorageFull` → assert ritual transitions to `Escalated` within 1.5s (allowing for retries).
- Unit test: simulate transient IO error on first attempt → assert ritual succeeds on retry without escalation.
- Unit test: simulate primary failure + fallback success → assert state is written to fallback path AND user notification is emitted AND ritual continues.
- Manual: artificially fill disk (`dd` a large file) before triggering verify → confirm Telegram notification appears and ritual moves to Escalated within 5s.
