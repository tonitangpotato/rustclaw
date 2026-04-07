# Review: gepa-core Master Design Document

> Systematic review of `/Users/potato/rustclaw/.gid/features/gepa-core/design.md`
> 
> Reviewer: Claude (design-review skill)
> Date: 2026-04-06
> Review scope: All 28 checks applied to master architectural design document

---

## 🔴 Critical (blocks implementation)

### FINDING-M-1: [Check #1] ExecutionTrace.asi field undefined
**Location:** §6 Public API Surface, `ExecutionTrace` struct

The `ExecutionTrace` struct includes:
```rust
pub struct ExecutionTrace {
    pub example_id: ExampleId,
    pub output: String,
    pub score: Option<f64>,
    pub asi: Option<String>,  // ← What is "asi"?
}
```

The field `asi` is never defined, explained, or used anywhere in the master design document. Two engineers would implement this differently:
- Is it "Agent System Instruction"?
- Is it "Analysis/Summary Information"?
- Should it be present at all?

The requirements master mentions "capture full execution traces (reasoning, tool calls, outputs, errors)" but never defines "asi". No feature design doc reference explains this field.

**Suggested fix:**
1. If `asi` stands for a specific concept, add a comment or rename to a self-documenting name (e.g., `agent_system_instruction`, `analysis_summary`, `additional_state_info`)
2. Add a prose explanation in §6 or reference the feature design doc that defines execution traces
3. If it's not needed, remove it from the struct definition

---

### FINDING-M-2: [Check #2] Missing section reference for §3.7
**Location:** §5 Data Flow, step 1

The text states:
> Check cancellation token (GOAL-3.7)

But there is no §3.7 in the document. The cross-cutting concerns section (§3) goes from §3.1 to §3.12, skipping §3.7. This appears to be a numbering error — §3.6 jumps directly to §3.8.

**Suggested fix:**
Either:
1. Renumber §3.8-§3.12 to §3.7-§3.11 and update all references, OR
2. Add the missing §3.7 section to cover the cancellation token mechanism referenced in §5

---

### FINDING-M-3: [Check #5] State machine — unbounded retry policy
**Location:** §5 Data Flow, steps 5-8

The design references "retry policy" in multiple locations:
- §5 step 5: "`adapter.execute(&parent, &batch)` with retry policy"
- §3.1 Error Handling: "Adapter errors → engine retry policy (GOAL-7.5) → after exhaustion, `Skip`"

However, the retry policy itself is never defined:
- How many retries? (bounded or unbounded?)
- What's the backoff strategy?
- Which errors are retryable vs non-retryable?
- Is the retry counter reset between iterations?

The `GEPAError::AdapterError` has a `retryable: bool` field, but the actual retry loop logic is unspecified. Without bounds, this could create infinite retry loops, violating the review rule: "Retry logic has bounded retries (no unbounded retry loops)".

**Suggested fix:**
Add a new subsection (§3.7 or §3.13) defining:
```
Retry Policy:
- Max retries per adapter call: 3 (configurable in GEPAConfig as `adapter_max_retries`)
- Retry only when GEPAError::AdapterError has retryable=true
- Exponential backoff: 1s, 2s, 4s between retries
- After exhaustion: log warning, increment skip counter, emit IterationSkipped event
- Non-retryable errors: Cancelled, InvalidConfig, InvalidCandidate → immediate halt
- Retryable errors: AdapterError, Timeout, RateLimited → bounded retry
```

And update GEPAConfig to include:
```rust
pub struct GEPAConfig {
    // ...
    pub adapter_max_retries: u32,  // default: 3
    pub adapter_retry_backoff_ms: Vec<u64>,  // default: [1000, 2000, 4000]
}
```

---

### FINDING-M-4: [Check #5] State machine — deadlock on empty Pareto front
**Location:** §5 Data Flow, step 4

Step 4 states:
> **Select** — `ParetoFront::select(&mut rng)` returns parent ID.

But what happens if the Pareto front is empty? The error type `GEPAError::EmptyFrontError` exists (§3.1), but §5 doesn't specify when this error is returned or how the engine handles it.

Tracing the state machine:
1. Engine starts with seed candidates
2. All seeds fail evaluation → `GEPAError::AllSeedsFailed` terminates run
3. But what if the front later becomes empty due to a bug in dominance pruning?

If `select()` is called on an empty front, does it:
- Return `EmptyFrontError` → halt the run? (this would terminate gracefully)
- Panic? (violates GUARD-8)
- Enter undefined behavior?

**Suggested fix:**
Add explicit handling in §5 step 4:
```
4. **Select** — Check if Pareto front is empty. If empty:
   - Emit GEPAEvent::FatalError with "Pareto front unexpectedly empty"
   - Return Err(GEPAError::EmptyFrontError)
   Otherwise, `ParetoFront::select(&mut rng)` returns parent ID using round-robin 
   with overfitting-delta deprioritization (GOAL-2.3).
```

