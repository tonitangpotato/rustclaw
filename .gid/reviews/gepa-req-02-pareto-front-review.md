# Review: requirements-02-pareto-front.md

> Reviewed: 2026-04-04 | Reviewer: Requirements Review Skill | Document: 6 GOALs (3 P0, 3 P1)

---

## 🔴 Critical (blocks implementation)

1. **FINDING-1 [Check #4] GOAL-2.2: Compound requirement — update logic + re-evaluation recomputation are two distinct behaviors**
   
   GOAL-2.2 describes two separate behaviors: (a) incremental front update when a new candidate is accepted, and (b) full front recomputation after re-evaluation backfill. These have different triggers, different algorithmic approaches (incremental add+prune vs. full pairwise recheck), and would be tested independently. Combining them makes it ambiguous which part failed if a test breaks.
   
   **Suggested fix:** Split into GOAL-2.2a (incremental update on candidate acceptance) and GOAL-2.2b (full recomputation after re-evaluation backfill). Each gets its own trigger, behavior, and outcome description.

2. **FINDING-2 [Check #4] GOAL-2.3: Compound requirement — selection strategy + starvation prevention are independent concerns**
   
   GOAL-2.3 combines: (a) selection must vary across front members, (b) MAY use overfitting delta as secondary signal, (c) MUST NOT remove based on re-evaluation alone, (d) starvation prevention with round-robin floor. These are four independently testable behaviors. An implementer needs to reason about each separately.
   
   **Suggested fix:** Split into GOAL-2.3a (selection diversity — strategy must vary across front members) and GOAL-2.3b (overfitting delta deprioritization — bounded by round-robin floor, MUST NOT remove). Alternatively, keep as one GOAL but add explicit sub-requirements labeled (a)-(d) with individual test criteria.

3. **FINDING-3 [Check #9] GOAL-2.1: Boundary condition — score type and range undefined**
   
   GOAL-2.1 defines dominance via "A scores ≥ B on every shared example and strictly > on at least one" but never specifies the score type or range. Is a score an `f64`? `f32`? `i64`? What range? What about NaN, infinity, or negative scores? Floating-point equality (≥) for scores is dangerous — does ≥ mean exact floating-point comparison, or within epsilon? Two engineers would implement this differently.
   
   **Suggested fix:** Specify: "Scores are `f64` values in the range [0.0, 1.0]. Dominance comparison uses exact floating-point ordering (`>=` and `>`). NaN scores are treated as evaluation failures and excluded from dominance comparison (the example is treated as unevaluated for that candidate). Scores outside [0.0, 1.0] are an adapter error." Alternatively, if score type is defined elsewhere (GOAL-6.3), add an explicit cross-reference stating the type.

---

## 🟡 Important (should fix before implementation)

4. **FINDING-4 [Check #5] GOAL-2.3: Missing actor/trigger specification**
   
   GOAL-2.3 says "Pareto front selection returns a candidate for mutation" but doesn't specify: who calls it? When? The trigger is implicit (the Select step of the engine loop, per GOAL-1.2 presumably), but not stated. An implementer would need to trace back to the core engine requirements to understand when this is invoked.
   
   **Suggested fix:** Add: "Triggered by the engine's Select step (GOAL-1.2) at the beginning of each iteration."

5. **FINDING-5 [Check #1] GOAL-2.3: Vague selection strategy — "must vary across front members" is underspecified**
   
   "The selection strategy must not always pick the same candidate — it should vary across front members to ensure diversity of exploration" is too vague. What distribution? Uniform random? Weighted? Round-robin? Two engineers could implement this very differently. The starvation prevention clause implies round-robin with reordering, but the primary selection mechanism is unspecified.
   
   **Suggested fix:** Specify the concrete algorithm: "Selection uses shuffled round-robin: each 'round' of `pareto_front.len()` iterations selects each front member exactly once. Within each round, the order is randomized (using the seeded RNG per GUARD-9), with overfitting delta used to influence but not determine the order. Candidates with lower overfitting delta are more likely to be selected earlier in the round."

6. **FINDING-6 [Check #7] GOAL-2.2: No error handling for re-evaluation failures**
   
   GOAL-2.2 describes front recomputation after re-evaluation backfill, but what happens if the re-evaluation partially fails (some candidates evaluated, some errored)? Does the front recompute with partial data? Does it abort the recomputation? This is especially important because re-evaluation calls the adapter's `evaluate` which can fail.
   
   **Suggested fix:** Add: "If re-evaluation backfill (GOAL-8.5) partially fails, front recomputation proceeds with whatever new scores were successfully obtained. Failed evaluations do not modify the evaluation cache; the front is recomputed using the same rules as if fewer new scores were added."

7. **FINDING-7 [Check #9] GOAL-2.4: Boundary condition — crowding distance with ≤2 candidates**
   
   Crowding distance requires at least 3 candidates to be meaningful (the extreme candidates on each dimension get infinite distance). What happens when the front has exactly `max_size + 1` candidates and multiple have infinite crowding distance? Also, what happens when all candidates have the same scores on all shared examples (all crowding distances are 0)?
   
   **Suggested fix:** Add: "Candidates at the extreme positions on any dimension have infinite crowding distance and are never pruned by crowding distance alone. If multiple candidates have equal crowding distance (including all-infinite or all-zero), the age-based tie-breaker applies (oldest removed first). When the front has ≤2 candidates, crowding distance pruning is not applicable (the front is already within any reasonable max_size)."

8. **FINDING-8 [Check #9] GOAL-2.4: Boundary condition — crowding distance dimensions undefined**
   
   GOAL-2.4 says crowding distance uses M (number of examples), meaning each example's score is treated as a separate dimension. This is clarified parenthetically but should be explicit. Also: crowding distance is computed on which examples? The intersection of all front members? Each candidate's own evaluated set? The superset? This matters enormously for the algorithm.
   
   **Suggested fix:** Clarify: "Crowding distance is computed over the union of all examples that any front member has been evaluated on. For candidates missing a score on a particular example, that dimension is excluded from their crowding distance calculation (only dimensions where a candidate has a score contribute to its crowding distance)." OR "Crowding distance is computed only over examples that ALL front members have been evaluated on (the intersection)." Pick one — these give very different pruning behavior.

9. **FINDING-9 [Check #15] GOAL-2.2 vs GUARD-1: Invariant verification timing during recomputation**
   
   GUARD-1 states "After every update (add, remove, re-evaluate), this invariant is verified in debug builds via `debug_assert!`." GOAL-2.2's re-evaluation recomputation involves iteratively removing dominated candidates. Should the invariant be checked after each individual removal, or only after the full recomputation pass? Checking after each removal could be O(N³·M) in debug builds.
   
   **Suggested fix:** Clarify in GOAL-2.2: "After the full recomputation pass (not after each individual removal), the no-dominated-candidate invariant is verified per GUARD-1."

10. **FINDING-10 [Check #18] GOAL-2.4: Crowding distance data requirements — M dimensions for sorting**
    
    GOAL-2.4 says O(N·M·log M) complexity but M is "number of examples per minibatch" — actually crowding distance sorts on each dimension (each example), so it's O(M·N·log N), not O(N·M·log M). You sort N candidates on each of M dimensions. This is a factual error in the complexity description.
    
    **Suggested fix:** Change to: "O(M·N·log N) where M is the number of dimensions (examples) and N is the front size — for each of M dimensions, sort the N candidates."

11. **FINDING-11 [Check #20] Missing explicit non-requirements**
    
    The document doesn't state what the Pareto front does NOT do. Relevant non-requirements to state explicitly:
    - No multi-front support (single Pareto front per engine run)
    - No objective weighting (all examples are equally weighted dimensions)
    - No archive of dominated candidates (once removed, they're gone from the front — though they remain in candidate history per GOAL-6.x)
    - No dynamic objective rebalancing
    
    **Suggested fix:** Add a "## Non-requirements" section listing these explicitly.

---

## 🟢 Minor (can fix during implementation)

12. **FINDING-12 [Check #12] Terminology: "front members" vs "candidates in the front"**
    
    The document uses "front members", "existing candidates", "candidates in the front", and "Pareto front members" interchangeably. While clear in context, standardizing on one term would improve readability. GOAL-2.3 uses "front members" and "front member" consistently; GOAL-2.2 mixes "existing candidates" and "candidates."

13. **FINDING-13 [Check #22] Cross-reference section references GOAL-1.7a-d as a range**
    
    The cross-references section lists "GOAL-1.7a-d (Core Engine)" as a range. This works but is less precise — explicitly listing each one (GOAL-1.7a, GOAL-1.7b, GOAL-1.7c, GOAL-1.7d) with their specific relevance would improve traceability.

14. **FINDING-14 [Check #25] GOAL-2.6 is system-internal**
    
    GOAL-2.6 (serialization) is written from the system perspective ("The Pareto front is serializable"). For a library crate, this is appropriate, but it could benefit from the user perspective: "Consumers can checkpoint and resume engine runs; the Pareto front is included in checkpoint data and round-trips perfectly through serde."

15. **FINDING-15 [Check #26] No success metrics beyond tests**
    
    No observable production metrics are defined for the Pareto front. Useful metrics: front size over time, pruning frequency, selection distribution entropy, average crowding distance. These would help consumers understand if the Pareto front is working effectively. This may be covered by GOAL-9.x events, but it's not stated here.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path — compute front | GOAL-2.1 | - |
| Happy path — update front | GOAL-2.2 | - |
| Happy path — select from front | GOAL-2.3 | - |
| Happy path — prune oversized front | GOAL-2.4 | - |
| Happy path — serialize/deserialize | GOAL-2.6 | - |
| Performance | GOAL-2.5 | No memory usage requirement for front data structure |
| Error handling — re-eval failures | - | ⚠️ FINDING-6: What if re-evaluation partially fails? |
| Error handling — empty front | - | ⚠️ What does GOAL-2.3 (select) do when front is empty? (Presumably impossible if seed candidate exists, but not stated) |
| Error handling — all candidates tied | - | ⚠️ What if all candidates have identical scores? (Crowding distance all 0, round-robin works, but worth stating) |
| Boundary — score type/range | - | ⚠️ FINDING-3: Score type, NaN, infinity undefined |
| Boundary — front size = 0 or 1 | - | ⚠️ What if front has 0 or 1 candidates? Selection from size-1 front? |
| Boundary — min_shared_examples = 0 | - | ⚠️ What if configured to 0? All pairs dominating? |
| Boundary — crowding distance edge cases | - | ⚠️ FINDING-7, FINDING-8 |
| Security | N/A (library crate, no I/O) | Correctly out of scope |
| Reliability — front invariant | GUARD-1 (via master) | - |
| Observability | Implied via GOAL-9.x events | No front-specific metrics defined (FINDING-15) |
| Scalability | GOAL-2.5 (soft perf target) | No memory bound for front |
| Determinism | Implied via GUARD-9 | GOAL-2.3 selection must use seeded RNG — stated indirectly |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 6 GOALs, well under the 15-GOAL limit.
- **Check #1: Specificity** — 4/6 pass. GOAL-2.1 is precise on dominance definition. GOAL-2.4 is precise on crowding distance. GOAL-2.5 specifies complexity. GOAL-2.6 is precise. GOAL-2.2 is mostly precise but mixes two behaviors. GOAL-2.3 has vague selection strategy (FINDING-5). **Partial pass: 4/6.**
- **Check #2: Testability** ✅ — 6/6 GOALs have testable conditions. GOAL-2.1: construct candidates with known scores, verify front computation. GOAL-2.2: add candidate, verify dominated removed. GOAL-2.3: run selection N times, verify all members selected within window. GOAL-2.4: exceed max size, verify smallest crowding distance removed. GOAL-2.5: benchmark N=100, M=200 front computation. GOAL-2.6: serialize, deserialize, assert equality.
- **Check #3: Measurability** ✅ — GOAL-2.5 has concrete numbers (N≤100, M≤200, ~10ms). GOAL-2.4 has concrete default (50). GOAL-2.3 has concrete starvation bound (`pareto_max_size` iterations).
- **Check #5: Completeness (actor/trigger/behavior/outcome)** — 5/6 pass. GOAL-2.3 missing explicit trigger (FINDING-4). All others have clear trigger, behavior, and outcome.
- **Check #6: Happy path coverage** ✅ — All normal flows covered: compute front, update front, select from front, prune when oversized, serialize/resume. Verified by tracing: engine starts → seed candidate on front → iterations produce new candidates → front updates → selection picks next parent → front grows/shrinks → checkpoint saves front.
- **Check #10: State transitions** ✅ — Front states: empty → has seed → grows (early iterations, sparse scores) → stabilizes (re-evaluation fills matrix, dominance detectable) → possibly shrinks → checkpointed. All transitions covered. No dead-end states. Empty front shouldn't occur (seed candidate always present per GOAL-1.1 presumably).
- **Check #11: Internal consistency** ✅ — Verified all 15 GOAL pairs (6 choose 2). No contradictions found. GOAL-2.1's dominance definition is consistent with GOAL-2.2's update rule. GOAL-2.3's "MUST NOT remove" is consistent with GOAL-2.2 being the sole removal mechanism. GOAL-2.4's pruning supplements (doesn't contradict) GOAL-2.2's dominance-based removal. GOAL-2.5's performance target doesn't conflict with GOAL-2.1's algorithm. GOAL-2.6's serialization is independent.
- **Check #12: Terminology consistency** — Minor issue (FINDING-12) but no semantic ambiguity. "Front", "Pareto front", "front members" all clearly refer to the same thing.
- **Check #13: Priority consistency** ✅ — P0 items (GOAL-2.1, 2.2, 2.3) are foundational. P1 items (GOAL-2.4, 2.5, 2.6) build on them. No priority inversions. GOAL-2.3 depends on GOAL-2.1 (both P0) ✅. GOAL-2.4 depends on GOAL-2.1 (P1 depends on P0) ✅.
- **Check #14: Numbering/referencing** ✅ — All cross-references resolve: GOAL-6.3 exists in requirements-06-state.md ✅. GOAL-7.1 exists in requirements-07-config.md ✅. GOAL-7.2 exists in requirements-07-config.md ✅. GOAL-8.5 exists in requirements-08-data-loading.md ✅. GOAL-1.7a-d all exist in requirements-01-core-engine.md ✅.
- **Check #15: GUARDs vs GOALs alignment** ✅ — GUARD-1 (no dominated candidate in front) is upheld by GOAL-2.1 (definition) and GOAL-2.2 (update maintains invariant). GUARD-2 (candidates immutable) — Pareto front operations don't modify candidates, only the front membership. GUARD-3 (no adapter calls during front maintenance) — Pareto operations use cached scores from evaluation cache, no adapter calls. GUARD-5 (no LLM calls) — front operations are purely algorithmic. GUARD-6 (engine overhead <5%) — GOAL-2.5 addresses this. GUARD-9 (determinism) — GOAL-2.3's selection uses seeded RNG (implicit), GOAL-2.4's tie-breaker is deterministic (age-based). No contradictions.
- **Check #16: Technology assumptions** ✅ — GOAL-2.6 assumes serde (explicitly justified in master doc's Dependencies section). GOAL-2.4 references NSGA-II crowding distance (well-defined algorithm, no technology assumption). No implicit technology assumptions.
- **Check #17: External dependencies** ✅ — No external service dependencies. All inputs come from the evaluation cache (internal) and config (internal).
- **Check #19: Migration/compatibility** ✅ — N/A, this is a new system with no predecessor.
- **Check #21: Unique identifiers** ✅ — GOAL-2.1 through GOAL-2.6, sequential, no gaps, no duplicates.
- **Check #22: Grouping/categorization** ✅ — Requirements are logically ordered: definition (2.1) → update (2.2) → selection (2.3) → size management (2.4) → performance (2.5) → serialization (2.6).
- **Check #23: Dependency graph** ✅ — Implicit but clear: GOAL-2.2 depends on GOAL-2.1 (uses dominance definition). GOAL-2.3 depends on GOAL-2.1 (selects from computed front). GOAL-2.4 depends on GOAL-2.1 (prunes front). GOAL-2.5 constrains GOAL-2.1. GOAL-2.6 serializes GOAL-2.1's output. No circular dependencies.
- **Check #24: Acceptance criteria** — Each GOAL has implicit acceptance criteria via its specification. No separate acceptance criteria section, but the specificity of each GOAL (especially GOAL-2.1, 2.4, 2.5) makes this acceptable for a feature-level requirements doc.
- **Check #27: Risk identification** ✅ — GOAL-2.4 (crowding distance at high M) is explicitly called out as a known limitation AND is listed in the master doc's Risks section. GOAL-2.1 (score alignment with sparse matrices) is listed in master doc risks.

---

## Summary

- **Total requirements:** 6 GOALs (3 P0, 3 P1), reviewed against 9 GUARDs from master
- **Critical:** 3 (FINDING-1, FINDING-2, FINDING-3)
- **Important:** 8 (FINDING-4 through FINDING-11)
- **Minor:** 4 (FINDING-12 through FINDING-15)
- **Total findings:** 15
- **Coverage gaps:** Score type/range undefined; error handling for partial re-evaluation failure; boundary conditions for empty/single-candidate front and min_shared_examples=0; crowding distance dimension semantics; explicit non-requirements missing
- **Recommendation:** **Needs fixes first** — the 3 critical findings (compound requirements in GOAL-2.2/2.3 and undefined score type) should be resolved before design. The important findings around boundary conditions and crowding distance semantics will cause implementer questions if not addressed.
- **Estimated implementation clarity:** **Medium** — core dominance logic is well-specified, but selection strategy and crowding distance details need clarification before an implementer could start confidently.
