# Review: GEPA Core Engine & Pareto Front Design Documents

**Documents reviewed:**
- `design-01-core-engine.md` (Core Engine)
- `design-02-pareto-front.md` (Pareto Front)

**Review date:** 2026-04-06
**Reviewer:** Claude (Subagent)

---

## 🔴 Critical (blocks implementation)

### FINDING-1-1: **[Check #1] Incomplete type definition for IterationMetrics**

**Document:** design-01-core-engine.md (§3.6)

`IterationMetrics` references a `phase_durations` field but doesn't define its type. The struct shows:

```rust
pub struct IterationMetrics {
    pub iteration: usize,
    pub phase_durations: ?,  // ← UNDEFINED
    pub total_duration: Duration,
    ...
}
```

The pseudocode in §4.3 uses:
```rust
metrics.phase_durations.insert("select", select_duration);
```

This implies `phase_durations` is a `HashMap<&str, Duration>` or similar, but the exact type is not specified. Two engineers would implement this differently (one might use `HashMap<String, Duration>`, another `HashMap<&'static str, Duration>`, another `Vec<(String, Duration)>`).

**Suggested fix:**
```rust
pub struct IterationMetrics {
    pub iteration: usize,
    pub phase_durations: HashMap<&'static str, Duration>,  // Fixed set of phase names
    pub total_duration: Duration,
    pub minibatch_size: usize,
    pub selected_candidate_id: Option<CandidateId>,
    pub mutation_accepted: bool,
    pub front_size: usize,
    pub cumulative_adapter_calls: u64,
}
```

---

### FINDING-1-2: **[Check #1] Missing type definition for TerminationContext**

**Document:** design-01-core-engine.md (§3.7)

`TerminationContext` is referenced in §4.5 pseudocode:
```rust
let term_ctx = TerminationContext { ... };
```

But §3.7 only shows:
```rust
pub enum TerminationReason { ... }
```

There's no definition of what `TerminationContext` contains. From context it should include things like `iteration`, `stagnation_count`, `consecutive_skips`, etc., but these fields are never explicitly defined.

**Suggested fix:** Add to §3.7:
```rust
pub struct TerminationContext {
    pub iteration: usize,
    pub stagnation_count: usize,
    pub consecutive_skips: usize,
    pub elapsed: Duration,
    pub total_accepted: usize,
}
```

---

### FINDING-2-1: **[Check #5] State machine deadlock in Pareto front when empty**

**Document:** design-02-pareto-front.md (§2.2, §4.1)

The `select()` operation (§2.2) says:
> "If the front is empty, return `Err(GEPAError::EmptyFrontError)`."

But there's no mechanism to prevent or recover from an empty front during the optimization loop. Consider this scenario:

1. Engine starts with 1 seed candidate
2. Re-evaluation backfill (GOAL-8.5) adds scores
3. Full front recomputation (§4.3) runs
4. The seed is found to be dominated by... nothing (impossible) OR corrupt data causes removal
5. Front becomes empty → next iteration `select()` returns `Err(EmptyFrontError)`
6. Engine loop has no path to recover

The design says in GOAL-2.3:
> "If the front is empty (should not occur given seed candidate requirement GOAL-5.1 + GOAL-1.12), the engine returns an error."

This is a logical deadlock: the front CAN become empty (via buggy recomputation or dominance logic), but if it does, the only option is to crash. There's no guard preventing this, no assertion maintaining the "front never empty after initialization" invariant.

**Suggested fix:**

1. Add `debug_assert!(!front.is_empty())` after every dominance recomputation (§4.3)
2. Document the invariant explicitly in §2.1: "Invariant: after engine initialization completes, `members.len() >= 1` always holds. Recomputation logic must preserve at least one non-dominated candidate."
3. Add a fallback in `full_recomputation`: if all candidates would be pruned, keep the newest candidate as a bootstrap

---

### FINDING-2-2: **[Check #6] Data flow breaks when front is empty in selection**

**Document:** design-02-pareto-front.md (§2.2)

The `select()` method returns `Result<CandidateId, GEPAError>` with `Err(EmptyFrontError)` when empty. But the engine's main loop (design-01, §4.3) doesn't handle this error case:

```rust
let parent_id = self.state.pareto_front.select(...)?;  // ← propagates error
```

The `?` operator would terminate the entire `run()` future with `EmptyFrontError`. But according to requirements GOAL-1.2d, termination reasons are: `MaxIterations`, `TimeBudget`, `Stagnation`, `TooManySkips`, `Cancelled`. There's no `EmptyFront` variant in `TerminationReason`.

This means an empty front would cause `run()` to return `Err(GEPAError::EmptyFrontError)` instead of `Ok(GEPAResult { reason: ... })`, which violates the result contract.

**Suggested fix:**

