# Design Review: 07-Config, 08-Data Loading, 09-Events

**Reviewer:** subagent  
**Date:** 2026-04-04  
**Docs reviewed:** design-07-config.md, design-08-data-loading.md, design-09-events.md  
**Against:** requirements-07-config.md, requirements-08-data-loading.md, requirements-09-events.md

---

## Design 07 — Config

### FINDING-1 [🟢] (design-07)
**GOAL:** GOAL-7.1
**Issue:** All required fields present: `rng_seed`, `min_shared_examples`, `max_re_eval_per_iteration`, `max_lesson_depth`, `eval_cache_max_size`, `data_loader_timeout_secs`. Defaults match requirements (min_shared_examples = minibatch_size, max_re_eval_per_iteration = pareto_max_size × minibatch_size / 2, max_lesson_depth = 10, eval_cache_max_size = None).
**Fix:** None needed.

### FINDING-2 [🟢] (design-07)
**GOAL:** GOAL-7.2
**Issue:** Default values table matches requirements exactly: max_iterations=100, minibatch_size=16, stagnation_limit=20, checkpoint_interval=1, pareto_max_size=50. `GEPAConfig::default()` implemented via builder with validation (not derive), ensuring defaults pass validation.
**Fix:** None needed.

### FINDING-3 [🟡] (design-07)
**GOAL:** GOAL-7.3
**Issue:** Validation returns only the **first** error found. Requirements list many invalid conditions. The design explicitly says "returns the first invalid condition found (not all errors at once)." While the rationale ("simpler for users to fix iteratively") is reasonable, the requirements spec says "descriptive error **message**" (singular), so this is technically fine. However, the design lacks a validation check for `min_shared_examples > total training examples` — but the requirements explicitly state this is "checked at run start, not construction", so the design correctly omits it from builder validation. Also missing: explicit mention that `retry_max=0`, `max_consecutive_skips=0`, and `time_budget=Duration::ZERO` are allowed (valid edge cases). The design says "Valid edge cases explicitly allowed: `retry_max=0`, `max_consecutive_skips=0`, `time_budget=Some(Duration::ZERO)`" — this matches.
**Fix:** Minor — consider documenting that `min_shared_examples > training_set_size` is validated at engine run start (in design-01 or design-08), not here. Current design is correct but the cross-feature validation could be more explicit.

### FINDING-4 [🟢] (design-07)
**GOAL:** GOAL-7.4
**Issue:** Serde support fully designed. Derives `Serialize, Deserialize`. Duration fields use custom humantime-style serializer. Checkpoint includes `config: GEPAConfig` as serialized field. On resume, checkpoint config is used (ignoring new config).
**Fix:** None needed.

### FINDING-5 [🟢] (design-07)
**GOAL:** GOAL-7.5
**Issue:** Backoff/retry fully designed: Fixed and Exponential strategies, base_delay, max_retry_delay cap, RateLimited retry_after handling (`max(d, computed_backoff)`), ErrorPolicy Skip/Halt, max_consecutive_skips → TooManySkips. Skipped iterations don't count toward stagnation. All matches requirements.
**Fix:** None needed.

### FINDING-6 [🟢] (design-07)
**GOAL:** GOAL-7.6
**Issue:** `time_budget: Option<Duration>` present with default `None`. Design states engine checks at start of each iteration before Select step. In-progress iterations run to completion. Matches requirements.
**Fix:** None needed.

### FINDING-7 [🟢] (design-07)
**GOAL:** GOAL-7.7
**Issue:** Merge settings present: `merge_enabled` (bool, default false), `merge_interval` (u64, default 10), `merge_strategy` (Complementary/Random, default Complementary). Validation rejects `merge_interval=0` when merge enabled. Requirements mention "All merge selection ties are broken using the seeded RNG per GUARD-9" — this is a runtime behavior, not config, so correctly not in this design.
**Fix:** None needed.

### FINDING-8 [🟡] (design-07)
**GOAL:** GOAL-7.3 (edge case validation)
**Issue:** The `ConfigError` enum has `ZeroBaseDelayExponential` for `base_delay=0` with exponential backoff, but requirements say "retry_max with base_delay=0 when strategy=exponential" is invalid. The design checks `base_delay > 0` for exponential strategy regardless of `retry_max`. If `retry_max=0` (no retries), `base_delay=0` with exponential is harmless since no retries will occur. The requirements phrase "retry_max with base_delay=0 when strategy=exponential" is ambiguous — does it mean "retry_max > 0 AND base_delay = 0 AND exponential"? The design may over-reject.
**Fix:** Clarify: only reject `base_delay=0` with `BackoffStrategy::Exponential` when `retry_max > 0`. If `retry_max=0`, base_delay is irrelevant. Update validation logic to check `if config.backoff_strategy == BackoffStrategy::Exponential && config.retry_max > 0 && config.base_delay == Duration::ZERO`.

