# Review: GEPA Design 05 (Candidates) + 06 (State/Checkpointing)

**Reviewed:** `design-05-candidates.md`, `design-06-state.md`  
**Date:** 2026-04-06  
**Reviewer:** Automated design review (28-check systematic analysis)

---

## 🔴 Critical (blocks implementation)

### FINDING-5-1: **[Check #1] Incomplete type definition for CandidateStore**
The `CandidateStore` type is referenced extensively in design-06 (§2.1, §2.4) and in design-05's serialization discussion, but **design-05 never defines it**. Design-05 §2.2 says "CandidateStore is a wrapper around HashMap<u64, Candidate> with helpers for lineage traversal" but provides no struct definition, field list, or method signatures beyond `lineage()`.

**Impact:** Two engineers would implement different APIs. One might add caching for lineage queries, another might add indexing by generation, etc.

**Suggested fix:** Add to design-05 §2.2:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateStore {
    candidates: HashMap<u64, Candidate>,
}

impl CandidateStore {
    pub fn new() -> Self;
    pub fn insert(&mut self, candidate: Candidate) -> Result<(), GEPAError>;
    pub fn get(&self, id: u64) -> Option<&Candidate>;
    pub fn lineage(&self, id: u64) -> Result<Vec<u64>, GEPAError>;
    pub fn contains(&self, id: u64) -> bool;
    pub fn len(&self) -> usize;
    pub fn iter(&self) -> impl Iterator<Item = (&u64, &Candidate)>;
}
```

---

### FINDING-5-2: **[Check #6] ID generation data flow broken on checkpoint resume**
Design-05 §2.2 describes ID generation as "monotonically increasing from a per-run counter starting at 0". Design-06 §2.1 includes `next_candidate_id: u64` in `GEPAState`.

**Problem:** The data flow for writing/reading this counter is incomplete:
- Design-05 never specifies WHERE the counter lives (implied to be in GEPAState, but not stated)
- Design-05 §2.2 `generate_id()` signature shows `&self` (immutable), but incrementing a counter requires mutation
- Design-06 §2.4 checkpoint restore pseudocode says `state.next_candidate_id` is restored but design-05 never specifies that `Candidate::new()` consumes this field

**Trace the data flow:**
- **Write path:** `Candidate::new(id, ...)` receives an already-generated ID → who incremented `next_candidate_id`? Not specified.
- **Read path (checkpoint resume):** `state.next_candidate_id = 42` → next call to... what function? Design-05 has no `CandidateStore::allocate_id()` or similar.

**Suggested fix:** 
1. Add to design-05 §2.2:
```rust
impl CandidateStore {
    /// Generate next unique ID and increment counter. Not pub — engine calls this internally.
    fn allocate_id(&mut self, next_id: &mut u64) -> u64 {
        let id = *next_id;
        *next_id += 1;
        id
    }
}
```
2. Update design-05 §2.2 text: "The engine passes `&mut state.next_candidate_id` to `CandidateStore` when creating candidates. After each successful insert, `next_candidate_id` is incremented. On checkpoint restore (design-06 §2.4), the restored `next_candidate_id` value ensures subsequent IDs continue the sequence without collision."

---

### FINDING-6-1: **[Check #7] Unbounded retry/stagnation without forced halt**
Design-06 §2.1 includes `stagnation_counter: u64` and `consecutive_skips: u64` in state, but neither design-05 nor design-06 specifies **what happens when these counters exceed thresholds**.

Design-01 (Core Engine, referenced in §1 Overview) defines stopping criteria (GOAL-1.2a-d), but stagnation/skip thresholds are not among them. GOAL-7.5 (error retry policy) mentions skip increment but doesn't define a max.

**Trace the failure path:**
- Iteration N: adapter fails → skip counter++ → continue
- Iteration N+1: adapter fails → skip counter++ → continue
- ... (repeats forever)
- **No bounded retry** → infinite loop if adapter is persistently broken

**Suggested fix:** Add to design-06 §2.1:
```markdown
**Halt on excessive skips:** If `consecutive_skips` reaches `config.max_consecutive_skips` (default: 10), 
the engine terminates with `GEPAError::AdapterPersistentFailure`. This prevents infinite loops when the 
adapter is persistently broken. The counter resets to 0 on any successful iteration (accepted or rejected).
```

---

### FINDING-6-2: **[Check #8] UTF-8 unsafe string slicing in candidate reflection**
Design-05 §2.1 `Candidate::reflection: Option<String>` stores "the natural-language reflection text that guided the mutation" (from requirements-05 GOAL-5.2). This is **LLM-generated text** (non-ASCII-guaranteed).

Design-06 §4 "Pruning and Retention Policy" mentions "Retain candidates with instructive reflections (e.g., non-empty reflection field)" but provides no implementation details. If any downstream code does substring extraction (e.g., for logging truncation, display, or hashing), it risks UTF-8 boundary splits.

**Adversarial input:** LLM outputs emoji-heavy reflection "🔥🔥 Key insight..." → any `&reflection[..50]` for truncation → panic on non-char boundary.

**Suggested fix:** Add to design-05 §2.1 under `reflection` field:
```markdown
**UTF-8 safety:** Reflection text is LLM-generated and may contain multibyte Unicode. Any substring 
operation must use `.chars().take(n).collect()` or `.char_indices()`, never byte-slicing `[..n]`.
```

And add to design-06 §4:
```markdown
**Display truncation:** When logging reflections, use `.chars().take(100).collect::<String>()` 
instead of byte-slice truncation to avoid UTF-8 boundary panics.
```

---

## 🟡 Important (should fix before implementation)

### FINDING-5-3: **[Check #2] Missing reference to ParetoFront type definition**
Design-06 §2.1 includes `pareto_front: ParetoFront` in `GEPAState`, and design-05 §3.2 mentions "accepts/rejects from the Pareto front," but neither design-05 nor design-06 **defines or cross-references** the `ParetoFront` type.

The master design (§2 Architecture diagram) shows "Pareto Front (2)" referencing feature 2, but design-05/06 should explicitly say "see design-02" for the type definition.

**Suggested fix:** Add to design-06 §2.1 under `pareto_front: ParetoFront`:
```markdown
`pareto_front: ParetoFront` — The current non-dominated set (defined in design-02-pareto.md §2.1). 
Serialized as a list of candidate IDs plus crowding distances.
```

---

### FINDING-5-4: **[Check #14] Coupling: Candidate carries merge_parent_id but design-04 (Proposers) owns merge logic**
Design-05 §2.1 includes `merge_parent_id: Option<u64>` as a field on `Candidate`. But merge is a proposer concern (design-04 "Proposers"), not a candidate intrinsic property. This creates coupling: the candidate struct knows about a specific proposer strategy.

**Why this is a problem:**
- If a future proposer (e.g., crossover, recombination) needs to track 3+ parents, the `Candidate` struct must change
- Lineage query (design-05 §2.2) has special case: "Merge candidates also track merge_parent_id... lineage follows parent_id back to seed, NOT merge_parent_id" → the candidate struct encodes proposer-specific semantics

**Alternative design:** Store merge metadata in a separate `MergeMetadata` map owned by the proposer or in `GEPAState`. The candidate only has `parent_id` (primary lineage). Merge provenance is tracked separately if needed for analysis.

**Trade-off:** Current design is simpler for serialize/deserialize (all metadata co-located). Suggested design separates concerns but requires coordinated serialization.

**Suggested fix (if keeping current design):** Add explicit justification to design-05 §2.1:
```markdown
**Design note:** `merge_parent_id` is candidate metadata rather than proposer-internal state because:
1. Merge provenance is part of the candidate's identity (appears in lineage queries)
2. Co-locating all metadata simplifies checkpoint serialization
3. The field is optional; non-merge proposers leave it as None without overhead
```

---

### FINDING-6-3: **[Check #15] Hardcoded checkpoint filename default**
Design-06 §2.3 specifies checkpoint path as `GEPAConfig::checkpoint_path: PathBuf` with default `./gepa-checkpoint.json`. This is a **hardcoded string** in a library crate.

**Problem:** Two separate GEPA runs in the same directory will overwrite each other's checkpoints. Users must manually set distinct paths for concurrent runs.

**Better default:** Use run-specific filename based on timestamp or config hash:
```rust
// Default in GEPAConfig::default()
checkpoint_path: PathBuf::from(format!("./gepa-checkpoint-{}.json", Utc::now().timestamp()))
```

Or make it **required** (no default) to force users to think about checkpoint locations.

**Suggested fix:** Change design-06 §2.3:
```markdown
`checkpoint_path: PathBuf` — Path for checkpoint file. **Required field** (no default). 
Users must explicitly specify a path to avoid accidental overwrites in multi-run scenarios. 
Example: `PathBuf::from("./checkpoints/run-2026-04-06.json")`.
```

---

### FINDING-6-4: **[Check #21] Ambiguous: "same-filesystem rename" not guaranteed**
Design-06 §2.3 atomic checkpoint write: "The temp file is written to the same directory as the target (to ensure same-filesystem rename)."

**Ambiguity:** What if the target directory doesn't exist? What if it's a symlink? Two engineers would implement differently:
- Engineer A: `std::fs::create_dir_all(target.parent())` before writing temp
- Engineer B: Return error if parent doesn't exist
- Engineer C: Write temp to OS temp dir, fall back to cross-filesystem copy on `EXDEV`

**Suggested fix:** Be explicit in design-06 §2.3:
```markdown
**Atomic write implementation:**
1. Compute `temp_path = checkpoint_path.with_extension("tmp")`
2. If `checkpoint_path.parent()` does not exist, create it via `std::fs::create_dir_all()`
3. Write full JSON to `temp_path`
4. `std::fs::rename(temp_path, checkpoint_path)`
5. If rename fails with `EXDEV` (cross-filesystem), return `GEPAError::CheckpointError { message: "checkpoint path must be on local filesystem" }`
```

---

### FINDING-6-5: **[Check #6] EvaluationCache LRU timestamp not defined**
Design-06 §2.2 EvaluationCache uses LRU eviction with "An entry's LRU timestamp is updated on any read (cache hit) or write."

**Missing definition:** What is the timestamp type? `Instant`? `u64` monotonic counter? `DateTime<Utc>`?
- If `Instant`: not serializable (checkpoint won't preserve LRU order)
- If `u64`: must be incremented somewhere (where?)
- If `DateTime`: wall-clock time is not monotonic (clock adjustments)

**Suggested fix:** Add to design-06 §2.2:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    score: f64,
    lru_counter: u64,  // Monotonic access counter, incremented per hit/write
}

pub struct EvaluationCache {
    entries: HashMap<(u64, ExampleId), CacheEntry>,
    next_lru_counter: u64,  // Incremented on every access; checkpointed in GEPAState
    max_size: Option<usize>,
}
```

