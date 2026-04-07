# Review: GEPA Design Documents 07, 08, 09

**Documents Reviewed:**
- `design-07-config.md` (Configuration)
- `design-08-data-loading.md` (Data Loading)
- `design-09-events.md` (Events/Observability)

**Review Date:** 2026-04-06  
**Reviewer:** Claude (Subagent via review-design skill)

---

## 🔴 Critical (blocks implementation)

### FINDING-7-1: **[Check #7] Retry backoff overflow without bounds**

**Location:** design-07-config.md, §3.2 BackoffStrategy + §5 Validation Logic

**Issue:** The exponential backoff calculation `base_delay * 2^attempt` has no explicit saturation check before comparing to `max_retry_delay`. If `base_delay=1s` and `attempt=64`, the intermediate result (`1 << 64` nanoseconds) will wrap in u64 arithmetic before the min() is applied.

**Trace:**
```
attempt=0 → delay = 1s * 2^0 = 1s ✅
attempt=30 → delay = 1s * 2^30 = ~1 billion seconds (wrapped?) ⚠️
attempt=64 → delay = 1s * 2^64 → integer overflow ❌
```

The design says "returns the smaller of (base × 2^attempt) and max_delay" but doesn't specify overflow handling. The validation only checks `base_delay > Duration::ZERO when retry_max > 0 and strategy=Exponential`, not whether `base_delay * 2^retry_max` will overflow.

**Suggested fix:**

In §3.2 BackoffStrategy::compute_delay pseudocode:
```rust
Exponential => {
    // Saturating arithmetic: clamp at u64::MAX before converting to Duration
    let exponent = attempt.min(63); // cap exponent to prevent overflow
    let multiplier = 1u64.saturating_shl(exponent);
    let computed = self.base_delay.saturating_mul(multiplier);
    computed.min(self.max_retry_delay)
}
```

In §5 Validation Logic, add check:
```rust
if config.backoff_strategy == BackoffStrategy::Exponential 
    && config.retry_max > 0 {
    if config.base_delay.as_secs() > 0 
        && (config.base_delay.as_secs().leading_zeros() < config.retry_max as u32) {
        return Err("exponential backoff would overflow: reduce base_delay or retry_max");
    }
}
```

Or simpler: Document that `attempt` is capped at 63 in the computation to prevent shift overflow.

---

### FINDING-8-1: **[Check #5] State machine: epoch boundary wraparound undefined for edge case**

**Location:** design-08-data-loading.md, §3.2 MinibatchSampler, "Epoch Wraparound"

**Issue:** The design says "When fewer than minibatch_size examples remain... concatenate remaining + next epoch" but doesn't specify what happens when `training_set_size < minibatch_size` **and** the current position wraps. 

**Concrete trace:**
```
Training set size: 10
Minibatch size: 16
Current position: 8

Remaining in current epoch: 10 - 8 = 2
Next epoch needs: 16 - 2 = 14
But training set only has 10 examples!

Does it:
a) Fill with [8,9] + [0,1,2,3,4,5,6,7,8,9] (20 total) then take first 16? ❓
b) Fill with [8,9] + [0,1,2,3,4,5,6,7,8,9] + [0,1,2,3] (wrap twice)? ❓
c) Return [8,9] + all 10 shuffled = actual batch size 12? ✅
```

The requirements (GOAL-8.3) say "actual batch size = min(minibatch_size, training_set_size)" which suggests (c), but the design's "fill the minibatch by concatenating" language is ambiguous. Two engineers could implement differently.

**Suggested fix:**

In §3.2 MinibatchSampler, replace "Epoch Wraparound" paragraph with:
```
**Epoch wraparound:** When fewer than `minibatch_size` examples remain in the 
current epoch:
1. Take all remaining examples from current epoch
2. Increment epoch counter and shuffle the pool with RNG
3. Take additional examples from the new epoch until either:
   - Total collected == minibatch_size, OR
   - We've taken all examples in the new epoch (when training_set_size < minibatch_size)
4. Return the concatenated batch (size may be < minibatch_size if training_set_size < minibatch_size)

This ensures that when training_set_size < minibatch_size, each "minibatch" is 
actually the full training set, reshuffled each iteration. No example duplication 
within a single batch.
```

Add to §5 Edge Cases:
```rust
// Edge case: training_set_size=10, minibatch_size=16
let batch = sampler.next_batch(); 
assert_eq!(batch.len(), 10); // not 16
// Next call will return another shuffled copy of all 10
```

---

