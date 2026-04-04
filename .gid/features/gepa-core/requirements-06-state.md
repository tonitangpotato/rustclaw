# Requirements: GEPA State Management

> Feature 6 of 9 — Master doc: `requirements-master.md`

`GEPAState` holds everything needed to checkpoint and resume an optimization run: the Pareto front, all candidate history, evaluation cache, iteration count, and accumulated statistics.

## Goals

- **GOAL-6.1** [P0]: `GEPAState` can be serialized to JSON and deserialized back. A round-trip produces identical state: same Pareto front (same members, same order), same candidate history (same candidates, same metadata), same evaluation cache (same entries, same scores to the bit), same iteration counter. Equality is defined as bitwise-identical JSON output from re-serialization.

- **GOAL-6.2** [P0]: The engine writes a checkpoint after every N iterations (configurable, default: every iteration). Checkpoint is a single JSON file written atomically (write to temp file, then rename) so a crash mid-write never corrupts the checkpoint. Checkpoint path is configurable via `GEPAConfig::checkpoint_path: PathBuf` (default: `./gepa-checkpoint.json`). Each checkpoint overwrites the previous one (single file, not one per iteration). The temp file is written to the same directory as the target (to ensure same-filesystem rename). If the checkpoint write fails (disk full, permission denied), the engine emits a `CheckpointSaved` event with an error flag and continues the optimization loop — a failed checkpoint does not halt the engine.

- **GOAL-6.3** [P0]: `GEPAState` contains an evaluation cache mapping (candidate_id, example_id) → score. When a candidate is evaluated on an example that was previously scored, the cached score is returned without calling the adapter.

- **GOAL-6.4** [P1]: The evaluation cache has a configurable maximum number of entries (each entry is one `(candidate_id, example_id) → f64` mapping), configured via `eval_cache_max_size: Option<usize>` in `GEPAConfig` (default: `None`, meaning unlimited — LRU eviction only activates when a limit is set). When the cache exceeds the limit, least-recently-used entries are evicted, **with the constraint that entries for candidates currently on the Pareto front are never evicted** (pinned). An entry's LRU timestamp is updated on any read (cache hit) or write. Entries that are frequently involved in dominance comparisons will naturally be retained. Only entries for candidates that have left the front (pruned or historically rejected) are eligible for LRU eviction. This ensures that dominance relationships among active front members are never invalidated by cache eviction — the score matrix for front members remains complete. **Boundary condition:** if all cached entries belong to front candidates and the cache is at capacity, the cache size limit is temporarily exceeded (soft limit) rather than blocking new evaluations. A warning event is emitted. The next front pruning (GOAL-2.2) will free entries for eviction. Cache hit rate is tracked and reported in statistics.

- **GOAL-6.5** [P1]: `GEPAState` tracks run statistics: total iterations, skipped iterations (adapter failures), total adapter calls broken down by type (execute, reflect, mutate, evaluate, merge), total candidates generated, total candidates accepted, acceptance rate (accepted/generated), best score over time (per-iteration best), and Pareto front size over time (per-iteration snapshot). All counters are `u64`. Rate/ratio fields are `f64`.

- **GOAL-6.6** [P2]: Support incremental checkpoint: instead of writing full state every time, write only the delta (new candidates, updated scores, updated front) since the last checkpoint. Deltas are sequentially numbered starting from 1. Full state is reconstructable from the initial checkpoint plus all deltas applied in order. If a delta is missing or corrupt during reconstruction, the engine returns `GEPAError::CheckpointCorrupt { message }` and the caller must fall back to the last known-good full checkpoint. A full checkpoint can be compacted from the base + deltas at any time to create a new base (resetting the delta counter). Delta format and ordering are implementation-defined but must be deterministic (GUARD-9).

### Applicable GUARDs

- **GUARD-2** (determinism) — state serialization must be deterministic for reproducibility
- **GUARD-4** (no panics) — atomic checkpoint writes prevent corruption; errors return Result, never panic
- **GUARD-8** (Debug impls) — GEPAState and all contained types implement Debug

## Cross-references

- GOAL-1.9 (Core Engine) — resumption from checkpoint
- GOAL-2.x (Pareto Front) — front serialization
- GOAL-5.6 (Candidates) — candidate serialization
- GUARD-4 — atomic checkpoint writes

**Summary: 6 GOALs** (3 P0, 2 P1, 1 P2)
