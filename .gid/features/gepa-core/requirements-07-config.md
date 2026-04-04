# Requirements: GEPA Configuration

> Feature 7 of 9 — Master doc: `requirements-master.md`

`GEPAConfig` controls all tunable parameters of the engine, proposers, and evaluation. Sensible defaults for all parameters. Invalid configurations are rejected at construction time with descriptive errors.

## Goals

- **GOAL-7.1** [P0]: `GEPAConfig` includes all parameters defined in GOAL-7.2 through GOAL-7.7, plus: optional RNG seed (`Option<u64>`, default: random — per GUARD-9), `min_shared_examples` (default: `minibatch_size` — per GOAL-2.1), `max_re_eval_per_iteration` (default: `pareto_max_size × minibatch_size / 2` — per GOAL-1.7c), `max_lesson_depth` (default: 10 — per GOAL-4.2b), and `eval_cache_max_size: Option<usize>` (default: `None` = unlimited — per GOAL-6.4). The full parameter list is the union of all fields specified across GOAL-7.1 through GOAL-7.7.

- **GOAL-7.2** [P0]: All config parameters have sensible defaults. A user can construct `GEPAConfig::default()` and run the engine without setting any parameter. Defaults: max_iterations=100, minibatch_size=16, stagnation_limit=20, checkpoint_interval=1, pareto_max_size=50.

- **GOAL-7.3** [P0]: Invalid config is rejected at construction time with a descriptive error message. Invalid conditions include: minibatch_size=0, max_iterations=0, stagnation_limit > max_iterations, Pareto front max_size < 1, min_shared_examples=0, min_shared_examples > total training examples (checked at run start, not construction), checkpoint_interval=0, re_eval_interval=0, re_eval_sample_size=0, max_re_eval_per_iteration=0 (use None/disabled pattern instead), merge_interval=0 when merge enabled, retry_max with base_delay=0 when strategy=exponential. Valid edge cases (explicitly allowed): retry_max=0 (no retries, immediate skip/halt), max_consecutive_skips=0 (halt on first skip), time_budget of Duration::ZERO (immediately terminate — treated as "run at most 1 iteration").

- **GOAL-7.4** [P1]: `GEPAConfig` is serializable (serde) so it can be saved alongside checkpoints for full reproducibility of a run.

- **GOAL-7.5** [P0]: Config includes retry policy for adapter errors: max retries per call (`retry_max`, default: 3), backoff strategy (fixed or exponential, default: exponential), base delay (`base_delay`, default: 1 second), and max retry delay (`max_retry_delay`, default: 60s — caps the computed backoff delay; effective delay = min(computed_backoff, max_retry_delay)). When the adapter returns `RateLimited { retry_after: Some(d) }`, the engine uses `max(d, computed_backoff)` as the delay before the next retry; when `retry_after` is None, the engine uses the configured backoff strategy. After exhausting retries, the engine either skips the iteration or halts, based on a configurable error policy (`ErrorPolicy::Skip` vs `ErrorPolicy::Halt`, default: Skip). **Interaction with stagnation (GOAL-1.2b):** a skipped iteration does NOT count toward the stagnation counter — stagnation only increments when a mutation was attempted and the resulting candidate was rejected as dominated. Skipped iterations are tracked separately in run statistics (GOAL-6.5) as `skipped_iterations`. If consecutive skipped iterations exceed `max_consecutive_skips` (configurable, default: 5), the engine halts with termination reason `TooManySkips` regardless of the error policy, to prevent infinite loops on persistently failing adapters.

- **GOAL-7.6** [P0]: Config includes optional wall-clock time budget (Duration). Wall-clock timer starts when `engine.run()` begins (after any resumption setup). The engine checks elapsed time at the start of each iteration BEFORE the Select step and terminates gracefully if the budget is exceeded. An iteration that started within budget runs to completion even if it exceeds the budget during execution. Termination reason: per GOAL-1.2a. This is the configuration surface for the stopping criterion described in GOAL-1.2a.

- **GOAL-7.7** [P2]: Config includes merge proposer settings: enabled (bool, default: false), merge interval (every N iterations, default: 10), and merge selection strategy (complementary per GOAL-4.4, or random, default: complementary). All merge selection ties are broken using the seeded RNG per GUARD-9.

### Applicable GUARDs

- **GUARD-4** (no panics) — invalid config returns Result, never panics
- **GUARD-8** (Debug impls) — GEPAConfig implements Debug

## Cross-references

- GOAL-1.2a-d (Core Engine) — stopping criteria
- GOAL-2.1 (Pareto Front) — `min_shared_examples`
- GOAL-8.5 (Data Loading) — re-evaluation interval and sample size
- GUARD-9 — deterministic RNG seed

**Summary: 7 GOALs** (4 P0, 1 P1, 2 P2)