Either:
1. Add `EmptyFront` to `TerminationReason` and handle it explicitly in the loop
2. OR enforce "front never empty" as a hard invariant with recovery logic

Option 2 is safer. Add to design-01-core-engine.md §4.3:
```rust
let parent_id = match self.state.pareto_front.select(...) {
    Ok(id) => id,
    Err(GEPAError::EmptyFrontError) => {
        // Invariant violation: should never happen after initialization
        error!("Pareto front became empty during optimization");
        return Err(GEPAError::Internal {
            message: "front invariant violated: became empty mid-run".into()
        });
    }
    Err(e) => return Err(e),
};
```

---

### FINDING-2-3: **[Check #9] Integer overflow in selection cursor**

**Document:** design-02-pareto-front.md (§2.2)

The `select()` method increments `selection_cursor`:

```rust
let idx = self.selection_cursor % self.members.len();
self.selection_cursor = self.selection_cursor.wrapping_add(1);
```

The use of `wrapping_add(1)` prevents overflow panics, but after `usize::MAX` iterations (2^64 on 64-bit), the cursor wraps to 0. This changes the selection pattern mid-run, breaking determinism (GUARD-9).

While 2^64 iterations is astronomically large (~10^19 iterations), the design explicitly claims determinism across unlimited runs. The wrap-around introduces a non-determinism boundary.

**Suggested fix:**

Change §2.2 implementation notes:
```rust
// Increment with saturation instead of wrapping
self.selection_cursor = self.selection_cursor.saturating_add(1);
```

Saturation preserves the modulo pattern indefinitely (once saturated, `usize::MAX % N` is a fixed value). Add a note: "After `usize::MAX` iterations, selection stabilizes to the same candidate repeatedly — acceptable for an unreachable boundary."

---

## 🟡 Important (should fix before implementation)

### FINDING-1-3: **[Check #6] Stale data in phase_durations between iterations**

**Document:** design-01-core-engine.md (§4.3)

The `IterationMetrics` struct is reused across iterations:
```rust
let mut metrics = IterationMetrics { ... };
```

But `phase_durations` (once we define it as `HashMap<&'static str, Duration>`) is populated incrementally:
```rust
metrics.phase_durations.insert("select", select_duration);
metrics.phase_durations.insert("execute", execute_duration);
// ...
```

If an iteration skips a phase (e.g., mutation fails, so no "mutate" duration), the previous iteration's value persists in the map. Consumers of the metrics would see stale data.

**Suggested fix:**

Clear the map at the start of each iteration:
```rust
metrics.phase_durations.clear();
metrics.iteration = iteration;
metrics.total_duration = Duration::ZERO;
```

OR create a fresh `IterationMetrics` each iteration (safer):
```rust
let mut metrics = IterationMetrics::new(iteration);
```

---

### FINDING-1-4: **[Check #7] Missing error handling for adapter timeout during evaluate**

**Document:** design-01-core-engine.md (§4.3)

The evaluate phase wraps adapter calls in timeout:
```rust
let scores = timeout(config.adapter_timeout, adapter.evaluate(child, &batch)).await??;
```

But the pseudocode doesn't show what happens when `timeout` returns `Err(Elapsed)`. The double-`??` would unwrap the timeout result, then the adapter result. If the timeout fires, we get `Err(Elapsed)`, which is then... mapped to what?

Looking at §3.8, the error flow section mentions:
> "Adapter errors → engine retry policy"

But timeout is a separate case. The design doesn't specify whether timeout during `evaluate` should:
1. Retry the call
2. Skip the iteration
3. Halt the run

**Suggested fix:**

Add explicit timeout handling in §4.3:
```rust
let scores = match timeout(config.adapter_timeout, adapter.evaluate(child, &batch)).await {
    Ok(Ok(scores)) => scores,
    Ok(Err(e)) => {
        // Adapter error during evaluate
        warn!("Adapter evaluate failed: {}", e);
        self.handle_adapter_error(e, "evaluate")?;
        continue;  // Skip iteration
    }
    Err(_elapsed) => {
        warn!("Adapter evaluate timed out");
        self.state.consecutive_skips += 1;
        if self.state.consecutive_skips >= config.max_consecutive_skips {
            return Ok(GEPAResult {
                reason: TerminationReason::TooManySkips,
                ...
            });
        }
        continue;
    }
};
```

---

### FINDING-1-5: **[Check #22] Missing helper: `handle_adapter_error()` never defined**

**Document:** design-01-core-engine.md (§4.3, §4.4)

The pseudocode in §4.3 references:
```rust
self.handle_adapter_error(e, "execute")?;
```

And §4.4 mentions:
> "The retry policy is encapsulated in a helper method."

But no signature or specification for `handle_adapter_error()` appears anywhere. The method is used but never defined. Key questions:

