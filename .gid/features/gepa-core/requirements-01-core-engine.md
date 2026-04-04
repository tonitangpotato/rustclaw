# Requirements: GEPA Core Engine

> Feature 1 of 9 — Master doc: `requirements-master.md`

The main GEPA optimization loop. Orchestrates the Select → Execute → Reflect → Mutate → Accept cycle, delegates to adapter for LLM calls, and terminates based on configured stopping criteria.

## Goals

- **GOAL-1.0** [P1]: The complete happy path is: construct `GEPAConfig` (with defaults or custom) → construct `GEPAEngine` with config + adapter + data_loader + seed candidates → call `engine.run()` → receive `GEPAResult`. The engine builder validates all inputs at construction time (GOAL-7.3) so `run()` cannot fail due to misconfiguration.

- **GOAL-1.1** [P0]: `GEPAEngine::run()` executes the full optimization loop: select a candidate from the Pareto front, execute it on a minibatch, reflect on traces, mutate to produce a new candidate, and accept/reject based on score comparison. Each iteration performs all 5 steps in order.

- **GOAL-1.2a** [P0]: The engine terminates when maximum iterations reached or wall-clock time budget exhausted.

- **GOAL-1.2b** [P0]: The engine terminates on stagnation: no new candidate accepted to the Pareto front for N consecutive iterations where a mutation was actually attempted and rejected as dominated. Skipped iterations (due to adapter errors) do NOT count toward the stagnation counter — see GOAL-7.5 for skip/stagnation interaction.

- **GOAL-1.2c** [P0]: The engine terminates on too many consecutive skips (GOAL-7.5) or cancellation (GOAL-3.7).

- **GOAL-1.2d** [P0]: Termination reason is reported in the result as one of: `MaxIterations`, `TimeBudget`, `Stagnation`, `TooManySkips`, `Cancelled`.

- **GOAL-1.3** [P0]: At the start of each iteration, the engine selects a parent candidate from the current Pareto front. Selection must consider candidates' performance across different task subsets (Pareto-aware selection), not just global average score.

- **GOAL-1.4** [P0]: The engine passes the selected candidate to the adapter for execution on a minibatch of training examples. The adapter returns execution traces containing: input, generated output, score, and optional actionable side information (ASI) per example.

- **GOAL-1.5** [P0]: After execution, the engine passes execution traces to the adapter for reflection. The adapter returns a natural-language diagnosis of failure causes and improvement directions. The engine stores this reflection as part of the candidate's lineage.

- **GOAL-1.6** [P0]: After reflection, the engine passes the parent candidate, reflection text, and accumulated ancestor lessons to the adapter for mutation. The adapter returns a new candidate with modified text parameters.

- **GOAL-1.7** [P0]: After mutation, the engine calls the adapter's `evaluate` method (GOAL-3.5) on the new candidate with the current iteration's minibatch (the same minibatch sampled in step 2, on which the parent was executed in GOAL-1.4). `evaluate` is used instead of `execute` because the Accept step only needs numeric scores for dominance comparison, not full execution traces. The resulting per-example scores are stored in the evaluation cache (GOAL-6.3).

- **GOAL-1.7a** [P0]: **Score alignment** — dominance comparison between any two candidates is performed only on the intersection of examples that both candidates have been evaluated on. The engine also evaluates all current front members on the current minibatch (using cached scores from GOAL-6.3 where available, only calling `evaluate` for uncached `(candidate_id, example_id)` pairs). This ensures that after each Accept step, the new candidate and all front members share at least the current minibatch as common ground.

- **GOAL-1.7b** [P0]: **Minimum shared examples** — dominance can only be established when two candidates share at least `min_shared_examples` evaluated examples (configurable, default: `minibatch_size` — see GOAL-7.1). If the intersection is smaller, the candidates are considered mutually non-dominating (neither can dominate the other). This is conservative by design: early iterations will grow the front as candidates lack sufficient shared evaluations, but the front naturally converges as the score matrix fills via re-evaluation backfill (GOAL-8.5).

- **GOAL-1.7c** [P0]: **Front re-evaluation** — the engine evaluates front members on the current minibatch (cache-aware) after each Accept step. **Re-evaluation cost budget:** front re-evaluation is capped at `max_re_eval_per_iteration` adapter `evaluate` calls per iteration (configurable, default: `pareto_max_size × minibatch_size / 2`). If the cap would be exceeded, only the front members with the stalest (oldest) scores on the current minibatch are re-evaluated, up to the cap. Remaining front members retain cached scores.

- **GOAL-1.7d** [P0]: **Acceptance rule** — the new candidate is accepted if it is non-dominated by any existing front member on their shared examples — i.e., no front member scores ≥ on every shared example and strictly > on at least one. After acceptance, any existing front members now dominated by the new candidate (on their shared examples, with sufficient intersection) are removed. **Edge case: mutual non-dominance** — if the new candidate and an existing front member each score higher on different examples (neither dominates the other), both remain on the front. This is the expected behavior and enables diversity. Front size is bounded by GOAL-2.4.

- **GOAL-1.8** [P0]: `GEPAEngine::run()` returns a `GEPAResult` containing: the final Pareto front (all non-dominated candidates), the single best candidate by average score, total iterations run, total wall-clock time, and the termination reason.

- **GOAL-1.9** [P1]: The engine supports resumption from a previously checkpointed `GEPAState`. Calling `GEPAEngine::run()` with a restored state continues optimization from where it left off, preserving the Pareto front, candidate history, and iteration count.

- **GOAL-1.10** [P2]: The engine supports a merge step: periodically (configurable interval), select two Pareto-optimal candidates that excel on different task subsets and ask the adapter to produce a merged candidate combining both strengths. The merged candidate is evaluated and accepted/rejected like any mutation.

## Cross-references

- GOAL-2.x (Pareto Front) — front operations used in Accept step
- GOAL-3.x (Adapter) — all LLM interactions
- GOAL-6.3 (Evaluation Cache) — score storage
- GOAL-7.x (Config) — stopping criteria, retry policy
- GOAL-8.x (Data Loading) — minibatch sampling

**Summary: 14 GOALs** (11 P0, 2 P1, 1 P2)
