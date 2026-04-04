# Review: requirements-06-state.md (State Management)

> Reviewed: 2026-04-04 | Reviewer: automated | Document: 6 GOALs (3 P0, 2 P1, 1 P2) | Master: 9 GUARDs

---

## 🔴 Critical (blocks implementation)

1. **FINDING-1 [Check #17/#18] GOAL-6.4: Cache max size config parameter missing from GOAL-7.1** — GOAL-6.4 says "configurable maximum size" for the evaluation cache, but GOAL-7.1 (which defines the exhaustive config surface) does not list any `cache_max_size` or equivalent parameter. No default value is specified anywhere. An implementer cannot implement this without knowing: (a) the config field name, (b) the default value, (c) whether `None` means unlimited. **Suggested fix:** Add to GOAL-7.1's parameter list: `eval_cache_max_size: Option<usize>` (default: `None` = unlimited). Or add to GOAL-6.4: "The maximum size is configured via `eval_cache_max_size` in `GEPAConfig` (default: `None`, meaning unlimited — LRU eviction only activates when a limit is set)."

2. **FINDING-2 [Check #9] GOAL-6.4: Cache size unit undefined** — "configurable maximum size" — size in what unit? Number of entries (candidate_id, example_id pairs)? Bytes of memory? This is critical because entries have variable implicit cost (the score is just an f64, so entry count is the natural unit, but this must be stated). **Suggested fix:** Add "maximum number of entries (each entry is one `(candidate_id, example_id) → f64` mapping)" to GOAL-6.4.

3. **FINDING-3 [Check #7] GOAL-6.6: No error handling for corrupt/incomplete delta chain** — GOAL-6.6 says "Full state is reconstructable from the initial checkpoint plus all deltas." What happens if a delta file is missing or corrupt? The requirement doesn't specify: (a) how deltas are numbered/ordered, (b) how to detect a missing delta, (c) what to do if reconstruction fails (fall back to last full checkpoint? error?), (d) how to compact deltas into a new full checkpoint. Without these, incremental checkpointing is unimplementable beyond a trivial append log. **Suggested fix:** Add: "Deltas are numbered sequentially. On load, if any delta is missing or corrupt, the engine falls back to the most recent valid full checkpoint and logs a warning. A full checkpoint is written every `full_checkpoint_interval` (configurable, default: 10) iterations to bound recovery time. Old deltas preceding the most recent full checkpoint may be deleted."

---

## 🟡 Important (should fix before implementation)

4. **FINDING-4 [Check #5] GOAL-6.1: No specification of "functionally identical" semantics** — GOAL-6.1 says round-trip produces "functionally identical state." What does "functionally identical" mean exactly? Bitwise identical JSON? Structural equality (same fields, same values)? Does it permit floating-point rounding differences? Does it require identical HashMap iteration order? This matters for testing. **Suggested fix:** Replace "functionally identical" with "structurally equal: `assert_eq!(original, deserialized)` passes when `GEPAState` derives `PartialEq`. Floating-point scores must be preserved exactly (no rounding). Field ordering in JSON output is not required to be stable, but deserialization must be order-independent."

5. **FINDING-5 [Check #5] GOAL-6.2: Checkpoint file path not specified** — GOAL-6.2 says "a single JSON file written atomically" but doesn't specify: (a) where the file is written (configurable path? current directory?), (b) the file naming convention (e.g., `gepa-checkpoint.json`? `checkpoint-{iteration}.json`?), (c) whether old checkpoints are overwritten or retained. **Suggested fix:** Add: "Checkpoint path is configurable via `GEPAConfig::checkpoint_path: PathBuf` (default: `./gepa-checkpoint.json`). Each checkpoint overwrites the previous one (single file, not one per iteration). The temp file is written to the same directory as the target (to ensure same-filesystem rename)."

6. **FINDING-6 [Check #4] GOAL-6.4: Compound requirement** — GOAL-6.4 packs at least 4 distinct behaviors: (a) configurable max size, (b) LRU eviction with Pareto front pinning, (c) soft-limit boundary condition with warning event, (d) cache hit rate tracking. Each is independently testable and could fail independently. **Suggested fix:** Consider splitting: GOAL-6.4a (LRU eviction with max size + Pareto pinning), GOAL-6.4b (soft-limit overflow with warning event), GOAL-6.4c (cache hit rate tracking in statistics). This is a style issue; the current formulation is implementable but harder to trace in test coverage.

7. **FINDING-7 [Check #10] GOAL-6.4: LRU "recently used" definition ambiguous** — LRU eviction requires a definition of "use." Is a cache entry "used" when: (a) it is written (initial evaluation), (b) it is read (cache hit during dominance check), (c) it is read during re-evaluation backfill lookup, (d) all of the above? The eviction behavior differs significantly depending on the answer. **Suggested fix:** Add: "An entry's LRU timestamp is updated on any read (cache hit) or write. Entries that are frequently involved in dominance comparisons will naturally be retained."

8. **FINDING-8 [Check #7] GOAL-6.2: No specification for what happens when checkpoint write fails** — GOAL-6.2 specifies atomic write (temp + rename) but doesn't say what happens if the write fails (disk full, permissions error). Does the engine halt? Log a warning and continue? Retry? **Suggested fix:** Add: "If checkpoint writing fails, the engine emits a `CheckpointFailed { error }` event and continues the optimization loop. The failure does not halt the run. Consecutive checkpoint failures are logged but do not trigger termination."

9. **FINDING-9 [Check #5] GOAL-6.5: "best score over time" and "Pareto front size over time" data structure unspecified** — These are time-series data. How is "over time" stored? A Vec of (iteration, value) pairs? Only the last N values? What is "best score" when there are multiple objectives (per-example)? Is it the average score of the best candidate? The max score across any example? **Suggested fix:** Clarify: "`best_score_history: Vec<(u64, f64)>` records (iteration, best_average_score) where best_average_score is the highest mean per-example score across all front candidates at that iteration. `front_size_history: Vec<(u64, usize)>` records (iteration, front_size). Both append one entry per completed iteration."

10. **FINDING-10 [Check #8] GOAL-6.5: "acceptance rate" definition ambiguous** — Is acceptance rate: (a) total_accepted / total_generated (all-time), (b) a rolling window, (c) per-iteration (0 or 1)? Stagnation detection (GOAL-1.2b) may depend on this. **Suggested fix:** Clarify: "Acceptance rate is `total_candidates_accepted / total_candidates_generated` (cumulative, all-time ratio as f64)."

11. **FINDING-11 [Check #15] GOAL-6.4 vs GUARD-9: LRU eviction may break determinism** — GUARD-9 requires determinism given the same seed. LRU eviction order depends on access patterns, which are deterministic if the engine is single-threaded and deterministic. However, if LRU timestamps have wall-clock resolution, two entries accessed in the same tick might have the same timestamp, making eviction order non-deterministic. **Suggested fix:** Add: "LRU ordering uses a monotonic logical clock (incrementing counter), not wall-clock time, to ensure deterministic eviction order per GUARD-9."

---

## 🟢 Minor (can fix during implementation)

12. **FINDING-12 [Check #9] GOAL-6.4: No minimum value for cache max size** — What if the user sets cache max size to 1? Or 0? Should config validation reject values below some threshold? **Suggested fix:** Add validation in GOAL-7.3: "eval_cache_max_size, if set, must be ≥ pareto_max_size × minibatch_size (to hold at least one full evaluation round for all front members)."

13. **FINDING-13 [Check #22] Cross-references: GOAL-2.x is imprecise** — The cross-references section says "GOAL-2.x (Pareto Front) — front serialization" but no specific GOAL-2.x addresses front serialization. GOAL-2.1 is about computation, GOAL-2.2 about updates. Front serialization is implicit (Pareto front is part of GEPAState which is serialized by GOAL-6.1). **Suggested fix:** Change "GOAL-2.x (Pareto Front) — front serialization" to "GOAL-2.1 (Pareto Front) — Pareto front data structure (serialized as part of GEPAState)".

14. **FINDING-14 [Check #9] GOAL-6.3: Score type unspecified** — GOAL-6.3 says "(candidate_id, example_id) → score" — score is presumably f64 (matching GOAL-3.5's `Vec<f64>`), but this should be explicit. **Suggested fix:** Change to "(candidate_id, example_id) → f64 score".

15. **FINDING-15 [Check #20] GOAL-6.6: No non-requirement for concurrent checkpoint reads** — Incremental checkpointing might invite questions about reading state while deltas are being written, or parallel runs sharing checkpoint files. A non-requirement statement would prevent scope creep. **Suggested fix:** Add to out-of-scope or as a note: "Concurrent access to checkpoint files from multiple engine instances is not supported."

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | GOAL-6.1 (serialize/deserialize), GOAL-6.2 (checkpoint write), GOAL-6.3 (cache hit), GOAL-6.5 (stats tracking) | - |
| Error handling | GOAL-6.4 boundary condition (soft limit) | Checkpoint write failure (FINDING-8), Delta chain corruption (FINDING-3), Deserialization failure (what if JSON is valid but schema-incompatible?) |
| Performance | GOAL-6.4 (cache eviction to bound memory) | No checkpoint write time budget; no cache lookup latency requirement |
| Security | N/A (library crate, no auth) | N/A — correctly out of scope per master |
| Reliability | GOAL-6.2 (atomic write), GOAL-6.4 (soft limit) | No specification for schema versioning / forward compatibility of checkpoint format |
| Observability | GOAL-6.5 (statistics), GOAL-6.4 (cache hit rate, warning event) | No event for cache eviction count per iteration |
| Scalability | GOAL-6.4 (cache size limit), GOAL-6.6 (incremental checkpoint) | No guidance on expected cache sizes at scale (e.g., 1000 candidates × 1000 examples = 1M entries) |
| Determinism | Indirectly via GUARD-9 | LRU clock type needs clarification (FINDING-11) |
| Data format | GOAL-6.1 (JSON round-trip) | No schema version field in checkpoint JSON for forward/backward compat |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 6 GOALs, well under the 15 limit.
- **Check #1: Specificity** ✅ — 5/6 GOALs are specific enough for independent implementation. GOAL-6.6 is borderline (see FINDING-3). No vague language like "fast", "user-friendly", "robust" detected.
- **Check #2: Testability** ✅ — 5/6 GOALs have clear pass/fail conditions. GOAL-6.1: serialize→deserialize→assert_eq. GOAL-6.2: write checkpoint, kill process mid-write, verify no corruption. GOAL-6.3: evaluate, check cache hit. GOAL-6.4: fill cache, verify eviction order. GOAL-6.5: run iterations, check stats. GOAL-6.6: partially testable but delta chain error recovery is unspecified (FINDING-3).
- **Check #3: Measurability** ✅ — No quantitative performance claims made in this document. Quantitative aspects (checkpoint interval = N) are configurable with defaults. No "low latency" or "fast" language.
- **Check #6: Happy path coverage** ✅ — Normal flows covered: create state → run iterations → checkpoint → resume → cache hits → track stats.
- **Check #11: Internal consistency** ✅ — No contradictions found between the 6 GOALs. GOAL-6.3 (cache) and GOAL-6.4 (cache eviction) are complementary. GOAL-6.1 (full serialization) and GOAL-6.6 (incremental) are explicitly separate (full vs delta).
- **Check #12: Terminology consistency** ✅ — "evaluation cache", "checkpoint", "GEPAState", "candidate_id", "example_id" used consistently throughout. No synonym confusion detected.
- **Check #13: Priority consistency** ✅ — No priority inversions. P0s (6.1, 6.2, 6.3) are independent foundations. P1s (6.4, 6.5) build on P0s. P2 (6.6) builds on P0 (6.2). Correct dependency order.
- **Check #14: Numbering/referencing** ✅ — All cross-references verified: GOAL-1.9 exists in requirements-01-core-engine.md (resumption). GOAL-2.2 exists in requirements-02-pareto-front.md (front update). GOAL-5.6 exists in requirements-05-candidates.md (candidate serialization). GUARD-4 exists in master (atomic writes).
- **Check #15: GUARDs vs GOALs alignment** ✅ (with caveat) — GUARD-4 (atomic checkpoint) aligns with GOAL-6.2. GUARD-9 (determinism) is compatible but needs LRU clock clarification (FINDING-11). GUARD-2 (immutability) not violated. GUARD-7 (memory) is supported by GOAL-6.4 (cache limits). No contradictions found.
- **Check #16: Technology assumptions** ✅ — JSON serialization is explicitly stated and justified (serde/serde_json in allowed dependencies). Atomic file write (temp+rename) is standard POSIX, appropriate for a Rust crate.
- **Check #19: Migration/compatibility** ✅ (N/A) — This is a new crate; no migration needed. However, checkpoint format versioning is noted as a gap in the coverage matrix.
- **Check #21: Unique identifiers** ✅ — GOAL-6.1 through GOAL-6.6, sequential, no gaps, no duplicates.
- **Check #22: Grouping/categorization** ✅ — All 6 GOALs relate to state management. Logical grouping: serialization (6.1), checkpointing (6.2, 6.6), caching (6.3, 6.4), statistics (6.5).
- **Check #23: Dependency graph** ✅ — Implicit but clear: GOAL-6.3 must exist before GOAL-6.4 (eviction requires cache). GOAL-6.2 must exist before GOAL-6.6 (incremental requires base checkpoint). GOAL-6.1 is foundational. No circular dependencies.
- **Check #24: Acceptance criteria** ⚠️ Partial — Acceptance criteria are embedded in each GOAL (e.g., "round-trip produces functionally identical state") rather than being separate. Adequate for implementation but the "functionally identical" phrasing needs tightening (FINDING-4).
- **Check #25: User perspective** ✅ — Requirements are appropriately system-internal (this is infrastructure). User-facing aspects (checkpoint path, config) are handled in GOAL-7.x. No user-facing confusion.
- **Check #26: Success metrics** ✅ — GOAL-6.5 defines observable run statistics. Cache hit rate (GOAL-6.4) is observable. These serve as production success metrics.
- **Check #27: Risk identification** ⚠️ — GOAL-6.4 (LRU with Pareto pinning) is moderately complex but not called out in the master doc's risk section. GOAL-6.6 (incremental checkpointing) is novel and underspecified. Neither is high-risk enough to block implementation, but GOAL-6.6 would benefit from a spike.

---

## Summary

- **Total requirements:** 6 GOALs, 9 GUARDs (from master)
- **Critical:** 3 (FINDING-1, FINDING-2, FINDING-3)
- **Important:** 8 (FINDING-4 through FINDING-11)
- **Minor:** 4 (FINDING-12 through FINDING-15)
- **Coverage gaps:** Checkpoint write failure handling, delta chain error recovery, checkpoint schema versioning, cache size unit, LRU determinism
- **Recommendation:** **Needs fixes first** — the 3 critical findings (missing config parameter, undefined cache size unit, unspecified delta error handling) must be resolved before implementation. The 8 important findings should also be addressed for production quality but won't block a first pass.
- **Estimated implementation clarity:** **Medium** — GOALs 6.1–6.3 are clear enough to implement now. GOAL-6.4 needs the fixes from FINDING-1, 2, 7, 11. GOAL-6.5 needs FINDING-9, 10. GOAL-6.6 needs significant elaboration (FINDING-3).
