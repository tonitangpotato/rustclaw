---
id: "ISS-029"
title: "Ritual state liveness signal — detect stuck/dead rituals"
status: closed
priority: P2
created: 2026-04-26
closed: 2026-04-28
component: "src/ritual.rs"
related: ["ISS-025"]
---
# ISS-029: Agent reads ritual state file mid-write, mistakes ongoing ritual for stuck

**Status:** closed
**Closed:** 2026-04-28

## Resolution

Shipped via two-task split (autopilot 2026-04-28):

- **ISS-029a** — gid-rs `a919d10` (add `phase_entered_at` + `last_heartbeat` to `V2State` with `#[serde(default)]` for back-compat) + `4e63c2a` (`RitualHealth { Healthy | LongRunning | Wedged | Terminal }` enum + `health(now)` method).
- **ISS-029b** — gid-rs `7ec0079` (heartbeat tick wired into `drive_event_loop` — single tick point covers Implementing/Reviewing/Verifying instead of per-phase tokio tasks; simpler than the original sidecar-file design and avoids state-file churn) + rustclaw `bfd2ac0` (health deserialization + `gid_ritual_status` rendering `- **Health**: <classification>` line, with serde defaults for forward/back compat).

Documentation: `RITUAL_RUNTIME.md` at rustclaw repo root (the `.gid/runtime/` and `docs/` paths were both gitignored).

Deviation from "Notes for the implementer":
- Original suggestion was tokio task per phase + `r-XXXX.heartbeat` sidecar file. Implementation instead uses the existing `drive_event_loop` event-loop iteration as the unified tick point. Reasoning: drive_event_loop already runs for every phase, so a single tick site is more robust than three coordinated per-phase tasks; and writing `last_heartbeat` directly to the state file (instead of a sidecar) is fine because event-loop iterations are coarse-grained — no churn observed.
- Wedged threshold: `now - last_heartbeat > 60s` (2× tick interval), per design.

Tests: gid-rs 71/71 v2_executor green; rustclaw `cargo test ritual_registry` green.

---

**Original status was:** open
**Severity:** low — diagnostic confusion only (no data loss), but caused agent to misreport ritual health to user and almost trigger an unnecessary cancel
**Filed:** 2026-04-26
**Reporter:** RustClaw (caught self misdiagnosing live ritual r-430839 as "stuck for 5 minutes" when it was actually progressing normally)
**Related:**
- rustclaw ISS-027 (ritual observer + LLM context injection — the broader fix)
- rustclaw ISS-019 (ritual cancel persistence — adjacent ritual lifecycle work)
- rustclaw ISS-028 (duplicate rituals — the underlying cause that made this confusing)

---

## Symptom (concrete incident, 2026-04-26)

Working with potato on something else (post-ISS-034 context), I checked `.gid/rituals/r-430839.json` and saw:
- `phase: Implementing`
- last transition `Graphing → Implementing` at `19:29:21`
- file mtime `15:34` (local) — i.e. apparently 5 minutes ago

I reported to potato: "ritual is stuck in Implementing for 5 minutes, FSM didn't transition to Verifying/Done. Looks like ISS-019 residual." Then proposed cancel options.

Re-read the file ~30 seconds later: ritual now showed `phase: Done` with full transition list including `Implementing → Verifying → Done` at `19:34:35`. **The ritual was never stuck.** I read the file during the implement phase (which legitimately takes 4-5 minutes for an 18k-token Opus call) and interpreted "no recent transition" as "stuck".

## Why I got it wrong (root causes)

1. **No "expected duration per phase" baseline.** I had no prior on how long `Implementing` should take. 5 minutes felt long; for a SingleLlm strategy with ~18k implement tokens it's actually normal. Without a baseline, "5 minutes since last transition" looks alarming when it's just business-as-usual LLM latency.

2. **Snapshot read of a live state file.** State files are atomically rewritten on each transition, but between transitions there's no in-progress signal — the file just sits there with the last completed transition timestamp. From outside, a healthy slow phase and a wedged FSM look identical.

3. **No liveness signal in the state file.** `RitualState` doesn't carry anything like `phase_started_at`, `last_heartbeat`, or `expected_completion`. A reader cannot distinguish:
   - phase started 4 minutes ago, LLM call still in progress, healthy
   - phase started 4 minutes ago, action panicked silently, wedged

