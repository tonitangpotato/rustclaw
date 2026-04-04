# Design: GEPA Events / Callbacks

## 1. Overview

The event subsystem provides observable hooks into the GEPA engine's optimization loop without coupling the engine to any specific monitoring, logging, or visualization tool. Consumers register typed callbacks before `engine.run()`; the engine emits events at defined points in the loop. The design optimizes for zero overhead when no callbacks are registered (GOAL-9.6) and resilience against callback panics (GOAL-9.4).

Key trade-off: synchronous callback invocation (callbacks run inline in the engine loop) over async/channel-based dispatch. This keeps the implementation simple and avoids channel allocation, at the cost of callback execution time directly impacting iteration latency. Since GUARD-6 already constrains total overhead to <5%, this naturally limits callback duration.

**Satisfies:** GOAL-9.1a, GOAL-9.1b, GOAL-9.3, GOAL-9.4, GOAL-9.5, GOAL-9.6, GUARD-6, GUARD-8, GUARD-9.

## 2. Components

### 2.1 GEPAEvent Enum

**Responsibility:** Define all event variants with typed payloads.

**Interface:**
```rust
#[derive(Debug, Clone)]
pub enum GEPAEvent {
    IterationStarted {
        iteration: u64,
        timestamp: Instant,
    },
    CandidateSelected {
        candidate_id: CandidateId,
        selection_method: SelectionMethod,
    },
    ExecutionCompleted {
        candidate_id: CandidateId,
        scores: Vec<(ExampleId, f64)>,
        trace_count: usize,
    },
    ReflectionCompleted {
        candidate_id: CandidateId,
        reflection_length: usize,
    },
    MutationCompleted {
        parent_id: CandidateId,
        child_id: CandidateId,
    },
    CandidateAccepted {
        candidate_id: CandidateId,
        scores: Vec<(ExampleId, f64)>,
        front_size: usize,
    },
    CandidateRejected {
        candidate_id: CandidateId,
        scores: Vec<(ExampleId, f64)>,
        dominator_id: Option<CandidateId>,
    },
    IterationSkipped {
        reason: String,
        retry_count: u32,
    },
    ReEvaluationCompleted {
        candidate_id: CandidateId,
        new_scores: Vec<(ExampleId, f64)>,
        overfitting_delta: f64,
        candidates_pruned: Vec<CandidateId>,
    },
    StagnationWarning {
        counter: u64,
        limit: u64,
    },
    DataLoaderWarning {
        message: String,
    },
    CheckpointSaved {
        path: PathBuf,
        iteration: u64,
    },
    IterationCompleted {
        iteration: u64,
        elapsed: Duration,
        best_score: f64,
        front_size: usize,
    },
    ValidationProgress {
        candidate_index: usize,
        total_candidates: usize,
    },
    RunCompleted {
        termination_reason: TerminationReason,
        total_iterations: u64,
    },
}
```

**Key Details:**
- `CandidateId`, `ExampleId`, `SelectionMethod`, `TerminationReason` are types defined in features 01/02/06. Summarized here: `CandidateId(String)`, `ExampleId(String)`, `SelectionMethod` enum (`Tournament`, `LeastCrowded`, `Random`, `MergeComplement`), `TerminationReason` enum (`MaxIterations`, `Stagnation`, `TimeBudget`, `ConvergedFront`, `TooManySkips`).
- `Instant` is `std::time::Instant` — not serializable, but events are transient (not checkpointed).
- `PathBuf` is `std::path::PathBuf` for the checkpoint file path.
- All variants derive `Debug` per GUARD-8. `Clone` is derived to allow callbacks to store copies if needed.
- Events are **not** `Serialize/Deserialize` — they are transient in-process notifications, not persisted. The `TracingCallback` (§2.6) handles durable logging.

**Satisfies:** GOAL-9.1a, GUARD-8

### 2.2 Callback Registry

**Responsibility:** Store registered callbacks per event type and dispatch events to matching callbacks.