And add to design-06 §2.1 `GEPAState`:
```rust
pub eval_cache_lru_counter: u64,  // Restored on resume to preserve LRU order
```

---

### FINDING-5-5: **[Check #23] Dependency: serde HashMap key ordering assumption**
Design-05 §3.2 and design-06 §2.3 mention JSON serialization. Design-06 §2.4 says "Equality is defined as bitwise-identical JSON output from re-serialization" (GOAL-6.1).

**Problem:** `HashMap` serialization order is **not guaranteed deterministic** in serde_json unless using `preserve_order` feature or `BTreeMap`. Two serializations of the same HashMap may produce different JSON key orderings → checkpoint equality check fails.

From requirements-06 GOAL-6.1: "same evaluation cache (same entries, same scores to the bit)" — this requires key-order stability.

**Suggested fix:** Change design-05 §2.1 and design-06 §2.2:
- Replace `HashMap<String, String>` with `BTreeMap<String, String>` for candidate params
- Replace `HashMap<(u64, ExampleId), CacheEntry>` with deterministic structure (either `BTreeMap` or document the `preserve_order` serde feature requirement)

Or: Add to design-06 §2.4:
```markdown
**Deterministic serialization:** All `HashMap` types use `serde_json` with the `preserve_order` 
feature enabled (via `BTreeMap` internally) to ensure stable key ordering. This is required for 
the bitwise-identical equality check in GOAL-6.1.
```

