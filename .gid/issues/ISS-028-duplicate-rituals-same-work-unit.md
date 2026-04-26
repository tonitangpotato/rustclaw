# ISS-028: Duplicate rituals on the same work_unit run in parallel, burn redundant tokens, risk divergent edits

**Status:** open
**Severity:** high — wastes ~18k tokens per duplicate and enables silent edit races on the target repo
**Filed:** 2026-04-26
**Reporter:** RustClaw (forensic audit of `.gid/rituals/` after ISS-043 work)
**Related:**
- rustclaw ISS-019 (ritual cancel persistence — adjacent ritual-lifecycle bug)
- rustclaw ISS-026 (start_ritual tool misreports progress)
- rustclaw ISS-022 (start_ritual now requires structured WorkUnit — gives us the dedup key for free)

---

## Symptom (concrete incidents, all on this Mac)

Forensic scan of `/Users/potato/rustclaw/.gid/rituals/` for ritual JSON files grouped by `work_unit`:

```
gid-rs:ISS-043:
  r-429789.json  started 2026-04-26T19:27:12Z  phase=Done   (tokens: implement=18884)
  r-430839.json  started 2026-04-26T19:27:41Z  phase=Done   (tokens: implement=18286)
  → second ritual started 29s after the first; both reached Done within ~1 min of each other.

gid-rs:ISS-039:
  r-d893f3.json  started 2026-04-25T22:52:55Z  phase=Done
  r-d97bb3.json  started 2026-04-25T22:53:54Z  phase=Done
  → second ritual started 59s after the first; both Done.

rustclaw:ISS-021:
  r-6ee931.json  started 2026-04-23T17:41:41Z  phase=Done
  r-e4e1f7.json  started 2026-04-23T15:10:55Z  phase=Cancelled
  → only this one self-corrected (one cancelled), but only because a human (or the agent) noticed.
```

For ISS-043 specifically, **both rituals ran the full SingleLlm pipeline through `Implementing` (~18k tokens each)** and reached `Done`. Only one git commit (`08d0d08`) exists on the gid-rs side — meaning at least one ritual's edits were either:
- silently overwritten by the other,
- redundantly recomputed (same fix proposed twice, second one was a no-op edit),
- or lost entirely.

We cannot tell which from the state files alone. That ambiguity is itself the bug.

## Why this matters

1. **Token waste.** Each duplicate = ~20k tokens × Opus pricing. ISS-043 alone burned ~$1 of redundant LLM calls. Across the three confirmed incidents we've already seen, this is real money for zero additional work product.
2. **Edit race risk.** Two rituals editing the same files in the same repo with no coordination is a classic race. Today we got lucky (same intended fix → same diff). A more open-ended task (refactor, multi-file feature) would diverge — one ritual's commit overwrites the other's, or worse, a partial interleaving.
3. **State observability is broken.** When potato or the agent runs `gid_tasks`/queries the work_unit, the answer becomes ambiguous: "which ritual is the source of truth for ISS-043?" The newer one? The older one? The one that committed?
4. **Confuses post-hoc debugging.** When investigating "did this issue actually get fixed?", finding two ritual records for the same work_unit forces a manual reconcile every time.

## Root cause

`src/ritual_runner.rs::start_with_work_unit` (rustclaw lines ~269-296) creates a fresh `RitualState` and persists it without **any check for an existing active ritual on the same `work_unit`**:

```rust
let state = RitualState::new().with_work_unit(unit.clone(), resolved_root.clone());
self.save_state(&state)?;
self.advance(&state.id, RitualEvent::Start { task }).await
```

There is no `find_active_ritual_for_work_unit(&unit)` call before this. Compare to the natural shape:

```rust
if let Some(existing) = self.find_active_ritual_for_work_unit(&unit)? {
    return Err(RitualConflict::AlreadyActive {
        ritual_id: existing.id,
        phase: existing.phase,
        started_at: existing.started_at,
    });
}
```

The check is straightforward — `RitualState` already carries `work_unit: Option<WorkUnit>` (post-ISS-022) and the runner can scan `.gid/rituals/*.json` for non-terminal `phase` matching `unit.label()`.

## Why this happened (interaction of fixed bugs)

Two recent fixes set up this regression:

1. **ISS-022** migrated `start_ritual` to take a structured `WorkUnit`. Before that, work-unit identity lived in free-text task descriptions and dedup was practically impossible. Now we *have* the key but never check it.
2. **ISS-019** fixed cancel-persistence and added orphan sweep. That fix made cancelled rituals reliably terminal, which means the `find_active` check below is now actually safe to write — it can trust `phase ∈ {Done, Cancelled, Failed}` as terminal.

