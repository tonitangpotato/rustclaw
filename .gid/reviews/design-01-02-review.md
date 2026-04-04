# Design Review: design-01 (Core Engine) & design-02 (Pareto Front)

**Reviewer:** subagent  
**Date:** 2026-04-04  
**Docs reviewed:**  
- `.gid/features/gepa-core/design-01-core-engine.md`  
- `.gid/features/gepa-core/requirements-01-core-engine.md`  
- `.gid/features/gepa-core/design-02-pareto-front.md`  
- `.gid/features/gepa-core/requirements-02-pareto-front.md`  

---

## design-01: Core Engine

### FINDING-1 [🟢] (design-01)
**GOAL:** GOAL-1.0 (happy path: builder → run → result)
**Issue:** Fully covered. Typestate builder (§2.1) ensures compile-time validation. `build()` validates seeds. `run()` performs data-dependent validation before the loop. Two-phase validation matches requirements exactly.
**Fix:** None needed.

### FINDING-2 [🟢] (design-01)
**GOAL:** GOAL-1.1 (full optimization loop)
**Issue:** Fully covered. §2.2 pseudocode shows all 5 steps: Select → Execute → Reflect → Mutate → Evaluate, plus Accept/Reject. Each is described with delegation targets.
**Fix:** None needed.

### FINDING-3 [🟢] (design-01)
**GOAL:** GOAL-1.2a–d (termination conditions)
**Issue:** Fully covered. §2.3 defines `TerminationReason` enum with all 5 variants. Check order is explicit (cancellation first, then max iterations, time budget, too many skips, stagnation). Priority ordering is well-defined.
**Fix:** None needed.

### FINDING-4 [🟡] (design-01)
**GOAL:** GOAL-1.2b (stagnation counter semantics)
**Issue:** §2.2 step 8 says "If rejected, increment stagnation counter" but GOAL-1.2b specifies that stagnation counts only iterations "where a mutation was actually attempted and rejected as dominated." The design doesn't explicitly distinguish between a reject because the child is *dominated* vs. a reject because there's *insufficient shared examples* for dominance. If insufficient data leads to the candidate being "non-dominating" and thus accepted, this is fine. But if the acceptance rule in GOAL-1.7d says a candidate is accepted if non-dominated (which includes insufficient-data cases), then there may be no issue. However, the design should explicitly state that "rejected" means "dominated by at least one front member" and that insufficient-data comparisons don't count as rejection.
**Fix:** Add clarifying note in §2.2 step 8: "A candidate is 'rejected' only when it is dominated by at least one front member (on sufficient shared examples). Candidates that are non-dominating due to insufficient shared data are accepted to the front (per GOAL-1.7d). Thus the stagnation counter only increments on actual dominance-based rejections."

### FINDING-5 [🟡] (design-01)
**GOAL:** GOAL-1.3 (Pareto front selection)
**Issue:** §2.2 step 3 calls `ParetoFront::select(&mut self.rng)` but design-02 §2.3 shows the signature as `select(&mut self, cache: &EvalCache, overfitting_deltas: &HashMap<CandidateId, f64>, rng: &mut StdRng)`. The integration table at the bottom of design-01 also says `ParetoFront::select(&mut rng) -> &Candidate`. The method signature mismatch (missing `cache` and `overfitting_deltas` params) and return type mismatch (`&Candidate` vs `CandidateId`) need alignment.
**Fix:** Update design-01 §2.2 step 3 and the integration table to use the full signature from design-02: `ParetoFront::select(&mut self, &cache, &overfitting_deltas, &mut rng) -> Result<CandidateId, GEPAError>`. Return type is `CandidateId`, not `&Candidate`.

### FINDING-6 [🟡] (design-01)
**GOAL:** GOAL-1.7a (score alignment — evaluate all front members on current minibatch)
**Issue:** §2.2 step 8 mentions "Check if child dominates parent on shared examples" and "insert child into Pareto front; prune dominated members" but does NOT explicitly describe evaluating all front members on the current minibatch. GOAL-1.7a requires: "The engine also evaluates all current front members on the current minibatch (using cached scores where available, only calling evaluate for uncached pairs)." This critical step is absent from the pseudocode. It's only partially implied by step 10 (re-evaluate) which runs at a configurable interval, not every iteration.
**Fix:** Add an explicit sub-step between steps 7 and 8: "7b. **Front backfill** — for each front member, look up cached scores for the current minibatch examples. For uncached (member_id, example_id) pairs, call `adapter.evaluate()` up to `max_re_eval_per_iteration` budget (GOAL-1.7c). Store results in cache." This is distinct from step 10's periodic full re-evaluation.

