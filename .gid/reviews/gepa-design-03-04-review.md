# Design Review: GEPA Adapter Trait (03) + Proposers (04)

**Reviewed:** 2026-04-06
**Documents:**
- `design-03-adapter.md` (Adapter Trait)
- `design-04-proposers.md` (Proposers/Mutators)

## 🔴 Critical (blocks implementation)

### FINDING-3-1: **[Check #1] ExecutionTrace type incompletely defined**

**Document:** design-03-adapter.md §2.2

The `ExecutionTrace` struct is partially defined but critical fields are missing or inconsistent between sections:

```rust
pub struct ExecutionTrace {
    pub input: String,
    pub output: String,
    pub score: Option<f64>,
    pub asi: Option<String>,
}
```

However, in §2.4 (Error Handling), the text says: "the adapter sets the trace's output to an empty string and score to `None`" — but it doesn't specify what happens to `input` or `asi` fields in error cases. Requirements (GOAL-3.2) state that "each trace contains: the input, the generated output, an optional score, and optional actionable side information (ASI)" but the design never specifies:

1. Is `input` a copy of the Example's input or just the ID?
2. Should `asi` be `Some("")` or `None` in error cases?
3. The relationship between ExecutionTrace and Example is unclear — does ExecutionTrace store the Example or just reference it?

**Suggested fix:**

```rust
/// Execution trace for a single example. Returned by Adapter::execute.
pub struct ExecutionTrace {
    /// Input text from the example
    pub input: String,
    /// Generated output (empty string if execution failed for this example)
    pub output: String,
    /// Optional score (None if execution failed or scoring N/A)
    pub score: Option<f64>,
    /// Optional actionable side information (diagnostic context, None if not provided)
    pub asi: Option<String>,
}
```

And add to §2.4 error handling: "On per-example failure: set `output = ""`, `score = None`, `asi = Some(error_diagnostic)`. The `input` field always contains the original example input regardless of success/failure."

---

### FINDING-3-2: **[Check #7] Unbounded retry logic in error handling**

**Document:** design-03-adapter.md §2.4

The error handling section describes retry behavior: "The engine retries retryable errors up to `max_retries` (default: 3) using exponential backoff" but never specifies:

1. Is `max_retries` per-adapter-call or per-iteration?
2. What is the max backoff delay? Exponential backoff can reach hours if unbounded.
3. What happens if `retry_after` from `RateLimited` exceeds the wall-clock budget? Does it count toward budget?
4. Does a retry consume from the per-call timeout or does each retry get a fresh timeout?

This creates implementation ambiguity and potential infinite waiting if `retry_after` is large or if retries don't count toward timeout.

**Suggested fix:**

Add to §2.4:

```
Retry policy details:
- max_retries: per adapter call (each execute/reflect/mutate/evaluate call has independent retry budget)
- Exponential backoff: initial_delay = 1s, multiplier = 2, max_delay = 60s (capped)
- RateLimited with retry_after: if retry_after > remaining_wall_clock_budget, treat as non-retryable (skip)
- Each retry attempt gets a fresh per_call_timeout (retries don't share timeout budget)
- Total time per adapter call (including retries) is bounded by: min(max_retries * per_call_timeout, remaining_wall_clock_budget)
```

---

### FINDING-4-1: **[Check #1] ProposerOutput parent_ids semantics ambiguous**

**Document:** design-04-proposers.md §2.1

The `ProposerOutput` struct has a `parent_ids: Vec<u64>` field, but the design never specifies:

1. For `MutationProposer`: is this always a single-element vec `[parent_id]`?
2. For `MergeProposer`: is this always a two-element vec `[parent_a_id, parent_b_id]`?
3. What's the ordering guarantee for merge parent IDs? (Does order matter for lineage tracking?)
4. Can this vec be empty? (What about seed candidates with no parents?)

The requirements (GOAL-4.5) say merged candidates have "both parents recorded in its lineage (tree-structured)" but the design doesn't specify how `parent_ids` maps to lineage.

**Suggested fix:**

Add to §2.1 after the ProposerOutput definition:

