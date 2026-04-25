---
name: review-design
description: Systematically review design documents for bugs, inconsistencies, and missing cases
version: "1.1.0"
author: potato
triggers:
  patterns:
    - "review design"
    - "review设计"
    - "审核设计"
    - "check design"
    - "检查design"
  regex:
    - "(?i)review.*design"
    - "(?i)design.*review"
tags:
  - development
  - quality
priority: 55
always_load: false
recommended_iterations: 50
max_body_size: 8192
subagent_preamble: |
  You are a sub-agent reviewing a design document. Key rules:
  - The design document is ALREADY pre-loaded in your context. Do NOT call read_file to re-read it.
  - Read the pre-loaded content carefully, then run review checks against it.
  - If you need to check existing source code (Phase 7), those reads are legitimate — but only read files directly named in the design.
  - MANDATORY: write the review INCREMENTALLY. First write_file the skeleton (~40 lines), then append each finding as you discover it via edit_file. Never accumulate findings and write them all at once. See 'Incremental Output Protocol' section in the skill body.
  - Budget: max 30% reading (source code for verification), 70% analysis and incremental writing.
---
# SKILL: Design Document Reviewer

> One-pass systematic review that catches bugs before implementation. No "looks good" — find problems or prove there are none.

## Purpose

Design docs reviewed by LLMs tend to need 5-6 iterative rounds because each pass only catches surface issues. This skill enforces a structured checklist that catches deep bugs in a single pass.

## When to Use

- After writing or updating any design document (DESIGN.md, DESIGN-*.md)
- Before starting implementation of a design
- When a design has been revised and needs re-verification

## Sub-Agent Configuration

This skill is iteration-heavy — reading the full design, running 35 checks, and verifying code references all consume tool calls. When spawning a sub-agent for this skill:

- **full review**: `max_iterations: 60` minimum (35 checks + code verification = 50-60 tool calls)
- **standard review**: `max_iterations: 40` minimum
- **quick review**: `max_iterations: 25` minimum

Under-provisioned iterations are the #1 cause of incomplete reviews — the agent runs out mid-Phase-7 and never writes the review file.

## Review Process

### Review Depth (Triage-Driven)

Check the beginning of your prompt for a `[REVIEW_DEPTH: quick|standard|full]` directive. This is injected by the ritual system based on triage size.

| Depth | Triage Size | Phases to Run | Checks |
|---|---|---|---|
| **quick** | small | Phase 0 + Phase 1 + Phase 4 + Phase 8 | 0-4, 13-16, 30-32 (12 checks) |
| **standard** | medium | Phase 0-5 + Phase 8 | 0-20, 30-35 (27 checks) |
| **full** | large (default) | Phase 0-8 | All 36 checks (0-35) |

**If no `[REVIEW_DEPTH]` directive is present, default to `full`.**

