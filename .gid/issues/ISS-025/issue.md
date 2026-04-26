---
id: "ISS-025"
title: "Ritual implement phase burns tokens with no output"
status: open
priority: P1
created: 2026-04-24
component: "src/ritual.rs (implement phase)"
related: ["ISS-027", "ISS-029"]
---
# ISS-025: Ritual `Implementing` Phase Burns Tokens But Produces No File Changes

**Created**: 2026-04-25
**Priority**: High
**Status**: **Resolved 2026-04-25** — fix landed in gid-rs `52a84ee` (see Resolution below)
**Resolved by**: gid-rs ISS-038 (file_snapshot post-condition)

---

## Problem

A ritual ran end-to-end (`Idle → ... → Done`, 11 transitions) and recorded substantial token spend in `phase_tokens`, but **zero files were modified** in the working tree of either the host project (rustclaw) or the cross-crate target (gid-rs).

The state machine reports success. The file system reports nothing happened.

This is a more dangerous failure mode than ISS-019 (zombie state files): there, an external action stopped the ritual and the state file lied about it. Here, the ritual *thinks it succeeded* but did no work.

---

## Reproducer

**Ritual**: `r-af714a` — `/Users/potato/rustclaw/.gid/runtime/rituals/r-af714a.json`

**Task**: implement ISS-019 root fix (cancel persistence + status field schema change).

**Started**: 2026-04-25 ~17:25 UTC
**Completed (state-machine-wise)**: 2026-04-25 17:42:16 UTC

**Final state**:
- `phase`: `"Done"`
- `status`: `null` ← (also a separate symptom — exactly the schema gap ISS-019 is trying to fix)
- `transitions`: 11 (full happy path including 2× `Reviewing → WaitingApproval` cycles)
- `phase_tokens`:
  - `triage`: 1,009
  - `planning`: 700
  - `graph`: 3,455
  - `implement`: **19,026**
  - `reviewing`: 3,963
- `triage_size`: `large`
- `strategy`: `SingleLlm`
- `verify_retries`: 0
- `phase_retries`: `{}`
- `failed_phase`: `None`
- `error_context`: `"triage: size=large, skip_design=true, skip_graph=false"`

**Working tree after completion** (verified via `find` with `-newer` ritual state file):
- `/Users/potato/rustclaw`: only `.gid/graph.yml` and `memory/2026-04-25.md` modified, neither related to ISS-019.
- `/Users/potato/clawd/projects/gid-rs` (where the actual code lives — `crates/gid-core/src/ritual/`): zero source files modified.
- `.gid/issues/ISS-019-ritual-cancel-does-not-persist/ISSUE.md`: not modified.

19k tokens were spent in `Implementing` and 4k in `Reviewing`, yet no diff was produced anywhere.

---

## Hypotheses

In rough order of likelihood:

### a) Implement phase ran in a sandbox / worktree that was never merged back

The ritual subsystem may be writing to a temporary worktree (e.g. `.gid/runtime/sandbox/<ritual-id>/`) and relying on a later step to merge changes back to the host repo. If that merge step is missing, no-op, or silently failed, work disappears.

**Check**: `find .gid/runtime -type f -newer <ritual-state-file>` and inspect any sandbox directories.

### b) Sub-agent ran but only "researched" — no enforcement that it actually wrote files

If the implement phase delegates to an LLM sub-agent without a hard post-condition (e.g. "at least one source file under `<target_root>` must be modified"), the sub-agent can satisfy the loop by producing analysis text only. 19k tokens of "I would change X by..." with no `write_file`/`edit_file` calls would match this pattern.

**Check**: implement-phase logs (if persisted), or the sub-agent's tool-call history for this ritual.

### c) `skip_design=true` + missing design.md → no executable plan to implement

