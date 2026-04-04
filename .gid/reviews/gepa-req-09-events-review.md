# Review: requirements-09-events.md

**Reviewed:** 2026-04-04
**Document:** `.gid/features/gepa-core/requirements-09-events.md`
**Scope:** 5 GOALs (GOAL-9.1 through GOAL-9.5), cross-checked against 9 GUARDs in `requirements-master.md`

---

## Phase 0: Document Size Check

- **5 GOALs** — well within the ≤15 limit. ✅

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #5] GOAL-9.1: Missing event data specifications for most event types**

GOAL-9.1 lists 14 event types but only specifies payload fields for `IterationSkipped` and `ReEvaluationCompleted`. GOAL-9.2 adds payload details for `CandidateAccepted` and `IterationCompleted`. That means **10 of 14 event types have no payload specification**:

- `IterationStarted` — What data? Iteration number? Timestamp?
- `CandidateSelected` — Which candidate? Selection reason? From which front position?
- `ExecutionCompleted` — Candidate, scores, trace summary? Minibatch used?
- `ReflectionCompleted` — Candidate, reflection text?
- `MutationCompleted` — Parent candidate, new candidate, reflection used?
- `CandidateRejected` — The candidate, its scores, which front member dominated it?
- `StagnationWarning` — Current stagnation counter, configured limit, last accepted iteration?
- `DataLoaderWarning` — Only has `{ message }` — is message free-form string?
- `CheckpointSaved` — File path? State size? Duration?
- `RunCompleted` — Final result summary? Or just a signal?

An implementer cannot define these event structs without guessing. A tester cannot verify correct payloads.

**Suggested fix:** Add a payload specification table to GOAL-9.1 or GOAL-9.2 listing every event type and its required fields. Example:

```
| Event | Fields |
|---|---|
| IterationStarted | iteration: u64, timestamp: Instant |
| CandidateSelected | candidate_id: CandidateId, selection_method: SelectionMethod |
| ExecutionCompleted | candidate_id: CandidateId, scores: Vec<(ExampleId, f64)>, trace_count: usize |
| ReflectionCompleted | candidate_id: CandidateId, reflection_length: usize |
| MutationCompleted | parent_id: CandidateId, child_id: CandidateId |
| CandidateRejected | candidate_id: CandidateId, scores: Vec<(ExampleId, f64)>, dominator_id: Option<CandidateId> |
| StagnationWarning | counter: u64, limit: u64 |
| DataLoaderWarning | message: String |
| CheckpointSaved | path: PathBuf, iteration: u64 |
| RunCompleted | termination_reason: TerminationReason, total_iterations: u64 |
```

