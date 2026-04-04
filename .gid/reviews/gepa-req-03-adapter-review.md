# Review: requirements-03-adapter.md (GEPA Adapter Interface)

**Reviewed:** 2026-04-04
**Reviewer:** Automated Requirements Review (27-check)
**Document:** `.gid/features/gepa-core/requirements-03-adapter.md`
**Context:** `.gid/features/gepa-core/requirements-master.md` (GUARDs), `requirements-01-core-engine.md`, `requirements-07-config.md`

---

## Phase 0: Document Size Check

**Total GOALs: 8** (5 P0, 2 P1, 1 P2) — ✅ Under the 15-GOAL limit.

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #5] GOAL-3.2: Missing error/failure semantics for `execute`**
GOAL-3.2 specifies what `execute` returns on success (`Vec<ExecutionTrace>`) but doesn't specify what happens when execution partially fails. If 5 of 16 examples fail to execute (e.g., LLM produces garbage for some), does the adapter:
(a) return a trace with `None` score and empty output?
(b) omit those examples from the result vec?
(c) return an error for the entire batch?

This matters because the Reflect step (GOAL-3.3) consumes these traces, and the engine needs to know whether the Vec length always equals the input slice length.

**Suggested fix:** Add to GOAL-3.2: "The returned `Vec<ExecutionTrace>` has exactly one entry per input example, in the same order. If execution fails for a specific example, the adapter sets the trace's output to an empty string and score to `None`, and may include diagnostic information in the ASI field. Whole-batch failures are returned as `Err(GEPAError::AdapterError { ... })`."