### FINDING-9-1: **[Check #11] Match exhaustiveness: catch-all in callback panic handling**

**Location:** design-09-events.md, §3.2 EventDispatcher, emit() pseudocode

**Issue:** The panic handling uses a catch-all pattern without verifying all remaining panic types are correctly handled:

```rust
let result = std::panic::catch_unwind(AssertUnwindSafe(|| callback(&event)));
if let Err(e) = result {
    tracing::warn!("Callback panicked: {:?}", e);
    // continue with next callback
}
```

The `Err(e)` branch silently swallows **all** panic types. Rust's `catch_unwind` doesn't catch all panics (e.g., panics that abort the process). The design should clarify what happens if:
- A callback calls `std::process::abort()` (not catchable)
- A callback panics with a non-Send payload (rare but possible)
- A callback spawns a thread that panics (not caught by this wrapper)

**Suggested fix:**

In §3.2 EventDispatcher, replace the pseudocode with:
```rust
for callback in &self.handlers[&event_type] {
    // Wrap in panic boundary. Only catches unwinding panics; aborts will 
    // terminate the process per Rust semantics.
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| callback(&event)));
    if let Err(panic_payload) = result {
        // Best-effort debug output. The payload may not be Send or Display.
        let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_payload.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("Non-string panic payload: {:?}", panic_payload)
        };
        tracing::warn!(
            event_type = ?event.variant_name(),
            "Callback panicked: {}. Continuing with remaining callbacks.",
            msg
        );
        // Continue to next callback. The panicking callback remains registered.
    }
}
```

Add to §6 Error Handling:
```
**Unrecoverable panics:** If a callback calls `std::process::abort()` or triggers 
a panic that doesn't unwind (e.g., panic=abort build mode), the entire process 
terminates per Rust's panic semantics. `catch_unwind` only catches unwinding panics. 
Consumers must ensure callbacks don't abort.
```

---

## 🟡 Important (should fix before implementation)

### FINDING-7-2: **[Check #1] Incomplete type definition: ErrorPolicy**

**Location:** design-07-config.md, §3.4 ErrorPolicy

**Issue:** `ErrorPolicy` enum is defined with two variants (Skip, Halt) but is never used in any subsequent pseudocode or state transition. The design mentions "error policy (`ErrorPolicy::Skip` vs `ErrorPolicy::Halt`, default: Skip)" in §1 but doesn't show how the engine consumes this field during error handling.

**Missing:** Data flow from `GEPAConfig.error_policy` → engine loop error branch.

**Suggested fix:**

In §3.4, add a "Usage" section:
```
**Usage:** After adapter call exhausts retries (see §3.3 RetryPolicy), the engine 
checks `config.error_policy`:
- `ErrorPolicy::Skip`: Emit `IterationSkipped` event, increment skip counter, 
  continue to next iteration
- `ErrorPolicy::Halt`: Return `Err(GEPAError::AdapterError)` immediately, 
  terminating the run

Pseudocode:
```rust
match self.adapter.execute(...).retry(&self.retry_policy).await {
    Ok(traces) => { /* continue with reflect */ },
    Err(e) => {
        match self.config.error_policy {
            ErrorPolicy::Skip => {
                self.emit(IterationSkipped { reason: e.to_string(), ... });
                self.skip_counter += 1;
                if self.skip_counter >= self.config.max_consecutive_skips {
                    return Err(GEPAError::TooManySkips);
                }
                continue; // next iteration
            },
            ErrorPolicy::Halt => return Err(e),
        }
    }
}
```
```

---

### FINDING-7-3: **[Check #6] Data flow gap: merge_enabled but no merge_interval**

**Location:** design-07-config.md, §2.1 GEPAConfig struct

**Issue:** The struct has `merge_enabled: bool` but no `merge_interval: u64` field, yet the requirements (GOAL-7.7) explicitly specify "merge interval (every N iterations, default: 10)". The design's field list is incomplete.

**Verified fields present:**
- max_iterations ✅
- minibatch_size ✅
- merge_enabled ✅
- merge_selection_strategy ✅
- merge_interval ❌ (missing!)

**Suggested fix:**

In §2.1 GEPAConfig, add after `merge_enabled`:
```rust
pub merge_interval: u64,
```

In §4 Defaults, add:
```rust
merge_interval: 10,
```

In §5 Validation Logic, add:
```rust
if config.merge_enabled && config.merge_interval == 0 {
    return Err(ConfigError::MergeIntervalZero);
}
```

---

### FINDING-7-4: **[Check #15] Hardcoded timeout without configuration surface**

