---
name: review-requirements
description: Systematically review requirements documents for completeness, testability, and consistency
file_policy: forbidden
version: "1.1.0"
author: potato
triggers:
  patterns:
    - "review requirements"
    - "review需求"
    - "审核需求"
    - "check requirements"
    - "检查需求"
    - "review req"
  regex:
    - "(?i)review.*req"
    - "(?i)req.*review"
tags:
  - development
  - quality
priority: 55
always_load: false
max_body_size: 8192
subagent_preamble: |
  You are a sub-agent reviewing a requirements document. Key rules:
  - The requirements document is ALREADY pre-loaded in your context. Do NOT call read_file to re-read it.
  - Run review checks against the pre-loaded content directly.
  - Write findings to the review file EARLY — start writing after the first few checks, then append.
  - Budget: max 20% reading (only for cross-referencing other docs if needed), 80% analysis and writing.
---
# SKILL: Requirements Document Reviewer

> Systematic review ensuring every requirement is specific, testable, non-contradictory, and implementation-ready. No "looks reasonable" — find gaps or prove there are none.

## Purpose

Requirements docs are the contract between design and implementation. Vague requirements → vague implementations → rework. This skill enforces a structured checklist that catches ambiguity, gaps, and contradictions in a single pass.

## When to Use

- After writing or updating any requirements document (requirements-*.md, REQUIREMENTS.md)
- Before starting design or implementation based on requirements
- When requirements have been revised after stakeholder feedback

## Review Process

### Review Depth (Triage-Driven)

Check the beginning of your prompt for a `[REVIEW_DEPTH: quick|standard|full]` directive. This is injected by the ritual system based on triage size.

| Depth | Triage Size | Phases to Run | Checks |
|---|---|---|---|
| **quick** | small | Phase 0 + Phase 1 + Phase 4 + Phase 7 | 0-6, 17-21, 29-33 (17 checks) |
| **standard** | medium | Phase 0-5 + Phase 7 | 0-25, 29-33 (31 checks) |
| **full** | large (default) | Phase 0-7 | All 33 checks |

**If no `[REVIEW_DEPTH]` directive is present, default to `full`.**

