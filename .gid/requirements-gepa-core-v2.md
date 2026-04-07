# Requirements: gepa-core (Simplified)

## Overview

`gepa-core` is a Rust crate implementing the GEPA (Genetic-Pareto) prompt evolution algorithm from the ICLR 2026 paper. It provides the core optimization loop (Select → Execute → Reflect → Mutate → Accept), Pareto front management, and an adapter-based interface for plugging in any LLM provider and evaluation logic. The crate never calls any LLM API directly.

**Core Algorithm (5-step loop):**
1. **Select** — Pick a parent candidate from the Pareto front
2. **Execute** — Run candidate on a minibatch, capture execution traces
3. **Reflect** — Teacher LLM reads traces, diagnoses failures, proposes improvements
4. **Mutate** — Generate improved candidate from parent + reflection + ancestor lessons
5. **Accept** — If new candidate is non-dominated, accept into Pareto front

**First consumer:** RustClaw Self-Improvement System — SkillAdapter, SystemPromptAdapter, RitualAdapter.

## Priority Levels

- **P0**: Core — the crate cannot function without this
- **P1**: Production-quality — needed for reliable real-world usage

## Guard Severity

- **hard**: Violation = system is broken, must halt
- **soft**: Violation = degraded quality, warn but continue

## Goals

### 1. Core Engine

- **GOAL-1.1** [P0]: `GEPAEngine::run()` executes the full optimization loop: select → execute → reflect → mutate → accept/reject. Each iteration performs all 5 steps in order.

- **GOAL-1.2** [P0]: The engine terminates when any of: (a) maximum iterations reached, (b) wall-clock time budget exhausted, (c) stagnation — no new candidate accepted for N consecutive iterations where a mutation was attempted and rejected, (d) too many consecutive adapter errors (default: 5), (e) external cancellation. Termination reason is reported in the result as an enum.

- **GOAL-1.3** [P0]: Parent selection from the Pareto front varies across front members (not always the same candidate). Selection is round-robin with random ordering — simple, deterministic, guarantees every front member gets selected.

- **GOAL-1.4** [P0]: The engine passes the selected candidate + minibatch to the adapter's `execute`, which returns execution traces (input, output, score, optional diagnostic info per example).

- **GOAL-1.5** [P0]: After execution, traces go to the adapter's `reflect`, which returns a natural-language diagnosis + improvement directions. The reflection is stored in the candidate's lineage.

- **GOAL-1.6** [P0]: After reflection, the parent candidate + reflection + ancestor lessons go to the adapter's `mutate`, which returns a new candidate.

- **GOAL-1.7** [P0]: After mutation, the new candidate is scored via the adapter's `evaluate` method (not `execute` — no traces needed, just scores) on the same minibatch. The engine also evaluates all current front members on this minibatch (reusing any cached scores from this iteration's `execute` step for the parent). The new candidate is accepted if it is non-dominated by every front member on the current minibatch — i.e., no front member scores ≥ on every example and strictly > on at least one. After acceptance, any front members now dominated by the new candidate are removed (GOAL-2.2).

  > **Design note (tech debt consideration):** Dominance is checked against all front members, but only on the current minibatch (not historical scores). This keeps GUARD-1 consistent (no dominated candidates on shared examples) while avoiding the sparse score matrix complexity of v1 GOAL-1.7a-d. Front size is ≤20, so full pairwise check is negligible overhead. The full cross-minibatch comparison with evaluation cache can be added later (Tier 1) without changing Candidate or ParetoFront interfaces.

- **GOAL-1.8** [P0]: `GEPAEngine::run()` returns a `GEPAResult` containing: the final Pareto front, the single best candidate by average score, total iterations, wall-clock time, and termination reason.

- **GOAL-1.9** [P1]: The engine supports resumption from a checkpointed `GEPAState`. Calling `run()` with restored state continues from where it left off.

### 2. Pareto Front

- **GOAL-2.1** [P0]: Maintain a set of non-dominated candidates. Candidate A dominates B if A scores ≥ B on every shared example and strictly > on at least one. After every update, all front members are mutually non-dominating.

- **GOAL-2.2** [P0]: When a new candidate is accepted, add it to the front and remove any existing members now dominated by it.

- **GOAL-2.3** [P1]: The front has a configurable maximum size (default: 20). When exceeded, the candidate with the lowest average score is pruned. Ties broken by age (oldest removed first).

  > **Design note (tech debt consideration):** v1 used crowding distance (NSGA-II style) for pruning. The simple lowest-average-score approach works for MVP because front sizes are small (≤20). Crowding distance can be added later as an alternative `PruningStrategy` trait implementation without changing the front's public API — the pruning logic is already isolated behind the max-size check.

- **GOAL-2.4** [P1]: The Pareto front is serializable (serde) for checkpoint/resume.

### 3. Adapter Interface