**Location:** design-07-config.md, §2.1 GEPAConfig struct

**Issue:** The requirements (GOAL-8.4) specify `data_loader_timeout_secs: u64` (default: 30) as part of the config, but this field is missing from the GEPAConfig struct definition. The data loading design (08) references "config.data_loader_timeout_secs" but it's not defined in the config design.

**Cross-reference check:**
- design-08-data-loading.md §2.1: "tokio::time::timeout(Duration::from_secs(config.data_loader_timeout_secs))"
- design-07-config.md §2.1: field list does not include `data_loader_timeout_secs`

This is a cross-document inconsistency: doc 08 assumes a field that doc 07 doesn't define.

**Suggested fix:**

In design-07-config.md §2.1 GEPAConfig, add:
```rust
pub data_loader_timeout_secs: u64,
```

In §4 Defaults:
```rust
data_loader_timeout_secs: 30,
```

No validation needed (any u64 value is valid; 0 means immediate timeout which is technically valid).

---

### FINDING-8-2: **[Check #7] Error handling incomplete: validation set timeout retry**

**Location:** design-08-data-loading.md, §4 Validation Runner, "Error handling"

**Issue:** The design says "If the adapter's evaluate call fails during validation, the same error/retry policy from GOAL-7.5 applies" but doesn't specify what happens if retries are exhausted during validation. Options:
a) Halt the entire run (validation is mandatory)
b) Skip the failed candidate and continue validating others
c) Mark validation as incomplete and return partial results

The requirements (GOAL-8.6) only say "validation evaluation emits progress events" and mention the retry policy, but don't specify the failure mode.

**Suggested fix:**

In §4 Validation Runner, replace "Error handling" paragraph with:
```
**Error handling:** If `adapter.evaluate()` fails during validation:
1. Apply the same retry policy from GOAL-7.5 (up to `config.retry_max` retries)
2. If retries are exhausted:
   - Emit `ValidationFailed { candidate_id, error }` event (add to GEPAEvent enum)
   - Mark the candidate's validation scores as `None` in `GEPAResult.validation_scores`
   - Continue validating remaining candidates
3. If all candidates fail validation, set `validation_skipped = true` in the result
   (same as if validation_examples() returned empty)
4. Return `GEPAResult` with partial validation data (some candidates may have scores,
   others may be `None`)

This allows the run to complete even if validation partially fails, but the consumer
can inspect which candidates validated successfully.
```

Add to GEPAEvent enum in design-09-events.md:
```rust
ValidationFailed {
    candidate_id: CandidateId,
    error: String,
},
```

---

### FINDING-8-3: **[Check #21] Ambiguous: "sparsest score coverage" tie-breaking**

**Location:** design-08-data-loading.md, §3.3 ScoreBackfill, "Candidate selection"

**Issue:** The design says "Select N candidates with the sparsest coverage (fewest evaluated examples; ties broken by candidate age — newest first)". But "newest first" is ambiguous when multiple candidates have the same age (e.g., accepted in the same iteration due to merge or simultaneous non-dominance).

**Concrete scenario:**
```
Iteration 10:
- Candidate A accepted (age=10, coverage=5)
- Candidate B accepted (age=10, coverage=5) via merge
- Candidate C accepted (age=9, coverage=6)

Who gets selected for backfill when N=1?
A or B? (same age and coverage)
```

**Suggested fix:**

In §3.3 ScoreBackfill, replace "ties broken by candidate age — newest first" with:
```
Ties broken by:
1. Candidate age (newest first = highest iteration number)
2. If age is equal, by candidate ID (lowest ID first, for determinism per GUARD-9)

Pseudocode:
```rust
candidates.sort_by_key(|c| {
    let coverage = eval_cache.coverage(c.id);
    (coverage, Reverse(c.created_at_iteration), c.id)
});
let selected = candidates.into_iter().take(N).collect();
```
```

---

### FINDING-9-2: **[Check #1] Dead definition: EventType enum never used**

**Location:** design-09-events.md, §3.1 EventType enum

**Issue:** The design defines `EventType` enum with 15 variants but never references it in any subsequent section. The registration API in §2.2 shows `engine.on_event(callback)` without any type parameter or EventType argument, suggesting the EventType enum is unused.

**Cross-check:** The requirements (GOAL-9.3) say "on_event(EventType, callback)" but the design's §2.2 shows:
```rust
builder.on_event(|event: &GEPAEvent| { ... })
```
This takes a closure, not an EventType discriminant.