---

### FINDING-6-6: **[Check #17] Missing non-goals for incremental checkpointing**
Design-06 §1 Overview mentions "Incremental/delta checkpoints (GOAL-6.6) are a P2 extension" but doesn't list **non-goals** for the P0/P1 implementation.

Without explicit non-goals, implementers might add partial incremental features (e.g., delta compression) thinking they're improving the design.

**Suggested fix:** Add to design-06 §1:
```markdown
**Non-goals (P0/P1):**
- Delta/incremental checkpoints (deferred to P2, GOAL-6.6)
- Checkpoint compression (gzip/zstd can be added externally)
- Multi-version checkpoint compatibility (v1 only reads v1 checkpoints; migration tools are separate)
- Checkpoint encryption (out of scope; users can encrypt checkpoint files externally)
```

---

## 🟢 Minor (can fix during implementation)

### FINDING-5-6: **[Check #4] Inconsistent naming: "params" vs "parameters"**
Design-05 uses both "params" (struct field name) and "parameters" (prose) interchangeably. Requirement GOAL-5.2 says "text parameters" but the field is `params: HashMap<String, String>`.

**Suggested fix:** Pick one term and use it consistently. Recommend: field name `params` (concise), prose "parameters" (formal in documentation).

---

### FINDING-6-7: **[Check #4] Inconsistent naming: ExampleId vs example_id**
Design-06 §2.2 uses `ExampleId` (capitalized type) but never defines it. Is it `String`? `u64`? `usize`?