```rust
/// Semantics of parent_ids:
/// - MutationProposer: always exactly one element [parent_id]
/// - MergeProposer: always exactly two elements [parent_a_id, parent_b_id], ordered by selection
/// - For seed candidates (no parents): empty vec []
/// The engine uses parent_ids to construct the candidate's lineage field.
```

And update §2.2 and §2.3 to explicitly set `parent_ids = vec![parent.id]` and `parent_ids = vec![parent_a.id, parent_b.id]` respectively.

---

### FINDING-4-2: **[Check #6] Selection counter state not persisted in checkpoint**

**Document:** design-04-proposers.md §2.2

The `MutationProposer` maintains a `selection_counts: HashMap<u64, usize>` for round-robin fairness (GOAL-4.3), but §2.2.1 Checkpoint Representation only serializes:

```rust
{
  "type": "mutation",
  "lesson_depth": 10
}
```

This means if the engine checkpoints mid-run and resumes, the selection counts are lost, violating the round-robin guarantee. A candidate that was selected 5 times before checkpoint would start fresh at 0 after resume, allowing it to be selected again before other candidates.

**Suggested fix:**

Change §2.2.1 checkpoint format to:

```rust
{
  "type": "mutation",
  "lesson_depth": 10,
  "selection_counts": { "42": 3, "59": 1, "67": 2 }  // Map<candidate_id, count>
}
```

And add to §2.2: "The selection_counts map is serialized in checkpoints to preserve round-robin fairness across resume. When a candidate is removed from the front, its entry is removed from selection_counts (prevents unbounded growth)."

---

### FINDING-4-3: **[Check #5] Complementary pair selection has unreachable state**

**Document:** design-04-proposers.md §2.3

The complementary pair selection algorithm in §2.3 says: "If the front has fewer than 2 candidates, the merge step is skipped for that interval."

But this creates a logical contradiction with the engine loop: the requirements (GOAL-4.4) say merge proposer is only called "periodic" (controlled by GOAL-7.7), and the master design data flow shows proposers are only invoked after "ParetoFront::select" — meaning the front must already be non-empty.

However, the design never specifies what happens if the front drops to size 1 during the run (e.g., if pruning removes all but one candidate). Does the engine:
1. Skip the merge interval silently?
2. Return an error?
3. Fall back to mutation proposer for that iteration?

This is an unreachable state if handled incorrectly (merge proposer called with insufficient front) OR a silent skip that violates iteration budgets (user expects N iterations but gets fewer).

**Suggested fix:**

Add to §2.3 after the "fewer than 2 candidates" sentence:

```
When front.size() < 2:
- The engine detects this before calling the merge proposer
- Returns Err(GEPAError::InsufficientFrontSize { size: front.size() })
- The engine's error policy applies (per GOAL-7.5): skip this iteration and increment skip counter
- The merge interval counter does NOT advance (next merge attempt happens at the originally scheduled iteration)
```

And add a guard condition to the master design (design.md §3.1) under `GEPAError::InsufficientFrontSize`: "Thrown when merge proposer is scheduled but front has <2 candidates. Treated as retryable=false (immediate skip)."

---

## 🟡 Important (should fix before implementation)

### FINDING-3-3: **[Check #13] Separation of concerns: adapter methods mixing concerns**

**Document:** design-03-adapter.md §2.1

The trait design has `execute` and `evaluate` methods that overlap significantly:
- Both receive `&Candidate` and `&[Example]`
- Both run the candidate on examples
- Both can produce scores

The distinction is stated as: "`execute` captures rich traces for reflection, `evaluate` produces scores for ranking" but the implementation boundary is fuzzy. Specifically:

1. `execute` returns `ExecutionTrace` with **optional** `score` field — so it CAN score
2. `evaluate` returns `Vec<f64>` — so it CAN'T capture traces

This means an adapter implementation might need to:
- Run the same LLM call twice (once for execute, once for evaluate) — inefficient
- Or cache execution results to reuse in evaluate — complex state management
- Or compute scores in execute and return them, then re-extract in evaluate — duplicated logic

