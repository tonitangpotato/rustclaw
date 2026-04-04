# Design: GEPA State Management

## 1. Overview

`EngineState` is the checkpoint-and-resume backbone of the GEPA engine. It aggregates everything needed to pause an optimization run and restart it later with identical behavior: the Pareto front, all candidate history, the evaluation cache, iteration counter, RNG state, proposer state, and run statistics.

The design prioritizes **correctness over performance**: checkpoints are full JSON snapshots written atomically via write-temp-then-rename (GUARD-4). Incremental/delta checkpoints (GOAL-6.6) are a P2 extension layered on top of the base mechanism. Memory management for the evaluation cache uses LRU eviction with front-member pinning to bound growth without invalidating active dominance relationships.

**Addresses:** GOAL-6.1 through GOAL-6.6, GUARD-2, GUARD-4, GUARD-8, GUARD-9

## 2. Components

### 2.1 EngineState Struct

**Responsibility:** Aggregate all engine state into a single serializable checkpoint unit.

**Interface:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineState {
    pub schema_version: u32,
    pub iteration: u64,
    pub pareto_front: ParetoFront,
    pub candidates: CandidateStore,
    pub eval_cache: EvaluationCache,
    pub next_candidate_id: u64,
    pub rng_state: RngState,
    pub proposer_state: ProposerState,
    pub statistics: GEPAStatistics,
    pub config_snapshot: GEPAConfig,
    pub stagnation_counter: u64,
    pub consecutive_skips: u64,
}

/// Serializable RNG state — captures the full internal state of StdRng.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RngState(pub Vec<u8>);

impl RngState {
    pub fn capture(rng: &StdRng) -> Self {
        let bytes = bincode::serialize(rng)
            .expect("StdRng serialization is infallible");
        Self(bytes)
    }

    pub fn restore(&self) -> Result<StdRng, GEPAError> {
        bincode::deserialize(&self.0).map_err(|e| GEPAError::CheckpointCorrupt {
            message: format!("failed to restore RNG state: {}", e),
        })
    }
}

/// Serializable proposer state for checkpoint/resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposerState {
    pub selection_counts: HashMap<u64, u64>,
    pub round_robin_cursor: usize,
}
```

**Key Details:**
- `schema_version: u32` starts at 1. On deserialization, if version != current, return `GEPAError::CheckpointCorrupt`. Forward compatibility strategy: future versions bump the version and add migration logic or reject incompatible checkpoints. V1 does not support cross-version resume (per GOAL-5.6 scope).
- `CandidateStore` is `HashMap<u64, Candidate>` — all candidates ever created, never pruned (design-05 §2.3).
- `ParetoFront` is serialized as its member ID list + any internal state (design-02).
- `rng_state` captures the full `StdRng` state via `bincode` serialization. This ensures that resumed runs produce identical sequences (GUARD-9). `StdRng` from `rand` crate implements `Serialize/Deserialize` via the `serde1` feature.
- `config_snapshot` stores the config used for this run. On resume, the engine can warn if the provided config differs from the snapshot (informational, not enforced in v1).
- `stagnation_counter` and `consecutive_skips` are engine loop counters needed for correct resume of termination condition tracking.

**Satisfies:** GOAL-6.1, GOAL-1.9, GUARD-9

### 2.2 Evaluation Cache

**Responsibility:** Map `(candidate_id, example_id) → f64` scores with LRU eviction and front-member pinning.

**Interface:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationCache {
    entries: HashMap<(u64, u64), CacheEntry>,
    hit_count: u64,
    miss_count: u64,
    access_counter: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry { score: f64, last_accessed: u64 }

impl EvaluationCache {
    pub fn new() -> Self;
    pub fn get(&mut self, candidate_id: u64, example_id: u64) -> Option<f64>;
    pub fn insert(&mut self, candidate_id: u64, example_id: u64, score: f64);
    pub fn evict_lru(&mut self, max_size: usize, pinned_ids: &HashSet<u64>);
    pub fn scores_for_candidate(&self, candidate_id: u64) -> Vec<(u64, f64)>;
    pub fn peek(&self, candidate_id: u64, example_id: u64) -> Option<f64>;
    pub fn len(&self) -> usize;
    pub fn hit_rate(&self) -> f64;
}
```

