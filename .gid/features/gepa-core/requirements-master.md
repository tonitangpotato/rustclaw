# Requirements: gepa-core (Master)

## Overview

`gepa-core` is a Rust crate implementing the GEPA (Genetic-Pareto) prompt evolution algorithm from the ICLR 2026 Oral paper by Stanford/Berkeley (Omar Khattab, Matei Zaharia). GEPA is the state-of-the-art prompt optimization algorithm that treats prompt engineering as a multi-objective evolutionary search problem.

The crate provides the core optimization loop (Select → Execute → Reflect → Mutate → Accept), Pareto front management, and an adapter-based interface that allows any consumer to plug in their own LLM provider and evaluation logic. The crate itself never calls any LLM API directly — it is purely algorithmic infrastructure.

**Core Algorithm (5-step loop):**
1. **Select** — Pick a candidate from the Pareto front (candidates that are best on different task subsets)
2. **Execute** — Run candidate on a minibatch, capture full execution traces (reasoning, tool calls, outputs, errors)
3. **Reflect** — Teacher LLM reads traces, diagnoses failure causes, proposes improvement directions
4. **Mutate** — Generate improved candidate based on reflection + accumulated lessons from all ancestors
5. **Accept** — If new candidate improves on parent, accept and update Pareto front

**First consumer:** RustClaw Self-Improvement System — SkillAdapter (evolve skill definitions), SystemPromptAdapter (optimize prompt sections), RitualAdapter (optimize ritual/harness task prompts).

## Priority Levels

- **P0**: Core — the crate cannot function without this
- **P1**: Production-quality — needed for reliable real-world usage
- **P2**: Enhancement — improves efficiency, observability, or developer experience

## Guard Severity

- **hard**: Violation = system is broken, results are invalid, must halt
- **soft**: Violation = degraded quality or performance, warn but continue

## Terminology

Throughout this document, "task subset" refers to a subset of training examples. The Pareto front maintains per-example scores; "excels on different task subsets" means a candidate scores higher on certain examples than others. "Example" and "task subset" are used at different levels of granularity: individual scores are per-example, while Pareto diversity is described in terms of task subsets (groups of examples).

### Example Types

- **Training examples** — The full set of examples returned by `DataLoader::training_examples()`. Used for the optimization loop: minibatch sampling, evaluation, and re-evaluation backfill.
- **Validation examples** — The set returned by `DataLoader::validation_examples()`. Used only for final evaluation after the optimization loop terminates (GOAL-8.6). Never used during optimization.
- **Shared examples** — The intersection of examples on which two specific candidates have both been evaluated. Used for dominance comparison (GOAL-2.1). This is a per-pair concept, not a global set.

### Candidate Terminology

- **Candidate** — An immutable set of named text parameters (e.g., "system_prompt", "tool_description") with metadata. The unit of evolution.
- **Candidate prompt** — Synonym for "candidate" when emphasizing that the text parameters represent a prompt being optimized. Avoid using bare "prompt" which is ambiguous.
- **Seed candidate** — A candidate provided by the user at engine construction time, before optimization begins.

### GOAL Numbering Convention

GOALs are numbered `X.Y` where X is the feature number (1–9) and Y is a sequential index within that feature. Sub-items within a GOAL use lowercase letter suffixes (e.g., GOAL-1.7a, GOAL-1.7b, GOAL-1.7c, GOAL-1.7d) to denote tightly coupled aspects of the same requirement that share a common prefix. Sub-items are counted as part of their parent GOAL for numbering purposes but may be independently testable.

## Feature Index

