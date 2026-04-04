# Requirements: gepa-core

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

## Goals

### 1. Core Engine

The main GEPA optimization loop. Orchestrates the Select → Execute → Reflect → Mutate → Accept cycle, delegates to adapter for LLM calls, and terminates based on configured stopping criteria.

- **GOAL-1.0** [P1]: The complete happy path is: construct `GEPAConfig` (with defaults or custom) → construct `GEPAEngine` with config + adapter + data_loader + seed candidates → call `engine.run()` → receive `GEPAResult`. The engine builder validates all inputs at construction time (GOAL-7.3) so `run()` cannot fail due to misconfiguration.

- **GOAL-1.1** [P0]: `GEPAEngine::run()` executes the full optimization loop: select a candidate from the Pareto front, execute it on a minibatch, reflect on traces, mutate to produce a new candidate, and accept/reject based on score comparison. Each iteration performs all 5 steps in order.

- **GOAL-1.2** [P0]: The engine terminates when any configured stopping criterion is met: maximum number of iterations reached, wall-clock time budget exhausted, no improvement observed for N consecutive iterations (stagnation), too many consecutive adapter failures (GOAL-7.5), or cancellation via callback (GOAL-3.7). Stagnation is defined as: no new candidate accepted to the Pareto front for N consecutive iterations where a mutation was actually attempted and rejected as dominated. Skipped iterations (due to adapter errors) do NOT count toward the stagnation counter — see GOAL-7.5 for skip/stagnation interaction. The specific criterion that triggered termination is reported in the result as one of: `MaxIterations`, `TimeBudget`, `Stagnation`, `TooManySkips`, `Cancelled`.

- **GOAL-1.3** [P0]: At the start of each iteration, the engine selects a parent candidate from the current Pareto front. Selection must consider candidates' performance across different task subsets (Pareto-aware selection), not just global average score.

- **GOAL-1.4** [P0]: The engine passes the selected candidate to the adapter for execution on a minibatch of training examples. The adapter returns execution traces containing: input, generated output, score, and optional actionable side information (ASI) per example.

- **GOAL-1.5** [P0]: After execution, the engine passes execution traces to the adapter for reflection. The adapter returns a natural-language diagnosis of failure causes and improvement directions. The engine stores this reflection as part of the candidate's lineage.

- **GOAL-1.6** [P0]: After reflection, the engine passes the parent candidate, reflection text, and accumulated ancestor lessons to the adapter for mutation. The adapter returns a new candidate with modified text parameters.

- **GOAL-1.7** [P0]: After mutation, the engine calls the adapter's `evaluate` method (GOAL-3.5) on the new candidate with the current iteration's minibatch (the same minibatch sampled in step 2, on which the parent was executed in GOAL-1.4). `evaluate` is used instead of `execute` because the Accept step only needs numeric scores for dominance comparison, not full execution traces. The resulting per-example scores are stored in the evaluation cache (GOAL-6.3). **Score alignment for dominance:** dominance comparison between any two candidates is performed only on the intersection of examples that both candidates have been evaluated on. The engine also evaluates all current front members on the current minibatch (using cached scores from GOAL-6.3 where available, only calling `evaluate` for uncached `(candidate_id, example_id)` pairs). This ensures that after each Accept step, the new candidate and all front members share at least the current minibatch as common ground. **Minimum shared examples:** dominance can only be established when two candidates share at least `min_shared_examples` evaluated examples (configurable, default: `minibatch_size` — see GOAL-7.1). If the intersection is smaller, the candidates are considered mutually non-dominating (neither can dominate the other). This is conservative by design: early iterations will grow the front as candidates lack sufficient shared evaluations, but the front naturally converges as the score matrix fills via re-evaluation backfill (GOAL-8.5). **Acceptance rule:** the new candidate is accepted if it is non-dominated by any existing front member on their shared examples — i.e., no front member scores ≥ on every shared example and strictly > on at least one. After acceptance, any existing front members now dominated by the new candidate (on their shared examples, with sufficient intersection) are removed. **Edge case: mutual non-dominance** — if the new candidate and an existing front member each score higher on different examples (neither dominates the other), both remain on the front. This is the expected behavior and enables diversity. Front size is bounded by GOAL-2.4. **Re-evaluation cost budget:** front re-evaluation is capped at `max_re_eval_per_iteration` adapter `evaluate` calls per iteration (configurable, default: `pareto_max_size × minibatch_size / 2`). If the cap would be exceeded, only the front members with the stalest (oldest) scores on the current minibatch are re-evaluated, up to the cap. Remaining front members retain cached scores. See GOAL-1.7a through GOAL-1.7d for sub-requirements.

