# Design: gepa-core (Master)

> Architectural overview — HOW the crate is built. Per-feature details live in feature design docs.

## §1 Overview

`gepa-core` is a pure-algorithmic Rust library implementing the GEPA evolutionary prompt
optimization loop. It owns zero LLM dependencies (GUARD-5); all LLM interaction is delegated
through the `GEPAAdapter` async trait. The crate orchestrates: selecting candidates, managing
the Pareto front, sampling minibatches, accumulating scores, checkpointing state, and emitting events.

**Key design trade-offs:**

| Decision | Chosen | Rationale |
|---|---|---|
| Pareto pruning | Crowding distance | O(N·M·log M) vs O(N^M) hypervolume; M can reach 200 (GOAL-2.4) |
| Score storage | Separate eval cache | Keeps candidates immutable (GUARD-2), lightweight (GUARD-7) |
| Concurrency | Sequential single-threaded loop | Determinism (GUARD-9); adapter may use internal concurrency |
| RNG | ChaCha8Rng | Cross-version determinism (GUARD-9); StdRng may change across versions |
| Extension points | Trait objects (`Box<dyn>`) | Runtime polymorphism; LLM latency dwarfs vtable cost |
| Async runtime | Runtime-agnostic, tokio for timeouts only | Per GUARD-11; only `tokio::time::timeout` used |

