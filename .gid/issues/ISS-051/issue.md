---
id: "ISS-051"
title: "rustclaw ritual_runner bypasses gid-core v2_executor → file_snapshot post-condition never runs"
status: open
priority: P0
created: 2026-04-27
component: "src/ritual_runner.rs (run_skill, line ~1041) + crates/gid-core/src/ritual/v2_executor.rs (run_skill, line ~485)"
---
# ISS-051: rustclaw ritual_runner bypasses gid-core v2_executor → file_snapshot post-condition never runs

**Status**: open
**Severity**: critical (defeats ISS-025 / ISS-038 quality gate; implement phase can claim success with zero file changes)
**Filed**: 2026-04-27
**Discovered while**: forensics on r-950ebf — implement phase emitted `SkillCompleted { artifacts: [] }` after 13.5k tokens with no expected diff check, no file changes, and 2 self-review rounds claiming "issues found" then silently passing

## Symptom

In a rustclaw ritual cycle, the implement phase completes "successfully" (transitions Implementing → Verifying) even when:

- The LLM made zero `Write` / `Edit` tool calls (or only made calls that no-op'd).
- `git diff` shows zero changes in the target tree.
- `artifacts_created` from the skill result is `[]`.

This is **exactly the bug ISS-025 was filed to prevent** and **ISS-038 was filed to enforce the fix for**. The post-condition machinery exists, has unit tests, is wired into `gid-core/v2_executor.rs:535-575`. It just **never executes** during a rustclaw ritual.

## Forensic Evidence (r-950ebf)

From `/Users/potato/.rustclaw/logs/rustclaw2.err`:

```
01:39:38  Skill 'implement' completed: 21 tool calls, 2998 tokens
01:39:38  Implement self-review round 1/4
01:41:36  (round 1 hit 20-turn limit) Implement self-review round 2/4
01:43:24  SkillCompleted { phase: "implement", artifacts: [] }
          Advancing ritual ... from_phase=Implement event=SkillCompleted
```

Critically absent from the log:
- **No `"Phase file-change diff"` info!() line** (would be emitted by v2_executor.rs:541 if post-condition ran).
- **No `"implement phase produced no file changes"` warn!() line** (would be emitted at v2_executor.rs:553 if diff was empty).

Confirmed via `git status` on `/Users/potato/rustclaw` and `/Users/potato/clawd/projects/gid-rs` immediately after cancel: zero modifications attributable to r-950ebf's implement phase. The 2 self-review rounds each said "found issues" but never produced REVIEW_PASS, ran out of turns, and the loop exited the implement phase as `SkillCompleted` regardless.

## Root cause

There are **two parallel implementations of `run_skill`**, and rustclaw uses the wrong one:

1. **`gid-core/src/ritual/v2_executor.rs::V2Executor::run_skill`** (line ~485)
   - Has `phase_requires_file_changes(name)` check
   - Snapshots `mutation_root` before the skill runs
   - Diffs after the skill runs
   - Returns `SkillFailed` if `name == "implement" && diff.is_empty()`
   - Returns `SkillCompleted { artifacts: artifact_strings(&diff) }` with **real artifacts derived from the diff**, not from the LLM's self-report

2. **`rustclaw/src/ritual_runner.rs::RitualRunner::run_skill`** (line 1041)
   - Calls `gid_client.run_skill(...)` directly — this is the **api_llm_client**, not the V2Executor
   - Has its own self-review loop after the skill returns
   - On completion, emits `SkillCompleted { artifacts: skill_result.artifacts_created.iter()... }` — but `artifacts_created` is **always `vec![]`** (per the comment in `api_llm_client.rs`: "tracking was deferred to the engine layer")
   - **Never calls** `phase_requires_file_changes`, `snapshot_dir`, or `diff_snapshots`
   - **Never imports** `file_snapshot` module

So the chain is:
- `gid-core` has the post-condition (correct, tested)
- `rustclaw` does not call `gid-core::V2Executor` at all
- `rustclaw` calls `gid_client.run_skill` directly and trusts the empty artifacts vec

The file_snapshot post-condition is **dead code** as far as rustclaw rituals are concerned. Every rustclaw ritual since ISS-038 was "fixed" has been running without this gate.

## Why the self-review loop didn't catch it

The self-review loop (ritual_runner.rs:1281-1351) is a separate mitigation but has its own flaws relevant here:

1. It asks the LLM to review files it "created or modified" — but if the LLM didn't actually modify anything, the review-of-nothing is vacuously fine.
2. The loop only `break`s on `REVIEW_PASS`. If the LLM hits 20-turn limit each round, it exits the round without producing REVIEW_PASS, and the **outer loop continues to the next round**. After max rounds (4), it falls through to `SkillCompleted` regardless of whether any REVIEW_PASS was ever emitted.
3. In r-950ebf, log shows rounds 1 and 2 both hit 20-turn limit (no REVIEW_PASS), and the ritual still proceeded to Verifying as if implement succeeded.

So even the secondary safety net is broken: a hung review loop is not distinguished from a passed review loop at the phase boundary.

## Why this wasn't caught by tests

- gid-core's `phase_requires_file_changes_only_implement` test (v2_executor.rs:1936) verifies the function works.
- gid-core's `non_implement_phase_does_not_enforce_file_change_postcondition` test (v2_executor.rs:1911) verifies non-implement phases skip it.
- **No integration test covers the rustclaw → gid-core handoff.** The two crates are tested independently, and the rustclaw side never exercises a path that would have surfaced the missing post-condition.

## Fix proposal

**Option A — Remove the duplication (preferred, root fix):**
Make rustclaw's `RitualRunner::run_skill` call `gid_core::V2Executor::run_skill` instead of going to `gid_client` directly. The V2Executor already does:
- LLM invocation
- File snapshot before/after
- Diff post-condition
- Returns the right `RitualEvent`

This collapses the two parallel implementations into one. The self-review loop (currently in rustclaw) should also move to V2Executor, because it's part of the implement-phase contract, not channel-specific.

**Option B — Mirror the post-condition in rustclaw (patch fix):**
Copy `snapshot_dir` + `diff_snapshots` calls into `ritual_runner.rs::run_skill`. Faster to land, but creates two places that must stay in sync. Future drift is guaranteed.

**Recommendation: Option A.** It removes the architectural duplication that allowed this bug to exist in the first place. ISS-025/ISS-038 work should not have stopped at gid-core — it should have included migrating rustclaw to call V2Executor.

## Self-review loop fixes (companion to either option)

Whichever option above is chosen, the self-review loop also needs hardening:

1. **Track whether ANY round produced REVIEW_PASS.** If the loop exits without a single REVIEW_PASS (because all rounds hit turn limit or errored), that should be treated as a non-passing review and surfaced — not silently swallowed.
2. **Detect "review of nothing":** if the diff at end of implement is empty, emit SkillFailed unconditionally. Don't even start review rounds.
3. **Reduce review rounds to 2 by default** (currently 4) — with a real post-condition gate, 4 rounds is overkill and wastes Opus tokens (~10k per round).

## Reproduction

1. Build current rustclaw release (or attach to running daemon).
2. Start any ritual with an implement phase whose prompt is too narrow to produce changes (e.g. ask LLM to "review and improve" without write tools, or with malformed file paths).
3. Observe: implement phase completes "successfully", artifacts: [], no error, no file change.

## Verification

- **Integration test**: end-to-end ritual run where the LLM is mocked to return success with zero tool calls. Assert ritual ends in `Escalated` (or equivalent failure phase), not `Done`.
- **Manual**: re-run a ritual after the fix, verify that the "Phase file-change diff" log line appears for implement, and that an empty diff produces SkillFailed.
- **Audit**: grep all `SkillCompleted { artifacts: [] }` log lines from past 30 days. Each is potentially a false success that needs investigation. (Past damage is bounded — the LLM did real work for ISS-038, ISS-029, etc., even if the post-condition didn't enforce it; but at least one ritual we know of, r-950ebf, was a complete no-op.)

## Relationship to other issues

- **ISS-025** (gid-rs): filed the original bug "implement runs end-to-end producing zero file changes." The fix landed in gid-core but didn't propagate to rustclaw. ISS-051 is the rustclaw-side completion of ISS-025.
- **ISS-038** (gid-rs): added file_snapshot post-condition. Same situation — fix in gid-core, not propagated.
- **ISS-050** (rustclaw): silent ritual wedge on save_state IO error. Independent bug; both happen to manifest in r-950ebf but have no shared cause.
- **ISS-029** (rustclaw): liveness signal for in-flight rituals. Would help users notice ISS-051 happening (long phase with no progress) but doesn't fix the root cause.