- **GOAL-1.8** [P0]: `GEPAEngine::run()` returns a `GEPAResult` containing: the final Pareto front (all non-dominated candidates), the single best candidate by average score, total iterations run, total wall-clock time, and the termination reason.

- **GOAL-1.9** [P1]: The engine supports resumption from a previously checkpointed `GEPAState`. Calling `GEPAEngine::run()` with a restored state continues optimization from where it left off, preserving the Pareto front, candidate history, and iteration count.

- **GOAL-1.10** [P2]: The engine supports a merge step: periodically (configurable interval), select two Pareto-optimal candidates that excel on different task subsets and ask the adapter to produce a merged candidate combining both strengths. The merged candidate is evaluated and accepted/rejected like any mutation.

### 2. Pareto Front

Multi-objective candidate management. The Pareto front maintains the set of non-dominated candidates — those that are best on at least one task subset. This prevents catastrophic forgetting where improving on one subset regresses another.

- **GOAL-2.1** [P0]: Given a set of candidates with per-example scores stored in the evaluation cache (GOAL-6.3), compute the Pareto front: the subset of candidates where no candidate is dominated by another. Candidate A dominates candidate B if, on the intersection of examples both have been evaluated on (looked up from the evaluation cache, GOAL-6.3), A scores ≥ B on every shared example and strictly > on at least one. Dominance can only be established when the intersection size meets `min_shared_examples` (GOAL-7.1); otherwise, the two candidates are treated as mutually non-dominating.

- **GOAL-2.2** [P0]: When a new candidate is accepted, update the Pareto front: add the new candidate, then remove any existing candidates that are now dominated by it (on shared examples meeting the `min_shared_examples` threshold). The front must remain valid (no dominated candidates) after every update. **Re-evaluation triggered recomputation:** when re-evaluation backfill (GOAL-8.5) adds new scores to the evaluation cache, dominance relationships may change — a previously non-dominating pair may now have sufficient shared examples to establish dominance. After each re-evaluation round, the engine recomputes the front by re-checking all pairwise dominance relationships with updated score coverage. Candidates newly found to be dominated are removed.

- **GOAL-2.3** [P0]: Pareto front selection returns a candidate for mutation. The selection strategy must not always pick the same candidate — it should vary across front members to ensure diversity of exploration. Selection MAY use re-evaluation scores (GOAL-8.5) as a secondary signal to deprioritize candidates with high overfitting delta (large gap between training and re-evaluation scores), but MUST NOT remove candidates from the front based on re-evaluation alone — only the dominance mechanism (GOAL-2.2) removes front members. **Starvation prevention:** overfitting delta deprioritization is bounded: every front member must be selected at least once every `pareto_max_size` iterations (round-robin floor) to prevent starvation. Overfitting delta influences selection order within each round, not exclusion.

- **GOAL-2.4** [P1]: The Pareto front has a configurable maximum size (default: 50, per GOAL-7.2). When the front exceeds the maximum, the least-contributing candidate is removed using **crowding distance** (the candidate with the smallest crowding distance is pruned). Crowding distance is chosen over hypervolume contribution because: (a) it computes in O(N·M·log M) vs O(N^M) for exact hypervolume in high dimensions, (b) GEPA's typical M (number of examples per minibatch, 16-200) makes hypervolume computation intractable, and (c) crowding distance is well-understood from NSGA-II with predictable behavior. Ties in crowding distance are broken by candidate age (oldest removed first). Known limitation: crowding distance becomes less discriminating at high M (>50 examples). This is acceptable for v1 because (a) most real workloads use M=16-64, and (b) the age-based tie-breaker provides a reasonable fallback when crowding distances converge. If empirically problematic, a future version can introduce a pluggable pruning strategy trait.

