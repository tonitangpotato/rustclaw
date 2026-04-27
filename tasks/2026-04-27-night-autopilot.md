# Night Autopilot — engram v0.3 (2026-04-27)

> **Goal**: Continue v0.3 implementation while potato sleeps.
> **Strategy**: Pointer-style. This doc tells you WHERE to look, not WHAT'S there.
> **Source-of-truth**: the per-feature `*-build-plan.md` files in `.gid-v03-context/`. They list every task with file_path + design ref + GOAL/GUARDs + dependencies. **DO NOT re-derive plan content into this file.**

---

## 0. Canonical paths (memorize / always use full path)

| What | Full path |
|---|---|
| engram repo root | `/Users/potato/clawd/projects/engram/` |
| engramai crate (library, retrieval, resolution code) | `/Users/potato/clawd/projects/engram/crates/engramai/` |
| **engramai-migrate crate** (already exists — migration code goes HERE) | `/Users/potato/clawd/projects/engram/crates/engramai-migrate/` |
| engram-bench crate (benchmarks — NEW, create in §D) | `/Users/potato/clawd/projects/engram/crates/engram-bench/` |
| **v0.3 working graph DB** (NOT `.gid/graph.db`) | `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db` |
| v0.3 context dir (build plans live here) | `/Users/potato/clawd/projects/engram/.gid-v03-context/` |
| v0.3 PATHS lock-in doc | `/Users/potato/clawd/projects/engram/.gid-v03-context/PATHS.md` |
| Feature docs root | `/Users/potato/clawd/projects/engram/.gid/features/<feature>/` |
| this agent's workspace | `/Users/potato/rustclaw/` |

**Rules:**
- All `gid_*` tool calls MUST pass `graph_path="/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db"`. Forget this, you'll hit the wrong DB.
- All file edits happen inside the engram monorepo — never in `engram-ai-rust/` (deprecated).
- After each task: `cd /Users/potato/clawd/projects/engram && cargo test -p <crate> --lib` must pass.
- Conventional Commits, reference the task ID.

---

## 1. Where to find what (the index)

For ANY task `task:<feature-prefix>-impl-<thing>` or `task:<prefix>-test-<thing>`:

| You need… | Look at… |
|---|---|
| Full task description + DoD pointer | `gid_tasks(graph_path=<v03 db>)` — read the **ready** subset. Then `gid_query_deps(id=<task_id>, graph_path=<v03 db>)` to confirm zero unmet deps. |
| Canonical task list for the feature (file_path, design ref, GOAL/GUARDs per task) | `.gid-v03-context/v03-<feature>-build-plan.md` — **THIS is ground truth**, not this autopilot doc. |
| Design section it implements | `.gid/features/<feature>/design.md` at the §-number the build plan cites |
| Requirements (GOAL-X.Y / GUARD-N) it satisfies | `.gid/features/<feature>/requirements.md` |
| Existing code style/conventions | sibling files in the same module |
| Cross-feature interface contracts | each design's "Cross-feature References" section (numbering varies — last numbered § before Traceability) |

**Feature → prefix map:**
- `task:res-*` → v03-resolution
- `task:retr-*` → v03-retrieval
- `task:mig-*` → v03-migration
- `task:bench-*` → v03-benchmarks

---

## 2. Per-task workflow (do this every time, no shortcuts)

