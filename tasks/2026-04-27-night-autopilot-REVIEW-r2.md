# Re-review of 2026-04-27-night-autopilot.md (after r1 fixes)

> Pass: 2026-04-27 ~01:00. Reviewer: RustClaw (self-review, after applying r1 findings).
> Result: **3 critical, 2 important, 1 minor** new findings. Do NOT deploy until critical fixed.

## Method

Cross-checked every claim in the doc against:
- `/Users/potato/clawd/projects/engram/.gid/features/v03-resolution/design.md`
- `/Users/potato/clawd/projects/engram/.gid/features/v03-migration/design.md`
- `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-*-build-plan.md`
- `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db` (task IDs + counts)
- Filesystem (which crates exist, which modules already exist)

---

## 🔴 FINDING-R2-1: §A.1 misattributes `resolve_for_backfill` to `Memory`

**Location**: §A.1 "Design §s (verified)" row + §3a cross-section deps table.

**Claim in doc**:
> §6.5 — `Memory::resolve_for_backfill` (migration handoff)

**Ground truth** (`design.md` §6.5, line 898–917):
```rust
impl ResolutionPipeline {
    pub fn resolve_for_backfill(
        &mut self,
        memory: &MemoryRecord,
    ) -> Result<GraphDelta, PipelineError>;
}
```

`resolve_for_backfill` is on **`ResolutionPipeline`**, not `Memory`. Design.md is explicit: "`resolve_for_backfill` is the **pipeline entry point** that differs from normal `resolve()`...". Design.md §10 also confirms this is a `ResolutionPipeline` method.

**Why this matters**: A.1's task is about additions to `Memory` (per build plan: "extends existing file `memory.rs`"). If the night agent reads my doc, they'll add `resolve_for_backfill` to `Memory` — wrong impl block, wrong return type (`PipelineError` not `EngramError`), wrong file possibly. Then §C migration's per-record task can't find the method where expected and stalls.

**Note**: The build plan ALSO has this wrong (line 52 of `v03-resolution-build-plan.md` lists `Memory::resolve_for_backfill`). So this is a build-plan-vs-design disagreement which is itself a stop-condition #4 trigger. The autopilot doc should not silently echo the build plan's error — it should flag the discrepancy.