- **GOAL-2.5** [P1]: Pareto dominance checking for N candidates completes in O(N²·M) time or better, where M is the size of the largest candidate's evaluated example set (the per-pair intersection is computed via sorted example ID merge in O(M)). For typical workloads (N ≤ 100, M ≤ 200), full front recomputation should be negligible relative to adapter call time (~10ms on modern hardware). This is a soft target validated by benchmarks, not a hard SLA.

- **GOAL-2.6** [P1]: The Pareto front is serializable (serde Serialize + Deserialize) for checkpoint/resume. Deserialized front is identical to the original (same candidates, same ordering, same dominance relationships).

### 3. Adapter Interface

The `GEPAAdapter` trait is the integration boundary. Users implement this trait to connect GEPA to their specific LLM provider, evaluation logic, and domain. The crate never calls any LLM API directly.

- **GOAL-3.1** [P0]: `GEPAAdapter` is an async trait with the following required methods: `execute` (run candidate on examples, return execution traces with optional per-example ASI — used during the Reflect step to provide rich diagnostic context), `reflect` (analyze traces, return diagnosis), `mutate` (produce improved candidate from parent + reflection), and `evaluate` (score a candidate on a batch of examples, returning only numeric scores — used for acceptance testing and Pareto front updates). `execute` and `evaluate` differ in purpose: `execute` captures rich traces for reflection, `evaluate` produces scores for ranking.

- **GOAL-3.2** [P0]: The `execute` method receives a `&Candidate` and a slice of input examples, and returns a `Vec<ExecutionTrace>` where each trace contains: the input, the generated output, an optional score, and optional actionable side information (ASI) as a free-form string.

- **GOAL-3.3** [P0]: The `reflect` method receives a `&Candidate` and a slice of `ExecutionTrace`s, and returns a `Reflection` containing: a natural-language diagnosis string and a list of suggested improvement directions.

- **GOAL-3.4** [P0]: The `mutate` method receives the parent `&Candidate`, the `&Reflection`, and a slice of ancestor lessons (accumulated from the lineage), and returns a new `Candidate` with potentially modified text parameters.

- **GOAL-3.5** [P0]: The `evaluate` method receives a `&Candidate` and a slice of input examples, and returns a `Vec<f64>` of per-example scores. This is used for acceptance testing and Pareto front updates.

- **GOAL-3.6** [P1]: `GEPAAdapter` has an optional method `merge` (default: returns `Err`) that receives two parent candidates and returns a new candidate combining both. This enables the merge proposer (GOAL-1.10).

- **GOAL-3.7** [P1]: All adapter methods return `Result<T, GEPAError>` so that LLM failures, timeouts, and rate limits can be propagated cleanly. The engine handles adapter errors according to a configurable retry policy (GOAL-7.5). `GEPAError` is an enum with the following variants:
  - `AdapterError { source: String, retryable: bool }` — LLM call failed (timeout, rate limit, invalid response). `retryable` indicates whether the engine should attempt retry.
  - `ConfigError(String)` — Invalid configuration detected at construction time (GOAL-7.3).
  - `EmptyDataError(String)` — DataLoader returned no training or validation examples (GOAL-8.7).
  - `CheckpointError { source: String }` — Failed to write or read checkpoint file.
  - `SerializationError { source: String }` — Failed to serialize/deserialize state or candidates.
  - `Cancelled` — Optimization cancelled via the cancellation callback. The engine accepts an optional `cancel_fn: Option<Box<dyn Fn() -> bool + Send + Sync>>` in `GEPAEngine::new()`. At the start of each iteration, if `cancel_fn` returns `true`, the engine stops with termination reason `Cancelled`. If no `cancel_fn` is provided, cancellation is not checked (zero overhead). This enables external integration (e.g., Ctrl-C handler, UI cancel button) without requiring async channels.

  `GEPAError` implements `std::error::Error`, `Display`, and `Debug` (per GUARD-8). `AdapterError` is the only variant the adapter should return; all other variants are engine-internal.

### 4. Proposers

Proposers are the "how to generate new candidates" strategies. The reflective mutation proposer is the primary strategy (single-parent + reflection). The merge proposer combines two Pareto-optimal candidates.