So this issue has only become *cleanly fixable* in the last few days. We weren't structurally able to dedup before ISS-022.

## The 30-second-window pattern

All three confirmed incidents have the second ritual starting within **30-60 seconds** of the first. This strongly suggests the trigger is one of:

- **Telegram message retry** — user sent same `start_ritual`-triggering message twice (e.g., 网络 hiccup, no ack received, retried).
- **Agent self-retry** — agent called `start_ritual` tool, got no immediate response, called it again.
- **Two concurrent agents** — *ruled out for ISS-043*. Verified after filing: both r-429789 and r-430839 have `adapter_pid=9066` (rustclaw-2 daemon). Same daemon fired `start_ritual` twice. So the trigger is most likely the agent itself invoking the tool twice within the same conversation, or an LLM-side parallel-tool-use block that included two `start_ritual` calls.

(For ISS-039 and ISS-021 — adapter_pids not re-verified at filing time. They may share or differ; orphan reconciler in GOAL-5 should record this.)

The 30-second window is the human-or-agent reaction time. With dedup at the runner level, all three trigger paths become harmless — second call returns `AlreadyActive { ritual_id, phase }` and the user sees "already running, phase=Implementing" instead of a silent second pipeline.

## Goals (acceptance criteria)

- **GOAL-1:** `RitualRunner::start_with_work_unit` returns a structured `RitualConflict::AlreadyActive` error if a non-terminal ritual exists with the same `work_unit.label()`. No new ritual is created or persisted.
- **GOAL-2:** "Non-terminal" is defined as `phase ∉ {Done, Cancelled, Failed}`. Use the existing terminal-phase check (whatever ISS-019 settled on) — do not introduce a parallel definition.
- **GOAL-3:** The error surfaces to the agent via `StartRitualTool` as a clear tool result like `"ritual already active for gid-rs:ISS-043: ritual_id=r-429789, phase=Implementing, started=…"`. Agent can then decide to cancel-and-restart, wait, or report to user.
- **GOAL-4:** Add a unit test in `src/ritual_runner.rs` covering: (a) start succeeds when no active ritual on the work_unit exists, (b) second start returns `AlreadyActive` while first is non-terminal, (c) second start succeeds *after* first reaches `Done`/`Cancelled`/`Failed`.
- **GOAL-5:** Add a one-time orphan reconciler at runner startup that detects historical duplicates (same work_unit, both terminal) and logs them at WARN level for human review. Do not auto-delete — just surface the data so we can audit. (This catches the three already-existing incidents.)

## Non-goals

- Cross-process locking (file lock or DB row lock). The single-runner-per-rustclaw-instance assumption is fine; the cross-instance duplicate problem (rustclaw + rustclaw-2 both running) is solved at the *registry* layer, not here. If we later run multi-instance, revisit.
- Detecting duplicate *commits* on the gid-rs side. That's a different problem (git-level race) and out of scope.
- Auto-cancelling the older ritual when the newer one starts. Default to "fail loud, let user decide" — auto-cancel surprises people.

## Verification

1. Reproduce: trigger `start_ritual` twice in <60s with same WorkUnit. Before fix: two ritual files materialize. After fix: second call returns `AlreadyActive` and only one ritual file exists.
2. Run the new unit test suite — must pass.
3. Run rustclaw test suite — must remain green (140+ tests as of last count, up to 281 per MEMORY.md).
4. Manual check on a real workflow: ask agent to start a ritual for the same issue twice; second one should be rejected cleanly.

## Notes for the implementer

- The right place to add `find_active_ritual_for_work_unit` is `src/ritual_runner.rs` near `load_state*` helpers — it's a sibling concern (scan + filter ritual state files).
- `WorkUnit::label()` already exists and is the canonical comparison key — use it, don't re-derive.
- Be careful with file-system iteration order; do **not** depend on dirent order. Sort by `started_at` if multiple matches found (shouldn't happen post-fix, but the orphan reconciler in GOAL-5 will).
- The error type should be a real enum variant on `RitualError` or similar, not a stringly-typed `anyhow!` — downstream agent UX is much better when it can pattern-match.

---

**Discovered while investigating:** `/Users/potato/rustclaw/.gid/rituals/r-430839.json` for ISS-043. Initially looked like a stuck ritual (5-min gap between `Implementing` and the next transition). On re-read, ritual had already reached `Done`. The "stuck" appearance was actually two rituals running in parallel — the file I was reading hadn't been updated yet because *the other ritual was still writing to the same work_unit*. That confusion is itself a symptom of GOAL-3 above.
