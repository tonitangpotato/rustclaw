# Review: requirements-09-events.md (GEPA Callback / Events)

> Reviewed: 2026-04-04
> Reviewer: Requirements Review Agent (27-check systematic review)
> Source: `.gid/features/gepa-core/requirements-09-events.md`
> Context: `.gid/features/gepa-core/requirements-master.md` (GUARDs, Out of Scope, Dependencies)

---

## Phase 0: Document Size Check

**Check #0: Document size** — 5 GOALs. Well within the ≤15 limit. ✅

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #5] GOAL-9.1: Missing outcome/behavior for several event types** — GOAL-9.1 lists 14 event types but only describes payload structure for 3 of them (`IterationSkipped`, `ReEvaluationCompleted`, `DataLoaderWarning`). The remaining 11 events (`IterationStarted`, `CandidateSelected`, `ExecutionCompleted`, `ReflectionCompleted`, `MutationCompleted`, `CandidateAccepted`, `CandidateRejected`, `StagnationWarning`, `CheckpointSaved`, `IterationCompleted`, `RunCompleted`) have no specified payload. An implementer cannot know what data each event carries without guessing. GOAL-9.2 specifies payload for only 2 of these (`CandidateAccepted` and `IterationCompleted`), leaving 9 event types with completely undefined payloads.

**Suggested fix:** Either (a) expand GOAL-9.1 to include payload definitions for every event type inline (structured as a table: event name → trigger point → payload fields), or (b) add a new GOAL-9.2b that systematically lists the payload for every event. At minimum, every event should specify:
- `IterationStarted { iteration: usize }`
- `CandidateSelected { iteration: usize, candidate_id: CandidateId, selection_reason: String }`
- `ExecutionCompleted { iteration: usize, candidate_id: CandidateId, num_examples: usize, elapsed: Duration }`
- `ReflectionCompleted { iteration: usize, candidate_id: CandidateId, reflection_summary: String }`
- `MutationCompleted { iteration: usize, parent_id: CandidateId, child_id: CandidateId }`
- `CandidateRejected { iteration: usize, candidate_id: CandidateId, reason: String }`
- `StagnationWarning { stagnation_count: usize, stagnation_limit: usize }`
- `CheckpointSaved { path: PathBuf, size_bytes: usize }`
- `RunCompleted { total_iterations: usize, termination_reason: TerminationReason, elapsed: Duration, front_size: usize }`