**Key Details:**
- **LRU tracking:** `access_counter` is a monotonic u64 (not wall-clock — deterministic per GUARD-9). Incremented on every `get()` hit and `insert()`. `peek()` does NOT update the counter — used for dominance comparisons.
- **Eviction (GOAL-6.4):** When `config.eval_cache_max_size` is `Some(limit)` and `len() > limit`, `evict_lru()` sorts entries by `last_accessed` ascending, skips entries whose `candidate_id` is in `pinned_ids` (front members), and removes oldest until at capacity. If all entries are pinned, limit is soft-exceeded with a warning event.
- **Score sanitization (GUARD-10):** `insert()` discards `NaN` (treated as unevaluated). `+Inf`/`-Inf` clamped to `f64::MAX`/`f64::MIN` with warning.
- **Memory:** ~24 bytes/entry. At 100K entries ≈ 2.4MB (GUARD-7).

**Satisfies:** GOAL-6.3, GOAL-6.4, GUARD-7, GUARD-9, GUARD-10

### 2.3 Checkpoint Format

**Responsibility:** Define the on-disk JSON format for checkpoints.

**Key Details:**

The checkpoint is a single JSON file containing the serialized `EngineState`. Example structure:

```json
{
  "schema_version": 1,
  "iteration": 42,
  "pareto_front": { "member_ids": [0, 3, 7] },
  "candidates": { "0": { "id": 0, "params": {"system_prompt": "..."}, ... } },
  "eval_cache": { "entries": { "(0,1)": { "score": 0.85, "last_accessed": 120 } } },
  "next_candidate_id": 43,
  "rng_state": "/* bincode bytes base64-encoded */",
  "statistics": { "..." }
}
```

- `schema_version` at top level. Unknown versions → `GEPAError::CheckpointCorrupt`.
- **Deterministic JSON (GOAL-6.1):** HashMap keys sorted during serialization (via `BTreeMap` conversion). Bitwise-identical output on re-serialization.
- Tuple keys `(u64, u64)` serialized as string keys for JSON compatibility.

**Satisfies:** GOAL-6.1, GOAL-6.6 (format foundation)

### 2.4 Checkpoint Save/Restore

**Responsibility:** Atomically persist and recover engine state.

**Interface:**
```rust
impl EngineState {
    pub fn save(&self, path: &Path) -> Result<(), GEPAError>;
    pub fn load(path: &Path) -> Result<Self, GEPAError>;
}

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
```

**Key Details:**
- **Atomic write (GUARD-4):** `save()` serializes to JSON, writes to `.gepa-checkpoint-{pid}.tmp` in the same directory (same filesystem), then `fs::rename`. Rename is atomic on POSIX. Crash during write leaves previous checkpoint intact.
- **Save frequency:** Called every `config.checkpoint_interval` iterations (default: 1, per GOAL-6.2). Each save overwrites the single checkpoint file.
- **Failed save (GOAL-6.2):** Returns `Err`, engine emits `CheckpointSaved { success: false }` event and continues — no halt.
- **Load:** Reads file, deserializes, validates `schema_version == CURRENT_SCHEMA_VERSION`. Mismatches return `GEPAError::CheckpointCorrupt`.

**Satisfies:** GOAL-6.2, GUARD-4

### 2.5 Run Statistics

**Responsibility:** Track aggregate metrics about the optimization run.

**Interface:**
```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GEPAStatistics {
    pub total_iterations: u64,
    pub skipped_iterations: u64,
    pub adapter_calls_execute: u64,
    pub adapter_calls_reflect: u64,
    pub adapter_calls_mutate: u64,
    pub adapter_calls_evaluate: u64,
    pub adapter_calls_merge: u64,
    pub candidates_generated: u64,
    pub candidates_accepted: u64,
    pub best_score_history: Vec<f64>,
    pub front_size_history: Vec<u64>,
    pub cache_hit_rate: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum AdapterCallType { Execute, Reflect, Mutate, Evaluate, Merge }

impl GEPAStatistics {
    pub fn new() -> Self;
    pub fn acceptance_rate(&self) -> f64;
    pub fn record_iteration(&mut self, accepted: bool, best_score: f64, front_size: u64, cache_hit_rate: f64);
    pub fn record_skip(&mut self);
    pub fn record_adapter_call(&mut self, call_type: AdapterCallType);
}
```

