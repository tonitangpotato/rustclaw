# Design Review: gepa-core Master Design

**Reviewer:** Subagent (automated)
**Date:** 2026-04-04
**Document:** `.gid/features/gepa-core/design.md`
**Against:** `.gid/features/gepa-core/requirements-master.md`

---

## Checklist Results

### FINDING-1 [🟢] ✅ Applied
**Check:** Does §3 cross-cutting concerns cover GUARD-1 (Pareto front invariant)?
**Issue:** GUARD-1 requires that no candidate in the front is dominated by any other, verified via `debug_assert!` after every update. §3 does not explicitly mention GUARD-1, but §5 step 9 references GOAL-2.2 (dominance pruning) and §4 references GOAL-2.2 for the Pareto front feature. The design defers Pareto front details to the per-feature design doc (feature 2).
**Fix:** No fix needed — GUARD-1 is structural to the Pareto front feature and appropriately deferred. However, a brief mention in §3 cross-cutting concerns would improve traceability. Consider adding a "§3.x Invariant Enforcement" subsection noting that `debug_assert!` checks guard critical invariants (GUARD-1, GUARD-2).

### FINDING-2 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-2 (candidate immutability)?
**Issue:** §3.4 states "Candidate is not generic — stores text parameters as `HashMap<String, String>`" and the trade-off table mentions "Keeps candidates immutable (GUARD-2)." §6 type definition shows `Candidate` with `pub` fields which could be mutated. However, §3.7 describes Candidate as "immutable value type."
**Fix:** The `Candidate` struct in §6 has `pub` fields. The feature design should clarify enforcement: either make fields private with getters, or document that immutability is enforced by convention (candidates consumed by move, never &mut). This is a concern for the feature design doc, not a master design gap.

### FINDING-3 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-3 (adapter call order)?
**Issue:** §5 data flow explicitly shows the call order: select → execute → reflect → mutate → evaluate → accept. The architecture data flow diagram in §2 also shows this sequence. GUARD-3 is well-covered.
**Fix:** None needed.

### FINDING-4 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-4 (atomic checkpoint writes)?
**Issue:** §5 step 11 explicitly mentions: "Serialize GEPAState → temp file → rename (GUARD-4)." This directly addresses the atomic write requirement.
**Fix:** None needed.

### FINDING-5 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-5 (no direct LLM calls)?
**Issue:** §1 overview states "It owns zero LLM dependencies (GUARD-5)." §7 Dependency Choices has an explicit "Excluded" line: "HTTP clients, LLM SDKs, database drivers (GUARD-5)." Well-covered.
**Fix:** None needed.

### FINDING-6 [🟡] ✅ Applied
**Check:** Does §3 cover GUARD-6 (engine overhead < 5%)?
**Issue:** GUARD-6 requires engine-internal computation to add <5% overhead relative to adapter call time. The design trade-off table mentions "vtable negligible vs LLM latency" and the concurrency choice is "sequential single-threaded loop." However, there is no explicit design mechanism to **measure** or **enforce** this guard. There's no mention of benchmarking, profiling hooks, or performance budgets.
**Fix:** Add a note in §3 or §5 acknowledging GUARD-6 and the intended enforcement strategy: "Engine overhead is expected to be negligible since all heavy computation (LLM calls) is in the adapter. GUARD-6 compliance will be validated via benchmarks comparing engine-internal time vs total iteration time." This could also be a test requirement.

### FINDING-7 [🟡] ✅ Applied
**Check:** Does §3 cover GUARD-7 (linear memory growth)?
**Issue:** The trade-off table mentions "lightweight (GUARD-7)" for the eval cache design decision, and §3.4 mentions concrete types. However, there's no explicit analysis of memory growth characteristics. The eval cache stores per-candidate-per-example scores — if all candidates are evaluated on all examples, this is O(candidates × examples), which is quadratic in total work. The GUARD specifically says "linearly with candidates," which requires that eval cache entries per candidate are bounded.
**Fix:** Add a brief memory analysis to §3 or delegate to the State feature design: "Memory per candidate is O(parameters + evaluated_examples). With minibatch-based evaluation, each candidate is evaluated on O(minibatch_size × re_eval_rounds) examples, not all examples. Total memory is O(candidates × max_evals_per_candidate), which is linear in candidates for fixed config." This would make GUARD-7 compliance explicit.