### FINDING-2
**[Check #5] GOAL-3.5: Missing output length/ordering contract for `evaluate`**
GOAL-3.5 says `evaluate` returns `Vec<f64>` of per-example scores but doesn't specify:
(a) Must the Vec length equal the input slice length? (Presumably yes.)
(b) Must scores be in the same order as input examples? (Presumably yes.)
(c) What is the valid range for scores? Can scores be negative? NaN? Infinity?

This is critical because these scores feed directly into Pareto dominance comparison (GOAL-1.7d), and NaN/Inf values would break dominance logic silently.

**Suggested fix:** Add to GOAL-3.5: "The returned `Vec<f64>` has exactly one entry per input example, in the same order. All scores must be finite (`f64::is_finite()`). The engine validates this post-condition and returns `GEPAError::AdapterError` if violated. Score range is unconstrained (negative values are permitted); higher scores indicate better performance."

### FINDING-3
**[Check #7] Missing: Partial adapter failure handling across methods**
GOAL-3.7 defines `GEPAError` variants and says the engine retries per GOAL-7.5. But the adapter interface document doesn't specify what should happen for each method on failure:
- If `execute` fails → skip iteration (ok, GOAL-7.5 covers this)
- If `reflect` fails after `execute` succeeded → skip? Retry just reflect? The execution traces are already computed.
- If `mutate` fails after `reflect` succeeded → skip? Retry just mutate?
- If `evaluate` fails after `mutate` succeeded → the new candidate exists but has no scores.

The engine calls these in sequence (GUARD-3). If a mid-sequence method fails, what happens to the partial work? This is ambiguous.

**Suggested fix:** Add a new GOAL or expand GOAL-3.7: "Retry policy applies to each adapter call independently. If an adapter call fails after exhausting retries, the entire iteration is skipped — partial results from earlier steps in that iteration are discarded. The candidate produced by a failed-iteration's `mutate` is not added to history."

---

## 🟡 Important (should fix before implementation)

### FINDING-4
**[Check #1] GOAL-3.8: Vague measurability of "< 50 lines of boilerplate"**
While 50 lines is technically a number, it's hard to enforce: does this count blank lines? Comments? Import statements? `use` declarations? The trait method signatures themselves? This is a developer experience goal that's hard to verify mechanically.

**Suggested fix:** Reword to: "A minimal working adapter implementing all 4 required methods can be written in ≤ 50 non-blank, non-comment lines of Rust (excluding `use` declarations and module boilerplate). The crate's `examples/` directory includes a `minimal_adapter.rs` demonstrating this."

### FINDING-5
**[Check #4] GOAL-3.1: Compound requirement**
GOAL-3.1 defines the trait AND lists all 4 required methods AND explains the difference between `execute` and `evaluate`. This is really 3 things:
1. The trait exists and is async
2. The list of required methods
3. The semantic distinction between `execute` and `evaluate`

While the subsequent GOALs (3.2–3.5) do break out each method, GOAL-3.1 restates them all again, creating redundancy and potential for contradiction if one is updated without the other.

**Suggested fix:** Simplify GOAL-3.1 to: "`GEPAAdapter` is an async trait with 4 required methods: `execute`, `reflect`, `mutate`, and `evaluate`, each specified in GOAL-3.2 through GOAL-3.5 respectively. The trait uses `async_trait` for async support."

### FINDING-6
**[Check #9] GOAL-3.4: Boundary conditions for ancestor lessons**
GOAL-3.4 says `mutate` receives "a slice of ancestor lessons (accumulated from the lineage)". What happens when:
- The candidate is a seed candidate with no ancestors → empty slice? Explicitly stated?
- The lineage is very long (100+ generations) → is the slice all ancestors or truncated?
- An ancestor's reflection/lesson was empty (failed reflect step)?

These boundary conditions affect implementation decisions.

**Suggested fix:** Add to GOAL-3.4: "For seed candidates with no lineage, the ancestor lessons slice is empty. The engine accumulates lessons from all ancestors in the candidate's lineage (no truncation). Lesson accumulation strategy (full history vs windowed) is configurable — see GOAL-5.x."

### FINDING-7
**[Check #16] Missing: Async runtime assumptions**
The trait is described as "async" but the requirements don't specify:
- Whether the adapter methods may spawn their own tokio tasks
- Whether adapter calls happen concurrently or sequentially within an iteration
- Whether `Send + Sync` bounds are required on the trait

GUARD-3 implies sequential within an iteration, but could multiple iterations run concurrently? GUARD-9 (determinism) implies no, but this isn't stated in the adapter doc.

**Suggested fix:** Add to GOAL-3.1 or a new GOAL: "All `GEPAAdapter` methods must be `Send`. The engine calls adapter methods sequentially within each iteration (one at a time, never concurrently). Adapter implementations may use internal concurrency (e.g., parallel LLM calls within `execute`) as long as they return a single result. The engine runs one iteration at a time (no concurrent iterations)."

### FINDING-8
**[Check #15] GUARD-3 vs GOAL-3.6 (`merge`): Call sequence ambiguity**
GUARD-3 specifies the call order per iteration is: `select → execute → reflect → mutate → evaluate → accept`. GOAL-3.6 introduces `merge`, and GOAL-1.10 says merge happens "periodically." But `merge` is not listed in GUARD-3's call sequence. This creates ambiguity: does a merge iteration follow the same sequence? Does merge replace mutate? Is it a separate iteration type?

**Suggested fix:** This should be addressed in GUARD-3 (master doc), but GOAL-3.6 should reference it: "The merge step runs as an alternative iteration type where `merge` replaces the `reflect + mutate` steps. The merge iteration sequence is: `select (two candidates) → merge → evaluate → accept`. See GUARD-3 for call sequence constraints."

### FINDING-9
**[Check #18] GOAL-3.3: Data format underspecified for Reflection**
GOAL-3.3 says `Reflection` contains "a natural-language diagnosis string and a list of suggested improvement directions." The "list of suggested improvement directions" — what type is each element? Strings? Structured objects? How many? Can the list be empty?

**Suggested fix:** Specify: "`Reflection` contains: a `diagnosis: String` field and an `improvements: Vec<String>` field. The improvements list may be empty (the adapter found no specific improvement directions). Each improvement is a natural-language description of a single actionable change."

### FINDING-10
**[Check #8] Missing: Timeout/cancellation behavior for individual adapter calls**
GOAL-3.7 defines `Timeout` and `Cancelled` error variants, but there's no requirement specifying:
- Who enforces timeouts — the engine or the adapter?
- Is there a per-call timeout configuration?
- How is cancellation signaled to the adapter (CancellationToken? Drop?)

**Suggested fix:** Add a new GOAL (GOAL-3.9 [P1]): "The engine enforces per-adapter-call timeouts via `tokio::time::timeout`. Timeout duration is configurable in `GEPAConfig` (default: 60 seconds per adapter call). When a timeout triggers, the engine cancels the adapter future and returns `GEPAError::Timeout`. Cancellation of the entire run is signaled via a `tokio_util::sync::CancellationToken` passed to `GEPAEngine::run()` — the engine checks the token at the start of each iteration and between adapter calls."

---

## 🟢 Minor (can fix during implementation)

### FINDING-11
**[Check #12] Terminology: "input examples" vs "examples" vs "training examples"**
GOAL-3.2 says "a slice of input examples." GOAL-3.5 says "a slice of input examples." GOAL-1.4 says "training examples." The master doc says "task subset" and "examples." While the meaning is clear, the modifier varies ("input", "training", bare "examples").

**Suggested fix:** Standardize on "examples" (bare) in the adapter interface since the adapter doesn't care whether they're training or test. Use "training examples" only in the data loading context (requirements-08).

### FINDING-12
**[Check #21] No gaps but numbering starts at 3.1**
The numbering GOAL-3.1 through GOAL-3.8 is sequential with no gaps. ✅ Minor note: the feature-scoped numbering (3.x) is clear and traceable.

### FINDING-13
**[Check #22] Good grouping, but could use subsections**
All 8 GOALs are in a single flat "Goals" section. Grouping into subsections (e.g., "Required Methods", "Error Handling", "Developer Experience") would improve scanability but isn't blocking.

### FINDING-14
**[Check #25] User perspective language**
Requirements are mostly system-internal ("the adapter receives..."). GOAL-3.8 is the only user-facing requirement. This is appropriate for a trait interface spec — the "user" is an adapter implementer, and the language reflects that. Minor: GOAL-3.8 could be strengthened by listing exactly what the adapter author needs to know.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path | GOAL-3.1, 3.2, 3.3, 3.4, 3.5 | ✅ Full method signatures covered |
| Error handling | GOAL-3.7 (error types + retry ref) | ⚠️ Partial failure per-example (FINDING-1), mid-sequence failure (FINDING-3) |
| Merge (optional) | GOAL-3.6 | ⚠️ Call sequence undefined (FINDING-8) |
| Performance | — | ⚠️ No per-call timeout specified (FINDING-10); covered partially by GUARD-6 in master |
| Security | — | Not applicable (no auth, no network — GUARD-5) |
| Reliability | GOAL-3.7 (retryable flag, retry_after) | ⚠️ No per-call timeout (FINDING-10) |
| Observability | — | No adapter-level logging/tracing requirements (deferred to GOAL-9.x) |
| Scalability | — | Not applicable at trait level |
| Boundary conditions | — | ⚠️ Empty inputs, score ranges, empty ancestor lessons (FINDING-2, FINDING-6) |
| State transitions | N/A (stateless trait) | ✅ |
| DX / Ergonomics | GOAL-3.8 | ✅ |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 8 GOALs, well under 15-GOAL limit.
- **Check #2: Testability** ✅ — 8/8 GOALs have testable conditions. GOAL-3.1: trait compiles with required methods. GOAL-3.2–3.5: method signatures match spec. GOAL-3.6: merge returns Err by default. GOAL-3.7: error variants exist and match. GOAL-3.8: example adapter < 50 lines (though measurability is flagged separately).
- **Check #3: Measurability** ✅ — Only one quantitative requirement (GOAL-3.8, "< 50 lines"), which is concrete enough (flagged for clarification as FINDING-4, but not a measurability failure).
- **Check #10: State transitions** ✅ — The adapter trait is stateless from the engine's perspective. No state machine.
- **Check #11: Internal consistency** ✅ — Verified all 8 GOALs pairwise. No contradictions found. GOAL-3.1's method list matches GOAL-3.2–3.5. GOAL-3.7's error types are consistent with GOAL-3.6's default Err behavior.
- **Check #12: Terminology consistency** ⚠️ Minor issue flagged (FINDING-11), not a contradiction.
- **Check #13: Priority consistency** ✅ — P0 GOALs (3.1–3.5) have no dependencies on P1/P2 GOALs. GOAL-3.6 [P1] depends on GOAL-3.1 [P0] ✅. GOAL-3.7 [P1] depends on GOAL-3.1 [P0] ✅. GOAL-3.8 [P2] depends on GOAL-3.1–3.5 [P0] ✅. No priority inversions.
- **Check #14: Cross-references** ✅ — GOAL-3.6 references GOAL-1.10 (exists ✅). GOAL-3.7 references GOAL-7.5 (exists ✅). Cross-references section cites GOAL-1.x and GOAL-7.5 (valid ✅).
- **Check #15: GUARDs vs GOALs alignment** ⚠️ — Mostly aligned. GUARD-2 (immutable candidates) is consistent with GOAL-3.4 (mutate returns *new* candidate). GUARD-3 (call sequence) has ambiguity with GOAL-3.6 merge (FINDING-8). GUARD-5 (no LLM calls) is the raison d'être of GOAL-3.1. GUARD-8 (Debug, Error traits) should apply to `GEPAError` in GOAL-3.7 — not explicitly stated but implied. GUARD-9 (determinism) is compatible — adapter is the non-deterministic part.
- **Check #17: External dependencies** ✅ — The adapter trait IS the external dependency boundary. By design, GEPA has no external dependencies (GUARD-5). Adapter implementers bring their own. This is well-scoped.
- **Check #19: Migration/compatibility** ✅ — N/A, this is a new crate. No migration needed.
- **Check #20: Scope boundaries** ✅ — Master doc's "Out of Scope" section covers this well: no LLM integration, no domain-specific logic. The adapter doc implicitly inherits these.
- **Check #21: Unique identifiers** ✅ — 8 GOALs numbered 3.1–3.8, no duplicates, no gaps.
- **Check #23: Dependency graph** ✅ — Implicit but clear: GOAL-3.1 is the root (trait definition), GOAL-3.2–3.5 depend on it (method specs), GOAL-3.6–3.7 extend it (optional method, error handling), GOAL-3.8 depends on all (DX). No circular dependencies.
- **Check #26: Success metrics** ⚠️ — Partially covered by GOAL-3.8 (50-line minimal adapter). Production success would mean adapter implementers find the trait intuitive, but that's not measurable in requirements. Acceptable for a trait interface spec.
- **Check #27: Risk identification** ✅ — No high-risk items in this feature. The adapter interface is a straightforward trait definition. The risks lie in the *consumers* of the trait (core engine, Pareto front), which are identified in the master doc's Risks section.

---

## Summary

| Metric | Value |
|---|---|
| Total requirements | 8 GOALs, 0 GUARDs (GUARDs in master) |
| Critical findings | 3 (FINDING-1, FINDING-2, FINDING-3) |
| Important findings | 7 (FINDING-4 through FINDING-10) |
| Minor findings | 4 (FINDING-11 through FINDING-14) |
| Total findings | 14 |
| Coverage gaps | Partial failure semantics, mid-sequence error handling, timeout/cancellation, score validation, boundary conditions |
| Recommendation | **Needs fixes first** — 3 critical findings on output contracts and failure semantics must be resolved before implementation. The trait signatures will be wrong without FINDING-1 and FINDING-2. |
| Implementation clarity | **Medium** — Method signatures are clear, but failure modes and boundary conditions need tightening. An implementer would have 3–5 questions before starting. |