- **GOAL-4.1** [P0]: The reflective mutation proposer constructs a prompt context for the adapter's `mutate` method containing: the parent candidate's text parameters, the reflection diagnosis, improvement directions, and accumulated lessons from all ancestors in the lineage (not just the immediate parent).

- **GOAL-4.2** [P0]: Ancestor lessons accumulate along the lineage chain. When candidate C is mutated from parent B which was mutated from grandparent A, the mutation context for C includes lessons from both A→B and B→C reflections. Lessons are ordered chronologically.

- **GOAL-4.3** [P1]: The reflective mutation proposer includes a configurable maximum number of ancestor lessons to include (window size), preventing unbounded growth of mutation context. Default: 10 most recent lessons.

- **GOAL-4.4** [P1]: The merge proposer selects two candidates from the Pareto front that have maximally complementary strengths — high scores on different, non-overlapping subsets of examples. Selection is based on score vectors, not random pairing.

- **GOAL-4.5** [P2]: The merge proposer provides the adapter's `merge` method with sufficient context: both parent candidates' text parameters, their respective per-example scores, and identification of which task subsets each parent excels on. The adapter uses this context to produce a merged candidate. (This goal specifies the WHAT of merge context; GOAL-1.10 specifies WHEN merging occurs in the engine loop.)

### 5. Candidate Management

Candidates are the evolving text artifacts. Each candidate is a dictionary of named text parameters with metadata tracking its lineage, scores, and history.

- **GOAL-5.1** [P0]: The engine requires at least one seed candidate to begin optimization. The user provides one or more seed candidates via `GEPAEngine::new()` or `GEPAConfig`. Seed candidates form the initial Pareto front at iteration 0. If no seed candidate is provided, engine construction fails with a descriptive error.

- **GOAL-5.2** [P0]: A `Candidate` contains: a unique ID, a `HashMap<String, String>` of named text parameters (e.g., "system_prompt", "tool_description"), and metadata including: parent ID (None for seed), generation number, creation timestamp, and the reflection that produced it. Per-example scores are NOT stored on the Candidate struct — they are maintained in the evaluation cache (GOAL-6.3) keyed by `(candidate_id, example_id)`. This separation ensures candidates remain lightweight and immutable (GOAL-5.3) while scores accumulate incrementally via evaluation and re-evaluation backfill (GOAL-8.5).

- **GOAL-5.3** [P0]: Candidates are immutable after creation. Mutation produces a new candidate; it never modifies the parent in-place. This guarantees lineage integrity and safe concurrent reads.

- **GOAL-5.4** [P0]: Candidate IDs are unique within a run. Two candidates produced by different mutations always have different IDs, even if their text parameters happen to be identical.

- **GOAL-5.5** [P1]: The full lineage of any candidate is reconstructable from the candidate history: given a candidate, traverse parent IDs to recover the complete chain of mutations and reflections back to the seed candidate.

- **GOAL-5.6** [P1]: Candidates are serializable (serde Serialize + Deserialize). Serialized format is JSON. A round-trip serialize→deserialize produces an identical candidate (all fields preserved, including metadata).

### 6. State Management

`GEPAState` holds everything needed to checkpoint and resume an optimization run: the Pareto front, all candidate history, evaluation cache, iteration count, and accumulated statistics.

- **GOAL-6.1** [P0]: `GEPAState` can be serialized to JSON and deserialized back. A round-trip produces functionally identical state: same Pareto front, same candidate history, same evaluation cache, same iteration counter.

- **GOAL-6.2** [P0]: The engine writes a checkpoint after every N iterations (configurable, default: every iteration). Checkpoint is a single JSON file written atomically (write to temp file, then rename) so a crash mid-write never corrupts the checkpoint.

- **GOAL-6.3** [P0]: `GEPAState` contains an evaluation cache mapping (candidate_id, example_id) → score. When a candidate is evaluated on an example that was previously scored, the cached score is returned without calling the adapter.

- **GOAL-6.4** [P1]: The evaluation cache has a configurable maximum size. When the cache exceeds the limit, least-recently-used entries are evicted, **with the constraint that entries for candidates currently on the Pareto front are never evicted** (pinned). Only entries for candidates that have left the front (pruned or historically rejected) are eligible for LRU eviction. This ensures that dominance relationships among active front members are never invalidated by cache eviction — the score matrix for front members remains complete. **Boundary condition:** if all cached entries belong to front candidates and the cache is at capacity, the cache size limit is temporarily exceeded (soft limit) rather than blocking new evaluations. A warning event is emitted. The next front pruning (GOAL-2.2) will free entries for eviction. Cache hit rate is tracked and reported in statistics.