## §2 Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      GEPAEngine (feat 1)                    │
│  ┌──────────┐  ┌───────────┐  ┌──────────┐  ┌───────────┐  │
│  │ Config   │  │   State   │  │  Events  │  │    RNG    │  │
│  │ (feat 7) │  │  (feat 6) │  │ (feat 9) │  │ ChaCha8   │  │
│  └──────────┘  └─────┬─────┘  └──────────┘  └───────────┘  │
│         ┌────────────┼────────────┐                         │
│  ┌──────┴─────┐ ┌────┴────┐ ┌────┴──────┐                  │
│  │ Pareto     │ │  Eval   │ │ Candidate │                  │
│  │ Front (2)  │ │Cache(6.3│ │Registry(5)│                  │
│  └──────┬─────┘ └─────────┘ └───────────┘                  │
│  ┌──────┴───────────────────────────┐                       │
│  │      Proposers (feat 4)          │                       │
│  │  ┌──────────┐ ┌───────────────┐  │                       │
│  │  │ Mutation  │ │ Merge (opt.)  │  │                       │
│  └──┴──────────┴─┴───────────────┴──┘                       │
│         ▼ delegates to ▼                                    │
│  ┌────────────────────────┐  ┌───────────────────────────┐  │
│  │ dyn GEPAAdapter (3)   │  │ dyn DataLoader (8)        │  │
│  └────────────────────────┘  └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

**Data flow per iteration:**
```
DataLoader ──minibatch──→ Engine
  → ParetoFront::select → parent Candidate
  → Adapter::execute(parent, batch) → Vec<ExecutionTrace>
  → Adapter::reflect(parent, traces) → Reflection
  → Adapter::mutate(parent, reflection, lessons) → child Candidate
  → Adapter::evaluate(child, batch) → Vec<f64> scores
  → EvalCache.insert + ParetoFront::try_accept → accept/reject → Events
```

## §3 Cross-Cutting Concerns

### 3.1 Error Handling

All fallible operations return `Result<T, GEPAError>`. Uses `thiserror` (GUARD-8).

```rust
#[derive(Debug, thiserror::Error)]
pub enum GEPAError {
    #[error("adapter error: {source} (retryable: {retryable})")]
    AdapterError { source: String, retryable: bool },
    #[error("adapter call timed out")]
    Timeout,
    #[error("rate limited (retry after {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },
    #[error("run cancelled")]
    Cancelled,
    #[error("invalid config: {message}")]
    InvalidConfig { message: String },
    #[error("invalid candidate: {message}")]
    InvalidCandidate { message: String },
    #[error("no training data provided")]
    EmptyDataError,
    #[error("candidate not found: {id}")]
    CandidateNotFound { id: u64 },
    #[error("corrupt checkpoint: {message}")]
    CheckpointCorrupt { message: String },
    #[error("all seed candidates failed evaluation")]
    AllSeedsFailed,
    #[error("pareto front is empty")]
    EmptyFrontError,
    #[error("insufficient front size for merge: need 2, have {size}")]
    InsufficientFrontSize { size: usize },
    #[error("internal error: {message}")]
    Internal { message: String },
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
}
```

**Error flow:** Adapter errors → engine retry policy (GOAL-7.5) → after exhaustion, `Skip`
(increment skip counter) or `Halt` per `ErrorPolicy`. Config errors caught at construction
(GOAL-7.3); data-dependent validation at `run()` start (GOAL-1.0). No `.unwrap()` in library
code (GUARD-8).

### 3.2 Async Design

`GEPAEngine::run()` is `async fn` returning a `Send` future (GUARD-11). The loop is sequential
— one adapter call at a time. Adapter implementations may use internal concurrency. Tokio
dependency is minimal: only `tokio::time::timeout` for adapter call timeouts. Wall-clock budget
uses `std::time::Instant`.

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
    async fn merge(&self, _a: &Candidate, _b: &Candidate)
        -> Result<Candidate, GEPAError> { /* default: Err */ }
}
```

### 3.3 Determinism (GUARD-9)

A single `ChaCha8Rng` seeded at engine construction from `GEPAConfig::rng_seed`. All
randomness — minibatch sampling (GOAL-8.3), Pareto selection (GOAL-2.3), tie-breaking
(GOAL-2.4, GOAL-4.4) — draws from this RNG in a fixed call order. The RNG is never cloned,
shared, or accessed outside the sequential loop. When `rng_seed` is `None`, a random seed is
generated and recorded in `GEPAState` for post-hoc reproducibility.

### 3.4 Generic Type Parameters

`Candidate` is **not** generic — it stores text parameters as `HashMap<String, String>`. GEPA
optimizes opaque text; structured domain knowledge lives in the adapter. All core types
(`Candidate`, `GEPAState`, `GEPAConfig`, `GEPAResult`) are concrete. The only polymorphic
boundaries are `Box<dyn GEPAAdapter>` and `Box<dyn DataLoader>`.

### 3.5 Trait Objects vs Generics

| Boundary | Mechanism | Reason |
|---|---|---|
| `GEPAAdapter` | `Box<dyn GEPAAdapter>` | Runtime polymorphism; vtable negligible vs LLM latency |
| `DataLoader` | `Box<dyn DataLoader>` | Same rationale |
| Event callbacks | `Box<dyn Fn(&GEPAEvent) + Send + Sync>` | Multiple heterogeneous callbacks |
| Internal algorithms | Concrete types | Pareto front, eval cache — no polymorphism needed |

### 3.6 Logging / Tracing (GOAL-9.5)

All instrumentation uses `tracing`. Each iteration runs inside `info_span!("gepa_iteration",
iteration = i)`. Log levels per GOAL-9.5: INFO for lifecycle events, DEBUG for step details,
WARN for errors/stagnation. A built-in `TracingCallback` bridges events to `tracing`,
serializing payloads at TRACE level. No `println!` or `eprintln!`.

### 3.8 Invariant Enforcement (GUARD-1, GUARD-2)

Critical data-structure invariants are guarded by `debug_assert!` checks after every mutation:

- **GUARD-1 (Pareto front invariant):** After every `ParetoFront::try_accept` or dominance
  pruning operation, `debug_assert!` verifies that no candidate in the front is dominated by
  any other. Details in the Pareto front feature design (feature 2).
- **GUARD-2 (Candidate immutability):** Candidates are immutable value types — consumed by
  move, never passed as `&mut`. Fields are `pub` for ergonomic construction, but all engine
  code treats candidates as read-only after creation. `debug_assert!` checks in the candidate
  registry verify that no candidate ID is ever reused or mutated.

These assertions are compiled out in release builds (`debug_assertions` cfg) but run in every
test and CI build, catching invariant violations early.

### 3.9 Score Semantics (GUARD-10)

All score values follow consistent semantics across every feature (Pareto front, eval cache,
statistics, selection):

1. **Higher is better** — all comparisons, dominance checks, and rankings assume higher
   scores are superior. Adapters must return scores in this convention.
2. **NaN → None** — `f64::is_nan()` scores are treated as unevaluated (`None` in the eval
   cache). The example is marked as needing re-evaluation.
3. **±Inf → clamp** — `f64::INFINITY` is clamped to `f64::MAX`; `f64::NEG_INFINITY` is
   clamped to `f64::MIN`. A `ScoreWarning` event is emitted on each clamped value.
4. **Consistent application** — Score sanitization is applied at the single entry point
   (§5 step 8, after `adapter.evaluate`) so all downstream consumers see clean values.

### 3.10 Performance Budget (GUARD-6)

Engine overhead is expected to be negligible since all heavy computation (LLM calls) is in
the adapter. The sequential loop (§5) performs only in-memory data structure operations
(Pareto front updates, eval cache lookups, RNG draws) between adapter calls. GUARD-6
compliance (engine-internal computation < 5% of adapter call time) will be validated via
benchmarks comparing engine-internal time vs total iteration time, tracked in CI.

### 3.11 Memory Growth (GUARD-7)

Memory per candidate is O(parameters + evaluated_examples). With minibatch-based evaluation,
each candidate is evaluated on O(minibatch_size × re_eval_rounds) examples, not all examples.
Total memory is O(candidates × max_evals_per_candidate), which is linear in candidates for
fixed configuration. The Pareto front caps at `pareto_max_size` candidates; evicted candidates
are removed from the eval cache. Peak memory is therefore bounded by
O(pareto_max_size × max_evals_per_candidate × avg_parameter_size).

### 3.12 Send + Sync (GUARD-11)

All public types are `Send + Sync`: `Candidate` (immutable value type), `GEPAConfig` (plain
data), `GEPAState` (owned data, no interior mutability), `GEPAEngine` (holds `Box<dyn Adapter>`
bound `Send + Sync`). The `run()` future is `Send`. Event callbacks bound `Send + Sync`.

## §4 Feature Index

1. **Core Engine** (`requirements-01`) — `GEPAEngine` and `run()`. Orchestrates the 5-step loop,
   checks stopping criteria (GOAL-1.2a-d), coordinates all features.

2. **Pareto Front** (`requirements-02`) — Non-dominated set with incremental insert + dominance
   pruning (GOAL-2.2), crowding-distance size cap (GOAL-2.4), diversity-aware selection (GOAL-2.3).

3. **Adapter Interface** (`requirements-03`) — `GEPAAdapter` async trait: `execute`, `reflect`,
   `mutate`, `evaluate`, optional `merge`. Sole integration boundary (GUARD-5).

4. **Proposers** (`requirements-04`) — Mutation proposer (every iteration) and optional merge
   proposer (periodic, GOAL-7.7). Owns selection tracking and ancestor lesson chain (GOAL-4.2).

5. **Candidates** (`requirements-05`) — `Candidate` with monotonic ID, text parameter map,
   lineage metadata. Immutable after creation (GUARD-2), scores stored externally.

6. **State** (`requirements-06`) — `GEPAState`: Pareto front, candidate registry, eval cache
   (GOAL-6.3), iteration counter, statistics. Atomic checkpoint/resume (GUARD-4).

7. **Configuration** (`requirements-07`) — `GEPAConfig` with validated defaults. Two-phase
   validation: config-only at construction, data-dependent at `run()` start.

8. **Data Loading** (`requirements-08`) — `DataLoader` trait for training/validation examples.
   Epoch-based minibatch sampling (GOAL-8.3). Score matrix backfill (GOAL-8.5a-c).

9. **Events** (`requirements-09`) — `GEPAEvent` enum (16 variants). Callback registration via
   builder. Zero-alloc with no callbacks (GOAL-9.6). Panic-safe invocation (GOAL-9.4).

## §5 Data Flow — One Complete Iteration

Given iteration `i`, current `GEPAState`, seeded RNG:

1. **Time gate + cancellation** — Check cancellation token (GOAL-3.7); if cancelled,
   terminate with `Cancelled`. If `Instant::elapsed() ≥ time_budget`, terminate `TimeBudget`
   (GOAL-7.6). Cancellation is also checked before each adapter call (steps 5–8) to allow
   prompt termination mid-iteration.
2. **Emit** `IterationStarted`.
3. **Sample minibatch** — `EpochSampler` draws `minibatch_size` examples using seeded RNG.
   Epoch-boundary concatenation ensures every example used exactly once per epoch (GOAL-8.3).
4. **Select** — `ParetoFront::select(&mut rng)` returns parent ID. Round-robin with
   overfitting-delta deprioritization (GOAL-2.3). Merge iterations select two complementary
   candidates (GOAL-4.4). Emit `CandidateSelected`.
5. **Execute** — `adapter.execute(&parent, &batch)` with retry policy. Emit `ExecutionCompleted`.
6. **Reflect** — `adapter.reflect(&parent, &traces)`. Emit `ReflectionCompleted`.
7. **Mutate** — Build ancestor lesson chain (walk lineage, truncate to `max_lesson_depth` per
   GOAL-4.2b). `adapter.mutate(...)` → new `Candidate` with next monotonic ID. Emit
   `MutationCompleted`.
8. **Evaluate** — `adapter.evaluate(&child, &batch)` → scores. Sanitize per GUARD-10
   (NaN→None, ±Inf→clamp). Write to eval cache.
9. **Accept/Reject** — Compare child vs parent on shared examples (GOAL-1.7a). If not dominated,
   insert into front, prune dominated members (GOAL-2.2), cap at `pareto_max_size` via crowding
   distance (GOAL-2.4). Emit `CandidateAccepted` or `CandidateRejected`. Update stagnation counter.
10. **Backfill** (every `re_eval_interval`) — Evaluate sparsest-coverage front candidates on
    unseen examples (GOAL-8.5a), recompute dominance (GOAL-8.5b), compute overfitting deltas
    (GOAL-8.5c). Emit `ReEvaluationCompleted`.
11. **Checkpoint** (every `checkpoint_interval`) — Serialize `GEPAState` → temp file → rename
    (GUARD-4). Emit `CheckpointSaved`.
12. **Stagnation check** — Emit `StagnationWarning` at >50% limit. Terminate on limit breach
    or `max_consecutive_skips` exceeded.
13. **Emit** `IterationCompleted`. Loop or terminate → final validation (GOAL-8.6) → `GEPAResult`.

## §6 Public API Surface

```rust
// Builder pattern (GOAL-1.0)
pub struct GEPAEngineBuilder { /* ... */ }
impl GEPAEngineBuilder {
    pub fn new(config: GEPAConfig) -> Self;
    pub fn adapter(self, a: impl GEPAAdapter + 'static) -> Self;
    pub fn data_loader(self, l: impl DataLoader + 'static) -> Self;
    pub fn seed_candidates(self, seeds: Vec<Candidate>) -> Self;
    pub fn on_event(self, cb: impl Fn(&GEPAEvent) + Send + Sync + 'static) -> Self;
    pub fn build(self) -> Result<GEPAEngine, GEPAError>;
}

pub struct GEPAEngine { /* ... */ }
impl GEPAEngine {
    pub async fn run(self) -> Result<GEPAResult, GEPAError>;
    pub fn resume(state: GEPAState, ...) -> Result<GEPAEngine, GEPAError>;
}

// Core data types
pub type CandidateId = u64;
pub type ExampleId = u64;

pub struct Candidate {
    pub id: CandidateId,
    pub parameters: HashMap<String, String>,
    pub parent_id: Option<CandidateId>,
    pub generation: u32,
    pub reflection: Option<String>,
    pub lesson: Option<String>,
    pub created_at: SystemTime,
}
// Note: `reflection` is the raw analysis from the reflect step; `lesson` is the
// distilled improvement insight carried forward. The ancestor lesson chain (GOAL-4.2b)
// is built by walking `parent_id` links and collecting `lesson` from each ancestor,
// truncated to `max_lesson_depth`. Fields are `pub` for ergonomic construction but
// candidates are treated as immutable after creation (GUARD-2; see §3.8).

pub struct Example {
    pub id: ExampleId,
    pub input: serde_json::Value,
    pub expected_output: Option<serde_json::Value>,
    pub metadata: HashMap<String, String>,
    pub difficulty: Option<String>,
}

pub struct ExecutionTrace {
    pub example_id: ExampleId,
    pub output: String,
    pub score: Option<f64>,
    pub asi: Option<String>,
}

pub struct Reflection {
    pub diagnosis: String,
    pub improvement_directions: Vec<String>,
}

pub struct GEPAResult {
    pub pareto_front: Vec<Candidate>,
    pub validation_scores: HashMap<CandidateId, Vec<(ExampleId, f64)>>,
    pub validation_skipped: bool,
    pub best_candidate: Candidate,
    pub termination_reason: TerminationReason,
    pub statistics: GEPAStatistics,
    pub state: GEPAState,
    pub total_iterations: u64,
    pub elapsed_time: Duration,
}

pub enum TerminationReason {
    MaxIterations, TimeBudget, Stagnation, TooManySkips, Cancelled,
}
```

## §7 Dependency Choices

| Crate | Purpose | Justification |
|---|---|---|
| `serde` + `serde_json` | Checkpoint serialization (GOAL-6.1), config persistence (GOAL-7.4) | Industry standard for Rust serialization |
| `thiserror` | `Error` + `Display` derive on `GEPAError` | Zero-cost; satisfies GUARD-8 |
| `tracing` | Structured logging with span context (GOAL-9.5) | Standard async-ecosystem logging; no runtime dep |
| `rand` | RNG trait interface for all randomized operations | Required by GOAL-8.3, GOAL-2.3 |
| `rand_chacha` | `ChaCha8Rng` deterministic PRNG | Explicit algorithm for GUARD-9 cross-version determinism |
| `async-trait` | Async trait desugaring for `GEPAAdapter`, `DataLoader` | Required until stable async fn in dyn traits |
| `tokio` (feature: `time`) | `tokio::time::timeout` for adapter calls (GOAL-8.4) | Minimal surface; consumer provides runtime |

**Excluded:** HTTP clients, LLM SDKs, database drivers (GUARD-5). Tokio is used only for its
`time` module — no tasks spawned, no runtime managed.