- Does it return `Result<(), GEPAError>`?
- Does it mutate `self.state.consecutive_skips`?
- How does it decide between retry vs skip vs halt?
- Where does it store retry state?

**Suggested fix:**

Add to §3.x (new subsection "3.9 Adapter Error Handling"):

```rust
impl GEPAEngine {
    fn handle_adapter_error(&mut self, error: GEPAError, phase: &str) -> Result<(), GEPAError> {
        match error {
            GEPAError::Timeout | GEPAError::RateLimited { .. } => {
                warn!("Adapter {} timed out or rate-limited", phase);
                self.state.consecutive_skips += 1;
                if self.state.consecutive_skips >= self.config.max_consecutive_skips {
                    return Err(GEPAError::Internal {
                        message: format!("too many consecutive skips: {}", self.state.consecutive_skips)
                    });
                }
                Ok(())  // Skip this iteration
            }
            GEPAError::AdapterError { retryable: false, .. } => {
                // Non-retryable: halt immediately
                Err(error)
            }
            GEPAError::AdapterError { retryable: true, .. } => {
                // Retryable: log and skip (retry at iteration level, not call level)
                warn!("Retryable adapter error in {}: {}", phase, error);
                self.state.consecutive_skips += 1;
                Ok(())
            }
            _ => Err(error),  // Unknown error: propagate
        }
    }
}
```

---

### FINDING-1-6: **[Check #14] Coupling: IterationMetrics carries derived state**

**Document:** design-01-core-engine.md (§3.6)

`IterationMetrics` includes:
```rust
pub front_size: usize,
pub cumulative_adapter_calls: u64,
```

Both of these are derived from `GEPAState`:
- `front_size` = `state.pareto_front.len()`
- `cumulative_adapter_calls` = running counter in state

The metrics struct carries values that are already in state, creating coupling. If state is serialized, these values would be duplicated. The metrics are emitted via events, meaning every event payload duplicates this state data.

This violates the principle: "Events/actions carry only what they observed, not derived state."

**Suggested fix:**

Remove `front_size` and `cumulative_adapter_calls` from `IterationMetrics`. Event consumers that need these can read them from the state reference passed in the event context. If not passed, add a `state_snapshot` field to the event that carries the full state by reference:

```rust
pub enum GEPAEvent {
    IterationComplete {
        metrics: IterationMetrics,
        state: &GEPAState,  // Allow derived queries
    },
    ...
}
```

Alternatively, make metrics truly observational:
```rust
pub struct IterationMetrics {
    pub iteration: usize,
    pub phase_durations: HashMap<&'static str, Duration>,
    pub total_duration: Duration,
    pub minibatch_size: usize,
    pub selected_candidate_id: Option<CandidateId>,
    pub mutation_accepted: bool,
    // Removed: front_size, cumulative_adapter_calls
}
```

---

### FINDING-2-4: **[Check #6] Dominance computation reads scores that may not exist**

**Document:** design-02-pareto-front.md (§4.2)

The `dominates(a, b, cache)` function computes:

```rust
let shared_examples = intersection of a's evaluated examples and b's evaluated examples
if shared_examples.len() < min_shared_examples {
    return false;  // Insufficient data
}

for ex in shared_examples {
    let score_a = cache.get(a, ex)?;  // ← May return None
    let score_b = cache.get(b, ex)?;
    // ...
}
```

The design assumes that if an example is in the "evaluated examples" set for a candidate, then `cache.get(candidate, example)` will return `Some(score)`. But what if:

1. Re-evaluation backfill (GOAL-8.5) marks an example as needing re-eval due to NaN score
2. The example ID is removed from the "evaluated set"
3. But then recomputation runs before backfill completes

The intersection may include examples that no longer have scores in the cache.

**Suggested fix:**

Add explicit None-handling in §4.2:
```rust
for ex in &shared_examples {
    let score_a = match cache.get(a, ex) {
        Some(s) => s,
        None => {
            warn!("Missing score for candidate {} on example {} during dominance check", a, ex);
            return false;  // Treat as non-dominating if data is incomplete
        }
    };
    let score_b = match cache.get(b, ex) {
        Some(s) => s,
        None => {
            warn!("Missing score for candidate {} on example {} during dominance check", b, ex);
            return false;
        }
    };
    // ... comparison logic
}
```

---

### FINDING-2-5: **[Check #5] Crowding distance tie-breaking may fail when all distances are equal**

**Document:** design-02-pareto-front.md (§4.4)

The pruning logic says:
> "Ties in crowding distance are broken by candidate age (oldest removed first)."

But candidate age is not stored in `Candidate`, nor in `ParetoFront`. Looking at design-01 §3.2:

```rust
pub struct Candidate {
    pub id: CandidateId,
    pub parameters: HashMap<String, String>,
    pub parent_id: Option<CandidateId>,
    pub generation: usize,
    pub metadata: HashMap<String, String>,
}
```