### FINDING-2
**[Check #4] GOAL-9.1: Non-atomic — compound requirement** — GOAL-9.1 is doing two things: (1) defining the event type enum (the list of all events), and (2) defining the trigger/emission points ("at key points"). These are conceptually separate: the event catalog vs. when events fire. The payload issue (FINDING-1) compounds this — a single GOAL is trying to define the event catalog, payloads, AND trigger points. This should be at least two GOALs: one for the event catalog with payloads, one for when/where events are emitted.

**Suggested fix:** Split GOAL-9.1 into:
- **GOAL-9.1a**: Event type catalog — enumerate all event types and their payload structures (table format).
- **GOAL-9.1b**: Emission points — define at which point in the optimization loop each event is emitted (e.g., `IterationStarted` fires before Select; `CandidateAccepted` fires after the Accept step updates the front).

### FINDING-3
**[Check #7] Missing error/edge case: callback panics** — No requirement specifies what happens if a callback panics. Does it crash the engine? Is it caught? Is the panicking callback removed? Given GOAL-9.4 says callbacks "execute synchronously" in the engine loop, an unhandled panic would abort the entire optimization run. This is a critical gap — external user code running synchronously in the engine loop without panic isolation is dangerous.

**Suggested fix:** Add a requirement: "If a callback panics, the engine catches the panic (via `std::panic::catch_unwind`), logs a WARN via `tracing`, removes the panicking callback from the registry, and continues the optimization loop. The panic does not propagate to the engine." Or alternatively, document that callbacks MUST NOT panic and that doing so is UB/abort.

---

## 🟡 Important (should fix before implementation)

### FINDING-4
**[Check #1] GOAL-9.4: Vague "expected to be fast"** — "Callbacks must not block the optimization loop — they execute synchronously but are expected to be fast (logging, metric recording)." The phrase "expected to be fast" is vague. What is the contract? If a callback takes 10 seconds, does the engine enforce a timeout? Or is it purely advisory? Two engineers would implement this differently — one might add a timeout, the other might trust callers.

**Suggested fix:** Either (a) make it a hard contract with enforcement: "If a callback takes longer than `callback_timeout` (default: 100ms), the engine logs a WARN and continues. If a callback exceeds `callback_timeout` 3 times consecutively, it is deregistered." Or (b) explicitly state it's advisory: "This is a documentation-only recommendation; the engine does not enforce callback duration. Callers are responsible for ensuring callbacks return promptly."

### FINDING-5
**[Check #3] GOAL-9.5: Vague "appropriate levels"** — The first sentence says "at appropriate levels" which is vague, but the rest of the requirement then specifies exact levels for each category (INFO, DEBUG, WARN, ERROR). The opening phrase is misleading — the details are actually specific. This is a minor wording issue but could cause confusion.

**Suggested fix:** Remove "at appropriate levels" from the first sentence. Replace with: "The engine emits structured log records via the `tracing` crate at the following levels:" and then list them.

### FINDING-6
**[Check #9] GOAL-9.3: Boundary condition — registering callbacks after `run()` starts** — GOAL-9.3 says "Consumers register callbacks via `GEPAEngine::on_event(EventType, callback)` before calling `run()`." What happens if a consumer calls `on_event()` after `run()` has started? Is it a compile-time error (ownership prevents it)? Runtime error? Silently ignored? The requirement says "before calling `run()`" but doesn't specify the enforcement mechanism.

**Suggested fix:** Add: "Calling `on_event()` after `run()` has been called is prevented by Rust's ownership model — `run()` takes `&mut self` or consumes `self`, making further `on_event()` calls impossible at compile time." Or if using a builder pattern: "Callbacks are registered on `GEPAEngineBuilder` before `build()` produces the final `GEPAEngine`, which has no `on_event` method."

### FINDING-7
**[Check #8] Missing non-functional: performance impact of events** — No requirement specifies the performance overhead of the event system itself. GUARD-6 says engine overhead should be <5% of adapter call time, but the event system (allocating event structs, invoking N callbacks per event, 14 event types per iteration) could meaningfully contribute to that overhead. There should be a requirement or explicit acknowledgment that the event system falls under GUARD-6.

**Suggested fix:** Add a note: "The event emission and callback invocation overhead is included in the engine-internal computation budget (GUARD-6). Event data structures should be allocated on the stack or use borrowed references to avoid heap allocation per event."

### FINDING-8
**[Check #10] Missing state: callback registration lifecycle** — The system has implicit states for callback registration (registered → active during run → done after run), but no requirement specifies whether callbacks can be deregistered, whether they persist across `run()` resumptions (GOAL-1.9), or whether callbacks are serialized as part of checkpoint state.

**Suggested fix:** Add: "Callbacks are not serialized and are not part of checkpoint state. When resuming from a checkpoint, consumers must re-register callbacks before calling `run()`. Callbacks cannot be deregistered once registered; they remain active for the entire `run()` invocation."

### FINDING-9
**[Check #16] GOAL-9.5: Technology assumption — `tracing` crate specifics** — GOAL-9.5 correctly names `tracing` (which is in the allowed dependencies list). However, the `TracingCallback` is described as "built-in" — is this a struct provided by the `gepa-core` crate itself? It mentions `trace` level for full event payloads, which implies event types implement `Debug` or have a serializable format. This should be explicit.

**Suggested fix:** Clarify: "`TracingCallback` is a public struct in `gepa_core::events` that implements the callback interface. It requires all event types to implement `Debug` (consistent with GUARD-8). At `trace` level, it logs the full `Debug` representation of each event."

### FINDING-10
**[Check #15] GUARDs vs GOALs: GUARD-9 determinism and callback invocation order** — GUARD-9 requires determinism given same inputs. Callbacks are user-provided code that could observe ordering. GOAL-9.3 specifies "invoked in registration order" which is good. But: if callbacks have side effects that affect the RNG or engine state (even indirectly), determinism could break. The requirement should explicitly state that callbacks receive immutable references (GOAL-9.4 says this — good) and cannot influence engine state.

**Suggested fix:** Add explicit statement: "Callbacks cannot influence engine state or the RNG. They receive immutable event data and have no return value. The engine's behavior is identical whether zero or one hundred callbacks are registered (GUARD-9 compliance)."

### FINDING-11
**[Check #18] GOAL-9.2: Data requirements — incomplete payload spec for `IterationCompleted`** — `IterationCompleted` includes "current best score" — best by what metric? Average across all examples? Best on a specific example? This is ambiguous in a multi-objective Pareto system where there is no single "best score." GOAL-1.8 mentions "single best candidate by average score" — is that what's meant here?

**Suggested fix:** Clarify: "`IterationCompleted` includes iteration number, elapsed time, best candidate's average score (same metric as GOAL-1.8), and current Pareto front size."

---

## 🟢 Minor (can fix during implementation)

### FINDING-12
**[Check #12] Terminology: "callback" vs "consumer" vs "handler"** — The document uses "consumers" (intro paragraph, GOAL-9.3), "callbacks" (GOAL-9.3, 9.4), and implicitly "handlers" (via the `on_event` pattern). This is mostly consistent (consumers register callbacks), but the TracingCallback in GOAL-9.5 suggests callbacks are struct types, not just closures. Clarify whether callbacks are closures (`Fn` trait), trait objects, or structs implementing a trait.

**Suggested fix:** Add to GOAL-9.3: "Callbacks are closures implementing `Fn(&EventType) + Send + Sync + 'static`" or "Callbacks are types implementing a `GEPAEventHandler` trait with a `fn handle(&self, event: &GEPAEvent)` method." The `TracingCallback` struct suggests the latter.

### FINDING-13
**[Check #21] Numbering consistency** — GOALs are numbered 9.1–9.5 with no gaps. Clean. ✅ However, unlike other feature docs, there are no sub-IDs (e.g., 9.1a, 9.1b) despite GOAL-9.1 being compound (see FINDING-2).

### FINDING-14
**[Check #22] Grouping** — The 5 GOALs are loosely grouped: event catalog (9.1), event payload (9.2), registration API (9.3), callback semantics (9.4), logging integration (9.5). Reasonable grouping but could benefit from explicit section headers to match the pattern of other feature docs.

### FINDING-15
**[Check #14] Cross-reference: "GOAL-1.x"** — The cross-references section says "GOAL-1.x (Core Engine) — events emitted at each step." This is vague — which specific GOAL-1.x items emit events? GOAL-1.1 through GOAL-1.8 all describe steps that should emit events, but none of them mention emitting events. The cross-reference is one-way.

**Suggested fix:** Add specific cross-references: "GOAL-1.1 (loop steps emit per-step events), GOAL-1.2b (stagnation triggers `StagnationWarning`), GOAL-1.7d (acceptance triggers `CandidateAccepted`/`CandidateRejected`), GOAL-1.8 (run completion triggers `RunCompleted`)."

### FINDING-16
**[Check #25] User perspective** — Events are described from the system perspective ("the engine emits"). For an API consumer, it would be helpful to include a brief usage example showing how to register a callback and what the consumer experience looks like. This is minor since this is a library crate, but a code snippet in the requirements would eliminate ambiguity.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | GOAL-9.1 (event emission), 9.2 (payloads), 9.3 (registration), 9.5 (logging) | Event deregistration, callback listing |
| Error handling | — | ⚠️ Callback panics, callback errors, event emission failures |
| Performance | GUARD-6 (indirectly) | No explicit event system overhead budget |
| Security | N/A (internal library, no auth) | — (correctly out of scope) |
| Reliability | GOAL-9.4 (non-blocking expectation) | No panic isolation, no timeout enforcement |
| Observability | GOAL-9.5 (tracing integration) | No metrics for callback execution time, no event count tracking |
| Scalability | — | No limit on number of registered callbacks, no guidance on many-callback behavior |
| Determinism | GOAL-9.3 (registration order), GOAL-9.4 (immutable refs) | No explicit GUARD-9 compliance statement |
| Checkpoint/Resume | — | ⚠️ No specification of callback behavior across checkpoint/resume |

---

## ✅ Passed Checks

- **Check #0**: Document size ✅ — 5 GOALs, well within 15 limit
- **Check #2**: Testability ✅ — 4/5 GOALs have testable conditions. GOAL-9.1: can test each event is emitted at the right point. GOAL-9.2: can assert payload fields. GOAL-9.3: can test registration and invocation order. GOAL-9.5: can test tracing output. GOAL-9.4 partially testable (immutable reference — yes; "fast" — no, see FINDING-4).
- **Check #6**: Happy path coverage ✅ — The normal flow is covered: register callbacks → run engine → receive events. All 5 steps of the loop have corresponding events.
- **Check #11**: Internal consistency ✅ — No contradictions found between the 5 GOALs. GOAL-9.4 (immutable refs, synchronous) is consistent with GOAL-9.3 (registration order invocation).
- **Check #13**: Priority consistency ✅ — P0 GOALs (9.1, 9.2) define events and payloads. P1 GOALs (9.3, 9.4, 9.5) define registration API and logging. P1 depends on P0, not vice versa. No priority inversion.
- **Check #17**: External dependencies ✅ — Only external dependency is `tracing` crate, which is explicitly in the allowed dependencies list (master doc).
- **Check #19**: Migration/compatibility ✅ — N/A, this is a new system, no migration needed.
- **Check #20**: Scope boundaries ✅ — Master doc "Out of Scope" covers this well. Events are explicitly internal to the crate; no external pub/sub, no networking.
- **Check #23**: Dependency graph ✅ — GOAL-9.2 depends on GOAL-9.1 (payloads depend on event types). GOAL-9.3/9.4 depend on GOAL-9.1 (registration depends on event types). GOAL-9.5 depends on GOAL-9.1/9.2 (logging depends on events and payloads). No circular dependencies.
- **Check #24**: Acceptance criteria — partially covered. Each GOAL has implicit acceptance criteria (events are emitted, payloads contain specified fields, callbacks invoked in order, tracing output at correct levels). Not explicit but derivable. ⚠️ Borderline pass.
- **Check #26**: Success metrics — partially covered. GOAL-9.5 provides observable tracing output. GOAL-6.5 (cross-ref) tracks statistics from events. No explicit "in production" metrics, but for a library crate this is acceptable.
- **Check #27**: Risk identification ✅ — Events system is straightforward (observer pattern). No high-risk items. The master doc's risk section correctly doesn't flag any events-related risks.

---

## Summary

| Metric | Count |
|---|---|
| Total requirements | 5 GOALs, 0 GUARDs (GUARDs in master) |
| 🔴 Critical | 3 (FINDING-1, FINDING-2, FINDING-3) |
| 🟡 Important | 8 (FINDING-4 through FINDING-11) |
| 🟢 Minor | 5 (FINDING-12 through FINDING-16) |
| Total findings | 16 |

**Coverage gaps:**
- Error handling for callback failures (panics, slow callbacks)
- Event payload definitions for 9 of 14 event types
- Callback lifecycle across checkpoint/resume
- Explicit GUARD-9 determinism compliance statement

**Recommendation:** **Needs fixes first** — FINDING-1 (undefined payloads for 9/14 events) is the primary blocker. An implementer cannot build the event system without knowing what data each event carries. FINDING-3 (callback panic handling) is a runtime safety concern that should be resolved before implementation.

**Estimated implementation clarity:** **Medium** — The overall architecture (typed events, registration API, tracing integration) is clear, but the missing payload definitions for most event types and the undefined callback error semantics create ambiguity that would require implementer judgment calls.
