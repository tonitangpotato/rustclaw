# Design: GEPA Pareto Front

## 1. Overview

The Pareto front maintains the set of non-dominated candidates — those that are best on at least one subset of training examples. It provides three core operations: dominance-based insertion/pruning, diversity-aware selection for mutation, and crowding-distance-based capacity management.

The design uses a `Vec<CandidateId>`-based front backed by the external evaluation cache for score lookups. Dominance is computed lazily over the intersection of evaluated examples between candidate pairs, with a `min_shared_examples` threshold to prevent premature dominance conclusions from sparse data. Crowding distance from NSGA-II handles pruning at capacity. Selection uses round-robin with overfitting-delta reordering to balance exploration across front members.

**Satisfies:** GOAL-2.1 through GOAL-2.6. **Applicable GUARDs:** GUARD-1 (front invariant), GUARD-2 (candidate immutability), GUARD-7 (memory), GUARD-9 (determinism), GUARD-10 (score semantics).

## 2. Components

### 2.1 ParetoFront Struct

**Responsibility:** Store the set of non-dominated candidate IDs and provide indexed access for selection, insertion, and pruning.

**Interface:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParetoFront {
    members: Vec<CandidateId>,
    max_size: usize,
    min_shared_examples: usize,
    selection_cursor: usize,
}

pub type CandidateId = u64;

impl ParetoFront {
    pub fn new(max_size: usize, min_shared_examples: usize) -> Self;
    pub fn members(&self) -> &[CandidateId];
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, id: CandidateId) -> bool;
}
```

**Key Details:**

- `members` is a `Vec<CandidateId>`, not full `Candidate` structs — keeps memory lightweight (GUARD-7). Full candidate data lives in `GEPAState::candidates: HashMap<CandidateId, Candidate>`.
- `max_size` from `config.pareto_max_size` (default 50, GOAL-7.2). `min_shared_examples` from `config.min_shared_examples` (GOAL-7.1).
- `selection_cursor` tracks round-robin position for starvation prevention (GOAL-2.3). Serialized for checkpoint/resume.
- Implements `Serialize + Deserialize` for checkpoint support (GOAL-2.6). Round-trip preserves member order, cursor position, and all config.
- `debug_assert!` after every mutation verifies no member is dominated by another (GUARD-1).

**Satisfies:** GOAL-2.1, GOAL-2.6

### 2.2 Dominance Checking and Front Updates

**Responsibility:** Determine pairwise dominance between candidates using per-example scores from the evaluation cache, and maintain front validity on insertion and re-evaluation.

**Interface:**

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum DominanceResult {
    ADominatesB,
    BDominatesA,
    NonDominating,
    InsufficientData,
}

impl ParetoFront {
    pub fn check_dominance(
        a: CandidateId,
        b: CandidateId,
        cache: &EvalCache,
        min_shared: usize,
    ) -> DominanceResult;

    pub fn try_insert(
        &mut self,
        candidate: CandidateId,
        cache: &EvalCache,
    ) -> bool;

    pub fn recompute(&mut self, cache: &EvalCache);
}
```

**Dominance algorithm (`check_dominance`):**

1. Retrieve evaluated example IDs for both candidates from `cache`: `examples_a = cache.examples_for(a)`, `examples_b = cache.examples_for(b)`.
2. Compute intersection via sorted merge in O(M) where M = max(|examples_a|, |examples_b|). Both example ID lists are maintained sorted in the cache.
3. If `intersection.len() < min_shared`, return `InsufficientData` (GOAL-2.1 — treated as non-dominating).
4. Iterate intersection. Track: `a_geq_b = true`, `b_geq_a = true`, `a_strict = false`, `b_strict = false`. For each shared example: compare `score_a` vs `score_b`. If `score_a > score_b`: `b_geq_a = false`, `a_strict = true`. If `score_b > score_a`: `a_geq_b = false`, `b_strict = true`. If both flags go false, early-exit with `NonDominating`.
5. Return: `a_geq_b && a_strict` → `ADominatesB`; `b_geq_a && b_strict` → `BDominatesA`; else `NonDominating`.

**Incremental insertion (`try_insert`):**

1. Check if `candidate` is dominated by any current member. If yes, reject (return `false`).
2. Remove any current members dominated by `candidate`.
3. Add `candidate` to `members`.
4. If `members.len() > max_size`, trigger pruning (§2.4).
5. `debug_assert!` front invariant (GUARD-1).
6. Return `true`.

