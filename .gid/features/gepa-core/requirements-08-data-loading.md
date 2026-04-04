# Requirements: GEPA Data Loading

> Feature 8 of 9 — Master doc: `requirements-master.md`

The `DataLoader` trait provides training and validation examples to the engine. Abstracts over data sources so consumers can load from files, databases, or generate dynamically.

## Goals

- **GOAL-8.1** [P0]: `DataLoader` is a trait with methods: `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>`. The engine uses training examples for the optimization loop and validation examples for final evaluation of the result.

- **GOAL-8.2** [P0]: `Example` contains at minimum: a unique ID (string) and an input payload (string or structured data via `serde_json::Value`). Optional fields: expected output (for reference), metadata (key-value pairs), and difficulty tag.

- **GOAL-8.3** [P0]: The engine samples minibatches from training examples each iteration. Each training example is used at least once before any example is reused (epoch-based coverage), provided the total number of evaluations (iterations × minibatch_size) is ≥ the training set size. When total evaluations < training set size (e.g., 10 iterations × 16 batch = 160 evaluations but 1000 examples), the engine samples uniformly without replacement within each epoch-segment. Minibatch composition varies across iterations to prevent overfitting to a fixed subset. **Epoch boundary behavior:** when fewer than `minibatch_size` examples remain in the current epoch, the engine fills the minibatch by concatenating the remaining examples with the beginning of the next epoch (shuffled with the seeded RNG). This ensures every example is used exactly once per epoch and no examples are wasted. Example: 100 training examples, minibatch_size=16 → batch 7 gets the last 4 from epoch 1 + the first 12 from epoch 2.

- **GOAL-8.4** [P1]: The `DataLoader` trait supports async loading (`async fn`) for consumers that need to fetch examples from network sources or databases. Async DataLoader calls have a configurable timeout (default: 30s). If loading fails or times out, the engine retries up to 3 times, then halts with `GEPAError::AdapterError { source: 'DataLoader timeout', retryable: false }`.

- **GOAL-8.5** [P1]: The engine tracks which examples each candidate has been evaluated on via the evaluation cache (GOAL-6.3). **Score matrix backfill:** every `re_eval_interval` iterations (configurable, default: 5 — see GOAL-7.1), the engine selects front candidates with the sparsest score coverage (fewest evaluated examples; ties broken by candidate age — newest first — for GUARD-9 determinism) and evaluates them on examples they haven't seen (sample size = `re_eval_sample_size`, configurable, default: `minibatch_size` — see GOAL-7.1). New scores are written to the evaluation cache, progressively filling the score matrix. **Front recomputation:** after each backfill round, the engine recomputes dominance relationships across the front using the updated score coverage (see GOAL-2.2). Candidates that are now dominated (because sufficient shared examples reveal dominance) are removed from the front. This is the mechanism by which the front converges: early iterations grow the front (few shared examples → most candidates are non-dominating), later iterations shrink it (dense score matrix → dominance becomes detectable). **Overfitting detection:** the engine computes per-candidate overfitting delta (difference between average training score and average re-evaluation score) and reports it in events (`ReEvaluationCompleted`) and statistics. High overfitting delta influences selection (GOAL-2.3) but does not directly remove candidates — only dominance does.

- **GOAL-8.6** [P0]: After the optimization loop terminates, the engine evaluates all Pareto front candidates on the full validation set (from `DataLoader::validation_examples()`). The `GEPAResult` includes validation scores for each front candidate, enabling the consumer to select the best candidate based on held-out data rather than training performance. If `validation_examples()` returns empty, the engine skips final validation and reports training-only scores in `GEPAResult` (with a `validation_skipped: true` flag).

- **GOAL-8.7** [P0]: The engine validates DataLoader output at startup before entering the optimization loop. If `training_examples()` returns empty, the engine returns `Err(GEPAError::EmptyDataError)` immediately — optimization cannot proceed without training data. If `validation_examples()` returns empty, the engine proceeds but emits a warning event (`DataLoaderWarning { message }`) via the callback system and sets `validation_skipped: true` in the result.

## Cross-references

- GOAL-1.4 (Core Engine) — minibatch passed to adapter execute
- GOAL-6.3 (State) — evaluation cache for score tracking
- GOAL-7.1 (Config) — `re_eval_interval`, `re_eval_sample_size`, `minibatch_size`
- GUARD-9 — deterministic sampling with seeded RNG

**Summary: 7 GOALs** (4 P0, 2 P1, 1 P2)
