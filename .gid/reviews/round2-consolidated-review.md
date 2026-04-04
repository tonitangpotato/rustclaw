# Round 2 Design Review: gepa-core (All 10 Documents)

**Reviewer:** RustClaw (manual, post sub-agent timeout)
**Date:** 2026-04-04
**Scope:** design.md (master) + design-01 through design-09
**Context:** R1 found 54 findings (4 Critical, 20 Important, 30 Minor), all applied.

---

## Previous Critical Fixes Verification

1. ✅ **design-01: per-iteration front backfill** — Step 8 now explicitly describes budget-capped backfill of front members on current minibatch before accept/reject. Verified in §2.2 step 8.
2. ✅ **design-04: merge iteration execute step** — §2.3 MergeProposer now documents full sequence: `select_complementary_pair → execute(parent_a) → execute(parent_b) → merge(parent_a, parent_b, scores_a, scores_b) → return ProposerOutput`. Verified.
3. ✅ **design-05: merge_parent_id field** — `Candidate` struct in §2.1 now includes `pub merge_parent_id: Option<u64>`. Constructor `new()` accepts it. Verified.
4. ✅ **design-09: CandidateAccepted carries full Candidate** — `CandidateAccepted { candidate: Candidate, scores, front_size }` confirmed. The `emit_event!` macro note about clone-on-demand is present. Verified.

**All 4 R1 critical fixes are intact.** ✅

---

## Cross-Document Consistency Findings

### FINDING-1 ✅🔴 Critical — CandidateId type inconsistency across documents

**design-02** (Pareto Front §2.1): `pub type CandidateId = u64;`
**design-05** (Candidates §2.2): ID generator produces `u64`, struct field `pub id: u64`
**design-09** (Events §2.1 Key Details): States `CandidateId(String)` — "CandidateId, ExampleId, SelectionMethod, TerminationReason are types defined in features 01/02/06. Summarized here: `CandidateId(String)`, `ExampleId(String)`"

This is a direct contradiction. design-02 and design-05 define `CandidateId = u64`, but design-09 says it's `CandidateId(String)`. The event payloads use `CandidateId` throughout — if the type is wrong, every event variant's payload is wrong.

**Suggested fix:** Update design-09 §2.1 Key Details to say `CandidateId = u64` (matching design-02/05). Remove the "(String)" claim. Also update `ExampleId` description to match design-08's `ExampleId(pub String)` newtype wrapper.

### FINDING-2 ✅🔴 Critical — ExampleId type inconsistency: String newtype vs u64 in EvalCache

**design-08** (Data Loading §2.2): `pub struct ExampleId(pub String);` — newtype wrapper around String
**design-06** (State §2.2): `entries: HashMap<(u64, u64), CacheEntry>` — cache keyed by `(u64, u64)`

If `ExampleId` is a `String` newtype, the cache key should be `(u64, ExampleId)` or `(CandidateId, ExampleId)`, not `(u64, u64)`. This would mean the cache API is wrong, or `ExampleId` is silently converted to a `u64` somewhere (which is never documented).

Additionally, design-06 §2.3 shows serialization as `"candidate_id:example_id"` with "Tuple keys (u64, u64)" — confirming the cache uses u64 for example IDs.

**Suggested fix:** Either:
- (A) Change `ExampleId` to `ExampleId(pub u64)` across design-08 and all consumers (simpler, matches cache), OR
- (B) Change EvalCache keys to `(CandidateId, ExampleId)` i.e. `(u64, String)` — but this has performance implications for hashing.

Recommend option (A) since the cache is a hot path and u64 keys are much faster. The DataLoader assigns numeric IDs; string IDs can be kept in a separate lookup table.

### FINDING-3 ✅🟡 Important — RNG type inconsistency: StdRng vs ChaCha8Rng