---

## 🟡 Important (should fix before implementation)

### FINDING-M-5: [Check #17] Non-goals missing
**Location:** §1 Overview

The master design has a clear "Key design trade-offs" table documenting decisions and rationales, which is excellent. However, there are no explicit **non-goals** — things that could be goals but are deliberately excluded.

Examples of useful non-goals for this design:
- "We are NOT implementing hypervolume-based Pareto pruning (too expensive for M=200)"
- "We are NOT supporting multi-threaded concurrent evaluation (breaks determinism GUARD-9)"
- "We are NOT providing built-in LLM adapters (GUARD-5 — consumers must implement)"
- "We are NOT implementing gradient-based optimization (GEPA is evolutionary, not gradient-descent)"

Non-goals prevent scope creep and clarify what the crate intentionally does not do.

**Suggested fix:**
Add a "Non-Goals" section to §1 or create a new §1.1 "Goals and Non-Goals":
```markdown
### What GEPA-core does NOT do:
1. **No built-in LLM clients** — per GUARD-5, all LLM interaction is delegated to adapters
2. **No concurrent evaluation** — sequential loop preserves determinism (GUARD-9)
3. **No hypervolume pruning** — O(N^M) is too expensive; crowding distance is O(N·M·log M)
4. **No gradient-based optimization** — GEPA is evolutionary; gradients not applicable
5. **No automatic hyperparameter tuning** — config values must be set by consumer
6. **No distributed execution** — single-machine, single-threaded loop
```

---

### FINDING-M-6: [Check #3] Dead definition — GEPAError::ValidationError never used
**Location:** §3.1 Error Handling

The error type includes:
```rust
#[error("validation error: {0}")]
ValidationError(String),
```

However, this error variant is never referenced in the master design. The validation logic is described as:
- "Config errors caught at construction (GOAL-7.3)"
- "data-dependent validation at `run()` start (GOAL-1.0)"

But both of these map to `InvalidConfig`, not `ValidationError`. Searching the document:
- `InvalidConfig` is referenced explicitly in §3.1 error flow
- `ValidationError` is mentioned zero times outside its definition

Either `ValidationError` is dead code, or it serves a distinct purpose that's undocumented.

**Suggested fix:**
1. If `ValidationError` is redundant with `InvalidConfig`, remove it
2. If they're distinct (e.g., `InvalidConfig` = config syntax errors, `ValidationError` = semantic validation failures), document the distinction in §3.1:
```
- InvalidConfig: Malformed config (missing required fields, type errors)
- ValidationError: Semantically invalid config (e.g., minibatch_size > training set size)
```

---

### FINDING-M-7: [Check #4] Inconsistent naming — "lesson" vs "lessons"
**Location:** Multiple locations

The design uses both singular "lesson" and plural "lessons" inconsistently:

**Singular usage:**
- §6: `Candidate.lesson: Option<String>` (singular field name)
- §6 comment: "the distilled improvement insight" (singular semantic meaning)

**Plural usage:**
- §3.2: `async fn mutate(..., ancestor_lessons: &[String])`
- §5 step 7: "Build ancestor lesson chain"
- §4 Feature Index: "ancestor lesson chain (GOAL-4.2)"

The confusion: Is `Candidate.lesson` a single string, or is `ancestor_lessons` built by collecting multiple `lesson` fields? The §6 comment clarifies:
> "The ancestor lesson chain (GOAL-4.2b) is built by walking `parent_id` links and collecting `lesson` from each ancestor"

So each candidate has one lesson (singular), but the adapter receives an array of lessons from ancestors (plural). This is correct semantically but could be clearer.

**Suggested fix:**
Clarify in §6 Candidate struct:
```rust
pub struct Candidate {
    // ...
    pub lesson: Option<String>,  // Single distilled insight from this candidate's reflection
}

// The adapter's mutate function receives ancestor_lessons: &[String], which is the
// chain of lessons from parent → grandparent → ... up to max_lesson_depth (GOAL-4.2b).
```

And ensure all prose consistently uses:
- "lesson" when referring to a single candidate's insight
- "ancestor lessons" or "lesson chain" when referring to the accumulated array

---

### FINDING-M-8: [Check #6] Data flow — Candidate ID assignment unclear
**Location:** §5 Data Flow, step 7

Step 7 states:
> `adapter.mutate(...)` → new `Candidate` with next monotonic ID.

But who assigns the ID? Three possibilities:
1. The adapter assigns the ID (bad — breaks monotonicity guarantee if adapter is buggy)
2. The engine assigns the ID after receiving the candidate from adapter (good — engine owns ID generation)
3. The ID is passed to the adapter, which must use it (awkward — leaks engine internals to adapter)

