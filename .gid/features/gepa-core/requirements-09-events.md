# Requirements: GEPA Callback / Events

> Feature 9 of 9 — Master doc: `requirements-master.md`

Observable event system for monitoring, logging, visualization, and integration with external tools. Consumers register callbacks that are invoked at specific points in the optimization loop.

## Goals

- **GOAL-9.1a** [P0]: The engine defines a typed event enum with the following variants and payload fields:

  | Event | Fields |
  |---|---|
  | `IterationStarted` | `iteration: u64, timestamp: Instant` |
  | `CandidateSelected` | `candidate_id: CandidateId, selection_method: SelectionMethod` |
  | `ExecutionCompleted` | `candidate_id: CandidateId, scores: Vec<(ExampleId, f64)>, trace_count: usize` |
  | `ReflectionCompleted` | `candidate_id: CandidateId, reflection_length: usize` |
  | `MutationCompleted` | `parent_id: CandidateId, child_id: CandidateId` |
  | `CandidateAccepted` | `candidate: Candidate, scores: Vec<(ExampleId, f64)>, front_size: usize` |
  | `CandidateRejected` | `candidate_id: CandidateId, scores: Vec<(ExampleId, f64)>, dominator_id: Option<CandidateId>` |
  | `IterationSkipped` | `reason: String, retry_count: u32` |
  | `ReEvaluationCompleted` | `candidate_id: CandidateId, new_scores: Vec<(ExampleId, f64)>, overfitting_delta: f64, candidates_pruned: Vec<CandidateId>` |
  | `StagnationWarning` | `counter: u64, limit: u64` |
  | `DataLoaderWarning` | `message: String` |
  | `CheckpointSaved` | `path: PathBuf, iteration: u64` |
  | `IterationCompleted` | `iteration: u64, elapsed: Duration, best_score: f64, front_size: usize` |
  | `ValidationProgress` | `candidate_index: usize, total_candidates: usize` |
  | `RunCompleted` | `termination_reason: TerminationReason, total_iterations: u64` |

- **GOAL-9.1b** [P0]: Events are emitted at specific points in the optimization loop (referencing GOAL-1.1's 5-step sequence and GUARD-3's call ordering):
  - `IterationStarted` — at the beginning of each iteration, before the Select step
  - `CandidateSelected` — immediately after the Select step returns, before Execute begins
  - `ExecutionCompleted` — after the Execute step returns scores, before Reflect begins
  - `ReflectionCompleted` — after the Reflect step returns, before Mutate begins
  - `MutationCompleted` — after the Mutate step produces a new candidate, before Accept/Reject
  - `CandidateAccepted` / `CandidateRejected` — after the front update decision
  - `IterationSkipped` — when adapter error exhausts retries (replaces the normal step sequence)
  - `ReEvaluationCompleted` — after periodic backfill round (GOAL-8.5a) completes
  - `StagnationWarning` — at iteration end when stagnation counter > 50% of limit
  - `DataLoaderWarning` — during startup validation (GOAL-8.7)
  - `CheckpointSaved` — after checkpoint write completes (GOAL-6.4)
  - `ValidationProgress` — during final validation (GOAL-8.6), once per candidate
  - `IterationCompleted` — at the end of each iteration, after all step events
  - `RunCompleted` — after the loop exits (any termination reason per GOAL-1.2a-d)

- **GOAL-9.3** [P1]: Event handlers are registered as callbacks via `GEPAEngine::on_event(EventType, callback)` before calling `run()`. The builder pattern (GOAL-1.0) enforces this at compile time — `on_event` is available on the builder, not on the running engine. Multiple callbacks can be registered for the same event type; they are invoked in registration order. The same callback may be registered multiple times; each registration is independent. There is no deregistration API.

- **GOAL-9.4** [P1]: Callbacks receive an immutable reference to the event data. The engine provides no callback timeout enforcement — long-running callbacks will directly delay the optimization loop. This is the consumer's responsibility; callback execution time counts toward GUARD-6's 5% overhead budget, which naturally constrains callback duration. Long-running callbacks should spawn their own tasks. If a callback panics, the engine catches the panic (using `std::panic::catch_unwind`), emits a warning log via `tracing`, and continues the optimization loop. Remaining callbacks for the same event are still invoked. The panicking callback remains registered — it is not automatically deregistered.

- **GOAL-9.5** [P1]: The engine emits structured log records via the `tracing` crate. Log levels per event type:

  | Event | Level |
  |---|---|
  | `IterationStarted` | INFO |
  | `CandidateSelected` | DEBUG |
  | `ExecutionCompleted` | DEBUG |
  | `ReflectionCompleted` | DEBUG |
  | `MutationCompleted` | DEBUG |
  | `CandidateAccepted` | INFO |
  | `CandidateRejected` | DEBUG |
  | `IterationSkipped` | WARN |
  | `ReEvaluationCompleted` | DEBUG |
  | `StagnationWarning` | WARN |
  | `DataLoaderWarning` | WARN |
  | `CheckpointSaved` | INFO |
  | `ValidationProgress` | DEBUG |
  | `IterationCompleted` | INFO |
  | `RunCompleted` | INFO |
  | Unrecoverable failures | ERROR |

  Log records include span context (iteration number, candidate ID) for filtering. A built-in `TracingCallback` is provided that logs all events at the levels above. At `trace` level, the `TracingCallback` serializes full event payloads using the event's `Debug` implementation. At higher levels (INFO/WARN/etc.), it logs a human-readable summary line without the full payload.

- **GOAL-9.6** [P0]: Event data is constructed only if at least one callback is registered for that event type. With zero callbacks registered for all event types, the event system adds zero allocation overhead per iteration.

### Applicable GUARDs

- **GUARD-6** (<5% overhead) — event system overhead including callback execution counts toward this budget
- **GUARD-8** (Debug/Error impls) — all event types must implement Debug
- **GUARD-9** (determinism) — events are side-effect observations; callbacks receive immutable references and cannot feed back into the engine

## Cross-references

- GOAL-1.1 (Core Engine) — defines the 5-step loop that determines event emission points
- GOAL-1.2a-d (Core Engine) — termination events (RunCompleted)
- GOAL-6.4 (State) — CheckpointSaved emission point
- GOAL-6.5 (State) — statistics accumulated from events
- GOAL-8.5 (Data Loading) — `ReEvaluationCompleted` event
- GOAL-8.6 (Data Loading) — `ValidationProgress` event
- GOAL-8.7 (Data Loading) — `DataLoaderWarning` event

**Summary: 6 GOALs** (3 P0, 3 P1) — GOAL-9.1 split into 9.1a/b, added GOAL-9.6
