---
name: review-design
description: Systematically review design documents for bugs, inconsistencies, and missing cases
version: "1.0.0"
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
  - Write findings to the review file EARLY — start writing after Phase 2 at latest, then append as you go.
  - Budget: max 30% reading (source code for verification), 70% analysis and writing findings.
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

This skill is iteration-heavy — reading the full design, running 29 checks, and verifying code references all consume tool calls. When spawning a sub-agent for this skill:

- **full review**: `max_iterations: 50` minimum (29 checks + code verification = 40-50 tool calls)
- **standard review**: `max_iterations: 35` minimum
- **quick review**: `max_iterations: 20` minimum

Under-provisioned iterations are the #1 cause of incomplete reviews — the agent runs out mid-Phase-7 and never writes the review file.

## Review Process

### Review Depth (Triage-Driven)

Check the beginning of your prompt for a `[REVIEW_DEPTH: quick|standard|full]` directive. This is injected by the ritual system based on triage size.

| Depth | Triage Size | Phases to Run | Checks |
|---|---|---|---|
| **quick** | small | Phase 0 + Phase 1 + Phase 4 | 0-4, 13-16 (9 checks) |
| **standard** | medium | Phase 0-5 | 0-20 (21 checks) |
| **full** | large (default) | Phase 0-7 | All 29 checks |

**If no `[REVIEW_DEPTH]` directive is present, default to `full`.**

For `quick` reviews: skip logic correctness, type safety edge cases, doc quality, implementability, and code alignment checks. Focus on structure and architecture only — the goal is fast validation that the design is internally consistent.

For `standard` reviews: skip Phase 6 (Implementability) and Phase 7 (Existing Code Alignment). These are expensive checks that require codebase knowledge and are best reserved for large/complex designs.

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

## Rules

- **Run ALL 29 checks.** Don't skip checks even if the first few find nothing.
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