| # | Feature | File | GOALs | Priority Mix | Dependencies |
|---|---------|------|-------|-------------|-------------|
| 1 | Core Engine | `requirements-01-core-engine.md` | 20 | 17 P0, 2 P1, 1 P2 | — (foundational) |
| 2 | Pareto Front | `requirements-02-pareto-front.md` | 6 | 3 P0, 3 P1 | depends on: 1, 5, 6 |
| 3 | Adapter Interface | `requirements-03-adapter.md` | 8 | 6 P0, 1 P1, 1 P2 | depends on: 5 |
| 4 | Proposers | `requirements-04-proposers.md` | 5 | 2 P0, 1 P1, 2 P2 | depends on: 2, 3 |
| 5 | Candidate Management | `requirements-05-candidates.md` | 6 | 4 P0, 2 P1 | — (foundational) |
| 6 | State Management | `requirements-06-state.md` | 6 | 3 P0, 2 P1, 1 P2 | depends on: 2, 5 |
| 7 | Configuration | `requirements-07-config.md` | 7 | 5 P0, 1 P1, 1 P2 | — (foundational) |
| 8 | Data Loading | `requirements-08-data-loading.md` | 7 | 5 P0, 2 P1, 0 P2 | depends on: 3, 6 |
| 9 | Callback / Events | `requirements-09-events.md` | 5 | 2 P0, 3 P1 | depends on: 5, 6 |

**Total: 70 GOALs** (44 P0 / 19 P1 / 7 P2) + **11 GUARDs** (7 hard / 4 soft)

## Guards

- **GUARD-1** [hard]: All Pareto front operations must maintain the invariant: no candidate in the front is dominated by any other candidate in the front. After every update (add, remove, re-evaluate), this invariant is verified in debug builds via `debug_assert!`.

- **GUARD-2** [hard]: The engine must not modify any candidate after creation. Candidates are immutable value types. Mutation produces new candidates; it never modifies existing ones.

- **GUARD-3** [hard]: The engine must not call any adapter method outside the optimization loop's defined sequence. The call order per iteration is: select → execute → reflect → mutate → evaluate → accept. No adapter call is made during Pareto front maintenance, serialization, or statistics gathering.

- **GUARD-4** [hard]: Checkpoint files must be written atomically (write to temp file, then rename). A crash at any point during checkpoint writing must not corrupt the existing checkpoint.

- **GUARD-5** [hard]: The crate must never make any LLM API call or network request directly. All LLM interaction goes through the `GEPAAdapter` trait. The crate's dependency list must not include any LLM SDK, HTTP client, or network library.

- **GUARD-6** [soft]: Single-iteration wall-clock time should be dominated by adapter calls (LLM), not by engine overhead. Engine-internal computation (Pareto updates, candidate management, serialization) should add < 5% overhead relative to adapter call time for typical workloads.

- **GUARD-7** [soft]: Memory usage grows linearly with the number of candidates in history, not quadratically. Storing 1,000 candidates with 10 text parameters of ~1KB each should use < 50MB of heap memory (excluding adapter-side allocations).

- **GUARD-8** [soft]: All public types in the crate implement `Debug`. All error types implement `std::error::Error` and `Display` with descriptive messages. No `.unwrap()` or `.expect()` on fallible operations in library code (only in tests).

- **GUARD-9** [hard]: The engine must be deterministic given the same RNG seed, config, adapter responses, and data ordering. Two runs with identical inputs (including a user-provided RNG seed) produce identical candidate histories, Pareto fronts, and final results. All engine-internal randomness (minibatch sampling, Pareto front selection, tie-breaking, etc.) draws from a single seeded RNG instance in a deterministic call order — the RNG is never accessed from multiple threads concurrently, and the sequence of draw operations is fixed by the algorithm structure. Non-determinism may only come from the adapter (LLM responses).

- **GUARD-10** [hard]: Score semantics — All per-example scores are `f64` values where **higher is better**. Valid score range: any finite `f64` (negative values are permitted). `NaN` scores are treated as evaluation failures: the engine replaces `NaN` with `None` in the evaluation cache (the example is treated as unevaluated for that candidate). `+Inf` and `-Inf` are forbidden — the engine clamps infinite scores to `f64::MAX` and `f64::MIN` respectively and emits a warning event. All Pareto dominance comparisons, crowding distance calculations, and "best score" selections assume this higher-is-better convention.

