# Review: requirements-08-data-loading.md

**Reviewer:** automated-requirements-review  
**Date:** 2026-04-04  
**Document:** `.gid/features/gepa-core/requirements-08-data-loading.md`  
**Master doc:** `.gid/features/gepa-core/requirements-master.md`

---

## Phase 0: Document Size Check

**7 GOALs** — well under the 15-GOAL limit. ✅

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #4] GOAL-8.5: Compound requirement — should be split into 3 GOALs**

GOAL-8.5 contains three distinct capabilities bundled into one requirement:
1. Score matrix backfill (selecting sparse candidates, evaluating on unseen examples)
2. Front recomputation after backfill (dominance re-checking)
3. Overfitting detection (computing delta, reporting, influencing selection)

Each is independently testable and implementable. Combining them makes it impossible to assign priority/status independently and creates a requirement that an engineer must read 3 times to fully parse.

**Suggested fix:** Split GOAL-8.5 into:
- **GOAL-8.5a** [P1]: Score matrix backfill — every `re_eval_interval` iterations, select front candidates with sparsest score coverage (fewest evaluated examples; ties broken by candidate age — newest first — for GUARD-9 determinism) and evaluate them on examples they haven't seen (sample size = `re_eval_sample_size`, configurable, default: `minibatch_size` — see GOAL-7.1). New scores are written to the evaluation cache (GOAL-6.3).
- **GOAL-8.5b** [P1]: Front recomputation after backfill — after each backfill round (GOAL-8.5a), recompute dominance relationships across the front using updated score coverage (see GOAL-2.2). Candidates that are now dominated are removed from the front. This is the convergence mechanism: early iterations grow the front (few shared examples → non-dominating), later iterations shrink it (dense matrix → dominance detectable).
- **GOAL-8.5c** [P1]: Overfitting detection — per-candidate overfitting delta (difference between average training score and average re-evaluation score) is computed after backfill rounds, reported via `ReEvaluationCompleted` event and statistics. High overfitting delta influences selection (GOAL-2.3) but does not directly remove candidates — only dominance does.

