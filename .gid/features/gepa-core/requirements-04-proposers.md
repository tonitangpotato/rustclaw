# Requirements: GEPA Proposers

> Feature 4 of 9 — Master doc: `requirements-master.md`

Proposers generate new candidates from the Pareto front. The mutation proposer is the primary mechanism (every iteration); the merge proposer is an optional secondary mechanism (periodic).

## Goals

- **GOAL-4.1** [P0]: The mutation proposer implements the core engine loop's execute → reflect → mutate steps (GOAL-1.4, GOAL-1.5, GOAL-1.6). It is the component that the engine delegates to for candidate generation. It selects a parent from the Pareto front (via GOAL-2.3 selection), runs execute → reflect → mutate via the adapter, and produces exactly one new candidate per iteration on the success path. If any adapter call fails after retry exhaustion (per GOAL-7.5), the proposer propagates the error to the engine, which applies the configured error policy (skip or halt). "Exactly one candidate per iteration" applies to the success path.

- **GOAL-4.2** [P0]: The mutation proposer constructs the ancestor lesson chain: the reflections from all ancestors in the lineage (parent, grandparent, etc.), ordered from most recent to oldest, and passes it to the adapter's `mutate` method.

- **GOAL-4.2b** [P0]: If the ancestor lesson chain exceeds a configurable maximum lesson depth (`max_lesson_depth`, default: 10, see GOAL-7.1), only the most recent N lessons are passed to the adapter's `mutate` method.

- **GOAL-4.3** [P1]: The mutation proposer maintains a selection counter per front member and enforces the round-robin floor described in GOAL-2.3: it tracks selections and ensures every front member is selected at least once every `pareto_max_size` iterations before any member is selected again. The proposer owns the tracking state; GOAL-2.3 defines the policy.

- **GOAL-4.4** [P2]: The merge proposer (optional, controlled by GOAL-7.7) selects two front candidates with the most complementary performance profiles (excel on the most different task subsets). Complementarity is computed over the intersection of examples both candidates have been evaluated on (from the evaluation cache, GOAL-6.3). "Complementary" is defined as: maximize |A_better ∪ B_better| where A_better is the set of shared examples where A scores higher, and B_better where B scores higher. Ties (identical scores on an example) count toward neither A_better nor B_better. Tie-breaking: prefer the pair with the highest combined average score; if still tied, break using the seeded RNG per GUARD-9. If the front has fewer than 2 candidates, the merge step is skipped for that interval. Complementary pair selection scans all O(N²) pairs where N is the front size. For typical workloads (N ≤ 50, M ≤ 200), this is negligible relative to adapter call time (per GUARD-6). This ensures the merge combines specialists, not generalists, which produces more diverse merged candidates.

- **GOAL-4.5** [P2]: The merge proposer provides the adapter's `merge` method with sufficient context: both parent `Candidate` objects (which include text parameters), their respective per-example scores, and identification of which task subsets each parent excels on. The adapter's `merge` method returns a new `Candidate` (per GOAL-3.6). The merged candidate has both parents recorded in its lineage (tree-structured). After production, the merged candidate is evaluated and accepted/rejected using the same rules as mutated candidates (GOAL-1.7, GOAL-1.7d). (This goal specifies the WHAT of merge context; GOAL-1.10 specifies WHEN merging occurs in the engine loop.)

### Applicable GUARDs

- **GUARD-2** (determinism) — proposers produce new candidates, never modify existing ones
- **GUARD-5** (no LLM in core) — proposers delegate all LLM calls to the adapter
- **GUARD-8** (Debug impls) — all proposer types implement Debug

## Cross-references

- GOAL-1.10 (Core Engine) — merge step scheduling
- GOAL-2.3 (Pareto Front) — selection strategy
- GOAL-3.2 (execute), GOAL-3.3 (reflect), GOAL-3.4 (mutate), GOAL-3.6 (merge) — adapter methods used by proposers
- GOAL-7.7 (Config) — merge proposer settings

**Summary: 6 GOALs** (3 P0, 1 P1, 2 P2)