- **GUARD-11** [soft]: Async compatibility — All public types must be `Send + Sync` where required for use in multi-threaded async runtimes (e.g., tokio). The `GEPAAdapter` trait must require `Send + Sync + 'static`. The `DataLoader` trait must require `Send + Sync + 'static`. The engine's `run()` future must be `Send`. The crate is tested against tokio's multi-threaded runtime but is designed to be runtime-agnostic — it uses no tokio-specific APIs beyond `tokio::time` for timeouts. Alternative runtimes (e.g., async-std) should work but are not tested.

## Risks

High-risk GOALs requiring prototype/spike before full implementation:

- **Score alignment with sparse matrices (GOAL-1.7a)** — Dominance comparison on sparse, incrementally-filled score matrices is algorithmically subtle. Correctness of intersection-based comparison and minimum shared examples threshold needs validation with synthetic data before production implementation.
- **Crowding distance at high M (GOAL-2.4)** — Crowding distance computation with many objectives (high M) may produce unintuitive pruning decisions. Needs prototyping to validate that the right candidates are preserved.
- **Epoch boundary sampling correctness (GOAL-8.3)** — Epoch-based sampling with boundary concatenation and seeded RNG must guarantee every example is used exactly once per epoch. Off-by-one errors here would silently bias the optimization. Requires thorough property-based testing.

## Non-Goals\n\nThe following are explicitly **not** goals for `gepa-core` v1:\n\n- **Multi-objective score aggregation** \u2014 The crate does not aggregate per-example scores into a single fitness value. Pareto dominance operates on per-example scores directly.\n- **Automatic hyperparameter tuning** \u2014 Config parameters (minibatch size, stagnation limit, etc.) are set by the user. The crate does not auto-tune them.\n- **Concurrent/parallel iterations** \u2014 The engine runs one iteration at a time. Parallelizing across iterations or across candidates within an iteration is out of scope.\n- **Warm-starting from a different run\u2019s checkpoint** \u2014 Checkpoints are tied to a specific run\u2019s candidate IDs and evaluation cache. Cross-run checkpoint migration is not supported.\n- **Pluggable pruning strategies** \u2014 Crowding distance is the only pruning strategy. A trait-based pluggable strategy may be added in a future version if crowding distance proves insufficient at high M.\n- **Score normalization** \u2014 The crate assumes scores are on a comparable scale across examples. If scores from different examples have different ranges, the adapter must normalize them before returning.\n\n## Out of Scope

- **No LLM integration**: The crate does not include any LLM client or API wrapper. Consumers bring their own via the adapter trait.
- **No domain-specific logic**: The crate knows nothing about prompts, skills, tools, or any particular optimization target. It optimizes opaque string parameters.
- **No UI/CLI**: This is a library crate only. No binary, no CLI, no web interface.
- **No distributed execution**: Single-process only. Distributed GEPA across multiple machines is a future concern.
- **No gradient-based optimization**: GEPA is purely evolutionary/LLM-based. No autodiff, no numerical optimization.
- **No built-in persistence backend**: Checkpoint is JSON file. Database backends, cloud storage, etc. are the consumer's responsibility.

## Dependencies (Allowed)

- **serde + serde_json** — Serialization for checkpoints, candidates, config
- **tokio** — Async runtime for adapter calls
- **async-trait** — Async trait support for GEPAAdapter and DataLoader
- **tracing** — Structured logging and diagnostics
- **rand** — Minibatch sampling, Pareto front selection randomness — with explicit PRNG algorithm (e.g., `rand_chacha::ChaCha8Rng`) for cross-version determinism. Do not use `rand::thread_rng()` or `StdRng` which may change algorithms across versions.
- **thiserror** — Error type derivation

No other dependencies without explicit justification. In particular: no HTTP clients, no LLM SDKs, no database drivers, no UI frameworks.
