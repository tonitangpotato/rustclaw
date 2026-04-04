# Requirements: GEPA State Management

> Feature 6 of 9 — Master doc: `requirements-master.md`

`GEPAState` holds everything needed to checkpoint and resume an optimization run: the Pareto front, all candidate history, evaluation cache, iteration count, and accumulated statistics.

## Goals

- **GOAL-6.1** [P0]: `GEPAState` can be serialized to JSON and deserialized back. A round-trip produces functionally identical state: same Pareto front, same candidate history, same evaluation cache, same iteration counter.

- **GOAL-6.2** [P0]: The engine writes a checkpoint after every N iterations (configurable, default: every iteration). Checkpoint is a single JSON file written atomically (write to temp file, then rename) so a crash mid-write never corrupts the checkpoint.

- **GOAL-6.3** [P0]: `GEPAState` contains an evaluation cache mapping (candidate_id, example_id) → score. When a candidate is evaluated on an example that was previously scored, the cached score is returned without calling the adapter.

- **GOAL-6.4** [P1]: The evaluation cache has a configurable maximum size. When the cache exceeds the limit, least-recently-used entries are evicted, **with the constraint that entries for candidates currently on the Pareto front are never evicted** (pinned). Only entries for candidates that have left the front (pruned or historically rejected) are eligible for LRU eviction. This ensures that dominance relationships among active front members are never invalidated by cache eviction — the score matrix for front members remains complete. **Boundary condition:** if all cached entries belong to front candidates and the cache is at capacity, the cache size limit is temporarily exceeded (soft limit) rather than blocking new evaluations. A warning event is emitted. The next front pruning (GOAL-2.2) will free entries for eviction. Cache hit rate is tracked and reported in statistics.

- **GOAL-6.5** [P1]: `GEPAState` tracks run statistics: total iterations, skipped iterations (adapter failures), total adapter calls (execute, reflect, mutate, evaluate), total candidates generated, total candidates accepted, acceptance rate, best score over time, and Pareto front size over time.

- **GOAL-6.6** [P2]: Support incremental checkpoint: instead of writing full state every time, write only the delta (new candidates, updated scores, updated front) since the last checkpoint. Full state is reconstructable from the initial checkpoint plus all deltas.

## Cross-references

- GOAL-1.9 (Core Engine) — resumption from checkpoint
- GOAL-2.x (Pareto Front) — front serialization
- GOAL-5.6 (Candidates) — candidate serialization
- GUARD-4 — atomic checkpoint writes

**Summary: 6 GOALs** (3 P0, 2 P1, 1 P2)
