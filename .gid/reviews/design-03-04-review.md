# Design Review: 03-Adapter & 04-Proposers

**Reviewer:** subagent  
**Date:** 2026-04-04  
**Docs reviewed:** `design-03-adapter.md`, `design-04-proposers.md`  
**Reqs referenced:** `requirements-03-adapter.md`, `requirements-04-proposers.md`

---

## Design 03 — Adapter Trait

### FINDING-1 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.1
**Issue:** None. The trait is defined as `#[async_trait]` with `Send + Sync + 'static` bounds. All four required methods (`execute`, `reflect`, `mutate`, `evaluate`) and the optional `merge` are present. Sequential calling semantics are documented. All methods receive `&self` (immutable). Fully satisfies GOAL-3.1.
**Fix:** N/A

### FINDING-2 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.2
**Issue:** None. `execute` receives `&Candidate` and `&[Example]`, returns `Vec<ExecutionTrace>`. The 1:1 contract (one trace per example, same order) is explicitly documented. Partial failure semantics (empty output, `None` score, ASI for diagnostics) are specified. Engine validates `traces.len() == examples.len()`. `ExecutionTrace` struct includes `example_id`, `input`, `output`, `score: Option<f64>`, `asi: Option<String>` — matches requirement exactly.
**Fix:** N/A

### FINDING-3 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.3
**Issue:** None. `reflect` receives `&Candidate` and `&[ExecutionTrace]`, returns `Reflection { diagnosis: String, directions: Vec<String> }`. Matches requirement.
**Fix:** N/A

### FINDING-4 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.4
**Issue:** None. `mutate` receives `&Candidate`, `&Reflection`, `&[String]` (ancestor lessons). Empty slice for seeds documented. New candidate has `parent_id = Some(parent.id)`, `generation = parent.generation + 1`. Engine validates key consistency and assigns ID.
**Fix:** N/A

### FINDING-5 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.5
**Issue:** None. `evaluate` receives `&Candidate` and `&[Example]`, returns `Vec<f64>` with 1:1 contract. GUARD-10 semantics documented. Engine sanitizes NaN/Inf post-return. All four calling contexts listed (seed eval, acceptance, backfill, validation).
**Fix:** N/A

### FINDING-6 [🟡 ✅ Applied] (design-03)
**GOAL:** GOAL-3.6
**Issue:** The merge call sequence documented in GOAL-3.6 requirements is: `select (two candidates) → execute (both on minibatch) → merge → evaluate → accept`. However, the adapter's `merge` method signature only receives two `&Candidate` references — there is no indication of where or how the `execute` step on both parents fits in. The design-03 merge contract says "Input: Two `&Candidate` references" without mentioning that `execute` should have been called on both parents before `merge`. This "execute before merge" step is not mentioned in design-03 at all; it's deferred to design-04's `MergeProposer`. This is acceptable as a separation of concerns, but the adapter doc should at least cross-reference this sequence so implementors understand the full context.
**Fix:** Add a note in §2.2 merge contract clarifying that the engine/proposer executes both parents before calling `merge`, and cross-reference design-04 §2.3 for the full merge iteration sequence.

### FINDING-7 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.7
**Issue:** None. All methods return `Result<T, GEPAError>`. The `GEPAError` variants (`AdapterError { source, retryable }`, `Timeout`, `RateLimited { retry_after }`, `Cancelled`) are referenced consistently. Cancellation forwarding is documented in §2.6 with a concrete example showing the adapter checking `CancellationToken` internally.
**Fix:** N/A

### FINDING-8 [🟢 ✅] (design-03)
**GOAL:** GOAL-3.8
**Issue:** None. The example adapter in §3 shows ~60 lines for all 4 methods. The design acknowledges this slightly exceeds the "< 50 lines" target but is close enough for a complete working example. Default `merge` implementation eliminates that boilerplate.
**Fix:** N/A

### FINDING-9 [🟡 ✅ Applied] (design-03)
**GOAL:** GOAL-3.5 / GUARD-10
**Issue:** The `mutate` contract states "The returned candidate must have the same parameter keys as the parent." This is a useful invariant, but it's not in the requirements (GOAL-3.4 says "returns a new Candidate with potentially modified text parameters"). This added constraint should be validated — is it intentional? It would prevent mutation from adding/removing parameters, which could be limiting for some use cases.
**Fix:** Either: (a) Add this constraint to requirements-03 GOAL-3.4 if intentional, or (b) remove it from the design and let the engine be flexible about parameter keys. Recommend option (a) with documentation that parameter schema is fixed at seed time.