4. **ISS-028 (duplicate rituals) confused things further.** There were *two* concurrent rituals on ISS-043 (r-429789 and r-430839). When I read r-430839 mid-Implementing, r-429789 had already finished. So the work was "done" from gid-rs's perspective (commit landed, clippy clean) while r-430839 was still in `Implementing`. That mismatch — "issue file says RESOLVED, ritual file says Implementing" — was the actual evidence that pushed me to "stuck" instead of "still working". Both rituals on the same work_unit broke my mental model of "one ritual = one piece of work".

## Why this matters

- **User-facing misreporting.** Same root as ISS-027 — agent describes ritual state confidently from a stale snapshot. Here the snapshot wasn't stale (it was current!), but it lacked the *progress* dimension needed to interpret it correctly.
- **Almost-cancelled live work.** I offered potato three options including "manually edit state file to cancelled". Had we gone that path while the ritual was in flight, we'd have created a real ISS-019-class corruption (cancelled state + still-running adapter).
- **False-positive bug reports.** I almost filed an issue titled "stuck ritual — ISS-019 residual path". It would have been a fake issue. With ISS-028 correctly identified as the real bug, this kind of false-flag becomes traceable.

## Goals (acceptance criteria)

- **GOAL-1:** `RitualState` carries a `phase_entered_at: DateTime<Utc>` field, updated on each transition. Distinct from `updated_at` (which can refresh for non-transition reasons like persistence rewrites).
- **GOAL-2:** Long-running phases (Implementing, Reviewing, Verifying) emit a periodic heartbeat by touching a `last_heartbeat: DateTime<Utc>` field at fixed cadence (e.g., every 30s) while the phase action is alive. This is a liveness probe — its absence proves wedged.
- **GOAL-3:** A reader-side helper `RitualState::health(now: DateTime<Utc>) -> RitualHealth` returns one of:
  - `Healthy { phase_age: Duration }` — phase progressing, last_heartbeat recent
  - `LongRunning { phase_age: Duration, expected_max: Duration }` — phase started long ago but still heart-beating
  - `Wedged { phase_age: Duration, last_heartbeat_age: Duration }` — heartbeat stale → likely crashed adapter
  - `Terminal { phase: Phase }` — Done/Cancelled/Failed
- **GOAL-4:** Agent-facing tool (e.g., `gid_ritual_status` or extension to existing one) reports `RitualHealth` not just raw phase. So agent says "ritual progressing through Implementing (4m12s, normal range)" instead of "phase=Implementing".
- **GOAL-5:** Document expected phase durations per strategy in `.gid/runtime/rituals/README.md` or similar — even rough numbers ("Implementing: 1-10min for SingleLlm, 5-30min for MultiAgent") give downstream readers (humans + agents) the prior they need.

## Non-goals

- Fancy progress-percentage UI. Heartbeat + phase-age is enough.
- Watching the adapter process directly via `kill(0, pid)`. Heartbeat-via-file is the right granularity (single source of truth = the state file). Process-level liveness can be added later if needed.

## Verification

1. Reproduce: start a ritual, read state file mid-Implementing (before fix). Manually compare to file 5 min later. Notice no progress signal.
2. After fix: read mid-Implementing. `last_heartbeat` is fresh (<30s old). `health()` returns `Healthy`. Reader correctly concludes "still working".
3. Negative test: kill adapter mid-phase. After 60s, `last_heartbeat` is stale. `health()` returns `Wedged`. Reader/agent now has signal to act.

## Notes for the implementer

- Heartbeat should be a tokio task spawned at phase entry, cancelled at phase exit. Don't do it inside the action's main loop — keeps action code clean.
- Persisting the state file on every heartbeat is overkill. Either (a) write `last_heartbeat` to a sidecar file (`r-XXXX.heartbeat`), or (b) batch heartbeats with state writes. Discuss before implementing.
- ISS-027's observer hook can consume `RitualHealth` directly — no new context-injection plumbing needed; just plumb `health()` through the existing observer.

---

**Discovered while filing:** ISS-028 (duplicate rituals). The duplicate-ritual confusion made me misread the live ritual as stuck. Filing this separately because the fix is different (heartbeat/liveness, not dedup) and the bug class generalizes — agents reading state files always need a "is this actually still alive?" signal.