### FINDING-2
**[Check #14] Summary line: Priority count mismatch**

The summary line at the bottom of the document says:
> **Summary: 7 GOALs** (4 P0, 2 P1, 1 P2)

Actual counts from the document body: 5 P0 (8.1, 8.2, 8.3, 8.6, 8.7), 2 P1 (8.4, 8.5), 0 P2. The master doc's feature index correctly says "5 P0, 2 P1". The in-document summary is wrong.

**Suggested fix:** Change the summary line to:
```
**Summary: 7 GOALs** (5 P0, 2 P1)
```

---

## 🟡 Important (should fix before implementation)

### FINDING-3
**[Check #5] GOAL-8.2: Incomplete — difficulty tag type and semantics unspecified**

GOAL-8.2 mentions an optional "difficulty tag" but does not specify:
- What type is it? String? Enum? Numeric?
- What are valid values?
- What consumes it? No other GOAL in the data-loading doc or elsewhere references difficulty tag.
- If nothing uses it, why require it?

An implementer would not know how to represent this field. A tester could not validate it.

**Suggested fix:** Either (a) specify type and consumer: "difficulty tag (string, optional — used by [future feature X] for curriculum learning)" or (b) remove it and add as a future enhancement if/when needed, or (c) clarify it's a free-form metadata tag the consumer can use opaquely: "difficulty tag (optional `String`, opaque to the engine — available for consumer-side logic)."

### FINDING-4
**[Check #9] GOAL-8.3: Boundary condition — minibatch_size ≥ training set size not specified**

GOAL-8.3 specifies epoch behavior when total evaluations < training set, and the concatenation behavior at epoch boundaries. But what happens when `minibatch_size` >= number of training examples? E.g., 10 training examples, minibatch_size=16. Does the engine:
- Use all 10 and pad? (With what?)
- Cap the minibatch at the training set size?
- Return an error?

This is a plausible edge case (small datasets during testing/debugging).

**Suggested fix:** Add: "If `minibatch_size` ≥ number of training examples, each minibatch contains all training examples (effectively full-batch mode). The actual batch size equals `min(minibatch_size, training_set_size)`. No padding or duplication occurs."

### FINDING-5
**[Check #7] GOAL-8.4: Retry behavior underspecified — what about non-timeout failures?**

GOAL-8.4 specifies retry on timeout and loading failure, but groups them together. Questions:
- Does "loading fails" mean any error (parse error, auth error, connection refused)? Or only transient errors?
- Should parse errors (e.g., malformed data) be retried? Probably not.
- The error variant says `retryable: false` — but the engine just retried 3 times. This is contradictory: the engine treated it as retryable (it retried), but the final error says `retryable: false`.

**Suggested fix:** Clarify: "If async loading fails with a retryable error (timeout, connection error) or times out, the engine retries up to 3 times with backoff (per GOAL-7.1 backoff strategy). After exhausting retries, the engine halts with `GEPAError::AdapterError { source, retryable: false }`. Non-retryable errors (e.g., deserialization failure) cause immediate halt without retry."

### FINDING-6
**[Check #5] GOAL-8.5: Missing — what method is used for re-evaluation?**

GOAL-8.5 says "evaluates them on examples they haven't seen" but doesn't specify which adapter method is called. GOAL-1.7 distinguishes between `execute` (full traces) and `evaluate` (scores only). Re-evaluation backfill only needs scores, so `evaluate` should be used. This should be explicit.

**Suggested fix:** Add to GOAL-8.5 (or 8.5a after split): "The engine calls the adapter's `evaluate` method (GOAL-3.5) for backfill evaluations, since only numeric scores are needed."

### FINDING-7
**[Check #9] GOAL-8.3: Boundary condition — training set size = 1 not addressed**

If there's exactly 1 training example, every minibatch contains only that example. The epoch concept degenerates. While this technically works, it should be explicitly acknowledged as a valid (if degenerate) case, or flagged as a minimum training set size requirement.

**Suggested fix:** Either add a minimum training set size check to GOAL-8.7 (e.g., "If training set has fewer than `minibatch_size` examples, a warning event is emitted") or explicitly state in GOAL-8.3 that single-example training sets are valid.

### FINDING-8
**[Check #8] Missing non-functional requirement: Performance — validation set evaluation cost**

GOAL-8.6 says "evaluates all Pareto front candidates on the full validation set." For a front of 50 candidates and a validation set of 1000 examples, that's 50,000 adapter `evaluate` calls. There's no:
- Cost budget or cap on validation evaluations
- Progress reporting during validation
- Timeout for the validation phase as a whole

This could be the most expensive part of the entire run.

**Suggested fix:** Add to GOAL-8.6: "Validation evaluation emits progress events (`ValidationProgress { candidate_index, total_candidates }`) via the callback system. If the adapter's `evaluate` call fails during validation, the same error/retry policy from GOAL-7.5 applies. The engine may batch validation calls per candidate (all examples at once) or per example (all candidates at once) — the implementation chooses the more efficient ordering."

### FINDING-9
**[Check #17] GOAL-8.4: Timeout configuration location unclear**

GOAL-8.4 says "configurable timeout (default: 30s)" but doesn't specify where this is configured. Is it a field in `GEPAConfig`? GOAL-7.1 lists the config fields and does not include a DataLoader timeout. This creates ambiguity about where the 30s default lives.

**Suggested fix:** Either add `data_loader_timeout` to GOAL-7.1's config field list, or specify in GOAL-8.4 that the timeout is a field on `GEPAConfig` (e.g., `data_loader_timeout_secs: u64, default: 30`).

### FINDING-10
**[Check #5] GOAL-8.1: Return type may be too rigid — `Vec<Example>` forces loading all into memory**

GOAL-8.1 specifies `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>`. For large datasets, loading all examples into memory at once may be impractical. There's no streaming/iterator alternative.

Given the master doc's "No distributed execution" and single-process scope, this is probably acceptable for v1, but should be explicitly acknowledged.

**Suggested fix:** Add a note: "For v1, examples are loaded eagerly into memory. Streaming/iterator-based loading is a future enhancement (not in scope)." This prevents scope creep and documents the trade-off.

---

## 🟢 Minor (can fix during implementation)

### FINDING-11
**[Check #12] Terminology: "Example" vs "training example" vs "task subset"**

The document uses "Example" (the struct), "training examples," "validation examples," and the master doc's terminology section references "task subset" as groups of examples. Within this document, usage is mostly consistent, but GOAL-8.5 says "examples they haven't seen" — "seen" is informal. Suggest using "evaluated on" consistently.

### FINDING-12
**[Check #22] Cross-references section incomplete**

The cross-references section lists GOAL-1.4, GOAL-6.3, GOAL-7.1, GUARD-9. But GOAL-8.5 also references GOAL-2.2 and GOAL-2.3, and GOAL-8.4 references `GEPAError::AdapterError` (GOAL-3.x). These are missing from the cross-references section.

**Suggested fix:** Add to cross-references:
```
- GOAL-2.2 (Pareto Front) — front recomputation after backfill
- GOAL-2.3 (Pareto Front) — overfitting delta in selection
- GOAL-3.5 (Adapter) — evaluate method used for backfill
```

### FINDING-13
**[Check #21] No gaps in numbering**

GOALs are 8.1 through 8.7 — no gaps. ✅ Minor note: if FINDING-1 is applied (splitting 8.5 into 8.5a/b/c), the total becomes 9 GOALs and the summary should update.

### FINDING-14
**[Check #25] GOAL-8.1: User perspective could be stronger**

GOAL-8.1 is system-internal ("The engine uses training examples for..."). Consider adding consumer-facing context: "Consumers implement `DataLoader` to provide their domain-specific training and validation data to the engine." The current phrasing is adequate but could be clearer about who does what.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path — trait definition | GOAL-8.1, GOAL-8.2 | - |
| Happy path — sampling | GOAL-8.3 | Boundary: minibatch_size ≥ training set (FINDING-4) |
| Happy path — validation | GOAL-8.6 | Validation cost budget (FINDING-8) |
| Error handling — empty data | GOAL-8.7 | - |
| Error handling — async failure | GOAL-8.4 | Non-retryable vs retryable distinction (FINDING-5) |
| Error handling — duplicate example IDs | - | ⚠️ No requirement for uniqueness enforcement |
| Error handling — data changes between epochs | - | ⚠️ No requirement specifying whether DataLoader is called once or per-epoch |
| Performance — sampling | Implicit in GUARD-6 | No explicit O(n) bound on sampling |
| Performance — validation | - | ⚠️ No validation cost cap (FINDING-8) |
| Security | N/A (library crate, GUARD-5) | - |
| Reliability — retries | GOAL-8.4 | Distinction between transient/permanent errors (FINDING-5) |
| Observability — events | GOAL-8.5 (ReEvaluationCompleted), GOAL-8.7 (DataLoaderWarning) | No event for minibatch sampling, no validation progress events |
| Scalability — data volume | GOAL-8.1 (Vec in memory) | No streaming/large dataset support documented as out of scope (FINDING-10) |
| Determinism | GOAL-8.3 (seeded RNG), GOAL-8.5 (tie-breaking) | - |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 7 GOALs, well under 15 limit
- **Check #1: Specificity** ✅ — 7/7 GOALs use concrete language. GOAL-8.3 is especially specific with worked example. GOAL-8.5 specifies exact tie-breaking rules. No vague terms like "fast" or "appropriate" found.
- **Check #2: Testability** ✅ — 7/7 GOALs have clear pass/fail conditions. GOAL-8.3 can be tested with property-based tests (epoch coverage invariant). GOAL-8.7 has explicit error types. GOAL-8.6 has the `validation_skipped` flag.
- **Check #3: Measurability** ✅ — Quantitative values are concrete: timeout 30s, retry 3 times, re_eval_interval default 5, minibatch_size default 16. No unmeasured quantities.
- **Check #6: Happy path coverage** ✅ — Flow: DataLoader provides data → engine validates at startup → samples minibatches → runs optimization → periodic backfill → final validation. All steps covered.
- **Check #10: State transitions** ✅ — No explicit state machine in this feature. Data flow is linear: load → validate → sample → evaluate → backfill → final validation. No states with missing exits.
- **Check #11: Internal consistency** ✅ — Verified all 21 pairs of GOALs (7 choose 2). No contradictions found. GOAL-8.6 and GOAL-8.7 align (both handle empty validation). GOAL-8.3 and GOAL-8.5 are complementary (sampling vs backfill).
- **Check #13: Priority consistency** ✅ — P0 GOALs (8.1, 8.2, 8.3, 8.6, 8.7) do not depend on P1 GOALs. GOAL-8.5 [P1] depends on GOAL-8.1 [P0] and GOAL-6.3 [P0] — correct direction. GOAL-8.4 [P1] extends GOAL-8.1 [P0] — correct.
- **Check #15: GUARDs vs GOALs alignment** ✅ — Verified against all 9 GUARDs:
  - GUARD-1 (front invariant): GOAL-8.5 backfill triggers re-check, maintaining invariant ✅
  - GUARD-2 (candidate immutability): No GOAL mutates candidates ✅
  - GUARD-3 (adapter call order): GOAL-8.5 calls `evaluate` outside the main loop step sequence — but GUARD-3 says "outside the optimization loop's defined sequence." Backfill IS part of the loop (every N iterations). GOAL-1.7c establishes precedent for front re-evaluation. ✅
  - GUARD-4 (atomic checkpoints): Not relevant to data loading ✅
  - GUARD-5 (no LLM calls): DataLoader loads data, not LLM calls ✅
  - GUARD-6 (engine overhead <5%): Sampling is O(n) shuffle, should be negligible ✅
  - GUARD-7 (linear memory): Vec<Example> is linear in examples ✅
  - GUARD-8 (Debug/Error impls): Example type should implement Debug ✅
  - GUARD-9 (determinism): GOAL-8.3 uses seeded RNG, GOAL-8.5 has deterministic tie-breaking ✅
- **Check #16: Technology assumptions** ✅ — `serde_json::Value` in GOAL-8.2 is justified (serde_json is an allowed dependency in master doc). `async fn` in GOAL-8.4 uses tokio (allowed dependency).
- **Check #19: Migration/compatibility** ✅ — N/A — new system, no migration needed.
- **Check #20: Scope boundaries** ✅ — Master doc covers out-of-scope items. Data loading doesn't add domain-specific scope issues. Streaming is implicitly out of scope (Vec return type).
- **Check #23: Dependency graph** ✅ — Dependencies are clear: GOAL-8.3 depends on GOAL-8.1/8.2 (need trait and Example type). GOAL-8.5 depends on GOAL-8.3 (needs sampling) and GOAL-6.3 (cache). GOAL-8.6 depends on GOAL-8.1. GOAL-8.7 depends on GOAL-8.1. No circular dependencies.
- **Check #24: Acceptance criteria** ✅ — Each GOAL contains its acceptance criteria inline. GOAL-8.3 even has a worked numerical example. GOAL-8.7 specifies exact error types.
- **Check #26: Success metrics** ⚠️ Partial — GOAL-8.5 reports overfitting delta (observable metric). Cache hit rate tracked (GOAL-6.4). No explicit metric for "sampling quality" but epoch coverage is verifiable.
- **Check #27: Risk identification** ✅ — Master doc explicitly identifies GOAL-8.3 (epoch boundary sampling) as high-risk requiring property-based testing. This is appropriate.

---

## Summary

| Metric | Value |
|---|---|
| Total requirements | 7 GOALs (5 P0, 2 P1), 0 GUARDs (in master) |
| Critical findings | 2 (FINDING-1, FINDING-2) |
| Important findings | 8 (FINDING-3 through FINDING-10) |
| Minor findings | 4 (FINDING-11 through FINDING-14) |
| Total findings | 14 |
| Coverage gaps | Duplicate example ID handling, DataLoader call frequency (once vs per-epoch), validation cost cap, streaming out-of-scope acknowledgment |
| Recommendation | **Needs fixes first** — FINDING-1 (atomicity split) and FINDING-2 (summary correction) are quick fixes. FINDING-4 (boundary condition) and FINDING-5 (retry semantics) should be resolved before implementation to prevent ambiguity. |
| Implementation clarity | **High** — despite the findings, this is a well-written requirements doc. GOAL-8.3 is exceptionally detailed. Most findings are about edge cases and missing boundaries rather than fundamental ambiguity. |