- **GOAL-3.1** [P0]: `GEPAAdapter` is an async trait with 4 required methods:
  - `execute(&Candidate, &[Example]) -> Result<Vec<ExecutionTrace>>` — run candidate, return rich traces
  - `reflect(&Candidate, &[ExecutionTrace]) -> Result<Reflection>` — diagnose failures
  - `mutate(&Candidate, &Reflection, &[String]) -> Result<Candidate>` — produce improved candidate
  - `evaluate(&Candidate, &[Example]) -> Result<Vec<f64>>` — score-only evaluation

  All methods return `Result<T, GEPAError>`.

- **GOAL-3.2** [P0]: `ExecutionTrace` contains: input, generated output, score (f64), and optional diagnostic string (actionable side information).

- **GOAL-3.3** [P0]: `Reflection` contains: diagnosis string and list of improvement direction strings.

- **GOAL-3.4** [P0]: `GEPAError` is an enum with variants:
  - `AdapterError { source: String, retryable: bool }` — LLM call failed (timeout, rate limit, bad response). Only variant adapters should return.
  - `ConfigError(String)` — Invalid configuration at construction time.
  - `EmptyDataError(String)` — DataLoader returned no training examples.
  - `Cancelled` — External cancellation signal.

  Implements `std::error::Error`, `Display`, `Debug`.

- **GOAL-3.5** [P1]: The engine retries `AdapterError { retryable: true }` with configurable max retries and exponential backoff. After exhausting retries, the iteration is skipped.

  > **Design note (tech debt consideration):** The `merge` optional method (v1 GOAL-3.6) is omitted. When needed, add it as a default-implemented method on the trait returning `Err(Unimplemented)` — this is backwards-compatible and won't break existing adapters. Additional error variants (`CheckpointError`, `SerializationError`) can be added as needed.

### 4. Proposers

- **GOAL-4.1** [P0]: The reflective mutation proposer constructs context for `mutate` containing: parent's text parameters, reflection diagnosis, improvement directions, and accumulated ancestor lessons (chronologically ordered).

- **GOAL-4.2** [P0]: Ancestor lessons accumulate along the lineage. Candidate C mutated from B mutated from A includes lessons from A→B and B→C.

- **GOAL-4.3** [P1]: Configurable maximum ancestor lesson window (default: 10 most recent) to prevent unbounded context growth.

### 5. Candidate Management

- **GOAL-5.1** [P0]: At least one seed candidate required. Seeds form the initial Pareto front. No seeds = construction error.

- **GOAL-5.2** [P0]: `Candidate` contains: unique ID, `HashMap<String, String>` of named text parameters, parent ID (None for seeds), generation number, and the reflection that produced it.

- **GOAL-5.3** [P0]: Candidates are immutable after creation. Mutation always produces a new candidate.

- **GOAL-5.4** [P1]: Candidates and their lineage are serializable (serde JSON).

### 6. State & Checkpointing

- **GOAL-6.1** [P0]: `GEPAState` holds: Pareto front, candidate history, iteration count, and RNG state. Serializable to/from JSON.

- **GOAL-6.2** [P0]: Checkpoint written after every N iterations (configurable, default: every iteration). Written atomically (temp file + rename) — crash-safe.

- **GOAL-6.3** [P1]: `GEPAState` tracks run statistics: total iterations, skipped iterations, candidates generated/accepted, acceptance rate, best score over time.

### 7. Configuration

- **GOAL-7.1** [P0]: `GEPAConfig` includes: max_iterations (default: 100), minibatch_size (default: 16), stagnation_limit (default: 20), checkpoint_interval (default: 1), pareto_max_size (default: 20), max_consecutive_skips (default: 5), retry_max (default: 3), optional wall-clock time budget, optional RNG seed.

- **GOAL-7.2** [P0]: `GEPAConfig::default()` works out of the box. Invalid config rejected at construction time with descriptive errors. Invalid conditions include: minibatch_size=0, max_iterations=0, stagnation_limit > max_iterations, pareto_max_size < 1.

- **GOAL-7.3** [P1]: Config is serializable (serde) for reproducibility.

### 8. Data Loading

- **GOAL-8.1** [P0]: `DataLoader` trait with: `training_examples() -> Vec<Example>` and `validation_examples() -> Vec<Example>`.

- **GOAL-8.2** [P0]: `Example` contains: unique ID (String) and input payload (`serde_json::Value`). Optional: expected output, metadata.

- **GOAL-8.3** [P0]: Minibatch sampling varies each iteration. Within an epoch, each example is used at least once before reuse. Sampling uses a seeded RNG for determinism.

- **GOAL-8.4** [P0]: After the optimization loop, evaluate all front candidates on the full validation set. Results included in `GEPAResult`. Empty validation set = skip with warning.

### 9. Events

- **GOAL-9.1** [P0]: The engine emits events via a callback: `IterationStarted`, `CandidateAccepted`, `CandidateRejected`, `IterationSkipped`, `CheckpointSaved`, `RunCompleted`. Events carry relevant data (candidate, scores, iteration number, front size).

- **GOAL-9.2** [P1]: Consumers register callbacks via `GEPAEngine::on_event(callback)` before `run()`. A built-in `TracingCallback` logs all events via the `tracing` crate at appropriate levels.