---

## Design 08 — Data Loading

### FINDING-9 [🟡] (design-08)
**GOAL:** GOAL-8.1
**Issue:** Requirements say `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>` (sync signatures). Design makes them `async fn ... -> Result<Vec<Example>, GEPAError>`. The async change is driven by GOAL-8.4 (P1), which is fine — but the `Result` return type is an addition over the base GOAL-8.1 spec. This is a good design decision (errors are inevitable), but technically changes the interface from what GOAL-8.1 specifies.
**Fix:** Minor discrepancy. Requirements GOAL-8.1 should be updated to reflect `Result<Vec<Example>, GEPAError>` return types, or the design should note the deviation explicitly. The design is correct; the requirements underspecified the error case.

### FINDING-10 [🟢] (design-08)
**GOAL:** GOAL-8.2
**Issue:** `Example` struct has: `id: ExampleId` (String newtype), `input: serde_json::Value`, `expected_output: Option<serde_json::Value>`, `metadata: HashMap<String, serde_json::Value>`, `difficulty_tag: Option<String>`. All required fields present. `ExampleId` implements `Hash + Eq` for HashMap keys. Debug derived.
**Fix:** None needed.

### FINDING-11 [🟢] (design-08)
**GOAL:** GOAL-8.3
**Issue:** `MinibatchIterator` algorithm is fully specified: shuffled indices, cursor-based traversal, epoch boundary crossing (fill remainder from next epoch's shuffle), deterministic via ChaCha8Rng. Edge cases handled: `batch_size >= len(examples)` = full-batch mode, single-example training sets. Epoch boundary concatenation logic matches requirements example (100 examples, batch 16, batch 7 gets last 4 + first 12).
**Fix:** None needed.

### FINDING-12 [🟢] (design-08)
**GOAL:** GOAL-8.4
**Issue:** `async_trait` used, `tokio::time::timeout` wraps calls, retry up to 3 times with backoff, non-retryable errors halt immediately, `data_loader_timeout_secs` configurable (default 30). Matches requirements.
**Fix:** None needed.

### FINDING-13 [🟡] (design-08)
**GOAL:** GOAL-8.5a
**Issue:** `BackfillScheduler::select_candidates_for_backfill` takes `front`, `eval_cache`, `sample_size`, `max_evals`, `rng`. Candidate selection: sort by coverage count ascending, ties by candidate age (newest first). Example selection: uniform random from unevaluated set. Budget cap: total ≤ `max_re_eval_per_iteration`. However, the design returns `Vec<BackfillTask>` where each task has `candidate_id` and `example_ids`, but it doesn't specify **how many candidates** are selected. It says "candidates with the sparsest score coverage" (plural) but doesn't specify a limit or stopping condition beyond the total budget cap.
**Fix:** Clarify the algorithm: iterate candidates in sparsest-first order, assigning up to `sample_size` unevaluated examples each, accumulating until total assigned examples reaches `max_re_eval_per_iteration`. This is implied but should be explicit.

### FINDING-14 [🟢] (design-08)
**GOAL:** GOAL-8.5b
**Issue:** Design states: "After the engine executes all backfill tasks... it writes new scores to the evaluation cache and triggers front recomputation (§2.5 of design-02-pareto via GOAL-8.5b)." References correct integration point (`ParetoFront::recompute_dominance()`). Dominated candidates removed. Matches requirements.
**Fix:** None needed.

### FINDING-15 [🟢] (design-08)
**GOAL:** GOAL-8.5c
**Issue:** `compute_overfitting_delta` is a pure function: `mean(training) - mean(reeval)`. Positive = overfitting. Reported in `ReEvaluationCompleted` event and statistics. High delta influences selection (GOAL-2.3) but does NOT remove candidates. Matches requirements.
**Fix:** None needed.

### FINDING-16 [🟢] (design-08)
**GOAL:** GOAL-8.6
**Issue:** `ValidationRunner::run_validation` evaluates all front candidates on full validation set after loop exit. Empty validation → `validation_skipped: true`. Emits `ValidationProgress` per candidate. Adapter errors use same retry policy. Validation scores in `GEPAResult`. Matches requirements.
**Fix:** None needed.

### FINDING-17 [🟡] (design-08)
**GOAL:** GOAL-8.6
**Issue:** Design says adapter errors during validation produce `GEPAError::ValidationError` after retry exhaustion. But the requirements say "the same error/retry policy from GOAL-7.5 applies" — which means the error policy (Skip vs Halt) should also apply. For validation, what does "Skip" mean? Skip that candidate's validation? The design doesn't address this semantic distinction. During the optimization loop, Skip means skip the iteration. During validation, skipping a candidate's validation evaluation is a different behavior that needs specification.
**Fix:** Specify: during final validation, `ErrorPolicy::Skip` means skip the failing candidate (record no validation scores for it) and continue to the next candidate. `ErrorPolicy::Halt` means abort validation entirely and propagate the error. Add a note that skipped candidates get `validation_scores: None` in the result.

### FINDING-18 [🟢] (design-08)
**GOAL:** GOAL-8.7
**Issue:** `validate_data_loader_output` checks: empty training → `Err(GEPAError::EmptyDataError)`, empty validation → `DataLoaderWarning` event + `validation_skipped = true`, duplicate IDs → warning (not rejection). Matches requirements.
**Fix:** None needed.

### FINDING-19 [🟡] (design-08)
**GOAL:** GOAL-8.3 / GOAL-8.7
**Issue:** The `MinibatchIterator` is constructed with `Vec<ExampleId>`, but the design doesn't specify where the actual `Example` data is stored or how the engine maps `ExampleId` → `Example` when constructing the minibatch to pass to the adapter. The iterator returns `ExampleId`s, but the adapter presumably needs full `Example` objects (input data, etc.). There should be a lookup mechanism (e.g., `HashMap<ExampleId, Example>` in the engine or a separate `ExampleStore`).
**Fix:** Add a brief note about the engine maintaining an `examples: HashMap<ExampleId, Example>` (or `Vec<Example>` with index mapping) alongside the `MinibatchIterator`, used to resolve `ExampleId`s to full `Example` objects before passing to the adapter.

---

## Design 09 — Events

### FINDING-20 [🔴] (design-09)
**GOAL:** GOAL-9.1a
**Issue:** The requirements specify `CandidateAccepted` has field `candidate: Candidate` (the full Candidate object), but the design has `candidate_id: CandidateId` (just the ID). This is a payload mismatch. The requirements explicitly say `candidate: Candidate` for `CandidateAccepted`, while `CandidateRejected` only has `candidate_id: CandidateId`. This distinction is intentional — accepted candidates are presumably more interesting (consumers may want the full prompt text).
**Fix:** Change design's `CandidateAccepted` variant from `candidate_id: CandidateId` to `candidate: Candidate` (or a clone/ref thereof) to match requirements. This may impact the zero-cost optimization (cloning a `Candidate` only when callbacks are registered), but the `emit_event!` macro already handles this by constructing the event only when callbacks exist.

### FINDING-21 [🟢] (design-09)
**GOAL:** GOAL-9.1b
**Issue:** All 16 emission points are specified in the correct order in the pseudocode (§2.3). Emission sequence matches requirements: IterationStarted before Select, CandidateSelected after Select, etc. DataLoaderWarning during startup (before loop). RunCompleted after loop exit. ValidationProgress during final validation.
**Fix:** None needed.

### FINDING-22 [🟢] (design-09)
**GOAL:** GOAL-9.3
**Issue:** `CallbackRegistry` supports multiple callbacks per event type, invoked in registration order. Same callback can be registered multiple times (independent). No deregistration API. Registration via builder (`on_event(EventType, callback)`) before `run()`. All matches requirements.
**Fix:** None needed.

### FINDING-23 [🟢] (design-09)
**GOAL:** GOAL-9.4
**Issue:** `catch_unwind(AssertUnwindSafe(...))` wraps every callback. On panic: log warning via tracing, continue to next callback. Panicking callback remains registered. Remaining callbacks still invoked. Callbacks receive `&GEPAEvent` (immutable reference). No timeout enforcement — documented as consumer responsibility. All matches requirements.
**Fix:** None needed.

### FINDING-24 [🟡] (design-09)
**GOAL:** GOAL-9.5
**Issue:** `TracingCallback` log levels match the requirements table exactly. At `trace` level, full `Debug` representation is logged. At higher levels, summary lines with key fields. `register_all` convenience method registers for all event types. However, the `TracingCallback::register_all` code snippet is slightly inconsistent — it references both `self.callback()` and `self.callback_fn()`. The method `callback_fn()` is not defined in the interface; only `callback()` is. This is a code snippet bug.
**Fix:** Rename `self.callback_fn()` to `self.callback()` in the `register_all` implementation, or define `callback_fn` as a separate private method. The intent is clear but the code won't compile as-is.

### FINDING-25 [🟢] (design-09)
**GOAL:** GOAL-9.6
**Issue:** `emit_event!` macro checks `has_callbacks()` before constructing the event. With zero callbacks, overhead is 15 × O(1) HashMap lookups per iteration ≈ negligible. Event data (including Vec clones) only constructed inside the `if` branch. Performance analysis provided: 15ns/iteration with zero callbacks, 11µs/iteration with TracingCallback. Well within GUARD-6 budget.
**Fix:** None needed.

### FINDING-26 [🟡] (design-09)
**GOAL:** GOAL-9.3 / GOAL-9.5
**Issue:** Requirements say `on_event(EventType, callback)` is on the builder, enforced at compile time. The design says "The builder owns the registry until `build()` transfers it to the engine." and "EngineBuilder::on_event(EventType, callback) delegates to CallbackRegistry::register()." However, there's no mechanism shown to prevent calling `on_event` after `build()` — this would need a typestate pattern or simply not exposing `on_event` on `GEPAEngine`. The design mentions compile-time enforcement but doesn't show the typestate or method-unavailability mechanism.
**Fix:** Minor — add a note that `on_event` is only on the `EngineBuilder` type, not on `GEPAEngine`. Since `build()` consumes the builder, calling `on_event` after `build()` is a compile error (builder moved). This is implicit in the consumed-self builder pattern but should be stated explicitly.

### FINDING-27 [🟡] (design-09)
**GOAL:** GOAL-9.1a / GUARD-8
**Issue:** `GEPAEvent` derives `Clone`, but `Instant` (in `IterationStarted`) is `Clone` — OK. However, `GEPAEvent` does NOT derive `Serialize/Deserialize` — the design explicitly says "Events are not Serialize/Deserialize — they are transient in-process notifications." This is fine for the requirements (no GOAL requires event serialization), but the `TracingCallback` logs events at `trace` level using `Debug`. If anyone wants structured JSON logging of events, they'd need to manually serialize. This is a design choice, not a bug.
**Fix:** None needed — just noting the design decision is intentional and acceptable.

---

## Summary

### Design 07 — Config
**Verdict: ✅ PASS with minor notes**
- All 7 GOALs (7.1–7.7) have corresponding design mechanisms
- Rust code snippets are valid (builder pattern, derive macros, enums)
- Validation logic is complete and explicit
- 2 yellow findings: first-error-only validation (acceptable), base_delay validation edge case with retry_max=0
- No contradictions with master design

### Design 08 — Data Loading
**Verdict: ✅ PASS with minor gaps**
- All 9 GOALs (8.1–8.7, with 8.5a/b/c) have corresponding design mechanisms
- Algorithms are well-specified (MinibatchIterator, BackfillScheduler, overfitting delta)
- 4 yellow findings: Result return type deviation from reqs, backfill candidate count unspecified, validation error policy semantics, ExampleId→Example lookup gap
- No contradictions with master design

### Design 09 — Events
**Verdict: ⚠️ PASS with 1 required fix**
- All 6 GOALs (9.1a, 9.1b, 9.3, 9.4, 9.5, 9.6) have corresponding design mechanisms
- 1 red finding: `CandidateAccepted` payload mismatch (`candidate_id` vs `candidate: Candidate`) — must fix
- 1 yellow: `callback_fn()` undefined method in code snippet — code won't compile
- 1 yellow: compile-time enforcement mechanism for `on_event` not explicitly shown
- Performance analysis is thorough and well within GUARD-6 budget

### Totals
- 🔴 Red (must fix): **1** (FINDING-20)
- 🟡 Yellow (should fix/clarify): **8** (FINDING-3, 8, 9, 13, 17, 19, 24, 26)
- 🟢 Green (good): **18** (FINDING-1, 2, 4, 5, 6, 7, 10, 11, 12, 14, 15, 16, 18, 21, 22, 23, 25, 27)