- **GOAL-6.5** [P1]: `GEPAState` tracks run statistics: total iterations, skipped iterations (adapter failures), total adapter calls (execute, reflect, mutate, evaluate), total candidates generated, total candidates accepted, acceptance rate, best score over time, and Pareto front size over time.

- **GOAL-6.6** [P2]: Support incremental checkpoint: instead of writing full state every time, write only the delta (new candidates, updated scores, updated front) since the last checkpoint. Full state is reconstructable from the initial checkpoint plus all deltas.

### 7. Configuration

`GEPAConfig` controls all tunable parameters of the engine, proposers, and evaluation. Sensible defaults for all parameters. Invalid configurations are rejected at construction time with descriptive errors.

- **GOAL-7.1** [P0]: `GEPAConfig` includes at minimum: maximum iterations, minibatch size (number of examples per evaluation), stagnation limit (iterations without improvement before termination), checkpoint interval, Pareto front maximum size, optional RNG seed (`Option<u64>`, default: random — if provided, enables deterministic runs per GUARD-9), max consecutive skips (`max_consecutive_skips`, default: 5, see GOAL-7.5), error policy (skip vs halt, default: skip, see GOAL-7.5), retry max (`retry_max`, default: 3), backoff strategy (fixed/exponential, default: exponential), base retry delay (default: 1s), re-evaluation interval (`re_eval_interval`, default: 5, in iterations — see GOAL-8.5), re-evaluation sample size (`re_eval_sample_size`, default: equal to `minibatch_size` — see GOAL-8.5), and minimum shared examples for dominance (`min_shared_examples`, default: equal to `minibatch_size` — see GOAL-2.1). The full parameter list is specified across GOAL-7.1 through GOAL-7.7; this goal defines the core set that every GEPAConfig must include.

- **GOAL-7.2** [P0]: All config parameters have sensible defaults. A user can construct `GEPAConfig::default()` and run the engine without setting any parameter. Defaults: max_iterations=100, minibatch_size=16, stagnation_limit=20, checkpoint_interval=1, pareto_max_size=50.

- **GOAL-7.3** [P0]: Invalid config is rejected at construction time with a descriptive error message. Invalid conditions include: minibatch_size=0, max_iterations=0, stagnation_limit > max_iterations, Pareto front max_size < 1, min_shared_examples=0, min_shared_examples > total training examples (checked at run start, not construction).

- **GOAL-7.4** [P1]: `GEPAConfig` is serializable (serde) so it can be saved alongside checkpoints for full reproducibility of a run.

- **GOAL-7.5** [P1]: Config includes retry policy for adapter errors: max retries per call (default: 3), backoff strategy (fixed or exponential, default: exponential), and base delay (default: 1 second). After exhausting retries, the engine either skips the iteration or halts, based on a configurable error policy (skip vs halt, default: skip). **Interaction with stagnation (GOAL-1.2):** a skipped iteration does NOT count toward the stagnation counter — stagnation only increments when a mutation was attempted and the resulting candidate was rejected as dominated. Skipped iterations are tracked separately in run statistics (GOAL-6.5) as `skipped_iterations`. If consecutive skipped iterations exceed `max_consecutive_skips` (configurable, default: 5), the engine halts with termination reason `TooManySkips` regardless of the error policy, to prevent infinite loops on persistently failing adapters.

- **GOAL-7.6** [P2]: Config includes optional wall-clock time budget (Duration). The engine checks elapsed time at the start of each iteration and terminates gracefully if the budget is exceeded. This is the configuration surface for the stopping criterion described in GOAL-1.2.

- **GOAL-7.7** [P2]: Config includes merge proposer settings: enabled (bool, default: false), merge interval (every N iterations, default: 10), and merge selection strategy (complementary vs random).

### 8. Data Loading

The `DataLoader` trait provides training and validation examples to the engine. Abstracts over data sources so consumers can load from files, databases, or generate dynamically.