**Note on front invariant:** The invariant that "no member dominates another" holds *modulo* `min_shared_examples` gaps. Two members may have insufficient shared data to establish dominance between them, while having sufficient shared data with a new candidate. This is by design — the `min_shared_examples` threshold prevents premature dominance conclusions. The invariant is fully restored after `recompute()` runs with updated cache data (GOAL-8.5b).

**Full recomputation (`recompute`) — called after re-evaluation backfill (GOAL-8.5b):**

1. Perform all-pairs dominance check on current `members` with updated cache scores.
2. Build a `dominated: HashSet<CandidateId>` — any member dominated by another member.
3. Remove all dominated members: `members.retain(|id| !dominated.contains(id))`.
4. Clamp `selection_cursor` to new `members.len()`.
5. `debug_assert!` front invariant (GUARD-1).

Complexity: O(N² · M) for N front members, M intersection size (GOAL-2.5). For N=100, M=200, this is ~4M comparisons — negligible vs adapter call time (GUARD-6).

**Satisfies:** GOAL-2.1, GOAL-2.2, GOAL-2.5

### 2.3 Selection Strategy

**Responsibility:** Select a front member for the next mutation, balancing diversity and deprioritizing overfitting.

**Interface:**

```rust
impl ParetoFront {
    pub fn select(
        &mut self,
        cache: &EvalCache,
        overfitting_deltas: &HashMap<CandidateId, f64>,
        rng: &mut ChaCha8Rng,
    ) -> Result<CandidateId, GEPAError>;
}

/// Selection method used, reported in events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionMethod {
    /// Round-robin with overfitting-delta reordering (default mutation selection)
    RoundRobinOverfitting,
    /// Complementary pair selection for merge iterations
    ComplementaryPair,
    /// Random selection (fallback)
    Random,
}
```

**Algorithm:**

1. If `members.is_empty()`, return `Err(GEPAError::EmptyFrontError)`.
2. If `members.len() == 1`, return the single member (boundary condition, GOAL-2.3).
3. **Round-robin floor** (starvation prevention): advance `selection_cursor` by 1 (wrapping). This guarantees every member is selected at least once every `members.len()` iterations (GOAL-2.3).
4. **Overfitting-aware reordering within the round:** the overfitting-delta sort order is computed once at the start of each full round-robin cycle (when cursor wraps to 0) and held constant for that cycle. Within this fixed ordering, members are sorted by ascending overfitting delta (lower delta = selected earlier). Ties broken by candidate age (newer first) then by candidate ID for full determinism (GUARD-9). This ensures every member is visited exactly once per cycle regardless of delta changes mid-cycle.
5. The candidate at the reordered position `selection_cursor % members.len()` is returned.

**Key Details:**

- `overfitting_deltas` is computed externally (Feature 01 §2.2, step 11) after re-evaluation backfill. Between backfill rounds, the deltas remain stale — this is acceptable since the round-robin floor guarantees no member is starved.
- If `overfitting_deltas` is empty (no re-evaluation yet), selection degrades to pure round-robin — still correct.
- Selection never removes candidates from the front (GOAL-2.3 — "MUST NOT remove candidates based on re-evaluation alone"). Only `try_insert` and `recompute` modify membership.
- `rng` is accepted for future use (e.g., random tie-breaking) but current selection is fully deterministic given the cursor state and overfitting deltas. The parameter ensures the API doesn't need to change if randomness is added later (GUARD-9).

**Satisfies:** GOAL-2.3, GOAL-1.3

### 2.4 Pruning via Crowding Distance

**Responsibility:** Remove the least-contributing member when the front exceeds `max_size`, using NSGA-II crowding distance.

**Interface:**

```rust
impl ParetoFront {
    fn prune_to_capacity(&mut self, cache: &EvalCache);
}

pub fn compute_crowding_distances(
    members: &[CandidateId],
    cache: &EvalCache,
) -> Vec<(CandidateId, f64)>;
```

**Crowding distance algorithm:**