**Two possible interpretations:**
a) The design is wrong — registration should filter by EventType: `on_event(EventType::IterationStarted, callback)`
b) The EventType enum is obsolete — registration is global and callbacks filter internally by matching on GEPAEvent

**Suggested fix (choose one):**

**Option A (EventType-specific registration):**

In §2.2 Builder API, change to:
```rust
pub fn on_event(mut self, event_type: EventType, callback: impl Fn(&GEPAEvent) + Send + Sync + 'static) -> Self {
    self.dispatcher.register(event_type, Box::new(callback));
    self
}
```

In §3.1, add usage note:
```
**Usage:** Consumers pass EventType to filter which events trigger the callback:
```rust
builder
    .on_event(EventType::IterationStarted, |ev| { /* only iteration starts */ })
    .on_event(EventType::CandidateAccepted, |ev| { /* only accepts */ })
```
```

**Option B (remove EventType, use global registration):**

Delete §3.1 EventType enum entirely. Change §3.2 EventDispatcher to:
```rust
pub struct EventDispatcher {
    handlers: Vec<Box<dyn Fn(&GEPAEvent) + Send + Sync>>,
}

impl EventDispatcher {
    pub fn emit(&self, event: GEPAEvent) {
        // Invoke all handlers regardless of event variant
        for handler in &self.handlers { /* ... */ }
    }
}
```

**Recommendation:** Option A aligns with GOAL-9.6 (zero overhead when no callbacks registered for a specific event type) and is more efficient. Option B requires every callback to match on all event variants.

---

### FINDING-9-3: **[Check #18] Missing trade-off: sync vs async callbacks**

**Location:** design-09-events.md, §1 Overview

**Issue:** The design states "Key trade-off: synchronous callback invocation... over async/channel-based dispatch" but doesn't document the rejected alternative's rationale beyond "keeps the implementation simple". 

What's missing:
- Why was async dispatch rejected? (Complexity? Runtime dependency? Ordering guarantees?)
- What are the concrete downsides of sync dispatch? (Blocking the loop is mentioned, but no mitigation strategies)
- Could callbacks be made async? (Would require the engine to await them)

**Suggested fix:**

In §1 Overview, expand the trade-off:
```
Key trade-off: synchronous callback invocation (callbacks run inline in the engine 
loop) over async/channel-based dispatch.

**Rationale for sync:**
- Preserves strict event ordering (async dispatch could reorder events if callbacks 
  have different completion times)
- Avoids channel allocation overhead (zero-cost when no callbacks registered, per GOAL-9.6)
- Simpler implementation (no spawning, no join handles, no cancellation logic)
- Natural backpressure (slow callbacks automatically slow the loop, making overhead visible)

**Rejected alternative: async dispatch**
- Callbacks would be `async fn` invoked via `tokio::spawn(callback(event.clone()))`
- Would allow long-running callbacks (e.g., network logging) without blocking the loop
- **Rejected because:**
  - Requires event cloning (current design uses `&GEPAEvent` borrows, cheaper)
  - Non-deterministic event delivery order when callbacks have different latencies
  - Harder to enforce GUARD-6 (5% overhead budget) — spawned tasks don't naturally 
    contribute to iteration timing
  - Callback panics would terminate background tasks silently instead of being caught inline

**Mitigation for sync blocking:** Callbacks that need to do expensive work should 
use an internal channel:
```rust
let (tx, rx) = mpsc::channel();
builder.on_event(move |ev| { tx.send(ev.clone()).ok(); });
// Spawn separate task that drains rx
```
```

---

### FINDING-8-4: **[Check #13] Separation of concerns: DataLoader timeout enforcement leaks into engine**

**Location:** design-08-data-loading.md, §2.1 DataLoader Trait, "Key Details"

**Issue:** The design says "The engine wraps each call with tokio::time::timeout" but timeout enforcement is not the trait's responsibility — it's a cross-cutting concern (retry policy, GOAL-7.5). This leaks engine-internal logic into the trait contract.

Better separation: the trait should specify timeout behavior (via Result), not require the engine to wrap every call.

**Suggested fix:**

In §2.1 DataLoader Trait, remove "The engine wraps each call with tokio::time::timeout" paragraph. Replace with:
```
**Timeout behavior:** Implementations SHOULD respect reasonable timeouts internally
(e.g., database query timeouts, HTTP client timeouts). If an operation exceeds
`config.data_loader_timeout_secs`, the implementation SHOULD return 
`Err(GEPAError::Timeout)` or `Err(GEPAError::AdapterError { retryable: true })`.

The engine applies its own timeout wrapper as a backstop:
```rust
tokio::time::timeout(
    Duration::from_secs(config.data_loader_timeout_secs),
    loader.training_examples()
).await
  .map_err(|_| GEPAError::Timeout)?
