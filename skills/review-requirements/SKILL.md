---
name: review-requirements
description: Systematically review requirements documents for completeness, testability, and consistency
version: "1.0.0"
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

Read the entire requirements document, then run ALL checks below. Do not stop after finding the first issue.

### Phase 1: Individual Requirement Quality

1. **Specificity** — Each GOAL/requirement must be specific enough that two engineers would implement it the same way. Flag any that use vague language: "should be fast", "user-friendly", "robust", "scalable", "appropriate", "as needed".
2. **Testability** — Every GOAL must have a clear pass/fail condition. Can you write a test for it? If not → flag. "System handles errors gracefully" fails this. "System returns 4xx with error JSON on invalid input" passes.
3. **Measurability** — Quantitative requirements must have concrete numbers. "Low latency" → flag. "<200ms p95" → pass. "High availability" → flag. "99.9% uptime" → pass.
4. **Atomicity** — Each requirement should describe ONE thing. Compound requirements ("System does X AND Y AND Z") should be split. Each should be independently implementable and testable.
5. **Completeness of each requirement** — Does each requirement specify: (a) the actor/trigger, (b) the expected behavior, (c) the expected outcome? Missing any of these → flag.

### Phase 2: Coverage & Gaps

6. **Happy path coverage** — Are all normal user flows covered by at least one requirement? Trace through: user starts → main actions → expected outcomes.
7. **Error/edge case coverage** — What happens when things go wrong? Network failure, invalid input, empty data, concurrent access, timeout. Each error scenario should have a requirement or explicit non-requirement.
8. **Non-functional requirements** — Check for presence of:
   - Performance (latency, throughput, resource limits)
   - Security (auth, authz, data protection, input validation)
   - Reliability (error recovery, data durability, retry behavior)
   - Observability (logging, metrics, alerting)
   - Scalability (data volume, user count, growth projections)
   If any category is missing entirely → flag (it might be intentionally out of scope, but should be stated).
9. **Boundary conditions** — For any numeric parameter: what are min/max values? What happens at 0? At MAX_INT? Empty string? Null?
10. **State transitions** — If the system has states, are transitions between ALL states defined? Any state with no exit? Any state unreachable from the initial state?

### Phase 3: Consistency & Contradictions

11. **Internal consistency** — Do any two requirements contradict each other? Check every pair of requirements that touch the same feature/component.
12. **Terminology consistency** — Same concept, same name everywhere. Check for synonyms used interchangeably (e.g., "user" vs "client" vs "customer" vs "account").
13. **Priority consistency** — If requirements have priorities, do high-priority items depend on low-priority items? That's a priority inversion → flag.
14. **Numbering/referencing** — Do cross-references resolve? If GOAL-42 says "see GOAL-15", does GOAL-15 exist and is it relevant?
15. **GUARDs vs GOALs alignment** — GUARDs (constraints) should not contradict GOALs (features). A GUARD that makes a GOAL unimplementable → critical flag.

### Phase 4: Implementability

16. **Technology assumptions** — Does a requirement implicitly assume a specific technology? If so, is that technology choice documented and justified? "Use WebSocket for real-time" is fine if justified; "real-time updates" without specifying mechanism is ambiguous.
17. **External dependencies** — Requirements that depend on external services/APIs: are those dependencies explicitly named? Version pinned? What happens if the dependency is unavailable?
18. **Data requirements** — For features that need data: where does the data come from? What format? How much? How often updated? Storage requirements?
19. **Migration/compatibility** — If replacing existing functionality: is backward compatibility required? Data migration plan? Feature parity checklist?
20. **Scope boundaries** — Are explicit non-requirements/non-goals stated? Without them, scope creep is inevitable. Every "we won't do X" is as valuable as "we will do Y".

### Phase 5: Traceability & Organization

21. **Unique identifiers** — Every requirement has a unique ID (GOAL-1, GUARD-1, etc.)? No duplicates? No gaps in numbering that suggest deleted requirements without explanation?
22. **Grouping/categorization** — Are requirements organized by feature/domain? Or is it a flat list where related requirements are scattered?
23. **Dependency graph** — Are dependencies between requirements explicit? Which requirements must be implemented before others? Any circular dependencies?
24. **Acceptance criteria** — Does each requirement (or at least each epic/feature group) have clear acceptance criteria that differ from the requirement itself?

### Phase 6: Stakeholder Alignment

25. **User perspective** — Are requirements written from the user's perspective where appropriate? Or are they all system-internal ("the database shall...")? User-facing features need user-centric language.
26. **Success metrics** — How will you know the requirements are met in production? Are there observable metrics beyond "tests pass"?
27. **Risk identification** — Are high-risk requirements identified? Complex, novel, or uncertain requirements should be flagged for prototyping/spike.

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

1. Write the review to `.gid/reviews/<document-name>-review.md` (e.g., `.gid/reviews/requirements-gepa-core-review.md`)
2. Create `.gid/reviews/` directory if it doesn't exist
3. Each finding must have a unique ID: `FINDING-1`, `FINDING-2`, etc.
4. For each finding that suggests a change, include a `Suggested fix:` block with the concrete change

After writing the review file, report a **brief summary** to the user:
- Total findings count by severity
- List of finding IDs with one-line descriptions
- Ask: "Which findings should I apply? (e.g., 'apply FINDING-1,3,5' or 'apply all')"

## Rules

- **Run ALL 27 checks.** Don't skip checks even if early ones find nothing.
- **No "looks good" without evidence.** For each passed check, note what you verified and the count.
- **Check EVERY requirement individually for checks #1-5.** Don't just sample — exhaustive review.
- **Build the coverage matrix.** This is the most valuable output — it shows what's missing, not just what's wrong.
- **Flag vague language with concrete alternatives.** Not "this is vague" — show what the specific version would look like.
- **Count everything.** "Most requirements are testable" is useless. "38/42 GOALs are testable, 4 are not (GOAL-7, 12, 29, 35)" is actionable.
- **Cross-reference GOALs and GUARDs.** A GUARD that no GOAL references might be dead. A GOAL that violates a GUARD is a contradiction.
- **Think like an implementer.** For each requirement: could you start coding right now? What questions would you need answered first?
- **Think like a tester.** For each requirement: could you write a test right now? What test data would you need?
- **Distinguish "not specified" from "not needed".** Missing non-functional requirements might be intentionally out of scope — but that should be explicitly stated, not left ambiguous.
