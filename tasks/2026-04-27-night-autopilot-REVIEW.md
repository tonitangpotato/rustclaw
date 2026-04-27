# Review Findings — 2026-04-27 night-autopilot.md

> Reviewed 2026-04-27 by RustClaw (self-review against review-tasks skill).
> Total findings: 8 (3 🔴 Critical, 4 🟡 Important, 1 🟢 Minor)

---

## 🔴 FINDING-1 (Critical) — Wrong / fabricated design §-numbers in §B Retrieval table

**Where:** §B per-task pointer table, "Design §s to read" column.

**What's wrong:** I cited §-numbers without reading the design. Cross-checked against `.gid/features/v03-retrieval/design.md`, the truth is:

| My citation | Real location | Task affected |
|---|---|---|
| `§3.3, §3.5` for factual-bitemporal | §4.1 (factual) + §4.6 (bitemporal projection) | T3 |
| `§3.9` for graph-query-api | §6.2 (`GraphQuery` API) + §6.5 (Tier API) | T11 |
| `§3.10, §3.12` for typed-outcomes | §6.4 (typed outcomes) + §6.6 (novel-predicate) | T12 |
| `§3.11` for explain-trace | §6.3 (explain surface) | T14 |
| `§3.13` for fusion | §5 (entire Fusion & Ranking section, esp. §5.1, §5.2, §5.4) | T9 |
| `§4.5 step 5` for affective | §4.5 is the affective plan; "step 5" is unverified | T7 |
| `§5.4 + §3.1 + ...` for routing-accuracy | §5.4 = determinism only; routing accuracy is in §9 (Testing Strategy) | T16 |
| `§8.1 (11 surfaces)` for metrics | §8 = Observability (single-tier); "11 surfaces" / "8.1" subsection unverified | T15 |

**Impact:** Autopilot follows my pointers, can't find the section, hits STOP condition #2 ("section doesn't exist"). Worst case, it reads the WRONG section and implements wrong behavior.

**Fix:** Delete the §-numbers column from §B entirely. Replace with a single line: "Read `.gid-v03-context/v03-retrieval-build-plan.md` §5 for the file_path + design ref + GOAL-IDs of every task." That build plan IS the ground truth — already accurate, already detailed.

---

## 🔴 FINDING-2 (Critical) — Wrong task ID prefix in §C T1

**Where:** §C T1 row.

**What's wrong:**
- I wrote `task:migration-impl-error` — actual ID is `task:mig-impl-error`.
- AND that task is **already done** (status=done in graph DB).

**Impact:** First action autopilot takes is `gid_update_task(id="task:migration-impl-error", ...)` → fails with "node not found" → STOP, wasted iteration. Or if it auto-corrects, it tries to redo done work.