For `quick` reviews: skip coverage & gaps, consistency checks, traceability, and stakeholder alignment. Focus on individual requirement quality (including implementation leakage detection), implementability, and engineering integrity (debt/shortcuts/conflicts #29-33) — the goal is fast validation that each requirement is specific, testable, not a disguised design decision, and not quietly introducing debt.

For `standard` reviews: skip Phase 6 (Stakeholder Alignment). Phase 7 (Engineering Integrity) is always run — debt and conflict detection are non-negotiable, even for incremental requirement updates.

---

Read the entire requirements document, then run the checks applicable to your review depth. Do not stop after finding the first issue.

### Phase 0: Document Size Check

0. **Document size** — Count total GOALs. If >15 GOALs in a single document → **Critical finding**: document must be split into feature-level requirements (see draft-requirements skill for structure). A single requirements doc should have ≤15 GOALs. GUARDs stay in the master doc; GOALs are split by feature. This is the root fix for review quality — no complex multi-agent architecture needed, just smaller documents.

### Phase 1: Individual Requirement Quality

1. **Specificity** — Each GOAL/requirement must be specific enough that two engineers would implement it the same way. Flag any that use vague language: "should be fast", "user-friendly", "robust", "scalable", "appropriate", "as needed".
2. **Testability** — Every GOAL must have a clear pass/fail condition. Can you write a test for it? If not → flag. "System handles errors gracefully" fails this. "System returns 4xx with error JSON on invalid input" passes.
3. **Measurability** — Quantitative requirements must have concrete numbers. "Low latency" → flag. "<200ms p95" → pass. "High availability" → flag. "99.9% uptime" → pass.
4. **Atomicity** — Each requirement should describe ONE thing. Compound requirements ("System does X AND Y AND Z") should be split. Each should be independently implementable and testable.
5. **Completeness of each requirement** — Does each requirement specify: (a) the actor/trigger, (b) the expected behavior, (c) the expected outcome? Missing any of these → flag.
6. **Implementation leakage** — Apply the Substitution Test to every GOAL: "Could someone satisfy this with a completely different internal implementation?" If the GOAL specifies field names, schema layouts, algorithm choices, function signatures, enum variants, or internal data structures → it's a design decision disguised as a requirement. This is the #1 source of requirements that pass all other checks but cause endless review cycles — the contradictions are structural (design decisions conflicting with each other), not editorial. **Especially watch for infrastructure/framework features** where the "product" is a schema or API — the schema details feel like requirements but aren't. Strip field names and code identifiers; what remains should still be a valid, verifiable requirement.

### Phase 2: Coverage & Gaps

7. **Happy path coverage** — Are all normal user flows covered by at least one requirement? Trace through: user starts → main actions → expected outcomes.
8. **Error/edge case coverage** — What happens when things go wrong? Network failure, invalid input, empty data, concurrent access, timeout. Each error scenario should have a requirement or explicit non-requirement.
9. **Non-functional requirements** — Check for presence of:
   - Performance (latency, throughput, resource limits)
   - Security (auth, authz, data protection, input validation)
   - Reliability (error recovery, data durability, retry behavior)
   - Observability (logging, metrics, alerting)
   - Scalability (data volume, user count, growth projections)
   If any category is missing entirely → flag (it might be intentionally out of scope, but should be stated).
10. **Boundary conditions** — For any numeric parameter: what are min/max values? What happens at 0? At MAX_INT? Empty string? Null?
11. **State transitions** — If the system has states, are transitions between ALL states defined? Any state with no exit? Any state unreachable from the initial state?

### Phase 3: Consistency & Contradictions

12. **Internal consistency** — Do any two requirements contradict each other? Check every pair of requirements that touch the same feature/component.
13. **Terminology consistency** — Same concept, same name everywhere. Check for synonyms used interchangeably (e.g., "user" vs "client" vs "customer" vs "account").
14. **Priority consistency** — If requirements have priorities, do high-priority items depend on low-priority items? That's a priority inversion → flag.
15. **Numbering/referencing** — Do cross-references resolve? If GOAL-42 says "see GOAL-15", does GOAL-15 exist and is it relevant?
16. **GUARDs vs GOALs alignment** — GUARDs (constraints) should not contradict GOALs (features). A GUARD that makes a GOAL unimplementable → critical flag.

### Phase 4: Implementability

17. **Technology assumptions** — Does a requirement implicitly assume a specific technology? If so, is that technology choice documented and justified? "Use WebSocket for real-time" is fine if justified; "real-time updates" without specifying mechanism is ambiguous.
18. **External dependencies** — Requirements that depend on external services/APIs: are those dependencies explicitly named? Version pinned? What happens if the dependency is unavailable?
19. **Data requirements** — For features that need data: where does the data come from? What format? How much? How often updated? Storage requirements?
20. **Migration/compatibility** — If replacing existing functionality: is backward compatibility required? Data migration plan? Feature parity checklist?
21. **Scope boundaries** — Are explicit non-requirements/non-goals stated? Without them, scope creep is inevitable. Every "we won't do X" is as valuable as "we will do Y".

### Phase 5: Traceability & Organization

22. **Unique identifiers** — Every requirement has a unique ID (GOAL-1, GUARD-1, etc.)? No duplicates? No gaps in numbering that suggest deleted requirements without explanation?
23. **Grouping/categorization** — Are requirements organized by feature/domain? Or is it a flat list where related requirements are scattered?
24. **Dependency graph** — Are dependencies between requirements explicit? Which requirements must be implemented before others? Any circular dependencies?
25. **Acceptance criteria** — Does each requirement (or at least each epic/feature group) have clear acceptance criteria that differ from the requirement itself?

### Phase 6: Stakeholder Alignment

26. **User perspective** — Are requirements written from the user's perspective where appropriate? Or are they all system-internal ("the database shall...")? User-facing features need user-centric language.
27. **Success metrics** — How will you know the requirements are met in production? Are there observable metrics beyond "tests pass"?
28. **Risk identification** — Are high-risk requirements identified? Complex, novel, or uncertain requirements should be flagged for prototyping/spike.

### Phase 7: Engineering Integrity (Technical Debt, Shortcuts, Conflicts)

Requirements can introduce debt just like designs can — by being too narrow, by conflicting with existing requirements, or by quietly cutting scope that should be kept. This phase surfaces those.

29. **Technical debt in requirement framing** — Does any GOAL/GUARD implicitly accept debt? Look for:
    - "For now, only support X" without a follow-up requirement for Y (scope cut disguised as phase 1)
    - "Assume Z" when Z is actually a decision that should be a GUARD
    - "Legacy compatibility required" without specifying which legacy behaviors (ambiguous debt)
    - Each instance → 🟡 Important. Either make the scope cut explicit (add non-goal), make the assumption a GUARD, or remove the caveat.

30. **Shortcut / simplification detection** — Is the requirement **dodging the hard part**? potato's rule: 问题有多复杂就处理多复杂. Red flags:
    - Requirement that names the easy path but omits error/edge cases ("user logs in" without "what if creds expire mid-session")
    - Quantitative targets conspicuously loose ("handles requests" with no latency/throughput) — might be dodging a hard NFR
    - "Best-effort" / "when possible" language on requirements that actually have hard dependencies
    - Each shortcut → 🟡 Important. Ask: "Is this loose because the real answer is hard, or because it genuinely doesn't matter?"

31. **Conflicts with existing requirements / system invariants** — Does this document contradict:
    - Requirements in other feature docs (cross-feature conflict)? — list the conflicting IDs
    - GUARDs in the master requirements doc (e.g., feature GOAL contradicts global security GUARD)?
    - Existing production behavior that callers depend on (breaking change not flagged as such)?
    - Conventions established across the project (naming, error semantics, data ownership)?
    - Each conflict → 🔴 Critical. Conflicts discovered in design/implementation are 10x more expensive than conflicts caught here.

32. **Missing non-goals (scope creep prevention)** — potato's rule: "every 'we won't do X' is as valuable as 'we will do Y'." For each feature boundary that's ambiguous, is there an explicit non-goal? If readers could reasonably think "this requires X" but the author meant to exclude X → 🟡 Important. Add a non-goal.

33. **Requirement stability** — Are any requirements flagged as "TBD" / "to be decided" / "pending stakeholder input" without owner + deadline? Unresolved requirements that go to design become discovered-in-implementation rework. Each unowned TBD → 🟡 Important.

## Output Format

```markdown
## Review: [document name]

### 🔴 Critical (blocks implementation)
1. **[Check #N] GOAL-XX: Brief title** — Detailed explanation. Suggested fix: ...

### 🟡 Important (should fix before implementation)
1. **[Check #N] GOAL-XX: Brief title** — Detailed explanation. Suggested fix: ...

### 🟢 Minor (can fix during implementation)
1. **[Check #N] Brief title** — Detailed explanation.

### 📊 Coverage Matrix
| Category | Covered | Missing |
|---|---|---|
| Happy path | GOAL-1,2,3 | - |
| Error handling | GOAL-10,11 | Network timeout, concurrent write |
| Performance | GOAL-20 | No throughput requirement |
| Security | - | ⚠️ No security requirements at all |
| Observability | GOAL-25 | No alerting criteria |

### ✅ Passed Checks
- Check #1: Specificity ✅ (verified: 42/42 GOALs have concrete conditions)
- Check #2: Testability ✅ (verified: each GOAL has pass/fail criteria)
- ...

### Summary
- Total requirements: N GOALs, M GUARDs
- Critical: N, Important: N, Minor: N
- Coverage gaps: [list missing categories]
- Recommendation: [ready for design / needs fixes first / needs major revision]
- Estimated implementation clarity: [high/medium/low]
```

## Output Destination

**ALWAYS write the full review to a file**, not just respond in chat. This preserves the review for human approval and enables the apply phase.

1. Write the review to the feature's reviews directory: `.gid/features/{feature}/reviews/requirements-r{N}.md`
   - Determine the feature from the document path (e.g., `.gid/features/auth/requirements.md` → feature is `auth`)
   - Determine round number N by checking existing review files (r1, r2, etc.) and incrementing
   - For master requirements (`.gid/docs/requirements.md`), write to `.gid/docs/reviews/requirements-r{N}.md`
2. Create the `reviews/` directory if it doesn't exist
3. Each finding must have a unique ID: `FINDING-1`, `FINDING-2`, etc.
4. For each finding that suggests a change, include a `Suggested fix:` block with the concrete change

After writing the review file, report a **brief summary** to the user:
- Total findings count by severity
- List of finding IDs with one-line descriptions
- Ask: "Which findings should I apply? (e.g., 'apply FINDING-1,3,5' or 'apply all')"

## ⚠️ Incremental Output Protocol (MANDATORY)

**Reviews MUST be written incrementally.** A single `write_file` with all findings frequently fails mid-write and loses all analysis work.

**Step 1** — Write skeleton with `write_file` (~40 lines): header, summary table with TBD counts, empty `<!-- FINDINGS -->` marker, empty `## Applied` section.

**Step 2** — Append each finding with `edit_file` as you discover it. Anchor against `<!-- FINDINGS -->`:

```
edit_file(
  old_string: "<!-- FINDINGS -->",
  new_string: "## FINDING-{N} {icon} {severity} — {title}\n\n{body}\n\n---\n\n<!-- FINDINGS -->"
)
```

**Step 3** — Update the summary table counts with a final `edit_file` after all findings are written.

**Hard rules:**
- NEVER write a >300-line review in a single `write_file` call.
- NEVER accumulate 5+ findings in memory before writing any.
- If a finding append fails, retry just that one; continue with the rest.
- Finding numbering is monotonic in discovery order, not check order.

Exception: if review has ≤3 findings and doc is <100 lines, a single write is fine.

## Rules

- **Run ALL 33 checks.** Don't skip checks even if early ones find nothing.
- **No "looks good" without evidence.** For each passed check, note what you verified and the count.
- **Check EVERY requirement individually for checks #1-5.** Don't just sample — exhaustive review.
- **Build the coverage matrix.** This is the most valuable output — it shows what's missing, not just what's wrong.
- **Flag vague language with concrete alternatives.** Not "this is vague" — show what the specific version would look like.
- **Count everything.** "Most requirements are testable" is useless. "38/42 GOALs are testable, 4 are not (GOAL-7, 12, 29, 35)" is actionable.
- **Cross-reference GOALs and GUARDs.** A GUARD that no GOAL references might be dead. A GOAL that violates a GUARD is a contradiction.
- **Think like an implementer.** For each requirement: could you start coding right now? What questions would you need answered first?
- **Think like a tester.** For each requirement: could you write a test right now? What test data would you need?
- **Distinguish "not specified" from "not needed".** Missing non-functional requirements might be intentionally out of scope — but that should be explicitly stated, not left ambiguous.