1. `gid_update_task(graph_path=<v03 db>, id=<task_id>, status="in_progress")`
2. Read the task node (description + cited §s + cited GOAL/GUARDs)
3. Cross-check the build plan for that feature — confirm the file_path, design ref, and `implements:` mapping match what the task node says. **If they disagree, STOP** (condition #4 below).
4. Open `.gid/features/<feature>/design.md` at the cited §s, read fully
5. Open `.gid/features/<feature>/requirements.md`, read the cited GOAL/GUARDs
6. Read 1-2 sibling files in the same module for style
7. Implement
8. `cargo test -p <crate> --lib` (engramai / engramai-migrate / engram-bench depending on the task) — must be green
9. Commit (Conventional Commits, ref task ID)
10. `gid_complete(graph_path=<v03 db>, id=<task_id>)` — note the unblocked downstream tasks
11. Move to next ready task

**Never mark a task done if (a) tests fail OR (b) the target file doesn't exist on disk.**

---

## 3. Execution layers (dependency order)

```
Layer 0  ✅ DONE    v03-graph-layer (14/14)
Layer 1  →          §A  Resolution remaining (2 tasks)              ← do first
Layer 2  →          §B  Retrieval (16 tasks)   ┐ parallel-safe
                    §C  Migration (17 tasks)   ┘ after §A
Layer 3  →          §D  Benchmarks (16 tasks)  ← needs §A + §B + §C
```

**Order of attack tonight:**
1. Finish §A (it unblocks the rest)
2. Pick whichever of §B / §C has the largest **ready frontier** — work that
3. Don't start §D unless §A + §B + §C are fully green (benchmarks consume their public APIs)

### 3a. Cross-section dependencies (don't start downstream before upstream is `done`)

These dependencies cross feature boundaries and may not be fully encoded as graph edges. **Verify the upstream is `done` before starting any of these:**

| Downstream task | Upstream needed | Why |
|---|---|---|
| `task:retr-impl-abstract-l5` (§B) | `task:res-impl-knowledge-compile` (§A.2) | Consumes `KnowledgeTopic` store + cluster_weights |
| `task:mig-impl-backfill-perrecord` (§C) | `task:res-impl-memory-api` (§A.1) — specifically `Memory::resolve_for_backfill` | Per-record migration calls this method |
| `task:bench-impl-driver-cost` (§D) | `task:res-impl-memory-api` (§A.1) — specifically `Memory::ingest_with_stats` + `ResolutionStats` | Cost driver consumes the stats public contract |
| `task:bench-impl-driver-test-preservation` (§D) | All of §C | Runs v0.2 tests through the migration tool |
| `task:bench-impl-driver-migration` (§D) | All of §C | Drives migration end-to-end |

If a task you want to start has any of these upstream deps still `todo` or `in_progress` → pick a different ready task.

---

## 4. Ready-frontier rule

`gid_tasks(graph_path=<v03 db>)` reports a `ready` (unblocked) subset — that's the frontier. Don't pick tasks off the raw `--status=todo` list (it includes blocked ones). For every task you want to start, run `gid_query_deps(id=<task_id>, graph_path=<v03 db>)` and confirm **all** returned deps have status=done.

Plus the cross-section table in §3a above (graph may not encode every cross-feature dep).

---

## 5. Stop conditions (read before starting)

STOP and write `tasks/2026-04-27-night-autopilot-STATUS.md` if any of:

1. A previously-green test goes red and 2 fix attempts fail
2. A task description references a file or section that doesn't exist (e.g., design § not found, file path doesn't resolve)
3. You'd need to make a non-trivial design decision not pinned in design.md
4. Build plan and graph task node disagree on file_path / design ref / IDs (one of them is stale — don't guess)
5. Cross-feature contract conflict (e.g., `ResolutionStats` field doesn't match what benchmarks expect)
6. v0.3 graph DB integrity looks off (unexpected snapshot files, edge-count drift, `gid_validate` failures)
7. `cargo build` breaks at the workspace level (not just the crate you're editing)
8. **You hit a §C migration test failure — STOP IMMEDIATELY, migration bugs corrupt user data**

DO NOT skip a task and continue. Layers gate each other; broken upstream silently corrupts downstream.

---

# §A — Resolution remaining (2 tasks)

> **Build plan**: `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-resolution-build-plan.md`
> **Design**: `/Users/potato/clawd/projects/engram/.gid/features/v03-resolution/design.md`
> **Requirements**: `/Users/potato/clawd/projects/engram/.gid/features/v03-resolution/requirements.md`

These two unblock everything downstream. Do them first, in this order.

## A.1 — `task:res-impl-memory-api`

| Field | Value |
|---|---|
| Target file | `/Users/potato/clawd/projects/engram/crates/engramai/src/memory.rs` (additions, NOT rewrite) |
| Design §s (verified) | **§6.2** — `Memory::reextract`, `Memory::reextract_failed`, `Memory::compile_knowledge`, `Memory::list_knowledge_topics` (all four covered here); **§6.4** — `Memory::ingest_with_stats` + `ResolutionStats` public contract; **§6.5** — `Memory::resolve_for_backfill` (migration handoff); **§5bis** — read for context on what `compile_knowledge` actually does |
| Requirements | GOAL-2.1 (idempotence), GOAL-2.2/2.3 (failure surfacing), GOAL-2.11 (stats surface), GOAL-2.14 (rolling-avg surface) |
| Depends on | v03-graph-layer (DONE). **`compile_knowledge` body depends on §A.2** — stub it now (return empty `Vec<KnowledgeTopicId>` or `unimplemented!()` behind a TODO referencing A.2), fill in after A.2 lands. |
| Test command | `cd /Users/potato/clawd/projects/engram && cargo test -p engramai --lib memory::` |
| DoD | All 6 methods present with correct signatures; doc comments cite design §s + GOAL ids; unit tests for each (idempotence on `reextract` is **mandatory** per GOAL-2.1) |
| Notes | `ResolutionStats` is a public benchmarks contract — once shipped, breaking it requires version bump. Mirror the `&mut self` borrowing pattern from already-done graph methods in the same file. |

## A.2 — `task:res-impl-knowledge-compile`

| Field | Value |
|---|---|
| Target file | `/Users/potato/clawd/projects/engram/crates/engramai/src/knowledge_compile/mod.rs` (new module — also add `pub mod knowledge_compile;` to `lib.rs`) |
| Design §s | **§5bis** Knowledge Compiler (entire section — K1, K2, K3 stages) |
| Requirements | GOAL-3.6, GOAL-3.7 (cross-feature, produce-side — these are consumed by retrieval) |
| Depends on | A.1 stub of `Memory::compile_knowledge` and `Memory::list_knowledge_topics` |
| Test command | `cd /Users/potato/clawd/projects/engram && cargo test -p engramai --lib knowledge_compile::` |
| DoD | K1 candidate selection + K2 clustering trait with default HDBSCAN impl + K3 LLM summary + atomic per-cluster persist with topic supersession. Cost metrics use `knowledge_compile_*` namespace (cost isolation per design). |
| After done | Go back to A.1, replace the stubs in `Memory::compile_knowledge` / `Memory::list_knowledge_topics` with real calls into this module, re-run tests. |

---

# §B — Retrieval (16 tasks)

> **Build plan** (canonical task list): `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-retrieval-build-plan.md` — every task's file_path, design ref §s, GOAL/GUARDs, and dependencies are listed there. **READ IT, don't infer.**
> **Design**: `/Users/potato/clawd/projects/engram/.gid/features/v03-retrieval/design.md` (r3 approved, 668 lines)
> **Requirements**: `/Users/potato/clawd/projects/engram/.gid/features/v03-retrieval/requirements.md`

**Common conventions for ALL §B tasks:**
- Code lives under `/Users/potato/clawd/projects/engram/crates/engramai/src/retrieval/`
- Test command: `cd /Users/potato/clawd/projects/engram && cargo test -p engramai --lib retrieval::`
- Property tests are mandatory where the task name says "contract" / "determinism" / "purity"
- Zero clippy warnings: `cargo clippy -p engramai -- -D warnings`

**Hot-path / ordering tips:**
- `task:retr-impl-fusion` blocks effective behavior of the affective / abstract-l5 / hybrid plans (they all weight via fusion). Do fusion before those if dep order allows.
- `task:retr-impl-metrics` touches every plan — do AFTER the plan implementations are stable, easier to wire metrics once shapes are settled.
- `task:retr-test-determinism-routing-accuracy` is the **acceptance gate** — only attempt after the other 15 are green.

**Cross-section dep:** `task:retr-impl-abstract-l5` requires §A.2 done (consumes KnowledgeTopic store).

---

# §C — Migration (17 tasks)

> **Build plan** (canonical task list): `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-migration-build-plan.md`
> **Design**: `/Users/potato/clawd/projects/engram/.gid/features/v03-migration/design.md`
> **Requirements**: `/Users/potato/clawd/projects/engram/.gid/features/v03-migration/requirements.md`
> **Note**: `task:mig-impl-error` is already DONE — don't re-do it. The remaining count is 17.

**Common conventions for ALL §C tasks:**
- **Code crate**: `/Users/potato/clawd/projects/engram/crates/engramai-migrate/` (this crate **already exists** — `error.rs` and `lib.rs` are present from `task:mig-impl-error`). All migration code goes HERE, NOT in `engramai`.
- Read `crates/engramai-migrate/Cargo.toml` and `src/lib.rs` before adding code so module structure stays consistent.
- Test command: `cd /Users/potato/clawd/projects/engram && cargo test -p engramai-migrate --lib`
- Backward-compat tests (`task:mig-impl-compat`, `task:mig-test-compat-rollback`): also run `cargo test -p engramai-migrate` (full, not just `--lib`).
- Migration touches schema. Run **`cargo test -p engramai-migrate`** (full) before each `gid_complete`.
- Schema changes are **non-destructive adds only** — never drop columns/tables in v0.3.

**CLI binary**: per build plan / design — confirm via the build plan whether it's `crates/engramai-migrate/src/main.rs` (binary added to existing crate) or a `bin/` target. Read `crates/engramai-migrate/Cargo.toml` first; if no `[[bin]]` section, the build plan / `task:mig-impl-cli` description tells you what to add.

**Hot-path / ordering tips:**
- Foundation tasks (progress, checkpoint, lock, preflight) before orchestrator/per-record/failure.
- Schema (`task:mig-impl-schema`) must be done before any backfill task — backfill writes into the new columns.
- Phase machine (`task:mig-impl-phase-machine`) wires everything — do after the phase contents exist.
- CLI (`task:mig-impl-cli`) consumes most of the above — late.

**Cross-section dep:** `task:mig-impl-backfill-perrecord` requires §A.1 done (calls `Memory::resolve_for_backfill`).

**🚨 GUARD specific to migration:** if any migration test fails, STOP IMMEDIATELY (stop condition #8). Migration bugs corrupt user data — there is no "small" migration regression.

---

# §D — Benchmarks (16 tasks)

> **Build plan** (canonical task list): `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-benchmarks-build-plan.md`
> **Design**: `/Users/potato/clawd/projects/engram/.gid/features/v03-benchmarks/design.md`
> **Requirements**: `/Users/potato/clawd/projects/engram/.gid/features/v03-benchmarks/requirements.md`

**⚠️ Do NOT start §D unless §A + §B + §C are fully green.** Benchmarks consume the public APIs you just built; broken upstream = wasted work here.

**Common conventions for ALL §D tasks:**
- Code lives in a NEW crate: `/Users/potato/clawd/projects/engram/crates/engram-bench/`
- **GUARD-9 boundary** (CRITICAL): benchmark-only deps live in `engram-bench/Cargo.toml`, NEVER in `engramai/Cargo.toml`. Validation: `cargo build -p engramai` must succeed without ANY benchmark dep present.
- Test command: `cd /Users/potato/clawd/projects/engram && cargo test -p engram-bench`
- After ALL §D tasks done: `cd /Users/potato/clawd/projects/engram && cargo build -p engramai && cargo build -p engram-bench` — both must pass independently.
- Fixtures live under `crates/engram-bench/fixtures/` and `crates/engram-bench/benchmarks/` per design §6.

**Hot-path / ordering** (per the build plan):
- Cargo.toml + lib.rs first — establishes crate structure
- Then baselines + harness + repro + gates (infrastructure, strict order per build plan)
- Then anonymizer + scorers (independent, can interleave)
- Then drivers (each consumes scorers + harness)
- Then main + reporting (last — they consume everything)

**Cross-section deps:**
- `task:bench-impl-driver-cost` → §A.1 (`Memory::ingest_with_stats` + `ResolutionStats`)
- `task:bench-impl-driver-test-preservation` → §C complete
- `task:bench-impl-driver-migration` → §C complete

---

# §E — Stop conditions, status reporting, escalation

## When to stop

See §5 above for the full list. The short version: **anything ambiguous, broken, or contract-violating → STOP and write a STATUS file**, don't guess.

## Status file format

Write `tasks/2026-04-27-night-autopilot-STATUS.md`:

```markdown
# Autopilot Status — 2026-04-27 night

## Completed
- task:res-impl-memory-api  ✅ commit abc123
- task:res-impl-knowledge-compile  ✅ commit def456
- ...

## Stopped at
- task:retr-impl-fusion

## Why
{one paragraph — what failed, what you tried, what you think is needed}

## Tests state
- engramai: {N pass, M fail}  ← list failing tests
- engramai-migrate: {N pass, M fail}
- engram-bench: {N pass, M fail}

## Suggested next step for potato
{your best guess at what's needed to unblock}
```

## Reminders / mental model

- The `ready` subset of `gid_tasks(graph_path=<v03 db>)` is your **ground truth** for what's startable, not this doc.
- The build plan files in `.gid-v03-context/` are **ground truth** for per-task content — file path, design refs, GOAL/GUARDs.
- This autopilot doc only tells you HOW to navigate; the build plans tell you WHAT each task is.
- After EVERY task: `gid_complete` — don't batch. Losing track of which is done is the #1 failure mode.
- Commits small and atomic — one task per commit, conventional commit format with task ID.
- If unsure between two interpretations: pick the one that makes existing tests still pass + matches design.md verbatim. If design.md is ambiguous: STOP (condition #3).

## Done condition for the night

The night's work is "successful" if:
- §A complete (2 tasks, resolution feature 13/13)
- Either §B fully complete OR §C fully complete (one full feature is better than two half-features)
- All `cargo test` for the touched crates passes
- `gid_validate(graph_path=<v03 db>)` clean

Anything beyond that is bonus. **Don't sacrifice correctness for coverage.**

---

*End of autopilot doc. 4 sections, 52 remaining tasks pointed at (2 + 16 + 17 + 16, with mig-impl-error already done).*
