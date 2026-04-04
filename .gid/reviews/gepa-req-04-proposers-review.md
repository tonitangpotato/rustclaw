# Review: requirements-04-proposers.md

> Reviewed: 2026-04-04 | Reviewer: requirements-review skill | Document: GEPA Proposers (Feature 4 of 9)

---

## Phase 0: Document Size Check

**5 GOALs** — well within the ≤15 GOAL limit. ✅

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #5] GOAL-4.1: Missing specification of what "execute → reflect → mutate" means in proposer context**

GOAL-4.1 says the mutation proposer "runs execute → reflect → mutate via the adapter." This is the same sequence described in GOAL-1.1 (the core engine loop). It's unclear whether:
- (a) The mutation proposer **is** the engine loop's implementation of steps 2-4, or
- (b) The mutation proposer is a separate component that independently calls execute → reflect → mutate.

If (a), then GOAL-4.1 is redundant with GOAL-1.1/1.4/1.5/1.6 and the requirement should clarify it's specifying the implementation of those steps. If (b), there are two independent callers of the adapter per iteration, which contradicts GUARD-3's strict call ordering.

**Suggested fix:** Clarify the relationship: "The mutation proposer implements the core engine loop's execute → reflect → mutate steps (GOAL-1.4, GOAL-1.5, GOAL-1.6). It is the component that the engine delegates to for candidate generation."