The §3.4 statement "Candidate is not generic — it stores text parameters" and §6 showing `Candidate` fields as `pub` suggests adapters construct candidates directly. If so, how does the engine enforce monotonic IDs (GUARD-2)?

**Suggested fix:**
Clarify in §5 step 7:
```
7. **Mutate** — Build ancestor lesson chain (walk lineage, truncate to max_lesson_depth).
   Call `adapter.mutate(...)` → returns a "raw" candidate (parameters, reflection, lesson).
   Engine wraps this in a Candidate struct with:
   - next monotonic ID (incremented counter in GEPAState)
   - parent_id set to selected parent's ID
   - generation = parent.generation + 1
   - created_at = SystemTime::now()
   Emit MutationCompleted.
```

And update §6 to show two types:
```rust
// Adapter returns this:
pub struct CandidateData {
    pub parameters: HashMap<String, String>,
    pub reflection: Option<String>,
    pub lesson: Option<String>,
}

// Engine wraps it in this:
pub struct Candidate {
    pub id: CandidateId,  // Assigned by engine
    pub parameters: HashMap<String, String>,
    pub parent_id: Option<CandidateId>,  // Assigned by engine
    pub generation: u32,  // Assigned by engine
    pub reflection: Option<String>,
    pub lesson: Option<String>,
    pub created_at: SystemTime,  // Assigned by engine
}
```

This makes the ownership boundary explicit.

---

### FINDING-M-9: [Check #15] Configuration — minibatch_size not shown as configurable
**Location:** §5 step 3, §7 Dependencies

Step 3 references `minibatch_size`:
> `EpochSampler` draws `minibatch_size` examples using seeded RNG.

But `GEPAConfig` is never fully defined in the master doc. Section §6 shows partial API but doesn't list config fields. Section §4 references "Configuration (requirements-07)" but doesn't enumerate fields in the master doc.

The design principle (§3.3) emphasizes determinism and configurability, yet the master doc doesn't show which values are configurable vs hardcoded.

**Suggested fix:**
Add a new subsection §6.1 "Configuration Fields (Summary)" that lists the fields in `GEPAConfig` with brief descriptions:
```rust
pub struct GEPAConfig {
    // Stopping criteria
    pub max_iterations: u64,
    pub time_budget: Option<Duration>,
    pub stagnation_limit: u64,
    pub max_consecutive_skips: u64,
    
    // Pareto front
    pub pareto_max_size: usize,
    pub selection_method: SelectionMethod,
    
    // Evaluation
    pub minibatch_size: usize,
    pub re_eval_interval: u64,
    pub adapter_timeout: Duration,
    pub adapter_max_retries: u32,
    
    // Merge
    pub merge_enabled: bool,
    pub merge_interval: u64,
    
    // Ancestry
    pub max_lesson_depth: usize,
    
    // Checkpointing
    pub checkpoint_interval: u64,
    pub checkpoint_path: PathBuf,
    
    // Determinism
    pub rng_seed: Option<u64>,
}
```

Note: Detailed validation rules live in feature design 07, but the master should list the fields for overview.

---

### FINDING-M-10: [Check #9] Integer overflow — iteration counter unbounded
**Location:** §5 Data Flow loop, GEPAState

The design shows:
- §6: `GEPAResult { pub total_iterations: u64, ... }`
- §5: Loop iterates with counter `i`
- §4: "`GEPAState`: ... iteration counter"

But there's no explicit check for `u64` overflow on the iteration counter. With `max_iterations` also a `u64`, if both are set to `u64::MAX`, the loop could theoretically overflow.

In practice, this is unlikely (2^64 iterations would take millennia), but per the review rule "Check arithmetic with concrete values", we should verify:
- Is there a guard against `iteration_counter + 1` overflow?
- Is `iteration_counter < max_iterations` checked before increment?

**Suggested fix:**
Add a note in §5 or §3.1 that iteration counter overflow is impossible in practice:
```
Note: The iteration counter is u64, allowing up to 2^64 iterations. With typical 
LLM latency (1-10s per iteration), reaching overflow would take >10^12 years, 
far exceeding any practical time_budget. No explicit overflow check is needed.
However, the loop checks stopping criteria BEFORE incrementing the counter, 
ensuring counter < max_iterations at all increments.
```

Or add an explicit `saturating_add` in the pseudocode if paranoia is warranted.

---

### FINDING-M-11: [Check #21] Ambiguous — what's in "statistics"?
**Location:** §6 GEPAResult

The `GEPAResult` struct includes:
```rust
pub statistics: GEPAStatistics,
```

But `GEPAStatistics` is never defined in the master design. Two engineers would implement different fields:
- Total candidates evaluated?
- Accept/reject ratios?
- Average score improvement per generation?
- Time spent in adapter calls vs engine overhead?

