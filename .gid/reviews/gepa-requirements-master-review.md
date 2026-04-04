# Review: requirements-master.md (GEPA Core)

**Reviewed:** 2026-04-04
**Scope:** Master document + all 9 feature-level requirement docs
**Total:** 68 GOALs (42 P0, 19 P1, 7 P2), 9 GUARDs (6 hard, 3 soft)

---

## 🔴 Critical (blocks implementation)

### FINDING-C-1: [Check #7] No cancellation trigger mechanism specified
**Affected:** GOAL-1.2c, GOAL-1.2d, GOAL-3.7
GOAL-1.2c/1.2d reference `Cancelled` as a termination reason and GOAL-3.7 defines a `Cancelled` error variant, but **no GOAL specifies how cancellation is triggered**. Is it a `CancellationToken` passed to the engine? A method call on a handle? A signal? An implementer reading this cannot build cancellation support.

**Suggested fix:** Add a new GOAL (e.g., GOAL-1.18) in Feature 1:
```
### GOAL-1.18 — Cancellation mechanism [P0]
The engine accepts a `CancellationToken` (or equivalent) at construction time.
When the token is triggered, the engine completes the current adapter call,
then terminates with `TerminationReason::Cancelled`. The token must be
`Clone + Send + Sync + 'static`.
```

### FINDING-C-2: [Check #7] Seed candidate initial evaluation gap
**Affected:** GOAL-5.1, GOAL-1.3, GOAL-2.3
GOAL-5.1 says seed candidates "form the initial Pareto front at iteration 0." But seeds have no scores yet — they haven't been evaluated. GOAL-1.3 (select parents via Pareto ranking) and GOAL-2.3 (Pareto dominance comparison) both require scores to function. The gap: **no GOAL specifies when/how seeds are first evaluated before iteration 1 begins.**

Is there an "iteration 0" evaluation pass? Do seeds bypass Pareto selection for the first iteration? This is the bootstrap problem and it must be specified.

**Suggested fix:** Add a GOAL in Feature 1 or Feature 5:
```
### GOAL-1.19 — Seed evaluation at initialization [P0]
Before iteration 1 begins, the engine evaluates all seed candidates on
the initial training examples via the adapter's evaluate method. Seeds
that fail evaluation are discarded (logged via event). At least one seed
must survive evaluation or the engine returns an error.
```

### FINDING-C-3: [Check #3, #9] Score semantics unspecified
**Affected:** GOAL-1.7d, GOAL-2.3, GOAL-2.4, GOAL-1.8, all scoring paths
Scores are `f64` throughout but critical semantics are missing:
- **Direction:** GOAL-1.7d implies higher=better ("improvement"), but this is never explicitly stated.
- **Range:** No valid range specified. Can scores be negative? Is [0,1] expected?
- **NaN/Infinity handling:** What happens if an adapter returns `f64::NAN` or `f64::INFINITY`? Pareto dominance comparisons with NaN produce undefined behavior in sorting.
- **Missing scores:** If a candidate hasn't been evaluated on a particular example, what score value is used?

**Suggested fix:** Add a GOAL or GUARD:
```
### GUARD-10 — Score semantics [hard]
Scores are f64 where higher values indicate better performance. Scores
must be finite (not NaN, not ±Infinity). The engine must reject or
discard any score that is not finite, logging the event. No assumed
range — scores are ordinal only (relative ordering matters, not
absolute values).
```

---

## 🟡 Important (should fix before implementation)

### FINDING-I-1: [Check #13] Priority inversions — P0 GOALs depend on P1/P2 GOALs
Multiple P0 requirements cannot be implemented without lower-priority requirements:

| P0 GOAL | Depends on | Priority | Issue |
|---|---|---|---|
| GOAL-1.2c (cancellation termination) | GOAL-3.7 (Cancelled variant) | P1 | Can't implement cancellation without the error type |
| GOAL-1.2c (cancellation termination) | GOAL-7.5 (stop conditions) | P1 | Stop conditions define cancellation config |
| GOAL-1.2a (convergence termination) | GOAL-7.6 (time budget) | P2 | Time budget is a termination condition |
| GOAL-1.10 (usage tracking) | GOAL-3.6 (usage struct) | P1 | Can't track usage without the struct |

**Suggested fix:** Promote GOAL-3.7, GOAL-7.5, GOAL-7.6, and GOAL-3.6 to P0. Alternatively, demote the dependent GOALs. Either way, priority ordering must be implementable.

### FINDING-I-2: [Check #11] Contradiction — GOAL-1.0 vs GOAL-7.3 on validation timing
GOAL-1.0 states: *"The engine builder validates all inputs at construction time (GOAL-7.3) so `run()` cannot fail due to misconfiguration."*
But GOAL-7.3 explicitly states: *"min_shared_examples > total training examples (checked at **run start**, not construction)"*

This is a direct contradiction. Either:
(a) All validation happens at construction and GOAL-7.3's "run start" check is wrong, or
(b) `run()` CAN fail from validation, and GOAL-1.0's claim is wrong.

**Suggested fix:** Amend GOAL-1.0 to acknowledge that `run()` can fail with a validation error for checks that require runtime data (like total training examples). Or move ALL validation to construction time by requiring training example count as a builder input.

### FINDING-I-3: [Check #14] Stale summary line in Feature 1
`requirements-01-core-engine.md` footer says: *"14 GOALs (11 P0, 2 P1, 1 P2)"*
Actual count: **18 GOALs (15 P0, 2 P1, 1 P2)**

This means 4 GOALs were added without updating the summary. Implementers relying on the summary will underestimate scope.

**Suggested fix:** Update footer to `18 GOALs (15 P0, 2 P1, 1 P2)`.

### FINDING-I-4: [Check #14] Stale summary line in Feature 8
`requirements-08-data-loading.md` footer says priority breakdown: *"4 P0, 2 P1, 1 P2"*
Actual: **5 P0, 2 P1, 0 P2** (7 GOALs total is correct)

**Suggested fix:** Update footer to `5 P0, 2 P1, 0 P2`.

### FINDING-I-5: [Check #15] GUARDs not cross-referenced by feature docs
Six of 9 GUARDs are never referenced in any feature-level document:

| GUARD | Description | Should be referenced by |
|---|---|---|
| GUARD-1 | Pareto front invariant | Feature 2 (Pareto Front) |
| GUARD-3 | Adapter call order | Features 1 (Engine), 3 (Adapter) |
| GUARD-5 | No LLM calls in core | Feature 3 (Adapter) |
| GUARD-6 | Engine overhead <5% | Feature 1 (Engine) |
| GUARD-7 | Memory: 10K candidates <1GB | Feature 5 (Candidates), Feature 2 (Pareto) |
| GUARD-8 | Debug/Error impls | All features with public types |

Unreferenced GUARDs risk being ignored during implementation. Each feature doc should list which GUARDs constrain it.

**Suggested fix:** Add a "### Applicable GUARDs" section to each feature doc listing the relevant GUARDs, or add GUARD cross-references to individual GOALs.

### FINDING-I-6: [Check #8] No Send + Sync / thread-safety requirements
The engine is `async` (GOAL-1.1), presumably running on tokio. But no requirement specifies:
- Whether the `LlmAdapter` trait requires `Send + Sync`
- Whether the engine itself is `Send`
- Whether concurrent engine runs are supported
- Thread-safety requirements for shared state

In Rust async, these bounds are critical — without them, the engine may not be usable in multi-threaded runtimes.

**Suggested fix:** Add a GUARD:
```
### GUARD-11 — Async compatibility [hard]
All public types must be Send + Sync where required for use in
multi-threaded async runtimes (e.g., tokio). The LlmAdapter trait
must be Send + Sync + 'static. The engine's run() future must be Send.
```

### FINDING-I-7: [Check #5] GOAL-1.8 ambiguous — "average score" over what?
GOAL-1.8: *"single best candidate by average score"*
Average over which scores? Options: (a) all training examples, (b) all validation examples, (c) all evaluated examples, (d) the shared examples. This matters because candidates may not all be evaluated on the same examples (GOAL-1.3c uses subsets).

**Suggested fix:** Specify explicitly: "average score across all training examples on which the candidate has been evaluated" or "average score across the shared evaluation examples (GOAL-8.5)".

### FINDING-I-8: [Check #16] Async runtime not specified
GOAL-1.1 says the engine exposes an `async fn run()`. But no requirement specifies:
- Which async runtime (tokio, async-std, runtime-agnostic)?
- If runtime-agnostic, is `#[tokio::test]` acceptable for testing?
- Are there constraints on the executor (single-thread vs multi-thread)?

