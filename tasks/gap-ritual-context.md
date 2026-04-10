# Gap Analysis: ritual-context-integration

**Design doc**: `.gid/features/ritual-context-integration/design.md`
**Date**: 2026-04-08

---

## Summary

The core architecture (enrich in executor, not state machine) is implemented correctly. The main gaps are in **review depth scaling** (GOAL-5) — the review skill name matching is broken, the `ReviewConfig`/`CheckSet` types don't exist, and the light-review check injection is missing. Several specified tests are also absent.

---

## GOAL-by-GOAL Status

| Goal | Description | Status |
|------|-------------|--------|
| GOAL-1 | Implementing phase carries assembled context | ✅ Implemented |
| GOAL-2 | Other phases continue using `state.task` | ✅ Implemented |
| GOAL-3 | Best-effort fallback to `state.task` | ✅ Implemented |
| GOAL-4 | `TaskContext::render_prompt()` produces prompt-friendly string | ✅ Implemented |
| GOAL-5 | Review depth scales with triage size | ⚠️ Partial — see §3 |

---

## §1 — Context Enrichment (§4.1–4.7 of design)

### §4.1 Strategy: Enrich in Executor, Not State Machine
✅ **Implemented.** State machine continues to emit `RunSkill { context: state.task.clone() }`. Enrichment happens in `V2Executor`.

### §4.2 `enrich_implement_context()`
✅ **Implemented** (line 870 of `v2_executor.rs`).
- Signature matches design: `fn enrich_implement_context(&self, raw_context: &str, state: &RitualState) -> String`
- Calls `build_graph_context()` and falls back to `raw_context` if None.

### §4.3 Task Node Discovery via `build_graph_context()`
✅ **Implemented** (line 828 of `v2_executor.rs`).
- Filters `node_type == "task"` and `status != Done` — matches design §5.2.1 spec.
- Calls `assemble_task_context()` per task node.
- Joins with `"\n\n---\n\n"` separator — matches design.
- Uses `tracing::warn` for parse errors and `tracing::debug` for missing graph — matches intent (minor: design said `warn` for missing graph, impl uses `debug`, which is arguably better).

### §4.4 `TaskContext::render_prompt()`
✅ **Implemented** (line 74 of `harness/types.rs`).
- Matches design spec exactly: sections for Task, Design Reference, Requirements (GOALs), Guards, Completed Dependencies.
- Same heading format, same conditional logic for empty fields.

### §4.5 Integration Point in `run_skill()`
✅ **Implemented** (line 296 of `v2_executor.rs`).
- `if name == "implement"` → enrich; else → pass through. Matches design.

### §4.6 Threading State to `run_skill()`
✅ **Implemented.** Option C from design (pass `state` through `execute()`).
- `execute()` at line 104: `self.run_skill(name, context, state).await` — state passed.
- `run_skill` signature at line 271: `async fn run_skill(&self, name: &str, context: &str, state: &RitualState)`.

### §4.7 Handling Verify-Fix Cycles
✅ **Implemented.** `enrich_implement_context()` prepends graph context before `raw_context`, so error messages (from fix cycles) are preserved in the raw portion. Format matches design: `"{ctx}\n\n## Original Task\n{raw_context}"`.

**Design context before error message** (design note on ordering): ✅ Correct — graph context appears first.

---

## §2 — Helper Functions (§5.2)

### `safe_truncate()`
✅ **Implemented** (line 809 of `v2_executor.rs`).
- Matches design §5.2.2 spec exactly.
- Also fixes the pre-existing UTF-8 issue in `run_planning` — now uses `Self::safe_truncate(&design_content, 15000)` instead of raw `&design_content[..15000]`. ✅

### `resolve_gid_root()`
✅ **Implemented** (line 820 of `v2_executor.rs`).
- Uses `state.target_root` if Some, else `self.config.project_root`. Matches design §4.2 step 1.

---

## §3 — Review Depth Scaling (§9 of design)

### `review_config_for_triage_size()`
⚠️ **Partial.** The function exists (line 591) but deviates significantly from design:

| Aspect | Design Spec | Implementation | Status |
|--------|------------|----------------|--------|
| Return type | `ReviewConfig { model, max_iterations, checks }` | `(String, usize)` — tuple, no checks | ⚠️ Simplified |
| `small` handling | `unreachable!()` | Returns `("sonnet", 30)` | ⚠️ Deviation (safer but different) |
| `medium` model | `"claude-sonnet-4-5-20250929"` (Sonnet) | `self.config.skill_model` (whatever default is) | ⚠️ Deviation — doesn't force Sonnet |
| `medium` iterations | 30 | 50 | ⚠️ Deviation |
| `large` model | `"claude-opus-4-6"` (Opus) | `self.config.skill_model` (default) | ⚠️ Deviation — doesn't force Opus |
| `large` iterations | 55 | 100 | ⚠️ Deviation |
| `CheckSet::Light` / `CheckSet::Full` | Separate enum controlling 10 vs 28 checks | Not implemented | ❌ Missing |

### Light Review Check Injection (§9.4)
❌ **Missing.** Design specifies injecting `"REVIEW SCOPE: LIGHT\nRun ONLY checks #1, #2, #5, #6, #7, #8, #11, #13, #21, #27"` for medium tasks. Implementation only injects a generic `[REVIEW_DEPTH: standard]` hint string with no specific check numbers.

### `ReviewConfig` struct
❌ **Missing.** Design specifies a `ReviewConfig { model, max_iterations, checks: CheckSet }` struct. Implementation uses a `(String, usize)` tuple.

### `CheckSet` enum
❌ **Missing.** Design specifies `CheckSet::Light` (10 checks) and `CheckSet::Full` (28 checks). Not implemented.

### Review Skill Name Matching — **BUG**
❌ **Bug.** The state machine emits skill names `"review-design"`, `"review-requirements"`, and `"review-tasks"`. But the executor matches against `name == "review"` (lines 311 and 318 of `v2_executor.rs`), which will **never** match. This means:
- Review model/iteration config is **never applied** — reviews always use default model + 100 iterations.
- Review depth hint is **never injected** into the prompt.

The design (§9.4) correctly specifies matching `name == "review-design" || name == "review-requirements" || name == "review-tasks"`.

---

## §4 — Changes Required Checklist (§5 of design)

### §5.1 `harness/types.rs`
| Item | Status |
|------|--------|
| `TaskContext::render_prompt()` | ✅ Implemented |

### §5.2 `ritual/v2_executor.rs`
| Item | Status |
|------|--------|
| Change `run_skill` signature to add `state: &RitualState` | ✅ Implemented |
| Update `execute()` to pass `state` | ✅ Implemented |
| Update `run_harness()` to pass `state` | ✅ Implemented (line 484: `self.run_skill("implement", &context, state).await`) |
| Add `enrich_implement_context()` | ✅ Implemented |
| Add `build_graph_context()` | ✅ Implemented |
| Add `safe_truncate()` | ✅ Implemented |
| Use `enriched_context` in `run_skill()` for "implement" | ✅ Implemented |
| Add `review_config_for_triage_size()` | ⚠️ Partial — exists but deviates (see §3) |
| Inject check scope into review prompt (light=10, full=28) | ❌ Missing |
| Select review model (Sonnet for medium, Opus for large) | ❌ Missing — always uses default model |

### §5.3 `harness/context.rs`
| Item | Status |
|------|--------|
| No changes required | ✅ Correct — no changes made |

### §5.4 `harness/mod.rs`
| Item | Status |
|------|--------|
| `assemble_task_context` is `pub` and accessible | ✅ Confirmed — `pub use context::assemble_task_context;` in mod.rs, imported in v2_executor.rs |

---

## §5 — Error Handling (§6 of design)

| Scenario | Design | Implementation | Status |
|----------|--------|----------------|--------|
| Graph file missing | Return raw context | `read_to_string().ok()?` → returns None → falls back | ✅ |
| Graph parse error | Log warning, return raw context | `tracing::warn!()` + `.ok()?` → returns None → falls back | ✅ |
| No task nodes | Return raw context | `if task_ids.is_empty() { return None; }` → falls back | ✅ |
| `assemble_task_context` error | Log warning, skip that task | `tracing::warn!()` + `.ok()` in filter_map | ✅ |
| All non-fatal | Enrichment is additive | Correct — never panics, always falls back | ✅ |

