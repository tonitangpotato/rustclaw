# Review: requirements-05-candidates.md (GEPA Candidate Management)

**Reviewed:** 2026-04-04
**Reviewer:** Automated Requirements Review (27-check process)
**Document:** `.gid/features/gepa-core/requirements-05-candidates.md`
**Master doc:** `.gid/features/gepa-core/requirements-master.md`

---

## Phase 0: Document Size Check

**6 GOALs** — well within the ≤15 GOAL limit. ✅

---

## 🔴 Critical (blocks implementation)

### FINDING-1
**[Check #5] GOAL-5.4: Candidate ID generation mechanism unspecified**
GOAL-5.4 requires IDs be unique within a run, and that different mutations produce different IDs even if text is identical. However, it does not specify HOW IDs are generated. UUID v4? Monotonic counter? Hash-based? This matters for:
- GUARD-9 (determinism): UUID v4 uses random bytes — if drawn from the seeded RNG, it's deterministic; if from `Uuid::new_v4()`, it violates GUARD-9.
- Serialization stability (GOAL-5.6): ID format affects JSON output.
- Cross-references: Other features key on `candidate_id` (GOAL-6.3 evaluation cache).

Two engineers would implement this differently (one might use UUIDs, another might use monotonic u64 counters), violating the specificity check.

**Suggested fix:** Add to GOAL-5.4:
> Candidate IDs are generated as monotonically increasing `u64` values from a per-run counter starting at 0 for the first seed candidate. This ensures uniqueness, determinism (GUARD-9), compact serialization, and efficient use as HashMap keys. Seed candidates receive IDs 0..N-1 in the order provided; subsequent candidates receive the next available ID.

---

### FINDING-2
**[Check #9] GOAL-5.2: Boundary conditions for text parameters HashMap unspecified**
GOAL-5.2 defines `HashMap<String, String>` for text parameters but does not specify:
- Can the HashMap be empty? (A candidate with zero text parameters — is this valid?)
- Can keys be empty strings?
- Can values be empty strings?
- Is there a maximum number of parameters?
- Is there a maximum size per parameter value? (GUARD-7 references ~1KB each for memory estimation, but this isn't a constraint on the candidate itself.)

An empty parameter map would be a degenerate candidate that can't be usefully mutated. This should be explicitly validated.

**Suggested fix:** Add a new sentence to GOAL-5.2:
> A candidate must have at least one text parameter; construction with an empty parameter map fails with `GEPAError::InvalidCandidate`. Keys must be non-empty strings. Values may be empty strings (representing a parameter to be filled by mutation).

---

## 🟡 Important (should fix before implementation)

### FINDING-3
**[Check #7] Missing: Error handling for seed candidate validation**
GOAL-5.1 says engine construction fails if no seed is provided, but doesn't specify validation of seed candidate *content*. What if a seed candidate:
- Has parameters with keys that don't match what the adapter expects?
- Has the wrong number of parameters?
- Has extremely large parameter values (e.g., 100MB string)?

The adapter is opaque so the engine can't fully validate content, but parameter count ≥1 (see FINDING-2) and basic size sanity checks could prevent surprising failures mid-run.

**Suggested fix:** Add to GOAL-5.1:
> Seed candidates are validated at construction: each must have at least one text parameter (GOAL-5.2 constraint), and all seed candidates must have the same set of parameter keys. Mismatched parameter keys across seeds produce `GEPAError::InvalidCandidate { message }`.

---

### FINDING-4
**[Check #5] GOAL-5.5: Lineage traversal — completeness of actor/trigger/outcome**
GOAL-5.5 says lineage is "reconstructable" but doesn't specify:
- (a) Actor/trigger: Who initiates lineage traversal? (Caller via a public method? Or only internal engine use?)
- (b) Expected behavior: Is there a public API method like `candidate.lineage() -> Vec<&Candidate>`? Or is it a state-level query `state.lineage(candidate_id) -> Vec<CandidateId>`?
- (c) Outcome: What is the return type? Full candidate objects? Just IDs? Does it include the reflections at each step?

This is a P1 so it won't block initial implementation, but the API shape should be defined before design.

**Suggested fix:** Revise GOAL-5.5:
> The engine provides `GEPAState::lineage(candidate_id) -> Result<Vec<CandidateId>, GEPAError>` which returns the ordered chain of ancestor IDs from the given candidate back to its seed (inclusive). Returns `Err(GEPAError::CandidateNotFound)` if the candidate_id is unknown. Full candidate data and reflections for each ancestor are available via `GEPAState::candidate(id)`.

---

### FINDING-5
**[Check #10] Missing: State transitions for candidate lifecycle**
Candidates have an implicit lifecycle: Created (seed or mutation) → potentially on Pareto front → potentially pruned from front. While candidates are immutable (GOAL-5.3), their *status* changes:
- Is a candidate on the current front?
- Has a candidate ever been on the front?
- Was a candidate rejected at the Accept step?

These states are not tracked on the candidate itself (which is correct given immutability), but there's no requirement in this doc or in the state doc (requirements-06) that explicitly tracks candidate status/lifecycle. GOAL-6.5 tracks aggregate stats but not per-candidate status.

**Suggested fix:** Either add a requirement here or in requirements-06-state.md:
> Per-candidate status (accepted_to_front, still_on_front, rejected_at_accept) is queryable from `GEPAState` for observability and debugging purposes.

Or explicitly mark this as out of scope for v1.

---

### FINDING-6
**[Check #18] GOAL-5.2: Data format details incomplete for "reflection that produced it"**
GOAL-5.2 metadata includes "the reflection that produced it" but doesn't specify the type. Is this:
- A `String` (raw reflection text)?
- A `Option<String>` (None for seeds, since seeds have no reflection)?
- A structured type with the reflection text plus the proposer/mutation strategy used?

This intersects with GOAL-1.5 (reflection returns "natural-language diagnosis") and GOAL-4.x (proposers), but the exact data stored on the candidate needs pinning down.

**Suggested fix:** In GOAL-5.2, change "the reflection that produced it" to:
> `reflection: Option<String>` — the natural-language reflection text that guided the mutation producing this candidate. `None` for seed candidates. The reflection text is the full output from the adapter's `reflect` method (GOAL-1.5).

---

### FINDING-7
**[Check #16] GOAL-5.6: Serialization format version/migration unspecified**
GOAL-5.6 requires JSON serialization with round-trip fidelity. But what happens when the `Candidate` struct gains a new field in a future version?
- Is there a schema version field?
- Is forward/backward compatibility required?
- Can a v2 engine load v1 candidates?

This matters because candidates are part of checkpoints (GOAL-6.1), and checkpoint compatibility is critical for long-running optimization that may span code updates.

**Suggested fix:** Add to GOAL-5.6:
> Candidate serialization includes a `schema_version: u32` field (starting at 1). Deserialization of older schema versions is supported via explicit migration. Deserialization of unknown future versions returns `Err(GEPAError::IncompatibleVersion)`.

Or explicitly declare: "Checkpoint format compatibility across code versions is out of scope for v1."

---

### FINDING-8
**[Check #8] Missing: Non-functional requirements for candidate management**
This feature doc has no non-functional requirements. While GUARD-6 and GUARD-7 cover performance/memory at the master level, candidate-specific NFRs are absent:
- **Performance**: How fast must candidate creation be? (Probably trivially fast, but should be stated or excluded.)
- **Memory**: GUARD-7 says 1000 candidates × 10 params × ~1KB < 50MB. But what about the reflection strings stored on each candidate? Long reflections could dominate memory.
- **Concurrency**: GOAL-5.3 says "safe concurrent reads" — does this imply candidates must be `Send + Sync`? This should be explicit.

**Suggested fix:** Add a note:
> `Candidate` implements `Clone`, `Send`, `Sync`, `Debug`, `Serialize`, `Deserialize`, `PartialEq`, and `Eq`. (GUARD-8 requires Debug; GUARD-9 determinism requires PartialEq for verification; Send+Sync for safe concurrent reads per GOAL-5.3.)

---

## 🟢 Minor (can fix during implementation)

### FINDING-9
**[Check #12] Terminology: "text parameters" vs "named text parameters" vs "parameter map"**
GOAL-5.2 uses both "named text parameters" and just refers to the HashMap. GOAL-5.4 says "text parameters". This is minor since the meaning is clear, but a canonical term should be chosen.

**Suggested fix:** Standardize on "text parameters" everywhere, define once in GOAL-5.2: "A candidate's **text parameters** are a `HashMap<String, String>` of named values."

---

### FINDING-10
**[Check #22] Cross-reference to GUARD-2 could be more explicit**
The cross-references section mentions GUARD-2 but doesn't tie it to a specific GOAL. GOAL-5.3 is the GOAL that implements GUARD-2's constraint. This linkage should be bidirectional.

**Suggested fix:** In cross-references, change:
> `GUARD-2 — immutability invariant`

to:
> `GUARD-2 — immutability invariant (implemented by GOAL-5.3)`

---

### FINDING-11
**[Check #21] No gaps in numbering, but GOAL numbering starts at 5.1**
GOALs are 5.1 through 5.6 — sequential, no gaps. ✅ Minor note: the prefix "5." ties to the feature number, which is good for global uniqueness.

---

## 📊 Coverage Matrix

| Category | Covered | Missing |
|---|---|---|
| Happy path (create seed → mutate → lineage) | GOAL-5.1, 5.2, 5.3, 5.5 | ✅ Covered |
| Error handling | GOAL-5.1 (no seed → error) | Empty parameter map, invalid seed content, candidate not found during lineage traversal (FINDING-2, FINDING-3, FINDING-4) |
| Immutability invariant | GOAL-5.3, GUARD-2 | ✅ Covered |
| Uniqueness | GOAL-5.4 | ID generation mechanism unspecified (FINDING-1) |
| Serialization | GOAL-5.6 | Schema versioning (FINDING-7) |
| Performance | GUARD-6, GUARD-7 (master) | No candidate-specific perf requirements (FINDING-8, acceptable if deferred to master) |
| Security | N/A (library crate, no auth/network) | ✅ Not applicable per GUARD-5 |
| Concurrency | GOAL-5.3 mentions "safe concurrent reads" | Send+Sync trait bounds not explicit (FINDING-8) |
| Observability | GOAL-5.5 (lineage) | Per-candidate status/lifecycle (FINDING-5) |
| Boundary conditions | — | Parameter map empty/min/max (FINDING-2) |
| State transitions | — | Candidate lifecycle status (FINDING-5) |
| Data format | GOAL-5.2 | Reflection type unclear (FINDING-6) |

---

## ✅ Passed Checks

- **Check #0: Document size** ✅ — 6 GOALs, well within ≤15 limit.
- **Check #1: Specificity** — 4/6 GOALs are fully specific (GOAL-5.1, 5.2, 5.3, 5.6). GOAL-5.4 lacks ID generation mechanism (FINDING-1). GOAL-5.5 lacks API shape (FINDING-4). Partial pass.
- **Check #2: Testability** ✅ — 6/6 GOALs have testable conditions:
  - 5.1: Provide no seeds → error. Provide seeds → they appear on initial front.
  - 5.2: Create candidate → check all fields present in struct.
  - 5.3: Create candidate → attempt mutation → verify original unchanged + new candidate exists.
  - 5.4: Create two candidates → IDs differ. Create two with identical text → IDs still differ.
  - 5.5: Create chain of mutations → traverse lineage → verify all ancestors found.
  - 5.6: Serialize → deserialize → assert equality.
- **Check #3: Measurability** ✅ — No quantitative requirements in this doc (all qualitative/behavioral). Master doc GUARD-7 provides the quantitative memory constraint. No unmeasured quantitative claims.
- **Check #4: Atomicity** ✅ — 6/6 GOALs describe one thing each. GOAL-5.2 is the most complex (struct definition + separation of concerns note), but the "separation" note is explanatory, not a separate requirement.
- **Check #6: Happy path** ✅ — User provides seeds (5.1) → engine creates candidates via mutation (5.3) → each has unique ID (5.4) → metadata tracks lineage (5.2, 5.5) → candidates are serializable (5.6).
- **Check #11: Internal consistency** ✅ — Verified all 15 pairs of GOALs (6 choose 2). No contradictions found. GOAL-5.2 (scores NOT on candidate) is consistent with GOAL-5.3 (immutability) — scores accumulate in cache, not on candidate.
- **Check #12: Terminology** — Minor inconsistency noted (FINDING-9), but no confusion risk.
- **Check #13: Priority consistency** ✅ — P1 GOALs (5.5 lineage, 5.6 serialization) do not depend on other P1 items. P0 GOALs are independent of P1. No priority inversions.
- **Check #14: Numbering/referencing** ✅ — Cross-references verified:
  - GOAL-6.3 exists in requirements-06-state.md ✅ (evaluation cache)
  - GOAL-8.5 exists in requirements-08-data-loading.md ✅ (re-evaluation backfill)
  - GUARD-2 exists in requirements-master.md ✅ (immutability)
- **Check #15: GUARDs vs GOALs alignment** ✅ — Verified against all 9 GUARDs:
  - GUARD-2 (immutability) ↔ GOAL-5.3: directly implements it. ✅
  - GUARD-7 (memory linear) ↔ GOAL-5.2: lightweight candidates (no scores stored). ✅
  - GUARD-8 (Debug, Error) ↔ not explicit on Candidate but not contradicted. (FINDING-8 suggests making explicit.)
  - GUARD-9 (determinism) ↔ GOAL-5.4: ID generation must be deterministic — not contradicted but not explicitly addressed (FINDING-1).
  - Remaining GUARDs (1, 3, 4, 5, 6) don't directly apply to candidate management. ✅
- **Check #17: External dependencies** ✅ — Candidates have no external dependencies. All data is internal to the crate.
- **Check #19: Migration/compatibility** — Not replacing existing functionality. New crate. ✅ (but schema versioning for future-proofing is FINDING-7)
- **Check #20: Scope boundaries** — Partially covered: GOAL-5.2 explicitly states scores are NOT on candidates. No broader "non-goals" section for candidate management specifically, but the master doc's out-of-scope section covers the crate-level boundaries. ✅
- **Check #22: Grouping** ✅ — All 6 GOALs are logically ordered: creation (5.1) → structure (5.2) → immutability (5.3) → identity (5.4) → lineage (5.5) → serialization (5.6).
- **Check #23: Dependency graph** — Implicit but clear: GOAL-5.2 (structure) is foundational; 5.1, 5.3, 5.4, 5.5, 5.6 all depend on it. GOAL-5.5 (lineage) depends on 5.2 (parent_id in metadata). No circular dependencies. ✅
- **Check #24: Acceptance criteria** — Each GOAL serves as its own acceptance criterion (all are specific enough to be pass/fail). No separate AC needed for a doc this small. ✅
- **Check #25: User perspective** ✅ — GOAL-5.1 is written from user perspective ("The user provides seed candidates"). Others are system-internal, which is appropriate for a library crate's internal data structures.
- **Check #26: Success metrics** — For a library crate, "tests pass" IS the success metric. Production observability is covered by GOAL-6.5 (run statistics) and GOAL-9.x (events). ✅
- **Check #27: Risk identification** — No high-risk GOALs in this feature. Candidate management is straightforward data modeling. The master doc's risk section doesn't list any candidate-related risks. ✅ Appropriate.

---

## Summary

- **Total requirements:** 6 GOALs (4 P0, 2 P1), 0 GUARDs (GUARDs in master)
- **Critical:** 2 (FINDING-1, FINDING-2)
- **Important:** 6 (FINDING-3 through FINDING-8)
- **Minor:** 3 (FINDING-9 through FINDING-11)
- **Total findings:** 11
- **Coverage gaps:** Boundary conditions for parameters, ID generation mechanism, candidate lifecycle status, reflection data type, schema versioning
- **Recommendation:** **Needs fixes first** — FINDING-1 (ID generation) and FINDING-2 (parameter validation) must be resolved before design, as they affect API shape and GUARD-9 compliance. The 6 important findings should be addressed before implementation but won't block design.
- **Estimated implementation clarity:** **Medium** — The core concept (immutable text-parameter bags with lineage) is clear, but an implementer would need to ask questions about ID generation, parameter validation, reflection storage type, and trait bounds before writing production code.