**Suggested fix:** Either add a GUARD specifying runtime-agnosticism, or specify tokio as the expected runtime. Given Rust ecosystem norms, a simple note like "Engine is runtime-agnostic but tested under tokio" would suffice.

---

## 🟢 Minor (can fix during implementation)

### FINDING-M-1: [Check #12] Terminology: "training examples" vs "examples" vs "shared examples"
Three related but distinct terms are used:
- "training examples" (GOAL-8.1, 8.2) — the full dataset
- "shared examples" (GOAL-8.5, 1.3c) — subset used per iteration
- "examples" (various) — ambiguous, could mean either

Most usages are clear in context, but a glossary entry in the master doc would prevent confusion.

**Suggested fix:** Add a terminology section to the master doc defining: training examples, validation examples, shared examples, evaluation examples.

### FINDING-M-2: [Check #12] Terminology: "prompt" vs "candidate" vs "candidate prompt"
Used interchangeably in places. GOAL-5.1 defines `Candidate` struct clearly, but some GOALs say "prompt" when they mean the candidate's text content vs the candidate object.

**Suggested fix:** Standardize: "candidate" = the struct with id/text/metadata, "prompt text" = the string content of a candidate.

### FINDING-M-3: [Check #21] GOAL numbering uses hierarchical scheme but not documented
GOALs use `GOAL-X.Y` and sub-items use `GOAL-X.Ya/b/c/d` (e.g., GOAL-1.2a, GOAL-1.2b). This scheme works but the convention isn't documented. Sub-items sometimes represent independent requirements (GOAL-1.2a through 1.2d are four different termination conditions) but share a single priority.