There's no `created_at` or `age` field. The `generation` field exists, but generation is not age — two candidates in generation 5 have the same generation but different creation times.

If all candidates have identical crowding distances (can happen when M is large and distances converge, per the known limitation in GOAL-2.4), how is the tie broken? Without an age field, the implementation would have to use candidate ID (which is not time-ordered if generated from a hash).

**Suggested fix:**

Add to §4.4:
```rust
// Tie-breaking: use generation first, then candidate ID as final fallback
let min_candidate = candidates_with_distance
    .min_by_key(|(id, distance)| {
        let candidate = state.candidates.get(id).unwrap();
        (OrderedFloat(*distance), candidate.generation, *id)
    })
    .unwrap();
```

Document this in §2.4: "Age tie-breaking uses `generation`, then `id` as final deterministic fallback."

---

### FINDING-2-6: **[Check #21] Ambiguous: "intersection of examples that all current front members have been evaluated on"**

**Document:** design-02-pareto-front.md (GOAL-2.4 in requirements, §4.4 in design)

The crowding distance computation says:
> "Crowding distance is computed per-candidate across the dimensions defined by the intersection of examples that all current front members have been evaluated on."

This is ambiguous. Does it mean:

**Interpretation A:** The universal intersection — examples that EVERY front member has been evaluated on (∩ of all evaluated sets)

**Interpretation B:** For each candidate being pruned, the intersection of that candidate's evaluated examples with all other front members

The fallback clause compounds the ambiguity:
> "If no such universal intersection exists, crowding distance falls back to the examples shared by the most candidates."

What does "most candidates" mean? Majority? Plurality? The candidate with the largest evaluated set?

Two engineers would implement this differently.

**Suggested fix:**

Replace the ambiguous text in §4.4 with:
```rust
// Crowding distance computation:
// 1. Compute universal intersection U = examples evaluated by ALL front members
// 2. If U.is_empty(), use the largest pairwise intersection instead:
//    U = examples shared by the two candidates with the most overlapping evaluated sets
// 3. For each candidate, compute crowding distance over dimensions in U

let universal_intersection = front.members().iter()
    .map(|id| cache.evaluated_examples(id))
    .reduce(|acc, set| acc.intersection(&set).cloned().collect())
    .unwrap_or_default();

let dimensions = if universal_intersection.is_empty() {
    // Fallback: find the pair with largest intersection
    let mut max_intersection = HashSet::new();
    for a in front.members() {
        for b in front.members() {
            if a != b {
                let inter: HashSet<_> = cache.evaluated_examples(a)
                    .intersection(&cache.evaluated_examples(b))
                    .cloned()
                    .collect();
                if inter.len() > max_intersection.len() {
                    max_intersection = inter;
                }
            }
        }
    }
    max_intersection
} else {
    universal_intersection
};
```

---

### FINDING-1-7: **[Check #15] Hardcoded phase names in metrics**

**Document:** design-01-core-engine.md (§4.3)

The pseudocode uses string literals for phase names:
```rust
metrics.phase_durations.insert("select", select_duration);
metrics.phase_durations.insert("execute", execute_duration);
metrics.phase_durations.insert("reflect", reflect_duration);
metrics.phase_durations.insert("mutate", mutate_duration);
metrics.phase_durations.insert("evaluate", evaluate_duration);
metrics.phase_durations.insert("accept", accept_duration);
```

These are hardcoded magic strings. If a phase name changes or a new phase is added, every insertion site must be updated. Typos ("evalute" instead of "evaluate") would silently create new keys.

**Suggested fix:**

Define phase names as constants in §3.6:
```rust
pub mod phase_names {
    pub const SELECT: &str = "select";
    pub const EXECUTE: &str = "execute";
    pub const REFLECT: &str = "reflect";
    pub const MUTATE: &str = "mutate";
    pub const EVALUATE: &str = "evaluate";
    pub const ACCEPT: &str = "accept";
}

// Usage:
metrics.phase_durations.insert(phase_names::SELECT, select_duration);
```

---

### FINDING-2-7: **[Check #18] Missing trade-off: why crowding distance over hypervolume contribution?**

**Document:** design-02-pareto-front.md (§2.4)

GOAL-2.4 in the requirements explains the crowding distance choice:
> "Crowding distance is chosen over hypervolume contribution because: (a) it computes in O(N·M·log M) vs O(N^M) for exact hypervolume..."

But the design document §2.4 doesn't repeat this rationale. It only says:
> "When the front exceeds the maximum, the least-contributing candidate is removed using crowding distance."

A reader of the design doc alone wouldn't understand why this particular metric was chosen. The requirements explain it, but design docs should be self-contained per Check #18 (trade-offs documented).

**Suggested fix:**