1. Identify the set of example IDs (dimensions) to use: ideally the intersection of examples all front members have been evaluated on. If no universal intersection exists, fall back to ranking examples by the number of front members that have been evaluated on them (descending). Use the top-K examples where K = `min_shared_examples` or all examples with coverage ≥ 2 members, whichever is larger. Members without scores on the selected examples are excluded from crowding distance computation for those dimensions (GOAL-2.4).
2. For each dimension (example ID), sort members by their score on that example.
3. Boundary members (best and worst per dimension) receive `f64::INFINITY` distance.
4. Interior members accumulate: `distance[i] += (score[i+1] - score[i-1]) / (max_score - min_score)` for each dimension. If `max_score == min_score` for a dimension (all members tied), skip that dimension (contributes 0).
5. After all dimensions, the member with the smallest crowding distance is pruned. Ties broken by candidate age — oldest removed first (GOAL-2.4). Secondary tie-break: lowest candidate ID (GUARD-9 determinism).
6. Repeat until `members.len() <= max_size`.

**Complexity:** O(N · M · log M) per prune cycle — M dimensions, N members, sorting per dimension (GOAL-2.5). Typically only 1 member pruned per insertion, so this runs once.

**Known limitation:** At high M (>50 examples as dimensions), crowding distances converge and the age tie-breaker dominates. Acceptable for v1 per GOAL-2.4 rationale.

**Satisfies:** GOAL-2.4

### 2.5 Front Statistics

**Responsibility:** Expose metrics about the front for the event system and run statistics.

**Interface:**

```rust
impl ParetoFront {
    pub fn front_size(&self) -> usize;
    pub fn best_scores(&self, cache: &EvalCache) -> HashMap<String, f64>;
    pub fn mean_scores(&self, cache: &EvalCache) -> HashMap<String, f64>;
    pub fn age_distribution(&self, candidates: &HashMap<CandidateId, Candidate>) -> FrontAgeStats;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontAgeStats {
    pub min_generation: u64,
    pub max_generation: u64,
    pub mean_generation: f64,
}
```

**Key Details:**

- `best_scores()` computes per-example max score across all front members. Returns the best score found for each example ID that any front member has been evaluated on.
- `mean_scores()` computes per-example mean score across front members that have been evaluated on that example.
- These methods read from the evaluation cache (GOAL-6.3), never from candidate structs directly (candidates don't store scores, GOAL-5.2).
- Statistics are computed on-demand, not cached — front changes are infrequent relative to adapter calls (GUARD-6).
- All statistics are serializable for inclusion in `GEPAState::stats` (GOAL-6.5).

**Satisfies:** GOAL-2.5 (performance characteristic), GOAL-6.5 (statistics contribution)

## 3. Algorithmic Complexity Analysis

| Operation | Complexity | Typical Values | Wall-clock Estimate |
|---|---|---|---|
| `check_dominance` (one pair) | O(M) sorted merge | M ≤ 200 | < 1μs |
| `try_insert` (one candidate) | O(N · M) | N ≤ 50, M ≤ 200 | < 50μs |
| `recompute` (full front) | O(N² · M) | N ≤ 100, M ≤ 200 | < 10ms |
| `compute_crowding_distances` | O(N · M · log M) | N ≤ 50, M ≤ 200 | < 5ms |
| `select` | O(N log N) sorting | N ≤ 50 | < 10μs |

All operations are dominated by adapter call time (typically 1-30 seconds), satisfying GUARD-6 (< 5% overhead). Memory for the front itself is O(N) candidate IDs — the `Vec<u64>` for 50 members is 400 bytes. Score data lives in the shared evaluation cache (GUARD-7).

## 4. Integration Points

| This Feature | Depends On | Interface |
|---|---|---|
| §2.2 `check_dominance` | Feature 06 (State) GOAL-6.3 | `EvalCache::scores_for_candidate(candidate_id) -> Vec<(u64, f64)>` |
| §2.2 `try_insert` | Feature 06 (State) GOAL-6.3 | `EvalCache::examples_for(candidate_id) -> Vec<u64>` |
| §2.3 `select` | Feature 01 (Engine) §2.2 | Called at step 3 of each iteration |
| §2.4 `prune_to_capacity` | Feature 07 (Config) GOAL-7.2 | `config.pareto_max_size` |
| §2.2 `recompute` | Feature 08 (Data Loading) GOAL-8.5b | Triggered after re-evaluation backfill |
| §2.3 overfitting deltas | Feature 08 (Data Loading) GOAL-8.5c | `HashMap<CandidateId, f64>` computed by engine |
