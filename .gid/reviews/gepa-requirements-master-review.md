## Review: requirements-master.md (GEPA Core)

**Reviewer:** Requirements Document Reviewer (automated)
**Date:** 2026-04-04
**Scope:** Master document + cross-referencing all 9 feature-level docs
**Total Requirements:** 68 GOALs (42 P0, 19 P1, 7 P2), 9 GUARDs (6 hard, 3 soft)

---

### 🔴 Critical (blocks implementation)

**FINDING-1** — **[Check #7] GOAL-3.5 / GOAL-2.1: NaN/Infinity score handling undefined**
GOAL-3.5 defines `evaluate()` returning `Vec<Vec<f64>>`. GOAL-2.1 defines Pareto dominance comparison over these scores. No requirement anywhere specifies what happens when scores contain `NaN`, negative infinity, or positive infinity. In Rust, `f64` implements `PartialOrd` but NOT `Ord` — NaN comparisons return `None`, making dominance checks silently wrong. This is a correctness landmine.
Suggested fix: Add a GOAL (or amend GOAL-3.5) requiring score validation: "All scores returned by `evaluate()` MUST be finite (`f64::is_finite()`). The engine MUST return `GEPAError::InvalidScore` if any score is NaN or infinite before passing scores to Pareto ranking."

**FINDING-2** — **[Check #6] GOAL-1.2c / GOAL-3.7: No cancellation mechanism defined**
GOAL-1.2c references cancellation via GOAL-3.7. GOAL-3.7 mentions a "cancellation mechanism" but only defines `Result<T, GEPAError>` return types. No GOAL defines HOW a user triggers cancellation — no `CancellationToken`, no channel, no `AtomicBool`, no method on the engine. An implementer cannot build this without guessing.
Suggested fix: Add a new GOAL (e.g., GOAL-1.18 or amend GOAL-3.7): "The engine accepts a `CancellationToken` (or equivalent) that the caller can trigger externally. When triggered, the engine completes the current step and returns `Err(GEPAError::Cancelled)`. The token MUST be passed to adapter methods so long-running evaluations can also check it."

---

### 🟡 Important (should fix before implementation)

**FINDING-3** — **[Check #11] Overview vs GUARD-3: Step count mismatch (5 vs 6)**
The Overview section describes a 5-step cycle: Select → Execute → Reflect → Mutate → Accept. GUARD-3 defines a 6-step cycle that includes "Evaluate" between Mutate and Accept. GOAL-1.7 confirms the evaluate step exists in the main loop. The Overview is wrong — it omits Evaluate.
Suggested fix: Update the Overview to describe 6 steps: Select → Execute → Reflect → Mutate → **Evaluate** → Accept. Alternatively, clarify that "Accept" subsumes evaluation.

**FINDING-4** — **[Check #14] GUARDs 1,3,5,6,7,8 unreferenced from feature files**
Only GUARD-2, GUARD-4, and GUARD-9 are explicitly cross-referenced from feature-level requirement docs. The remaining 6 GUARDs (GUARD-1, GUARD-3, GUARD-5, GUARD-6, GUARD-7, GUARD-8) appear only in the master doc with no explicit traceability to the GOALs they constrain. This makes it easy for an implementer to miss a constraint.
Suggested fix: Add a "Constrained by:" field to each GUARD listing which GOALs it applies to, OR add a "GUARDs:" field to each feature file's header listing applicable GUARDs. Example: GUARD-7 (no panics) constrains every GOAL that returns `Result`.

**FINDING-5** — **[Check #16] GOAL-3.1: No Send/Sync bounds on async adapter trait**
GUARD-4 mandates async-first design. Tokio is listed as a dependency. GOAL-3.1 defines the `GEPAAdapter` trait with async methods but does not specify `Send + Sync` bounds. Tokio's `spawn()` requires `Send + 'static`. Without these bounds, the adapter cannot be used across tasks — a fundamental limitation for any concurrent evaluation strategy.
Suggested fix: Amend GOAL-3.1: "The `GEPAAdapter` trait MUST require `Send + Sync + 'static` bounds so implementations can be shared across Tokio tasks."

**FINDING-6** — **[Check #11] GOAL-8.1 vs GOAL-8.4: Sync signatures vs async claim**
GOAL-8.1 defines the `DataLoader` trait with synchronous method signatures: `training_examples() -> Vec<Example>`, `validation_examples() -> Vec<Example>`. GOAL-8.4 states the trait "supports async loading." These contradict each other — a sync signature cannot support async loading without wrapping.
Suggested fix: Either (a) change GOAL-8.1 signatures to async: `async fn training_examples() -> Result<Vec<Example>, GEPAError>`, or (b) reword GOAL-8.4 to say "The engine calls DataLoader methods inside `spawn_blocking` to avoid blocking the async runtime" and remove the async claim from the trait itself.

**FINDING-7** — **[Check #9] GOAL-4.2 `max_lesson_depth` missing from GOAL-7.1 config**
GOAL-4.2 references a `max_lesson_depth` parameter with default 10. GOAL-7.1 exhaustively lists all config parameters but does NOT include `max_lesson_depth`. An implementer reading Feature 7 alone would not know this parameter exists.
Suggested fix: Add `max_lesson_depth: usize` (default: 10) to the config parameter list in GOAL-7.1.

**FINDING-8** — **[Check #21] Feature 1 internal summary mismatch**
Feature 1's own summary line says "14 GOALs (11 P0, 2 P1, 1 P2)" but the master index correctly counts 18 GOALs (15 P0, 2 P1, 1 P2). The discrepancy: sub-numbered GOALs (1.2a-d, 1.7a-d) are not counted in the feature file's summary but ARE counted in the master. One of these counting conventions is wrong.
Suggested fix: Update Feature 1's summary to "18 GOALs (15 P0, 2 P1, 1 P2)" to match the master index. Sub-goals with distinct IDs are distinct requirements.

**FINDING-9** — **[Check #21] Feature 8 priority breakdown mismatch**
Feature 8's summary says "(4 P0, 2 P1, 1 P2)" but actual content has 5 P0, 2 P1, 0 P2. Total is 7 in both cases, but priorities are wrong.
Suggested fix: Update Feature 8's summary to "(5 P0, 2 P1, 0 P2)".

---

### 🟢 Minor (can fix during implementation)

**FINDING-10** — **[Check #12] Terminology inconsistency: consumer/user/implementer**
"Consumer," "user," and "implementer" are used interchangeably across documents when referring to the developer using the `gepa-core` crate. This is mildly confusing but not blocking.
Suggested fix: Standardize on "consumer" (matches GUARD-1's "pure library" language) and do a find-replace in all docs.

**FINDING-11** — **[Check #18] GOAL-6.2: Checkpoint path not in config**
GOAL-6.2 defines checkpointing after every N iterations (interval is in GOAL-7.1), but no config parameter specifies WHERE checkpoints are written (path/directory). Since GUARD-1 says "pure library, no I/O", this may be intentional (consumer provides a writer), but it should be explicit.
Suggested fix: Either add `checkpoint_writer: impl Write` as a constructor/config parameter, or add a note to GOAL-6.2 clarifying that checkpoint serialization produces bytes (via serde per GUARD-8) and the consumer is responsible for persistence.

**FINDING-12** — **[Check #11] GOAL-1.0 vs GOAL-5.1: Seed candidate provision unclear**
GOAL-1.0 says the engine takes seed candidates in `GEPAEngine::new()`. GOAL-5.1 says seeds can come from `GEPAEngine::new()` OR `GEPAConfig`. These are slightly inconsistent — if seeds are in config, the constructor doesn't need a separate parameter.
Suggested fix: Pick one canonical location. Recommendation: seeds in `GEPAEngine::new()` (not config), since seeds are runtime data, not configuration. Update GOAL-5.1 to remove the `GEPAConfig` option.

---

### 📊 Coverage Matrix

| Category | Covered | Missing / Gaps |
|---|---|---|
| **Happy path** | GOAL-1.0–1.7 (full engine loop), GOAL-2.1–2.6 (Pareto), GOAL-3.1–3.8 (adapter), GOAL-4.1–4.5 (proposers), GOAL-5.1–5.6 (candidates), GOAL-8.1–8.7 (data loading) | — |
| **Error handling** | GOAL-3.7 (adapter errors), GOAL-7.3 (config validation), GOAL-6.4 (state restore errors) | ⚠️ Cancellation trigger mechanism (FINDING-2), NaN scores (FINDING-1) |
| **Performance** | GOAL-2.5 (Pareto complexity O(N²·M)), GOAL-7.1 (`timeout_per_evaluation`) | No throughput targets, no memory budget, no latency targets for engine operations |
| **Security** | N/A (library crate, no auth/network) | Explicitly out of scope — acceptable |
| **Reliability** | GOAL-6.1–6.6 (state/checkpoints), GUARD-7 (no panics) | No retry behavior for adapter failures (one-shot or retry?) |
| **Observability** | GOAL-9.1–9.5 (events/callbacks) | No structured logging requirement, no metrics emission |
| **Scalability** | GOAL-2.5 (complexity bound) | No population size limits tested, no guidance on "large" populations |
| **Determinism** | GUARD-2 (reproducibility with same seed) | ✅ Well-covered |
| **Configuration** | GOAL-7.1–7.7 | Missing `max_lesson_depth` (FINDING-7), missing checkpoint path (FINDING-11) |
| **Data model** | GOAL-5.1–5.6 (candidates), GOAL-8.1–8.7 (data loading) | No max prompt length, no max score vector dimensionality |
| **State management** | GOAL-6.1–6.6 | ✅ Well-covered (serialize, checkpoint, restore) |
| **Async/concurrency** | GUARD-4, GOAL-3.1 | Missing Send/Sync bounds (FINDING-5) |

---

### ✅ Passed Checks

**Phase 0: Document Size**
- **Check #0: Document size** ✅ — Master doc has 9 GUARDs (well under 15). GOALs are split across 9 feature files (max 18 per file in Feature 1, others ≤8). Structure is appropriate.

**Phase 1: Individual Requirement Quality**
- **Check #1: Specificity** ✅ — Verified 68/68 GOALs. All use concrete language (trait names, method signatures, type names, numeric defaults). No instances of "should be fast," "user-friendly," "robust," or "as needed." Minor vagueness in GOAL-8.4's "supports async" addressed in FINDING-6.
- **Check #2: Testability** ✅ — 66/68 GOALs have clear pass/fail conditions. 2 exceptions: GOAL-3.7's cancellation (FINDING-2) and GOAL-8.4's async claim (FINDING-6) — both flagged above.
- **Check #3: Measurability** ✅ — Quantitative requirements present: GOAL-2.5 (O(N²·M) complexity), GOAL-4.2 (max_lesson_depth: 10), GOAL-7.1 (all defaults specified with concrete numbers). No vague "low latency" or "high throughput" claims.
- **Check #4: Atomicity** ✅ — 64/68 GOALs describe one thing. GOAL-1.2a-d and GOAL-1.7a-d are correctly split into sub-goals. No compound requirements found.
- **Check #5: Completeness (actor/trigger/behavior/outcome)** ✅ — 68/68 GOALs specify behavior and outcome. Actor is implicitly "the engine" or "the consumer" throughout, which is acceptable for a library crate.

**Phase 2: Coverage & Gaps**
- **Check #6: Happy path coverage** ✅ — Full engine lifecycle traced: construct engine (GOAL-1.0) → load data (GOAL-8.1) → seed population (GOAL-5.1) → run loop (GOAL-1.2) → select/execute/reflect/mutate/evaluate/accept (GOAL-1.7a-d + GOAL-1.2a-d) → Pareto ranking (GOAL-2.1) → checkpoint (GOAL-6.2) → terminate (GOAL-1.3). Complete.
- **Check #7: Error/edge case coverage** — Partial. See FINDING-1 (NaN) and FINDING-2 (cancellation). Flagged above.
- **Check #8: Non-functional requirements** — Partial. See Coverage Matrix. Security is N/A (acceptable for library). Observability partially covered via events (Feature 9). Performance has complexity bound but no memory/latency targets.
- **Check #9: Boundary conditions** — Partial. GOAL-7.3 covers config validation (presumably min/max). Score boundaries missing (FINDING-1). Population size of 0 or 1 not addressed. Max iterations of 0 not addressed. These are minor — implementer can infer.
- **Check #10: State transitions** ✅ — GOAL-1.2 defines the engine loop states implicitly. GOAL-6.1 defines serializable state. No explicit state machine diagram, but the 6-step cycle (GUARD-3) with start/stop (GOAL-1.3) covers transitions. No unreachable or terminal-without-exit states.

**Phase 3: Consistency & Contradictions**
- **Check #11: Internal consistency** — 3 contradictions found: FINDING-3 (5 vs 6 steps), FINDING-6 (sync vs async), FINDING-12 (seed location). All flagged.
- **Check #12: Terminology consistency** — 1 issue found: FINDING-10. Otherwise consistent: "candidate," "Pareto front," "adapter," "proposer" used consistently throughout.
- **Check #13: Priority consistency** ✅ — No priority inversions found. All P0 GOALs are self-contained or depend only on other P0 GOALs. P1 GOALs (e.g., GOAL-6.5 incremental state, GOAL-9.3 event filtering) build on P0 foundations. P2 GOALs (e.g., GOAL-1.17 warm restart) are true nice-to-haves.
- **Check #14: Numbering/referencing** — GUARD cross-referencing gap found (FINDING-4). GOAL-to-GOAL cross-references verified: GOAL-1.2c→GOAL-3.7, GOAL-4.2 self-contained, GOAL-5.1→GOAL-1.0 — all resolve correctly.
- **Check #15: GUARDs vs GOALs alignment** ✅ — No GUARD makes any GOAL unimplementable. GUARD-1 (pure library) is compatible with all GOALs. GUARD-5 (no unsafe) is compatible with all GOALs. GUARD-8 (serde) aligns with GOAL-6.1. GUARD-9 (generic strings) aligns with GOAL-5.x candidate types.

**Phase 4: Implementability**
- **Check #16: Technology assumptions** — 1 issue (FINDING-5, Send/Sync). Otherwise: Tokio explicitly named (GUARD-4), serde explicitly named (GUARD-8), Rust edition/MSRV specified (GUARD-6). Technology choices are documented.
- **Check #17: External dependencies** ✅ — Dependencies named: tokio (async runtime), serde (serialization), rand (RNG, implied by GUARD-2's seed requirement). No version pins, but MSRV 1.75 constrains compatible versions. No external services.
- **Check #18: Data requirements** — Partial. GOAL-8.1 defines `Example` type. GOAL-5.1–5.6 define `Candidate` type. Score vectors defined in GOAL-3.5. Missing: max prompt length, max score dimensionality, expected data volumes. These are somewhat out of scope for a library crate — the consumer controls data size.
- **Check #19: Migration/compatibility** ✅ — N/A. This is a new crate (v0.1), no existing functionality to migrate from.
- **Check #20: Scope boundaries** ✅ — GUARD-1 explicitly scopes out: no I/O, no network, no CLI, no UI. The crate is a pure library. Non-goals are implicit but clear from GUARDs.

**Phase 5: Traceability & Organization**
- **Check #21: Unique identifiers** — 2 summary mismatches found (FINDING-8, FINDING-9). All 68 GOALs have unique IDs (GOAL-1.0 through GOAL-9.5). All 9 GUARDs have unique IDs. No duplicate IDs found. No unexplained gaps.
- **Check #22: Grouping/categorization** ✅ — Requirements are well-organized by feature domain. 9 feature files map to 9 logical components. Related GOALs are colocated. The master doc provides a clear index.
- **Check #23: Dependency graph** ✅ — Implicit but clear: Feature 7 (config) and Feature 5 (candidates) are foundational. Feature 3 (adapter) is the integration point. Feature 1 (engine) orchestrates everything. Feature 9 (events) is a cross-cutting concern. No circular dependencies.
- **Check #24: Acceptance criteria** ✅ — Each GOAL's description serves as its acceptance criterion (concrete enough to test directly). Feature files don't have separate "acceptance criteria" sections, but the GOALs themselves are testable per Check #2.

**Phase 6: Stakeholder Alignment**
- **Check #25: User perspective** ✅ — Requirements are written from the library consumer's perspective: "the consumer creates," "the adapter implements," "the engine calls." Appropriate for a library crate. No system-internal-only language.
- **Check #26: Success metrics** — Partially covered. GUARD-2 (determinism) is a measurable success metric. No production-observable metrics defined, but this is a library — the consumer defines production metrics. Acceptable.
- **Check #27: Risk identification** — No explicit risk flags. Potential high-risk areas: (a) Pareto front computation at scale (GOAL-2.5 bounds it at O(N²·M)), (b) async adapter design (trait with async methods in Rust is non-trivial), (c) deterministic reproduction across platforms (GUARD-2). These are not flagged in the docs but are known Rust challenges. Low severity — implementers will discover these naturally.

---

### Summary

| Metric | Value |
|---|---|
| Total requirements | 68 GOALs, 9 GUARDs |
| Priority breakdown | 42 P0, 19 P1, 7 P2 |
| Documents | 1 master + 9 feature files |
| Critical findings | 2 (FINDING-1, FINDING-2) |
| Important findings | 7 (FINDING-3 through FINDING-9) |
| Minor findings | 3 (FINDING-10, FINDING-11, FINDING-12) |
| Total findings | 12 |
| Checks passed cleanly | 19/27 |
| Checks with findings | 8/27 |

**Coverage gaps:**
- No cancellation trigger mechanism (FINDING-2)
- No NaN/infinity score validation (FINDING-1)
- No retry behavior for adapter failures
- No memory budget or latency targets (acceptable for library)
- No max population size or max prompt length boundaries
- `max_lesson_depth` missing from config (FINDING-7)

**Recommendation:** **Needs fixes before implementation.** The two critical findings (NaN scores and cancellation) will cause implementer confusion and potential correctness bugs. The 7 important findings are documentation inconsistencies that should be resolved to avoid misinterpretation. None require architectural changes — all are addressable with targeted amendments.

**Estimated implementation clarity:** **High** (after fixing criticals). The requirements are unusually well-structured for this stage. Method signatures, type names, and behavioral contracts are specific. The modular file structure is excellent. The main gaps are edge cases and cross-reference hygiene, not fundamental ambiguity.