Add to §2.4 after the interface definition:
```markdown
**Why crowding distance over hypervolume?**

- Complexity: O(N·M·log M) vs O(N^M) for exact hypervolume in M dimensions
- GEPA's typical M (16-200 examples per minibatch) makes hypervolume intractable
- Crowding distance is well-understood from NSGA-II with predictable behavior
- Known limitation: becomes less discriminating at high M (>50), acceptable because:
  - Most workloads use M=16-64
  - Age-based tie-breaker provides fallback when distances converge
```

---

### FINDING-1-8: **[Check #25] No unit test boundaries defined**

**Document:** design-01-core-engine.md (entire document)

The design shows a large async loop in §4.3 with many phases. But there's no discussion of testability or where test boundaries should be placed. The `run()` method is 200+ lines of pseudocode with complex control flow (multiple termination conditions, error handling, state mutations).

How would this be unit tested? Would each phase (select, execute, reflect, mutate, evaluate, accept) be broken into separate methods? Or is the entire loop expected to be tested integration-style with mock adapters?

The builder pattern (§2.1) is testable, but the core loop is not decomposed for testing.

**Suggested fix:**

Add a new section "§5 Testing Strategy" to design-01-core-engine.md:

```markdown
## §5 Testing Strategy

### 5.1 Unit Test Boundaries

The `run()` loop is decomposed into testable helper methods:

- `run_iteration(&mut self) -> Result<IterationOutcome, GEPAError>` — Execute one iteration, return accept/reject/skip
- `check_termination(&self) -> Option<TerminationReason>` — Check all termination conditions
- `handle_adapter_error(&mut self, error: GEPAError, phase: &str) -> Result<(), GEPAError>` — Error policy

Each helper is `pub(crate)` and unit-testable with a fixture `GEPAEngine`. The main `run()` loop becomes:

```rust
pub async fn run(mut self) -> Result<GEPAResult, GEPAError> {
    while let Some(reason) = self.check_termination() {
        return Ok(GEPAResult { reason, state: self.state, ... });
    }
    match self.run_iteration().await {
        Ok(IterationOutcome::Accepted) => { ... }
        Ok(IterationOutcome::Rejected) => { ... }
        Ok(IterationOutcome::Skipped) => { ... }
        Err(e) => { ... }
    }
}
```

### 5.2 Test Fixtures

- `MockAdapter` — Returns deterministic results for execute/reflect/mutate/evaluate
- `MockDataLoader` — Returns fixed minibatches
- `test_config()` — Minimal valid config for fast tests
```

---

## 🟢 Minor (can fix during implementation)

### FINDING-1-9: **[Check #4] Inconsistent naming: `rng_seed` vs `seed`**

**Document:** design-01-core-engine.md (§2.1, §3.1)

The builder has:
```rust
pub fn rng_seed(mut self, seed: u64) -> Self
```

But `GEPAConfig` (referenced from design.md) uses:
```rust
pub rng_seed: Option<u64>
```

And the builder field is:
```rust
rng_seed: Option<u64>
```

The method parameter is named `seed` while the field is `rng_seed`. Consistently use `rng_seed` everywhere.

**Suggested fix:**
```rust
pub fn rng_seed(mut self, rng_seed: u64) -> Self {
    self.rng_seed = Some(rng_seed);
    self
}
```

---

### FINDING-1-10: **[Check #3] Dead field: `cancellation_token` in builder may be unused**

**Document:** design-01-core-engine.md (§2.1)

The builder has:
```rust
cancellation_token: Option<CancellationToken>,
```

And a setter:
```rust
pub fn cancellation_token(mut self, token: CancellationToken) -> Self
```

But the pseudocode in §4.3 never checks the cancellation token. Looking at the loop:
```rust
loop {
    // No cancellation check here
    let parent_id = self.state.pareto_front.select(...)?;
    // ...
}
```

Either the field is dead (never used), or the cancellation check is missing from the pseudocode.

**Suggested fix:**

Add cancellation check to §4.3 loop:
```rust
loop {
    if let Some(ref token) = self.cancellation_token {
        if token.is_cancelled() {
            return Ok(GEPAResult {
                reason: TerminationReason::Cancelled,
                state: self.state,
                ...
            });
        }
    }
    // ... rest of iteration
}
```

---

### FINDING-2-8: **[Check #4] Inconsistent naming: `members` vs `candidates` in front**

**Document:** design-02-pareto-front.md (§2.1, §4.2)

`ParetoFront` has a field `members: Vec<CandidateId>` and method `members()`. But in pseudocode, dominance checking refers to "candidates":

§4.2:
> "For each candidate in the front..."

§4.3:
> "Collect all candidates into a Vec..."

Sometimes "member" (the ID in the front), sometimes "candidate" (the full struct). This is technically correct (members are IDs, candidates are structs), but the inconsistency could confuse readers.

