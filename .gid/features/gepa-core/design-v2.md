# Design: gepa-core (Simplified — based on requirements-gepa-core-v2)

> Single unified design document. v1 split across 9 files; v2 is compact enough for one.

## §1 Overview

`gepa-core` is a pure-algorithmic Rust library implementing the GEPA evolutionary prompt
optimization loop. Zero LLM dependencies (GUARD-4); all LLM interaction delegated through
the `GEPAAdapter` async trait.

**Key design decisions:**

| Decision | Chosen | Rationale |
|---|---|---|
| Pareto pruning | Lowest average score | Front ≤20; crowding distance unnecessary for MVP (Tier 1 upgrade) |
| Score storage | On current minibatch only | No eval cache needed for MVP; scores compared within same batch |
| Concurrency | Sequential single-threaded loop | Determinism (GUARD-5); adapter may use internal concurrency |
| RNG | ChaCha8Rng | Cross-version determinism; StdRng may change across versions |
| Extension points | Trait objects (`Box<dyn>`) | Runtime polymorphism; LLM latency dwarfs vtable cost |
| Builder | Simple builder (not typestate) | MVP; 3 required fields verified at `build()` runtime |

## §2 Architecture

```
┌─────────────────────────────────────────────────────┐
│                   GEPAEngine                        │
│  ┌──────────┐  ┌──────────┐  ┌──────┐  ┌────────┐  │
│  │  Config   │  │  State   │  │Events│  │  RNG   │  │
│  └──────────┘  └────┬─────┘  └──────┘  │ChaCha8 │  │
│                     │                   └────────┘  │
│  ┌──────────────────┼──────────────┐                │
│  │  ParetoFront     │  Candidate   │                │
│  │  Vec<CandId>     │  Registry    │                │
│  └──────────────────┴──────────────┘                │
│         ▼ delegates to ▼                            │
│  ┌──────────────────────┐  ┌─────────────────────┐  │
│  │ dyn GEPAAdapter      │  │ dyn DataLoader      │  │
│  └──────────────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

**Data flow per iteration:**
```
DataLoader ──minibatch──→ Engine
  → ParetoFront::select() → parent Candidate
  → Adapter::execute(parent, batch) → Vec<ExecutionTrace>
  → Adapter::reflect(parent, traces) → Reflection
  → Adapter::mutate(parent, reflection, lessons) → child Candidate
  → Adapter::evaluate(child, batch) → Vec<f64> scores
  → Adapter::evaluate(front_members, batch) → Vec<f64> scores per member
  → ParetoFront::try_accept(child, scores, front_scores) → accept/reject
  → Events emitted
```

## §3 Public Types

### 3.1 GEPAError

```rust
#[derive(Debug, thiserror::Error)]
pub enum GEPAError {
    #[error("adapter error: {source} (retryable: {retryable})")]
    AdapterError { source: String, retryable: bool },

    #[error("invalid config: {0}")]
    ConfigError(String),

    #[error("no training data provided: {0}")]
    EmptyDataError(String),

