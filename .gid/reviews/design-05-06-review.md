# Design Review: 05-Candidates & 06-State

**Reviewer:** Subagent  
**Date:** 2026-04-04  
**Scope:** design-05-candidates.md, design-06-state.md vs their requirements

---

## Design 05 — Candidates

### FINDING-1 [🔴] (design-05)
**GOAL:** GOAL-5.2 / §2.3
**Issue:** `merge_parent_id: Option<u64>` is mentioned in §2.3 (lineage) as "A separate `merge_parent_id: Option<u64>` field on `Candidate` records the secondary parent," but this field is **absent from the `Candidate` struct definition** in §2.1. The struct only shows `parent_id: Option<u64>`. The `Candidate::new()` constructor signature also has no parameter for `merge_parent_id`.
**Fix:** Add `pub merge_parent_id: Option<u64>` to the `Candidate` struct in §2.1 and add the corresponding parameter to `Candidate::new()`.

### FINDING-2 [🟡] (design-05)
**GOAL:** GOAL-5.5
**Issue:** Requirements say the lineage API is `GEPAState::lineage(candidate_id)` — i.e., a method on the state struct. Design-05 §2.3 defines it as a freestanding function `pub fn lineage(candidate_id: u64, store: &CandidateStore)`. This is fine architecturally (the state method can delegate), but design-06 does not show this delegation. The integration between the two designs is implicit.
**Fix:** Either (a) add a `lineage()` method on `EngineState` in design-06 that delegates to the freestanding function, or (b) note in design-05 §2.3 that this is the internal function and the public API on `GEPAState` wraps it.

### FINDING-3 [🟡] (design-05)
**GOAL:** GOAL-5.5
**Issue:** `lineage()` only follows `parent_id`, not `merge_parent_id`. For merge candidates, the lineage to the secondary parent is unreachable. The requirements say "ordered chain of ancestor IDs from the given candidate back to its seed," which is ambiguous for merges. This should be explicitly documented as a design decision.
**Fix:** Add a note in §2.3 clarifying that `lineage()` returns the primary-parent chain only, and optionally expose a `merge_parent_id` traversal helper for callers that need full provenance.

### FINDING-4 [🟢] (design-05)
**GOAL:** GOAL-5.1
**Issue:** `validate_seeds()` in §2.5 checks that all seeds share the same parameter keys. Correctly uses `BTreeSet` for deterministic error messages. The error type `GEPAError::InvalidCandidate` is referenced.
**Fix:** None — this is well-specified.

### FINDING-5 [🟢] (design-05)
**GOAL:** GOAL-5.3
**Issue:** Immutability is enforced by having no `&mut self` methods on `Candidate` post-construction. All fields are `pub`, which technically allows mutation by external code, but the design notes construction-time validation via `new()` and the conceptual contract is clear.
**Fix:** None, but consider making fields private with accessor methods in implementation if GUARD-2 strictness is desired.

### FINDING-6 [🟢] (design-05)
**GOAL:** GOAL-5.4
**Issue:** `CandidateIdGenerator` is simple, correct, and deterministic. `resume()` from checkpoint is supported. Seeds get IDs 0..N-1.
**Fix:** None.

### FINDING-7 [🟢] (design-05)
**GOAL:** GOAL-5.6, GOAL-5.7
**Issue:** Serde derives, `PartialEq`/`Eq`, `Clone`, `Debug`, `Send`+`Sync` all covered. Round-trip guarantee correctly noted that `HashMap` equality is content-based.
**Fix:** None.

### FINDING-8 [🟡] (design-05)
**GOAL:** GOAL-5.2
**Issue:** The `Candidate::new()` constructor signature has `reflection: Option<String>` as a parameter, but there's no validation that seed candidates (parent_id = None) have `reflection = None`, or that non-seed candidates have `reflection = Some(...)`. The requirements say "None for seed candidates; the full output from the adapter's reflect method." This is a soft constraint — should the constructor enforce it?
**Fix:** Either (a) validate reflection consistency in `new()` (seeds must have `None`, non-seeds should have `Some`), or (b) document that this is the caller's responsibility. Option (b) is probably fine since `Candidate::seed()` already sets it to `None` implicitly (though the `seed()` method signature isn't shown with reflection param).

---

## Design 06 — State

### FINDING-9 [🟡] (design-06)
**GOAL:** GOAL-6.1
**Issue:** Deterministic JSON serialization requires HashMap keys to be sorted. The design mentions "via `BTreeMap` conversion or `serde` key ordering" but does not specify the implementation mechanism. Standard `serde_json` with `HashMap` does NOT sort keys. This needs a concrete approach: either use `BTreeMap` throughout, use `serde_json::to_string` with a custom serializer, or convert to `BTreeMap` before serialization.
**Fix:** Specify the exact mechanism. Recommended: use `#[serde(serialize_with = "ordered_map")]` attribute on HashMap fields, or convert `EngineState` to an intermediate `BTreeMap`-based representation before serializing. The `EvaluationCache` entries HashMap and `CandidateStore` HashMap both need this treatment.

### FINDING-10 [🟡] (design-06)
**GOAL:** GOAL-6.1
**Issue:** `EvaluationCache` uses tuple keys `(u64, u64)` in a `HashMap`. JSON doesn't support non-string keys. §2.3 notes "Tuple keys `(u64, u64)` serialized as string keys" but doesn't show how. `serde_json` will fail to serialize `HashMap<(u64, u64), CacheEntry>` by default — it requires string keys. This needs a custom `Serialize`/`Deserialize` impl or a different storage format (e.g., `HashMap<String, CacheEntry>` with keys like `"0_1"`).
**Fix:** Either (a) use `#[serde(serialize_with = ..., deserialize_with = ...)]` for the entries field, (b) store as `Vec<((u64, u64), CacheEntry)>` for serialization, or (c) use a string-keyed map. Specify the approach explicitly.

