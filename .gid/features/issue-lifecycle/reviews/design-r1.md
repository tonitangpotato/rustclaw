# Review: Issue Full-Cycle Management Design (R1)

**Document**: `.gid/features/issue-lifecycle/design.md`
**Requirements**: `.gid/features/issue-lifecycle/requirements.md`
**Reviewer**: RustClaw
**Date**: 2026-04-08
**Depth**: Full (Phase 0–7)

---

## Phase 0: Document Size Check

✅ **Check 0**: 7 components (§2.1–§2.7) — within the ≤8 limit. No split needed.

## Phase 1: Structural Completeness

✅ **Check 1**: All types defined — issues-index.md format, projects.yml schema, ritual YAML, skill trigger patterns all have concrete definitions.

✅ **Check 2**: All references resolve — §2.4 references §2.5 (close-issue skill), §2.6 references §2.1 (projects.yml), etc.

✅ **Check 3**: No dead definitions — every component is referenced in at least one data flow (§3).

### 🔴 FINDING-1: [Check 4] Inconsistent naming — "issues-index.md" vs "ISSUES.md" ✅ Applied
The design says storage is `{project}/.gid/docs/issues-index.md` (matching requirements GOAL-1), but the existing `project-issues` skill creates files at `{project_root}/ISSUES.md` (root level). The migration (§2.7) handles converting old files, but the **existing `project-issues` skill body still writes to root-level `ISSUES.md`** — v3.0 needs to explicitly change the write path, and the design doesn't call this out as a change item in §2.3.

**Suggested fix**: In §2.3, add explicit note: "v3.0 changes the write path from `{project_root}/ISSUES.md` to `{project_root}/.gid/docs/issues-index.md`."

## Phase 2: Logic Correctness

### 🔴 FINDING-2: [Check 5] Ritual template mechanism doesn't exist ✅ Applied
The design assumes a `issue-fix.yml` template file in `.gid/rituals/` and that the ritual runner supports custom YAML templates with arbitrary phases. **This is incorrect.**

Verified in code:
- `gid-core/src/ritual/state_machine.rs`: `RitualPhase` is a hardcoded enum (Idle → Triaging → WritingRequirements → Designing → Reviewing → Planning → Graphing → Implementing → Verifying → Done). No custom phases.
- `ritual_runner.rs`: `runner.start(task)` enters the fixed state machine. No template loading. `.gid/rituals/` contains JSON state files (e.g. `r-04f359.json`), not YAML templates.
- `start_ritual` tool: takes only `task` and `workspace` params, no `--template`.

The entire §2.4 (issue-fix ritual template) is designing against a non-existent API. The `PhaseKind::Skill` concept doesn't exist. The `extends` problem from FINDING-3 in requirements was fixed, but replaced with another fiction.

**Impact**: This is the core architectural assumption of the design. fix → verify → close as a custom ritual pipeline cannot work without either:
- **(A) Extending gid-core state machine** to support custom phase sequences (significant Rust changes — contradicts "no Rust changes" design principle)
- **(B) Implementing issue-fix as an agent skill workflow** (no ritual engine, just skill instructions: "1. fix code, 2. run tests, 3. update issue status")
- **(C) Using `resume_from_phase`** to jump straight to `Implementing` phase with issue context, then handle close as a post-completion hook

**Suggested fix**: Replace §2.4 entirely. Recommend option **(B)**: make `issue-fix` a comprehensive skill (like `project-issues` but for fixing). The skill instructs the agent to: read issue → implement fix → commit → run verify_command → if pass, call close-issue skill steps → if fail, notify. No ritual engine needed. The ritual engine is overkill for "fix one bug" — it's designed for multi-phase feature development.

### 🔴 FINDING-3: [Check 5] P0 auto-trigger via heartbeat → Telegram message is unreliable ✅ Applied
The design (§2.6) says heartbeat sends a Telegram message → triggers normal session → agent calls start_ritual. Problems:

1. **Heartbeat responses already go to Telegram** (heartbeat channel routing). But the agent in that heartbeat session IS the normal agent with full context. There's no "simplified heartbeat session" — it's the same agent, same tools, same system prompt. The design's rationale for indirect triggering is based on a false premise.
2. **Message → new session** doesn't guarantee the new session will see the P0 context. The new session starts fresh (no memory of what heartbeat found). It would need to re-scan issues-index.md to rediscover the P0.
3. **Race condition**: What if heartbeat fires again before the fix session completes? It would detect the same P0 and send another "fix" message, potentially starting a duplicate ritual.

**Suggested fix**: Heartbeat session directly executes the fix workflow (option B from FINDING-2). The heartbeat session has full agent capabilities. Add a guard: before starting fix, check if issue status is already `in_progress` → skip. Update issue to `in_progress` atomically before starting fix.

✅ **Check 6**: Data flow completeness — §3 traces all data flows correctly (given the ritual assumption is fixed).

### 🟡 FINDING-4: [Check 7] No error handling for issues-index.md parse failures ✅ Applied
§2.3 Step B (dashboard scan) uses regex to parse issues-index.md. What if the file is malformed? A bad manual edit could break the regex. The design doesn't specify fallback behavior.

**Suggested fix**: Add: "If regex parse fails for a project's issues-index.md, skip that project in dashboard output and log a warning: 'Failed to parse issues for {project}, skipping.'"

## Phase 3: Type Safety & Edge Cases

### 🟡 FINDING-5: [Check 8] ISS number extraction regex is fragile ✅ Applied
§4 shows: `grep -oP 'ISS-(\d+)' ... | sort -t- -k2 -n | tail -1`