**Suggested fix:**

Be consistent in prose:
- "front members" = the IDs stored in the front
- "candidates" = the full `Candidate` structs looked up from state

Update §4.2:
> "For each **front member** (candidate ID) in the front, look up the full candidate from state..."

---

### FINDING-2-9: **[Check #3] Dead field: `selection_cursor` never read externally**

**Document:** design-02-pareto-front.md (§2.1)

`ParetoFront` has:
```rust
selection_cursor: usize,
```

This is mutated by `select()`, but there's no public getter. Is this field intended to be observable (for debugging/metrics)? Or is it internal state only?

If it's internal-only, it should be `pub(crate)`. If it's observable, add a getter.

**Suggested fix:**

If observable:
```rust
impl ParetoFront {
    pub fn selection_cursor(&self) -> usize {
        self.selection_cursor
    }
}
```

If internal-only, mark the field `pub(crate)`.

---

### FINDING-1-11: **[Check #11] Missing exhaustiveness note for TerminationReason**

**Document:** design-01-core-engine.md (§3.7)

`TerminationReason` is an enum:
```rust
pub enum TerminationReason {
    MaxIterations,
    TimeBudget,
    Stagnation,
    TooManySkips,
    Cancelled,
}
```

If a new variant is added in the future (e.g., `UserRequested`, `FrontCollapsed`), any match without a catch-all `_` would fail to compile. Good. But any match WITH a catch-all would silently accept the new variant.

The design should note: "Match on `TerminationReason` should be exhaustive (no `_` catch-all) to force updates when new variants are added."

**Suggested fix:**

Add to §3.7:
```rust
// NOTE: When matching TerminationReason, use exhaustive matches (no `_` branch)
// to ensure new variants are handled explicitly.
```

---

### FINDING-2-10: **[Check #20] Pseudocode too implementation-focused in crowding distance**

**Document:** design-02-pareto-front.md (§4.4)

The crowding distance pseudocode shows:
```rust
for i in 1..sorted_by_dim.len() - 1 {
    let prev_score = scores[sorted_by_dim[i - 1].1];
    let next_score = scores[sorted_by_dim[i + 1].1];
    let range = max_score - min_score;
    distances[i] += if range > 0.0 {
        (next_score - prev_score) / range
    } else {
        0.0
    };
}
```

This is very low-level — array indexing, loop bounds, etc. The design doc should explain WHAT crowding distance is (conceptually), not HOW to index arrays. The pseudocode reads like Rust code, not design intent.

**Suggested fix:**

Replace detailed pseudocode with:
```markdown
Crowding distance (from NSGA-II):

1. For each dimension (example ID in the universal intersection):
   - Sort candidates by score on that dimension
   - Assign infinite distance to boundary candidates (best and worst)
   - For middle candidates: distance += (neighbor difference) / (dimension range)

2. Sum distances across all dimensions → each candidate gets a total crowding distance

3. Select candidate with minimum distance for removal (most "crowded" in objective space)

Implementation: see NSGA-II paper section 3.2 or [reference].
```

The actual indexing logic goes in the implementation, not the design doc.

---

## 📋 Path Traces

### Trace 1: Happy Path (design-01-core-engine)

```
Start: GEPAEngine constructed with config, adapter, data_loader, seeds
  → run() called
  → Iteration 0:
    → check_termination() → None (no limits hit)
    → select() → parent_id = seed[0]
    → sample_minibatch() → batch of 16 examples
    → execute(parent, batch) → 16 traces
    → reflect(parent, traces) → reflection
    → mutate(parent, reflection, []) → child candidate
    → evaluate(child, batch) → 16 scores
    → EvalCache.insert(child, batch, scores)
    → ParetoFront.try_accept(child) → Accepted (new non-dominated)
    → stagnation_count = 0, consecutive_skips = 0
    → emit IterationComplete event
  → Iteration 1:
    → check_termination() → None
    → select() → parent_id = child (from front)
    → [repeat select → execute → reflect → mutate → evaluate → accept]
  → ...
  → Iteration 99:
    → check_termination() → Some(TerminationReason::MaxIterations)
    → return Ok(GEPAResult { reason: MaxIterations, state, ... })
Done ✅
```

### Trace 2: Stagnation Path (design-01-core-engine)

```
Start: Engine with stagnation_limit = 10
  → Iteration 0: accept child A
  → Iteration 1: accept child B
  → Iteration 2: 
    → mutate → child C
    → evaluate → scores lower than parent
    → try_accept(C) → Rejected (dominated)
    → stagnation_count += 1 (now 1)
  → Iteration 3-11: all rejected, dominated by A or B
    → stagnation_count increments to 10
  → Iteration 12:
    → check_termination() → Some(TerminationReason::Stagnation)
    → return Ok(GEPAResult { reason: Stagnation, ... })
Done ✅
```

