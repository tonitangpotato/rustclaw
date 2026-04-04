# Requirements: GEPA Proposers

> Feature 4 of 9 — Master doc: `requirements-master.md`

Proposers generate new candidates from the Pareto front. The mutation proposer is the primary mechanism (every iteration); the merge proposer is an optional secondary mechanism (periodic).

## Goals

- **GOAL-4.1** [P0]: The mutation proposer selects a parent from the Pareto front (via GOAL-2.3 selection), runs execute → reflect → mutate via the adapter, and produces exactly one new candidate per iteration.

- **GOAL-4.2** [P0]: The mutation proposer passes the full ancestor lesson chain to the adapter's `mutate` method. Ancestor lessons are the reflections from all ancestors in the lineage (parent, grandparent, etc.), ordered from most recent to oldest. If the lineage exceeds a configurable maximum lesson depth (default: 10), only the most recent N lessons are passed.

- **GOAL-4.3** [P1]: The mutation proposer tracks which Pareto front members have been selected as parents and ensures balanced selection: no front member is starved (see GOAL-2.3 starvation prevention).

- **GOAL-4.4** [P2]: The merge proposer (optional, controlled by GOAL-7.7) selects two front candidates with the most complementary performance profiles (excel on the most different task subsets). "Complementary" is defined as: maximize |A_better ∪ B_better| where A_better is the set of shared examples where A scores higher, and B_better where B scores higher. Tie-breaking: prefer the pair with the highest combined average score.

- **GOAL-4.5** [P2]: The merge proposer provides the adapter's `merge` method with sufficient context: both parent candidates' text parameters, their respective per-example scores, and identification of which task subsets each parent excels on. The adapter uses this context to produce a merged candidate. (This goal specifies the WHAT of merge context; GOAL-1.10 specifies WHEN merging occurs in the engine loop.)

## Cross-references

- GOAL-1.10 (Core Engine) — merge step scheduling
- GOAL-2.3 (Pareto Front) — selection strategy
- GOAL-3.x (Adapter) — execute, reflect, mutate, merge methods
- GOAL-7.7 (Config) — merge proposer settings

**Summary: 5 GOALs** (2 P0, 1 P1, 2 P2)