- **GOAL-8.1** [P0]: `DataLoader` is a trait with methods: `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>`. The engine uses training examples for the optimization loop and validation examples for final evaluation of the result.

- **GOAL-8.2** [P0]: `Example` contains at minimum: a unique ID (string) and an input payload (string or structured data via `serde_json::Value`). Optional fields: expected output (for reference), metadata (key-value pairs), and difficulty tag.

- **GOAL-8.3** [P0]: The engine samples minibatches from training examples each iteration. Each training example is used at least once before any example is reused (epoch-based coverage), provided the total number of evaluations (iterations × minibatch_size) is ≥ the training set size. When total evaluations < training set size (e.g., 10 iterations × 16 batch = 160 evaluations but 1000 examples), the engine samples uniformly without replacement within each epoch-segment. Minibatch composition varies across iterations to prevent overfitting to a fixed subset. **Epoch boundary behavior:** when fewer than `minibatch_size` examples remain in the current epoch, the engine fills the minibatch by concatenating the remaining examples with the beginning of the next epoch (shuffled with the seeded RNG). This ensures every example is used exactly once per epoch and no examples are wasted. Example: 100 training examples, minibatch_size=16 → batch 7 gets the last 4 from epoch 1 + the first 12 from epoch 2.

- **GOAL-8.4** [P1]: The `DataLoader` trait supports async loading (`async fn`) for consumers that need to fetch examples from network sources or databases. Async DataLoader calls have a configurable timeout (default: 30s). If loading fails or times out, the engine retries up to 3 times, then halts with `GEPAError::AdapterError { source: 'DataLoader timeout', retryable: false }`.

- **GOAL-8.5** [P1]: The engine tracks which examples each candidate has been evaluated on via the evaluation cache (GOAL-6.3). **Score matrix backfill:** every `re_eval_interval` iterations (configurable, default: 5 — see GOAL-7.1), the engine selects front candidates with the sparsest score coverage (fewest evaluated examples; ties broken by candidate age — newest first — for GUARD-9 determinism) and evaluates them on examples they haven't seen (sample size = `re_eval_sample_size`, configurable, default: `minibatch_size` — see GOAL-7.1). New scores are written to the evaluation cache, progressively filling the score matrix. **Front recomputation:** after each backfill round, the engine recomputes dominance relationships across the front using the updated score coverage (see GOAL-2.2). Candidates that are now dominated (because sufficient shared examples reveal dominance) are removed from the front. This is the mechanism by which the front converges: early iterations grow the front (few shared examples → most candidates are non-dominating), later iterations shrink it (dense score matrix → dominance becomes detectable). **Overfitting detection:** the engine computes per-candidate overfitting delta (difference between average training score and average re-evaluation score) and reports it in events (`ReEvaluationCompleted`) and statistics. High overfitting delta influences selection (GOAL-2.3) but does not directly remove candidates — only dominance does.

- **GOAL-8.6** [P0]: After the optimization loop terminates, the engine evaluates all Pareto front candidates on the full validation set (from `DataLoader::validation_examples()`). The `GEPAResult` includes validation scores for each front candidate, enabling the consumer to select the best candidate based on held-out data rather than training performance. If `validation_examples()` returns empty, the engine skips final validation and reports training-only scores in `GEPAResult` (with a `validation_skipped: true` flag).

- **GOAL-8.7** [P0]: The engine validates DataLoader output at startup before entering the optimization loop. If `training_examples()` returns empty, the engine returns `Err(GEPAError::EmptyDataError)` immediately — optimization cannot proceed without training data. If `validation_examples()` returns empty, the engine proceeds but emits a warning event (`DataLoaderWarning { message }`) via the callback system and sets `validation_skipped: true` in the result.

### 9. Callback / Events

Observable event system for monitoring, logging, visualization, and integration with external tools. Consumers register callbacks that are invoked at specific points in the optimization loop.