The requirements master mentions GOAL-9.1 "Statistics tracking" but doesn't enumerate fields in the master doc.

**Suggested fix:**
Add a struct definition in §6:
```rust
pub struct GEPAStatistics {
    pub total_candidates_evaluated: u64,
    pub total_accepted: u64,
    pub total_rejected: u64,
    pub total_skipped: u64,
    pub pareto_front_size_history: Vec<usize>,
    pub average_score_by_generation: HashMap<u32, f64>,
    pub total_adapter_time: Duration,
    pub total_engine_time: Duration,
    pub epochs_completed: u64,
}
```

Or reference: "See feature design 06 (State) §X for GEPAStatistics fields."

---

### FINDING-M-12: [Check #18] Trade-offs — merge proposer decision lacks alternatives
**Location:** §1 Key design trade-offs table, §4 Feature Index item 4

The design mentions "optional merge proposer (periodic, GOAL-7.7)" but this decision is not in the trade-offs table. What are the alternatives?
- Always merge (too expensive?)
- Never merge (lose diversity?)
- Merge periodically (chosen)

And why is merge *optional* rather than always-on? The trade-offs table documents other decisions clearly, but merge is only mentioned in passing.

**Suggested fix:**
Add a row to the §1 trade-offs table:
```markdown
| Merge proposer | Optional, periodic | Merge improves diversity but doubles adapter calls; optional allows consumers to disable if LLM budget is tight; periodic (every N iterations) balances exploration vs cost |
```

---

### FINDING-M-13: [Check #19] Cross-cutting concern — security not addressed
**Location:** §3 Cross-Cutting Concerns

The design thoroughly covers:
- Error handling (§3.1)
- Async design (§3.2)
- Determinism (§3.3)
- Logging/tracing (§3.6)
- Performance (§3.10)
- Memory growth (§3.11)

But **security** is never mentioned:
- Input validation: What if `Example.input` contains malicious JSON?
- Adapter sandboxing: Can a malicious adapter corrupt engine state?
- Checkpoint integrity: Can checkpoints be tampered with?
- Resource limits: Can a rogue adapter consume unbounded memory in a reflection?

For a library that delegates to user-provided adapters (which may call external LLMs), security boundaries matter.

**Suggested fix:**
Add §3.13 Security Considerations:
```markdown
### 3.13 Security Considerations

**Threat model:** The engine trusts the adapter implementation but not the LLM responses. 
Adapters are considered part of the trusted computing base; malicious adapter code can 
bypass all safety guarantees.

**Input validation:**
- `Example.input` is treated as opaque `serde_json::Value`; no sanitization applied
- Consumers must validate inputs before passing to engine
- String length limits not enforced by engine (adapter's responsibility)

**Checkpoint integrity:**
- Checkpoints use standard JSON serialization (serde_json)
- No encryption or signing; consumers must secure checkpoint storage
- Tampered checkpoints cause deserialization errors (fail-safe)

**Resource exhaustion:**
- Adapter timeouts (GOAL-8.4) prevent unbounded LLM call duration
- No memory limits on reflection/lesson strings (adapter must enforce)
- Pareto front size capped (GUARD-7), but candidate parameter size unbounded

**Out of scope:**
- Sandboxing malicious adapters (consumer's responsibility)
- Encrypting sensitive prompts in memory or checkpoints
- Rate limiting adapter calls to external services (adapter's responsibility)
```

---

### FINDING-M-14: [Check #22] Missing helper — ScoreWarning event undefined
**Location:** §3.9 Score Semantics

The text states:
> `f64::INFINITY` is clamped to `f64::MAX`; `f64::NEG_INFINITY` is clamped to `f64::MIN`. 
> A `ScoreWarning` event is emitted on each clamped value.

But `ScoreWarning` is not in the documented event types. Section §4 Feature Index item 9 says:
> `GEPAEvent` enum (16 variants)

But the master doc never lists those 16 variants. Is `ScoreWarning` one of them? If so, what's its structure?

**Suggested fix:**
Either:
1. Add a §6.2 "Event Types (Summary)" listing all 16 `GEPAEvent` variants, OR
2. Reference feature design 09: "See design-09-events.md for full event enum", OR
3. Correct the §3.9 text to reference an existing event type (e.g., `IterationSkipped` with a warning reason)

---

## 🟢 Minor (can fix during implementation)

### FINDING-M-15: [Check #4] Naming inconsistency — "Pareto front" vs "pareto front" vs "ParetoFront"
**Location:** Throughout document

The design uses three capitalization styles for the same concept:
- "Pareto front" (capitalized P, prose)
- "pareto front" (lowercase, prose)
- "ParetoFront" (type name)