```

This ensures that even if the implementation doesn't enforce timeouts, the engine
will not hang indefinitely.
```

This clarifies that timeout enforcement is a defense-in-depth strategy, not a trait contract requirement.

---

## 🟢 Minor (can fix during implementation)

### FINDING-7-5: **[Check #4] Naming inconsistency: re_eval_interval vs checkpoint_interval**

**Location:** design-07-config.md, §2.1 GEPAConfig

**Issue:** Some interval fields use full word "interval" (`checkpoint_interval`, `re_eval_interval`) while the merge proposer docs (requirements GOAL-7.7) call it "merge interval" but the struct field (if added per FINDING-7-3) would be `merge_interval`. Consistent, but the requirements also use "re-evaluation" while the field is `re_eval_`. Pick one style.

**Suggested fix:** Document the abbreviation policy in a comment:
```rust
// Intervals are always in iterations (not wall-clock time).
// Some fields abbreviate for brevity (re_eval = re-evaluation).
pub re_eval_interval: u64,
pub checkpoint_interval: u64,
pub merge_interval: u64, // if merge_enabled
```

---

### FINDING-8-5: **[Check #20] Abstraction level: pseudocode shows tokio-specific syntax**

**Location:** design-08-data-loading.md, §2.1 DataLoader Trait

**Issue:** The trait signature uses `#[async_trait]` macro, which is a specific implementation detail (the `async-trait` crate). This leaks implementation choice into the design doc. A design doc should specify "async methods" without prescribing the exact mechanism.

**Suggested fix:** Add a note:
```
**Implementation note:** Async trait methods require either the `async-trait` crate
macro or native async trait support (stable in Rust 1.75+). The design is compatible
with both. Reference impl uses `#[async_trait]` for backward compatibility.
```

---

### FINDING-9-4: **[Check #4] Naming inconsistency: CandidateId vs candidate_id**

**Location:** design-09-events.md, §2.1 GEPAEvent enum

**Issue:** Event fields use `candidate_id: CandidateId` (type and field name clash). This is valid Rust but may confuse readers. Some events also have `candidate: Candidate` (full object) vs `candidate_id: CandidateId` (just ID). No documented policy for when to include the full object vs ID.

**Suggested fix:** Add design note in §2.1:
```
**Payload design policy:**
- Events during processing (ExecutionCompleted, ReflectionCompleted) include only 
  the candidate ID to keep payloads lightweight. Callbacks can query the engine's 
  candidate registry if they need the full candidate.
- Events at decision points (CandidateAccepted) include the full Candidate since 
  it's being added to the front and is immediately relevant.
- Field name `candidate_id` is used even though the type is `CandidateId` for 
  clarity in event struct initialization: `CandidateAccepted { candidate_id, ... }`.