### FINDING-10 [🟡 ✅ Applied] (design-03)
**GOAL:** GOAL-3.6
**Issue:** The design says the engine treats the default merge error as "merge unsupported" and "disables merge for the remainder of the run." This auto-disable behavior is not specified in the requirements. GOAL-3.6 only says the default returns `Err`. The auto-disable-on-first-failure is an engine-level policy that should be documented in design-01 or at minimum cross-referenced.
**Fix:** Cross-reference this behavior with design-01 engine loop and ensure it's consistent with GOAL-1.10 merge scheduling. If the engine disables merge permanently after one failure, document this explicitly.

---

## Design 04 — Proposers

### FINDING-11 [🟢 ✅] (design-04)
**GOAL:** GOAL-4.1
**Issue:** None. `MutationProposer::generate()` implements select → execute → reflect → mutate via the adapter, producing exactly one candidate. Error propagation to the engine is documented. The `generate` flow is explicitly listed: `select_parent` → `adapter.execute` → `adapter.reflect` → `build_lesson_chain` → `adapter.mutate` → return `ProposerOutput`.
**Fix:** N/A

### FINDING-12 [🟢 ✅] (design-04)
**GOAL:** GOAL-4.2
**Issue:** None. `build_lesson_chain` walks `parent_id` links through `CandidateStore`, collecting ancestor reflections in most-recent-first order. Seeds with no lineage produce an empty chain.
**Fix:** N/A

### FINDING-13 [🟢 ✅] (design-04)
**GOAL:** GOAL-4.2b
**Issue:** None. Lesson chain truncation to `config.max_lesson_depth` (default 10, keeping most recent N) is explicitly documented.
**Fix:** N/A

### FINDING-14 [🟡 ✅ Applied] (design-04)
**GOAL:** GOAL-4.3
**Issue:** The selection algorithm described is "select the front member with the lowest selection count, break ties with RNG." While this achieves round-robin fairness, the `round_robin_cursor: usize` field in `MutationProposer` is declared but never explained. The algorithm description only uses `selection_counts` for lowest-count selection. If `round_robin_cursor` is unused, it's dead state that will confuse implementors. If it serves a purpose (e.g., deterministic traversal order for tie-breaking), that should be documented.
**Fix:** Either remove `round_robin_cursor` from the struct if unused, or document its role in the selection algorithm. The lowest-count + RNG tie-breaking approach doesn't need a cursor.

### FINDING-15 [🟢 ✅] (design-04)
**GOAL:** GOAL-4.4
**Issue:** None. Complementary pair selection scans all O(N²) pairs, computes `|A_better ∪ B_better|` over shared evaluated examples, with proper tie-breaking (highest combined average, then RNG). Front < 2 handled by returning error/skipping. Performance justification (N ≤ 50, negligible vs adapter time) is documented.
**Fix:** N/A

### FINDING-16 [🟡 ✅ Applied] (design-04)
**GOAL:** GOAL-4.5
**Issue:** GOAL-4.5 requires that the merge adapter call receives "both parent Candidate objects, their respective per-example scores, and identification of which task subsets each parent excels on." The design says the adapter receives the full candidate structs and "Per-example score context for the merge adapter call is assembled from the evaluation cache." However, the `GEPAAdapter::merge` signature in design-03 only accepts `(&self, parent_a: &Candidate, parent_b: &Candidate)` — there are no parameters for scores or task subset identification. The per-example scores and subset information cannot be passed through this signature.
**Fix:** Either: (a) Extend the `merge` signature to include score context (e.g., `scores_a: &[(String, f64)], scores_b: &[(String, f64)]`), or (b) embed the per-example scores into the `Candidate` struct or a wrapper, or (c) clarify in requirements that the adapter is expected to access scores through the candidate's metadata/params. This is a real interface mismatch between design-03 and design-04/requirements-04.