### FINDING-2
**[Check #7] GOAL-4.1: No error handling specification for mutation proposer**

GOAL-4.1 says the proposer "produces exactly one new candidate per iteration." What happens when:
- The adapter's `execute` call fails?
- The adapter's `reflect` call fails?
- The adapter's `mutate` call returns an error?
- The adapter returns a candidate with invalid/empty text parameters?

GOAL-7.5 defines retry policy for adapter errors at the engine level, but GOAL-4.1 doesn't reference it or clarify that proposer errors bubble up to the engine's error handling. The "exactly one" phrasing implies infallibility.

**Suggested fix:** Add: "If any adapter call fails after retry exhaustion (per GOAL-7.5), the proposer propagates the error to the engine, which applies the configured error policy (skip or halt). 'Exactly one candidate per iteration' applies to the success path."

### FINDING-3
**[Check #9] GOAL-4.4: "Complementary" definition has ambiguous boundary conditions**

GOAL-4.4 defines complementary as "maximize |A_better ∪ B_better|" where A_better is examples where A scores higher and B_better where B scores higher. Several boundary conditions are unspecified:

1. **Ties**: If A and B score identically on an example, it belongs to neither A_better nor B_better. This means two identical candidates have |A_better ∪ B_better| = 0, which is the minimum. That's reasonable but should be stated explicitly.
2. **Score source**: Which scores are used? The evaluation cache (GOAL-6.3)? Only shared examples? The requirement doesn't specify. If candidates have sparse score matrices (GOAL-1.7a), the union could be meaningless on non-overlapping evaluations.
3. **Front size = 1**: If the Pareto front has only one candidate, merge is impossible. What happens?
4. **Front size = 2**: Only one pair exists. Is the complementarity check still run, or is the pair automatically selected?

**Suggested fix:** Add: "Complementarity is computed over the intersection of examples both candidates have been evaluated on (from the evaluation cache, GOAL-6.3). If the front has fewer than 2 candidates, the merge step is skipped for that interval. Ties (identical scores on an example) count toward neither A_better nor B_better."

---

## 🟡 Important (should fix before implementation)

### FINDING-4
**[Check #4] GOAL-4.2: Compound requirement — lesson chain construction + depth limiting**

GOAL-4.2 specifies two distinct behaviors:
1. Constructing the ancestor lesson chain (ordered from most recent to oldest)
2. Truncating to a configurable maximum depth (default: 10)

These are independently testable and implementable. The truncation behavior is a separate concern from chain construction.

**Suggested fix:** Split into:
- **GOAL-4.2a**: "The mutation proposer constructs the ancestor lesson chain: the reflections from all ancestors in the lineage (parent, grandparent, etc.), ordered from most recent to oldest."
- **GOAL-4.2b**: "If the ancestor lesson chain exceeds a configurable maximum lesson depth (default: 10, see GOAL-7.x), only the most recent N lessons are passed to the adapter's `mutate` method."

### FINDING-5
**[Check #1] GOAL-4.3: "Balanced selection" is vague — what constitutes balanced?**

GOAL-4.3 says "ensures balanced selection: no front member is starved" and references GOAL-2.3. However, GOAL-2.3 defines the actual mechanism (round-robin floor: every front member selected at least once every `pareto_max_size` iterations). GOAL-4.3 adds no new information beyond GOAL-2.3 — it just restates it in vaguer terms.

Is GOAL-4.3 the proposer's responsibility to implement, while GOAL-2.3 is the Pareto front module's specification? If so, GOAL-4.3 should specify what the proposer does differently from GOAL-2.3. If not, GOAL-4.3 is redundant.

**Suggested fix:** Either (a) remove GOAL-4.3 as redundant with GOAL-2.3, or (b) clarify: "The mutation proposer maintains a selection counter per front member and enforces the round-robin floor described in GOAL-2.3: it tracks selections and ensures every front member is selected at least once every `pareto_max_size` iterations before any member is selected again. The proposer owns the tracking state; GOAL-2.3 defines the policy."

### FINDING-6
**[Check #18] GOAL-4.2: Maximum lesson depth config not cross-referenced to GOAL-7.x**

GOAL-4.2 mentions "a configurable maximum lesson depth (default: 10)" but this parameter is not listed in GOAL-7.1's config parameter enumeration, nor in any GOAL-7.x. This means there's no config requirement for this parameter — an implementer wouldn't know where to put it.

**Suggested fix:** Either (a) add `max_lesson_depth` to GOAL-7.1's config parameter list, or (b) add a new GOAL-7.8 for proposer-specific config parameters including `max_lesson_depth`.

### FINDING-7
**[Check #5] GOAL-4.5: Missing specification of merge candidate output format**

GOAL-4.5 specifies what context the merge proposer provides to the adapter's `merge` method, but doesn't specify:
- What the adapter returns (GOAL-3.6 says "returns a new candidate" — should be cross-referenced).
- How the merged candidate's lineage is constructed — does it have two parents? Is lineage tree-shaped instead of chain-shaped?
- How the merged candidate is evaluated and accepted (the requirement says "GOAL-1.10 specifies WHEN" but doesn't clarify that acceptance follows the same GOAL-1.7d rules).

**Suggested fix:** Add: "The adapter's `merge` method returns a new `Candidate` (per GOAL-3.6). The merged candidate has both parents recorded in its lineage (tree-structured). After production, the merged candidate is evaluated and accepted/rejected using the same rules as mutated candidates (GOAL-1.7, GOAL-1.7d)."

### FINDING-8
**[Check #16] GOAL-4.4: Complementary pair selection algorithm complexity unspecified**

GOAL-4.4 requires finding the pair with maximum |A_better ∪ B_better| across all pairs on the front. For a front of size N with M examples each, this is O(N² · M). For N=50 and M=200, that's 500,000 operations per merge step. Is this acceptable? The requirement should either:
- State the expected complexity, or
- Reference GUARD-6 (engine overhead < 5%) as the performance bound.

**Suggested fix:** Add: "Complementary pair selection scans all O(N²) pairs where N is the front size. For typical workloads (N ≤ 50, M ≤ 200), this is negligible relative to adapter call time (per GUARD-6)."

### FINDING-9
**[Check #14] Cross-reference "GOAL-3.x" is imprecise**

The cross-references section says "GOAL-3.x (Adapter) — execute, reflect, mutate, merge methods." This should reference specific GOALs: GOAL-3.1 (trait definition), GOAL-3.2 (execute), GOAL-3.3 (reflect), GOAL-3.4 (mutate), GOAL-3.6 (merge). "GOAL-3.x" is not a valid cross-reference.

**Suggested fix:** Replace "GOAL-3.x (Adapter) — execute, reflect, mutate, merge methods" with "GOAL-3.2 (execute), GOAL-3.3 (reflect), GOAL-3.4 (mutate), GOAL-3.6 (merge) — adapter methods used by proposers"

---

## 🟢 Minor (can fix during implementation)

### FINDING-10
**[Check #21] Summary says 5 GOALs but feature index in master says 5 — consistent ✓. Minor: GOAL numbering gap**

GOALs are numbered 4.1 through 4.5 with no gaps. However, if GOAL-4.2 is split per FINDING-4, renumbering to 4.2a/4.2b would be needed. No action needed now.

### FINDING-11
**[Check #25] Requirements are system-internal, not user-perspective**

All 5 GOALs are written from the system's perspective ("the mutation proposer selects..."). Since this is an internal library component (proposers are not user-facing), this is appropriate. However, GOAL-4.4's complementarity definition could benefit from a brief rationale from the user's perspective: "This ensures the merge combines specialists, not generalists, which produces more diverse merged candidates."

**Suggested fix:** Add a brief rationale sentence to GOAL-4.4 explaining the user benefit of complementary selection.

### FINDING-12
**[Check #12] Minor terminology: "text parameters" vs "candidate"**

GOAL-4.5 says "both parent candidates' text parameters" — elsewhere the system refers to passing `&Candidate` objects (GOAL-3.4, GOAL-3.6). The merge method in GOAL-3.6 receives "two parent candidates" (whole Candidate objects), not just text parameters. GOAL-4.5 should align with GOAL-3.6's signature.

**Suggested fix:** Change "both parent candidates' text parameters" to "both parent `Candidate` objects (which include text parameters)."

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path — mutation | GOAL-4.1, GOAL-4.2 | - |
| Happy path — merge | GOAL-4.4, GOAL-4.5 | - |
| Selection fairness | GOAL-4.3 (via GOAL-2.3) | - |
| Error handling | ❌ | ⚠️ No error handling for proposer failures (FINDING-2) |
| Edge cases — merge | ❌ | ⚠️ Front size <2, all candidates identical (FINDING-3) |
| Edge cases — mutation | ❌ | ⚠️ Empty lineage (first iteration, seed candidate has no ancestors) |
| Performance | ❌ | ⚠️ No complexity bounds for complementary pair search (FINDING-8) |
| Security | N/A | Internal library component — not applicable |
| Observability | ❌ | ⚠️ No logging/tracing requirements for proposer decisions (which parent selected, why, merge pair chosen, etc.) — likely covered by GOAL-9.x events but not cross-referenced |
| Reliability | ❌ | ⚠️ No retry/fallback behavior specified for proposers |
| Config integration | GOAL-4.2 (max depth), GOAL-4.4 (via GOAL-7.7) | ⚠️ max_lesson_depth not in GOAL-7.x (FINDING-6) |
| Lineage/ancestry | GOAL-4.2 | ⚠️ Merge candidate lineage (tree vs chain) unspecified (FINDING-7) |

---

## ✅ Passed Checks

| Check | Result | Evidence |
|---|---|---|
| #0 Document size | ✅ | 5 GOALs, well within ≤15 limit |
| #1 Specificity | ⚠️ Partial | 4/5 GOALs are specific. GOAL-4.3 is vague ("balanced") — see FINDING-5 |
| #2 Testability | ✅ | 5/5 GOALs have testable pass/fail conditions. GOAL-4.1: produces exactly one candidate. GOAL-4.2: lesson chain ordered recent-to-oldest, truncated at max. GOAL-4.3: no member starved (verifiable via selection counter). GOAL-4.4: pair maximizes |A_better ∪ B_better|. GOAL-4.5: merge method receives specified context. |
| #3 Measurability | ✅ | GOAL-4.2 has concrete default (10). GOAL-4.4 has a concrete formula. No vague quantitative claims. |
| #4 Atomicity | ⚠️ Partial | GOAL-4.2 is compound (FINDING-4). Other 4 GOALs are atomic. |
| #5 Completeness | ⚠️ Partial | GOAL-4.1 missing error path (FINDING-2). GOAL-4.5 missing output spec (FINDING-7). Other GOALs specify actor/behavior/outcome. |
| #6 Happy path | ✅ | Both mutation (GOAL-4.1, 4.2, 4.3) and merge (GOAL-4.4, 4.5) happy paths are covered. |
| #7 Error/edge cases | ❌ | No error handling specified (FINDING-2). Merge edge cases missing (FINDING-3). |
| #8 Non-functional reqs | ⚠️ Partial | Performance not specified (FINDING-8). Security N/A. Observability not referenced. Reliability not specified. |
| #9 Boundary conditions | ❌ | Merge with <2 front members, empty lineage, identical candidates unspecified (FINDING-3). |
| #10 State transitions | ✅ | No state machine in proposers. Proposers are stateless transformations (select → produce candidate). GOAL-4.3's selection tracking is the only state, and it's adequately described via GOAL-2.3. |
| #11 Internal consistency | ✅ | No contradictions found among the 5 GOALs. All 10 pairwise combinations checked. |
| #12 Terminology | ⚠️ Minor | "text parameters" vs "Candidate" inconsistency (FINDING-12). Otherwise consistent. |
| #13 Priority consistency | ✅ | P0 (GOAL-4.1, 4.2) have no dependencies on P2 (GOAL-4.4, 4.5). P1 (GOAL-4.3) depends on P0 GOAL-2.3. No inversions. |
| #14 Numbering/referencing | ⚠️ Partial | Cross-reference "GOAL-3.x" is imprecise (FINDING-9). GOAL-1.10, GOAL-2.3, GOAL-7.7 all resolve correctly. |
| #15 GUARDs vs GOALs | ✅ | GUARD-2 (immutability): proposers produce new candidates, not modify existing — consistent. GUARD-3 (call order): proposers call execute→reflect→mutate in order — consistent. GUARD-5 (no LLM calls): proposers delegate to adapter — consistent. GUARD-9 (determinism): selection in GOAL-4.3/4.4 uses seeded RNG — consistent with determinism requirement. No contradictions. |
| #16 Technology assumptions | ✅ | No implicit technology assumptions. Proposers are pure algorithmic components. |
| #17 External dependencies | ✅ | Proposers depend only on the adapter trait (internal dependency). No external services. |
| #18 Data requirements | ⚠️ Partial | Lesson chain data source specified (lineage). Merge score data source not specified (FINDING-3 point 2). |
| #19 Migration/compatibility | ✅ | N/A — new system, no migration needed. |
| #20 Scope boundaries | ⚠️ Minor | No explicit non-goals for proposers. E.g., "Proposers do not implement crossover, tournament selection, or other evolutionary operators beyond mutation and merge." Would help prevent scope creep. |
| #21 Unique identifiers | ✅ | GOAL-4.1 through GOAL-4.5, no duplicates, no gaps. |
| #22 Grouping/categorization | ✅ | Logically grouped: mutation proposer (4.1-4.3), merge proposer (4.4-4.5). Clear organization. |
| #23 Dependency graph | ⚠️ Partial | Implicit dependencies exist but aren't explicitly stated. GOAL-4.1 depends on GOAL-2.3 (selection) and GOAL-3.2/3.3/3.4 (adapter). GOAL-4.2 depends on GOAL-5.x (candidate lineage/ancestry storage). GOAL-4.4 depends on GOAL-6.3 (evaluation cache for scores). These should be explicit. |
| #24 Acceptance criteria | ⚠️ Partial | GOALs have implicit acceptance criteria via their specifications. No separate acceptance criteria section. Acceptable for this document size. |
| #25 User perspective | ✅ | Internal library component — system perspective is appropriate (FINDING-11 is minor). |
| #26 Success metrics | ⚠️ Minor | No observable metrics for proposer quality. E.g., "Mutation acceptance rate should be tracked" — likely in GOAL-9.x events but not referenced here. |
| #27 Risk identification | ✅ | No high-risk items in this document. Complementary pair selection (GOAL-4.4) is algorithmically straightforward. Master doc identifies risks for related areas (score alignment, crowding distance). |

---

## Summary

- **Total requirements:** 5 GOALs (2 P0, 1 P1, 2 P2), 0 GUARDs (GUARDs in master)
- **Critical:** 3 (FINDING-1, FINDING-2, FINDING-3)
- **Important:** 6 (FINDING-4 through FINDING-9)
- **Minor:** 3 (FINDING-10, FINDING-11, FINDING-12)
- **Total findings:** 12

### Coverage Gaps
- **Error handling**: No error/failure path for proposer operations
- **Edge cases**: Merge with small front, empty lineage, identical candidates
- **Performance**: No complexity/overhead bounds for proposer algorithms
- **Observability**: No logging/event requirements for proposer decisions
- **Config integration**: `max_lesson_depth` parameter not in config requirements

### Recommendation
**Needs fixes first** — the 3 critical findings (relationship to engine loop, error handling, merge boundary conditions) must be resolved before implementation. The document is well-structured and concise, but assumes too much implicit knowledge about how proposers integrate with the engine loop. An implementer would need to ask clarifying questions before starting.

### Estimated Implementation Clarity
**Medium** — mutation proposer (GOAL-4.1, 4.2) is fairly clear once the engine-loop relationship is clarified. Merge proposer (GOAL-4.4, 4.5) needs more boundary condition specification. Selection tracking (GOAL-4.3) is unclear about ownership vs. delegation to GOAL-2.3.
