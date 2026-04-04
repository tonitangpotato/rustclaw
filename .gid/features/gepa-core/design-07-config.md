# Design: GEPA Configuration

## 1. Overview

`GEPAConfig` is the immutable configuration bundle for the GEPA engine. It centralizes every tunable parameter — iteration limits, minibatch sizing, retry/backoff policy, checkpoint intervals, Pareto front bounds, merge proposer settings, and time budgets — into a single validated struct. Configuration is constructed via a builder pattern that enforces validation at `build()` time, returning `Err(ConfigError)` for any invalid combination (GOAL-7.3). Once built, the config is frozen: it is cloned into the engine at construction and snapshotted verbatim into every checkpoint via serde (GOAL-7.4).

Key trade-off: eagerly validating all constraints at build time (rather than lazily at first use) catches misconfigurations before any adapter calls are made, at the cost of a more complex builder. This is the right trade-off for a library where adapter calls are expensive LLM invocations.

**Satisfies:** GOAL-7.1 through GOAL-7.7, GUARD-4 (no panics — returns Result), GUARD-8 (Debug impl).

## 2. Components

### 2.1 GEPAConfig Struct

**Responsibility:** Hold all engine parameters as a read-only, serializable value type.

**Interface:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GEPAConfig {
    // Core loop
    pub max_iterations: u64,
    pub minibatch_size: usize,
    pub stagnation_limit: u64,
    pub checkpoint_interval: u64,

    // Pareto front
    pub pareto_max_size: usize,
    pub min_shared_examples: usize,

    // Re-evaluation / backfill
    pub re_eval_interval: u64,
    pub re_eval_sample_size: usize,
    pub max_re_eval_per_iteration: usize,

    // Retry / backoff
    pub retry_max: u32,
    pub backoff_strategy: BackoffStrategy,
    pub base_delay: Duration,
    pub max_retry_delay: Duration,
    pub error_policy: ErrorPolicy,
    pub max_consecutive_skips: u32,

    // Time budget
    pub time_budget: Option<Duration>,

    // Merge proposer
    pub merge_enabled: bool,
    pub merge_interval: u64,
    pub merge_strategy: MergeStrategy,

    // Misc
    pub rng_seed: Option<u64>,
    pub max_lesson_depth: usize,
    pub eval_cache_max_size: Option<usize>,
    pub data_loader_timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BackoffStrategy {
    Fixed,
    Exponential,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ErrorPolicy {
    Skip,
    Halt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MergeStrategy {
    Complementary,
    Random,
}
```

**Key Details:**
- All fields are `pub` for read access; mutation is prevented by only distributing `&GEPAConfig` references after engine construction.
- `min_shared_examples` defaults to `minibatch_size` — the builder computes this if the user doesn't set it explicitly.
- `max_re_eval_per_iteration` defaults to `pareto_max_size * minibatch_size / 2` — computed at build time from other fields.
- `rng_seed: None` means the builder generates a random seed at build time using `rand::random::<u64>()`, then stores the concrete seed. The built config always has an effective seed available (for checkpoint reproducibility).
- Implements `Default` via the builder's defaults, not via `#[derive(Default)]` — ensures validation runs.

**Satisfies:** GOAL-7.1, GOAL-7.2, GOAL-7.4, GOAL-7.5, GOAL-7.6, GOAL-7.7

### 2.2 ConfigBuilder

**Responsibility:** Accumulate user-provided overrides, compute derived defaults, validate, and produce `GEPAConfig`.

**Interface:**
```rust
#[derive(Debug)]
pub struct ConfigBuilder {
    max_iterations: Option<u64>,
    minibatch_size: Option<usize>,
    stagnation_limit: Option<u64>,
    checkpoint_interval: Option<u64>,
    pareto_max_size: Option<usize>,
    min_shared_examples: Option<usize>,
    re_eval_interval: Option<u64>,
    re_eval_sample_size: Option<usize>,
    max_re_eval_per_iteration: Option<usize>,
    retry_max: Option<u32>,
    backoff_strategy: Option<BackoffStrategy>,
    base_delay: Option<Duration>,
    max_retry_delay: Option<Duration>,
    error_policy: Option<ErrorPolicy>,
    max_consecutive_skips: Option<u32>,
    time_budget: Option<Duration>,
    merge_enabled: Option<bool>,
    merge_interval: Option<u64>,
    merge_strategy: Option<MergeStrategy>,
    rng_seed: Option<u64>,
    max_lesson_depth: Option<usize>,
    eval_cache_max_size: Option<Option<usize>>,
    data_loader_timeout_secs: Option<u64>,
}

impl ConfigBuilder {
    pub fn new() -> Self { /* all fields None */ }
    pub fn max_iterations(mut self, v: u64) -> Self { self.max_iterations = Some(v); self }
    pub fn minibatch_size(mut self, v: usize) -> Self { self.minibatch_size = Some(v); self }
    // ... one setter per field, same pattern ...
    pub fn build(self) -> Result<GEPAConfig, ConfigError> { /* validate + fill defaults */ }
}

impl Default for ConfigBuilder {
    fn default() -> Self { Self::new() }
}
```

**Key Details:**
- Each setter consumes and returns `self` for method chaining.
- `build()` applies defaults for any unset field, computes derived defaults (`min_shared_examples`, `max_re_eval_per_iteration`), then calls the validation logic from §2.3.
- `GEPAConfig::default()` is implemented as `ConfigBuilder::new().build().unwrap()` — the default values are guaranteed valid, so the unwrap is safe (tested).
- `GEPAConfig::builder()` is a convenience alias for `ConfigBuilder::new()`.

**Satisfies:** GOAL-7.2, GOAL-7.3

### 2.3 Validation Logic

**Responsibility:** Reject invalid parameter combinations with descriptive `ConfigError` variants.

**Interface:**
```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("minibatch_size must be > 0, got {value}")]
    ZeroMinibatchSize { value: usize },
    #[error("max_iterations must be > 0, got {value}")]
    ZeroMaxIterations { value: u64 },
    #[error("stagnation_limit ({stagnation_limit}) must be <= max_iterations ({max_iterations})")]
    StagnationExceedsMax { stagnation_limit: u64, max_iterations: u64 },
    #[error("pareto_max_size must be >= 1, got {value}")]
    ZeroParetoMaxSize { value: usize },
    #[error("min_shared_examples must be > 0, got {value}")]
    ZeroMinSharedExamples { value: usize },
    #[error("checkpoint_interval must be > 0, got {value}")]
    ZeroCheckpointInterval { value: u64 },
    #[error("re_eval_interval must be > 0, got {value}")]
    ZeroReEvalInterval { value: u64 },
    #[error("re_eval_sample_size must be > 0, got {value}")]
    ZeroReEvalSampleSize { value: usize },
    #[error("max_re_eval_per_iteration must be > 0 (use None to disable), got {value}")]
    ZeroMaxReEvalPerIteration { value: usize },
    #[error("merge_interval must be > 0 when merge is enabled, got {value}")]
    ZeroMergeInterval { value: u64 },
    #[error("base_delay must be > 0 for exponential backoff, got {value:?}")]
    ZeroBaseDelayExponential { value: Duration },
}

fn validate(config: &GEPAConfig) -> Result<(), ConfigError> { /* checks each rule, returns first error */ }
```

**Key Details:**
- Validation is called inside `ConfigBuilder::build()` after defaults are applied.
- Checks are ordered from most fundamental (zero sizes) to cross-field constraints (stagnation vs max_iterations).
- Valid edge cases explicitly allowed: `retry_max=0`, `max_consecutive_skips=0`, `time_budget=Some(Duration::ZERO)`.
- Returns the first invalid condition found (not all errors at once) — simpler for users to fix iteratively.
- No panics anywhere per GUARD-4.

**Satisfies:** GOAL-7.3, GUARD-4

### 2.4 Config Immutability

**Responsibility:** Ensure config cannot change after engine construction; snapshot config in checkpoints.

**Key Details:**
- `GEPAEngine` stores `GEPAConfig` by value (owned clone) and only exposes `&GEPAConfig` via `pub fn config(&self) -> &GEPAConfig`.
- The checkpoint struct (from feature 06) includes `config: GEPAConfig` as a serialized field. On resume, the engine uses the checkpoint's config, ignoring any newly-provided config — this guarantees reproducibility.
- `GEPAConfig` derives `Clone + Serialize + Deserialize` to support both snapshotting and checkpoint serialization.

**Satisfies:** GOAL-7.4, GUARD-9

### 2.5 Backoff/Retry Configuration

**Responsibility:** Parameterize exponential/fixed backoff for adapter error recovery.

**Key Details:**
- Backoff delay computation (used by the engine's retry loop, not implemented here):
  - `Fixed`: delay = `base_delay` for every retry.
  - `Exponential`: delay = `min(base_delay * 2^attempt, max_retry_delay)`.
  - When adapter returns `RateLimited { retry_after: Some(d) }`: delay = `max(d, computed_backoff)`.
- `max_consecutive_skips` caps sequential skipped iterations. When exceeded, the engine halts with `TerminationReason::TooManySkips`.
- Skipped iterations do NOT count toward the stagnation counter (per GOAL-7.5).
- `ErrorPolicy::Skip` skips the iteration; `ErrorPolicy::Halt` halts the engine immediately after retry exhaustion.

**Satisfies:** GOAL-7.5

### 2.6 Serde Support

**Responsibility:** Enable JSON/TOML serialization for checkpoints and human-editable config files.

**Key Details:**
- All types derive `Serialize, Deserialize` via serde.
- `Duration` fields use `#[serde(with = "humantime_serde")]`-style custom serializer: stored as `"1s"`, `"60s"`, `"500ms"` in human-readable formats; raw nanos in binary formats. Implementation: a small `serde_duration` module with `serialize`/`deserialize` functions using `serde::Serializer::serialize_str` and parsing `"{n}s"` / `"{n}ms"` patterns.
- `Option<Duration>` for `time_budget` serializes as `null` when `None`.
- Enum variants (`BackoffStrategy`, `ErrorPolicy`, `MergeStrategy`) serialize as lowercase strings: `"fixed"`, `"exponential"`, `"skip"`, `"halt"`, `"complementary"`, `"random"` via `#[serde(rename_all = "lowercase")]`.

**Satisfies:** GOAL-7.4

## 3. Default Values Table

| Parameter | Default | Source |
|-----------|---------|--------|
| `max_iterations` | 100 | GOAL-7.2 |
| `minibatch_size` | 16 | GOAL-7.2 |
| `stagnation_limit` | 20 | GOAL-7.2 |
| `checkpoint_interval` | 1 | GOAL-7.2 |
| `pareto_max_size` | 50 | GOAL-7.2 |
| `min_shared_examples` | `= minibatch_size` (16) | GOAL-7.1 |
| `re_eval_interval` | 5 | GOAL-8.5a |
| `re_eval_sample_size` | `= minibatch_size` (16) | GOAL-8.5a |
| `max_re_eval_per_iteration` | `= pareto_max_size * minibatch_size / 2` (400) | GOAL-7.1 |
| `retry_max` | 3 | GOAL-7.5 |
| `backoff_strategy` | `Exponential` | GOAL-7.5 |
| `base_delay` | 1 second | GOAL-7.5 |
| `max_retry_delay` | 60 seconds | GOAL-7.5 |
| `error_policy` | `Skip` | GOAL-7.5 |
| `max_consecutive_skips` | 5 | GOAL-7.5 |
| `time_budget` | `None` (unlimited) | GOAL-7.6 |
| `merge_enabled` | `false` | GOAL-7.7 |
| `merge_interval` | 10 | GOAL-7.7 |
| `merge_strategy` | `Complementary` | GOAL-7.7 |
| `rng_seed` | `None` (random at build) | GOAL-7.1 |
| `max_lesson_depth` | 10 | GOAL-7.1 |
| `eval_cache_max_size` | `None` (unlimited) | GOAL-7.1 |
| `data_loader_timeout_secs` | 30 | GOAL-8.4 |

## 4. Integration Points

- **Engine (feature 01):** `GEPAEngine::new(config, adapter, data_loader)` clones the config. The engine reads `&GEPAConfig` throughout the loop for iteration limits, retry policy, and time budget checks.
- **Checkpoint (feature 06):** `Checkpoint { config: GEPAConfig, ... }` — serialized via serde on save, deserialized on resume.
- **Data loading (feature 08):** Engine reads `minibatch_size`, `re_eval_interval`, `re_eval_sample_size`, `data_loader_timeout_secs` from config.
- **Events (feature 09):** No direct coupling — config is not exposed to callbacks. The `TracingCallback` does not reference config.
- **Adapter retry loop:** Engine's retry logic reads `retry_max`, `backoff_strategy`, `base_delay`, `max_retry_delay`, `error_policy`, `max_consecutive_skips` from config each iteration.