### FINDING-17 [🟡 ✅ Applied] (design-04)
**GOAL:** GOAL-4.4
**Issue:** The `complementarity` method returns `(usize, f64)` — presumably `(|A_better ∪ B_better|, combined_avg_score)` for tie-breaking. This is fine but the return type is not documented. Also, how does `select_complementary_pair` handle the case where two candidates share zero evaluated examples? The complementarity would be 0 with undefined average. This edge case should be specified.
**Fix:** Document the return type semantics of `complementarity`. Specify that pairs with zero shared examples have complementarity 0 and are ranked last (or equivalent policy).

### FINDING-18 [🟢 ✅] (design-04)
**GOAL:** GOAL-1.12 (cross-ref)
**Issue:** None. `evaluate_seeds` is a standalone async function (not a Proposer impl) that iterates seeds, calls `adapter.evaluate` with retry, stores scores in cache, discards failed seeds, and returns `Err(AllSeedsFailedError)` if all fail. Clean separation.
**Fix:** N/A

### FINDING-19 [🟡 ✅ Applied] (design-04)
**GOAL:** GOAL-4.1 / GUARD-3
**Issue:** The `Proposer::generate()` signature takes `adapter: &dyn GEPAAdapter`, meaning the adapter is passed as a trait object. However, design-03 defines `GEPAAdapter` with `async_trait` which makes methods return `Pin<Box<dyn Future + Send>>`. Using `&dyn GEPAAdapter` with `async_trait` requires the trait to be object-safe. The `merge` method has a default implementation which is fine for object safety. However, `async_trait` by default generates `Send` futures only when the trait is not `dyn`-dispatched OR when `#[async_trait]` (not `#[async_trait(?Send)]`) is used. This should work but deserves a note confirming `#[async_trait]` (with Send) is used, ensuring the trait is object-safe with Send futures.
**Fix:** Add a brief note in design-03 or design-04 confirming that `#[async_trait]` (not `?Send`) is used, and that the trait is object-safe for `&dyn GEPAAdapter` dispatch.

### FINDING-20 [🔴 ✅ Applied] (design-04)
**GOAL:** GOAL-4.5
**Issue:** The merge iteration sequence per GOAL-3.6 requirements is: `select (two candidates) → execute (both on minibatch) → merge → evaluate → accept`. In design-04 §2.3, the `MergeProposer::generate()` method calls `select_complementary_pair` then `adapter.merge(candidate_a, candidate_b)`. But there is NO mention of calling `adapter.execute()` on both parents before `merge`. The execute step is entirely missing from the merge proposer's generate flow. This contradicts the required merge call sequence.
**Fix:** Add the execute step to `MergeProposer::generate()`: after selecting the complementary pair, call `adapter.execute(parent_a, minibatch)` and `adapter.execute(parent_b, minibatch)` to produce traces, then pass these (or derived context) to the merge call. This may require extending the `merge` adapter signature or documenting how the execute traces feed into merge context.

---

## Summary

### Design-03 (Adapter Trait)
| Rating | Count |
|--------|-------|
| 🟢 Pass | 6 |
| 🟡 Minor | 3 |
| 🔴 Blocker | 0 |

**Overall:** Solid design. All 8 GOALs have clear design mechanisms. Code snippets are valid. The three minor issues are: (1) merge contract should cross-reference the full merge iteration sequence, (2) the "same parameter keys" constraint on mutate needs requirements alignment, (3) the auto-disable-merge-on-failure behavior needs cross-referencing with engine design. No blockers.

### Design-04 (Proposers)
| Rating | Count |
|--------|-------|
| 🟢 Pass | 4 |
| 🟡 Minor | 4 |
| 🔴 Blocker | 1 |

**Overall:** Good structure but has one blocker: **FINDING-20** — the merge iteration is missing the required `execute` step before `merge`, contradicting GOAL-3.6's specified call sequence. Additionally, **FINDING-16** identifies a real interface mismatch where the `merge` adapter signature cannot carry the per-example score context that GOAL-4.5 requires. The `round_robin_cursor` field is declared but unexplained. These issues should be resolved before implementation.

### Cross-Design Consistency
- **Design-03 ↔ Design-04 mismatch (FINDING-16, FINDING-20):** The `merge` method signature in design-03 is too narrow for what design-04 and requirements-04 need. The merge flow is incompletely specified across both docs. These should be resolved together.
- **Integration points** are otherwise consistent: proposers correctly reference `ParetoFront`, `EvaluationCache`, `CandidateStore`, `GEPAConfig` from their respective features.
- **GUARD compliance** is well-documented in both designs.