```

---

### FINDING-7-6: **[Check #10] Option<Duration> unwrap in time_budget logic**

**Location:** design-07-config.md, §2.1 GEPAConfig

**Issue:** The struct has `pub time_budget: Option<Duration>` but the design doesn't show how the engine safely unwraps this. If the engine code does `config.time_budget.unwrap()` anywhere, it violates GUARD-4 (no panics).

**Suggested fix:** In §2.1, add usage note:
```rust
pub time_budget: Option<Duration>,
// Engine checks with: if let Some(budget) = config.time_budget { ... }
// None = no time budget (run until iteration limit or stagnation)
```

Also verify the master design (feature 1) uses safe Option handling. (Out of scope for this review, but flag for cross-check.)

---

### FINDING-9-5: **[Check #8] Potential string slice on reflection_length**

**Location:** design-09-events.md, §2.1 GEPAEvent, ReflectionCompleted

**Issue:** The event includes `reflection_length: usize` but doesn't specify what "length" means. If it's character count, could be computed via `.len()` on a String (byte length, UTF-8 unsafe if sliced). If it's meant to be Unicode grapheme count, needs explicit implementation.

**Suggested fix:** Add doc comment:
```rust
ReflectionCompleted {
    candidate_id: CandidateId,
    /// Length in UTF-8 bytes (String::len()). For display purposes only; do not
    /// use for string slicing.
    reflection_length: usize,
},
```

If implementations need character count, change to:
```rust
reflection_length: usize, // Count via .chars().count(), not .len()
```

---

## 📋 Path Traces

### Config Validation (design-07-config.md)

**Happy path:**
```
User constructs ConfigBuilder with all valid values
→ builder.build() validates all constraints
→ Returns Ok(GEPAConfig)
→ Config passed to engine.new()
→ Config serialized into checkpoint ✅
```

**Invalid config path:**
```
User sets minibatch_size=0
→ builder.build() runs validation (§5)
→ Detects minibatch_size=0 ❌
→ Returns Err(ConfigError::MinibatchSizeZero { message: "..." })
→ User sees error, corrects config, retries ✅
```

**Edge case: time_budget=0:**
```
User sets time_budget = Some(Duration::ZERO)
→ Validation passes (explicitly allowed per GOAL-7.3)
→ Engine starts run()
→ Checks elapsed time at iteration 0 (0ns elapsed < 0ns budget? false)
→ Immediately terminates with TerminationReason::TimeBudgetExceeded
→ GEPAResult has 0 completed iterations ✅
```

### Data Loading Epoch Wraparound (design-08-data-loading.md)

**Happy path (training_set_size > minibatch_size):**
```
100 examples, batch size 16
→ Epoch 1: sample 16 (pos 0..16)
→ Epoch 1: sample 16 (pos 16..32)
→ ...
→ Epoch 1: sample 16 (pos 80..96)
→ Epoch 1: 4 examples remain (pos 96..100)
→ Epoch wraparound: take 4 from epoch 1 + shuffle + take 12 from epoch 2
→ Batch has [old[96..100], new_shuffled[0..12]] ✅
→ Epoch 2 continues from pos 12
```

**Edge case (training_set_size < minibatch_size):**
```
10 examples, batch size 16
→ Epoch 1: return all 10 examples (batch size = 10)
→ Epoch 2: shuffle, return all 10 examples again
→ Every batch is the full training set, reshuffled ✅
```

**Failure path (DataLoader timeout):**
```
Engine calls loader.training_examples()
→ Wrapped in tokio::timeout(30s)
→ Loader takes 35s (DB query slow)
→ Timeout fires → Err(GEPAError::Timeout)
→ Engine retries (up to retry_max=3)
→ All retries time out
→ Engine returns Err(GEPAError::Timeout) → run halted ❌
```

### Event Callback Panic (design-09-events.md)

**Happy path:**
```
User registers 3 callbacks for IterationStarted
→ Engine emits IterationStarted event
→ Callback 1 runs successfully ✅
→ Callback 2 runs successfully ✅
→ Callback 3 runs successfully ✅
→ Engine continues to Select step ✅
```

**Panic path:**
```
Callback 2 panics with "database connection lost"
→ catch_unwind intercepts panic
→ Engine logs: "Callback panicked: database connection lost" ⚠️
→ Callback 3 still runs ✅
→ Engine continues to Select step ✅
→ Callback 2 remains registered (will panic again next event)
```

**Unrecoverable panic:**
```
Callback calls std::process::abort()
→ catch_unwind cannot intercept (not an unwinding panic)
→ Process terminates immediately ❌
→ No checkpoint saved, run is lost
```

---

## ✅ Passed Checks

### Design 07 (Config)

- **Check #0 (Document size):** ✅ 4 components (§2.1 GEPAConfig, §3.1 ConfigBuilder, §3.2 BackoffStrategy, §3.3 RetryPolicy) — well under the 8-component limit
- **Check #2 (References resolve):** ✅ All GOAL references (GOAL-7.1 through GOAL-7.7) verified against requirements doc. All internal section refs (§3.2, §4, §5) exist.
- **Check #3 (No dead definitions):** ✅ All enum variants (BackoffStrategy, ErrorPolicy, MergeSelectionStrategy) are referenced in the config struct. Except: EventType in doc 09 (flagged as FINDING-9-2).
- **Check #9 (Integer overflow):** ⚠️ Flagged as FINDING-7-1 (exponential backoff). Other counters (max_iterations, stagnation_limit) are validated at construction and compared (no arithmetic).
- **Check #12 (Ordering sensitivity):** ✅ Validation checks in §5 are order-independent (all return early on first error, but the order of checks doesn't affect correctness).
- **Check #16 (API surface):** ✅ Config fields are public (required for serde), but config is immutable after construction. Builder is the only public construction path.
- **Check #17 (Goals/non-goals explicit):** ✅ Requirements GOAL-7.3 lists valid and invalid cases explicitly. Design §1 states trade-off (eager validation).
- **Check #19 (Cross-cutting concerns):** ✅ Security: config validation prevents panic (GUARD-4). Observability: config is serialized into checkpoints (GOAL-7.4).
- **Check #22 (Missing helpers):** ✅ ConfigBuilder.build() references validation logic in §5, which is fully specified.
- **Check #23 (Dependency assumptions):** ✅ Uses serde (explicitly stated in §2.1), Duration (std), ChaCha8Rng (rand crate, per master design §3.3).
- **Check #24 (Migration path):** ✅ New crate, no existing code to replace.
- **Check #25 (Testability):** ✅ ConfigBuilder.build() is pure (no IO), returns Result. Easy to unit test all validation branches.

### Design 08 (Data Loading)

- **Check #0 (Document size):** ✅ 5 components (§2.1 DataLoader, §2.2 Example, §3.1 EagerDataLoader, §3.2 MinibatchSampler, §3.3 ScoreBackfill, §4 ValidationRunner) — 5 ≤ 8 ✅
- **Check #1 (Types fully defined):** ✅ Example struct has all 5 fields. DataLoader trait has both methods. MinibatchSampler struct definition is complete. ScoreBackfill pseudocode references EvalCache (defined in feature 6 per master design).
- **Check #2 (References resolve):** ✅ GOAL-8.1 through GOAL-8.7 verified against requirements. "see GOAL-6.3" (eval cache) cross-ref exists in master design.
- **Check #6 (Data flow completeness):** ⚠️ Flagged FINDING-8-4 (timeout enforcement). Otherwise: training_examples() → MinibatchSampler → engine loop → ScoreBackfill → ValidationRunner ✅. Every field in Example is read (id for tracking, input for adapter calls, metadata for logging, difficulty for selection).
- **Check #10 (Option/None handling):** ✅ Example.expected_output is `Option<String>`, used only for logging (not unwrapped). validation_examples() returns `Vec` (never None).
- **Check #14 (Coupling):** ✅ DataLoader returns owned Vec, no shared state. MinibatchSampler owns its RNG and position. ScoreBackfill queries eval cache but doesn't mutate candidates.
- **Check #17 (Goals/non-goals explicit):** ✅ §1 Overview states "eager loading over streaming" with rationale. Requirements GOAL-8.1 explicitly says "streaming is not in scope".
- **Check #18 (Trade-offs documented):** ✅ Eager loading vs streaming trade-off is explained (memory cost vs simplicity). "typical workload" size assumption is stated.
- **Check #23 (Dependency assumptions):** ✅ Requires async runtime (tokio for timeout), serde (for Example serialization per checkpoint).
- **Check #25 (Testability):** ✅ MinibatchSampler can be tested in isolation with a seeded RNG. DataLoader trait allows mock implementations.

### Design 09 (Events)

- **Check #0 (Document size):** ✅ 3 components (§2.1 GEPAEvent, §2.2 Builder API, §3.2 EventDispatcher) — well under 8
- **Check #1 (Types fully defined):** ✅ GEPAEvent enum has 15 variants, each with complete field lists. EventDispatcher struct shows full fields (handlers: HashMap<EventType, Vec<...>>).
- **Check #2 (References resolve):** ✅ All GOAL-9.1a/b, GOAL-9.3 through GOAL-9.6 verified. Cross-refs to GOAL-1.1 (5-step loop) exist in master design.
- **Check #4 (Naming consistency):** ⚠️ Flagged as FINDING-9-4 (CandidateId vs candidate_id). Otherwise consistent (all events use past tense: IterationStarted, not IterationStart).
- **Check #6 (Data flow):** ✅ Engine constructs GEPAEvent → EventDispatcher.emit() → callbacks receive &GEPAEvent → callbacks read fields → log/store. No mutation, no feedback loop (per GUARD-9).
- **Check #13 (Separation of concerns):** ✅ Callbacks are side-effect observers (logging, metrics, viz). Engine logic is pure (events don't influence decisions, per GUARD-9).
- **Check #14 (Coupling):** ✅ Events carry observed values (scores, IDs, counts), not derived state. Example: CandidateAccepted includes `scores: Vec<(ExampleId, f64)>` (what was evaluated) and `front_size: usize` (observable consequence), not the internal front data structure.
- **Check #17 (Goals/non-goals explicit):** ✅ Requirements GOAL-9.1b lists exact emission points. Design §1 states non-goal (no async dispatch).
- **Check #19 (Cross-cutting concerns):** ✅ Performance: GOAL-9.6 (zero overhead). Security: panic isolation (catch_unwind). Observability: this IS the observability feature.
- **Check #25 (Testability):** ✅ EventDispatcher can be tested in isolation. Mock engine can emit events and verify callbacks were invoked with correct payloads.

---

## Summary

**Total Findings:**
- 🔴 Critical: 3 (FINDING-7-1, FINDING-8-1, FINDING-9-1)
- 🟡 Important: 7 (FINDING-7-2 through FINDING-7-4, FINDING-8-2 through FINDING-8-4, FINDING-9-2, FINDING-9-3)
- 🟢 Minor: 6 (FINDING-7-5, FINDING-7-6, FINDING-8-5, FINDING-9-4, FINDING-9-5)

**By Document:**
- Design 07 (Config): 6 findings (1 critical, 3 important, 2 minor)
- Design 08 (Data Loading): 4 findings (1 critical, 3 important, 0 minor)
- Design 09 (Events): 6 findings (1 critical, 2 important, 3 minor)

**Recommendation:** **Needs fixes before implementation**

**Critical blockers:**
1. Exponential backoff overflow (FINDING-7-1) — will cause runtime panic on high retry counts
2. Epoch wraparound edge case (FINDING-8-1) — undefined behavior for small training sets
3. Callback panic handling (FINDING-9-1) — catch-all without type-specific handling

**Important issues:**
1. Missing config fields (merge_interval, data_loader_timeout_secs) — cross-document inconsistencies
2. Incomplete error handling in validation runner
3. Dead EventType definition (conflicts with requirements)
4. Missing trade-off documentation

**Estimated implementation confidence:** **Medium**

- Config design is mostly solid (modulo the critical overflow bug)
- Data loading design has good structure but edge cases need clarification
- Events design needs clarification on registration API (EventType usage)
- All three designs would benefit from more explicit error handling paths

**Next steps:**
1. Fix all 3 critical findings (especially FINDING-7-1 and FINDING-8-1, which are safety issues)
2. Address cross-document inconsistencies (FINDING-7-4: add missing config fields)
3. Clarify event registration API (FINDING-9-2: EventType usage)
4. After fixes, implementation can proceed with high confidence

---

## Verification Notes

**Checks run comprehensively across all 28 framework checks:**

| Check # | Category | Coverage |
|---------|----------|----------|
| 0 | Document size | ✅ All 3 docs checked (4, 5, 3 components) |
| 1-4 | Structural completeness | ✅ All types verified, refs resolved, naming checked |
| 5-7 | Logic correctness | ✅ State machines traced (MinibatchSampler epochs, callback panic flow) |
| 8-12 | Type safety & edge cases | ✅ String ops safe (no slicing), overflow flagged, Option handling verified |
| 13-16 | Architecture consistency | ✅ Separation of concerns verified, coupling checked, hardcoded values flagged |
| 17-20 | Design doc quality | ✅ Goals/non-goals verified against requirements, trade-offs documented |
| 21-25 | Implementability | ✅ Ambiguous prose flagged, testability verified, dependencies explicit |
| 26-28 | Existing code alignment | ✅ New crate (no existing code), API compatibility N/A, feature flag N/A |

**Special focus areas (per task instructions):**
- ✅ Config validation completeness: Validation logic in §5 covers all invalid cases from GOAL-7.3, but missed overflow check (FINDING-7-1)
- ✅ Data loader streaming behavior: Epoch wraparound traced, edge case ambiguity found (FINDING-8-1)
- ✅ Event enum exhaustiveness: All 15 event variants mapped to loop steps, but EventType usage unclear (FINDING-9-2)
- ✅ Callback ergonomics: Panic handling examined, catch-all pattern flagged (FINDING-9-1)

**Cross-document consistency checks:**
- ⚠️ Config (07) missing fields referenced in Data Loading (08): `data_loader_timeout_secs` (FINDING-7-4)
- ⚠️ Config (07) missing `merge_interval` field specified in requirements GOAL-7.7 (FINDING-7-3)
- ✅ Events (09) variants align with engine steps from master design §2
- ✅ All three docs use consistent terminology (Candidate, Example, CandidateId, etc.)

**Requirements traceability:**
- ✅ All GOAL-7.x (Config) requirements mapped to design sections
- ✅ All GOAL-8.x (Data Loading) requirements mapped to design sections
- ✅ All GOAL-9.x (Events) requirements mapped to design sections
- ⚠️ One deviation found: DataLoader trait signature (async + Result) differs from GOAL-8.1 (sync, no Result), but design explicitly documents this as a necessary extension