The design should clarify whether:
- `execute` is allowed to skip scoring (always return `score: None`) and leave all scoring to `evaluate`
- OR `evaluate` should be allowed to use cached results from prior `execute` calls
- OR adapters are expected to run the candidate twice per iteration (wasteful but clean separation)

**Suggested fix:**

Add to §2.1 after the trait definition:

```
**Execute vs Evaluate separation:**

The engine calls both methods sequentially within each iteration:
1. execute(candidate, minibatch) → rich traces (for reflect)
2. evaluate(candidate, minibatch) → scores only (for accept/reject)

Adapters may choose one of two implementation strategies:

**Strategy A (Dual-run, strict separation):**
- execute: run candidate, return traces with score=None and rich ASI
- evaluate: independently run candidate again, return scores
- Pro: Clean separation, no state. Con: 2x LLM calls per iteration.

**Strategy B (Cached execution):**
- execute: run candidate, return traces with score=Some(s) and rich ASI
- evaluate: check internal cache for matching (candidate_id, example_id), return cached scores if fresh; otherwise re-run
- Pro: 1x LLM call per iteration. Con: Adapter must manage cache + staleness.

The engine NEVER assumes evaluate reuses execute results — it treats them as independent calls.
```

This removes implementation ambiguity while allowing efficient implementations.

---

### FINDING-3-4: **[Check #2] Reference to undefined GEPAConfig fields**

**Document:** design-03-adapter.md §2.4

The error handling section references "`max_retries` (default: 3)" and "`per_call_timeout`" as config fields, but these are never defined in design-03. The requirements (GOAL-3.7, GOAL-7.5) mention retry policy but don't specify these exact field names.

This creates a dangling reference — the adapter design assumes config fields that may not exist or may have different names in the actual config design (design-07).

**Suggested fix:**

Add a forward reference at the end of §2.4:

```
**Config fields (defined in design-07):**
- max_retries: usize (default: 3) — per adapter call
- per_call_timeout: Duration (default: 30s) — timeout per individual adapter method call
- error_policy: ErrorPolicy (default: Skip) — what to do after retry exhaustion
```

Or alternatively, check design-07 and update this section to use the actual config field names once design-07 is finalized.

---

### FINDING-3-5: **[Check #18] Trade-offs for async_trait not documented**

**Document:** design-03-adapter.md §1 Overview

The design states "uses `async_trait` for ergonomic async trait methods with `Send` bounds" but never documents the trade-off. `async_trait` has known costs:

1. Allocates a `Box<dyn Future>` for every async method call (heap allocation)
2. Requires all futures to be `Send` (can't use `Rc` or thread-local storage in async adapter methods)
3. Alternative: native async trait (Rust 1.75+) with return-position `impl Future` — zero alloc, but requires callers to name the type or use `Box` explicitly

The design should document WHY `async_trait` was chosen over native async trait, especially since "LLM latency dwarfs vtable cost" (from master design §2) — if latency dwarfs vtable, it also dwarfs box allocation, but native async trait would remove allocations entirely.

**Suggested fix:**

Add to §1 Overview after the async_trait mention:

```
**Trade-off:** async_trait vs native async trait

Chosen: async_trait (macro-based)
- Pro: Ergonomic syntax, wide ecosystem support, works on Rust 1.70+
- Con: Boxes every async call (heap allocation), requires Send bounds

Rejected: Native async trait (AFIT, Rust 1.75+)
- Pro: Zero-alloc, no macro magic, native compiler support
- Con: Requires callers to Box or name the exact Future type (verbose at trait object boundaries)

Since adapter calls are infrequent (1-4 per iteration) and dominated by LLM latency (seconds), the Box allocation cost (<1μs) is negligible. We prioritize ergonomics over micro-optimization.
```

---

### FINDING-4-4: **[Check #6] Ancestor lesson chain stale data risk**

**Document:** design-04-proposers.md §2.2

The mutation proposer builds the ancestor lesson chain by walking parent lineage: "collects all ancestor reflections by walking the lineage from parent → grandparent → ... until reaching a seed candidate."

But the design never specifies:
1. Are reflections stored in the `Candidate` object itself?
2. Or are they fetched from a separate reflection store?
3. What if a candidate's lineage references a parent that was removed from the front and pruned from the candidate store?

The master design (§3.4) says "Candidate is **not** generic" and stores text parameters in `HashMap<String, String>` — but doesn't mention reflections. This suggests reflections might be external, creating a data staleness risk:

- Candidate A has parent B
- B is pruned from the front and evicted from the candidate store
- Mutation proposer tries to walk lineage to B → data not found

**Suggested fix:**

Add to design-04 §2.2 after the lesson chain collection pseudocode:

```rust
// Reflection storage:
// Option 1: Store reflection in Candidate.metadata: HashMap<String, String>
//   - Key: "reflection", Value: JSON-serialized Reflection
//   - Pro: Self-contained, no external lookups. Con: Increases Candidate size.

// Option 2: Separate reflection cache in GEPAState
//   - reflection_cache: HashMap<u64 /*candidate_id*/, Reflection>
//   - Pro: Candidates stay lightweight. Con: Must handle missing reflections.

// **Chosen: Option 1** (reflection in Candidate.metadata)
// Rationale: Lessons are critical for lineage; missing reflections break the algorithm.
// Candidates without reflections (e.g., seeds) have empty metadata.

impl MutationProposer {
    fn collect_lessons(&self, parent: &Candidate, store: &CandidateStore) -> Vec<String> {
        let mut lessons = Vec::new();
        let mut current_id = parent.id;
        
        for _ in 0..self.lesson_depth {
            let candidate = store.get(current_id)?; // Returns None if pruned
            if let Some(refl_json) = candidate.metadata.get("reflection") {
                let refl: Reflection = serde_json::from_str(refl_json)?;
                lessons.push(refl.diagnosis);
            }
            // Walk to parent
            if candidate.parent_ids.is_empty() { break; } // Seed reached
            current_id = candidate.parent_ids[0]; // Assume single parent for mutation lineage
        }
        
        lessons.reverse(); // Oldest → newest
        lessons
    }
}
```

And add to design-05 (Candidate): "Candidate.metadata includes a `reflection` key with JSON-serialized Reflection for non-seed candidates."

---

### FINDING-4-5: **[Check #23] Complementarity calculation assumes cached scores exist**

**Document:** design-04-proposers.md §2.3

The complementary pair selection algorithm says: "Complementarity is computed over the intersection of examples both candidates have been evaluated on (from the evaluation cache, GOAL-6.3)."

But this assumes:
1. The evaluation cache always has overlapping examples for any two front candidates
2. The overlap is non-empty (otherwise complementarity is undefined)
3. Scores in the cache are still valid (not stale)

The design never handles the case where two candidates have zero overlapping evaluated examples (e.g., if minibatch sampling never draws the same examples for both). In this case:

```rust
let overlap = a_examples ∩ b_examples;
if overlap.is_empty() {
    // Complementarity undefined — should we:
    // 1. Skip this pair?
    // 2. Treat as maximally complementary (worst-case assumption)?
    // 3. Return an error?
}
```

**Suggested fix:**

Add to §2.3 after the complementarity definition:

```rust
// Edge case: No overlapping examples
// If intersection(a_examples, b_examples).is_empty():
//   - Set complementarity(a, b) = 0 (treat as non-complementary)
//   - Rationale: Can't determine complementarity without shared evaluation context
//   - These pairs are ranked last (prefer pairs with data-backed complementarity)
//
// If ALL pairs have zero overlap (pathological case):
//   - Fall back to random pair selection using the seeded RNG
//   - Log a warning: "Insufficient example overlap for complementarity-based merge"
```

This handles the dependency assumption explicitly.

---

### FINDING-4-6: **[Check #21] Ambiguous: "most complementary" tie-breaking order**

**Document:** design-04-proposers.md §2.3

The complementarity tie-breaking says: "Tie-breaking: prefer the pair with the highest combined average score; if still tied, break using the seeded RNG per GUARD-9."

But "combined average score" is ambiguous:
- Average of (A's average score, B's average score)?
- Average of all scores from both A and B (concatenated)?
- Harmonic mean, geometric mean, or arithmetic mean?

Two engineers could implement this differently. Also, the tie-breaking doesn't specify whether to prefer pairs with:
- Higher variance (more specialized) or lower variance (more general)?
- More overlapping examples (higher confidence) or fewer (more speculative)?

**Suggested fix:**

Replace the tie-breaking sentence with:

```
Tie-breaking (when multiple pairs have same complementarity score):
1. Prefer pair with highest combined average score:
   combined_avg(A, B) = (mean(A's scores on overlap) + mean(B's scores on overlap)) / 2
2. If still tied (within 1e-6): prefer pair with more overlapping examples (higher confidence)
3. If still tied: break using rng.gen_range(0..tied_pairs.len()) per GUARD-9
```

---

## 🟢 Minor (can fix during implementation)

### FINDING-3-6: **[Check #4] Inconsistent naming: Example vs example**

**Document:** design-03-adapter.md

The trait uses `examples: &[Example]` (capital E, type name) but prose sometimes refers to "input examples" (lowercase). While not technically wrong, it's slightly inconsistent. The master design consistently uses `Example` when referring to the type.

**Suggested fix:** Use `Example` consistently in prose when referring to the data type, reserve "example" (lowercase) for informal descriptions.

---

### FINDING-3-7: **[Check #16] merge method default impl leaks retryable hint**

**Document:** design-03-adapter.md §2.1

The default `merge` implementation says: "returns `Err(GEPAError::AdapterError { source: \"merge not implemented\", retryable: false })`"

This is fine, but the error message could be more specific. An adapter implementer might see "adapter error" in logs and think it's a real LLM failure, not an expected "merge not supported" condition.

**Suggested fix:**

Change the default impl error to use a more specific variant:

```rust
async fn merge(&self, _a: &Candidate, _b: &Candidate) -> Result<Candidate, GEPAError> {
    Err(GEPAError::Internal { 
        message: "merge() not implemented for this adapter (optional method)".to_string() 
    })
}
```

Or add a new variant: `GEPAError::MethodNotImplemented { method: String }` (cleaner, but requires master design change).

---

### FINDING-4-7: **[Check #8] Potential usize overflow in selection counter**

**Document:** design-04-proposers.md §2.2

The `selection_counts: HashMap<u64, usize>` increments without bounds checking:

```rust
*selection_counts.entry(parent.id).or_insert(0) += 1;
```

If a candidate is selected `usize::MAX` times (admittedly unlikely with typical run budgets of 1000 iterations), this overflows. While practically impossible for GEPA workloads, GUARD-8 says "no `.unwrap()` in library code" — overflow is similar undefined behavior.

**Suggested fix:**

Replace increment with saturating add:

```rust
let count = selection_counts.entry(parent.id).or_insert(0);
*count = count.saturating_add(1);
```

Or add a sanity check:

```rust
let count = selection_counts.entry(parent.id).or_insert(0);
if *count < usize::MAX { *count += 1; }
```

Saturating add is cleaner.

---

### FINDING-4-8: **[Check #20] Complementarity algorithm pseudocode too high-level**

**Document:** design-04-proposers.md §2.3

The complementarity calculation says "maximize |A_better ∪ B_better|" but the pseudocode is missing. A competent engineer could interpret this as:

1. Set union (correct): `|A_better ∪ B_better|` = count of examples where A OR B is better
2. Set cardinality: `|A_better| + |B_better|` (incorrect if sets overlap, but possible misread)

While the prose is clear, pseudocode would remove ambiguity.

**Suggested fix:**

Add concrete pseudocode to §2.3:

```rust
fn complementarity(a: &Candidate, b: &Candidate, cache: &EvalCache) -> usize {
    let overlap: HashSet<ExampleId> = cache.get_examples(a.id)
        .intersection(&cache.get_examples(b.id))
        .copied()
        .collect();
    
    let mut a_better = HashSet::new();
    let mut b_better = HashSet::new();
    
    for ex_id in overlap {
        let score_a = cache.get_score(a.id, ex_id).unwrap();
        let score_b = cache.get_score(b.id, ex_id).unwrap();
        
        if score_a > score_b { a_better.insert(ex_id); }
        else if score_b > score_a { b_better.insert(ex_id); }
        // Ties (score_a == score_b) go to neither set
    }
    
    a_better.union(&b_better).count() // Set union cardinality
}
```

---

## 📋 Path Traces

### Adapter Trait (design-03)

Not a state machine, but let's trace the adapter call sequences:

**Mutation iteration (happy path):**
```
Engine.run()
  → loader.next_batch() → Vec<Example>
  → front.select(rng) → parent Candidate
  → adapter.execute(&parent, &batch) → Vec<ExecutionTrace> ✅ (one per example)
  → adapter.reflect(&parent, &traces) → Reflection ✅
  → adapter.mutate(&parent, &reflection, &lessons) → child Candidate ✅
  → adapter.evaluate(&child, &batch) → Vec<f64> ✅ (one per example)
  → cache.insert + front.try_accept → accepted/rejected
```

**Merge iteration (happy path):**
```
Engine.run()
  → loader.next_batch() → Vec<Example>
  → front.select_pair(rng) → (parent_a, parent_b)
  → adapter.execute(&parent_a, &batch) → Vec<ExecutionTrace> ✅
  → adapter.execute(&parent_b, &batch) → Vec<ExecutionTrace> ✅
  → adapter.merge(&parent_a, &parent_b) → child Candidate ✅
  → adapter.evaluate(&child, &batch) → Vec<f64> ✅
  → cache.insert + front.try_accept → accepted/rejected
```

**Failure path: adapter.execute times out (retryable)**
```
Engine.run()
  → adapter.execute(&parent, &batch)
    → timeout after per_call_timeout
    → return Err(GEPAError::Timeout)
  → Engine retry logic (attempt 1)
    → adapter.execute(&parent, &batch) [retry]
    → succeeds → Vec<ExecutionTrace> ✅
  → continue with reflect...
```

**Failure path: adapter.mutate returns non-retryable error**
```
Engine.run()
  → adapter.execute → ✅
  → adapter.reflect → ✅
  → adapter.mutate → Err(GEPAError::AdapterError { retryable: false })
  → Engine error policy (Skip)
    → increment skip_count
    → skip to next iteration (no candidate produced) ✅
```

**Edge case: execute returns wrong number of traces**
```
Engine.run()
  → adapter.execute(&parent, &[ex1, ex2, ex3])
    → returns Vec<ExecutionTrace> with 2 elements (BUG in adapter impl)
  → Engine validation fails
    → return Err(GEPAError::InvalidCandidate { 
        message: "execute returned 2 traces, expected 3" 
      })
  → Treated as adapter error → retry or skip ✅
```

### Proposers (design-04)

**Mutation proposer happy path:**
```
Engine.run()
  → MutationProposer.generate(front, cache, adapter, ...)
    → select parent from front (round-robin)
    → collect ancestor lessons (walk lineage, max depth=10)
    → adapter.execute(&parent, &batch) → traces
    → adapter.reflect(&parent, &traces) → reflection
    → adapter.mutate(&parent, &reflection, &lessons) → child
    → return ProposerOutput { candidate: child, parent_ids: [parent.id] } ✅
```

**Merge proposer happy path:**
```
Engine.run()
  → MergeProposer.generate(front, cache, adapter, ...)
    → select complementary pair (scan O(N²) pairs)
    → adapter.execute(&parent_a, &batch) → traces_a
    → adapter.execute(&parent_b, &batch) → traces_b
    → adapter.merge(&parent_a, &parent_b) → child
    → return ProposerOutput { candidate: child, parent_ids: [a.id, b.id] } ✅
```

**Edge case: front drops to size 1 mid-run, merge scheduled**
```
Engine.run() at iteration 50 (merge interval)
  → MergeProposer.generate(front, ...)
    → front.size() = 1
    → return Err(GEPAError::InsufficientFrontSize { size: 1 }) ❌
  → Engine error policy (Skip)
    → increment skip_count
    → next merge at iteration 100 (interval doesn't advance) ✅
```

**Edge case: lesson depth exceeds lineage**
```
MutationProposer.generate()
  → lesson_depth = 10
  → lineage depth = 3 (candidate → parent → grandparent → seed)
  → collect 3 lessons (all available)
  → no error, just fewer lessons than max ✅
```

**Edge case: complementarity tie across all pairs**
```
MergeProposer.generate()
  → all 50 pairs have complementarity = 10
  → tie-break by combined average score
    → 5 pairs tied at avg = 0.85
  → tie-break by overlap size
    → 2 pairs tied at overlap = 30 examples
  → tie-break by RNG: rng.gen_range(0..2) → pair #1 ✅
```

---

## ✅ Passed Checks

### Design-03 (Adapter Trait)

- **Check #0: Document size** ✅ — 4 components (§2.1 trait, §2.2 ExecutionTrace, §2.3 Reflection, §2.4 error handling). Well under 8-component limit.

- **Check #2: References resolve** ✅ — All mentioned types exist: `Candidate` (design-05), `Example` (design-08), `GEPAError` (master design §3.1), `ExecutionTrace` and `Reflection` (defined in this doc).

- **Check #3: No dead definitions** ✅ — All types used: `ExecutionTrace` used by execute/reflect, `Reflection` used by reflect/mutate, `GEPAError` returned by all methods.

- **Check #5: State machine invariants** ✅ (N/A) — Adapter trait is stateless (methods don't transition state).

- **Check #9: Integer overflow** ✅ — No counter arithmetic in adapter trait itself (only in proposers, flagged separately).

- **Check #10: Option/None handling** ✅ — All Option fields (`score`, `asi` in ExecutionTrace) have explicit semantics. No unwraps.

- **Check #11: Match exhaustiveness** ✅ — No match statements in adapter trait (pure interface).

- **Check #12: Ordering sensitivity** ✅ — No if-else chains with guards in trait definition.

- **Check #14: Coupling** ✅ — Adapter methods receive only observed data (`&Candidate`, `&[Example]`, `&[ExecutionTrace]`), no derived state.

- **Check #15: Configuration vs hardcoding** ✅ — No hardcoded values in adapter trait itself. Config references (max_retries) deferred to config design.

- **Check #17: Goals and non-goals explicit** ✅ — §1 Overview states goals (GOAL-3.1 through GOAL-3.8) and non-goals (no LLM calls in core, GUARD-5).

- **Check #19: Cross-cutting concerns** ✅ — Error handling (§2.4), async design (§1), and trait bounds (Send+Sync) all addressed.

- **Check #22: Missing helpers** ✅ — No helper functions referenced. All methods are trait methods with clear signatures.

- **Check #24: Migration path** ✅ — This is a new trait (no existing code to migrate). Backward compat not applicable.

- **Check #25: Testability** ✅ — Adapter trait is pure interface; implementations can be mocked for testing. Example: `struct MockAdapter` that returns fixed traces/reflections.

- **Check #26: Similar functionality exists?** ✅ — No similar adapter trait in codebase (this is the first design).

- **Check #27: API compatibility** ✅ — New API, no existing callers to break.

- **Check #28: Feature flag** ✅ — Not applicable (this is core functionality, no gradual rollout).

### Design-04 (Proposers)

- **Check #0: Document size** ✅ — 3 components (§2.1 Proposer trait, §2.2 MutationProposer, §2.3 MergeProposer). Well under 8-component limit.

- **Check #2: References resolve** ✅ — All referenced types exist: `ParetoFront` (design-02), `EvaluationCache` (design-06), `GEPAAdapter` (design-03), `CandidateStore` (design-05), `Example` (design-08), `GEPAConfig` (design-07), `ChaCha8Rng` (rand_chacha crate).

- **Check #3: No dead definitions** ✅ — All types used: `Proposer` implemented by Mutation/Merge proposers, `ProposerOutput` returned by generate(), `selection_counts` used for round-robin.

- **Check #4: Consistent naming** ✅ — "Proposer" used consistently (not "Producer" or "Generator"), "parent" vs "parents" correctly distinguished (single parent for mutation, pair for merge).

- **Check #5: State machine invariants** ✅ (N/A) — Proposers don't define state machines (they're stateful but not state-machine-based).

- **Check #7: Error handling completeness** ✅ — All adapter errors propagated to engine. No silent swallowing. Retries handled by engine, not proposer.

- **Check #9: Integer overflow** ✅ (flagged as minor) — selection_counts increment should use saturating_add, but practically safe.

- **Check #10: Option/None handling** ✅ — No unwraps in proposer logic. cache.get_score() returns Option, handled explicitly.

- **Check #11: Match exhaustiveness** ✅ — No match statements with catch-all branches in proposer design.

- **Check #12: Ordering sensitivity** ✅ — Tie-breaking order is explicit (complementarity → avg score → overlap size → RNG).

- **Check #13: Separation of concerns** ✅ — Proposers are pure orchestration (no LLM calls). Adapter isolation maintained.

- **Check #14: Coupling** ✅ — Proposers receive immutable references to front, cache, candidates. Never modify shared state directly (return ProposerOutput instead).

- **Check #15: Configuration vs hardcoding** ✅ — lesson_depth, merge_interval configurable (references GOAL-7.1, GOAL-7.7).

- **Check #16: API surface** ✅ — Only `Proposer::generate()` is public. Internal helper methods (select_complementary, collect_lessons) are impl-private.

- **Check #17: Goals and non-goals explicit** ✅ — §1 Overview lists addressed goals (GOAL-4.1 through GOAL-4.5). Non-goal: proposers don't call LLMs (delegated to adapter).

- **Check #18: Trade-offs documented** ✅ — §2.3 documents O(N²) complementarity scan, justified by "negligible relative to adapter call time."

- **Check #19: Cross-cutting concerns** ✅ — Determinism (GUARD-9 via RNG parameter), error propagation, checkpointing all addressed.

- **Check #22: Missing helpers** ✅ — All referenced helpers are pseudocoded: collect_lessons, select_complementary, round-robin selection.

- **Check #24: Migration path** ✅ — New component, no existing code to migrate.

- **Check #25: Testability** ✅ — Proposers can be unit-tested with mock adapter (returns fixed traces/reflections). RNG seeding ensures deterministic tests.

- **Check #26: Similar functionality exists?** ✅ — No existing proposer mechanism in codebase.

- **Check #27: API compatibility** ✅ — New API, no breaking changes.

- **Check #28: Feature flag** ✅ — Merge proposer can be disabled via config (merge_interval = None), effectively a runtime feature flag.

---

## Summary

### Findings Count
- **Critical:** 5 (FINDING-3-1, FINDING-3-2, FINDING-4-1, FINDING-4-2, FINDING-4-3)
- **Important:** 6 (FINDING-3-3, FINDING-3-4, FINDING-3-5, FINDING-4-4, FINDING-4-5, FINDING-4-6)
- **Minor:** 3 (FINDING-3-6, FINDING-3-7, FINDING-4-7, FINDING-4-8)

### Recommendation

**Needs fixes before implementation.** 

The 5 critical findings must be resolved (they block implementation due to undefined behavior or missing state):
1. ExecutionTrace incomplete definition (FINDING-3-1)
2. Unbounded retry logic (FINDING-3-2)
3. ProposerOutput semantics ambiguous (FINDING-4-1)
4. Selection counter not checkpointed (FINDING-4-2)
5. Merge with insufficient front size (FINDING-4-3)

The 6 important findings should be resolved to avoid implementation divergence and performance issues.

The 3 minor findings can be addressed during implementation.

### Estimated Implementation Confidence

**Medium-high** — The core architecture is sound (trait design, proposer orchestration, error propagation). The main risks are:

1. **Missing field definitions** (ExecutionTrace, ProposerOutput) could cause rework
2. **Retry logic ambiguity** could lead to infinite waits or timeout violations
3. **Checkpoint completeness** could violate fairness guarantees on resume

Once the critical findings are addressed, confidence increases to **high**. The design is well-structured and testable.

---

## Next Steps

Please review the findings and indicate which should be applied:

- Apply all critical findings (1-5)?
- Apply all important findings (3-8)?
- Apply minor findings selectively?
- Or specify individual findings: e.g., "apply FINDING-3-1, FINDING-3-2, FINDING-4-2"

I can generate concrete patches for each finding once approved.