**Interface:**
```rust
pub type EventCallback = Box<dyn Fn(&GEPAEvent) + Send + Sync + 'static>;

#[derive(Debug)]
pub enum EventType {
    IterationStarted,
    CandidateSelected,
    ExecutionCompleted,
    ReflectionCompleted,
    MutationCompleted,
    CandidateAccepted,
    CandidateRejected,
    IterationSkipped,
    ReEvaluationCompleted,
    StagnationWarning,
    DataLoaderWarning,
    CheckpointSaved,
    IterationCompleted,
    ValidationProgress,
    RunCompleted,
}

#[derive(Default)]
pub struct CallbackRegistry {
    callbacks: HashMap<EventType, Vec<EventCallback>>,
}

impl CallbackRegistry {
    pub fn new() -> Self { Self { callbacks: HashMap::new() } }

    pub fn register(&mut self, event_type: EventType, callback: EventCallback) {
        self.callbacks.entry(event_type).or_default().push(callback);
    }

    pub fn has_callbacks(&self, event_type: &EventType) -> bool {
        self.callbacks.get(event_type).map_or(false, |v| !v.is_empty())
    }

    pub fn emit(&self, event: &GEPAEvent) {
        let event_type = EventType::from(event);
        if let Some(callbacks) = self.callbacks.get(&event_type) {
            for callback in callbacks {
                // Panic safety: see §2.5
                let result = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| callback(event))
                );
                if let Err(panic_info) = result {
                    tracing::warn!(
                        event = ?event_type,
                        "Callback panicked, continuing: {:?}",
                        panic_info
                    );
                }
            }
        }
    }
}

impl std::fmt::Debug for CallbackRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackRegistry")
            .field("callback_counts", &self.callbacks.iter()
                .map(|(k, v)| (k, v.len()))
                .collect::<Vec<_>>())
            .finish()
    }
}
```

**Key Details:**
- `EventType` is a discriminant-only enum matching `GEPAEvent` variants. `impl From<&GEPAEvent> for EventType` extracts the discriminant.
- Callbacks are `Box<dyn Fn(&GEPAEvent) + Send + Sync + 'static>` — immutable reference to event data per GOAL-9.4, `Send + Sync` for GUARD-11 compatibility.
- Multiple callbacks per event type, invoked in registration order (GOAL-9.3). Same callback can be registered multiple times.
- No deregistration API (GOAL-9.3).
- `Debug` impl shows callback counts per event type, not the closures themselves (closures are not `Debug`).
- `EventType` implements `Hash + Eq` for use as `HashMap` key.

**Satisfies:** GOAL-9.3, GUARD-8, GUARD-9

### 2.3 Emission Points

**Responsibility:** Define exactly where in the engine loop each event fires.

**Emission sequence per iteration:**
```
loop {
    // 1. Check time budget → if exceeded: emit RunCompleted, break
    emit!(IterationStarted { iteration, timestamp: Instant::now() });

    // 2. SELECT
    let candidate = front.select(&mut rng);
    emit!(CandidateSelected { candidate_id, selection_method });

    // 3. EXECUTE
    let (scores, traces) = adapter.execute(candidate, minibatch).await;
    emit!(ExecutionCompleted { candidate_id, scores, trace_count });

    // 4. REFLECT
    let reflection = adapter.reflect(candidate, traces).await;
    emit!(ReflectionCompleted { candidate_id, reflection_length });

    // 5. MUTATE
    let child = adapter.mutate(candidate, reflection, lessons).await;
    emit!(MutationCompleted { parent_id, child_id });

    // 6. ACCEPT/REJECT
    if front.try_insert(child, scores) {
        emit!(CandidateAccepted { candidate_id, scores, front_size });
    } else {
        emit!(CandidateRejected { candidate_id, scores, dominator_id });
    }

    // 7. PERIODIC: backfill (every re_eval_interval iterations)
    if iteration % re_eval_interval == 0 {
        // ... run backfill ...
        emit!(ReEvaluationCompleted { candidate_id, new_scores, overfitting_delta, candidates_pruned });
    }

    // 8. STAGNATION CHECK
    if stagnation_counter > stagnation_limit / 2 {
        emit!(StagnationWarning { counter, limit });
    }

    // 9. CHECKPOINT (every checkpoint_interval iterations)
    if iteration % checkpoint_interval == 0 {
        // ... write checkpoint ...
        emit!(CheckpointSaved { path, iteration });
    }

    emit!(IterationCompleted { iteration, elapsed, best_score, front_size });
}

// After loop exit:
emit!(RunCompleted { termination_reason, total_iterations });
// Then: ValidationProgress events during final validation
```

**Key Details:**
- `IterationSkipped` replaces the normal step sequence when adapter error exhausts retries with `ErrorPolicy::Skip`.
- `DataLoaderWarning` fires during startup validation (before the loop), not inside the iteration.
- `StagnationWarning` fires when counter exceeds 50% of limit, providing early warning.
- Event emission order within an iteration is deterministic and fixed by the algorithm structure (GUARD-9).