Triage marked the ritual `large` with `skip_design=true`. The user (potato) did not provide a `design.md` for ISS-019 (the issue file itself contains a Design section, but it's prose, not a structured design doc the ritual can consume). With no design artifact, the implement phase may have nothing concrete to translate into edits — and if the prompt then degenerates to "figure it out", the sub-agent may produce only commentary.

**Check**: what does the implement phase actually feed to the sub-agent when `skip_design=true` and no `.gid/issues/<id>/design.md` exists?

### d) Cross-workspace target resolution failed silently

ISS-019 lives in `rustclaw` (issue files) but the code to fix is in `gid-rs` (`crates/gid-core/src/ritual/`). If the implement phase resolved `target_root` to the wrong project — or to rustclaw and then couldn't find the code there — it may have completed without errors because it had nothing to edit.

**Check**: state file's `target_root` and `project` fields vs where the code actually lives.

---

## Why this is critical

- **False success is worse than failure.** A failed ritual with `failed_phase` set tells the operator "retry / investigate". A `Done` ritual with no diff tells the operator "you're done" — and the operator moves on. The bug compounds: every subsequent decision assumes ISS-019 is fixed when it isn't.
- **Token waste**: 27k+ tokens spent producing nothing. At scale this is expensive and erodes trust in the ritual subsystem.
- **Blocks ISS-019 directly**: I cannot fix ISS-019 via the ritual flow if the ritual flow is broken in this way. ISS-019's own fix has to be done outside the ritual until ISS-025 is resolved.
- **Interacts with ISS-016**: even with main-agent ritual awareness, the agent reading the state file would conclude "Done, success" and not investigate. ISS-016 needs to surface `status`-vs-`phase`-vs-`diff` discrepancies.

---

## Scope

### In scope

1. Determine which of (a)/(b)/(c)/(d) — or combination — caused `r-af714a` to produce no output.
2. Add a hard post-condition to `Implementing` phase: at minimum log a warning, ideally fail the phase, when zero files were modified under `target_root` during the phase. (Tunable: some rituals legitimately produce no code, e.g. doc-only — but those should be triaged differently.)
3. Surface `phase_tokens` ↔ `files_changed` mismatch in ritual completion notification (if a phase burned >5k tokens and 0 files changed → flag it).
4. If hypothesis (c) is correct: refuse to enter `Implementing` when `skip_design=true` AND no design artifact is resolvable. Either generate a minimal design or fail-fast with a clear message.

### Out of scope

- ISS-019 itself (this issue is about the ritual subsystem; ISS-019 is the fix that triggered the discovery).
- Full design of a sandbox/worktree merge protocol (separate issue if hypothesis (a) is correct).

---

## Verification

1. Reproduce: re-run a ritual with similar params (`large`, `skip_design=true`, cross-workspace target) and confirm whether the no-op behavior is deterministic.
2. Inspect implement-phase prompt and sub-agent tool calls for `r-af714a` (if logs are retrievable from engram or daemon log).
3. After fix: a ritual that produces no file changes must either (a) fail the `Implementing` phase with a clear reason, or (b) be explicitly tagged as "doc-only" / "investigation" and skip Implementing.

---

## References

- Ritual `r-af714a` state file (concrete reproducer — do not delete).
- ISS-019 — the original task that exposed this bug.
- ISS-016 — main-agent ritual awareness (must surface this kind of false-success).
- gid-rs ISS-038 — sibling issue tracking the actual code fix.

---

## Resolution (2026-04-25)

Confirmed root cause: **hypothesis (b)** — sub-agent ran but only "researched", and there was no enforcement that it actually wrote files. Hypothesis (c) (skip_design + missing design) and (d) (cross-workspace target) were aggravating factors but not the root.

### What was actually broken

The `api_llm_client` skill execution path in `crates/gid-core/src/ritual/api_llm_client.rs` returned `SkillResult { artifacts_created: vec![], ... }` regardless of whether the LLM made `Write`/`Edit` tool calls. The intent was that the executor layer would own artifact accounting — but the executor never did. So:

1. LLM produces 19k tokens of commentary, zero `Write`/`Edit` tool calls.
2. `api_llm_client` returns `artifacts_created = []` (as designed — but the contract was never finished).
3. `v2_executor` emits `SkillCompleted { artifacts: [] }` (no post-condition check).
4. Verify phase runs against an unchanged tree, trivially passes.
5. State machine reaches `Done` with `failed_phase = None`.

The bug was a **missing contract enforcement**, not a missing feature. The artifact list always existed in the data flow — nothing was checking it against ground truth.

### Fix (root, not patch)

Filesystem snapshot before/after the LLM invocation, for any phase that requires file changes (currently just `implement`). The diff is the authoritative artifact list — the LLM-reported list is ignored. Zero diff on `implement` → `SkillFailed` with diagnostic message naming tokens consumed and tool calls made.

Implementation in gid-rs (`52a84ee fix(ritual): enforce implement-phase post-condition via fs snapshot`):

- New `crates/gid-core/src/ritual/file_snapshot.rs` (390 lines): `snapshot_dir` / `diff_snapshots`, `FsDiff { added, modified, deleted }`. Uses sha256 with a 64-byte head/tail+size fingerprint fallback for files >1 MiB. Reuses the project's `.gidignore` so build artifacts don't trip detection.
- `v2_executor` wraps `run_skill` with `snapshot_before` / `snapshot_after` when `phase_requires_file_changes`. Mutation root resolved via `resolve_mutation_root` — prefers ritual's `target_root` over `config.project_root` (this is the cross-workspace fix that addresses hypothesis (d) as a side-effect: post-conditions are now evaluated against the actual code root, not the project config root).
- On real diff, executor builds the `artifacts` list from the diff itself — the LLM-reported list is now explicitly ignored. `api_llm_client` retains `artifacts_created: vec![]` with a comment documenting the contract: empty vec is intentional, executor owns artifact accounting.
- Cross-workspace warning: when `target_root != project_root`, executor logs a one-line warning so the discrepancy is visible in logs/notifications (addresses ISS-016 surface area).

### Design choices (and why)

1. **fs-diff over LLM self-reporting.** The LLM's claim that it wrote a file is one signal; the filesystem actually having the file is another. The latter is ground truth. We chose ground truth.
2. **Hash fingerprint with large-file fallback.** Full-file sha256 on every snapshot would be wasteful for large binary blobs (model weights, generated assets). 64-byte head + 64-byte tail + size catches all real edits; only adversarial edits (changing the middle of a >1 MiB file by exactly the same byte count) would slip through, and ritual targets aren't adversarial.
3. **Reuse `.gidignore`, not a separate ignore list.** Diverging ignore rules between graph extraction and ritual snapshot would create silent mismatches. One source of truth.
4. **Per-phase opt-in via `phase_requires_file_changes`, not global.** Some phases (triage, planning) legitimately produce no file changes. Hard-coding "implement requires changes" is correct for now; if `verify` ever wants self-correcting edits, we add it to the predicate.
5. **`target_root` preference over `project_root`.** ISS-029 introduced work-unit binding, where the ritual can target a different project than the one it's running from. The post-condition must be evaluated against where the code actually lives, not where the ritual was invoked.

### Verification

- 7 new `file_snapshot` unit tests (added/modified/deleted detection, empty-diff identity, large-file fallback, gidignore filtering, cross-platform path handling).
- 3 new `v2_executor` end-to-end tests:
  - `implement_phase_with_zero_changes_emits_skill_failed`
  - `implement_phase_with_file_writes_emits_skill_completed_with_artifacts`
  - `non_implement_phase_does_not_enforce_changes`
- 187/187 ritual tests pass. No regressions.
- One pre-existing unrelated failure (`storage::tests::test_load_graph_auto_empty_dir_returns_default`) — sqlite feature-gate issue present on origin/main, not introduced here.

### Follow-up suggestions (deferred — separate issues if needed)

- **F1: Prompt-time enforcement.** Currently we detect the failure post-hoc (phase ran, then we check). A future improvement: detect mid-stream that the LLM hasn't called `Write`/`Edit` after N turns and inject a corrective system message. Cheaper than burning the full token budget. Tradeoff: more invasive, harder to reason about.
- **F2: Surface `phase_tokens` ↔ `files_changed` mismatch in completion notification.** Even with the fix, an operator skimming a Telegram completion message can't tell at a glance whether the diff was substantial. Add `Δfiles=N` to the `Done` notification. Ties into ISS-016.
- **F3: Doc-only / investigation rituals.** Some legitimate rituals produce zero code changes (writing a design doc, writing this very issue file). Currently they'd fail the post-condition. Either route them to a different phase predicate, or tag the work-unit as `kind: doc` and skip the implement-phase enforcement.
- **F4: Sandbox/worktree merge protocol** (hypothesis (a)). Not needed for this fix — the no-op rituals were not running in a sandbox, they were running directly against `target_root` and just not writing. But if the ritual subsystem ever moves to sandboxed execution, the snapshot needs to be taken at the merge point, not the sandbox.
- **F5: `skip_design=true` + missing design artifact** (hypothesis (c)). Not the root, but still real: rituals shouldn't enter `Implementing` with no implementation plan. Either auto-generate a minimal design from the issue body, or fail-fast with "no design, no implement". Lower priority now that the post-condition catches the symptom.