**Fix:** Remove §C T1 row entirely (it's done). Renumber so T1 starts at "progress" (currently T2). Verify all other §C task IDs exist via the SQL list above.

---

## 🔴 FINDING-3 (Critical) — Wrong path for migration CLI binary

**Where:** §C "Common conventions": `CLI binary: crates/engramai/src/bin/engramai_migrate.rs`.

**What's wrong:** A separate crate **already exists**: `/Users/potato/clawd/projects/engram/crates/engramai-migrate/`. The CLI binary is there, not in `engramai/src/bin/`.

**Impact:** Autopilot creates a new file in the wrong place, ends up with two parallel migrate CLIs.

**Fix:** Update §C convention to: `CLI binary lives in the existing crate /Users/potato/clawd/projects/engram/crates/engramai-migrate/ (already scaffolded). Read its current Cargo.toml + src/ before adding code.`

---

## 🟡 FINDING-4 (Important) — §B/§C/§D tables duplicate (and corrupt) build-plan content

**Where:** All three feature sections (§B, §C, §D) have per-task tables.

**What's wrong:** I copied/paraphrased build-plan content into my doc. This:
1. Doubles the maintenance burden (any plan revision now has 2 sources to update)
2. Already drifted in §B (FINDING-1)
3. Violates the doc's own stated philosophy ("pointer-style, not content-style")

**Impact:** Future drift. Autopilot reads my (potentially stale) version and skips the canonical build plan.

**Fix:** Replace each §B/§C/§D body with:
```
**Read the build plan**: `<full-path>/v03-<feature>-build-plan.md`
That file lists every task with: ID, title, file_path, design ref §s, GOAL/GUARDs, dependencies. It's the canonical breakdown — do not infer from anywhere else.

**Common conventions** (apply to every task in this feature):
- {only the conventions, not the per-task table}
```
This drops ~80 lines and removes the drift surface entirely.

---

## 🟡 FINDING-5 (Important) — A.1 design §s are vague + partially wrong

**Where:** §A.1 row "Design §s".

**What's wrong:** I wrote *"search design.md for: Memory::reextract, ..., §6.5"*. Reality (verified by grep):
- `reextract` / `reextract_failed`: **§6.2** (line 756) — not "§6.5"
- `compile_knowledge` / `list_knowledge_topics`: **§6.2** (line 724-779) — also detailed in §5bis
- `ingest_with_stats`: **§6.4** (line 833 onwards) — not in my list AT ALL
- `resolve_for_backfill`: **§6.5** ✅ correct
- `ResolutionStats`: **§6.4** ✅ implied

**Impact:** Autopilot greps for "§6.5" and finds only `resolve_for_backfill`, missing the other 5 method specs.

**Fix:** Replace with explicit § list:
```
Design §s:
- §6.2 — Memory::reextract / reextract_failed / compile_knowledge / list_knowledge_topics
- §6.4 — Memory::ingest_with_stats + ResolutionStats public contract
- §6.5 — Memory::resolve_for_backfill (migration handoff)
- §5bis — Knowledge Compiler context (for understanding compile_knowledge)
```

---

## 🟡 FINDING-6 (Important) — Missing dependency: §C T9 needs §A.1 done; §B T6 needs §A.2 done

**Where:** Execution layer diagram (§3) says §B and §C can run "parallel-safe after §A".

**What's wrong:** Partially true but I noted the dependencies inline (good) without flagging them in the layer diagram (bad). Specifically:
- §C T9 (`task:mig-impl-backfill-perrecord`) needs `Memory::resolve_for_backfill` from §A.1 — autopilot might start §C T9 before §A.1 done.
- §B T6 (`task:retr-impl-abstract-l5`) needs §A.2's KnowledgeTopic store. I noted this inline but the layer diagram doesn't show it.
- §D T12 (cost driver) and §D T13 (test-preservation) have hard cross-section dependencies (on §A.1 and §C respectively) that the doc mentions inline only.

**Impact:** Autopilot picks "ready frontier" via `gid_tasks` which may not encode these cross-section deps. It starts a blocked task, hits a missing-symbol compile error, retries 2×, then STOPs.

**Fix:** Add a "Cross-section dependency table" right under §3 layer diagram listing: §A.1 → §C T9, §A.2 → §B T6, §A.1 → §D T12, §C → §D T13. Tell autopilot: "If you start any of these downstream tasks before the upstream is `done`, STOP."

---

## 🟡 FINDING-7 (Important) — `gid_query_deps` is the right tool, not `gid_tasks --status=todo`

**Where:** §3 "Order of attack" + §E "Reminders".

**What's wrong:** I told autopilot to use `gid_tasks --status=todo` to find ready tasks. But `--status=todo` returns ALL todo tasks, including blocked ones. The correct frontier query is "todo tasks with all deps done", which `gid_tasks` (built-in) actually does report as a "ready" subset — but I should be explicit about reading that subset, not the full todo list.

**Impact:** Autopilot picks a blocked task off the todo list, wastes time discovering it's blocked.

**Fix:** Replace with: "Use `gid_tasks(graph_path=<v03 db>)` and look at the **ready/unblocked** subset in its output (already filtered by built-in dep analysis). For any task you want to verify is truly ready, run `gid_query_deps(id=<task_id>)` and confirm all returned deps have status=done."

---

## 🟢 FINDING-8 (Minor) — Self-contradictory line count footer

**Where:** Last line: `*Total: 4 sections, 53 tasks pointed at, ~286 lines.*`

**What's wrong:** Doc is 348 lines, not 286. (Footer was written before §E was added.)

**Impact:** None functional, just sloppy.

**Fix:** Update or delete.

---

## Summary

The doc has the **right shape** (pointer-style, dependency layers, stop conditions, status format) but I **cited specifics without verifying** in §A.1 and §B-§D tables. The root fix is FINDING-4: collapse the §B/§C/§D tables into single pointers to the build plans, which are already the canonical, accurate breakdowns.

If I apply FINDING-1 through FINDING-7, the doc will be ~200 lines instead of 348, and every pointer will be correct. That's the doc autopilot can actually trust.

**Recommendation:** Don't ship as-is. Apply FINDING-1, 2, 3, 4 minimum (they're the bugs that will cause STOP-and-wakeup). FINDING-5, 6, 7 polish. FINDING-8 cosmetic.