**Fix**:
- A.1 row: drop `resolve_for_backfill` from §6.5 line, add a separate row noting "§6.5 `ResolutionPipeline::resolve_for_backfill` lives elsewhere — NOT a `Memory` method per design.md, though build plan currently lists it under `memory.rs`. **Stop-condition #4 if you encounter this disagreement.**"
- §3a row for `mig-impl-backfill-perrecord`: change `Memory::resolve_for_backfill` → `ResolutionPipeline::resolve_for_backfill` and note where it actually lives.
- DoD count adjusts: 5 `Memory` methods + 1 `ResolutionPipeline` method (or punt the pipeline method to a separate task — depends on what the agent decides when they hit stop-condition #4).

**Severity**: 🔴 critical — sets the agent up to write code in the wrong impl block.

---

## 🔴 FINDING-R2-2: Migration CLI location wrong (still!)

**Location**: §C "CLI binary" paragraph.

**Claim in doc**:
> CLI binary: per build plan / design — confirm via the build plan whether it's `crates/engramai-migrate/src/main.rs` (binary added to existing crate) or a `bin/` target. Read `crates/engramai-migrate/Cargo.toml` first; if no `[[bin]]` section, the build plan / `task:mig-impl-cli` description tells you what to add.

**Ground truth** (`v03-migration/design.md` §9.1, line 748):
> The migration tool is a **subcommand of the existing `engramai` binary** (GUARD-9 — no new external deps, **no new binary**).

The `engramai` binary is in `crates/engram-cli/src/main.rs` (the existing v0.2 user CLI, version 0.2.3). Migration is a **subcommand** added there, delegating to the **library** in `crates/engramai-migrate/`.

So the architecture is:
- `crates/engramai-migrate/` — library only (orchestrator, phase machine, schema, etc.)
- `crates/engram-cli/src/main.rs` — adds `migrate` subcommand to the `engramai` binary, calls into `engramai-migrate` lib

Both my r1 options were wrong. r1 fixed FINDING-3 (don't put it in `engramai/src/bin/`) but introduced a new error (suggesting a binary in `engramai-migrate`).

**Fix**: Replace the CLI binary paragraph with:
> **CLI surface (per design §9.1, GUARD-9 "no new binary"):** the `migrate` subcommand is added to the existing `engramai` binary in **`crates/engram-cli/src/main.rs`**. The orchestration logic lives in the **`engramai-migrate` library** (no `[[bin]]` target needed there). `task:mig-impl-cli` therefore touches both: (a) `engram-cli` for argument parsing + subcommand dispatch, (b) `engramai-migrate::cli` (or wherever the lib places the surface) for the actual phase driver. Read both crates' `Cargo.toml` + `main.rs`/`lib.rs` before adding code.

**Severity**: 🔴 critical — direct ground-truth violation, will cause architecture drift if implemented wrong.

---

## 🔴 FINDING-R2-3: §D task count wrong (16 vs actual 17)

**Location**: §D heading, footer.

**Claim in doc**:
> # §D — Benchmarks (16 tasks)
>
> *End of autopilot doc. 4 sections, 52 remaining tasks pointed at (2 + 16 + 17 + 16, with mig-impl-error already done).*

**Ground truth** (graph.db count): `bench|todo|17`. There are 17 benchmark tasks in todo state, not 16.

Also the footer math: `2 + 16 + 17 + 16 = 51` (not 52). With actual 17: `2 + 16 + 17 + 17 = 52` ✅.

The "with mig-impl-error already done" footnote is also redundant/confusing because that `done` task is already excluded from the migration count of 17.

**Fix**:
- §D heading: `(16 tasks)` → `(17 tasks)`
- Footer: `(2 + 16 + 17 + 16, with mig-impl-error already done)` → `(2 + 16 + 17 + 17 = 52). §C migration also has 1 already-done task (mig-impl-error), so 18 total nodes there.`

**Severity**: 🔴 critical — wrong task count, agent will think they're done before completing the feature.

---

## 🟡 FINDING-R2-4: §A.2 missing primary GOAL (GOAL-2.10)

**Location**: §A.2 "Requirements" row.

**Claim in doc**:
> Requirements: GOAL-3.6, GOAL-3.7 (cross-feature, produce-side — these are consumed by retrieval)

**Ground truth** (build plan line 51):
> 2.10 (topic supersession parity); cross-feature GOAL-3.6 / GOAL-3.7 produce-side

The PRIMARY satisfies-edge for `task:res-impl-knowledge-compile` is **GOAL-2.10** (topic supersession parity). GOAL-3.6/3.7 are cross-feature notes only (retrieval owns those, knowledge_compile produces what they consume).

**Fix**:
> Requirements: **GOAL-2.10** (topic supersession parity, primary `satisfies` edge). Cross-feature: GOAL-3.6 / GOAL-3.7 are owned by retrieval but produce-side lives here (§5bis) — implement the producer side; retrieval will add its own `satisfies` edges.

**Severity**: 🟡 important — wrong primary requirement reference would mean the agent doesn't write the supersession test that GOAL-2.10 demands.

---

## 🟡 FINDING-R2-5: §A.2 doesn't disambiguate from existing `compiler/` module

**Location**: §A.2 "Target file" row.

**Claim in doc**:
> Target file: `crates/engramai/src/knowledge_compile/mod.rs` (new module — also add `pub mod knowledge_compile;` to `lib.rs`)

**Ground truth**: `crates/engramai/src/compiler/` already exists (the v0.2 KnowledgeCompiler with 19 files: `api.rs`, `compilation.rs`, `intake.rs`, `topic_lifecycle.rs`, `feedback.rs`, etc.). `lib.rs` already has `pub mod compiler;`.

The new v0.3 module is `knowledge_compile` (singular, no -er) — different name, but a tired night agent might:
- Try to add v0.3 logic to the existing `compiler/` module
- Get confused which is which when reading `lib.rs`
- Accidentally `mod.rs` collide

**Fix**: Add note to A.2:
> ⚠️ The existing `crates/engramai/src/compiler/` module is the **v0.2 KnowledgeCompiler** — DO NOT modify it for this task. The v0.3 module is named `knowledge_compile` (singular) and is a NEW directory. Both will coexist during migration; v0.2 `compiler/` is consumed/replaced later (out of scope tonight).

**Severity**: 🟡 important — collision/confusion risk, especially at 3am.

---

## 🟢 FINDING-R2-6: §C task ID prefix in build plan is stale

**Location**: §C overall (informational, not a doc bug).

**Observation**: The build plan `v03-migration-build-plan.md` lines 83, 99, etc. use task ID prefix `task:migration-impl-*` which **does not match** the actual graph IDs `task:mig-impl-*`. The autopilot doc correctly uses `task:mig-impl-*` so this is fine — but the night agent following step 3 of §2 ("cross-check the build plan") will see this mismatch and might trip stop-condition #4.

**Fix** (preventive — add to §C conventions):
> Heads-up: the migration build plan uses stale task ID prefix `migration-impl-*` in places. The actual graph node IDs are `mig-impl-*`. This is a known cosmetic mismatch — NOT a stop-condition #4 (which is about file/section disagreements, not naming drift in stale build plans). Trust the graph IDs.

**Severity**: 🟢 minor — UX/false-stop prevention.

---

## Summary

| ID | Finding | Severity |
|---|---|---|
| R2-1 | A.1 says `Memory::resolve_for_backfill`; design says `ResolutionPipeline::resolve_for_backfill` | 🔴 critical |
| R2-2 | §C CLI binary location still wrong — design §9.1 says it's a subcommand in `engram-cli`, no new binary | 🔴 critical |
| R2-3 | §D task count is 16; actual is 17 (graph.db) | 🔴 critical |
| R2-4 | §A.2 missing primary GOAL-2.10 (supersession parity) | 🟡 important |
| R2-5 | §A.2 doesn't disambiguate from existing v0.2 `compiler/` module | 🟡 important |
| R2-6 | Stale task IDs in migration build plan — preempt false-stop | 🟢 minor |

**Recommendation**: do NOT deploy. Fix R2-1, R2-2, R2-3 minimum. R2-4/5/6 are cheap to fix in the same edit pass.

**Self-critique**: r1 fixed the table-duplication-causes-drift problem but I still let two ground-truth violations through (resolve_for_backfill, CLI location). Same root cause: didn't open design.md and verify line-by-line for the §A and §C claims I kept. Lesson: pointer-style ≠ "I can stop verifying." Every cited section number, every cited method signature, every cited file path must be confirmed against the actual file before shipping. The pointer just changes WHERE the error can hide, not WHETHER errors exist.
