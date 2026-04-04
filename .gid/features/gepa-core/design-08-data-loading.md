# Design: GEPA Data Loading

## 1. Overview

The data loading subsystem provides training and validation examples to the GEPA engine through the `DataLoader` trait, manages epoch-based minibatch cycling, drives periodic score matrix backfill to sharpen Pareto dominance, detects overfitting via training/re-evaluation score deltas, and runs final validation on the held-out set. The design prioritizes deterministic sampling (GUARD-9) and memory-bounded caching (GUARD-7).

Key trade-off: eager loading (all examples in memory) over streaming. This simplifies epoch tracking and random access for backfill at the cost of memory for very large datasets. The target workload (prompt optimization) typically has hundreds to low thousands of examples, making eager loading practical.

**Satisfies:** GOAL-8.1 through GOAL-8.7, GUARD-2 (deterministic sampling), GUARD-8 (Debug impls), GUARD-11 (Send + Sync).

## 2. Components

### 2.1 DataLoader Trait

**Responsibility:** Abstract over data sources so consumers provide examples from files, databases, or generators.

**Interface:**
```rust
#[async_trait]
pub trait DataLoader: Send + Sync + 'static + std::fmt::Debug {
    async fn training_examples(&self) -> Result<Vec<Example>, GEPAError>;
    async fn validation_examples(&self) -> Result<Vec<Example>, GEPAError>;
}
```

**Key Details:**
- Both methods return owned `Vec<Example>`. The engine calls each once at startup and caches the results in memory.
- **Deviation from GOAL-8.1:** The requirements specify sync signatures returning `Vec<Example>`. This design adds `async` (driven by GOAL-8.4 for network/database support) and `Result<_, GEPAError>` (to surface load errors). The `Result` wrapper is a necessary addition — the requirements underspecified the error case. Both changes are compatible with all other GOALs.
- Async to support network/database data sources (GOAL-8.4). The engine wraps each call with `tokio::time::timeout(Duration::from_secs(config.data_loader_timeout_secs))`.
- On timeout or retryable error (`GEPAError::AdapterError { retryable: true, .. }`): retry up to 3 times using the config's backoff strategy (§2.1 of design-07-config). Non-retryable errors cause immediate halt.
- After exhausting retries, return `Err(GEPAError::AdapterError { retryable: false, .. })`.
- Requires `Send + Sync + 'static` per GUARD-11 for multi-threaded tokio runtimes.
- Requires `Debug` per GUARD-8.

**Satisfies:** GOAL-8.1, GOAL-8.4, GUARD-8, GUARD-11

### 2.2 Example Struct

**Responsibility:** Represent a single training or validation example with an ID, input payload, and optional metadata.