### FINDING-11 [🟢] (design-06)
**GOAL:** GOAL-6.2
**Issue:** Atomic write via temp file + rename is well-specified. Same-directory temp file ensures same-filesystem rename. Failed save continues loop with event. Checkpoint interval is configurable.
**Fix:** None.

### FINDING-12 [🟢] (design-06)
**GOAL:** GOAL-6.3
**Issue:** `EvaluationCache` correctly maps `(candidate_id, example_id) → f64`. `get()` returns cached scores. `peek()` for reads without LRU update is a nice touch for dominance comparisons.
**Fix:** None.

### FINDING-13 [🟢] (design-06)
**GOAL:** GOAL-6.4
**Issue:** LRU eviction with front-member pinning is well-designed. Soft limit when all entries pinned. Monotonic `access_counter` instead of wall-clock is correct for determinism. `pinned_ids: &HashSet<u64>` parameter to `evict_lru()` allows the engine to pass current front members.
**Fix:** None.

### FINDING-14 [🟢] (design-06)
**GOAL:** GOAL-6.5
**Issue:** `GEPAStatistics` covers all required metrics: total iterations, skipped iterations, adapter calls by type, candidates generated/accepted, acceptance rate, best score history, front size history, cache hit rate. All counters are `u64`, rates are `f64`.
**Fix:** None.

### FINDING-15 [🟡] (design-06)
**GOAL:** GOAL-6.6
**Issue:** Incremental checkpoints (P2) are acknowledged but the design explicitly defers them: "a P2 extension layered on top of the base mechanism." §2.3 mentions "GOAL-6.6 (format foundation)" but there is no actual delta format design, no delta writer/reader, no compaction algorithm, and no sequential numbering scheme. This is acceptable for P2, but the "Satisfies: GOAL-6.6" tag on §2.3 overstates coverage — the format foundation alone doesn't satisfy the goal.
**Fix:** Change §2.3's "Satisfies" line to "Partial: GOAL-6.6 (format foundation only; full delta mechanism deferred to P2 implementation)."

### FINDING-16 [🟡] (design-06)
**GOAL:** GOAL-6.1 / GUARD-9
**Issue:** `RngState::capture()` uses `.expect("StdRng serialization is infallible")` which panics on failure. This violates GUARD-4 (no panics). While `bincode::serialize` on `StdRng` is practically infallible, the design should return a `Result` for consistency.
**Fix:** Change `capture()` to return `Result<Self, GEPAError>` and map the error to `GEPAError::Internal` or similar. Alternatively, add a comment justifying the panic as truly unreachable.

### FINDING-17 [🟢] (design-06)
**GOAL:** GOAL-6.2
**Issue:** Resume flow in §2.6 is thorough: ID generator, RNG state, front, candidates, cache with LRU timestamps, proposer state, stats, stagnation counters. Resumes at `iteration + 1`. Config comparison is warning-only.
**Fix:** None.

### FINDING-18 [🟡] (design-06)
**GOAL:** GOAL-6.1
**Issue:** The `EngineState` struct derives `Serialize, Deserialize` but contains `GEPAConfig` (config_snapshot) and `ParetoFront` — types defined in other designs. If those types don't also derive `Serialize, Deserialize`, the derive will fail. This is an implicit cross-design constraint that should be explicitly stated.
**Fix:** Add a note in §2.1 that `ParetoFront`, `GEPAConfig`, and all other embedded types MUST derive `Serialize, Deserialize`. Alternatively, add this as a constraint in the integration section.

---

## Summary

### Design 05 — Candidates
| Severity | Count |
|----------|-------|
| 🔴 Blocker | 1 |
| 🟡 Warning | 3 |
| 🟢 Good | 4 |

**Overall:** Solid design. One blocker: `merge_parent_id` is referenced but not in the struct definition — this is a straightforward fix. The immutability model, ID generation, serialization, and seed validation are all well-specified. Minor issues around lineage API location and reflection validation.

**All GOALs covered:** GOAL-5.1 ✅, GOAL-5.2 ✅ (with FINDING-1 fix), GOAL-5.3 ✅, GOAL-5.4 ✅, GOAL-5.5 ✅ (with FINDING-2 clarification), GOAL-5.6 ✅, GOAL-5.7 ✅

### Design 06 — State
| Severity | Count |
|----------|-------|
| 🔴 Blocker | 0 |
| 🟡 Warning | 5 |
| 🟢 Good | 4 |

**Overall:** Comprehensive design covering checkpoint, cache, stats, and resume. No blockers, but deterministic serialization (FINDING-9, FINDING-10) needs concrete implementation details — the current "BTreeMap conversion" hand-wave won't produce working code without explicit serde attributes or conversion logic. The RNG capture panic (FINDING-16) is a minor GUARD-4 violation. GOAL-6.6 incremental checkpoints are acknowledged as P2 but the "Satisfies" tag is premature.

**All GOALs covered:** GOAL-6.1 ✅ (with FINDING-9/10 fixes), GOAL-6.2 ✅, GOAL-6.3 ✅, GOAL-6.4 ✅, GOAL-6.5 ✅, GOAL-6.6 ⚠️ (P2 deferred, foundation only)
