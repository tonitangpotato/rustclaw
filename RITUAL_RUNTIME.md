# Ritual Runtime State Files

Each `r-*.json` file in this directory is the on-disk state snapshot of a single
ritual run (gid-rs `RitualState`, persisted by `V2Executor`). Files are written
atomically via `save_state`, scanned at startup by `RitualRegistry`, and
reconciled for orphans (ISS-028b).

## Health classification (ISS-029)

`ActiveRitual::health` is computed at scan time from `phase_entered_at` and
`last_heartbeat` (both stamped into the state file by gid-core). The legacy
`updated_at` mtime is **not** sufficient — a wedged ritual can sit on a
stale file for hours without `updated_at` advancing, but `last_heartbeat`
ticks at every event-loop iteration inside `drive_event_loop`, giving real
liveness signal.

### Phase budgets (expected wall-clock duration)

These budgets are **soft**: exceeding them flips status to `LongRunning` for
operator visibility but does not interrupt the ritual. They reflect typical
end-to-end times observed during ISS-052 / ISS-029 development; rituals on
larger codebases or with retries can legitimately exceed them.

| Phase                | Expected ≤ | Notes |
|----------------------|-----------:|-------|
| `Designing`          |     180s   | Single-LLM design phase; multi-agent runs longer. |
| `UpdatingGraph`      |      60s   | Pure SQLite writes — anything >60s is suspicious. |
| `Implementing`       |     900s   | Skill-driven; skill turn limit dominates. |
| `Reviewing`          |     300s   | Self-review subloop; depends on review skill. |
| `Verifying`          |     600s   | `cargo check` + `cargo test` for the touched scope. |

Rendered budget in the registry's `classify_health` is currently a single
conservative `LONG_RUNNING_THRESHOLD_SECS = 300` — this under-flags
`LongRunning` for slow phases (Implementing/Verifying) rather than
over-flagging fast ones, which is the safe direction for a status display.
gid-core carries per-phase budgets internally; the registry deliberately
duplicates only the wedged threshold to avoid coupling to the full
`RitualState` schema.

### Wedged threshold

`WEDGED_THRESHOLD_SECS = 60`. A ritual is `Wedged` when `now -
last_heartbeat > 60s` and the phase is non-terminal.

This is **2× the natural tick rate** of `drive_event_loop`: each event-loop
iteration touches `last_heartbeat = Utc::now()` before dispatching actions
(see `v2_executor.rs` ISS-029b comment). 60s gives ~2 missed ticks before
flagging, which is generous enough to absorb a long-running single action
(e.g. a multi-second cargo build) without false positives, while still
catching truly-stuck rituals (process killed mid-action, deadlock in a
hook, file lock contention) within ~1 minute.

### Health states

- **Healthy** — `now - last_heartbeat ≤ 60s` and `now - phase_entered_at ≤
  budget`. The default state for a ritual making progress.
- **LongRunning** — `now - phase_entered_at > 300s` (single conservative
  budget; see table above for per-phase). Heartbeat is still fresh, so
  the ritual is doing real work — just more than expected.
- **Wedged** — `now - last_heartbeat > 60s` and phase is non-terminal.
  Operator should investigate: check the adapter PID, look for crashed
  subprocesses, or `gid_ritual_cancel` if appropriate.
- **Terminal** — phase ∈ {`Done`, `Failed`, `Cancelled`}. No tick
  expected; `health` short-circuits to terminal regardless of stamp
  ages.

## Tool surface

`gid_ritual_status` (in rustclaw) renders `ActiveRitual::health` as a
`- **Health**: <classification>` line in the per-ritual section. Older
state files written before ISS-029a lack `phase_entered_at` /
`last_heartbeat` — for those, `health` is `None` and the renderer
falls back to the legacy `suspected_stuck` heading marker (mtime-based).

## File lifecycle

- **Created** when `RitualRunner::start_with_work_unit` accepts a fresh
  request and `V2Executor::initial_save` stamps the first state.
- **Updated** by `V2Executor::save_state` after every state-machine
  transition (atomic write via temp file + rename).
- **Terminal** when phase enters `Done` / `Failed` / `Cancelled`.
  Files are **kept** (not deleted) — they form the ritual audit log.
- **Orphan reconciler** (ISS-028b, runs in `RitualRunner::new`) scans
  for duplicate terminal rituals targeting the same `work_unit.label()`
  and emits a WARN log; it does **not** delete anything.

## Manual operations

If you need to inspect or recover from a stuck ritual:

```bash
# List active (non-terminal) rituals
ls -lt .gid/runtime/rituals/r-*.json | head

# Inspect one
jq '{id, phase, work_unit_label, last_heartbeat, phase_entered_at}' \
  .gid/runtime/rituals/r-XXXXXX.json

# Force-cancel via the tool surface (preferred)
# (use rustclaw's gid_ritual_cancel tool — do NOT delete files manually)
```

Manual file deletion is **not supported** — the registry will rebuild
its in-memory view from disk on next scan, and the gid-rs side may have
in-flight handles to the same path.
