# Requirements: GEPA Adapter Interface

> Feature 3 of 9 — Master doc: `requirements-master.md`

The `GEPAAdapter` trait is the integration boundary. Users implement this trait to connect GEPA to their specific LLM provider, evaluation logic, and domain. The crate never calls any LLM API directly.

## Goals

- **GOAL-3.1** [P0]: `GEPAAdapter` is an async trait requiring `Send + Sync + 'static` with the following required methods: `execute` (run candidate on examples, return execution traces with optional per-example ASI — used during the Reflect step to provide rich diagnostic context), `reflect` (analyze traces, return diagnosis), `mutate` (produce improved candidate from parent + reflection), and `evaluate` (score a candidate on a batch of examples, returning only numeric scores — used for acceptance testing and Pareto front updates). `execute` and `evaluate` differ in purpose: `execute` captures rich traces for reflection, `evaluate` produces scores for ranking. All methods must be `Send`. The engine calls adapter methods sequentially within each iteration (one at a time, never concurrently). Adapter implementations may use internal concurrency (e.g., parallel LLM calls within `execute`) as long as they return a single result. The engine runs one iteration at a time (no concurrent iterations).

- **GOAL-3.2** [P0]: The `execute` method receives a `&Candidate` and a slice of input examples, and returns a `Vec<ExecutionTrace>` where each trace contains: the input, the generated output, an optional score, and optional actionable side information (ASI) as a free-form string. **Output contract:** The returned `Vec<ExecutionTrace>` has exactly one entry per input example, in the same order. If execution fails for a specific example, the adapter sets the trace's output to an empty string and score to `None`, and may include diagnostic information in the ASI field. Whole-batch failures are returned as `Err(GEPAError::AdapterError { ... })`.

- **GOAL-3.3** [P0]: The `reflect` method receives a `&Candidate` and a slice of `ExecutionTrace`s, and returns a `Reflection` containing: a natural-language diagnosis string and a list of suggested improvement directions.

- **GOAL-3.4** [P0]: The `mutate` method receives the parent `&Candidate`, the `&Reflection`, and a slice of ancestor lessons (accumulated from the lineage), and returns a new `Candidate` with potentially modified text parameters. **Boundary condition:** for seed candidates with no lineage, the ancestor lessons slice is empty.

- **GOAL-3.5** [P0]: The `evaluate` method receives a `&Candidate` and a slice of input examples, and returns a `Vec<f64>` of per-example scores. **Output contract:** The returned `Vec<f64>` has exactly one entry per input example, in the same order. Scores follow the semantics defined in GUARD-10 (higher is better, finite f64). This method is used for acceptance testing and Pareto front updates.

- **GOAL-3.6** [P0]: `GEPAAdapter` has an optional method `merge` (default: returns `Err`) that receives two parent candidates and returns a new candidate combining both. This enables the merge proposer (GOAL-1.10). **Call sequence:** the merge step runs as an alternative iteration type where `merge` replaces the `reflect + mutate` steps. The merge iteration sequence is: `select (two candidates) → execute (both on minibatch) → merge → evaluate → accept`. This must be documented in GUARD-3 as a valid alternative sequence.

- **GOAL-3.7** [P0]: All adapter methods return `Result<T, GEPAError>` so that LLM failures, timeouts, and rate limits can be propagated cleanly. The engine handles adapter errors according to a configurable retry policy (GOAL-7.5). `GEPAError` is an enum with the following adapter-relevant variants: `AdapterError { source: String, retryable: bool }` (generic adapter failure with retryability hint), `Timeout` (adapter call exceeded time limit), `RateLimited { retry_after: Option<Duration> }` (LLM rate limit, with optional backoff hint from the provider), and `Cancelled` (run was cancelled via cancellation mechanism). The engine inspects `retryable` and `retry_after` to decide whether to retry or skip/halt per GOAL-7.5. Non-retryable errors skip immediately without using retry budget.

- **GOAL-3.8** [P2]: An adapter implementer can create a minimal working adapter by implementing only the 4 required methods (`execute`, `reflect`, `mutate`, `evaluate`) with < 50 lines of boilerplate. Optional methods (`merge`) have sensible defaults. The trait documentation includes a complete example adapter implementation.

## Applicable GUARDs

- **GUARD-3** — Adapter call order (select → execute → reflect → mutate → evaluate → accept)
- **GUARD-5** — No LLM calls in core (all LLM interaction via adapter trait)
- **GUARD-8** — Debug/Error impls on all public types
- **GUARD-10** — Score semantics (evaluate returns scores per GUARD-10)
- **GUARD-11** — Async compatibility (adapter must be Send + Sync + 'static)

## Cross-references

- GOAL-1.x (Core Engine) — calls adapter methods in sequence
- GOAL-7.5 (Config) — retry policy for adapter errors

**Summary: 8 GOALs** (6 P0, 1 P1, 1 P2)