### FINDING-2
**[Check #4] GOAL-9.1: Compound requirement — event type enumeration AND emission points are mixed**

GOAL-9.1 does two things: (a) defines the event type enum with 14 variants, and (b) states the engine emits them "at key points." These are different concerns. The event type definitions are a data model concern; the emission points are behavioral requirements that should specify *where* in the loop each event fires.

Currently, only `IterationSkipped` and `ReEvaluationCompleted` have context about when they fire. For the rest, "at key points" is the only specification. Where exactly does `CandidateSelected` fire — before or after the selection algorithm runs? Does `ExecutionCompleted` fire before or after scores are cached?

**Suggested fix:** Split into:
- GOAL-9.1a: Define the event type enum with all variants and their payload fields.
- GOAL-9.1b: Define emission points — map each event to a specific point in the optimization loop (referencing GOAL-1.1's 5-step sequence and GUARD-3's call ordering). Example: "`CandidateSelected` is emitted immediately after the Select step returns, before Execute begins."

---

## 🟡 Important (should fix before implementation)

### FINDING-3
**[Check #1] GOAL-9.4: "expected to be fast" is vague**

"Callbacks must not block the optimization loop — they execute synchronously but are expected to be fast (logging, metric recording)."

"Expected to be fast" has no enforcement mechanism. What happens if a callback takes 10 seconds? Does the engine proceed? Is there a timeout? Is there a warning? Without enforcement, this is a gentleman's agreement, not a requirement.

**Suggested fix:** Choose one:
- (a) Add a configurable per-callback timeout (e.g., 100ms). If exceeded, emit a `CallbackTimeout` warning event and skip the callback in future iterations.
- (b) State explicitly: "The engine provides no callback timeout enforcement. Long-running callbacks will directly delay the optimization loop. This is the consumer's responsibility." (Making it an explicit non-requirement.)
- (c) Document that callback execution time is included in `GUARD-6`'s 5% overhead budget, which naturally constrains callback duration.

### FINDING-4
**[Check #7] Missing: Callback error handling**

What happens when a callback panics or returns an error? None of the 5 GOALs address this. Scenarios:
- Callback panics (e.g., logging to a file that's been deleted)
- Callback writes to a channel that's closed
- One of N registered callbacks fails — do remaining callbacks still execute?

This is an important gap because a panicking callback could crash the entire optimization run.

**Suggested fix:** Add a requirement: "If a callback panics, the engine catches the panic (using `std::panic::catch_unwind`), emits a warning log via `tracing`, and continues the optimization loop. Remaining callbacks for the same event are still invoked. The panicking callback remains registered — it is not automatically deregistered."

### FINDING-5
**[Check #8] Missing: Non-functional requirements for the event system**

No performance/scalability requirements for the event system itself:
- How many callbacks can be registered per event type? Unbounded?
- What's the overhead of event emission with 0 callbacks registered? (Should be near-zero to satisfy GUARD-6.)
- What's the memory overhead of event data construction when no callbacks are registered? (Should events be lazily constructed?)

These interact directly with GUARD-6 (engine overhead < 5%).

**Suggested fix:** Add: "Event data is constructed only if at least one callback is registered for that event type. With zero callbacks registered for all event types, the event system adds zero allocation overhead per iteration."

### FINDING-6
**[Check #9] GOAL-9.3: Missing boundary conditions for callback registration**

- Can callbacks be registered *after* `run()` starts? (Probably not, but not stated.)
- Can callbacks be deregistered?
- Can the same callback function be registered twice for the same event?
- What happens if `on_event` is called with a callback after `run()` is already in progress? (Compile error? Runtime error? Ignored?)

**Suggested fix:** Add to GOAL-9.3: "Callbacks can only be registered before calling `run()`. The builder pattern (GOAL-1.0) enforces this at compile time — `on_event` is available on the builder, not on the running engine. The same callback may be registered multiple times; each registration is independent. There is no deregistration API."

### FINDING-7
**[Check #15] GOAL-9.5: "appropriate levels" is vague — partially specified**

GOAL-9.5 lists specific log levels for several categories (INFO for iteration start/end, DEBUG for selection details, WARN for retries, ERROR for unrecoverable failures) but uses the phrase "at appropriate levels" as a preamble. The listed levels are good and specific, but:
- What level for `CandidateRejected`? (Happens frequently — probably DEBUG, but not stated.)
- What level for `CheckpointSaved`? (INFO? DEBUG?)
- What level for `RunCompleted`? (INFO?)
- What level for `ReEvaluationCompleted`? (DEBUG?)

The `TracingCallback` needs to know the level for every event type.

**Suggested fix:** Replace "at appropriate levels" with a complete event→level mapping table:
```
| Event | Level |
|---|---|
| IterationStarted | INFO |
| CandidateSelected | DEBUG |
| ExecutionCompleted | DEBUG |
| ReflectionCompleted | DEBUG |
| MutationCompleted | DEBUG |
| CandidateAccepted | INFO |
| CandidateRejected | DEBUG |
| IterationSkipped | WARN |
| ReEvaluationCompleted | DEBUG |
| StagnationWarning | WARN |
| DataLoaderWarning | WARN |
| CheckpointSaved | INFO |
| IterationCompleted | INFO |
| RunCompleted | INFO |
```

### FINDING-8
**[Check #16] GOAL-9.5: TracingCallback — technology assumption not fully specified**

GOAL-9.5 says "A built-in `TracingCallback` is provided that logs all events via `tracing` at these levels, including `trace` level for full event payloads." This is good — it names the technology (`tracing` crate, which is in the allowed dependencies). However:
- Does `TracingCallback` use `tracing::info!()` macros directly, or does it use `tracing::event!()` with dynamic levels?
- The `trace` level for "full event payloads" implies serializing event data to string — what format? `Debug` output? JSON via serde? This matters for log parsing.

**Suggested fix:** Add: "The `TracingCallback` serializes event payloads at `trace` level using the event's `Debug` implementation. At higher levels (INFO/WARN/etc.), it logs a human-readable summary line without the full payload."

---

## 🟢 Minor (can fix during implementation)

### FINDING-9
**[Check #12] Terminology: "callback" vs "consumer" vs "listener"**

The document uses "consumers" (intro paragraph, GOAL-9.3), "callbacks" (GOAL-9.3, 9.4), and the API uses `on_event` (which implies listener pattern). This is mostly consistent but "consumer" in the intro is slightly ambiguous — it could mean "user of the crate" (as used in master doc) rather than "event consumer." Consider using "event handler" or "callback" consistently within this document, reserving "consumer" for the crate user.

**Suggested fix:** Change intro from "Consumers register callbacks" to "Event handlers are registered as callbacks" or simply "Callbacks are registered."

### FINDING-10
**[Check #22] Grouping: Event types could be categorized**

GOAL-9.1 lists 14 event types in a flat list. For implementability, grouping by lifecycle phase would help:
- **Loop lifecycle:** `IterationStarted`, `IterationCompleted`, `IterationSkipped`, `RunCompleted`
- **Candidate lifecycle:** `CandidateSelected`, `CandidateAccepted`, `CandidateRejected`
- **Step completion:** `ExecutionCompleted`, `ReflectionCompleted`, `MutationCompleted`
- **Maintenance:** `ReEvaluationCompleted`, `CheckpointSaved`
- **Warnings:** `StagnationWarning`, `DataLoaderWarning`

**Suggested fix:** Add a categorization comment or restructure the event list by phase.

### FINDING-11
**[Check #23] Missing: Explicit dependency on GOAL-1.1 loop structure**

The events document implicitly depends on GOAL-1.1 (the 5-step loop) for defining emission points, but this dependency isn't explicit in the cross-references section. The cross-references mention "GOAL-1.x (Core Engine) — events emitted at each step" which is correct but vague.

**Suggested fix:** Change cross-reference to: "GOAL-1.1 (Core Engine) — defines the 5-step loop that determines event emission points" and add "GOAL-1.2a-d — termination events (RunCompleted)".

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | GOAL-9.1 (event types), 9.2 (payloads), 9.3 (registration), 9.5 (logging) | Emission order within a single iteration not fully specified |
| Error handling | — | ⚠️ Callback panics, callback errors, event system failures — none addressed |
| Performance | — (GUARD-6 in master is only indirect constraint) | ⚠️ No overhead budget for event system, no lazy construction requirement |
| Security | N/A (internal library, no auth/network) | — (correctly out of scope) |
| Reliability | — | ⚠️ What if callback registration fails? What if event data can't be constructed? |
| Observability | GOAL-9.5 (tracing integration) | No metrics for event system itself (callback count, callback durations) |
| Scalability | — | ⚠️ No limit on callbacks per event, no guidance on event volume at scale |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 5 GOALs, well within ≤15 limit.
- **Check #2: Testability** ✅ — 4/5 GOALs have clear pass/fail conditions. GOAL-9.1: test that each event type is emitted at the right point (pending FINDING-2 for specificity on *which* points). GOAL-9.2: test payload field presence. GOAL-9.3: test multiple callback registration and invocation order. GOAL-9.4: test immutable reference (compiler enforced). GOAL-9.5: test tracing output at specified levels. (GOAL-9.4's "expected to be fast" is flagged separately in FINDING-3.)
- **Check #3: Measurability** ✅ — No quantitative requirements in this document (correct — the quantitative constraint is GUARD-6 in master, which is already concrete at <5%).
- **Check #6: Happy path coverage** ✅ — The normal flow is: register callbacks → run engine → receive events at each loop step → TracingCallback logs them. This is covered across GOAL-9.1 through 9.5.
- **Check #10: State transitions** ✅ — The event system itself is stateless (fire-and-forget callbacks). No state machine to validate.
- **Check #11: Internal consistency** ✅ — Verified all 10 GOAL pairs (5 choose 2). No contradictions found. GOAL-9.4 (immutable reference) is consistent with GOAL-9.2 (event carries data). GOAL-9.5 (TracingCallback) is a specific instance of GOAL-9.3 (callback registration). GOAL-9.1 (event types) and GOAL-9.2 (payloads) are complementary.
- **Check #13: Priority consistency** ✅ — P0 GOALs (9.1, 9.2) define the event types and data. P1 GOALs (9.3, 9.4, 9.5) define registration and logging. P1 depends on P0 — correct priority ordering. No inversions.
- **Check #14: Numbering/referencing** ✅ — Cross-references: GOAL-1.x, GOAL-6.5, GOAL-8.5, GOAL-8.7 — all verified to exist in their respective documents. GOAL-8.5 references `ReEvaluationCompleted` (matches GOAL-9.1). GOAL-8.7 references `DataLoaderWarning` (matches GOAL-9.1). GOAL-6.5 references statistics from events (matches GOAL-9.2 payload data).
- **Check #15: GUARDs vs GOALs alignment** ✅ (partial — see FINDING-5 for GUARD-6 interaction). No contradictions found:
  - GUARD-2 (candidate immutability): Events carry immutable references (GOAL-9.4) — consistent.
  - GUARD-3 (no adapter calls outside loop): Events are engine-internal, not adapter calls — no conflict.
  - GUARD-5 (no network): Event system is local callbacks — no conflict.
  - GUARD-9 (determinism): Events are side-effect observations, not inputs to the algorithm — no conflict with determinism as long as callbacks don't feed back into the engine. (Note: GOAL-9.4 says "immutable reference" which prevents feedback. ✅)
- **Check #17: External dependencies** ✅ — Only `tracing` crate (listed in allowed dependencies in master doc). No other external deps.
- **Check #18: Data requirements** ✅ — Event data comes from the engine's internal state (iteration number, candidates, scores). No external data sources.
- **Check #19: Migration/compatibility** ✅ — N/A. This is new functionality, no replacement of existing system.
- **Check #20: Scope boundaries** ✅ (partial) — The document implicitly scopes to synchronous callbacks (GOAL-9.4 says "execute synchronously"). However, explicit non-goals would strengthen this — see FINDING-12 below (counted as minor).
- **Check #21: Unique identifiers** ✅ — 5 GOALs: 9.1, 9.2, 9.3, 9.4, 9.5. Sequential, no gaps, no duplicates.
- **Check #24: Acceptance criteria** ✅ — Each GOAL is testable as stated (with caveats from findings above). GOAL-9.1: emit all 14 event types. GOAL-9.2: verify payload fields. GOAL-9.3: register and invoke callbacks. GOAL-9.4: compiler rejects mutable references. GOAL-9.5: tracing output at correct levels.
- **Check #25: User perspective** ✅ — Requirements are written from the engine consumer's perspective (register callbacks, receive events). GOAL-9.5 specifically addresses the developer experience (structured logging).
- **Check #26: Success metrics** ✅ (partial) — GOAL-9.5's tracing integration provides runtime observability. However, there's no metric for "event completeness" (did the engine emit all expected events for a given run?).
- **Check #27: Risk identification** ✅ — This feature is low-risk (standard observer pattern). No novel algorithms or uncertain external dependencies. Not flagged in master doc's Risks section — correct.

---

## Summary

- **Total requirements:** 5 GOALs (2 P0, 3 P1), cross-checked against 9 GUARDs
- **Critical:** 2 (FINDING-1, FINDING-2)
- **Important:** 6 (FINDING-3 through FINDING-8)
- **Minor:** 3 (FINDING-9 through FINDING-11)
- **Total findings:** 11
- **Coverage gaps:** Error handling (callback panics/errors), Performance (event system overhead), Scalability (callback limits)
- **Recommendation:** **Needs fixes first** — the critical findings (incomplete event payloads, unspecified emission points) would force implementers to make design decisions that should be in the requirements. The important findings (callback error handling, performance guarantees) should be addressed to avoid production surprises.
- **Estimated implementation clarity:** **Medium** — The event pattern (observer/callback) is well-understood, and the event types are clearly enumerated. However, missing payload specifications for 10/14 events and missing emission point definitions mean an implementer would need to ask ~10 questions before starting.
