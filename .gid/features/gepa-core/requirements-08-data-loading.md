# Requirements: GEPA Data Loading

> Feature 8 of 9 — Master doc: `requirements-master.md`

The `DataLoader` trait provides training and validation examples to the engine. Abstracts over data sources so consumers can load from files, databases, or generate dynamically.

## Goals

- **GOAL-8.1** [P0]: `DataLoader` is a trait with methods: `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>`. The engine uses training examples for the optimization loop and validation examples for final evaluation of the result. For v1, examples are loaded eagerly into memory. Streaming/iterator-based loading is a future enhancement (not in scope).

- **GOAL-8.2** [P0]: `Example` contains at minimum: a unique ID (string) and an input payload (string or structured data via `serde_json::Value`). Optional fields: expected output (for reference), metadata (key-value pairs), and difficulty tag (optional `String`, opaque to the engine — available for consumer-side logic such as curriculum learning).

- **GOAL-8.3** [P0]: The engine samples minibatches from training examples each iteration. Each training example is used at least once before any example is reused (epoch-based coverage), provided the total number of evaluations (iterations × minibatch_size) is ≥ the training set size. When total evaluations < training set size (e.g., 10 iterations × 16 batch = 160 evaluations but 1000 examples), the engine samples uniformly without replacement within each epoch-segment. Minibatch composition varies across iterations to prevent overfitting to a fixed subset. **Epoch boundary behavior:** when fewer than `minibatch_size` examples remain in the current epoch, the engine fills the minibatch by concatenating the remaining examples with the beginning of the next epoch (shuffled with the seeded RNG). This ensures every example is used exactly once per epoch and no examples are wasted. Example: 100 training examples, minibatch_size=16 → batch 7 gets the last 4 from epoch 1 + the first 12 from epoch 2. **Edge case:** if `minibatch_size` ≥ number of training examples, each minibatch contains all training examples (effectively full-batch mode). The actual batch size equals `min(minibatch_size, training_set_size)`. No padding or duplication occurs. Single-example training sets are valid (degenerate but functional).

- **GOAL-8.4** [P1]: The `DataLoader` trait supports async loading (`async fn`) for consumers that need to fetch examples from network sources or databases. Async DataLoader calls have a configurable timeout (`data_loader_timeout_secs: u64`, default: 30 — see GOAL-7.1). If async loading fails with a retryable error (timeout, connection error) or times out, the engine retries up to 3 times with backoff (per GOAL-7.5 backoff strategy). After exhausting retries, the engine halts with `GEPAError::AdapterError { source, retryable: false }`. Non-retryable errors (e.g., deserialization failure) cause immediate halt without retry.

- **GOAL-8.5a** [P1]: **Score matrix backfill** — The engine tracks which examples each candidate has been evaluated on via the evaluation cache (GOAL-6.3). Every `re_eval_interval` iterations (configurable, default: 5 — see GOAL-7.1), the engine selects front candidates with the sparsest score coverage (fewest evaluated examples; ties broken by candidate age — newest first — for GUARD-9 determinism) and evaluates them on examples they haven't seen using the adapter's `evaluate` method (GOAL-3.5, since only numeric scores are needed). Sample size = `re_eval_sample_size` (configurable, default: `minibatch_size` — see GOAL-7.1). New scores are written to the evaluation cache, progressively filling the score matrix.

- **GOAL-8.5b** [P1]: **Front recomputation after backfill** — After each backfill round (GOAL-8.5a), the engine recomputes dominance relationships across the front using the updated score coverage (see GOAL-2.2). Candidates that are now dominated (because sufficient shared examples reveal dominance) are removed from the front. This is the convergence mechanism: early iterations grow the front (few shared examples → most candidates are non-dominating), later iterations shrink it (dense score matrix → dominance becomes detectable).

- **GOAL-8.5c** [P1]: **Overfitting detection** — The engine computes per-candidate overfitting delta (difference between average training score and average re-evaluation score) after backfill rounds. Reported via `ReEvaluationCompleted` event and statistics. High overfitting delta influences selection (GOAL-2.3) but does not directly remove candidates — only dominance does.

- **GOAL-8.6** [P0]: After the optimization loop terminates, the engine evaluates all Pareto front candidates on the full validation set (from `DataLoader::validation_examples()`). The `GEPAResult` includes validation scores for each front candidate, enabling the consumer to select the best candidate based on held-out data rather than training performance. If `validation_examples()` returns empty, the engine skips final validation and reports training-only scores in `GEPAResult` (with a `validation_skipped: true` flag). Validation evaluation emits progress events (`ValidationProgress { candidate_index, total_candidates }`) via the callback system. If the adapter's `evaluate` call fails during validation, the same error/retry policy from GOAL-7.5 applies.

- **GOAL-8.7** [P0]: The engine validates DataLoader output at startup before entering the optimization loop. If `training_examples()` returns empty, the engine returns `Err(GEPAError::EmptyDataError)` immediately — optimization cannot proceed without training data. If `validation_examples()` returns empty, the engine proceeds but emits a warning event (`DataLoaderWarning { message }`) via the callback system and sets `validation_skipped: true` in the result.

### Applicable GUARDs

- **GUARD-2** (determinism) — seeded RNG for sampling, deterministic tie-breaking in backfill
- **GUARD-8** (Debug/Error impls) — `Example` and `DataLoader` types must implement Debug

## Cross-references

- GOAL-1.4 (Core Engine) — minibatch passed to adapter execute
- GOAL-2.2 (Pareto Front) — front recomputation after backfill
- GOAL-2.3 (Pareto Front) — overfitting delta in selection
- GOAL-3.5 (Adapter) — evaluate method used for backfill
- GOAL-6.3 (State) — evaluation cache for score tracking
- GOAL-7.1 (Config) — `re_eval_interval`, `re_eval_sample_size`, `minibatch_size`, `data_loader_timeout_secs`
- GOAL-7.5 (Config) — backoff/retry strategy for adapter errors
- GUARD-9 — deterministic sampling with seeded RNG

**Summary: 9 GOALs** (5 P0, 4 P1) — GOAL-8.5 split into 8.5a/b/c