**Suggested fix:** Add to design-06 §2.2:
```rust
pub type ExampleId = String;  // Or define as newtype: pub struct ExampleId(String);
```

---

### FINDING-5-7: **[Check #20] Pseudocode uses undefined DateTime::now()**
Design-05 §2.1 `Candidate::new()` shows `created_at: DateTime<Utc>` but the pseudocode doesn't show how it's populated. Is it `Utc::now()`? Passed in?

**Suggested fix:** Clarify in design-05 §2.1:
```rust
impl Candidate {
    pub fn new(...) -> Result<Self, GEPAError> {
        Ok(Self {
            created_at: Utc::now(),  // Timestamp at creation
            ...
        })
    }
}
```

---

### FINDING-6-8: **[Check #20] Ambiguous: "soft limit" for cache size**
Design-06 §2.2 says "if all cached entries belong to front candidates and the cache is at capacity, the cache size limit is temporarily exceeded (soft limit)."

**Ambiguity:** How much can it exceed? 2x? 10x? Unbounded until next pruning?

**Suggested fix:** Add explicit bound:
```markdown
**Soft limit behavior:** If all entries are pinned (front members) and cache is at `max_size`, 
new entries are still inserted, allowing the cache to exceed `max_size` by up to `pareto_max_size` 
entries (worst case: all front members need new eval entries). A warning is emitted when size 
exceeds `max_size * 1.1`.
```

---

### FINDING-5-8: **[Check #13] Pure function impurity: Candidate::new() does I/O via Utc::now()**
Design-05 §2.1 `Candidate::new()` is presented as a pure constructor, but calling `Utc::now()` is I/O (reads system clock).