## Guards

- **GUARD-1** [hard]: The Pareto front must never contain a dominated candidate. After every update, all members are mutually non-dominating.

- **GUARD-2** [hard]: Candidate immutability. Once created, text parameters and metadata never change.

- **GUARD-3** [hard]: Checkpoint atomicity. A crash during write must not corrupt the previous checkpoint.

- **GUARD-4** [hard]: No direct LLM calls. All LLM interaction goes through `GEPAAdapter`. No HTTP client or LLM SDK in dependencies.

- **GUARD-5** [hard]: Determinism given same RNG seed + same adapter responses + same data. All engine randomness from a single seeded PRNG (ChaCha8Rng).

- **GUARD-6** [soft]: No `.unwrap()` or `.expect()` in library code. All public types implement `Debug`. All errors implement `std::error::Error + Display`.

## Dependencies (Allowed)

- **serde + serde_json** — Serialization
- **tokio** — Async runtime
- **async-trait** — Async trait support
- **tracing** — Structured logging
- **rand + rand_chacha** — Deterministic RNG (ChaCha8Rng, not thread_rng/StdRng)
- **thiserror** — Error derivation

No other dependencies without justification. No HTTP clients, no LLM SDKs.

## Out of Scope

- No LLM integration (adapter-based only)
- No domain-specific logic (opaque string parameters)
- No UI/CLI (library crate only)
- No distributed execution (single-process)
- No gradient-based optimization (evolutionary only)

---

## Future Enhancements

Features cut from MVP, designed to be addable without major refactoring:

### Tier 1 — Add when front quality matters (after ~50 real optimization runs)

| Feature | v1 GOAL | How to add back | Tech debt notes |
|---------|---------|-----------------|-----------------|
| **Evaluation cache** | 6.3-6.4 | Add `HashMap<(CandidateId, ExampleId), f64>` to `GEPAState`. Accept step queries cache before calling adapter. | `Candidate` already separates scores from struct (GOAL-5.2), so no model changes needed. |
| **Cross-minibatch dominance** | 1.7a-d | Extend dominance check from current-minibatch-only to all historically cached scores. Add `min_shared_examples` config field for sparse matrix handling. | Accept step already checks all front members — just widen the score source from "current minibatch" to "evaluation cache". Config is serde so new fields with defaults are backwards-compatible. |
| **Re-evaluation backfill** | 8.5 | Periodic job evaluates front members on unseen examples to fill score matrix. Triggers front recomputation. | Front's `recompute()` method already exists (GOAL-2.2). Just need a scheduler (config: `re_eval_interval`). |
| **Crowding distance pruning** | 2.4 | Replace lowest-average-score pruning with crowding distance. | Pruning is a single function call — swap implementation. Consider a `PruningStrategy` trait if both strategies should coexist. |

### Tier 2 — Add when optimization runs are long/expensive

| Feature | v1 GOAL | How to add back |
|---------|---------|-----------------|
| **Merge proposer** | 1.10, 3.6, 4.4-4.5 | Add optional `merge()` to adapter trait (default: Err). Engine calls it every N iterations on complementary front members. |
| **Incremental checkpoints** | 6.6 | Write deltas instead of full state. Reconstruct from base + deltas. |
| **Overfitting detection** | 8.5 (partial) | Compare training vs re-evaluation scores. Flag candidates with high delta. Influence selection but don't remove. |

### Tier 3 — Nice to have

| Feature | v1 GOAL | How to add back |
|---------|---------|-----------------|
| **Rich typed events** | 9.1 (full set) | Expand event enum with `ReEvaluationCompleted`, `StagnationWarning`, etc. Callback interface is already generic. |
| **Cache eviction (LRU + pinning)** | 6.4 | Add LRU eviction with front-member pinning. Only matters when cache grows large. |
| **Performance SLAs** | 2.5, GUARD-6 (v1) | Benchmark and optimize. The algorithm is the same — this is implementation tuning. |
| **Async DataLoader** | 8.4 | Change trait method signatures to async. Existing sync loaders wrap trivially. |

### Extension Points Already Built In

These design choices in the simplified version specifically support future additions:

1. **`GEPAAdapter` trait** — New optional methods (merge, etc.) can be added with default implementations. No breaking changes to existing adapters.
2. **`GEPAConfig` with serde** — New fields with `#[serde(default)]` are backwards-compatible with old checkpoints.
3. **`GEPAState` with serde** — Same pattern. New fields (eval cache, statistics) added with defaults.
4. **`Candidate` stores no scores** — Scores live outside the candidate, so adding an evaluation cache doesn't touch the candidate model.
5. **Events via callback** — New event variants are additive. Existing callbacks ignore unknown events.
6. **Seeded PRNG** — ChaCha8Rng is version-stable. Determinism preserved across upgrades.

---

**Summary: 29 GOALs** (21 P0 / 8 P1) **+ 6 GUARDs** (5 hard / 1 soft) **across 9 modules**
