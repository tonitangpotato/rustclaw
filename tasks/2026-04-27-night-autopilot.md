# Night Autopilot — engram v0.3 (2026-04-27)

> **Goal**: Continue v0.3 implementation while potato sleeps.
> **Strategy**: Pointer-style. This doc tells you WHERE to look, not WHAT'S there.
> **Source-of-truth**: the per-feature `*-build-plan.md` files in `.gid-v03-context/`. They list every task with file_path + design ref + GOAL/GUARDs + dependencies. **DO NOT re-derive plan content into this file.**

---

## Autopilot Tasks (checkbox queue)

> The autopilot reads only the line text below as the task. Each line tells you the graph task ID + which § of THIS doc to read for context. Workflow per task is always **§2** below. **You MUST `cd /Users/potato/clawd/projects/engram` before any cargo or gid command, and pass `graph_path="/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db"` to every `gid_*` call.** Read §0 (paths) and §2 (workflow) before starting your first task.

### Layer 1 — Resolution (do first, blocks Layer 2 §C)

- [x] task:res-impl-memory-api — see §A.1 of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (note: `resolve_for_backfill` is on `ResolutionPipeline`, NOT `Memory` — if build plan disagrees → stop-condition #4). Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:res-impl-knowledge-compile — see §A.2 of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (NEW module `knowledge_compile/`, do NOT touch existing `compiler/`; primary GOAL-2.10). Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.

### Layer 2A — Retrieval (parallel-safe; pull ready tasks in dep order)

- [x] task:retr-impl-classifier-heuristic — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-graph-query-api — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-budget-cutoff — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-reranker-contract — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-classifier-llm — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-factual-bitemporal — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-episodic — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-associative — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-abstract-l5 — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (depends on §A.2). Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-affective — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-hybrid — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-fusion — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-typed-outcomes — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-explain-trace — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-impl-metrics — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:retr-test-determinism-routing-accuracy — see §B of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (cross-cutting acceptance gate; needs full retrieval). Authoritative task list: `.gid-v03-context/v03-retrieval-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.

### Layer 2B — Migration (parallel-safe with 2A; backfill-perrecord depends on §A.1)

- [x] task:mig-impl-progress — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Note CLI lives in `engram-cli/src/main.rs` per §C, NOT a new binary. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-checkpoint — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-lock — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-preflight — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-backup — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-schema — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-backfill-orchestrator — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [⛔ BLOCKED] task:mig-impl-backfill-perrecord — BLOCKED on stop-condition #4 (A.1 deferred `ResolutionPipeline::resolve_for_backfill` to human triage; method does not exist in codebase per `grep -rn resolve_for_backfill crates/engramai/src/`). DO NOT RETRY — see `tasks/2026-04-27-night-autopilot-STATUS.md` for why stubbing or implementing here is wrong. Unblock requires filing a new resolution task to add the method per design §6.5, not re-running this task. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-backfill-failure — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-topics — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`. ⛔ BLOCKED — stop-condition #5 (cross-feature contract conflict): migration design §6 references `knowledge_topics` columns (`legacy`, `provenance`, `content`, `status`, `version`, `quality_score`, `compilation_count`, `tags`, `source_memory_ids`, `created_at`, `updated_at`) that v03-graph-layer's actual schema does NOT have. Needs design decision (extend DDL / rewrite §6 / abandon carry-forward / new sub-feature). See `tasks/2026-04-27-night-autopilot-STATUS.md`.
- [x] task:mig-impl-phase-machine — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-compat — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-cli — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (CLI surface = `migrate` subcommand in `engram-cli/src/main.rs`, library logic in `engramai-migrate`. CLI is a thin wrapper over the rust crate — logic lives in the library, NOT main.rs). Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-impl-rollback-procedure — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-test-idempotency — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-test-resume — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:mig-test-compat-rollback — see §C of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-migration-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.

### Layer 3 — Benchmarks (after Layers 1+2; do in dep order, lib/cargo first)

- [x] task:bench-impl-cargo-toml — see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (GUARD-9 boundary; new crate `engram-bench` — bench deps NOT in engramai). Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-lib — see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-baselines — see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-harness — ✅ DONE (manually, 2026-04-27 10:35; 22/22 tests pass; GUARD-9 holds). All 6 sub-tasks below completed in order. ⚠️ SPLIT into 6 sub-tasks below (context-explosion root fix, 2026-04-27 10:15). Original task scope was too large for a single sub-agent or single main-agent attempt; each previous attempt died mid-write. All 6 sub-tasks land in `crates/engram-bench/src/harness/mod.rs` (additive, in order). Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-harness-1-types — Define `BenchError` enum (FixtureMissing, ChecksumMismatch, BaselineMissing, DriverPanic{name,msg}, BlockedBy(stage), IoError, Other) and `HarnessConfig` struct (fixture_root, baseline_root, output_root, parallel_limit, seed, override_gate, rationale_file). ~80 lines. Pre-load: design.md §3, §6.1, current `harness/mod.rs`, `harness/gates.rs` (for GateResult/Priority/ReleaseDecision types already defined). Verify: `cargo check -p engram-bench`.
  - [x] task:bench-impl-harness-2-driver-trait — Define `BenchDriver` trait (`name() -> &str`, `stage() -> Stage`, `cost_tier() -> CostTier`, `run(&self, cfg: &HarnessConfig) -> Result<RunReport, BenchError>`) + `Stage` enum (Stage1/Stage2) + `CostTier` enum (Cheap/Medium/Expensive). ~50 lines. Pre-load: design.md §3, §4.3, `harness/mod.rs` (with sub-task 1 applied), `lib.rs` re-exports. Verify: `cargo check -p engram-bench`.
  - [x] task:bench-impl-harness-3-fixture-db — Implement `verify_fixture_sha(path, expected_hex) -> Result<(), BenchError>` (read file, sha2::Sha256, hex compare) + `fresh_in_memory_db() -> Result<engramai::Memory, BenchError>` (wraps `Memory::new(":memory:", None)`). ~70 lines + 2 unit tests. Pre-load: design.md §6.1, §3, `harness/mod.rs`, `engramai/src/memory.rs:660-680`. Verify: `cargo test -p engram-bench --lib harness::`.
  - [x] task:bench-impl-harness-4-parallel-runner — Implement `run_drivers_parallel(drivers: &[Box<dyn BenchDriver>], cfg: &HarnessConfig) -> Vec<Result<RunReport, BenchError>>` using `std::thread::scope`: partition by stage, sort within stage by cost_tier, respect `cfg.parallel_limit`, catch panics → `DriverPanic`, stage 2 starts only after stage 1 complete. ~130 lines + 1 panic-catch test. Pre-load: design.md §4.3 (parallel execution paragraph + DAG), `harness/mod.rs` (sub-tasks 1-3 applied). Verify: `cargo test -p engram-bench --lib harness::`.
  - [x] task:bench-impl-harness-5-release-gate — Implement `run_release_gate(drivers, cfg) -> Vec<RunReport>` orchestrator: calls `run_drivers_parallel`, materializes errors as ERROR-status RunReports (per design §4.4 Level 1: missing/null = ERROR, never PASS), returns vector. ~80 lines + 1 integration test (mock driver returning error becomes ERROR report not silent skip). Pre-load: design.md §4.4 (failure semantics), §3 (RunReport shape from gates.rs), `harness/mod.rs` with sub-tasks 1-4 applied. Verify: `cargo test -p engram-bench`.
  - [x] task:bench-impl-harness-6-aggregate — Implement `aggregate_release_decision(reports: &[RunReport], override: Option<&Override>) -> ReleaseDecision`: P0 fail = Block (override only with signed Rationale per §4.4); P1 fail = Warn; P2 fail = Note; ANY ERROR (missing baseline / fixture mismatch / driver panic) = Block regardless of priority. ~100 lines + 4 table-driven tests (P0-pass-P1-fail = Warn; P0-fail-no-override = Block; P0-fail-with-override = Override; any-ERROR = Block). Pre-load: design.md §4.4, §7.2, `harness/gates.rs` (Priority, GateStatus, ReleaseDecision, Override, Rationale already defined), `harness/mod.rs` with sub-tasks 1-5 applied. Verify: `cargo test -p engram-bench` + final `cargo build -p engramai` (GUARD-9 boundary check).
- [x] task:bench-impl-repro — ✅ DONE 2026-04-27 ~11:50 (all 3 sub-tasks complete; 31 lib tests pass; zero warnings; GUARD-9 holds). ⚠️ SPLIT into 3 sub-tasks below (pre-emptive context-explosion fix, 2026-04-27 morning). Original scope ≈ 250-400 lines (TOML schema §6.1 has 8+ tables, plus writer/reader/replay+validator). All sub-tasks land in `crates/engram-bench/src/harness/repro.rs`. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-repro-1-schema — ✅ DONE 2026-04-27 ~11:25; 24 lib tests pass (22 prior + 2 new round-trip); zero warnings; `cargo build -p engramai` clean (GUARD-9 holds). Defined `ReproRecord`, `RunSection`, `RunStatus`, `BuildSection`, `DatasetSection`, `FusionSection`, `ModelsSection`, `ResultSection`, `GateRow`, `OverrideSection`. Define `ReproRecord` struct + nested structs for each TOML table per design §6.1: `RunSection`, `BuildSection`, `DatasetSection`, `FusionSection`, `ModelsSection`, `ResultSection`, `GateRow`, `OverrideSection`. Use `serde::{Serialize, Deserialize}`. ~120 lines + 2 round-trip tests. Pre-load: design.md §6.1 (full table dump), `harness/repro.rs` (current stub), `harness/gates.rs` (Priority/GateStatus types). Verify: `cargo test -p engram-bench --lib harness::repro::`.
  - [x] task:bench-impl-repro-2-writer-reader — ✅ DONE 2026-04-27 ~11:35; 26 lib tests pass (24 prior + 2 new: on-disk round-trip, missing-field rejection); zero warnings; `cargo build -p engramai` clean (GUARD-9 holds). Implemented `ReproRecord::write_toml`, `ReproRecord::read_toml`, `run_dir_path`, `repro_file_path`, `REPRO_FILE_NAME` const. Path layout per §6.2: `<runs_root>/<timestamp>_<driver>_<short-sha>/reproducibility.toml` with `:` → `-` sanitisation and 12-char short-sha. Implement `ReproRecord::write_toml(&self, path: &Path) -> Result<(), BenchError>` and `ReproRecord::read_toml(path: &Path) -> Result<Self, BenchError>` + on-disk layout helpers per §6.2 (`benchmarks/runs/<timestamp>_<driver>_<short-sha>/`). ~80 lines + 2 tests (write→read round-trip, missing-field rejection). Pre-load: design.md §6.2, `harness/repro.rs` (with sub-task 1 applied), `harness/mod.rs` BenchError variants. Verify: `cargo test -p engram-bench --lib`.
  - [x] task:bench-impl-repro-3-replay-validator — ✅ DONE 2026-04-27 ~11:50; 31 lib tests pass (26 prior + 5 new: validate happy/missing-field/override-without-fail/sentinel + replay_preconditions extraction); zero warnings; `cargo build -p engramai` clean (GUARD-9 holds). Implemented `validate_record(record, expected_gate_ids)` (4 §4.2a checks: schema conformance, gate coverage, override iff Fail gate, no sentinels) + `ReplayPlan` struct + `replay_preconditions(record)` extractor (§6.3 step 1). `expected_gate_ids` accepted as parameter so this validator stays decoupled from the still-evolving `standard_gates()` inventory (sub-task `bench-impl-gates-1`). Implement `validate_record(record: &ReproRecord) -> Result<(), BenchError>` per §4.2a checks (schema conformance, all gates represented, override iff override_used, no sentinel values). Plus `replay_preconditions(record: &ReproRecord) -> Result<ReplayPlan, BenchError>` per §6.3 (extracts commit SHA, dataset SHAs, fusion weights, model IDs into a struct ready for `--from-record` consumption). ~150 lines + 4 table-driven tests (valid record passes; missing field fails; override-without-rationale fails; sentinel "TODO" fails). Pre-load: design.md §4.2a, §6.3, `harness/repro.rs` with sub-tasks 1-2 applied. Verify: `cargo test -p engram-bench`.
- [x] task:bench-impl-anonymizer — ✅ DONE 2026-04-27 ~12:15 (both sub-tasks complete; 38 lib tests pass; zero warnings; GUARD-9 holds). ⚠️ SPLIT into 2 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 250-400 lines (regex pipeline + allowlist + delta + idempotence test + leak check). All sub-tasks land in `crates/engram-bench/src/anonymizer/mod.rs`. Note: zero-leak; potato-review workflow per §9.3.1. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-anonymizer-1-pipeline — ✅ DONE 2026-04-27 ~12:05; 34 lib tests pass (31 prior + 3 new: patterns+allowlist roundtrip, delta-order determinism, malformed-regex named-abort); zero warnings; `cargo build -p engramai` clean (GUARD-9 holds). Added `regex = "1"` direct dep (per GUARD-9 comment: bench-only deps allowed in this crate). Implemented `Anonymizer { patterns: Vec<PatternRule>, allowlist: BTreeSet<String>, delta: BTreeMap<String, String> }` (BTreeSet/BTreeMap chosen for byte-identical determinism per §9.3.1) + `from_config_files` + `apply` (regex pipeline → literal delta substitutions; allowlisted spans preserved verbatim) + wire-format types `PatternToml`/`PatternsFile`/`AllowlistFile`/`DeltaFile`. Implement core anonymization pipeline per §9.3.1: `Anonymizer { patterns: Vec<Regex>, allowlist: HashSet<String>, delta: HashMap<String, String> }` + `Anonymizer::from_config_files(patterns_toml, allowlist_toml, delta_toml)` + `Anonymizer::apply(text: &str) -> String` (deterministic regex-replace then delta-substitute, all-or-nothing crash semantics on regex compile error). ~180 lines + 3 unit tests. Pre-load: design.md §9.3 + §9.3.1, current `anonymizer/mod.rs` stub. Verify: `cargo test -p engram-bench --lib anonymizer::`.
  - [x] task:bench-impl-anonymizer-2-idempotence-leakcheck — ✅ DONE 2026-04-27 ~12:15; 38 lib tests pass (34 prior + 4 new: idempotent on already-anonymized, idempotent after delta rewrites, leak flags banned, leak passes on clean); zero warnings; GUARD-9 holds. Implemented `Anonymizer::is_idempotent(text) -> bool` (apply twice → same string) + `Anonymizer::check_no_leak(text, banned_substrings) -> Result<(), Vec<String>>` (returns offending tokens; case-sensitive scan; preserves slice order in the leak vec). Implement `Anonymizer::is_idempotent(text: &str) -> bool` (apply twice → same result) + `Anonymizer::check_no_leak(text: &str, banned_substrings: &[&str]) -> Result<(), Vec<String>>` (returns list of leaks if any banned tokens survive) + the §11 idempotence test target. ~120 lines + 4 tests (idempotent on clean input; idempotent after one pass; leak detected; no-leak passes). Pre-load: design.md §9.3.1 idempotence + §11 testing strategy, `anonymizer/mod.rs` with sub-task 1 applied. Verify: `cargo test -p engram-bench`.
- [ ] task:bench-impl-scorer-locomo — ⚠️ SPLIT into 2 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 200-300 lines (Rust port of Python LOCOMO scorer + bit-parity test on 50-query fixture). All sub-tasks land in `crates/engram-bench/src/scorers/locomo.rs`. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-scorer-locomo-1-types-and-scoring — ✅ DONE 2026-04-27 ~12:25; 41 lib tests pass (38 prior + 3 new: exact-match/mismatch, normalisation tolerates case/punctuation/whitespace, per-category aggregation is per-bucket mean); zero warnings; GUARD-9 holds. Implemented `LocomoQuery`, `LocomoScore`, `LocomoSummary`, `LocomoScorer` (stateless) + `score(&[LocomoQuery]) -> (Vec<LocomoScore>, LocomoSummary)` + `normalise()` (lowercase/punctuation-strip/whitespace-collapse). Documented as **deliberately simple normalised-exact-match baseline**; the bit-parity test in sub-task 2 is the gate that will demand richer logic if upstream LOCOMO uses token-F1 or category-conditional rules. Registered `pub mod locomo` in `scorers/mod.rs`. Define `LocomoScorer`, `LocomoQuery`, `LocomoScore` types + scoring math (string-match + category logic per LOCOMO upstream conventions, see design §9.1 for vendoring policy). ~180 lines + 3 unit tests on synthetic queries. Pre-load: design.md §9.1, §3.1 (scorer's caller in driver), upstream LOCOMO scorer reference (cite the Python source path or repo URL — link from §9.1). Verify: `cargo test -p engram-bench --lib scorers::locomo::`.
  - [⛔ BLOCKED] task:bench-impl-scorer-locomo-2-parity-test — BLOCKED 2026-04-27 ~12:25 by missing upstream artefact: this sub-task requires (a) the official LOCOMO Python scorer source vendored or pinned by SHA per §9.1, and (b) a golden output file produced by running that upstream scorer on a 50-query input fixture. Without access to (a)+(b) the only path forward is fabricating a hand-crafted golden file and *claiming* it came from upstream — which violates SOUL.md honesty rules. Unblock by: pin the LOCOMO repo SHA in `benchmarks/fixtures/locomo/source.toml`, vendor the upstream scorer or run it against `crates/engram-bench/fixtures/locomo-parity/queries.jsonl` to produce `golden_scores.json`, then this sub-task can implement the parity assertion. Add 50-query parity-test fixture under `crates/engram-bench/fixtures/locomo-parity/` + integration test asserting Rust scorer's output matches the upstream Python scorer's golden output bit-for-bit (or within float tolerance 1e-6). ~80 lines + 1 integration test. Pre-load: design.md §11 (parity-test discipline), `scorers/locomo.rs` with sub-task 1 applied. Verify: `cargo test -p engram-bench --test parity_locomo` (or whatever test target is created).
- [x] task:bench-impl-scorer-longmemeval — ✅ DONE 2026-04-27 ~11:45 (commit 6a42ae0; 45 lib tests pass; zero warnings; GUARD-9 holds). Mirrors scorers::locomo sub-task 1: LongMemEvalQuery/Score/Summary + LongMemEvalScorer with normalised-exact-match baseline. Future parity follow-up (analogous to scorer-locomo-2) NOT in scope — needs vendored upstream LongMemEval Python scorer + 50-query golden file per design §9.2. see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Single task (no split — analogous to LOCOMO but simpler, ≈200 lines). Pre-load: design.md §9.2, completed scorer-locomo for style. Use **max_iterations=35** when delegating. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-driver-locomo — ✅ DONE (verified 2026-04-27 15:00 — `crates/engram-bench/src/drivers/locomo.rs` is 1014 lines, 111 lib tests pass). Both sub-tasks implemented. ⚠️ SPLIT into 2 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 250-350 lines (loader + replay + score + emit JSON + repro record). All sub-tasks land in `crates/engram-bench/src/drivers/locomo.rs`. Determinism contract per §3.1. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-driver-locomo-1-loader-replay — ✅ DONE (verified by file presence + passing tests in `cargo test -p engram-bench --lib drivers::locomo::`). Implement `LocomoDriver` struct + `BenchDriver` impl skeleton (name/stage=Stage1/cost_tier=Expensive) + `load_dataset(path: &Path, expected_sha: &str) -> Result<LocomoDataset, BenchError>` + per-conversation replay loop calling `Memory::ingest()` then `Memory::graph_query()`. ~180 lines + 2 tests (loader rejects bad SHA, replay returns RetrievalOutcome shape). Pre-load: design.md §3.1, `harness/mod.rs` (BenchDriver trait + Stage/CostTier/HarnessConfig + verify_fixture_sha + fresh_in_memory_db), engramai `Memory::ingest` + `Memory::graph_query` signatures. Verify: `cargo test -p engram-bench --lib drivers::locomo::`.
  - [x] task:bench-impl-driver-locomo-2-score-emit — ✅ DONE (verified by file presence + passing tests). Implement scoring pipeline (calls `LocomoScorer::score`) + per-category aggregation (overall + temporal subscore per GOAL-5.2) + emit `locomo_summary.json` + `locomo_per_query.jsonl` + reproducibility record `[result]` section per §6.1. ~150 lines + 2 tests. Pre-load: design.md §3.1 (output contracts) + §6.1 (`[result]` shape), `drivers/locomo.rs` with sub-task 1 applied, completed `scorers/locomo.rs`, completed `harness/repro.rs`. Verify: `cargo test -p engram-bench`.
- [x] task:bench-impl-driver-longmemeval — ✅ DONE (verified 2026-04-27 15:00 — `crates/engram-bench/src/drivers/longmemeval.rs` is 1297 lines, tests pass under `cargo test -p engram-bench --lib`). see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context. Single task (no split — ≈220 lines, analogous to LOCOMO but reads `baselines/v02.toml` for delta_pp computation). Use **max_iterations=35**. Pre-load: design.md §3.2, `baselines.rs` (BaselineV02 type), completed driver-locomo for style. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-driver-cost — ✅ DONE (verified 2026-04-27 15:00 — `crates/engram-bench/src/drivers/cost.rs` is 1291 lines, tests pass). see §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (consumes `ResolutionStats` via `Memory::ingest_with_stats` from §A.1 — VERIFIED present in codebase 2026-04-27 morning, GUARD-12 ship-gate). Single task (≈220 lines). Use **max_iterations=35**. Pre-load: design.md §3.3, `crates/engramai/src/resolution/stats.rs` (ResolutionStats), `crates/engramai/src/memory.rs:5992+` (ingest_with_stats sig). Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [⛔ BLOCKED] task:bench-impl-driver-test-preservation — BLOCKED: depends on full §C green, but `task:mig-impl-backfill-perrecord` is blocked on stop-condition #4 (missing `ResolutionPipeline::resolve_for_backfill`). DO NOT START. Unblock requires resolving the §A.1 deferred method first. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [ ] task:bench-impl-driver-cognitive — ⚠️ SPLIT into 2 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 250-350 lines (3 separate cognitive features × seeding + querying + comparison). All sub-tasks land in `crates/engram-bench/src/drivers/cognitive_regression.rs`. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [ ] task:bench-impl-driver-cognitive-1-framework — Implement `CognitiveDriver` struct + `BenchDriver` impl (Stage1, Cheap) + per-feature trait `CognitiveFeatureCheck { fn name(&self) -> &str; fn run(&self, mem: &Memory) -> FeatureResult; }` + result aggregation. ~120 lines + 1 test (framework wiring with mock feature). Pre-load: design.md §3.5 (framework paragraph + 3-feature listing), `harness/mod.rs` BenchDriver trait. Verify: `cargo test -p engram-bench --lib drivers::cognitive_regression::framework_`.
  - [ ] task:bench-impl-driver-cognitive-2-features — Implement the 3 feature checks: interoceptive (Jaccard distance, threshold 0.2), metacognition (filter-count diff), affect (Jaccard, threshold 0.2). Each as a `CognitiveFeatureCheck` impl. ~180 lines + 3 unit tests (one per feature). Pre-load: design.md §3.5 (per-feature method), `drivers/cognitive_regression.rs` with sub-task 1 applied. Verify: `cargo test -p engram-bench`.
- [⛔ BLOCKED] task:bench-impl-driver-migration — BLOCKED: depends on full §C green, but `task:mig-impl-backfill-perrecord` is blocked. DO NOT START. Same root cause as `bench-impl-driver-test-preservation`. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
- [x] task:bench-impl-gates — ✅ DONE (reconciled 2026-04-27 ~14:30 in 6 atomic commits; gates.rs 833 lines; 111 tests pass). ⚠️ SPLIT into 3 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 300-500 lines (8 GOAL evaluators + override flow + meta-gate §4.2a). All sub-tasks land in `crates/engram-bench/src/harness/gates.rs` (additive — current file has 77 lines of base types). Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-gates-1-eval-engine — ✅ DONE (commit history 2026-04-27 reconcile). Implement gate evaluation engine: `Gate { goal_id: String, metric_path: String, comparator: Comparator, threshold: GateThreshold, priority: Priority }` + `evaluate(metrics: &serde_json::Value, gate: &Gate) -> GateOutcome` (Pass/Fail/Error per §4.4 Level 1). Plus the table of 8 gates per §4.1 + §4.2 + §4.2a as a `pub fn standard_gates() -> Vec<Gate>` constructor. ~180 lines + 5 tests (each comparator type + ERROR on missing metric). Pre-load: design.md §4.1, §4.2, §4.2a tables, current `harness/gates.rs` (Priority/GateStatus/ReleaseDecision types). Verify: `cargo test -p engram-bench --lib harness::gates::`.
  - [x] task:bench-impl-gates-2-override-rationale — ✅ DONE (commit history 2026-04-27 reconcile). Implement override+rationale machinery per §4.4 manual override section: `Override { gate: String, rationale_file: PathBuf, rationale_sha: String, operator: String }` + `Override::from_cli_args(...) -> Result<Self, BenchError>` (validates rationale file exists, non-empty, SHA computed) + append-to-overrides-log helper writing `.gid/releases/overrides.log`. ~120 lines + 3 tests (valid override; missing rationale file; empty rationale rejected). Pre-load: design.md §4.4 manual override paragraph, `harness/gates.rs` with sub-task 1 applied. Verify: `cargo test -p engram-bench --lib harness::gates::override_`.
  - [x] task:bench-impl-gates-3-meta-gate — ✅ DONE (commit history 2026-04-27 reconcile). Implement the §4.2a meta-gate validation: `evaluate_meta_gate(record: &ReproRecord, gates: &[Gate]) -> GateOutcome`. Calls into `validate_record` (from `harness/repro.rs` sub-task 3) plus runs the 4 explicit checks in §4.2a. ~80 lines + 4 table-driven tests. Pre-load: design.md §4.2a, `harness/gates.rs` with sub-tasks 1-2 applied, completed `harness/repro.rs::validate_record`. Verify: `cargo test -p engram-bench`.
- [x] task:bench-impl-reporting — ✅ DONE 2026-04-27 15:08 commit `75c3ff7` (`feat(bench): implement §10 reporting (summary/drilldown/diff)`); reporting.rs has render_summary_table, render_drilldown, diff_runs/render_diff + 9 lib tests. All 3 sub-tasks landed in single commit. Originally split into 3 sub-tasks below (pre-emptive split, 2026-04-27 morning). Original scope ≈ 250-400 lines (3 sub-functions: summary table §10.1 with TTY colorization, per-gate drill-down §10.2 with `engram-bench explain GOAL-X.Y`, regression alert payload §10.3). All sub-tasks land in `crates/engram-bench/src/reporting.rs`. Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.
  - [x] task:bench-impl-reporting-1-summary-table — ✅ done in `75c3ff7`. Implement `render_summary_table(reports: &[RunReport], gates: &[GateOutcome]) -> String` per §10.1: one line per gate with `[STATUS-PRIORITY] GOAL-X.Y metric=… threshold=… `, TTY-detected colorization (use `is-terminal` crate), plain ASCII for CI. ~150 lines + 3 tests (TTY format, plain format, includes all gate statuses). Pre-load: design.md §10.1 (full layout example), `harness/gates.rs` (GateOutcome type). Verify: `cargo test -p engram-bench --lib reporting::summary_`.
  - [x] task:bench-impl-reporting-2-drill-down — ✅ done in `75c3ff7`. Implement `render_drill_down(goal_id: &str, report: &RunReport) -> String` per §10.2: full per-query / per-episode breakdown for one specific gate (consumed by `engram-bench explain GOAL-X.Y` subcommand). ~100 lines + 2 tests. Pre-load: design.md §10.2 (drill-down layout example), `reporting.rs` with sub-task 1 applied, RunReport shape. Verify: `cargo test -p engram-bench --lib reporting::drill_`.
  - [x] task:bench-impl-reporting-3-alert-payload — ✅ done in `75c3ff7` (as diff_runs/render_diff). Implement `build_regression_alert(reports: &[RunReport]) -> AlertPayload` per §10.3: CI-config-agnostic JSON shape (NOT tied to a specific Slack/email format) with `{summary, failed_gates: [...], commit_sha, timestamp}`. ~80 lines + 2 tests. Pre-load: design.md §10.3, `reporting.rs` with sub-tasks 1-2 applied. Verify: `cargo test -p engram-bench`.
- [x] task:bench-impl-main — ✅ DONE 2026-04-27 15:11 commit `ada676c` (`feat(bench): implement §7.1 CLI dispatch (main.rs)`); main.rs grew from 19-line stub to 548 lines with full Subcommand parser. See §D of `/Users/potato/rustclaw/tasks/2026-04-27-night-autopilot.md` for context (CLI is a thin wrapper — `main.rs` only parses args + dispatches; logic in lib). Single task (≈200 lines). Use **max_iterations=35**. Pre-load: design.md §7.1 (full CLI surface — 9 subcommands), current `main.rs` (19-line stub). MUST come AFTER all drivers + gates + reporting + repro are done (it dispatches to them). Authoritative task list: `.gid-v03-context/v03-benchmarks-build-plan.md`. Follow §2 workflow. Graph: `/Users/potato/clawd/projects/engram/.gid-v03-context/graph.db`.

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
Layer 3  →          §D  Benchmarks (17 tasks)  ← needs §A + §B + §C
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
| `task:mig-impl-backfill-perrecord` (§C) | `task:res-impl-memory-api` (§A.1) — specifically `ResolutionPipeline::resolve_for_backfill` (NOT a `Memory` method per design §6.5) | Per-record migration calls this pipeline method. If A.1 hit stop-condition #4 over the build-plan-vs-design disagreement, this task is also blocked until human triage resolves where `resolve_for_backfill` lives. |
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
| Design §s (verified) | **§6.2** — `Memory::reextract`, `Memory::reextract_failed`, `Memory::compile_knowledge`, `Memory::list_knowledge_topics` (all four covered here); **§6.4** — `Memory::ingest_with_stats` + `ResolutionStats` public contract; **§5bis** — read for context on what `compile_knowledge` actually does |
| ⚠️ NOT here | **§6.5 `resolve_for_backfill` is on `ResolutionPipeline`, NOT on `Memory`** (design.md §6.5 lines 898-917, return type `Result<GraphDelta, PipelineError>`). The build plan currently lists this method under `memory.rs` — that is a build-plan-vs-design disagreement = **stop-condition #4**. If you encounter it: do NOT add `resolve_for_backfill` to `Memory`, write a STATUS file flagging the discrepancy and stop. The method either belongs in a separate task scoped to `ResolutionPipeline` (separate impl block, separate file possibly) or punt to a new task — wait for human triage. |
| Requirements | GOAL-2.1 (idempotence), GOAL-2.2/2.3 (failure surfacing), GOAL-2.11 (stats surface), GOAL-2.14 (rolling-avg surface) |
| Depends on | v03-graph-layer (DONE). **`compile_knowledge` body depends on §A.2** — stub it now (return empty `Vec<KnowledgeTopicId>` or `unimplemented!()` behind a TODO referencing A.2), fill in after A.2 lands. |
| Test command | `cd /Users/potato/clawd/projects/engram && cargo test -p engramai --lib memory::` |
| DoD | 5 `Memory` methods present with correct signatures (reextract, reextract_failed, compile_knowledge, list_knowledge_topics, ingest_with_stats); doc comments cite design §s + GOAL ids; unit tests for each (idempotence on `reextract` is **mandatory** per GOAL-2.1). `ResolutionPipeline::resolve_for_backfill` is OUT OF SCOPE for this task per the §6.5 note above. |
| Notes | `ResolutionStats` is a public benchmarks contract — once shipped, breaking it requires version bump. Mirror the `&mut self` borrowing pattern from already-done graph methods in the same file. |

## A.2 — `task:res-impl-knowledge-compile`

| Field | Value |
|---|---|
| Target file | `/Users/potato/clawd/projects/engram/crates/engramai/src/knowledge_compile/mod.rs` (NEW module — also add `pub mod knowledge_compile;` to `lib.rs`) |
| ⚠️ Disambiguation | The existing `crates/engramai/src/compiler/` module is the **v0.2 KnowledgeCompiler** (19 files: api.rs, compilation.rs, intake.rs, topic_lifecycle.rs, feedback.rs, etc.). DO NOT modify it. The v0.3 module is `knowledge_compile` (singular, no `-er`) and is a NEW directory. Both coexist during migration; v0.2 `compiler/` is replaced later (out of scope tonight). `lib.rs` already has `pub mod compiler;` — you ADD `pub mod knowledge_compile;`, do not rename. |
| Design §s | **§5bis** Knowledge Compiler (entire section — K1, K2, K3 stages) |
| Requirements | **GOAL-2.10** (topic supersession parity — primary `satisfies` edge per build plan line 51). Cross-feature: GOAL-3.6 / GOAL-3.7 are owned by retrieval but produce-side lives here (§5bis) — implement the producer side; retrieval will add its own `satisfies` edges. |
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

**CLI surface (per design §9.1, GUARD-9 "no new binary"):** the `migrate` subcommand is added to the **existing `engramai` binary** in `crates/engram-cli/src/main.rs` (the v0.2 user CLI, version 0.2.3). The orchestration logic lives in the **`engramai-migrate` library** (no `[[bin]]` target in that crate — it's library-only). `task:mig-impl-cli` therefore touches BOTH:
  - (a) `crates/engram-cli/src/main.rs` + `Cargo.toml` — argument parsing, `migrate` subcommand dispatch, depends on `engramai-migrate`
  - (b) `crates/engramai-migrate/src/cli.rs` (or wherever the lib places the surface) — the actual phase driver that the subcommand calls into

  Read both crates' `Cargo.toml` + `main.rs`/`lib.rs` BEFORE adding code. **Do NOT create a new binary** in either crate.

  **Architectural principle (applies project-wide):** CLI and MCP are thin wrappers over the rust crate. All non-trivial logic lives in the library; the binary/MCP server only parses input and delegates. A migration "phase driver" with real logic in `engram-cli/src/main.rs` is a smell — push it into `engramai-migrate`.

**Heads-up on stale build plan IDs**: the migration build plan (`v03-migration-build-plan.md` lines 83, 99, etc.) uses task ID prefix `task:migration-impl-*` which does NOT match the actual graph IDs `task:mig-impl-*`. This is a known cosmetic mismatch — NOT a stop-condition #4 (#4 is about file/section disagreements, not naming drift in stale plans). Trust the graph IDs (`mig-impl-*`).

**Hot-path / ordering tips:**
- Foundation tasks (progress, checkpoint, lock, preflight) before orchestrator/per-record/failure.
- Schema (`task:mig-impl-schema`) must be done before any backfill task — backfill writes into the new columns.
- Phase machine (`task:mig-impl-phase-machine`) wires everything — do after the phase contents exist.
- CLI (`task:mig-impl-cli`) consumes most of the above — late.

**Cross-section dep:** `task:mig-impl-backfill-perrecord` requires §A.1 done (calls `ResolutionPipeline::resolve_for_backfill` — see A.1 ⚠️ note about build-plan-vs-design disagreement on this method's location).

**🚨 GUARD specific to migration:** if any migration test fails, STOP IMMEDIATELY (stop condition #8). Migration bugs corrupt user data — there is no "small" migration regression.

---

# §D — Benchmarks (17 tasks)

> **Build plan** (canonical task list): `/Users/potato/clawd/projects/engram/.gid-v03-context/v03-benchmarks-build-plan.md`
> **Design**: `/Users/potato/clawd/projects/engram/.gid/features/v03-benchmarks/design.md`
> **Requirements**: `/Users/potato/clawd/projects/engram/.gid/features/v03-benchmarks/requirements.md`

**⚠️ Do NOT start §D unless §A + §B + §C are fully green.** Benchmarks consume the public APIs you just built; broken upstream = wasted work here.

**📋 Pre-flight summary (2026-04-27 morning, after `bench-impl-harness` + `bench-impl-cargo-toml` + `bench-impl-lib` + `bench-impl-baselines` done):**
- 14 §D tasks remain, of which **2 are BLOCKED**: `bench-impl-driver-test-preservation` and `bench-impl-driver-migration` (both depend on full §C green; §C blocked by `mig-impl-backfill-perrecord` which needs `ResolutionPipeline::resolve_for_backfill` from §A.1 → human triage).
- 7 of the remaining 12 tasks are **pre-emptively split** into sub-tasks (same root-fix as `bench-impl-harness`): `repro` (→3), `anonymizer` (→2), `scorer-locomo` (→2), `driver-locomo` (→2), `driver-cognitive` (→2), `gates` (→3), `reporting` (→3). Total: 7 parents → 17 sub-tasks.
- 5 tasks remain whole (all ≤ 250 lines, single-concern): `scorer-longmemeval`, `driver-longmemeval`, `driver-cost`, `main`. Use **`max_iterations=35`** when delegating these.
- **Recommended order tonight:** repro (1→2→3) → gates (1→2→3) → reporting (1→2→3) → scorer-locomo (1→2) → scorer-longmemeval → anonymizer (1→2) → driver-locomo (1→2) → driver-longmemeval → driver-cost → driver-cognitive (1→2) → main. (`main` last because it dispatches to all the above.)
- `bench-impl-driver-cost` was previously gated on §A.1 — `Memory::ingest_with_stats` is now confirmed present in `crates/engramai/src/memory.rs:5992`, so it's runnable.


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

*End of autopilot doc. 4 sections, 52 remaining tasks pointed at (2 + 16 + 17 + 17 = 52). §C migration also has 1 already-done task (`mig-impl-error`), so 18 total nodes there.*