**Not a blocker**, but violates the "pure logic stays pure" principle from master design §3.1.

**Suggested fix:** Document this explicitly:
```markdown
**Design note:** `Candidate::new()` calls `Utc::now()` for `created_at`, which reads the system 
clock (side effect). This is acceptable because:
1. Timestamp is metadata, not core logic
2. Determinism (GUARD-9) is preserved via checkpoint serialization (timestamps are restored, not regenerated)
```

---

### FINDING-6-9: **[Check #25] Testability: EvaluationCache pinning logic needs unit test strategy**
Design-06 §2.2 "entries for candidates currently on the Pareto front are never evicted (pinned)" is complex logic:
- How to test that pinned entries aren't evicted?
- How to test the soft-limit overflow?
- How to test LRU ordering when some entries are pinned?

**Not a design bug**, but the design should suggest test scaffolding.

**Suggested fix:** Add to design-06 §2.2:
```markdown
**Testing strategy:** 
- Unit tests mock `ParetoFront::members()` to return a fixed set of IDs
- Test: insert `max_size + 10` entries, verify only non-front entries are evicted
- Test: fill cache with front-only entries, insert one more, verify soft limit warning
```

---

## 📋 Path Traces

### Trace 1: Candidate creation (happy path)
```
Engine at iteration N
  → ProposerState::select_parent (design-04) → returns parent_id=5
  → Adapter::mutate(parent_id=5, ...) → returns params HashMap
  → state.next_candidate_id = 10
  → CandidateStore::allocate_id(&mut state.next_candidate_id) → returns 10, increments to 11 ✅
  → Candidate::new(id=10, params, parent_id=Some(5), ...) → validates params ✅
  → CandidateStore::insert(candidate) → stores in HashMap ✅
  → state.next_candidate_id now 11 for next iteration ✅
```

### Trace 2: Checkpoint write (happy path)
```
Engine after iteration N
  → config.checkpoint_every_n_iters = 1, iteration % 1 == 0 → trigger checkpoint
  → temp_path = "gepa-checkpoint.tmp"
  → serde_json::to_string_pretty(&state) → JSON string ✅
  → File::create(temp_path) → write JSON ✅
  → std::fs::rename(temp_path, "gepa-checkpoint.json") → atomic ✅
  → emit CheckpointSaved { success: true } ✅
```

### Trace 3: Checkpoint write (disk full failure path)
```
Engine after iteration N
  → trigger checkpoint
  → File::create(temp_path) → write JSON → returns Err(io::Error "No space left")
  → catch error, do NOT propagate (design-06 §2.3 "failed checkpoint does not halt engine")
  → emit CheckpointSaved { success: false, error: Some("No space left") } ⚠️
  → continue to next iteration ✅
  → **Issue:** temp file may exist and consume disk space → should delete on error
```

**Suggested fix for Trace 3:** Add to design-06 §2.3:
```markdown
If checkpoint write fails at any step (serialization, file write, rename), the temp file 
(if created) is deleted via `std::fs::remove_file()` before emitting the error event.
```

### Trace 4: Cache eviction with all entries pinned (edge case)
```
EvaluationCache at capacity (max_size=100, current size=100)
  → All 100 entries are for front candidates (pinned) ✅
  → New entry (candidate_id=101, example_id=5) → insert
  → Eviction triggered: scan all entries, all are pinned → cannot evict ⚠️
  → Insert anyway (soft limit) → size now 101 ✅
  → Emit warning: "Cache exceeded soft limit: 101/100" ✅
  → Next iteration: candidate 42 leaves front (pruned)
  → Its entries now unpinned → eligible for LRU eviction ✅
```