Examples:
- §1: "managing the Pareto front" (capital P)
- §5 step 9: "insert into front" (lowercase f)
- §2 diagram: "Pareto Front (2)" (capital P)
- §3.8: "Pareto front invariant" (capital P)
- §6: "pub pareto_front: Vec<Candidate>" (lowercase field name, which is correct Rust style)

**Suggested fix:**
Standardize prose to always use "Pareto front" (capitalized) since it's a proper noun (named after economist Vilfredo Pareto). Type names remain `ParetoFront` per Rust conventions, field names remain `pareto_front`.

---

### FINDING-M-16: [Check #20] Abstraction level — §5 mixes high-level and low-level details
**Location:** §5 Data Flow

Section §5 "Data Flow — One Complete Iteration" provides a 13-step walkthrough. Some steps are high-level:
- Step 2: "Emit `IterationStarted`" (clear, no detail)
- Step 4: "Select — ... Round-robin with overfitting-delta deprioritization" (algorithm mentioned but not specified)

Other steps dive into implementation details:
- Step 1: "Check cancellation token (GOAL-3.7); if cancelled, terminate with `Cancelled`. If `Instant::elapsed() ≥ time_budget`..."
- Step 8: "Sanitize per GUARD-10 (NaN→None, ±Inf→clamp). Write to eval cache."

This inconsistency makes it unclear whether §5 is:
- A high-level architectural overview (in which case, step 1 and 8 are too detailed), OR
- A pseudocode specification (in which case, step 4 is too vague)

**Suggested fix:**
Decide on the abstraction level for §5. If it's an **architectural overview**, simplify:
```
1. **Pre-flight checks** — Verify cancellation token and time budget. Terminate if exceeded.
```

If it's **pseudocode**, make step 4 concrete:
```
4. **Select** — Call `ParetoFront::select(&mut rng)`:
   - Round-robin through front candidates
   - Deprioritize candidates with overfitting_delta > threshold
   - Return selected parent ID
   - If front is empty, return EmptyFrontError
```

---

### FINDING-M-17: [Check #10] Option handling — parent_id unwrap potential
**Location:** §5 step 7, §6 Candidate struct

The `Candidate` struct has:
```rust
pub parent_id: Option<CandidateId>,
```

Step 7 says:
> Build ancestor lesson chain (walk lineage, truncate to `max_lesson_depth`)

Walking lineage requires following `parent_id` links. If any candidate in the chain has `parent_id = None`, the walk terminates. This is correct for seed candidates (generation 0), but the logic should be explicit to avoid `.unwrap()` bugs.

**Suggested fix:**
Add a note in §5 step 7 or §3 that clarifies:
```
Ancestor lesson chain construction:
- Start with current candidate
- While parent_id.is_some() and depth < max_lesson_depth:
  - Look up parent in candidate registry
  - If parent.lesson.is_some(), append to chain
  - Set current = parent
- If parent_id.is_none(), stop (reached a seed candidate)
```

This makes explicit that `parent_id = None` is the termination condition, not an error.

---

### FINDING-M-18: [Check #23] Dependency assumption — tokio runtime provided by consumer
**Location:** §7 Dependency Choices

The text states:
> `tokio` (feature: `time`) | `tokio::time::timeout` for adapter calls (GOAL-8.4) | Minimal surface; **consumer provides runtime**

But the async design (§3.2) says:
> `GEPAEngine::run()` is `async fn` returning a `Send` future (GUARD-11).