For `quick` reviews: skip logic correctness, type safety edge cases, doc quality, implementability, and full code alignment checks. Focus on structure, architecture, and the core integrity checks (debt/shortcuts/conflicts #30-32) — the goal is fast validation that the design is internally consistent and not introducing hidden debt.

For `standard` reviews: skip Phase 6 (Implementability) and Phase 7 (Existing Code Alignment). Phase 8 (Engineering Integrity) is always run — technical debt and shortcut detection are non-negotiable, even for incremental changes.

---

Read the design document completely, then run the checks applicable to your review depth. Do not stop after finding the first issue.

### Phase 0: Document Size Check

0. **Document size** — Count total components (§3.x sections). If >8 components in a single document → **Critical finding**: document must be split into feature-level design docs (see draft-design skill for structure). A single design doc should have ≤8 components. Cross-cutting concerns stay in the master doc; per-feature components are split by feature.

### Phase 1: Structural Completeness

1. **Every type fully defined?** — All structs, enums, traits mentioned in prose must have complete field/variant definitions somewhere in the doc. No "TBD" or implied fields.
2. **Every reference resolves?** — If the doc says "see §3.2" or "uses FooBar", verify that section/type actually exists.
3. **No dead definitions** — Every type/event/state/action defined must be used somewhere. If defined but never referenced in any logic → flag as dead code.
4. **Consistent naming** — Same concept must use the same name everywhere. Check for: singular vs plural, snake_case vs CamelCase inconsistency, abbreviated vs full names.

### Phase 2: Logic Correctness

5. **State machine invariants** (if applicable):
   - Every (state, event) pair: does the transition produce a terminal state OR exactly the expected number of side effects?
   - No unreachable states (every non-initial state has at least one incoming transition)
   - No deadlocks (every non-terminal state has at least one outgoing transition that produces forward progress)
   - Guard conditions are exhaustive (for any state+event, exactly one branch matches regardless of runtime values)
   - Self-transitions: verify they don't create infinite loops (must have a bounded retry counter or escalation)
   - **Trace concrete paths**: happy path (start → done), every failure path, every user-interrupt path. Write out each step explicitly.

6. **Data flow completeness**:
   - Every field read must be written somewhere upstream
   - Every field written must be read somewhere downstream (or explicitly marked as "for debugging")
   - No stale data — if state mutates, consumers of that state see the updated version

7. **Error handling completeness**:
   - Every operation that can fail has an explicit failure path
   - Failure paths don't silently swallow errors (log or propagate)
   - Retry logic has bounded retries (no unbounded retry loops)

### Phase 3: Type Safety & Edge Cases

8. **String operations** — Any `&s[..n]` or substring slicing on user/LLM-generated text? Flag as UTF-8 unsafe. Must use `char_indices()` or `.chars().take(n)`.
9. **Integer overflow** — Any `retries + 1` without bounds check? Counter increments without max?
10. **Option/None handling** — Any `.unwrap()` on optional values without guaranteed Some? Must have fallback.
11. **Match exhaustiveness** — Catch-all `_` branches: do they handle all remaining cases correctly? Would adding a new enum variant silently fall into the catch-all?
12. **Ordering sensitivity** — For match/if-else chains with guards: would reordering change behavior? Are guard conditions mutually exclusive or does order matter (and if so, is the order correct)?

### Phase 4: Architecture Consistency

13. **Separation of concerns** — Pure logic (no IO) stays pure. Side effects clearly isolated in executor/handler layer. No "this function is pure except it also reads a file".
14. **Coupling** — Events/actions carry only what they observed, not derived state. If an event carries a value that's already in state → coupling smell (transition should read from state).
15. **Configuration vs hardcoding** — Language-specific values, paths, commands, thresholds: are they configurable or hardcoded? Hardcoded values that vary by project → must be configurable.
16. **API surface** — Public types/functions: are they the minimal necessary set? Internal implementation details leaking into public API?

### Phase 5: Design Doc Quality (from Google's design doc practice)

17. **Goals and non-goals explicit?** — Are goals clearly stated? More importantly, are there explicit *non-goals* (things that could be goals but are deliberately excluded)? Non-goals prevent scope creep and clarify trade-offs.
18. **Trade-offs documented?** — For every design decision, are the alternatives considered and the trade-offs explained? A design doc without trade-offs is an implementation manual, not a design doc.
19. **Cross-cutting concerns** — Security, observability, error visibility, performance implications — are they addressed or explicitly marked as out of scope?
20. **Appropriate abstraction level?** — Is the doc at the right level? Too much code → implementation manual. Too vague → two engineers would implement differently. Pseudocode should clarify design intent, not specify syntax.

### Phase 6: Implementability

21. **Ambiguous prose** — Any section where two competent engineers would implement differently? Flag and suggest concrete specification.
22. **Missing helpers** — Functions referenced in pseudocode but never defined (e.g., `phase.next()` used but not specified)?
23. **Dependency assumptions** — Does the design assume a library/API exists without verifying? External dependencies should be named explicitly.
24. **Migration path** — If this replaces existing code, is the replacement scope clear? What's kept, what's deleted, what's adapted?
25. **Testability** — Can the core logic be unit-tested in isolation? Is the design structured so that tests don't need complex setup or mocking? Pure functions > stateful objects for testability.

### Phase 7: Existing Code Alignment & Ground Truth

26. **Does similar functionality already exist in the codebase?** — Search for existing implementations before designing new ones. Duplicate solutions are a maintenance burden.
27. **API compatibility** — Does the new design break existing callers? If yes, is the migration plan documented?
28. **Feature flag / gradual rollout** — Can the new design be introduced behind a feature flag? Is there a rollback plan if the implementation doesn't work?
29. **Ground truth verification** — For every reference to existing code (function calls, API usage, behavior assumptions), **read the actual source code** and verify:
    - Does the function/struct actually exist? With the assumed signature?
    - Does the function actually do what the design claims? (Read implementation, not just name)
    - Are effort estimates grounded? ("~20 lines" — is the target file 50 lines or 2000?)
    - Does the design say "function X handles Y" when it actually doesn't? (e.g., "merge handles dedup" when merge just overwrites)
    - **Use `search_files` and `read_file` to verify.** Don't trust the design author's memory of the codebase.
    - Every unverified assumption about existing code → 🔴 Critical finding. This is the #1 source of multi-round review cycles.

### Phase 8: Engineering Integrity (Technical Debt, Shortcuts, Conflicts)

This phase enforces potato's engineering philosophy: **no technical debt, root fix not patch, no shortcuts, no conflicts with existing architecture.** A design that introduces debt — even intentionally — must surface it explicitly so the trade-off is visible, not hidden.

30. **Technical debt introduction** — Does the design introduce code/structure the author would NOT want to maintain long-term? Look for:
    - "We'll clean this up later" / "Temporary solution" / "Good enough for now" phrases
    - Workarounds that paper over a deeper issue instead of fixing it
    - Hardcoded values marked as "TODO: make configurable"
    - Duplicated logic justified as "we'll dedupe in a future refactor"
    - Any accepted debt → **must be documented explicitly** with: (a) what the debt is, (b) why it's accepted now, (c) concrete trigger for paying it back (not "someday"). If debt is introduced without this framing → 🔴 Critical.

31. **Shortcut detection (patch vs root fix)** — Is the design solving the *symptom* or the *root cause*? Red flags:
    - "Add a check to prevent X from happening" when the real question is *why does X happen*
    - "Wrap this in try/catch" without understanding what can actually fail
    - "Add a retry" as a fix for an intermittent bug whose root cause is unidentified
    - "Special-case this one scenario" when a general solution exists at the same cost
    - New config flag to disable a broken feature instead of fixing the feature
    - Each shortcut → 🟡 Important. Ask: "What's the root cause? Is this design treating the symptom?"

32. **Conflicts with existing architecture** — Does the design contradict patterns/conventions already established in the codebase?
    - Uses a different error-handling style than the rest of the project (e.g., `unwrap()` when the codebase uses `Result<_, Error>` propagation)
    - Introduces a new data-flow pattern when an existing one fits
    - Duplicates abstractions that already exist under a different name (check Phase 7 Check #26 first)
    - Bypasses existing layers (e.g., talks directly to DB when a repository layer exists)
    - Conflicts with invariants enforced elsewhere (e.g., design assumes mutable access to a struct the rest of the codebase treats as immutable)
    - Each conflict → 🔴 Critical if it breaks invariants, 🟡 Important if it's inconsistent style.

33. **Simplification vs completeness** — Is the design **simplifying the problem** rather than solving it? potato's rule: "不要简化问题 — 问题有多复杂就处理多复杂." Red flags:
    - Edge cases mentioned in requirements are dropped in design with no justification
    - "We'll assume X never happens" without proof X can't happen
    - Error paths reduced to "return error" without specifying recovery semantics
    - Concurrency/ordering concerns waved away as "unlikely in practice"
    - Each unjustified simplification → 🟡 Important (escalate to 🔴 if it drops a requirement).

34. **Breaking-change risk assessment** — If the design modifies existing behavior, is the blast radius analyzed?
    - Which existing callers/tests/features does this affect?
    - Is there a grep/impact analysis in the design, or just "this shouldn't break anything"?
    - Are tests planned to prove existing behavior is preserved where intended?
    - Vague "should be backward-compatible" without verification → 🟡 Important.

35. **Purpose alignment** — Does every component in the design serve the stated goal? Or are there components that are "nice to have" / "might be useful later" / "for flexibility"?
    - Speculative flexibility (interfaces with one implementation, config knobs with one value) → 🟢 Minor, flag for removal
    - Components that don't trace to any GOAL → 🟡 Important (either add GOAL or remove component)

## Output Format

```markdown
## Review: [document name]

### 🔴 Critical (blocks implementation)
1. **[Check #N] Brief title** — Detailed explanation. Suggested fix: ...

### 🟡 Important (should fix before implementation)
1. **[Check #N] Brief title** — Detailed explanation. Suggested fix: ...

### 🟢 Minor (can fix during implementation)
1. **[Check #N] Brief title** — Detailed explanation.

### 📋 Path Traces (for state machines / workflows)
Happy path: State1 → Event → State2 → ... → Done ✅
Failure path 1: ... → Error → ... → Escalated ✅
Edge case 1: Skip from X → ... ✅

### ✅ Passed Checks
- Check #1: Types fully defined ✅ (verified: RitualState has 10 fields, all defined)
- Check #2: References resolve ✅ (verified: §3 referenced in §2.4 exists)
- ...

### Summary
- Critical: N, Important: N, Minor: N
- Recommendation: [ready to implement / needs fixes first / needs major revision]
- Estimated implementation confidence: [high/medium/low] — based on spec clarity
```

## Output Destination

**ALWAYS write the full review to a file**, not just respond in chat. This preserves the review for human approval and enables the apply phase.

1. Write the review to the feature's reviews directory: `.gid/features/{feature}/reviews/design-r{N}.md`
   - Determine the feature from the document path (e.g., `.gid/features/auth/design.md` → feature is `auth`)
   - Determine round number N by checking existing review files (r1, r2, etc.) and incrementing
   - For issue designs (`.gid/issues/{ISS-NNN}/design.md`), write to `.gid/issues/{ISS-NNN}/reviews/design-r{N}.md`
   - For master architecture (`.gid/docs/architecture.md`), write to `.gid/docs/reviews/architecture-r{N}.md`
2. Create the `reviews/` directory if it doesn't exist
3. Each finding must have a unique ID: `FINDING-1`, `FINDING-2`, etc.
4. For each finding that suggests a change, include a `Suggested fix:` block with the concrete change

After writing the review file, report a **brief summary** to the user:
- Total findings count by severity
- List of finding IDs with one-line descriptions
- Ask: "Which findings should I apply? (e.g., 'apply FINDING-1,3,5' or 'apply all')"

## ⚠️ Incremental Output Protocol (MANDATORY)

**Reviews MUST be written incrementally.** Never accumulate all findings in memory and dump them in a single write_file call. Large review dumps (>300 lines / >15KB in one write) fail at the network/context layer and lose all work.

### Why this matters

- Design review output for a large doc is 400-700+ lines across 15-25 findings.
- A single `write_file` of that size frequently fails mid-write on unstable connections.
- When it fails, ALL analysis work is lost — the sub-agent has no checkpoint to resume from.
- This was the #1 cause of review failures prior to v1.1 of this skill.

### The protocol

**Step 1 — Write skeleton first** (single `write_file`, ~40 lines):

```markdown
# Design Review r{N} — {feature-name}

> **Reviewer:** {agent}
> **Date:** {YYYY-MM-DD}
> **Target:** {path/to/design.md}
> **Requirements:** {path/to/requirements.md}
> **Method:** {N}-check review-design skill, depth={quick|standard|full}

## Summary

| Severity   | Count |
|------------|-------|
| Critical   | TBD   |
| Important  | TBD   |
| Minor      | TBD   |
| **Total**  | TBD   |

_Review in progress — findings appended below as they are discovered._

---

<!-- FINDINGS -->

## Applied

(None — awaiting human approval before apply phase.)
```

**Step 2 — Append each finding as you discover it** (one `edit_file` per finding):

After each check that produces a finding, immediately append it to the file using `edit_file`. Anchor against the `<!-- FINDINGS -->` marker so each new finding lands above it in order, OR append right before `## Applied`. Example pattern:

```
edit_file(
  old_string: "<!-- FINDINGS -->",
  new_string: "## FINDING-{N} {severity-icon} {severity} — {title}\n\n{body}\n\n---\n\n<!-- FINDINGS -->"
)
```

Each finding is 30-80 lines. If `edit_file` fails, only that one finding is lost — retry it, don't restart the review.

**Step 3 — Update summary at the end** (single `edit_file`):

After all checks run, compute final counts and replace the TBD rows in the summary table, and remove the "_Review in progress_" line.

### Hard rules

- **NEVER** write a review file in a single >300-line `write_file` call.
- **NEVER** accumulate 5+ findings in memory before writing. Write after each finding is analyzed.
- **If the skeleton write fails** — retry once, then stop and report the error to the user (don't proceed analyzing into the void).
- **If a finding append fails** — retry that one finding, continue with the rest.
- **Finding numbering** is monotonic. If you discover findings out of "check order", that's fine — number them in discovery order, not check order.

### When incremental writes are NOT required

- Review has ≤3 findings total AND full doc is <100 lines: single write is OK.
- Quick-depth reviews often fall into this category. Use judgment.

This protocol applies the "incremental write pattern" from AGENTS.md Rule 3 specifically to the review workflow.

## Rules

- **Run ALL 35 checks.** Don't skip checks even if the first few find nothing.
- **Write incrementally.** Skeleton first (write_file), then append each finding (edit_file) as you discover it. Never accumulate and dump. See 'Incremental Output Protocol' section — this is mandatory, not optional.
- **No "looks good" without evidence.** For each passed check, briefly note what you verified.
- **Find the ROOT issue, not symptoms.** If check #5 and #12 both flag the same underlying problem, consolidate into one finding with the root cause.
- **Suggest concrete fixes.** Not "this could be improved" — show the actual code/spec change.
- **One pass, all findings.** Do not say "I found 3 issues, want me to look for more?" — find ALL issues in one pass.
- **UTF-8 safety is always critical.** Any string slicing on non-ASCII-guaranteed text is 🔴.
- **Check arithmetic with concrete values.** Don't just eyeball `retries < 3` — trace through: retries=0 → first attempt, retries=1 → ..., retries=3 → which branch?
- **Trace full paths.** For state machines: pick the happy path, trace every transition. Then pick every failure path. Then pick the edge cases (skip, cancel, retry from escalated).
- **Think like an adversary.** What inputs would break this? What happens with empty strings, zero counts, concurrent access, network timeouts?
- **Check the "not" sections.** The "不做的事" / "non-goals" / "alternatives rejected" sections are as important as the design itself. Make sure rejected alternatives actually have valid rejection reasons, and non-goals don't conflict with goals.
- **Verify pseudocode compiles mentally.** For every code block: could you actually write this in the target language? Watch for: missing imports, wrong method signatures, type mismatches between prose description and code.