### Trace 5: Lineage traversal (merge candidate)
```
Candidate ID=20, parent_id=Some(15), merge_parent_id=Some(18)
  → CandidateStore::lineage(20)
  → Follow parent_id chain: 20 → 15 → 10 → 5 (seed) ✅
  → Returns [20, 15, 10, 5] ✅
  → merge_parent_id=18 is ignored (per design-05 §2.2 "lineage follows parent_id... NOT merge_parent_id") ✅
  → But merge_parent_id is still stored for provenance analysis (non-blocking) ✅
```

---

## ✅ Passed Checks

### Design-05 (Candidates)

- **Check #0 (Document size):** 4 components (§2.1–2.4) ✅ (well under 8-component limit)
- **Check #1 (Types fully defined):** `Candidate` struct fully defined with all 7 fields ✅ (except CandidateStore — see FINDING-5-1)
- **Check #2 (References resolve):** All cross-references to design-06, design-04, master design validated ✅ (except ParetoFront — see FINDING-5-3)
- **Check #3 (No dead definitions):** All fields in `Candidate` used in lineage, validation, or serialization ✅
- **Check #5 (State machines):** No state machine in this design ✅ (N/A)
- **Check #7 (Error handling):** `Candidate::new()` returns `Result<Self, GEPAError::InvalidCandidate>` for validation failures ✅
- **Check #9 (Integer overflow):** `generation: u32` — with default mutation rate, 4B generations is 4B iterations → effectively unbounded ✅; `id: u64` same reasoning ✅
- **Check #10 (Option handling):** `parent_id`, `merge_parent_id`, `reflection` all explicitly documented as `None` for seeds ✅
- **Check #11 (Match exhaustiveness):** No match statements in design ✅ (N/A)
- **Check #12 (Ordering sensitivity):** Validation checks are independent (params non-empty, keys non-empty) ✅
- **Check #16 (API surface):** `Candidate::new()`, `seed()`, `is_seed()` are minimal necessary set ✅; all fields `pub` for serialization ✅
- **Check #18 (Trade-offs documented):** Design-05 §1 justifies flat storage over tree: "simplifies serialization and avoids recursive ownership" ✅
- **Check #19 (Cross-cutting concerns):** Serialization format (JSON via serde) documented ✅; security out of scope ✅
- **Check #22 (Missing helpers):** `lineage()` is defined in §2.2 ✅
- **Check #24 (Migration path):** N/A (new feature, no existing code to replace) ✅
- **Check #26 (Similar functionality exists):** No overlap with existing codebase (verified from master design) ✅
- **Check #27 (API compatibility):** N/A (new API) ✅
- **Check #28 (Feature flag):** N/A (core feature, not optional) ✅

### Design-06 (State/Checkpointing)

- **Check #0 (Document size):** 4 components (§2.1–2.4) ✅ (well under 8-component limit)
- **Check #1 (Types fully defined):** `GEPAState` 15 fields fully specified ✅; `MinibatchState`, `RngState` fields defined ✅; `ProposerState` forward-ref to design-04 ✅
- **Check #2 (References resolve):** §2.3 atomic write references §2.1 fields ✅; §2.4 resume references §2.1 ✅
- **Check #3 (No dead definitions):** All `GEPAState` fields used in checkpoint/resume flow ✅
- **Check #5 (State machines):** No explicit state machine ✅ (checkpointing is event-driven, not state-transition)
- **Check #7 (Error handling):** Checkpoint failures return `GEPAError::CheckpointCorrupt` or emit error events ✅; resume failures propagate errors ✅
- **Check #9 (Integer overflow):** `iteration: u64`, `stagnation_counter: u64` — 2^64 iterations is effectively unbounded ✅
- **Check #10 (Option handling):** `config.eval_cache_max_size: Option<usize>` — None means unlimited ✅
- **Check #11 (Match exhaustiveness):** No match statements ✅
- **Check #12 (Ordering sensitivity):** Checkpoint write steps are sequential (no branching) ✅
- **Check #16 (API surface):** `GEPAState::resume()`, checkpoint write are minimal ✅; all fields `pub` for serialization ✅
- **Check #18 (Trade-offs documented):** §1 Overview: "full JSON snapshots" vs "incremental/delta (P2)" ✅; atomic write vs performance trade-off documented ✅
- **Check #19 (Cross-cutting concerns):** Observability via `CheckpointSaved` event ✅; error visibility in event payload ✅
- **Check #22 (Missing helpers):** All referenced functions defined ✅
- **Check #24 (Migration path):** N/A (new feature) ✅
- **Check #25 (Testability):** Resume logic can be unit-tested with mock JSON ✅ (but see FINDING-6-9 for cache pinning)
- **Check #26 (Similar functionality exists):** No overlap ✅
- **Check #27 (API compatibility):** N/A (new API) ✅
- **Check #28 (Feature flag):** Checkpointing is core (always on); incremental is P2 feature flag ✅