---

## §6 — Testing Gap

### Design §7.1 — Unit Tests

| # | Test | Design Spec | Implementation | Status |
|---|------|-------------|----------------|--------|
| 1 | `test_enrich_with_graph_context` | Mock graph with task+feature+design, verify enriched output | Not found | ❌ Missing |
| 2 | `test_enrich_no_graph` | No graph.yml, verify fallback | Not found | ❌ Missing |
| 3 | `test_enrich_no_task_nodes` | Graph with only code nodes, verify fallback | Not found | ❌ Missing |
| 4 | `test_enrich_with_error_context` | Fix cycle includes both error and design excerpt | Not found | ❌ Missing |
| 5 | `test_render_prompt` | Full TaskContext renders all sections | `test_render_prompt_full` (line 1066) | ✅ Implemented |
| 6 | `test_render_prompt_partial` | Only some fields populated | `test_render_prompt_partial` (line 1097) | ✅ Implemented |

### Design §7.2 — Integration Tests

| # | Test | Status |
|---|------|--------|
| 1 | Full ritual flow with `.gid/graph.yml` + design docs, mock LLM, verify enriched prompt | ❌ Missing |

### Design §9.6 — Review Depth Tests

| # | Test | Status |
|---|------|--------|
| 1 | `test_review_config_small` — verify small returns unreachable | ❌ Missing |
| 2 | `test_review_config_medium` — verify Sonnet + 30 iter + Light | ❌ Missing |
| 3 | `test_review_config_large` — verify Opus + 55 iter + Full | ❌ Missing |
| 4 | `test_light_review_prompt_injection` — verify 10 check numbers | ❌ Missing |
| 5 | `test_review_depth_with_state` — integration with triage_size | ❌ Missing |

### Existing Tests (not in design, but present)

- `test_safe_truncate_ascii` ✅
- `test_safe_truncate_utf8` ✅
- 4 `test_extract_json_*` ✅
- 3 `test_parse_planning_*` ✅

---

## §7 — Deviations Summary

### Critical (likely bugs)

1. **Review skill name mismatch**: Executor checks `name == "review"` but state machine emits `"review-design"`, `"review-requirements"`, `"review-tasks"`. Review depth scaling is effectively **dead code** — never triggers.

### Significant (design intent not met)

2. **No `CheckSet` enum or light review check injection**: The 10-core-checks-for-medium feature (design §9.3–9.4) is not implemented. Reviews always run with a generic depth hint instead of specific check numbers.

3. **Review model/iterations don't match design tiers**: Medium should use Sonnet/30iter, Large should use Opus/55iter. Implementation uses default model for both and 50/100 iterations respectively.

4. **4 of 6 specified unit tests missing**: The `enrich_*` and `enrich_no_*` test family is entirely absent.

5. **Integration test missing**: No full ritual flow test with graph + design docs.

### Minor

6. **`tracing::debug` vs `tracing::warn`** for missing graph.yml: Implementation uses `debug`, design says implicitly `warn`. This is arguably better (missing graph is expected, not a warning).

7. **`review_config_for_triage_size` returns tuple, not `ReviewConfig` struct**: Simplified but loses the `checks` field entirely.

---

## §8 — Recommendations (priority order)

1. **Fix review skill name matching** — change `name == "review"` to `name.starts_with("review-")` or match all three variants. This is a functional bug.

2. **Implement `CheckSet` and light review injection** — add the 10-check-number prompt for medium tasks per §9.4.

3. **Fix review model selection** — medium → Sonnet, large → Opus, per design §9.2 table.

4. **Add missing unit tests** — especially `test_enrich_with_graph_context`, `test_enrich_no_graph`, `test_enrich_no_task_nodes`, `test_enrich_with_error_context`. These validate the core feature.

5. **Add review depth tests** — `test_review_config_medium`, `test_review_config_large`, `test_light_review_prompt_injection`.