### Trace 3: Adapter Timeout → Skip → TooManySkips (design-01-core-engine)

```
Start: Engine with max_consecutive_skips = 3
  → Iteration 0: accept seed
  → Iteration 1:
    → execute() times out
    → handle_adapter_error → consecutive_skips = 1
    → continue (skip iteration)
  → Iteration 2:
    → execute() times out again
    → consecutive_skips = 2
    → continue
  → Iteration 3:
    → execute() times out
    → consecutive_skips = 3
    → check_termination() → Some(TerminationReason::TooManySkips)
    → return Ok(GEPAResult { reason: TooManySkips, ... })
Done ✅
```

### Trace 4: Pareto Front Selection with Overfitting Delta (design-02-pareto-front)

```
Start: Front has 3 members: [A, B, C]
  → selection_cursor = 0
  → select(cache, rng):
    → Round-robin index: 0 % 3 = 0 → candidate A
    → Check overfitting delta for A: delta_A = 0.1 (low)
    → Check B: delta_B = 0.5 (high overfitting)
    → Check C: delta_C = 0.2
    → Reorder round: [A (delta 0.1), C (delta 0.2), B (delta 0.5)]
    → Select A (lowest delta in this round)
    → selection_cursor = 1
  → Next call:
    → index: 1 % 3 = 1 → candidate B's round
    → Reorder: [B, A, C] by delta
    → Select B (even though high delta, it's B's turn per round-robin floor)
    → selection_cursor = 2
Done ✅
```

### Trace 5: Pareto Front Pruning at Capacity (design-02-pareto-front)

```
Start: Front at max_size = 50, all filled
  → try_accept(new_candidate):
    → Check dominance: new_candidate dominates none, is dominated by none
    → front.len() = 50 → will exceed after insert
    → Insert new_candidate → front.len() = 51
    → Trigger pruning:
      → Compute universal intersection of evaluated examples → U = {ex1, ex2, ..., ex20}
      → Compute crowding distance for all 51 candidates over U's 20 dimensions
      → Distances: [2.3, 1.8, 0.5, ..., 1.2]
      → Minimum distance = 0.5 → candidate ID 42
      → Remove candidate 42 from front
      → front.len() = 50 ✅
Done ✅
```

### Trace 6: Re-evaluation Triggers Full Recomputation (design-02-pareto-front)

```
Start: Front = [A, B, C] (all evaluated on examples [1, 2, 3])
  → Re-evaluation backfill runs (GOAL-8.5):
    → Candidate A re-evaluated on examples [4, 5, 6] → new scores added to cache
    → Candidate B re-evaluated on examples [4, 5, 6]
    → Now A and B share examples [1,2,3,4,5,6] (6 shared)
  → full_recomputation() triggered:
    → Check dominance between all pairs with updated coverage
    → A vs B: shared [1,2,3,4,5,6] → 6 examples (meets min_shared_examples threshold)
    → A scores: [0.8, 0.9, 0.7, 0.85, 0.9, 0.88]
    → B scores: [0.7, 0.8, 0.6, 0.75, 0.8, 0.78]
    → A >= B on all 6, strictly > on all → A dominates B
    → Remove B from front
  → Final front = [A, C] ✅
Done ✅
```

---

## ✅ Passed Checks

### Phase 0: Document Size
- **Check #0**: Document size ✅
  - design-01: 7 components in §3 (3.1-3.7) — within 8-component limit
  - design-02: 4 components in §2 (2.1-2.4) — within limit

### Phase 1: Structural Completeness
- **Check #2**: References resolve ✅
  - Verified: All cross-references to GOAL-X.Y exist in requirements
  - Verified: All internal §N.M references valid
  - Example: design-01 §4.3 references §3.2 (GEPAState) — exists
  - Example: design-02 §4.2 references §2.1 (ParetoFront struct) — exists

### Phase 2: Logic Correctness
- **Check #5**: State machine transitions ✅ (partial — see FINDING-2-1 for empty-front deadlock)
  - Happy path traced (see Trace 1) — complete from start to MaxIterations termination
  - Stagnation path traced (see Trace 2) — terminates correctly
  - Timeout/skip path traced (see Trace 3) — terminates at TooManySkips
  - All terminal states reachable from initial state

- **Check #7**: Error handling completeness ✅ (partial — see FINDING-1-4 for timeout handling gap)
  - Adapter errors have explicit handling via retry policy
  - Timeout errors trigger skip logic
  - Cancellation propagates via CancellationToken check
  - No silent error swallowing observed

### Phase 3: Type Safety & Edge Cases
- **Check #8**: String operations ✅
  - No string slicing on user/LLM text found in pseudocode
  - All string handling is via HashMap keys (owned Strings) or metadata fields
  - No `&s[..n]` patterns on potentially non-ASCII data