**design.md** (master §3.3): "A single `ChaCha8Rng` seeded at engine construction" — explicit choice for cross-version determinism.
**design-01** (Core Engine §2.1, §2.2): `GEPAEngine` struct field `rng: StdRng`. MinibatchSampler uses `StdRng`.
**design-04** (Proposers §2.1): `rng: &mut StdRng` parameter.
**design-08** (Data Loading §2.3): `MinibatchIterator` has field `rng: ChaCha8Rng`.

The master design explicitly says ChaCha8Rng for determinism (GUARD-9), and design-08 follows this. But design-01 and design-04 use `StdRng` which may change across Rust versions (defeating GUARD-9). 

**Suggested fix:** Replace all `StdRng` references with `ChaCha8Rng` in design-01 (engine struct, MinibatchSampler) and design-04 (proposer trait parameter, MutationProposer methods). The master design's rationale is correct — `StdRng` is explicitly documented in rand as potentially changing algorithm across versions.

### FINDING-4 ✅🟡 Important — SelectionMethod enum referenced but never defined

**design-09** (Events §2.1): `CandidateSelected { selection_method: SelectionMethod }` — uses `SelectionMethod` type.
**design-09** (Events §2.1 Key Details): Lists variants `Tournament, LeastCrowded, Random, MergeComplement`.
**design-02** (Pareto Front): Selection is round-robin with overfitting-delta reordering. No `SelectionMethod` enum defined.
**design-04** (Proposers): No `SelectionMethod` enum defined.

The enum is used in events but never formally defined in any feature design. Also, the listed variants don't match the actual selection algorithms: there's no "Tournament" or "LeastCrowded" — design-02 uses round-robin with overfitting-delta ordering, and design-04's merge uses complementarity-based selection.

**Suggested fix:** Define `SelectionMethod` in design-02 or design-04 with variants matching actual algorithms: `RoundRobinOverfitting`, `ComplementaryPair`, `Random` (if applicable). Remove `Tournament` and `LeastCrowded` which don't exist in the design.

### FINDING-5 ✅🟡 Important — TerminationReason variants inconsistent

**design-01** (Core Engine §2.3): `MaxIterations, TimeBudget, Stagnation, TooManySkips, Cancelled`
**design-09** (Events §2.1 Key Details): Claims `MaxIterations, Stagnation, TimeBudget, ConvergedFront, TooManySkips`
**design.md** (master §6): `MaxIterations, TimeBudget, Stagnation, TooManySkips, Cancelled`

design-09 has `ConvergedFront` (not in design-01 or master) and is missing `Cancelled`. 

**Suggested fix:** Align design-09's claimed variants with design-01/master: `MaxIterations, TimeBudget, Stagnation, TooManySkips, Cancelled`. Remove `ConvergedFront`.

### FINDING-6 ✅🟡 Important — MinibatchIterator state not captured in EngineState checkpoint

**design-06** (State §2.1): `EngineState` contains: pareto_front, candidates, eval_cache, next_candidate_id, rng_state, proposer_state, statistics, config_snapshot, stagnation_counter, consecutive_skips.

**Missing from EngineState:**
- `MinibatchIterator` state (cursor, epoch, shuffled_order) — design-08 §2.3 says these are "serializable for checkpoint/resume" but they're not in `EngineState`.
- `overfitting_deltas: HashMap<CandidateId, f64>` — computed by backfill (design-08 §2.5), used by selection (design-02 §2.3). Not in EngineState.
- `merge_disabled: bool` — design-03 §2.1 says merge auto-disables on first failure. This flag needs to survive checkpoint/resume.
- `start_time: Option<Instant>` — not serializable (`Instant`), but time budget tracking needs some form of elapsed time tracking on resume. Currently undefined behavior for time_budget across checkpoint/resume.

**Suggested fix:** Add to `EngineState`:
```rust
pub minibatch_state: MinibatchState, // cursor, epoch, shuffled_order
pub overfitting_deltas: HashMap<u64, f64>,
pub merge_disabled: bool,
pub elapsed_before_resume: Duration, // accumulated time from previous segments
```

