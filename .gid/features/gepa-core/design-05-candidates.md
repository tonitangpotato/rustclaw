# Design: GEPA Candidate Management

## 1. Overview

Candidates are the central data type in GEPA — immutable text artifacts that evolve through mutation and merge. This design covers the `Candidate` struct (generic-free, using `HashMap<String, String>` for text parameters), deterministic ID generation, lineage tracking, and lifecycle management.

The key design decision is **candidates are lightweight and immutable** (GOAL-5.3): scores live in the evaluation cache (design-06 §2.2), not on the candidate. This keeps candidates cheap to clone, serialize, and pass around. Lineage is tracked via `parent_id` back-pointers in a flat `CandidateStore` map rather than a tree data structure, which simplifies serialization and avoids recursive ownership.

**Addresses:** GOAL-5.1 through GOAL-5.7, GUARD-2, GUARD-7, GUARD-8, GUARD-9

## 2. Components

### 2.1 Candidate Struct

**Responsibility:** Hold the immutable text parameters and metadata for a single evolving prompt candidate.

**Interface:**
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u64,
    pub params: HashMap<String, String>,
    pub parent_id: Option<u64>,
    pub merge_parent_id: Option<u64>,
    pub generation: u32,
    pub created_at: DateTime<Utc>,
    pub reflection: Option<String>,
}

impl Candidate {
    pub fn new(id: u64, params: HashMap<String, String>, parent_id: Option<u64>,
               merge_parent_id: Option<u64>, generation: u32,
               reflection: Option<String>) -> Result<Self, GEPAError>;
    pub fn seed(id: u64, params: HashMap<String, String>) -> Result<Self, GEPAError>;
    pub fn is_seed(&self) -> bool;
}
```

**Key Details:**
- `params`: named text parameters (e.g., "system_prompt"). Values may be empty (placeholder for mutation). Keys must be non-empty. `new()` validates both constraints, returning `GEPAError::InvalidCandidate` on violation.
- `merge_parent_id`: secondary parent for merge candidates; `None` for seeds and single-parent mutations.
- `reflection`: natural-language output from the adapter's `reflect` method. `None` for seeds. Reflection consistency (seeds must have `None`, non-seeds should have `Some(...)`) is the caller's responsibility — the `seed()` convenience method sets it to `None` automatically.
- `generation`: 0 for seeds, `parent.generation + 1` for mutations, `max(a, b) + 1` for merges.
- `created_at`: set at construction. Used for age-based tie-breaking in Pareto pruning (GOAL-2.4).
- No scores on the struct — scores live in `EvaluationCache` keyed by `(candidate_id, example_id)` per GOAL-5.2.
- `Send + Sync` automatically (all fields are). `PartialEq/Eq` compare all fields including `id` (GUARD-9).

**Satisfies:** GOAL-5.2, GOAL-5.3, GOAL-5.7, GUARD-2, GUARD-7, GUARD-8

### 2.2 CandidateId Generation

**Responsibility:** Generate unique, deterministic candidate IDs within a run.

**Interface:**
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CandidateIdGenerator {
    next_id: u64,
}

impl CandidateIdGenerator {
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    /// Resume from a checkpoint — set next_id to continue from where we left off.
    pub fn resume(next_id: u64) -> Self {
        Self { next_id }
    }

    /// Allocate the next candidate ID. Monotonically increasing.
    pub fn next(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}
```

**Key Details:**
- IDs are `u64` values starting at 0, monotonically increasing per run (GOAL-5.4)
- Seed candidates receive IDs 0..N-1 in the order provided to `GEPAEngine::new()`
- Subsequent candidates receive the next available ID from the generator
- The generator is included in `EngineState` for checkpoint/resume (design-06 §2.1). On resume, `CandidateIdGenerator::resume(state.next_candidate_id)` ensures no ID collisions
- No randomness involved — deterministic by construction (GUARD-9)
- Even if two candidates have identical text parameters, they get different IDs (GOAL-5.4)

**Satisfies:** GOAL-5.4, GUARD-9

### 2.3 Candidate Metadata and Lineage

**Responsibility:** Track parent→child relationships and provide lineage traversal.

**Interface:**
```rust
pub type CandidateStore = HashMap<u64, Candidate>;

/// Internal implementation — the public API `EngineState::lineage(candidate_id)` (design-06) delegates to this function.
pub fn lineage(candidate_id: u64, store: &CandidateStore) -> Result<Vec<u64>, GEPAError>;

pub fn lesson_chain(candidate_id: u64, store: &CandidateStore, max_depth: usize) -> Result<Vec<String>, GEPAError>;
```

**Key Details:**
- `lineage()` walks `parent_id` back-pointers from the given candidate to its seed root, returning IDs ordered [candidate → ... → seed]. Returns `GEPAError::CandidateNotFound` if any ID is missing. O(D) where D is generation depth (≤1000 typical). **Design decision:** `lineage()` follows the primary-parent chain only (`parent_id`). For merge candidates, the secondary parent (`merge_parent_id`) is not traversed. This keeps lineage as a simple linear chain. Callers that need full merge provenance can inspect `merge_parent_id` directly and call `lineage()` on the secondary parent separately.
- `lesson_chain()` calls `lineage()` then collects `reflection` strings (skipping `None` for seeds), truncated to `max_depth` per GOAL-4.2b.
- For merged candidates, `parent_id` records the primary parent. A separate `merge_parent_id: Option<u64>` field on `Candidate` records the secondary parent. Lesson chain follows only `parent_id`.
- The `CandidateStore` is never pruned — all candidates retained for lineage + stats. Memory: 10K × ~1KB ≈ 10MB (GUARD-7).

