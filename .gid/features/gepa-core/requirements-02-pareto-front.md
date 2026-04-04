# Requirements: GEPA Pareto Front

> Feature 2 of 9 — Master doc: `requirements-master.md`

Multi-objective candidate management. The Pareto front maintains the set of non-dominated candidates — those that are best on at least one task subset. This prevents catastrophic forgetting where improving on one subset regresses another.

## Goals

- **GOAL-2.1** [P0]: Given a set of candidates with per-example scores stored in the evaluation cache (GOAL-6.3), compute the Pareto front: the subset of candidates where no candidate is dominated by another. Candidate A dominates candidate B if, on the intersection of examples both have been evaluated on (looked up from the evaluation cache, GOAL-6.3), A scores ≥ B on every shared example and strictly > on at least one. Dominance can only be established when the intersection size meets `min_shared_examples` (GOAL-7.1); otherwise, the two candidates are treated as mutually non-dominating.

- **GOAL-2.2** [P0]: When a new candidate is accepted, update the Pareto front: add the new candidate, then remove any existing candidates that are now dominated by it (on shared examples meeting the `min_shared_examples` threshold). The front must remain valid (no dominated candidates) after every update. **Re-evaluation triggered recomputation:** when re-evaluation backfill (GOAL-8.5) adds new scores to the evaluation cache, dominance relationships may change — a previously non-dominating pair may now have sufficient shared examples to establish dominance. After each re-evaluation round, the engine recomputes the front by re-checking all pairwise dominance relationships with updated score coverage. Candidates newly found to be dominated are removed.

- **GOAL-2.3** [P0]: Pareto front selection returns a candidate for mutation. The selection strategy must not always pick the same candidate — it should vary across front members to ensure diversity of exploration. Selection MAY use re-evaluation scores (GOAL-8.5) as a secondary signal to deprioritize candidates with high overfitting delta (large gap between training and re-evaluation scores), but MUST NOT remove candidates from the front based on re-evaluation alone — only the dominance mechanism (GOAL-2.2) removes front members. **Starvation prevention:** overfitting delta deprioritization is bounded: every front member must be selected at least once every `pareto_max_size` iterations (round-robin floor) to prevent starvation. Overfitting delta influences selection order within each round, not exclusion.

- **GOAL-2.4** [P1]: The Pareto front has a configurable maximum size (default: 50, per GOAL-7.2). When the front exceeds the maximum, the least-contributing candidate is removed using **crowding distance** (the candidate with the smallest crowding distance is pruned). Crowding distance is chosen over hypervolume contribution because: (a) it computes in O(N·M·log M) vs O(N^M) for exact hypervolume in high dimensions, (b) GEPA's typical M (number of examples per minibatch, 16-200) makes hypervolume computation intractable, and (c) crowding distance is well-understood from NSGA-II with predictable behavior. Ties in crowding distance are broken by candidate age (oldest removed first). Known limitation: crowding distance becomes less discriminating at high M (>50 examples). This is acceptable for v1 because (a) most real workloads use M=16-64, and (b) the age-based tie-breaker provides a reasonable fallback when crowding distances converge. If empirically problematic, a future version can introduce a pluggable pruning strategy trait.

- **GOAL-2.5** [P1]: Pareto dominance checking for N candidates completes in O(N²·M) time or better, where M is the size of the largest candidate's evaluated example set (the per-pair intersection is computed via sorted example ID merge in O(M)). For typical workloads (N ≤ 100, M ≤ 200), full front recomputation should be negligible relative to adapter call time (~10ms on modern hardware). This is a soft target validated by benchmarks, not a hard SLA.

- **GOAL-2.6** [P1]: The Pareto front is serializable (serde Serialize + Deserialize) for checkpoint/resume. Deserialized front is identical to the original (same candidates, same ordering, same dominance relationships).

## Cross-references

- GOAL-1.7a-d (Core Engine) — acceptance and re-evaluation logic
- GOAL-6.3 (Evaluation Cache) — score lookups for dominance
- GOAL-7.1 (Config) — `min_shared_examples`, `pareto_max_size`
- GOAL-8.5 (Data Loading) — re-evaluation backfill triggers recomputation

**Summary: 6 GOALs** (3 P0, 3 P1)