**Key Details:**
- `best_score_history` records the best average score across all front members at the end of each iteration. "Best" = highest average score across all evaluated examples for a single candidate (GUARD-10 higher-is-better).
- `front_size_history` records `pareto_front.len()` at the end of each iteration.
- `cache_hit_rate` is updated from `EvaluationCache::hit_rate()` at each iteration end.
- All counters are `u64`, rates are `f64` (GOAL-6.5).
- Statistics are serialized as part of `EngineState` and survive checkpoint/resume.
- `record_iteration` is called by the engine after the accept/reject step; `record_skip` on adapter failure.

**Satisfies:** GOAL-6.5

### 2.6 Resume from Checkpoint

**Responsibility:** Restore engine state from a checkpoint file and continue optimization.

**Key Details:**

The resume flow is:

1. `EngineState::load(path)` deserializes the checkpoint (§2.4)
2. Schema version is validated
3. Engine restores internal state:
   - `CandidateIdGenerator::resume(state.next_candidate_id)` — continue ID sequence
   - `state.rng_state.restore()` → `StdRng` — exact RNG state restored (GUARD-9)
   - `state.pareto_front` → engine's front (with all member IDs)
   - `state.candidates` → `CandidateStore`
   - `state.eval_cache` → `EvaluationCache` (including LRU timestamps)
   - `state.proposer_state` → `MutationProposer` selection counts and cursor
   - `state.statistics` → accumulated stats (resumed run appends to history vectors)
   - `state.stagnation_counter`, `state.consecutive_skips` → loop counters
4. Engine validates that `state.config_snapshot` is compatible with the provided config (warning if different, not error)
5. `engine.run()` begins at `iteration = state.iteration + 1`

**Determinism guarantee (GUARD-9):** Because the RNG state, all candidate data, cache entries, and proposer state are fully restored, a resumed run with the same adapter responses produces identical results to a run that never paused.

**Satisfies:** GOAL-1.9, GOAL-6.1, GUARD-9

## 3. Data Integrity

- **Atomic writes (GUARD-4):** Checkpoint is written to a temp file then renamed. A crash at any point leaves either the old checkpoint intact or the new one fully written.
- **Deterministic serialization (GUARD-9):** `HashMap` keys are sorted during serialization (via `BTreeMap` conversion or `serde` key ordering). Two identical `EngineState` values produce bitwise-identical JSON.
- **No partial state:** The entire `EngineState` is serialized as one unit. There are no partial writes or multi-file checkpoints in the base mechanism.
- **Validation on load:** Schema version check, deserialization errors, and RNG restore errors all produce `GEPAError::CheckpointCorrupt` with a descriptive message.

## 4. Integration Points

- **Core Engine (design-01):** The engine creates and mutates `EngineState` throughout the loop. Calls `state.save()` every N iterations. On resume, constructs engine from loaded state.
- **Candidates (design-05):** `CandidateStore` and `CandidateIdGenerator` are owned by `EngineState`.
- **Pareto Front (design-02):** `ParetoFront` is a field of `EngineState`, serialized and restored together.
- **Proposers (design-04):** `ProposerState` captures `MutationProposer`'s selection tracking for resume.
- **Config (design-07):** `GEPAConfig` snapshot is stored for reproducibility.
- **Events (design-09):** `CheckpointSaved { success: bool, path: PathBuf, error: Option<String> }` event emitted after each save attempt.

**Guard compliance:**
| Guard | How Addressed |
|-------|--------------|
| GUARD-2 | State serialization is read-only snapshot; no mutation of candidates |
| GUARD-4 | Atomic write via temp file + rename |
| GUARD-8 | All types derive Debug |
| GUARD-9 | RNG state captured/restored; deterministic serialization order |