**Satisfies:** GOAL-5.5, GOAL-4.2 (cross-ref), GOAL-4.2b (cross-ref), GUARD-7

### 2.4 Candidate Lifecycle

**Responsibility:** Define the state transitions a candidate goes through from creation to final disposition.

**Key Details:**

Candidates don't carry explicit state enums — their lifecycle is implicit in where they appear:

```
Proposed → Evaluated → Accepted/Rejected
                          ↓ (if accepted)
                       On Front → (possibly pruned by crowding distance)
```

- **Proposed:** Candidate created by `MutationProposer` or `MergeProposer` (§2.1 constructor). Inserted into `CandidateStore` immediately with a unique ID. No scores yet.
- **Evaluated:** Engine calls `adapter.evaluate()`, scores stored in `EvaluationCache` keyed by `(candidate_id, example_id)`. The candidate struct itself is unchanged (immutable, GOAL-5.3).
- **Accepted:** Candidate passes dominance check (GOAL-1.7d) and is added to `ParetoFront`. Dominated front members are removed from the front (but remain in `CandidateStore`).
- **Rejected:** Candidate fails dominance check. Remains in `CandidateStore` for lineage but is not on the front. Its cached scores remain available for future dominance comparisons.
- **Pruned:** A front member removed by crowding distance (GOAL-2.4) when the front exceeds `pareto_max_size`. Leaves the front but stays in `CandidateStore`.

No explicit `CandidateStatus` enum is needed. A candidate's current status can be derived:
```rust
/// Determine if a candidate is currently on the Pareto front.
pub fn is_on_front(candidate_id: u64, front: &ParetoFront) -> bool {
    front.members().contains(&candidate_id)
}
```

**Satisfies:** GOAL-5.3, GUARD-2

### 2.5 Seed Validation

**Responsibility:** Validate seed candidates at engine construction time.

**Interface:**
```rust
pub fn validate_seeds(seeds: &[Candidate]) -> Result<(), GEPAError>;
```

**Key Details:**
- All seeds must have at least one text parameter (enforced by `Candidate::new()` per §2.1)
- All seeds must share the same set of parameter keys (GOAL-5.1). This ensures the mutation/merge operations always operate on a consistent parameter set.
- Uses `BTreeSet` for deterministic key ordering in error messages
- Called during `GEPAEngine::new()`, not during `run()` — this is construction-time validation

**Satisfies:** GOAL-5.1

## 3. Serialization Design

Candidates use `#[derive(Serialize, Deserialize)]` via serde. The serialized format is JSON.

```rust
// Round-trip guarantee (GOAL-5.6):
let json = serde_json::to_string(&candidate)?;
let restored: Candidate = serde_json::from_str(&json)?;
assert_eq!(candidate, restored);
```

**Key Details:**
- `HashMap<String, String>` serializes as a JSON object. Key ordering in JSON is not guaranteed, but `PartialEq` on `HashMap` compares by content, not order — round-trip equality holds.
- `DateTime<Utc>` serializes as an ISO 8601 string via `chrono::serde`
- `CandidateStore` (`HashMap<u64, Candidate>`) serializes as a JSON object with string-encoded u64 keys (serde's default for integer map keys)
- `CandidateIdGenerator` serializes its `next_id: u64` field — sufficient for resume
- Cross-version checkpoint compatibility is explicitly out of scope for v1 (GOAL-5.6)

**Satisfies:** GOAL-5.6, GOAL-5.7

## 4. Integration Points

- **Proposers (design-04):** Proposers create candidates via `Candidate::new()` and traverse lineage via `lesson_chain()`. MutationProposer reads `CandidateStore` for lesson chain construction.
- **EvaluationCache (design-06 §2.2):** Scores are stored externally, keyed by `(candidate_id, example_id)`. Candidates never hold their own scores.
- **ParetoFront (design-02):** The front holds `Vec<u64>` of candidate IDs, not `Candidate` objects. Front operations look up candidates from `CandidateStore` as needed.
- **EngineState (design-06 §2.1):** `CandidateStore` and `CandidateIdGenerator` are serialized as part of the checkpoint.
- **Adapter (design-03):** Adapter methods receive `&Candidate` references. The adapter reads `params` for LLM prompt construction and may read metadata for context.

**Guard compliance:**
| Guard | How Addressed |
|-------|--------------|
| GUARD-2 | `Candidate` fields are pub but construction is via `new()`; no `&mut` methods exist post-creation |
| GUARD-7 | No scores on struct; 10K candidates × ~1KB ≈ 10MB, well under 50MB |
| GUARD-8 | `#[derive(Debug)]` on all types |
| GUARD-9 | IDs from monotonic counter; no randomness in candidate construction |