### FINDING-7 ✅🟡 Important — EvalCache API signatures inconsistent between definition and callers

**design-06** (State §2.2 — definition):
- `get(&mut self, candidate_id: u64, example_id: u64) -> Option<f64>`
- `scores_for_candidate(&self, candidate_id: u64) -> Vec<(u64, f64)>`

**design-02** (Pareto Front §2.2 — caller):
- References `cache.examples_for(a)` — this method is not in design-06's interface
- References `cache.scores_for(candidate_id)` — returns `&[(ExampleId, f64)]` (borrowed slice), but design-06 returns `Vec<(u64, f64)>` (owned Vec)

**design-01** (Core Engine §4 Integration Points):
- References `EvalCache::scores_for(candidate_id) -> &[(ExampleId, f64)]` — borrowed, with `ExampleId` not `u64`
- References `EvalCache::store(candidate_id, example_id, score)` — but design-06 has `insert()` not `store()`

**Suggested fix:** 
1. Add `examples_for(candidate_id: u64) -> Vec<u64>` to design-06's EvalCache interface
2. Standardize method name: `insert` (design-06 definition) everywhere, not `store`
3. Standardize return type: owned `Vec` (not borrowed slice — HashMap can't return contiguous slices)
4. Use `u64` consistently for example IDs in cache API (or `ExampleId` consistently — pick one)

### FINDING-8 ✅🟡 Important — adapter.evaluate return type inconsistency

**design-03** (Adapter §2.2): `evaluate` returns `Vec<f64>` — one score per example, same order
**design-08** (Data Loading §2.6, Integration Points): States adapter's evaluate returns `Vec<(ExampleId, f64)>`

These are different types. `Vec<f64>` relies on positional correspondence with the input examples. `Vec<(ExampleId, f64)>` is keyed. The engine would need different handling for each.

**Suggested fix:** Use `Vec<f64>` (matching design-03 §2.2 which is the authoritative trait definition). The engine correlates scores to examples by position. Update design-08's integration point reference.

### FINDING-9 ✅🟡 Important — GEPAResult struct defined inconsistently

**design.md** (master §6):
```rust
pub struct GEPAResult {
    pub pareto_front: Vec<Candidate>,
    pub validation_scores: HashMap<CandidateId, Vec<(ExampleId, f64)>>,
    pub validation_skipped: bool,
    pub best_candidate: Candidate,
    pub termination_reason: TerminationReason,
    pub statistics: RunStatistics,
    pub state: GEPAState,
}
```

**design-01** (Core Engine §2.2):
```rust
pub struct GEPAResult {
    pub best_candidate: Candidate,
    pub pareto_front: Vec<Candidate>,
    pub termination_reason: TerminationReason,
    pub stats: RunStatistics,
    pub total_iterations: u64,
    pub elapsed_time: Duration,
}
```

Fields differ: master has `validation_scores`, `validation_skipped`, `state`; design-01 has `total_iterations`, `elapsed_time` and uses `stats` not `statistics`. Neither is a superset of the other.

**Suggested fix:** Reconcile into one canonical definition (preferably in design-01 since it owns `run()`). Include all fields from both. Use consistent naming (`statistics` or `stats`).

### FINDING-10 ✅🟡 Important — ParetoFront::select signature inconsistency

**design-02** (Pareto Front §2.3): `select(&mut self, cache: &EvalCache, overfitting_deltas: &HashMap<CandidateId, f64>, rng: &mut StdRng) -> Result<CandidateId, GEPAError>`

**design-01** (Core Engine §2.2 step 3): `ParetoFront::select(&mut self, &cache, &overfitting_deltas, &mut rng)` — matches

**design-01** (Integration Points table): `ParetoFront::select(&mut self, &cache, &overfitting_deltas, &mut rng) -> Result<CandidateId, GEPAError>` — matches

**design-04** (Proposers §2.2): `MutationProposer::select_parent` takes `(front: &ParetoFront, rng: &mut StdRng)` — no cache, no deltas

So the proposer's `select_parent` is different from `ParetoFront::select`. design-04 §2.2 describes its own selection algorithm (lowest selection count, RNG tie-break) while design-02 describes a different one (round-robin with overfitting-delta reordering). Both claim to implement GOAL-2.3/4.3.

**Suggested fix:** Clarify the boundary: Either (A) proposers call `ParetoFront::select()` directly (removing `MutationProposer::select_parent`), or (B) document that `MutationProposer` wraps `ParetoFront::select()` with additional tracking. Currently they appear to be two competing implementations of the same requirement.

### FINDING-11 ✅🟢 Minor — Reflection struct field name inconsistency

**design-03** (Adapter §2.2): `Reflection { diagnosis: String, directions: Vec<String> }`
**design.md** (master §6): `Reflection { diagnosis: String, improvement_directions: Vec<String> }`

Field name: `directions` vs `improvement_directions`.

**Suggested fix:** Pick one. `improvement_directions` is more descriptive.

### FINDING-12 ✅🟢 Minor — Expected output type inconsistency in Example struct

**design.md** (master §6): `expected_output: Option<String>`
**design-08** (Data Loading §2.2): `expected_output: Option<serde_json::Value>`

`String` vs `serde_json::Value` — different types.

**Suggested fix:** Use `Option<serde_json::Value>` (design-08) since it's more flexible and aligns with `input: serde_json::Value`.

### FINDING-13 ✅🟢 Minor — GEPAError missing variant: AllSeedsFailedError

**design.md** (master §3.1): `GEPAError` enum lists variants. Does not include `AllSeedsFailed`.
**design-04** (Proposers §2.4): References `GEPAError::AllSeedsFailedError`
**design-01** (Core Engine §2.2): References "If all seeds fail, return `Err(GEPAError::AllSeedsFailed)`"
**design-02** (Pareto Front §2.3): References `GEPAError::EmptyFrontError`

Neither `AllSeedsFailed`/`AllSeedsFailedError` nor `EmptyFrontError` nor `InsufficientFrontSize` (design-04 §2.3) nor `Internal` (design-06 §2.1) nor `ValidationError` (design-08 §2.6) are listed in the master GEPAError enum.

**Suggested fix:** Update master design §3.1 GEPAError to include all variants referenced across feature docs: `AllSeedsFailed`, `EmptyFrontError`, `InsufficientFrontSize`, `Internal { message: String }`, `ValidationError(String)`.

### FINDING-14 ✅🟢 Minor — Statistics type name inconsistency

**design.md** (master §6): `RunStatistics` in GEPAResult
**design-06** (State §2.5): Defines `GEPAStatistics`

`RunStatistics` vs `GEPAStatistics` — same concept, different names.

**Suggested fix:** Standardize on one name. `GEPAStatistics` (design-06) since it's fully defined there.

### FINDING-15 ✅🟢 Minor — design-06 EngineState vs GEPAState naming

**design.md** (master): References `GEPAState` (§2, §5 step 11)
**design-01**: Uses both `GEPAState` and `EngineState` (§2.2 field `state: GEPAState`)
**design-06**: Defines `EngineState` as the struct name

`GEPAState` vs `EngineState` — same struct, different names across documents.

**Suggested fix:** Pick one. `GEPAState` is used more frequently. Update design-06 to use `GEPAState`.

### FINDING-16 ✅🟢 Minor — CheckpointSaved event payload inconsistency

**design-09** (Events §2.1): `CheckpointSaved { path: PathBuf, iteration: u64 }`
**design-06** (State §2.4): References emitting `CheckpointSaved { success: bool, path: PathBuf, error: Option<String> }`

design-06 includes `success` and `error` fields that design-09 doesn't have.

**Suggested fix:** Align. Add `success: bool` and `error: Option<String>` to the event variant in design-09 (failed checkpoints should be observable).

### FINDING-17 ✅🟢 Minor — EvalCache peek() vs get() semantics unclear for dominance

**design-06** (State §2.2): `peek()` does NOT update LRU counter. `get()` does. States peek is "used for dominance comparisons."

But design-02 (Pareto Front §2.2) references `cache.scores_for(a)` / `cache.examples_for(a)` for dominance — neither is `peek()`. It's unclear whether dominance checks should update LRU timestamps (potentially keeping stale entries alive) or not.

**Suggested fix:** Document explicitly: dominance checks should use `peek()` semantics (no LRU update) to avoid keeping rejected candidates' scores alive. Add a `peek_scores_for(candidate_id)` method to the EvalCache interface.

### FINDING-18 ✅🟢 Minor — design-03 adapter example uses `expected` field not in Example struct

**design-03** (Adapter §3, example implementation): `ex.expected.as_deref()` — references `expected` field.
**design-08** (Data Loading §2.2): Field is `expected_output`, not `expected`.

**Suggested fix:** Update design-03 example to use `ex.expected_output`.

---

## Per-Document Summary

### design.md (master)
- Generally consistent. Type definitions in §6 need alignment with feature docs (FINDING-9, 11, 12, 13, 14, 15).

### design-01-core-engine.md
- R1 critical fix (backfill step 8) verified ✅
- RNG type wrong (FINDING-3)
- GEPAResult inconsistent (FINDING-9)
- EvalCache method names wrong (FINDING-7)

### design-02-pareto-front.md  
- Solid. Missing `examples_for()` reference (FINDING-7). SelectionMethod undefined (FINDING-4).

### design-03-adapter.md
- evaluate return type inconsistency (FINDING-8). Example code bug (FINDING-18).

### design-04-proposers.md
- R1 critical fix (merge execute) verified ✅
- Dual selection logic issue (FINDING-10). RNG type wrong (FINDING-3).

### design-05-candidates.md
- R1 critical fix (merge_parent_id) verified ✅
- Clean. No new findings.

### design-06-state.md
- Missing checkpoint fields (FINDING-6). Cache API gaps (FINDING-7). Name inconsistency (FINDING-15).

### design-07-config.md
- Clean. Well-structured. No new findings.

### design-08-data-loading.md
- ExampleId type vs cache key mismatch (FINDING-2). evaluate return type inconsistency (FINDING-8).

### design-09-events.md
- CandidateId type wrong (FINDING-1). TerminationReason wrong (FINDING-5). SelectionMethod undefined (FINDING-4). CheckpointSaved payload mismatch (FINDING-16).

---

## Summary

| Severity | Count | Finding IDs |
|----------|-------|-------------|
| 🔴 Critical | 2 | FINDING-1 (CandidateId type), FINDING-2 (ExampleId/cache key type) |
| 🟡 Important | 8 | FINDING-3 (RNG type), FINDING-4 (SelectionMethod), FINDING-5 (TerminationReason), FINDING-6 (checkpoint completeness), FINDING-7 (EvalCache API), FINDING-8 (evaluate return), FINDING-9 (GEPAResult), FINDING-10 (dual selection) |
| 🟢 Minor | 8 | FINDING-11 through FINDING-18 |
| **Total** | **18** | |

**Recommendation:** Needs fixes before implementation. The 2 Critical findings (type mismatches across documents) would cause compile errors. The 8 Important findings are API inconsistencies that would cause confusion during implementation. Fix Criticals and Importants first, Minors can be addressed during implementation.

**Estimated implementation confidence:** Medium — the core algorithms are well-designed, but the cross-document type/API inconsistencies indicate the documents evolved somewhat independently and need a reconciliation pass.