**Suggested fix:** Add a note to master doc: "Sub-items (a,b,c,d) are independently implementable parts of a parent GOAL and inherit the parent's priority unless overridden."

### FINDING-M-4: [Check #22] Master doc feature table doesn't show dependencies between features
The feature index table lists 9 features with GOAL counts and priorities, but doesn't indicate which features depend on which. For example, Feature 1 (Engine) depends on almost every other feature.

**Suggested fix:** Add a dependency column or a simple dependency diagram to the master doc.

### FINDING-M-5: [Check #20] Explicit non-goals are sparse
The master doc lists a few non-goals in passing (e.g., "no LLM calls in core" — GUARD-5). But there's no dedicated non-goals section. Common questions that should be pre-answered:
- Is multi-objective optimization (beyond Pareto) in scope?
- Is distributed execution in scope?
- Is prompt template management in scope?
- Is result persistence/database storage in scope?

**Suggested fix:** Add a "### Non-Goals" section to the master doc listing explicit exclusions.

### FINDING-M-6: [Check #10] State machine for engine lifecycle not fully specified
GOAL-1.1 through GOAL-1.2 imply states: Building → Running → Terminated. GOAL-6.1–6.6 define state/checkpoint persistence. But there's no explicit state diagram. Questions:
- Can an engine be restarted from a checkpoint? (GOAL-6.4 implies yes)
- Can an engine be run twice? (Presumably no, but not stated)
- What state is the engine in if construction succeeds but `run()` hasn't been called?

**Suggested fix:** Add a brief state diagram or state transition table to Feature 1 or Feature 6.

---

## 📊 Coverage Matrix

### Feature × Category Coverage

| Category | Covered by | Gaps |
|---|---|---|
| **Happy path** | GOAL-1.0–1.8 (full loop), GOAL-2.1–2.6 (Pareto), GOAL-3.1–3.8 (adapter), GOAL-4.1–4.5 (proposers), GOAL-5.1–5.6 (candidates), GOAL-8.1–8.7 (data) | Seed evaluation bootstrap (FINDING-C-2) |
| **Error handling** | GOAL-3.7 (adapter errors), GOAL-3.8 (retries), GOAL-7.3 (validation), GOAL-1.2d (fatal error termination) | Cancellation trigger (FINDING-C-1), partial failure in batch evaluation, OOM handling |
| **Termination** | GOAL-1.2a–d (4 conditions) | Cancellation mechanism unspecified (FINDING-C-1) |
| **Performance** | GUARD-6 (overhead <5%), GUARD-7 (memory <1GB/10K), GOAL-2.2 (O(n²k) Pareto) | No latency requirements, no throughput requirements |
| **Security** | GUARD-5 (no LLM calls in core) | No input sanitization requirements for prompt text, no secrets handling |
| **Observability** | GOAL-9.1–9.5 (event system) | No metrics/counters requirements, no structured logging requirements, no alerting |
| **Persistence** | GOAL-6.1–6.6 (state/checkpoints) | No corruption recovery, no concurrent access to checkpoint files |
| **Configuration** | GOAL-7.1–7.7 (config/builder) | No environment variable support, no config file loading |
| **Scalability** | GUARD-7 (10K candidates) | No guidance beyond 10K, no population size recommendations |
| **Testing** | — | ⚠️ No testing requirements at all (test helpers, mocks, fixtures) |
| **Documentation** | GUARD-8 (Debug impls) | No doc requirements for public API |

### GUARD × Feature Cross-Reference