    #[error("run cancelled")]
    Cancelled,
}
```

**Satisfies:** GOAL-3.4, GUARD-6

### 3.2 Candidate

```rust
pub type CandidateId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: CandidateId,
    pub parameters: HashMap<String, String>,
    pub parent_id: Option<CandidateId>,
    pub generation: u32,
    pub reflection: Option<String>,
    pub lesson: Option<String>,
}
```

- Immutable after creation (GUARD-2). Fields are `pub` for construction; engine treats as read-only.
- `reflection` = raw diagnosis from reflect step. `lesson` = distilled insight carried forward in ancestor chain.
- Scores are NOT stored on Candidate — they live in the per-iteration score vectors passed to the Pareto front. This keeps candidates lightweight and preserves immutability.
- ID is monotonic `u64`, assigned by the engine's `next_id()` counter.

**Satisfies:** GOAL-5.2, GOAL-5.3, GOAL-5.4

### 3.3 Example & Traces

```rust
pub type ExampleId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub id: ExampleId,
    pub input: serde_json::Value,
    pub expected_output: Option<serde_json::Value>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub example_id: ExampleId,
    pub output: String,
    pub score: Option<f64>,
    pub asi: Option<String>,  // actionable side information
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub diagnosis: String,
    pub improvement_directions: Vec<String>,
}
```

**Satisfies:** GOAL-8.2, GOAL-3.2, GOAL-3.3

### 3.4 Config

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GEPAConfig {
    pub max_iterations: u64,           // default: 100
    pub minibatch_size: usize,         // default: 16
    pub stagnation_limit: u64,         // default: 20
    pub checkpoint_interval: u64,      // default: 1
    pub pareto_max_size: usize,        // default: 20
    pub max_consecutive_skips: u32,    // default: 5
    pub retry_max: u32,                // default: 3
    pub retry_base_delay: Duration,    // default: 1s
    pub time_budget: Option<Duration>, // default: None
    pub rng_seed: Option<u64>,         // default: None (random)
    pub max_lesson_depth: usize,       // default: 10
    pub checkpoint_path: PathBuf,      // default: "./gepa-checkpoint.json"
}

impl Default for GEPAConfig {
    fn default() -> Self; // always succeeds — defaults are valid
}

impl GEPAConfig {
    pub fn validate(&self) -> Result<(), GEPAError>; // called by builder.build()
}
```

**Validation (at construction):**
- `minibatch_size == 0` → `ConfigError`
- `max_iterations == 0` → `ConfigError`
- `stagnation_limit > max_iterations` → `ConfigError`
- `pareto_max_size < 1` → `ConfigError`

**Satisfies:** GOAL-7.1, GOAL-7.2, GOAL-7.3

### 3.5 State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GEPAState {
    pub pareto_front: ParetoFront,
    pub candidates: HashMap<CandidateId, Candidate>,
    pub next_candidate_id: u64,
    pub iteration: u64,
    pub rng_state: Vec<u8>,           // ChaCha8Rng serialized bytes
    pub statistics: GEPAStatistics,
    pub sampler_state: SamplerState,  // epoch cursor for resume
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GEPAStatistics {
    pub total_iterations: u64,
    pub skipped_iterations: u64,
    pub candidates_generated: u64,
    pub candidates_accepted: u64,
    pub best_score_history: Vec<(u64, f64)>,  // (iteration, score)
}
```

**Satisfies:** GOAL-6.1, GOAL-6.3

### 3.6 Result

```rust
pub struct GEPAResult {
    pub pareto_front: Vec<Candidate>,
    pub best_candidate: Candidate,
    pub validation_scores: HashMap<CandidateId, Vec<f64>>,
    pub validation_skipped: bool,
    pub termination_reason: TerminationReason,
    pub statistics: GEPAStatistics,
    pub state: GEPAState,               // for resume
    pub total_iterations: u64,
    pub elapsed_time: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TerminationReason {
    MaxIterations,
    TimeBudget,
    Stagnation,
    TooManySkips,
    Cancelled,
}
```

**Satisfies:** GOAL-1.2, GOAL-1.8

## §4 Adapter & DataLoader Traits

```rust
#[async_trait]
pub trait GEPAAdapter: Send + Sync + 'static {
    async fn execute(&self, candidate: &Candidate, examples: &[Example])
        -> Result<Vec<ExecutionTrace>, GEPAError>;

    async fn reflect(&self, candidate: &Candidate, traces: &[ExecutionTrace])
        -> Result<Reflection, GEPAError>;

    async fn mutate(&self, parent: &Candidate, reflection: &Reflection,
                    ancestor_lessons: &[String]) -> Result<Candidate, GEPAError>;

    async fn evaluate(&self, candidate: &Candidate, examples: &[Example])
        -> Result<Vec<f64>, GEPAError>;
}

pub trait DataLoader: Send + Sync + 'static {
    fn training_examples(&self) -> Vec<Example>;
    fn validation_examples(&self) -> Vec<Example>;
}
```

- `execute` returns rich traces (for reflection). `evaluate` returns scores only (for acceptance).
- All methods return `Result<T, GEPAError>`. Adapters should only return `AdapterError`.
- DataLoader is synchronous — MVP loads all examples into memory up front.

**Satisfies:** GOAL-3.1, GOAL-8.1

## §5 Engine

### 5.1 Builder

```rust
pub struct GEPAEngineBuilder {
    config: GEPAConfig,
    adapter: Option<Box<dyn GEPAAdapter>>,
    data_loader: Option<Box<dyn DataLoader>>,
    seeds: Option<Vec<Candidate>>,
    callbacks: Vec<Box<dyn Fn(&GEPAEvent) + Send + Sync>>,
    cancellation_token: Option<CancellationToken>,
}