### FINDING-7 [🟡] (design-01)
**GOAL:** GOAL-1.7c (re-evaluation cost budget)
**Issue:** GOAL-1.7c specifies a `max_re_eval_per_iteration` cap on adapter evaluate calls for front re-evaluation within each iteration. The design doesn't mention this parameter anywhere in the pseudocode or data structures. The `GEPAEngine` struct doesn't show it; the config reference is implicit.
**Fix:** Add `max_re_eval_per_iteration` to the relevant section (§2.2 or as a config field reference) and describe the staleness-based prioritization when the budget is exceeded.

### FINDING-8 [🟢] (design-01)
**GOAL:** GOAL-1.4 (minibatch sampling)
**Issue:** Fully covered. §2.4 `MinibatchSampler` implements epoch-based cycling with seeded RNG. Edge case (batch_size ≥ total examples) is handled. Checkpoint/resume is addressed via serializable cursor/epoch.
**Fix:** None needed.

### FINDING-9 [🟢] (design-01)
**GOAL:** GOAL-1.5, GOAL-1.6 (reflect and mutate steps)
**Issue:** Covered. Steps 5 and 6 in §2.2 describe reflect and mutate with correct delegation. Ancestor lesson gathering via `state.lineage(parent.id)` with max depth is specified.
**Fix:** None needed.

### FINDING-10 [🟢] (design-01)
**GOAL:** GOAL-1.8 (GEPAResult contents)
**Issue:** Not explicitly detailed in §2.2 as a struct definition, but the pseudocode's "After loop exit: evaluate all front candidates on validation set, build GEPAResult" references it. The `TerminationReason` enum is defined. The requirement for "best candidate by average score, ties broken by age" isn't explicitly in the design pseudocode but is a straightforward derivation.
**Fix:** Minor — consider adding a `GEPAResult` struct definition to §2.2 or a new §2.7 showing all fields explicitly. Not blocking.

### FINDING-11 [🟢] (design-01)
**GOAL:** GOAL-1.9 (checkpoint resume)
**Issue:** Covered. `run_from_state()` is in the interface. MinibatchSampler's cursor/epoch are serializable.
**Fix:** None needed.

### FINDING-12 [🟢] (design-01)
**GOAL:** GOAL-1.10 (merge step)
**Issue:** Covered in §2.2 step 11 with correct triggering condition and basic description.
**Fix:** None needed.

### FINDING-13 [🟢] (design-01)
**GOAL:** GOAL-1.11 (cancellation)
**Issue:** Fully covered. §2.6 defines `CancellationToken` with `Arc<AtomicBool>`, three check points documented, cooperative completion of in-flight calls. Matches requirements precisely.
**Fix:** None needed.

### FINDING-14 [🟢] (design-01)
**GOAL:** GOAL-1.12 (seed evaluation at initialization)
**Issue:** Covered. §2.1 mentions seed validation in `build()`. §2.2 pseudocode says "After loop exit" but the requirements text and the state transition diagram show seed evaluation happening before iteration 1. The design overview says the engine "evaluates all seed candidates" is implied by the "Satisfies" list including GOAL-1.12 but the pseudocode should be more explicit.
**Fix:** Minor — add explicit "Step 0: Seed Evaluation" before the loop pseudocode in §2.2 describing: evaluate all seeds on initial minibatch, discard failures, error if all fail, insert successes into Pareto front.

### FINDING-15 [🟡] (design-01)
**GOAL:** GOAL-1.7 (evaluate child on same minibatch)
**Issue:** §2.2 step 7 says "call `adapter.evaluate(&child, &minibatch)`" — the method takes `&child` and `&minibatch`, implying the child (a Candidate) is passed directly. But design-02 and GOAL-6.3 suggest scores are stored by `CandidateId`. The evaluate adapter call needs to return per-example scores. The design mentions "sanitize scores per GUARD-10 (NaN → None, ±Inf → clamped)" — but "NaN → None" implies Option<f64> scores, while the rest of the design (dominance checking, crowding distance) works with plain `f64`. The sanitization contract needs to be clear: does NaN become a missing score (not stored in cache) or a sentinel value?
**Fix:** Clarify score sanitization: "NaN scores are treated as evaluation failures for that example — the (candidate_id, example_id) pair is not stored in the cache. ±Inf scores are clamped to ±f64::MAX." This keeps the cache clean and dominance logic simple (only `f64` values in cache).

