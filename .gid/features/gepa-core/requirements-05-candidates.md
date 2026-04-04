# Requirements: GEPA Candidate Management

> Feature 5 of 9 — Master doc: `requirements-master.md`

Candidates are the evolving text artifacts. Each candidate is a dictionary of named text parameters with metadata tracking its lineage, scores, and history.

## Goals

- **GOAL-5.1** [P0]: The engine requires at least one seed candidate to begin optimization. The user provides one or more seed candidates via `GEPAEngine::new()` or `GEPAConfig`. Seed candidates form the initial Pareto front at iteration 0. If no seed candidate is provided, engine construction fails with a descriptive error.

- **GOAL-5.2** [P0]: A `Candidate` contains: a unique ID, a `HashMap<String, String>` of named text parameters (e.g., "system_prompt", "tool_description"), and metadata including: parent ID (None for seed), generation number, creation timestamp, and the reflection that produced it. Per-example scores are NOT stored on the Candidate struct — they are maintained in the evaluation cache (GOAL-6.3) keyed by `(candidate_id, example_id)`. This separation ensures candidates remain lightweight and immutable (GOAL-5.3) while scores accumulate incrementally via evaluation and re-evaluation backfill (GOAL-8.5).

- **GOAL-5.3** [P0]: Candidates are immutable after creation. Mutation produces a new candidate; it never modifies the parent in-place. This guarantees lineage integrity and safe concurrent reads.

- **GOAL-5.4** [P0]: Candidate IDs are unique within a run. Two candidates produced by different mutations always have different IDs, even if their text parameters happen to be identical.

- **GOAL-5.5** [P1]: The full lineage of any candidate is reconstructable from the candidate history: given a candidate, traverse parent IDs to recover the complete chain of mutations and reflections back to the seed candidate.

- **GOAL-5.6** [P1]: Candidates are serializable (serde Serialize + Deserialize). Serialized format is JSON. A round-trip serialize→deserialize produces an identical candidate (all fields preserved, including metadata).

## Cross-references

- GOAL-6.3 (State) — evaluation cache stores scores separately from candidates
- GOAL-8.5 (Data Loading) — re-evaluation backfill adds scores incrementally
- GUARD-2 — immutability invariant

**Summary: 6 GOALs** (4 P0, 2 P1)