impl GEPAEngineBuilder {
    pub fn new(config: GEPAConfig) -> Self;
    pub fn adapter(self, a: impl GEPAAdapter + 'static) -> Self;
    pub fn data_loader(self, d: impl DataLoader + 'static) -> Self;
    pub fn seeds(self, s: Vec<Candidate>) -> Self;
    pub fn on_event(self, cb: impl Fn(&GEPAEvent) + Send + Sync + 'static) -> Self;
    pub fn cancellation_token(self, t: CancellationToken) -> Self;
    pub fn build(self) -> Result<GEPAEngine, GEPAError>;
}
```

`build()` validates: adapter set, data_loader set, seeds non-empty, config valid. On success, initializes `GEPAState`:
- Inserts all seed candidates into `state.candidates` registry with monotonic IDs.
- Inserts all seed IDs into `ParetoFront::members` (seeds form the initial front).
- Sets `state.iteration = 0`, initializes statistics, creates `MinibatchSampler` from training examples.

**Satisfies:** GOAL-5.1

### 5.2 GEPAEngine

```rust
pub struct GEPAEngine {
    config: GEPAConfig,
    adapter: Box<dyn GEPAAdapter>,
    data_loader: Box<dyn DataLoader>,
    state: GEPAState,
    rng: ChaCha8Rng,
    callbacks: Vec<Box<dyn Fn(&GEPAEvent) + Send + Sync>>,
    cancellation_token: Option<CancellationToken>,
    start_time: Option<Instant>,
    consecutive_skips: u32,
    stagnation_counter: u64,
}

impl GEPAEngine {
    pub async fn run(mut self) -> Result<GEPAResult, GEPAError>;
}

// Resume goes through the builder to ensure callbacks/cancellation are set:
impl GEPAEngineBuilder {
    pub fn from_state(state: GEPAState, config: GEPAConfig) -> Self;
    // Then chain .adapter(), .data_loader(), .on_event(), .cancellation_token(), .build()
    // Seeds are NOT required — they're already in the restored state.
}
```

**Satisfies:** GOAL-1.1, GOAL-1.9

### 5.3 Main Loop — One Complete Iteration

Given iteration `i`, current `GEPAState`, seeded RNG:

1. **Check termination** — In order: cancellation → max iterations → time budget → too many skips → stagnation. First match wins.

2. **Emit** `IterationStarted { iteration: i }`.

3. **Sample minibatch** — `MinibatchSampler::next_batch(minibatch_size, &mut rng)` draws next batch. Epoch-based: each example used once before reuse (GOAL-8.3).

4. **Select** — `ParetoFront::select(&mut rng)` returns parent ID. Round-robin with random initial ordering, guarantees every front member is selected once per cycle. Resolve `CandidateId → &Candidate` from `state.candidates`.

5. **Execute** — `adapter.execute(&parent, &batch).await` with retry (§5.4). Returns `Vec<ExecutionTrace>`. Extract parent scores from traces. **Contract:** `execute` traces MUST include scores (non-None) for every example. If any trace has `score: None`, the engine calls `adapter.evaluate(&parent, &batch)` as fallback to get complete scores. This is a one-time cost; adapters should return scores in traces to avoid it.

6. **Reflect** — `adapter.reflect(&parent, &traces).await` with retry. Returns `Reflection`.

7. **Mutate** — Build ancestor lesson chain: walk `parent_id` links, collect `lesson` fields, truncate to `max_lesson_depth` (GOAL-4.3). `adapter.mutate(&parent, &reflection, &lessons).await` with retry. Assign new monotonic ID to returned candidate.

8. **Evaluate child** — `adapter.evaluate(&child, &batch).await` with retry. Returns `Vec<f64>` child scores.

9. **Evaluate front members** — For each front member (excluding parent, whose scores we got from step 5), call `adapter.evaluate(&member, &batch).await`. Collect `HashMap<CandidateId, Vec<f64>>` of all front member scores on this minibatch.

10. **Accept/Reject** — Check if child is dominated by any front member on this minibatch (§6.2). If non-dominated: insert into front, remove any members now dominated by child, prune if over capacity. Reset stagnation counter. If dominated: increment stagnation counter. Emit `CandidateAccepted` or `CandidateRejected`.

11. **Checkpoint** — If `iteration % checkpoint_interval == 0`: serialize `GEPAState` → temp file → rename (GUARD-3). Emit `CheckpointSaved`.

12. **Emit** `RunCompleted` on loop exit. Run final validation (§5.5). Return `GEPAResult`.

**Satisfies:** GOAL-1.1 through GOAL-1.8, GOAL-4.1, GOAL-4.2

### 5.4 Retry Logic

```rust
impl GEPAEngine {
    async fn call_with_retry<T, F, Fut>(&self, name: &str, f: F) -> Result<T, IterationOutcome>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, GEPAError>>;
}

enum IterationOutcome {
    Skipped { reason: String },
    Cancelled,
}
```

- On `AdapterError { retryable: true }`: retry up to `config.retry_max` with exponential backoff (`base_delay * 2^attempt`).
- On `AdapterError { retryable: false }`: skip immediately.
- After exhausting retries: skip iteration. Increment `consecutive_skips`. Do NOT increment stagnation counter.
- Between retries: check cancellation token.
- On successful adapter call: reset `consecutive_skips` to 0.

**Satisfies:** GOAL-3.5, GOAL-1.2(d)

### 5.5 Final Validation

After loop termination:
1. Call `data_loader.validation_examples()`.
2. If empty: set `validation_skipped = true`, skip.
3. For each front candidate: `adapter.evaluate(&candidate, &validation_examples).await`.
4. `best_candidate` = highest average validation score. Ties: newer candidate wins, then higher ID.

**Satisfies:** GOAL-8.4

## §6 Pareto Front

### 6.1 Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoFront {
    members: Vec<CandidateId>,
    max_size: usize,
    selection_cursor: usize,
}

impl ParetoFront {
    pub fn new(max_size: usize) -> Self;
    pub fn members(&self) -> &[CandidateId];
    pub fn len(&self) -> usize;
    pub fn contains(&self, id: CandidateId) -> bool;
    pub fn select(&mut self, rng: &mut ChaCha8Rng) -> Result<CandidateId, GEPAError>;
    pub fn try_accept(
        &mut self,
        child_id: CandidateId,
        child_scores: &[f64],
        front_scores: &HashMap<CandidateId, Vec<f64>>,
    ) -> bool;
}
```

- `members` stores `CandidateId`s only — lightweight (GOAL-2.4 serialization).
- `selection_cursor` tracks round-robin position.

**Satisfies:** GOAL-2.1, GOAL-2.4

### 6.2 Dominance & Acceptance

**`try_accept` algorithm:**

1. For each front member `m`, compare `child_scores` vs `front_scores[m]` element-wise on the current minibatch (same length, same example order).
2. Candidate A dominates B if: A scores ≥ B on every example AND strictly > on at least one.
3. If any member dominates child → reject (return `false`).
4. Accept child. Remove any members now dominated by child.
5. If `members.len() > max_size` → prune (§6.3).
6. `debug_assert!` front invariant (GUARD-1): no member dominates another.
7. Return `true`.

**Key simplification vs v1:** Dominance is checked on the current minibatch only — all candidates have scores for the same examples, so no sparse intersection logic, no `min_shared_examples` threshold needed. This is the core simplification that eliminates the evaluation cache for MVP.

**Trade-off:** A candidate accepted as non-dominated on minibatch A might be dominated on minibatch B. The front may retain candidates that a full-history comparison would prune. This is acceptable for MVP:
- Front size is small (≤20), so carrying a few extra members is cheap.
- As iterations progress, truly weak candidates will be dominated on future minibatches.
- Tier 1 upgrade adds evaluation cache + cross-minibatch dominance for stricter pruning.

**Satisfies:** GOAL-1.7, GOAL-2.1, GOAL-2.2, GUARD-1

### 6.3 Pruning

When `members.len() > max_size`:
1. Compute average score for each member from `front_scores` (current minibatch).
2. Remove the member with the lowest average. Ties: lowest `CandidateId` first (IDs are monotonic, so lower ID = older candidate).
3. Repeat until `members.len() <= max_size`.

Pruning is only triggered inside `try_accept`, which is always called with the current minibatch's `front_scores`. On resume from checkpoint, the front is already within capacity (checkpointed state is valid), so pruning never needs scores that aren't available.

**Satisfies:** GOAL-2.3

### 6.4 Selection

```
select(&mut self, rng: &mut ChaCha8Rng) -> Result<CandidateId, GEPAError>
```

1. If empty → unreachable in practice (seeds guarantee ≥1 member; `try_accept` only adds, never removes all). `debug_assert!(!self.members.is_empty())`. If somehow empty in release, return `Err(GEPAError::ConfigError("pareto front is empty — no seed candidates?".into()))`.
2. If `selection_cursor == 0` or `selection_cursor >= members.len()`: shuffle `members` using `rng`, reset cursor to 0.
3. Return `members[selection_cursor]`. Increment cursor.

This gives round-robin with randomized ordering. Every member is selected exactly once per cycle. Deterministic given same RNG seed (GUARD-5).

**Satisfies:** GOAL-1.3

## §7 Minibatch Sampling

```rust
pub struct MinibatchSampler {
    example_ids: Vec<ExampleId>,
    shuffled_order: Vec<usize>,
    cursor: usize,
    epoch: u64,
}

impl MinibatchSampler {
    pub fn new(example_ids: Vec<ExampleId>, rng: &mut ChaCha8Rng) -> Self;
    pub fn next_batch(&mut self, batch_size: usize, rng: &mut ChaCha8Rng) -> Vec<ExampleId>;
    pub fn state(&self) -> SamplerState;
    pub fn from_state(state: SamplerState, example_ids: Vec<ExampleId>) -> Self;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplerState {
    pub cursor: usize,
    pub epoch: u64,
}
```

- On construction: shuffle indices using seeded RNG.
- `next_batch`: read `batch_size` indices from cursor. On epoch boundary: take remaining, reshuffle for new epoch, fill rest from new epoch.
- If `batch_size >= example_ids.len()`: return all examples.
- `SamplerState` serialized in `GEPAState` for checkpoint resume.

**Satisfies:** GOAL-8.3

## §8 Events

```rust
#[derive(Debug, Clone)]
pub enum GEPAEvent {
    IterationStarted { iteration: u64 },
    CandidateAccepted { iteration: u64, candidate_id: CandidateId, front_size: usize },
    CandidateRejected { iteration: u64, candidate_id: CandidateId },
    IterationSkipped { iteration: u64, reason: String },
    CheckpointSaved { iteration: u64, path: PathBuf },
    RunCompleted { reason: TerminationReason, total_iterations: u64 },
}
```

- Callbacks registered via builder: `Vec<Box<dyn Fn(&GEPAEvent) + Send + Sync>>`.
- Engine calls `self.emit(event)` which iterates callbacks. Callbacks must be fast (no blocking).
- Built-in `TracingCallback`: logs events via `tracing` at INFO level.

```rust
pub fn tracing_callback() -> impl Fn(&GEPAEvent) + Send + Sync {
    |event| {
        match event {
            GEPAEvent::CandidateAccepted { iteration, candidate_id, front_size } =>
                tracing::info!(iteration, candidate_id, front_size, "candidate accepted"),
            GEPAEvent::CandidateRejected { iteration, candidate_id } =>
                tracing::info!(iteration, candidate_id, "candidate rejected"),
            GEPAEvent::IterationSkipped { iteration, reason } =>
                tracing::warn!(iteration, reason, "iteration skipped"),
            // ...
        }
    }
}
```

**Satisfies:** GOAL-9.1, GOAL-9.2

## §9 Checkpoint / Resume

**Write (atomic):**
1. Serialize `GEPAState` to JSON (includes RNG state, sampler state, candidates, front, stats).
2. Write to `{checkpoint_path}.tmp`.
3. `std::fs::rename(tmp, checkpoint_path)` — atomic on POSIX.

**Resume:**
1. Read + deserialize `GEPAState` from checkpoint file.
2. Restore RNG from `rng_state` bytes.
3. Restore `MinibatchSampler` from `SamplerState`.
4. Call `GEPAEngineBuilder::from_state(state, config).adapter(...).data_loader(...).build()`.
5. `run()` continues from `state.iteration`.

**Satisfies:** GOAL-6.1, GOAL-6.2, GOAL-1.9, GUARD-3

## §10 Cancellation

```rust
#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self;
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
}
```

- Checked at: (1) top of each iteration, (2) between retry attempts, (3) after each adapter call.
- Never aborts a mid-flight adapter call — always awaits completion first.
- If not provided, no overhead (skips all checks).

**Satisfies:** GOAL-1.2(e)

## §11 Cross-Cutting Concerns

### Determinism (GUARD-5)

Single `ChaCha8Rng` seeded from `config.rng_seed`. All randomness (minibatch sampling, selection shuffling) draws from this RNG in fixed call order. RNG never cloned or shared. When `rng_seed` is `None`, random seed generated and stored in `GEPAState` for post-hoc reproducibility.

### Score Semantics

- **Higher is better** — all comparisons assume higher scores are superior.
- **NaN** — treated as 0.0 (adapter should avoid returning NaN; engine does not crash).
- **±Inf** — clamped to ±f64::MAX.
- Sanitization applied once after every `evaluate` call, before any comparison.

### Async Design

`GEPAEngine::run()` is `async fn` returning a `Send` future. Loop is sequential — one adapter call at a time. Adapter implementations may use internal concurrency. Tokio dependency minimal: only for `tokio::time::sleep` in retry backoff.

### No `.unwrap()` (GUARD-6)

All fallible operations return `Result`. `debug_assert!` for invariant checks (compiled out in release). No panics in library code.

## §12 Dependency Choices

| Crate | Purpose | Justification |
|---|---|---|
| `serde` + `serde_json` | Checkpoint serialization, config persistence | Industry standard |
| `thiserror` | Error derive on `GEPAError` | Zero-cost; GUARD-6 |
| `tracing` | Structured logging | Standard async-ecosystem logging |
| `rand` + `rand_chacha` | Deterministic PRNG | GUARD-5 |
| `async-trait` | Async trait desugaring | Required until stable async fn in dyn traits |
| `tokio` (feature: `time`) | `sleep` for retry backoff | Minimal surface |

**Excluded:** HTTP clients, LLM SDKs, database drivers (GUARD-4).

## §13 Step 9 Cost Analysis — Evaluating All Front Members

The biggest change from v1 is that step 9 evaluates **all front members** on each minibatch (not just using cached scores). Cost analysis:

- Front size ≤ 20. Each `evaluate` call = 1 adapter (LLM) call.
- Per iteration: 1 execute + 1 reflect + 1 mutate + 1 evaluate(child) + up to 19 evaluate(front members) = ~23 adapter calls.
- v1 with eval cache: 1 execute + 1 reflect + 1 mutate + 1 evaluate(child) + ~0-5 cache-miss evaluates = ~4-8 adapter calls.

**This is ~3-5x more adapter calls per iteration.** Each `evaluate` call is a single LLM invocation that receives the full minibatch (16 examples) and returns 16 scores — it is NOT 16 separate calls. Acceptable because:
1. `evaluate` is cheaper than `execute` (scores only, no traces — shorter prompt, shorter response).
2. Front is small (≤20). Worst case: 23 LLM calls/iteration.
3. With eval cache (Tier 1 upgrade), most of these become cache hits.
4. Alternative: only compare child vs parent (loses GUARD-1 consistency). We chose correctness.

**Optimization (implemented):** Parent scores are extracted from execute traces (step 5), so parent is not re-evaluated. Only `front_size - 1` extra evaluate calls needed.

**Future optimization (Tier 1):** Add evaluation cache. Most front members will have cached scores for recent minibatches, reducing extra calls to near zero.