**Satisfies:** GOAL-9.1b, GUARD-3

### 2.4 Zero-Cost When Unused

**Responsibility:** Ensure zero allocation overhead when no callbacks are registered for an event type.

**Interface:**
```rust
// Macro used at each emission point in the engine loop:
macro_rules! emit_event {
    ($registry:expr, $event_type:expr, $event_builder:expr) => {
        if $registry.has_callbacks(&$event_type) {
            let event = $event_builder;  // Event constructed only if callbacks exist
            $registry.emit(&event);
        }
    };
}
```

**Key Details:**
- The `has_callbacks()` check is a `HashMap::get` + `Vec::is_empty` — O(1) with no allocation.
- The event struct (which may contain `Vec<(ExampleId, f64)>` clones) is only constructed inside the `if` branch. With zero callbacks, the `Vec` clone never happens.
- The `emit_event!` macro is the only way events are fired in engine code, enforcing the pattern consistently.
- With zero callbacks registered for all event types, per-iteration overhead is 15 × O(1) HashMap lookups ≈ negligible (well within GUARD-6's 5% budget).
- The macro approach avoids the need for lazy event construction closures, keeping the engine code readable.

**Satisfies:** GOAL-9.6, GUARD-6

### 2.5 Panic Safety

**Responsibility:** Catch panics from user-provided callbacks so the engine loop continues.

**Key Details:**
- Every callback invocation in `CallbackRegistry::emit()` is wrapped in `std::panic::catch_unwind(AssertUnwindSafe(|| callback(event)))`.
- `AssertUnwindSafe` is required because `&GEPAEvent` is not `UnwindSafe` by default (it contains non-`UnwindSafe` types). This is safe here because the engine does not read any mutable state from the callback — the event reference is immutable and the callback cannot mutate engine state.
- On panic: log a warning via `tracing::warn!` including the `EventType` discriminant and panic payload (`Box<dyn Any>`). Then continue to the next callback.
- The panicking callback remains registered — subsequent events still invoke it (it may recover). No automatic deregistration (GOAL-9.4).
- Remaining callbacks for the same event are still invoked after a panic in an earlier callback.

**Satisfies:** GOAL-9.4

### 2.6 TracingCallback

**Responsibility:** Built-in callback that logs all events via the `tracing` crate at configured levels.

**Interface:**
```rust
pub struct TracingCallback;

impl TracingCallback {
    pub fn new() -> Self { Self }

    pub fn callback(&self) -> EventCallback {
        Box::new(|event: &GEPAEvent| {
            match event {
                GEPAEvent::IterationStarted { iteration, .. } => {
                    tracing::info!(iteration, "Iteration started");
                }
                GEPAEvent::CandidateSelected { candidate_id, selection_method } => {
                    tracing::debug!(?candidate_id, ?selection_method, "Candidate selected");
                }
                GEPAEvent::ExecutionCompleted { candidate_id, trace_count, .. } => {
                    tracing::debug!(?candidate_id, trace_count, "Execution completed");
                }
                GEPAEvent::ReflectionCompleted { candidate_id, reflection_length } => {
                    tracing::debug!(?candidate_id, reflection_length, "Reflection completed");
                }
                GEPAEvent::MutationCompleted { parent_id, child_id } => {
                    tracing::debug!(?parent_id, ?child_id, "Mutation completed");
                }
                GEPAEvent::CandidateAccepted { candidate_id, front_size, .. } => {
                    tracing::info!(?candidate_id, front_size, "Candidate accepted into front");
                }
                GEPAEvent::CandidateRejected { candidate_id, dominator_id, .. } => {
                    tracing::debug!(?candidate_id, ?dominator_id, "Candidate rejected");
                }
                GEPAEvent::IterationSkipped { reason, retry_count } => {
                    tracing::warn!(reason, retry_count, "Iteration skipped");
                }
                GEPAEvent::ReEvaluationCompleted { candidate_id, overfitting_delta, .. } => {
                    tracing::debug!(?candidate_id, overfitting_delta, "Re-evaluation completed");
                }
                GEPAEvent::StagnationWarning { counter, limit } => {
                    tracing::warn!(counter, limit, "Stagnation warning");
                }
                GEPAEvent::DataLoaderWarning { message } => {
                    tracing::warn!(message, "Data loader warning");
                }
                GEPAEvent::CheckpointSaved { path, iteration } => {
                    tracing::info!(?path, iteration, "Checkpoint saved");
                }
                GEPAEvent::IterationCompleted { iteration, elapsed, best_score, front_size } => {
                    tracing::info!(iteration, ?elapsed, best_score, front_size, "Iteration completed");
                }
                GEPAEvent::ValidationProgress { candidate_index, total_candidates } => {
                    tracing::debug!(candidate_index, total_candidates, "Validation progress");
                }
                GEPAEvent::RunCompleted { termination_reason, total_iterations } => {
                    tracing::info!(?termination_reason, total_iterations, "Run completed");
                }
            }
            // At TRACE level, log full Debug representation
            tracing::trace!(event = ?event, "Full event payload");
        })
    }

    pub fn register_all(self, registry: &mut CallbackRegistry) {
        let cb = self.callback();
        // Register a clone-wrapped callback for every event type
        // Implementation: register one callback that handles all variants
        // by registering for each EventType individually
        for event_type in EventType::all() {
            let cb_clone: EventCallback = Box::new({
                let inner = std::sync::Arc::new(self.callback_fn());
                move |event| inner(event)
            });
            registry.register(event_type, cb_clone);
        }
    }
}
```

**Key Details:**
- Level mapping matches GOAL-9.5's table exactly: INFO for lifecycle events (started, accepted, checkpoint, completed), DEBUG for step details, WARN for problems.
- At `tracing::trace!` level, the full `Debug` representation of the event is logged. At higher levels, only a summary line with key fields is logged.
- `register_all` is a convenience method that registers the callback for every `EventType` variant. Uses `EventType::all()` which returns an iterator over all discriminants.
- Internally uses `Arc` for the shared callback function to avoid duplicating the closure for each event type registration. Each registered callback wraps the same `Arc<dyn Fn>`.
- The `TracingCallback` itself is zero-sized (`struct TracingCallback;`). All state lives in the closures.

**Satisfies:** GOAL-9.5, GUARD-8

## 3. Performance Budget

Event overhead must stay within GUARD-6's 5% budget relative to adapter call time.

**Analysis for zero-callback case:**
- 15 emission points per iteration × 1 `HashMap::get` + 1 `Vec::is_empty` per point ≈ 15 ns total.
- Adapter calls typically take 500ms–5s per iteration (LLM inference).
- Overhead: 15 ns / 500 ms = 0.000003% — negligible.

**Analysis with `TracingCallback` registered for all events:**
- 15 emissions × (catch_unwind overhead ~50ns + tracing macro ~200ns + field formatting ~500ns) ≈ 11 µs per iteration.
- Overhead: 11 µs / 500 ms = 0.0022% — well within 5%.

**Analysis with expensive user callbacks:**
- If a user callback does synchronous I/O (e.g., writes to a file), it could take 1-10 ms per invocation.
- 15 callbacks × 10 ms = 150 ms per iteration → 150 ms / 500 ms = 30% overhead → exceeds 5%.
- Mitigation: document that long-running callbacks should spawn async tasks. The engine provides no timeout enforcement (GOAL-9.4) — this is the consumer's responsibility.

**Key mechanisms keeping overhead low:**
1. `has_callbacks()` check prevents event construction (§2.4)
2. `HashMap`-based dispatch avoids scanning unrelated callbacks
3. No channel allocation or thread spawning for event delivery
4. No serialization of events unless explicitly done in a callback

## 4. Integration Points

- **Engine loop (feature 01):** The engine holds a `CallbackRegistry` and calls `emit_event!` at each defined emission point (§2.3). The registry is built during `GEPAEngine::builder()` via `on_event()` calls (GOAL-9.3).
- **Builder (feature 01):** `EngineBuilder::on_event(EventType, callback)` delegates to `CallbackRegistry::register()`. The builder owns the registry until `build()` transfers it to the engine.
- **Data loading (feature 08):** Emits `DataLoaderWarning` during startup validation, `ReEvaluationCompleted` after backfill, `ValidationProgress` during final validation.
- **Checkpoint (feature 06):** Emits `CheckpointSaved` after successful checkpoint write.
- **Config (feature 07):** No direct coupling. The `TracingCallback` does not read config; its log levels are hardcoded per the GOAL-9.5 table.
- **All features:** Events carry types from other features (`CandidateId`, `ExampleId`, `SelectionMethod`, `TerminationReason`). The event module imports these types but does not depend on their implementation logic.
