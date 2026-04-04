# Requirements: GEPA Candidate Management

> Feature 5 of 9 — Master doc: `requirements-master.md`

Candidates are the evolving text artifacts. Each candidate is a dictionary of named text parameters with metadata tracking its lineage, scores, and history.

## Goals

- **GOAL-5.1** [P0]: The engine requires at least one seed candidate to begin optimization. The user provides one or more seed candidates via `GEPAEngine::new()` or `GEPAConfig`. Seed candidates form the initial Pareto front at iteration 0. If no seed candidate is provided, engine construction fails with a descriptive error. Seed candidates are validated at construction: each must have at least one text parameter (GOAL-5.2 constraint), and all seed candidates must have the same set of parameter keys. Mismatched parameter keys across seeds produce `GEPAError::InvalidCandidate { message }`.

- **GOAL-5.2** [P0]: A `Candidate` contains: a unique ID (`u64`, see GOAL-5.4), a `HashMap<String, String>` of **text parameters** (e.g., "system_prompt", "tool_description"), and metadata including: parent ID (`Option<u64>`, None for seed), generation number, creation timestamp, and `reflection: Option<String>` — the natural-language reflection text that guided the mutation producing this candidate (None for seed candidates; the full output from the adapter's `reflect` method per GOAL-1.5). A candidate must have at least one text parameter; construction with an empty parameter map fails with `GEPAError::InvalidCandidate`. Keys must be non-empty strings. Values may be empty strings (representing a parameter to be filled by mutation). Per-example scores are NOT stored on the Candidate struct — they are maintained in the evaluation cache (GOAL-6.3) keyed by `(candidate_id, example_id)`. This separation ensures candidates remain lightweight and immutable (GOAL-5.3) while scores accumulate incrementally via evaluation and re-evaluation backfill (GOAL-8.5).

- **GOAL-5.3** [P0]: Candidates are immutable after creation. Mutation produces a new candidate; it never modifies the parent in-place. This guarantees lineage integrity and safe concurrent reads.

- **GOAL-5.4** [P0]: Candidate IDs are unique within a run. Two candidates produced by different mutations always have different IDs, even if their text parameters happen to be identical. Candidate IDs are generated as monotonically increasing `u64` values from a per-run counter starting at 0 for the first seed candidate. This ensures uniqueness, determinism (GUARD-9), compact serialization, and efficient use as HashMap keys. Seed candidates receive IDs 0..N-1 in the order provided; subsequent candidates receive the next available ID.

- **GOAL-5.5** [P1]: The engine provides `GEPAState::lineage(candidate_id) -> Result<Vec<CandidateId>, GEPAError>` which returns the ordered chain of ancestor IDs from the given candidate back to its seed (inclusive). Returns `Err(GEPAError::CandidateNotFound)` if the candidate_id is unknown. Full candidate data and reflections for each ancestor are available via `GEPAState::candidate(id)`.

- **GOAL-5.6** [P1]: Candidates are serializable (serde Serialize + Deserialize). Serialized format is JSON. A round-trip serialize→deserialize produces an identical candidate (all fields preserved, including metadata). Checkpoint format compatibility across code versions is out of scope for v1.

- **GOAL-5.7** [P1]: `Candidate` implements `Clone`, `Send`, `Sync`, `Debug`, `Serialize`, `Deserialize`, `PartialEq`, and `Eq`. GUARD-8 requires Debug; GUARD-9 determinism requires PartialEq for verification; Send+Sync for safe concurrent reads per GOAL-5.3.

### Applicable GUARDs

- **GUARD-7** (memory <1GB/10K) — candidates remain lightweight (no scores stored on struct)
- **GUARD-8** (Debug impls) — Candidate implements Debug (GOAL-5.7)

## Cross-references

- GOAL-6.3 (State) — evaluation cache stores scores separately from candidates
- GOAL-8.5 (Data Loading) — re-evaluation backfill adds scores incrementally
- GUARD-2 — immutability invariant (implemented by GOAL-5.3)

**Summary: 7 GOALs** (4 P0, 3 P1)