### FINDING-8 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-8 (Debug/Error/Display, no unwrap)?
**Issue:** §3.1 shows `GEPAError` deriving `Debug` and using `thiserror` for `Error + Display`. The error handling section states "No `.unwrap()` in library code (GUARD-8)." Well-covered.
**Fix:** None needed.

### FINDING-9 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-9 (determinism)?
**Issue:** §3.3 is entirely dedicated to determinism. It specifies ChaCha8Rng, single seeded instance, fixed call order, no cloning/sharing, seed recording when None. Excellently covered.
**Fix:** None needed.

### FINDING-10 [🟡] ✅ Applied
**Check:** Does §3 cover GUARD-10 (score semantics)?
**Issue:** GUARD-10 is a hard guard requiring: higher-is-better, NaN→None, ±Inf→clamp, warning event on Inf. §5 step 8 mentions "Sanitize per GUARD-10 (NaN→None, ±Inf→clamp)." However, there is no dedicated §3 subsection for score semantics, and the NaN/Inf handling is only mentioned in one step of the iteration flow. The "higher is better" convention is not explicitly stated anywhere in the design doc.
**Fix:** Add "§3.x Score Semantics (GUARD-10)" to cross-cutting concerns, explicitly stating: (1) higher is better everywhere, (2) NaN→None (unevaluated), (3) ±Inf→clamp to f64::MAX/MIN + warning event, (4) all comparisons use this convention. This ensures implementers of ALL features (Pareto, eval cache, statistics) apply consistent score handling.

### FINDING-11 [🟢] ✅ Applied
**Check:** Does §3 cover GUARD-11 (Send+Sync, async compatibility)?
**Issue:** §3.2 covers async design and §3.7 is entirely dedicated to Send+Sync. Both explicitly reference GUARD-11. Well-covered.
**Fix:** None needed.

### FINDING-12 [🟢] ✅ Applied
**Check:** Is §5 iteration flow complete?
**Issue:** All 5 GEPA steps are present (select, execute, reflect, mutate, evaluate/accept). Additional steps are covered: time gate, minibatch sampling, backfill, checkpoint, stagnation check, events. The flow maps to all GOAL-1.x requirements. Final validation (GOAL-8.6) is mentioned at loop termination. Cancellation (GOAL-3.7/GOAL-1.2c) is covered via time gate and termination conditions.
**Fix:** None needed.

### FINDING-13 [🟡] ✅ Applied
**Check:** Is §5 missing cancellation check?
**Issue:** §5 step 1 checks time budget, but there's no explicit cancellation token check. GOAL-1.2c and GOAL-3.7 require cancellation support. The design mentions `Cancelled` as a termination reason and `GEPAError::Cancelled` exists, but §5 doesn't show WHERE in the iteration loop the cancellation token is checked.
**Fix:** Add a cancellation check to §5, either at step 1 (alongside time gate) or as a separate step: "Check cancellation token (GOAL-3.7). If cancelled, terminate with `Cancelled`." Also clarify whether cancellation is checked once per iteration or before each adapter call.

### FINDING-14 [🟢] ✅ Applied
**Check:** Are §6 type definitions internally consistent?
**Issue:** Types are consistent:
- `CandidateId = u64` matches `Candidate.id: CandidateId` and `parent_id: Option<CandidateId>`
- `ExampleId = String` matches `Example.id: ExampleId` and `ExecutionTrace.example_id: ExampleId`
- `TerminationReason` enum matches GOAL-1.2d (MaxIterations, TimeBudget, Stagnation, TooManySkips, Cancelled)
- `GEPAResult` includes pareto_front, validation_scores, best_candidate, termination_reason, statistics, state
- `GEPAEngineBuilder` pattern matches GOAL-1.0
**Fix:** None needed.

