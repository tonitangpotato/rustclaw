# Requirements: GEPA Callback / Events

> Feature 9 of 9 — Master doc: `requirements-master.md`

Observable event system for monitoring, logging, visualization, and integration with external tools. Consumers register callbacks that are invoked at specific points in the optimization loop.

## Goals

- **GOAL-9.1** [P0]: The engine emits typed events at key points: `IterationStarted`, `CandidateSelected`, `ExecutionCompleted`, `ReflectionCompleted`, `MutationCompleted`, `CandidateAccepted`, `CandidateRejected`, `IterationSkipped { reason, retry_count }` (adapter error after retry exhaustion), `ReEvaluationCompleted { candidate_id, new_scores, overfitting_delta, candidates_pruned }` (after periodic re-evaluation backfill per GOAL-8.5; `candidates_pruned` lists front members removed due to newly established dominance), `StagnationWarning` (stagnation counter > 50% of limit), `DataLoaderWarning { message }` (empty validation set per GOAL-8.7), `CheckpointSaved`, `IterationCompleted`, `RunCompleted`.

- **GOAL-9.2** [P0]: Events carry relevant data: `CandidateAccepted` includes the candidate, its scores, and the updated Pareto front size. `IterationCompleted` includes iteration number, elapsed time, current best score, and front size.

- **GOAL-9.3** [P1]: Consumers register callbacks via `GEPAEngine::on_event(EventType, callback)` before calling `run()`. Multiple callbacks can be registered for the same event type; they are invoked in registration order.

- **GOAL-9.4** [P1]: Callbacks receive an immutable reference to the event data. Callbacks must not block the optimization loop — they execute synchronously but are expected to be fast (logging, metric recording). Long-running callbacks should spawn their own tasks.

- **GOAL-9.5** [P1]: The engine emits structured log records via the `tracing` crate at appropriate levels: INFO for iteration start/end and candidate acceptance, DEBUG for selection details and cache hits/misses, WARN for adapter retries and stagnation warnings, ERROR for unrecoverable failures. Log records include span context (iteration number, candidate ID) for filtering. A built-in `TracingCallback` is provided that logs all events via `tracing` at these levels, including `trace` level for full event payloads.

## Cross-references

- GOAL-1.x (Core Engine) — events emitted at each step
- GOAL-6.5 (State) — statistics accumulated from events
- GOAL-8.5 (Data Loading) — `ReEvaluationCompleted` event

**Summary: 5 GOALs** (2 P0, 3 P1)