### FINDING-16 [🔴] (design-01)
**GOAL:** GOAL-1.7a, GOAL-1.7c (front member evaluation on current minibatch)
**Issue:** This is the most significant gap. GOAL-1.7a explicitly requires that after each iteration's evaluation step, ALL front members are evaluated on the current minibatch (cache-aware). This is essential for dominance comparisons to have sufficient shared examples. The design pseudocode in §2.2 has NO step for this. Step 8 (Accept/Reject) assumes dominance can be checked but doesn't ensure the data exists. Step 10 (Re-evaluate) only runs periodically, not every iteration. Without per-iteration front backfill, the `min_shared_examples` threshold will rarely be met between the new child and existing front members (they may share only the examples from their respective minibatches), causing the front to grow unchecked since all candidates appear "non-dominating" due to insufficient data.
**Fix:** Add a mandatory step between current steps 7 and 8: "7b. Evaluate all front members on the current minibatch (cache-aware, budget-capped per GOAL-1.7c). For each front member, check cache for scores on each example in the current minibatch. Call `adapter.evaluate()` only for uncached pairs, up to `max_re_eval_per_iteration` calls. Store results. This ensures the new child and all front members share at least the current minibatch as common evaluation ground."

### FINDING-17 [🟡] (design-01)
**GOAL:** GUARD-10 (score semantics)
**Issue:** The `MinibatchSampler::next_batch` returns `Vec<String>` for example IDs. The design-02 dominance algorithm refers to sorted example ID lists for O(M) merge. If example IDs are arbitrary strings, sorted merge requires `Ord` on strings which is fine, but the design doesn't specify that example IDs in the cache are maintained sorted. This is a design-02 concern but affects design-01's contract for what it stores in the cache.
**Fix:** Design-01 should specify that when storing scores in the cache (step 8), example IDs are inserted in sorted order, or the cache maintains sorted order internally. Alternatively, this is purely a design-06 (cache) concern — just ensure cross-reference is explicit.

---

## design-02: Pareto Front

### FINDING-18 [🟢] (design-02)
**GOAL:** GOAL-2.1 (Pareto dominance computation)
**Issue:** Fully covered. §2.2 `check_dominance` algorithm is complete: sorted merge for intersection, `min_shared_examples` threshold, early-exit optimization, correct dominance flags. Returns `DominanceResult` enum with all cases.
**Fix:** None needed.

### FINDING-19 [🟢] (design-02)
**GOAL:** GOAL-2.2 (incremental update + full recomputation)
**Issue:** Both operations are clearly specified. `try_insert` handles incremental add+prune. `recompute` handles post-backfill full recheck. Both maintain the front invariant via `debug_assert!`. The distinction between the two triggers (new candidate vs. backfill) is clear.
**Fix:** None needed.

### FINDING-20 [🟡] (design-02)
**GOAL:** GOAL-2.3 (selection with starvation prevention)
**Issue:** The round-robin + overfitting-delta reordering algorithm is specified but the interaction is slightly ambiguous. Step 3 says "advance `selection_cursor` by 1 (wrapping)" and step 5 says "the candidate at the reordered position `selection_cursor % members.len()` is returned." But if members are reordered by overfitting delta each call, the cursor position maps to a different candidate depending on current deltas. This means the "round-robin guarantee" (every member selected at least once per N iterations) depends on the reordering being stable within a round. If deltas change mid-round (unlikely but possible if re-evaluation happens mid-round), a member could be skipped.
**Fix:** Clarify: "The overfitting-delta sort order is computed once at the start of each full round-robin cycle (when cursor wraps to 0) and held constant for that cycle. This ensures every member is visited exactly once per cycle regardless of delta changes." Alternatively, state that the guarantee is approximate and acceptable since deltas change infrequently.

### FINDING-21 [🟢] (design-02)
**GOAL:** GOAL-2.4 (crowding distance pruning)
**Issue:** Fully specified. §2.4 gives the complete NSGA-II crowding distance algorithm: dimension selection with fallback, per-dimension sorting, boundary handling (infinity), normalization, tie-breaking by age then ID. Known limitation at high M is documented with rationale.
**Fix:** None needed.

### FINDING-22 [🟢] (design-02)
**GOAL:** GOAL-2.5 (O(N²·M) complexity)
**Issue:** Covered. §3 provides a full complexity analysis table with typical values and wall-clock estimates. All operations are shown to be negligible relative to adapter calls.
**Fix:** None needed.

### FINDING-23 [🟢] (design-02)
**GOAL:** GOAL-2.6 (serialization)
**Issue:** `ParetoFront` derives `Serialize, Deserialize`. All fields (`Vec<CandidateId>`, `usize`, `usize`, `usize`) are trivially serializable. The requirement that "deserialized front is identical to the original" is satisfied by the derive.
**Fix:** None needed.