### FINDING-15 [🟡] ✅ Applied
**Check:** Does `Candidate` in §6 have all fields required by requirements?
**Issue:** The `Candidate` struct has: id, parameters, parent_id, generation, reflection, created_at. Requirements GOAL-5.1 mentions "lineage metadata" and GOAL-4.2 mentions "ancestor lesson chain." The design's `Candidate` has `reflection: Option<String>` but no `lesson` or `lessons` field. The ancestor lesson chain (GOAL-4.2b) requires walking lineage and collecting lessons — but if `reflection` is the only field, how are "lessons" (distilled from reflection) stored?
**Fix:** Clarify in the design whether `reflection` serves as the lesson (and the lesson chain is built by collecting `reflection` from ancestors), or whether a separate `lesson: Option<String>` field is needed on `Candidate`. The requirements mention "accumulated lessons from all ancestors" as input to mutate — the design should explicitly state how this is materialized.

### FINDING-16 [🟢] ✅ Applied
**Check:** Does `GEPAResult` cover GOAL-1.3 (return best Pareto front)?
**Issue:** `GEPAResult` includes `pareto_front: Vec<Candidate>`, `best_candidate: Candidate`, `validation_scores`, `termination_reason`, `statistics`, and `state` for resume. Well-covered.
**Fix:** None needed.

### FINDING-17 [🟢] ✅ Applied
**Check:** Does the adapter trait in §3.2 match GOAL-3.x requirements?
**Issue:** The trait has: `execute`, `reflect`, `mutate`, `evaluate`, `merge` (with default). This matches the 5-step algorithm plus optional merge (GOAL-7.7). All methods are async, return Result, and take appropriate parameters. The `merge` default returns Err, matching "optional" semantics.
**Fix:** None needed.

### FINDING-18 [🟡] ✅ Applied
**Check:** Does the design address GOAL-6.3 (eval cache with sparse score matrix)?
**Issue:** §1 trade-off table mentions "Separate eval cache" and §2 architecture shows "EvalCache(6.3)." §4 feature index mentions "eval cache (GOAL-6.3)." However, the master design doesn't show the EvalCache type signature or its interaction with dominance comparisons. The eval cache is critical for correctness (GUARD-1 depends on accurate score comparisons), yet its design is only implied.
**Fix:** This is appropriately deferred to the feature-06 design doc. No master design change needed, but ensure the feature-06 design doc exists and covers the sparse matrix structure, intersection-based lookups, and re-evaluation backfill integration.

### FINDING-19 [🟢] ✅ Applied
**Check:** Does §5 cover the backfill/re-evaluation mechanism (GOAL-8.5a-c)?
**Issue:** §5 step 10 explicitly covers: evaluate sparsest-coverage candidates on unseen examples (8.5a), recompute dominance (8.5b), compute overfitting deltas (8.5c). Well-covered.
**Fix:** None needed.

### FINDING-20 [🟢] ✅ Applied
**Check:** Does §7 dependency list match requirements-master allowed dependencies?
**Issue:** Design deps: serde+serde_json, thiserror, tracing, rand, rand_chacha, async-trait, tokio (time feature). Requirements allow: serde+serde_json, tokio, async-trait, tracing, rand (with explicit PRNG), thiserror. Perfect match. `rand_chacha` is the explicit PRNG algorithm required by the requirements.
**Fix:** None needed.

---

## Summary

| Severity | Count | Findings |
|----------|-------|----------|
| 🔴 Critical | 0 | — |
| 🟡 Moderate | 6 | #6, #7, #10, #13, #15, #18 |
| 🟢 Good | 14 | #1, #2, #3, #4, #5, #8, #9, #11, #12, #14, #16, #17, #19, #20 |

**Overall Assessment:** The master design document is **solid and well-structured**. All 11 GUARDs have corresponding design mechanisms, and the iteration flow is complete. The architecture and type definitions are internally consistent.

The 6 moderate findings are all about **missing explicitness** rather than missing functionality:
1. **GUARD-6 (performance)** — No measurement/enforcement strategy documented
2. **GUARD-7 (memory)** — No explicit memory growth analysis
3. **GUARD-10 (score semantics)** — Should be a dedicated cross-cutting concern section, not just an in-flow mention
4. **Cancellation** — Not shown in §5 iteration flow despite being a termination condition
5. **Candidate lessons field** — Unclear how ancestor lesson chain is materialized
6. **EvalCache type** — Appropriately deferred but worth confirming feature-06 design covers it

None of these are blocking — they're all addressable in feature design docs or minor additions to the master design. The document is ready for implementation with these clarifications tracked.