| GUARD | Description | Referencing Features | Status |
|---|---|---|---|
| GUARD-1 | Pareto front always valid | None | ⚠️ Unreferenced |
| GUARD-2 | Deterministic with same seed | F1 (GOAL-1.9) | ✅ |
| GUARD-3 | Adapter call order | None | ⚠️ Unreferenced |
| GUARD-4 | No panics in public API | F3 (GOAL-3.7) | Partial ✅ |
| GUARD-5 | No LLM calls in core | None | ⚠️ Unreferenced |
| GUARD-6 | Engine overhead <5% | None | ⚠️ Unreferenced |
| GUARD-7 | Memory <1GB for 10K | None | ⚠️ Unreferenced |
| GUARD-8 | Debug/Display impls | None | ⚠️ Unreferenced |
| GUARD-9 | Semver stability | F3 (implied) | Partial ✅ |

---

## ✅ Passed Checks

| Check | Description | Evidence |
|---|---|---|
| #0 | Document size | Master has 9 GUARDs (≤15). Each feature has ≤18 GOALs (largest is F1 with 18). Modular split is appropriate. ✅ |
| #1 | Specificity | 64/68 GOALs have concrete, specific conditions. 4 flagged (GOAL-1.8 ambiguous average, score semantics general). ✅ with exceptions noted. |
| #2 | Testability | 66/68 GOALs have clear pass/fail. GOAL-1.8 (best candidate selection) and GOAL-5.1 (seed bootstrap) need clarification before tests can be written. ✅ with exceptions. |
| #4 | Atomicity | 65/68 GOALs describe one thing. GOALs 1.2a-d are compound but use sub-items appropriately. ✅ |
| #6 | Happy path | Full evolution loop covered: init → iterate → select → propose → evaluate → update Pareto → terminate → return. ✅ |
| #11 | Internal consistency | 1 contradiction found (FINDING-I-2). Remaining 67 GOALs are internally consistent. ✅ with 1 exception. |
| #12 | Terminology | Mostly consistent. 2 minor issues flagged (FINDING-M-1, M-2). ✅ with minor issues. |
| #14 | Numbering/referencing | Cross-references (e.g., GOAL-1.0 → GOAL-7.3) all resolve to existing GOALs. 2 stale footers found (FINDING-I-3, I-4). ✅ with exceptions. |
| #15 | GUARDs vs GOALs alignment | No GUARD makes a GOAL unimplementable. 6 GUARDs unreferenced but not contradictory. ✅ |
| #17 | External dependencies | External dependency is only the LLM (via adapter trait). Clearly abstracted. No version pinning needed for a trait boundary. ✅ |
| #18 | Data requirements | GOAL-8.1–8.7 cover data loading comprehensively: source (adapter), format (struct), volume (shared examples), update frequency (per iteration). ✅ |
| #19 | Migration/compatibility | GUARD-9 covers semver. No legacy system to migrate from (greenfield). ✅ |
| #22 | Grouping | 9 well-organized feature groups. Related GOALs are co-located. ✅ |
| #23 | Dependency graph | Most inter-feature dependencies are expressed via cross-references (e.g., "see GOAL-X.Y"). Not exhaustive but adequate. ✅ |
| #25 | User perspective | Requirements are appropriately system-internal for a library crate. The "user" is a Rust developer using the API. GOALs read from that perspective. ✅ |
| #27 | Risk identification | No explicit risk flags, but the modular structure isolates complexity. The adapter trait (Feature 3) is the highest-risk interface — acknowledged implicitly by having 8 GOALs dedicated to it. Partial ✅. |

---

## Summary

| Metric | Value |
|---|---|
| **Total requirements** | 68 GOALs, 9 GUARDs |
| **Priority breakdown** | 42 P0, 19 P1, 7 P2 |
| **Features** | 9 (all indexed correctly in master) |
| **Critical findings** | 3 |
| **Important findings** | 8 |
| **Minor findings** | 6 |
| **Checks passed cleanly** | 16/27 |
| **Checks passed with exceptions** | 7/27 |
| **Checks with findings** | 4/27 |

### Coverage Gaps
- **Cancellation mechanism** — referenced but never defined
- **Seed bootstrap evaluation** — logical gap in initialization flow
- **Score semantics** — f64 scores lack direction/validity constraints
- **Thread-safety** — no Send/Sync requirements for async context
- **Testing** — no requirements for test infrastructure
- **Security** — minimal (only GUARD-5)
- **Observability** — events exist but no metrics/alerting

### Recommendation
**Needs fixes before design/implementation.** The 3 critical findings represent gaps that would block or confuse implementers. The priority inversions (FINDING-I-1) would cause implementation ordering problems. The remaining important findings are quality improvements that reduce ambiguity.

After fixing critical + important findings, this requirements set is **high quality** — well-structured, modular, and mostly specific and testable. The 68 GOALs cover the algorithm comprehensively.

### Estimated Implementation Clarity
**Medium-High** — An experienced Rust developer could implement ~85% of the GOALs without further clarification. The remaining ~15% (cancellation, seed bootstrap, score handling, thread safety) need the gaps filled first.