`-oP` (PCRE) isn't available on macOS default grep. macOS uses BSD grep. Need `grep -oE 'ISS-[0-9]+'` or use `ggrep` from homebrew.

**Suggested fix**: Use `grep -oE 'ISS-[0-9]+'` (POSIX ERE, works on both Linux and macOS) or note that the agent does this in LLM reasoning (reading the file and extracting the number), not literally running grep.

✅ **Check 9-12**: No integer overflow, Option handling, match exhaustiveness, or ordering sensitivity concerns (this is a skill-based design, not compiled code).

## Phase 4: Architecture Consistency

✅ **Check 13**: Separation of concerns — skills handle logic, files handle state. Clean.

✅ **Check 14**: Coupling — components communicate through files, not shared state.

### 🟡 FINDING-6: [Check 15] verify_command in two places ✅ Applied
`projects.yml` (§2.1) defines `verify_command` per project. But `.gid/config.yml` already stores `verify_command` (used by ritual_runner.rs line 1676-1681). Now there are two sources of truth for the same value.

**Suggested fix**: `projects.yml` should NOT duplicate verify_command. The fix workflow should read verify_command from the target project's `.gid/config.yml` (existing mechanism). If it doesn't exist there, fall back to language-default detection (also existing). Remove `verify_command` from projects.yml schema — keep it as just path + display_name.

✅ **Check 16**: API surface is minimal — two skills + one config file.

## Phase 5: Design Doc Quality

✅ **Check 17**: Goals and non-goals are in the requirements doc and referenced.

✅ **Check 18**: Trade-offs documented — §6 has 3 well-reasoned trade-offs.

### 🟢 FINDING-7: [Check 19] No observability/debugging section ✅ Applied
How do you debug a failed issue fix? The design covers failure notifications but not: where are fix attempt logs stored? Can you see the LLM's reasoning for the fix? Is there a "fix history" per issue?

**Suggested fix**: Add a note: "Fix attempts are logged in `memory/YYYY-MM-DD.md` (daily log). Failed attempts include the error context. For detailed LLM reasoning, check the Telegram notification which includes the full fix report."

✅ **Check 20**: Appropriate abstraction level — concrete enough to implement.

## Phase 6: Implementability

✅ **Check 21**: No ambiguous prose found.

✅ **Check 22**: All helpers referenced are defined.

✅ **Check 23**: No unverified dependency assumptions (after FINDING-2 is fixed).

✅ **Check 24**: Migration path is clear (§2.7).

✅ **Check 25**: Testability — verification criteria exist in requirements for each GOAL.

## Phase 7: Existing Code Alignment

### 🔴 FINDING-2 covers this — the ritual template mechanism is the major code misalignment.

### 🟡 FINDING-8: [Check 27] close-issue skill has no triggers but skill system requires them ✅ Applied
§2.5 says close-issue skill has empty trigger patterns. Verified in `src/skills.rs` — skills are loaded by scanning `skills/*/SKILL.md` and matched via triggers. A skill with empty triggers will **never be matched** by the skill engine. The design says "only called by issue-fix ritual's close phase" — but if we switch to option B (agent skill workflow), the agent would call close-issue steps inline, not as a separately-triggered skill.

**Suggested fix**: Either (a) make close-issue a section within the issue-fix skill (not a separate skill), or (b) give it a trigger keyword like `"close-issue"` that the agent can invoke by including it in its reasoning. Option (a) is cleaner — fewer files, less indirection.

### 🟢 FINDING-9: [Check 28] §2.3 trigger regex needs escaping review ✅ Applied
`"ISS-\\d+ (closed|wontfix|blocked|P[012])"` — in YAML, the double backslash `\\d` is fine, but the skill trigger system uses regex or keyword matching. Need to verify that `project-issues` skill's trigger patterns support full regex or just substring matching.

**Suggested fix**: Verify in SKILL.md trigger matching code. If it's substring-only, simplify patterns to keywords.

---

## Summary

### 🔴 Critical (3)
| ID | Check | Issue |
|---|---|---|
| FINDING-1 | #4 | Write path inconsistency — skill still writes to root ISSUES.md |
| FINDING-2 | #5 | **Ritual template mechanism doesn't exist** — core architecture invalid |
| FINDING-3 | #5 | P0 auto-trigger via message relay is unreliable + based on false premise |

### 🟡 Important (4)
| ID | Check | Issue |
|---|---|---|
| FINDING-4 | #7 | No error handling for malformed issues-index.md |
| FINDING-5 | #8 | grep -oP not available on macOS |
| FINDING-6 | #15 | verify_command duplicated in projects.yml and .gid/config.yml |
| FINDING-8 | #27 | close-issue skill with empty triggers won't be matched |

### 🟢 Minor (2)
| ID | Check | Issue |
|---|---|---|
| FINDING-7 | #19 | No observability/debugging section |
| FINDING-9 | #28 | Trigger regex may not work with skill matching system |

### ✅ Passed Checks (20/29)
Checks 0, 1, 2, 3, 6, 9, 10, 11, 12, 13, 14, 16, 17, 18, 20, 21, 22, 24, 25 — all pass.

### Recommendation
**Needs major revision on FINDING-2.** The ritual template assumption is foundational — once it's replaced with a skill-based workflow (option B), FINDING-3 and FINDING-8 also resolve naturally. The fix is conceptually simple: `issue-fix` becomes a comprehensive skill that instructs the agent step-by-step, not a ritual template. But it changes §2.4, §2.5, §2.6, and §3.2 significantly.