- **Check #10**: Option/None handling ✅
  - All Option values have explicit match or `?` propagation
  - No `.unwrap()` calls in pseudocode
  - Example: `selected_candidate_id: Option<CandidateId>` handled via match in §4.3

- **Check #12**: Ordering sensitivity ✅
  - Termination checks in §4.5 are independent (any order works)
  - Match branches in error handling are mutually exclusive (Timeout, RateLimited, AdapterError have non-overlapping patterns)

### Phase 4: Architecture Consistency
- **Check #13**: Separation of concerns ✅
  - Pure logic (dominance checking, crowding distance) is computation-only
  - Side effects isolated to adapter calls (execute, reflect, mutate, evaluate)
  - State mutations clearly marked (insert, try_accept)
  - No hidden IO in supposedly pure functions

- **Check #16**: API surface ✅
  - Public API is minimal: `GEPAEngineBuilder`, `GEPAEngine::run()`, `ParetoFront::{new, select, try_accept, full_recomputation}`
  - Internal helpers (dominance, crowding) are `pub(crate)` or private
  - No leakage of internal iteration state or RNG to public API

### Phase 5: Design Doc Quality
- **Check #17**: Goals and non-goals explicit ✅
  - Requirements docs have explicit GOAL-X.Y numbering
  - Non-requirements sections present in requirements (e.g., "No concurrent front access" in GOAL-2.6)
  - Clear boundaries: what GEPA does (optimization loop) vs. what it doesn't (LLM implementation)

- **Check #19**: Cross-cutting concerns addressed ✅
  - Observability: GOAL-9.x events, tracing integration (design.md §3.6)
  - Error visibility: all errors propagate or log via `tracing::warn!`
  - Performance: O(N²·M) dominance complexity documented with target latency (GOAL-2.5)
  - Security: not applicable (pure algorithmic library, no IO)

### Phase 6: Implementability
- **Check #23**: Dependency assumptions ✅
  - External dependencies explicit: `tokio::time::timeout`, `thiserror`, `serde`, `rand_chacha::ChaCha8Rng`
  - No assumptions about unverified libraries
  - Adapter trait explicitly delegates to external LLM implementation

- **Check #24**: Migration path ✅
  - This is a new crate (no existing code to replace)
  - N/A for v1 implementation

### Phase 7: Existing Code Alignment
- **Check #26**: Similar functionality check ✅
  - Verified: this is a new feature (gepa-core crate is new)
  - No existing Pareto front or evolutionary optimization in codebase

- **Check #27**: API compatibility ✅
  - New crate, no existing callers
  - N/A for v1

- **Check #28**: Feature flag / rollout ✅
  - Design explicitly mentions builder pattern allows gradual composition
  - Adapter trait enables testing with mocks before real LLM integration
  - No blocking dependencies on other features

---

## Summary

- **Critical:** 3 findings (FINDING-1-1, 1-2, 2-1, 2-2, 2-3)
- **Important:** 8 findings (FINDING-1-3 through 1-8, FINDING-2-4 through 2-7)
- **Minor:** 5 findings (FINDING-1-9 through 1-11, FINDING-2-8 through 2-10)

**Total:** 16 findings

### Recommendation

**Needs fixes before implementation.** 

The critical findings block implementation:
1. **Type incompleteness** (FINDING-1-1, 1-2) — missing type definitions will cause compilation failures
2. **Empty front deadlock** (FINDING-2-1, 2-2) — the state machine can enter an unrecoverable error state with no defined recovery path
3. **Integer overflow in selection** (FINDING-2-3) — breaks determinism guarantee at boundary conditions

The important findings introduce bugs that would surface during integration testing:
- Missing error handlers (FINDING-1-4, 1-5) → runtime panics or incorrect skip behavior
- Stale metrics data (FINDING-1-3) → incorrect observability
- Coupling issues (FINDING-1-6) → duplicated state in events
- Dominance computation edge cases (FINDING-2-4, 2-5, 2-6) → wrong candidates pruned from front

The minor findings are polish issues that can be fixed during implementation, but addressing them now improves code quality and reduces refactoring later.

### Estimated Implementation Confidence

**Medium** — The core algorithms (dominance, crowding distance, optimization loop) are well-specified and traceable. Path traces demonstrate the happy path and error paths are reachable. However:

- Type definitions are incomplete (reduce confidence)
- Critical edge cases (empty front, overflow) lack handling (reduce confidence)
- Error flow has gaps (missing helpers) (reduce confidence)
- Testability not addressed (reduce confidence)

After fixing critical and important findings, confidence would increase to **Medium-High**.

---

## Next Steps

**Which findings should I apply?** (e.g., 'apply FINDING-1-1,1-2,2-1,2-2,2-3' or 'apply all critical' or 'apply all')