### FINDING-24 [🟡] (design-02)
**GOAL:** GOAL-2.3 (select returns candidate for mutation)
**Issue:** The `select` signature returns `Result<CandidateId, GEPAError>`. The caller (design-01 engine) needs to look up the full `Candidate` struct from `GEPAState::candidates` using this ID. This is fine architecturally, but the integration table in design-01 says `ParetoFront::select(&mut rng) -> &Candidate` — a reference to a `Candidate`. This is a return type mismatch. Returning `&Candidate` from the front is impossible since the front only stores `CandidateId`s, not `Candidate`s.
**Fix:** Same as FINDING-5 — update design-01's integration table. The engine resolves `CandidateId → &Candidate` via `state.candidates.get(id)`.

### FINDING-25 [🟡] (design-02)
**GOAL:** GOAL-2.2 (incremental insertion)
**Issue:** In `try_insert`, step 1 says "Check if candidate is dominated by any current member. If yes, reject." But the acceptance rule in GOAL-1.7d says "the new candidate is accepted if it is non-dominated by any existing front member." These are equivalent, but there's a subtle gap: what if the new candidate is dominated by member A but dominates member B? The current algorithm rejects immediately at step 1 if *any* member dominates the new candidate. But this is correct per standard Pareto dominance — if A dominates new, then new is definitively dominated regardless of what new does to B (since A also dominates B or is non-dominated by B). However, this relies on the front invariant that no existing member dominates another. With `min_shared_examples`, this invariant may not hold strictly — member A and B might have insufficient shared data to establish dominance between them, but A has sufficient shared data with the new candidate to dominate it. This is fine — the algorithm is correct.
**Fix:** None needed, but adding a comment noting that the front invariant holds *modulo* `min_shared_examples` gaps would improve clarity.

### FINDING-26 [🟡] (design-02)
**GOAL:** GUARD-9 (determinism)
**Issue:** The `select` method takes `rng: &mut StdRng` but the current algorithm description doesn't actually use randomness — it's purely cursor-based with deterministic sorting. The `rng` parameter is accepted but unused. This is not a bug (future-proofing is fine) but should be documented to avoid confusion.
**Fix:** Add a note: "`rng` is accepted for future use (e.g., random tie-breaking) but current selection is fully deterministic given the cursor state and overfitting deltas. The parameter ensures the API doesn't need to change if randomness is added later."

### FINDING-27 [🟡] (design-02)
**GOAL:** GOAL-2.4 (crowding distance dimension selection fallback)
**Issue:** The fallback when no universal intersection exists is "examples shared by the most members." This is underspecified. Does this mean: (a) pick the set of examples shared by the largest subset of members, or (b) pick examples that appear in the most individual members' evaluations? Option (a) is an NP-hard set cover variant; option (b) is simpler (sort examples by membership count, take top-K). The algorithm should specify which.
**Fix:** Clarify: "Fallback: rank examples by the number of front members that have been evaluated on them (descending). Use the top-K examples where K = min_shared_examples or all examples with coverage ≥ 2 members, whichever is larger. Members without scores on the selected examples are excluded from crowding distance computation for those dimensions."

---

## Summary

### design-01 (Core Engine)
| Severity | Count |
|----------|-------|
| 🔴 Critical | 1 |
| 🟡 Moderate | 5 |
| 🟢 Good | 11 |

**Critical issue:** GOAL-1.7a/1.7c — front member evaluation on the current minibatch is completely missing from the loop pseudocode. This is a core algorithmic requirement without which the Pareto front will degenerate (insufficient shared examples → everything accepted → unbounded front growth → crowding-distance pruning with sparse data). Must be added.

**Overall:** The design is thorough and well-structured. The typestate builder, termination logic, error handling, and cancellation are all excellent. The main gap is the per-iteration front backfill step which is a significant algorithmic omission. Several integration point signatures need alignment with design-02.

### design-02 (Pareto Front)
| Severity | Count |
|----------|-------|
| 🔴 Critical | 0 |
| 🟡 Moderate | 4 |
| 🟢 Good | 6 |

**No critical issues.** The Pareto front design is solid — dominance, insertion, recomputation, selection, and pruning algorithms are all complete and well-specified. The moderate findings are mostly about clarifying edge cases (selection round-robin stability, crowding distance fallback specifics, unused rng parameter). All 6 GOALs (2.1–2.6) have corresponding design mechanisms.

### Cross-doc Consistency
- **Signature mismatch** (FINDING-5/24): `select()` signature differs between design-01 and design-02. Design-02 is authoritative (it defines the type). Design-01 must update.
- **Return type mismatch**: design-01 integration table says `&Candidate`, design-02 returns `CandidateId`. The front only stores IDs, so `CandidateId` is correct.
- **`try_insert` signature**: design-01 says `ParetoFront::try_insert(candidate, &cache) -> bool`, design-02 shows `try_insert(&mut self, candidate: CandidateId, cache: &EvalCache) -> bool`. These are consistent in spirit; design-01 should use the full signature.