How does the engine call `tokio::time::timeout` without a tokio runtime? Three possibilities:
1. Consumer must have tokio runtime active (dependency assumption)
2. Engine spawns minimal tokio runtime internally (violates "no runtime managed")
3. Timeout logic is runtime-agnostic (but tokio::time isn't)

The current design has an unstated assumption: **consumer must use tokio**. This conflicts with "runtime-agnostic, tokio for timeouts only" in §1 trade-offs table.

**Suggested fix:**
Clarify in §3.2 or §7:
```
The engine uses tokio::time::timeout, which requires a tokio runtime. Consumers must 
either:
1. Run GEPAEngine::run() within a tokio runtime (e.g., #[tokio::main]), OR
2. Use tokio::runtime::Builder to create a runtime before calling run()

Alternative: To support non-tokio runtimes, the engine could accept a generic timeout 
function: `Box<dyn Fn(Future, Duration) -> Result<T, Timeout>>`. This would allow 
consumers to provide their own timeout mechanism. However, this adds complexity for a 
rare use case; we assume tokio runtimes are acceptable for all consumers.
```

---

### FINDING-M-19: [Check #2] References to feature design docs — design-v2.md not explained
**Location:** §4 Feature Index, file listing

The master doc references 9 feature design docs (design-01 through design-09). File listing shows:
```
design-01-core-engine.md
design-02-pareto-front.md
...
design-09-events.md
design-v2.md  ← What is this?
```

The `design-v2.md` file exists but is never referenced in the master doc. Is it:
- An obsolete version (should be deleted)?
- A future revision plan (should be marked as draft)?
- An alternative architecture proposal (should be documented in the master)?

**Suggested fix:**
Either:
1. If `design-v2.md` is obsolete, delete it or move to `.gid/features/gepa-core/archive/`
2. If it's a future revision, rename to `design-v2-DRAFT.md` and add a note in the master
3. If it contains valuable alternative proposals, reference it in §1 or create an "Alternatives Considered" appendix

---

### FINDING-M-20: [Check #25] Testability — no mention of test strategy
**Location:** §3 Cross-Cutting Concerns (missing section)

The master design thoroughly covers error handling, async design, determinism, logging, performance, and memory. But it never discusses **testability**:
- How to unit-test the engine without real LLM calls?
- Are there mock adapters for testing?
- Can individual components (Pareto front, eval cache) be tested in isolation?
- What's the testing strategy for determinism (GUARD-9)?

The review rule states: "Can the core logic be unit-tested in isolation? Is the design structured so that tests don't need complex setup or mocking?"

**Suggested fix:**
Add §3.14 Testing Strategy:
```markdown
### 3.14 Testing Strategy

**Unit testing approach:**
- Pure algorithmic components (Pareto front, eval cache, candidate registry) have 
  zero dependencies and can be tested with simple fixtures
- Engine loop can be tested with a `MockAdapter` that returns deterministic results
- Determinism (GUARD-9) verified by running the same seed twice and asserting 
  identical final state

**Mock adapter:**
```rust
struct MockAdapter {
    execute_results: Vec<Vec<ExecutionTrace>>,
    reflect_results: Vec<Reflection>,
    mutate_results: Vec<Candidate>,
    evaluate_results: Vec<Vec<f64>>,
}
```
Returns pre-programmed results in sequence; used for integration tests.

**Property-based testing:**
- Pareto front invariants (GUARD-1) verified with proptest on random insert sequences
- Score sanitization (GUARD-10) tested with fuzzing (NaN, ±Inf, subnormal inputs)

**Benchmarks:**
- Engine overhead (GUARD-6) measured by comparing engine-internal time vs mock 
  adapter call duration
- Memory growth (GUARD-7) measured by tracking heap size over 10k iterations
```

---

---

## 📋 Path Traces (for state machines / workflows)

This design doesn't have a traditional state machine with explicit states, but the iteration loop is a workflow. Tracing key paths:

### Happy path (successful iteration):
```
Start → Check time/cancellation → Sample minibatch → Select parent → 
Execute (success) → Reflect (success) → Mutate (success) → Evaluate (success) → 
Accept (dominance check passes) → Update Pareto front → 
Emit CandidateAccepted → Check stopping criteria (not met) → Next iteration ✅
```

### Failure path 1 (adapter error, skip after retries):
```
Start → Sample minibatch → Select parent → Execute (adapter error, retryable=true) → 
Retry #1 (fail) → Retry #2 (fail) → Retry #3 (fail) → Retry exhausted → 
Emit IterationSkipped → Increment skip counter → Check max_consecutive_skips → 
(not exceeded) → Next iteration ✅
```

### Failure path 2 (too many consecutive skips):
```
... → IterationSkipped → skip_counter = max_consecutive_skips → 
Emit TerminationReason::TooManySkips → Halt run ✅
```

### Failure path 3 (candidate rejected, stagnation):
```
... → Evaluate (success) → Accept check (dominated by parent) → 
Emit CandidateRejected → Increment stagnation counter → 
(stagnation_counter reaches 50% of limit) → Emit StagnationWarning → 
(continues iterations) → (reaches 100%) → Emit TerminationReason::Stagnation → Halt ✅
```

### Edge case 1 (cancellation mid-iteration):
```
Start → Check cancellation (not cancelled) → Sample minibatch → Select parent → 
Execute (in progress) → [User triggers cancellation] → 
(Cancellation check before reflect) → Emit TerminationReason::Cancelled → Halt ✅
```

### Edge case 2 (time budget exceeded):
```
Start → Check time (elapsed < budget) → Sample minibatch → ... → Evaluate → 
Check time (elapsed ≥ budget) → Emit TerminationReason::TimeBudget → Halt ✅
```

### Edge case 3 (all seeds fail):
```
Engine::run() → Initialize with seed candidates → Evaluate seeds on initial minibatch → 
All evaluations fail or return NaN → Emit GEPAError::AllSeedsFailed → 
run() returns Err(AllSeedsFailed) ✅
```

---

## ✅ Passed Checks

### Phase 0: Document Size
- **Check #0: Document size** ✅ — Master design has 0 component definitions (§3.x). All 9 components are delegated to feature design docs (design-01 through design-09). The master is purely architectural overview + cross-cutting concerns, which is the correct use of a master design doc.

### Phase 1: Structural Completeness
- **Check #1: Types fully defined** ⚠️ — FINDING-M-1 (ExecutionTrace.asi), FINDING-M-8 (Candidate ID assignment), FINDING-M-11 (GEPAStatistics undefined). Otherwise, types like `GEPAError`, `Candidate`, `Example`, `Reflection`, `GEPAResult` have complete fields.

- **Check #2: References resolve** ⚠️ — FINDING-M-2 (§3.7 missing), FINDING-M-19 (design-v2.md unexplained). Otherwise, references like "(feat 1)", "(GOAL-2.3)", "(GUARD-5)" resolve correctly to requirements or feature design docs.

- **Check #3: No dead definitions** ⚠️ — FINDING-M-6 (GEPAError::ValidationError never used). Otherwise, all error types, traits, and data structures are referenced in §5 data flow or §3 cross-cutting concerns.

- **Check #4: Consistent naming** ⚠️ — FINDING-M-7 (lesson vs lessons), FINDING-M-15 (Pareto front capitalization). Otherwise, naming is consistent (CandidateId, ExampleId always u64; trait names use GEPAAdapter, DataLoader consistently).

### Phase 2: Logic Correctness
- **Check #5: State machine invariants** ⚠️ — FINDING-M-3 (unbounded retry policy), FINDING-M-4 (empty front deadlock). Otherwise, the iteration loop has clear termination conditions (max_iterations, time_budget, stagnation, too_many_skips, cancelled), no unreachable states detected.

- **Check #6: Data flow completeness** ⚠️ — FINDING-M-8 (Candidate ID assignment unclear). Otherwise, data flows are complete: minibatch → execute → traces → reflect → reflection → mutate → child → evaluate → scores → accept/reject. Every output is an input to the next step.

- **Check #7: Error handling completeness** ✅ — All adapter calls return `Result<T, GEPAError>`. Error flow (§3.1) specifies: retry policy → skip → halt. No silent error swallowing detected. Cancellation is checked at multiple points (§5 step 1, before each adapter call).

### Phase 3: Type Safety & Edge Cases
- **Check #8: String operations** ✅ — No unsafe string slicing detected. The design treats `Candidate.parameters` as opaque `HashMap<String, String>` and `Example.input` as `serde_json::Value`. No `&s[..n]` or substring operations in the documented logic. UTF-8 safety appears maintained.

- **Check #9: Integer overflow** ⚠️ — FINDING-M-10 (iteration counter overflow discussion). Otherwise, no unchecked arithmetic like `retries + 1` detected. Counters (skip, stagnation) have explicit max checks before increments.

- **Check #10: Option/None handling** ⚠️ — FINDING-M-17 (parent_id walk termination). Otherwise, §3.1 explicitly states "No `.unwrap()` in library code (GUARD-8)". The design uses `Option<CandidateId>`, `Option<Duration>`, `Option<String>` consistently.

- **Check #11: Match exhaustiveness** ✅ — The design doesn't show match statements with catch-all branches in the master doc. Error enum (§3.1) has 13 explicit variants covering all anticipated failures. The master doc delegates detailed logic to feature designs.

- **Check #12: Ordering sensitivity** ✅ — The design specifies deterministic ordering: RNG seed → minibatch sampling → Pareto selection → tie-breaking. No order-dependent match chains detected in master doc. Sequential loop (§3.2) eliminates race conditions.

### Phase 4: Architecture Consistency
- **Check #13: Separation of concerns** ✅ — Clean separation: pure algorithmic logic (Pareto front, eval cache) is stateless; IO (LLM calls) is delegated to adapters; engine orchestrates but never calls LLMs. §2 architecture diagram clearly shows the boundary: "dyn GEPAAdapter" is the sole IO interface.

- **Check #14: Coupling** ✅ — Events carry observed data, not derived state. Example: `CandidateSelected` would carry the candidate ID, not the full candidate object. Eval cache (§3.1) stores scores separately from candidates, maintaining immutability (GUARD-2).

- **Check #15: Configuration vs hardcoding** ⚠️ — FINDING-M-9 (minibatch_size and other config fields not enumerated). Otherwise, the design emphasizes configuration: "Language-specific values, paths, commands, thresholds: are they configurable" → the master doc implies all thresholds are in `GEPAConfig`, but doesn't enumerate them.

- **Check #16: API surface** ✅ — §6 Public API Surface shows minimal types: `GEPAEngineBuilder`, `GEPAEngine`, `Candidate`, `Example`, `ExecutionTrace`, `Reflection`, `GEPAResult`. Internal types (eval cache, candidate registry, epoch sampler) are not exposed. API is clean and minimal.

### Phase 5: Design Doc Quality
- **Check #17: Goals and non-goals explicit** ⚠️ — FINDING-M-5 (non-goals missing). Goals are implicit via GOAL-X.Y references to requirements master. Non-goals are not documented.

- **Check #18: Trade-offs documented** ⚠️ — FINDING-M-12 (merge proposer decision lacks alternatives). Otherwise, §1 "Key design trade-offs" table is excellent, documenting 6 major decisions with chosen option and rationale. This is a model for design docs.

- **Check #19: Cross-cutting concerns** ⚠️ — FINDING-M-13 (security not addressed), FINDING-M-20 (testability not addressed). Otherwise, §3 thoroughly covers error handling, async design, determinism, logging, performance, memory. Missing: security, testing strategy, migration/rollback.

- **Check #20: Appropriate abstraction level** ⚠️ — FINDING-M-16 (§5 mixes high-level and low-level details). The master doc is generally at the right level: architectural overview, not pseudocode. But §5 has inconsistencies.

### Phase 6: Implementability
- **Check #21: Ambiguous prose** ⚠️ — FINDING-M-1 (ExecutionTrace.asi), FINDING-M-11 (GEPAStatistics), FINDING-M-8 (Candidate ID assignment). These sections would lead to different implementations by different engineers.

- **Check #22: Missing helpers** ⚠️ — FINDING-M-14 (ScoreWarning event undefined). The master doc references helpers (EpochSampler, ParetoFront::select, adapter methods) but correctly delegates details to feature designs. One exception: events are referenced but not enumerated.

- **Check #23: Dependency assumptions** ⚠️ — FINDING-M-18 (tokio runtime assumption). Otherwise, dependencies are explicitly listed in §7 with justifications. External LLM APIs are explicitly excluded (GUARD-5).

- **Check #24: Migration path** ✅ — Not applicable. This is a new crate, not replacing existing code. The design doc correctly focuses on the target architecture, not migration.

- **Check #25: Testability** ⚠️ — FINDING-M-20 (testing strategy not documented). The design is testable (pure logic, dependency injection via adapters), but testability is not explicitly discussed.

### Phase 7: Existing Code Alignment
- **Check #26: Similar functionality exists?** ✅ — Not applicable for review of a design doc. This check applies during implementation. The design doc assumes a greenfield implementation.

- **Check #27: API compatibility** ✅ — New crate, no existing API to break. The design doc correctly uses semantic versioning assumptions (no 1.0 release yet, breaking changes expected).

- **Check #28: Feature flag / gradual rollout** ✅ — Not applicable for a library crate. Consumers control rollout by choosing when to integrate gepa-core. The optional merge proposer (GOAL-7.7) provides a built-in feature flag for that specific feature.

---

## Summary

**Findings by severity:**
- 🔴 Critical: 4 (blocks implementation)
- 🟡 Important: 10 (should fix before implementation)
- 🟢 Minor: 6 (can fix during implementation)

**Total findings:** 20

**Recommendation:** **Needs fixes before implementation**

### Critical blockers:
1. **FINDING-M-1**: `ExecutionTrace.asi` field is undefined — clarify or remove
2. **FINDING-M-2**: Missing §3.7 section — fix numbering or add section
3. **FINDING-M-3**: Retry policy is unbounded — add explicit bounds and backoff strategy
4. **FINDING-M-4**: Empty Pareto front could cause deadlock — add explicit error handling

### Key improvements needed:
- Define all referenced types (`GEPAStatistics`, `ScoreWarning` event)
- Enumerate `GEPAConfig` fields in master doc (or reference feature design clearly)
- Add non-goals section to prevent scope creep
- Clarify Candidate ID assignment ownership boundary
- Document security considerations and testing strategy
- Fix naming inconsistencies (lesson/lessons, Pareto front capitalization)

### Strengths:
✅ Excellent trade-offs table documenting design decisions
✅ Clear separation of concerns (pure logic vs adapter IO)
✅ Comprehensive cross-cutting concerns coverage (error handling, determinism, async, logging)
✅ Clean API surface with minimal exposed types
✅ Strong commitment to determinism (GUARD-9) and immutability (GUARD-2)
✅ Well-structured delegation to feature design docs

### Estimated implementation confidence: **Medium**

**Rationale:** The overall architecture is sound and well-documented. The critical findings are specification gaps (undefined fields, missing error handling) rather than fundamental design flaws. Once the 4 critical blockers are resolved, implementation can proceed with high confidence. The important findings (statistics fields, config enumeration, security) can be addressed incrementally during implementation, but should be documented before coding begins to avoid divergent implementations across features.

### Next steps:
1. Address FINDING-M-1 through FINDING-M-4 (critical blockers)
2. Review and apply FINDING-M-5 through FINDING-M-14 (important improvements)
3. Consider FINDING-M-15 through FINDING-M-20 during implementation
4. Validate feature design docs (design-01 through design-09) for consistency with master