---

## Summary

- **Critical:** 4 findings (FINDING-5-1, 5-2, 6-1, 6-2)
- **Important:** 6 findings (FINDING-5-3, 5-4, 5-5, 6-3, 6-4, 6-5, 6-6)
- **Minor:** 6 findings (FINDING-5-6, 5-7, 5-8, 6-7, 6-8, 6-9)
- **Path traces:** 5 traces (candidate creation, checkpoint write happy/failure, cache eviction edge case, lineage traversal)
- **Recommendation:** **Needs fixes before implementation** — Critical findings block implementation (incomplete type definitions, broken data flow, unbounded retry, UTF-8 safety). Important findings should be resolved to avoid rework.
- **Estimated implementation confidence:** **Medium** — Core logic is sound, but several data-flow and edge-case gaps need specification. After fixes, confidence → High.

---

## Detailed Finding Reference

### Critical Findings Summary
1. **FINDING-5-1:** CandidateStore type incomplete → add full struct definition
2. **FINDING-5-2:** ID generation data flow broken → add `allocate_id()` method, clarify counter ownership
3. **FINDING-6-1:** Unbounded retry/skip counters → add max thresholds and halt condition
4. **FINDING-6-2:** UTF-8 unsafe reflection strings → document safe substring operations

### Important Findings Summary
1. **FINDING-5-3:** Missing cross-reference to ParetoFront → add "see design-02"
2. **FINDING-5-4:** Coupling smell (merge_parent_id in Candidate) → add justification or refactor
3. **FINDING-5-5:** HashMap non-deterministic serialization → use BTreeMap or preserve_order
4. **FINDING-6-3:** Hardcoded checkpoint path → make it required or use run-specific default
5. **FINDING-6-4:** Ambiguous atomic write implementation → specify exact steps
6. **FINDING-6-5:** LRU timestamp type undefined → use monotonic counter, add to GEPAState
7. **FINDING-6-6:** Missing non-goals for incremental checkpointing → add explicit list

### Minor Findings Summary
1. **FINDING-5-6:** Inconsistent "params" vs "parameters" naming
2. **FINDING-5-7:** ExampleId type undefined
3. **FINDING-5-8:** Candidate::new() impure (calls Utc::now()) → document why acceptable
4. **FINDING-6-7:** Same as 5-7 (consolidated)
5. **FINDING-6-8:** "Soft limit" unbounded → specify max overflow
6. **FINDING-6-9:** Cache pinning testability → suggest test scaffolding

---

## Next Steps

**Recommended application order:**
1. Apply all Critical findings first (FINDING-5-1, 5-2, 6-1, 6-2) — these block implementation
2. Review Important findings with team — some may be design decisions to accept (e.g., FINDING-5-4 coupling)
3. Apply Important findings that are clear improvements (5-3, 5-5, 6-3, 6-4, 6-5, 6-6)
4. Address Minor findings during implementation or as documentation polish

**Which findings should I apply?** (respond with finding IDs, e.g., 'apply FINDING-5-1,5-2,6-1,6-2' or 'apply all critical')
