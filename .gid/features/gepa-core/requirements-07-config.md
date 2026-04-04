# Requirements: GEPA Configuration

> Feature 7 of 9 — Master doc: `requirements-master.md`

`GEPAConfig` controls all tunable parameters of the engine, proposers, and evaluation. Sensible defaults for all parameters. Invalid configurations are rejected at construction time with descriptive errors.

## Goals

- **GOAL-7.1** [P0]: `GEPAConfig` includes at minimum: maximum iterations, minibatch size (number of examples per evaluation), stagnation limit (iterations without improvement before termination), checkpoint interval, Pareto front maximum size, optional RNG seed (`Option<u64>`, default: random — if provided, enables deterministic runs per GUARD-9), max consecutive skips (`max_consecutive_skips`, default: 5, see GOAL-7.5), error policy (skip vs halt, default: skip, see GOAL-7.5), retry max (`retry_max`, default: 3), backoff strategy (fixed/exponential, default: exponential), base retry delay (default: 1s), re-evaluation interval (`re_eval_interval`, default: 5, in iterations — see GOAL-8.5), re-evaluation sample size (`re_eval_sample_size`, default: equal to `minibatch_size` — see GOAL-8.5), minimum shared examples for dominance (`min_shared_examples`, default: equal to `minibatch_size` — see GOAL-2.1), and maximum re-evaluation calls per iteration (`max_re_eval_per_iteration`, default: `pareto_max_size × minibatch_size / 2` — see GOAL-1.7c). The full parameter list is specified across GOAL-7.1 through GOAL-7.7; this goal defines the core set that every GEPAConfig must include.

- **GOAL-7.2** [P0]: All config parameters have sensible defaults. A user can construct `GEPAConfig::default()` and run the engine without setting any parameter. Defaults: max_iterations=100, minibatch_size=16, stagnation_limit=20, checkpoint_interval=1, pareto_max_size=50.

- **GOAL-7.3** [P0]: Invalid config is rejected at construction time with a descriptive error message. Invalid conditions include: minibatch_size=0, max_iterations=0, stagnation_limit > max_iterations, Pareto front max_size < 1, min_shared_examples=0, min_shared_examples > total training examples (checked at run start, not construction).

- **GOAL-7.4** [P1]: `GEPAConfig` is serializable (serde) so it can be saved alongside checkpoints for full reproducibility of a run.

- **GOAL-7.5** [P1]: Config includes retry policy for adapter errors: max retries per call (default: 3), backoff strategy (fixed or exponential, default: exponential), and base delay (default: 1 second). After exhausting retries, the engine either skips the iteration or halts, based on a configurable error policy (skip vs halt, default: skip). **Interaction with stagnation (GOAL-1.2b):** a skipped iteration does NOT count toward the stagnation counter — stagnation only increments when a mutation was attempted and the resulting candidate was rejected as dominated. Skipped iterations are tracked separately in run statistics (GOAL-6.5) as `skipped_iterations`. If consecutive skipped iterations exceed `max_consecutive_skips` (configurable, default: 5), the engine halts with termination reason `TooManySkips` regardless of the error policy, to prevent infinite loops on persistently failing adapters.

- **GOAL-7.6** [P2]: Config includes optional wall-clock time budget (Duration). The engine checks elapsed time at the start of each iteration and terminates gracefully if the budget is exceeded. This is the configuration surface for the stopping criterion described in GOAL-1.2a.

- **GOAL-7.7** [P2]: Config includes merge proposer settings: enabled (bool, default: false), merge interval (every N iterations, default: 10), and merge selection strategy (complementary vs random).

## Cross-references

- GOAL-1.2a-d (Core Engine) — stopping criteria
- GOAL-2.1 (Pareto Front) — `min_shared_examples`
- GOAL-8.5 (Data Loading) — re-evaluation interval and sample size
- GUARD-9 — deterministic RNG seed

**Summary: 7 GOALs** (3 P0, 2 P1, 2 P2)