**Interface:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Example {
    pub id: ExampleId,
    pub input: serde_json::Value,
    pub expected_output: Option<serde_json::Value>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub difficulty_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ExampleId(pub u64);
```

**Key Details:**
- `id` is a newtype wrapper around `u64` for type safety and fast hashing. Used as keys in the evaluation cache (`HashMap<(CandidateId, ExampleId), f64>` from feature 06). The `DataLoader` assigns numeric IDs; if original data uses string identifiers, the loader maintains a separate `HashMap<String, ExampleId>` lookup table.
- `input` is `serde_json::Value` — opaque structured data. The engine passes it through to the adapter; it never inspects contents.
- `expected_output` is optional reference data for consumer-side evaluation logic. The engine does not use it directly.
- `metadata` is arbitrary key-value pairs for consumer extensions (e.g., source file, creation date). The engine ignores it.
- `difficulty_tag` is `Option<String>`, opaque to the engine. Available for consumer-side curriculum learning or stratified sampling.
- `ExampleId` implements `Hash + Eq + Copy` for use as HashMap keys. Implements `Display` (delegates to inner u64). The `Copy` trait is derivable since `ExampleId` wraps a `u64`.

**Satisfies:** GOAL-8.2, GUARD-8

### 2.3 Minibatch Iterator

**Responsibility:** Cycle through training examples in epoch-based order, producing minibatches of configurable size with seeded shuffling.

**Interface:**
```rust
#[derive(Debug)]
pub struct MinibatchIterator {
    example_ids: Vec<ExampleId>,
    shuffled_order: Vec<usize>,
    cursor: usize,
    epoch: u64,
    rng: ChaCha8Rng,
}

impl MinibatchIterator {
    pub fn new(example_ids: Vec<ExampleId>, rng: &mut ChaCha8Rng) -> Self {
        /* shuffles example_ids indices using rng, sets cursor=0, epoch=0 */
    }

    pub fn next_batch(&mut self, batch_size: usize) -> MinibatchResult {
        /* returns batch_size example IDs, crossing epoch boundary if needed */
    }

    pub fn current_epoch(&self) -> u64 { self.epoch }
}

#[derive(Debug)]
pub struct MinibatchResult {
    pub example_ids: Vec<ExampleId>,
    pub epoch_boundary_crossed: bool,
    pub new_epoch: u64,
}
```

**Key Details:**
- Algorithm: maintain a shuffled index array over training example indices. A cursor advances by `batch_size` each call. When fewer than `batch_size` remain, take the remainder, increment epoch, re-shuffle with the seeded RNG, then take `batch_size - remainder` from the new epoch's start. This guarantees every example is used exactly once per epoch (GOAL-8.3).
- The RNG is `ChaCha8Rng` forked from the engine's master RNG at construction, ensuring deterministic shuffling per GUARD-9.
- Edge case: `batch_size >= len(examples)` → each batch contains all examples (full-batch mode). No duplication or padding.
- Edge case: single-example training set → every batch is that one example. Functional but degenerate.
- `epoch_boundary_crossed` flag lets the engine log epoch transitions.

**Satisfies:** GOAL-8.3, GUARD-9

### 2.4 Score Matrix Backfill

**Responsibility:** Select Pareto front candidates with sparse score coverage and schedule re-evaluations to fill the score matrix.

**Interface:**
```rust
pub struct BackfillScheduler;

impl BackfillScheduler {
    pub fn select_candidates_for_backfill(
        front: &ParetoFront,
        eval_cache: &EvaluationCache,
        sample_size: usize,
        max_evals: usize,
        rng: &mut ChaCha8Rng,
    ) -> Vec<BackfillTask> {
        /* returns tasks: (candidate_id, Vec<ExampleId to evaluate>) */
    }
}

#[derive(Debug)]
pub struct BackfillTask {
    pub candidate_id: CandidateId,
    pub example_ids: Vec<ExampleId>,
}
```

**Key Details:**
- **Candidate selection:** For each front candidate, count evaluated examples in the cache. Sort ascending by coverage count; ties broken by candidate age (newest first — most recent `CandidateId` value, which is monotonically increasing) for GUARD-9 determinism.
- **Example selection:** For each selected candidate, pick up to `sample_size` examples they haven't been evaluated on. Examples are chosen uniformly at random from the unevaluated set using the seeded RNG.
- **Budget cap:** Total evaluations across all tasks ≤ `max_re_eval_per_iteration` from config.
- **Candidate iteration and stopping:** Iterate candidates in sparsest-first order (ascending coverage count, ties broken by age). For each candidate, assign up to `sample_size` unevaluated examples. Accumulate the total number of assigned evaluations. Stop assigning candidates when adding the next candidate’s batch would exceed `max_re_eval_per_iteration`. If a candidate’s full batch would exceed the remaining budget, truncate that candidate’s batch to fit. This means not all front candidates may receive backfill in a given round — the sparsest are prioritized.
- Runs every `re_eval_interval` iterations (checked by the engine loop, not by this module).
- After the engine executes all backfill tasks (calling `adapter.evaluate()` per GOAL-3.5), it writes new scores to the evaluation cache and triggers front recomputation (§2.5 of design-02-pareto via GOAL-8.5b).

**Satisfies:** GOAL-8.5a, GOAL-8.5b, GUARD-9

### 2.5 Overfitting Detection

**Responsibility:** Compute per-candidate overfitting delta after backfill rounds for reporting and selection influence.

**Interface:**
```rust
pub fn compute_overfitting_delta(
    candidate_id: &CandidateId,
    training_scores: &[(ExampleId, f64)],
    reeval_scores: &[(ExampleId, f64)],
) -> f64 {
    /* returns avg(training_scores) - avg(reeval_scores) */
}
```

**Key Details:**
- `training_scores` are scores from the iteration where the candidate was first evaluated (the minibatch it was tested on).
- `reeval_scores` are scores from backfill evaluations on previously-unseen examples.
- Delta = `mean(training) - mean(reeval)`. Positive delta suggests overfitting (candidate performs worse on new examples).
- The delta is reported in the `ReEvaluationCompleted` event (feature 09) and stored in run statistics (feature 06).
- High delta influences Pareto front selection (GOAL-2.3) — the selection logic in feature 02 deprioritizes candidates with high overfitting delta.
- Delta does NOT directly remove candidates — only dominance does. This is purely a signal.

**Satisfies:** GOAL-8.5c

### 2.6 Final Validation

**Responsibility:** After the optimization loop, evaluate all Pareto front candidates on the full validation set.

**Interface:**
```rust
pub struct ValidationRunner;

impl ValidationRunner {
    pub async fn run_validation(
        front: &ParetoFront,
        validation_examples: &[Example],
        adapter: &dyn GEPAAdapter,
        config: &GEPAConfig,
        callbacks: &CallbackRegistry,
    ) -> Result<ValidationResult, GEPAError> {
        /* evaluates each front candidate on all validation examples */
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub scores: HashMap<CandidateId, Vec<(ExampleId, f64)>>,
    pub validation_skipped: bool,
}
```

**Key Details:**
- Called after the engine loop exits (any termination reason).
- If `validation_examples` is empty: returns `ValidationResult { scores: HashMap::new(), validation_skipped: true }`. The engine emits a `DataLoaderWarning` at startup (§2.7), not here.
- Iterates over front candidates in deterministic order (sorted by `CandidateId`). For each candidate, calls `adapter.evaluate(candidate, examples)` with the full validation set.
- Emits `ValidationProgress { candidate_index, total_candidates }` after each candidate completes (feature 09).
- Adapter errors during validation use the same retry/backoff policy from config (GOAL-7.5). The error policy (Skip vs Halt) also applies with validation-specific semantics:
  - **`ErrorPolicy::Skip`:** Skip the failing candidate after retry exhaustion — record no validation scores for it (`validation_scores: None` in the per-candidate result) and continue to the next candidate.
  - **`ErrorPolicy::Halt`:** Abort validation entirely after retry exhaustion and propagate the error as `GEPAError::ValidationError`.
- Validation scores are included in `GEPAResult` for the consumer to pick the best candidate.

**Satisfies:** GOAL-8.6

### 2.7 Startup Validation

**Responsibility:** Validate DataLoader output before entering the optimization loop.

**Interface:**
```rust
pub fn validate_data_loader_output(
    training: &[Example],
    validation: &[Example],
    callbacks: &CallbackRegistry,
) -> Result<DataLoadDiagnostics, GEPAError> {
    /* checks for empty sets, duplicate IDs, returns diagnostics */
}

#[derive(Debug)]
pub struct DataLoadDiagnostics {
    pub training_count: usize,
    pub validation_count: usize,
    pub validation_skipped: bool,
    pub duplicate_ids: Vec<ExampleId>,
}
```

**Key Details:**
- If `training` is empty: return `Err(GEPAError::EmptyDataError)` immediately. The engine cannot proceed.
- If `validation` is empty: emit `DataLoaderWarning { message: "No validation examples provided; final validation will be skipped" }` via the callback system, set `validation_skipped = true`, and continue.
- Check for duplicate `ExampleId` values within training and validation sets. Duplicates are logged as warnings but not rejected (consumers may intentionally duplicate for weighting).
- `DataLoadDiagnostics` is returned to the engine for inclusion in run statistics.

**Satisfies:** GOAL-8.7

### 2.8 Example Lookup

**Responsibility:** Map `ExampleId` values back to full `Example` objects for adapter calls.

**Key Details:**
- The engine maintains an `examples: HashMap<ExampleId, Example>` (populated once at startup from `DataLoader::training_examples()` results). This store lives alongside the `MinibatchIterator` in the engine.
- When the `MinibatchIterator` returns a batch of `ExampleId`s, the engine resolves each to a full `Example` via this map before passing to `adapter.evaluate()` or `adapter.execute()`.
- Validation examples are stored similarly in a separate `validation_examples: Vec<Example>` (or `HashMap`) for use by `ValidationRunner`.
- This lookup is O(1) per example and adds negligible memory overhead (the `Example` data is already in memory from the eager load).

## 3. Memory Analysis

Evaluation cache growth model for score matrix storage:

- Each cache entry: `(CandidateId, ExampleId) -> f64` ≈ 40 bytes (two u64 keys + f64 + HashMap overhead).
- Dense matrix: `C candidates × E examples` entries.
- With `C=50` (pareto_max_size), `E=1000` examples: 50,000 entries ≈ 4 MB.
- With `C=50`, `E=10,000`: 500,000 entries ≈ 40 MB.
- `eval_cache_max_size` (GOAL-7.1) caps entries via LRU eviction when set, preventing unbounded growth.
- The matrix is intentionally sparse early (only minibatch examples evaluated per candidate) and fills via backfill (§2.4). Worst case density depends on run length.
- GUARD-7 compliance: 1,000 candidates × 1,000 examples = 1M entries ≈ 80 MB. Exceeds the 50 MB guideline for candidate storage alone, but the cache is separate from candidate text storage. With `eval_cache_max_size` set, the consumer controls this bound.

## 4. Integration Points

- **Config (feature 07):** Reads `minibatch_size`, `re_eval_interval`, `re_eval_sample_size`, `max_re_eval_per_iteration`, `data_loader_timeout_secs` from `GEPAConfig`.
- **Pareto front (feature 02):** Backfill triggers `ParetoFront::recompute_dominance()` after new scores are written. Selection uses overfitting delta from §2.5.
- **Adapter (feature 03):** `adapter.evaluate()` is called for backfill (§2.4) and final validation (§2.6). The adapter trait's `evaluate` method returns `Vec<f64>` (one score per example, positional correspondence — see design-03 §2.2). The engine correlates scores to example IDs by position.
- **State/checkpoint (feature 06):** Evaluation cache stores per-example scores. `MinibatchIterator` state (cursor, epoch) is checkpointed for deterministic resume.
- **Events (feature 09):** Emits `DataLoaderWarning`, `ReEvaluationCompleted`, `ValidationProgress` events.
- **Engine loop (feature 01):** The engine calls `MinibatchIterator::next_batch()` each iteration, runs backfill every `re_eval_interval` iterations, and calls `ValidationRunner::run_validation()` after loop exit.