- **GOAL-9.1** [P0]: The engine emits typed events at key points: `IterationStarted`, `CandidateSelected`, `ExecutionCompleted`, `ReflectionCompleted`, `MutationCompleted`, `CandidateAccepted`, `CandidateRejected`, `IterationSkipped { reason, retry_count }` (adapter error after retry exhaustion), `ReEvaluationCompleted { candidate_id, new_scores, overfitting_delta, candidates_pruned }` (after periodic re-evaluation backfill per GOAL-8.5; `candidates_pruned` lists front members removed due to newly established dominance), `StagnationWarning` (stagnation counter > 50% of limit), `DataLoaderWarning { message }` (empty validation set per GOAL-8.7), `CheckpointSaved`, `IterationCompleted`, `RunCompleted`.

- **GOAL-9.2** [P0]: Events carry relevant data: `CandidateAccepted` includes the candidate, its scores, and the updated Pareto front size. `IterationCompleted` includes iteration number, elapsed time, current best score, and front size.

- **GOAL-9.3** [P1]: Consumers register callbacks via `GEPAEngine::on_event(EventType, callback)` before calling `run()`. Multiple callbacks can be registered for the same event type; they are invoked in registration order.

- **GOAL-9.4** [P1]: Callbacks receive an immutable reference to the event data. Callbacks must not block the optimization loop — they execute synchronously but are expected to be fast (logging, metric recording). Long-running callbacks should spawn their own tasks.

- **GOAL-9.5** [P2]: Built-in `TracingCallback` that logs all events via the `tracing` crate at appropriate log levels: `info` for iteration summaries, `debug` for individual step completions, `trace` for full event payloads.

## Guards

- **GUARD-1** [hard]: The Pareto front must never contain a candidate that is dominated by another front member on their shared evaluated examples (with intersection size ≥ `min_shared_examples`). After every update operation (Accept step or re-evaluation backfill recomputation), all candidates in the front are mutually non-dominating on their shared examples. Two candidates with insufficient shared examples (intersection < `min_shared_examples`) are always treated as non-dominating. Violation means the optimization is fundamentally broken — wrong candidates would be selected as parents.

- **GUARD-2** [hard]: Candidate immutability must be preserved. Once a candidate is created, its text parameters and metadata never change. Any code path that appears to modify a candidate must create a new one instead. Violation would corrupt lineage history and make results non-reproducible.

- **GUARD-3** [hard]: The evaluation cache must never return a stale or incorrect score. A cache entry for (candidate_id, example_id) → score must be consistent with what the adapter would return for the same inputs. Since candidates are immutable (GUARD-2), this is guaranteed as long as cache keys are correct.

- **GUARD-4** [hard]: Checkpoint atomicity must be maintained. A crash at any point during checkpoint writing must not corrupt the previous valid checkpoint. The engine must use atomic write (temp file + rename) or equivalent strategy.

- **GUARD-5** [hard]: The crate must never make any LLM API call or network request directly. All LLM interaction goes through the `GEPAAdapter` trait. The crate's dependency list must not include any LLM SDK, HTTP client, or network library.

- **GUARD-6** [soft]: Single-iteration wall-clock time should be dominated by adapter calls (LLM), not by engine overhead. Engine-internal computation (Pareto updates, candidate management, serialization) should add < 5% overhead relative to adapter call time for typical workloads.

- **GUARD-7** [soft]: Memory usage grows linearly with the number of candidates in history, not quadratically. Storing 1,000 candidates with 10 text parameters of ~1KB each should use < 50MB of heap memory (excluding adapter-side allocations).

- **GUARD-8** [soft]: All public types in the crate implement `Debug`. All error types implement `std::error::Error` and `Display` with descriptive messages. No `.unwrap()` or `.expect()` on fallible operations in library code (only in tests).

- **GUARD-9** [hard]: The engine must be deterministic given the same RNG seed, config, adapter responses, and data ordering. Two runs with identical inputs (including a user-provided RNG seed) produce identical candidate histories, Pareto fronts, and final results. All engine-internal randomness (minibatch sampling, Pareto front selection, tie-breaking, etc.) draws from a single seeded RNG instance in a deterministic call order — the RNG is never accessed from multiple threads concurrently, and the sequence of draw operations is fixed by the algorithm structure. Non-determinism may only come from the adapter (LLM responses).

## Out of Scope

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

---

**Summary: 59 GOALs** (35 P0 / 18 P1 / 6 P2) **+ 9 GUARDs** (6 hard / 3 soft) **across 9 modules**
