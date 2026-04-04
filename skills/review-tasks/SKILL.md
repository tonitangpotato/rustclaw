---
name: review-tasks
description: Systematically review task breakdowns for completeness, dependency correctness, and implementability
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "review tasks"
    - "review任务"
    - "审核任务"
    - "check tasks"
    - "检查任务"
  regex:
    - "(?i)review.*task"
    - "(?i)task.*review"
tags:
  - development
  - quality
priority: 55
always_load: false
max_body_size: 8192
---
# SKILL: Task Breakdown Reviewer

> Systematic review ensuring every task is implementable, correctly ordered, properly scoped, and traceable to requirements/design. No "looks reasonable" — find gaps or prove there are none.

## Purpose

Task breakdowns bridge design and implementation. Bad task decomposition → blocked developers, missed dependencies, scope confusion. This skill catches these issues before work starts.

## When to Use

- After generating tasks from a design document
- After updating the graph (`.gid/graph.yml`)
- Before starting a sprint/implementation cycle
- When tasks have been revised after design changes

## Data Source

**Tasks live in `.gid/graph.yml`**, not in separate task files. Read the graph YAML and extract all task nodes (nodes with `kind: task` or status fields like `todo`, `in_progress`, `done`). Dependencies are edges between nodes.

## Review Process

Read the graph YAML (`.gid/graph.yml`), the design document, and requirements (if they exist). Then run ALL checks below.

### Phase 1: Individual Task Quality

1. **Clarity** — Each task must be clear enough that a developer can start immediately without asking questions. Flag tasks like "set up infrastructure" or "implement the system". What infrastructure? Which part of the system?
2. **Scope** — Each task should be completable in one focused session (roughly 1-4 hours of work). Too big → should be split. Too small (e.g., "add import statement") → should be merged with parent task.
3. **Acceptance criteria** — Each task must have a clear "done" condition. "Implement auth" fails. "Auth middleware returns 401 for invalid tokens, 403 for insufficient permissions, passes valid tokens to handler" passes.
4. **Actionable verbs** — Each task should start with a concrete verb: implement, create, add, refactor, test, configure. Avoid: consider, explore, think about, look into.
5. **Single responsibility** — Each task does ONE thing. "Implement auth and add rate limiting" → should be two tasks.

### Phase 2: Dependencies & Ordering

6. **Dependency correctness** — For each declared dependency: is it actually needed? Can you start task B without completing task A? False dependencies slow down parallel work.
7. **Missing dependencies** — For each task: what does it read/modify? If task B modifies a file that task A creates → B depends on A. Check that this dependency is explicit.
8. **Circular dependencies** — Trace the dependency graph. Any cycles? A→B→C→A is a deadlock.
9. **Critical path** — What's the longest dependency chain? Is it reasonable? Can any tasks be parallelized to shorten it?
10. **Ordering feasibility** — If tasks are ordered (task-1, task-2...), can they actually be done in that order? Or does task-5 need something from task-8?

### Phase 3: Coverage & Traceability

11. **Design coverage** — Every component/feature in the design document should map to at least one task. Read the design, list its components, verify each has a task.
12. **Requirements coverage** — If requirements exist, every GOAL should map to at least one task. Any GOAL without a task → gap.
13. **Test tasks** — Are there explicit testing tasks? Unit tests, integration tests, E2E tests. "Add tests" as a single task for a 10-task feature → too vague.
14. **Documentation tasks** — If the feature needs docs (API docs, user guide, README updates), are there tasks for it?
15. **Cleanup/migration tasks** — If replacing old code: are there tasks to remove the old implementation, update imports, migrate data?

### Phase 4: Estimation & Risk

16. **Complexity distribution** — Are most tasks similarly sized, or is one task 10x bigger than the rest? Uneven distribution suggests the big task needs further breakdown.
17. **Risk identification** — Which tasks involve: new technology, external APIs, complex algorithms, data migration? These should be flagged as high-risk and ideally done first.
18. **Unknowns** — Are there tasks that depend on information not yet available? (e.g., "integrate with partner API" when the API spec isn't finalized) Flag these as blocked.
19. **Parallel workstreams** — Can the tasks be organized into independent streams for parallel execution? If everything is sequential → look for opportunities to parallelize.

### Phase 5: Consistency

20. **Naming consistency** — Same naming convention across all tasks. Don't mix "implement X" with "create Y" with "build Z" for the same type of work.
21. **Granularity consistency** — If task-1 is "implement complete auth system" and task-2 is "add semicolon to line 42" → inconsistent granularity.
22. **Status accuracy** — If tasks have statuses: are they correct? A task marked "done" whose output file doesn't exist → flag. A task marked "todo" whose implementation already exists in code → flag.

## Output Destination

**ALWAYS write the full review to a file**, not just respond in chat.

1. Write the review to `.gid/reviews/<document-name>-review.md`
2. Create `.gid/reviews/` directory if it doesn't exist
3. Each finding must have a unique ID: `FINDING-1`, `FINDING-2`, etc.
4. For each finding that suggests a change, include a `Suggested fix:` block

After writing the review file, report a **brief summary** to the user:
- Total findings count by severity
- List of finding IDs with one-line descriptions
- Ask: "Which findings should I apply? (e.g., 'apply FINDING-1,3,5' or 'apply all')"

## Output Format

```markdown
## Review: [task source]

### 🔴 Critical (blocks implementation)
1. **[Check #N] FINDING-1: task-XX title** — Detailed explanation. Suggested fix: ...

### 🟡 Important (should fix before starting)
1. **[Check #N] FINDING-2: title** — Detailed explanation. Suggested fix: ...

### 🟢 Minor (can fix during implementation)
1. **[Check #N] FINDING-3: title** — Detailed explanation.

### 📊 Coverage Matrix
| Design Component | Task(s) | Status |
|---|---|---|
| Auth module | task-1, task-2 | ✅ Covered |
| API gateway | task-5 | ✅ Covered |
| Data migration | - | ⚠️ Missing |
| Tests | task-10 | 🟡 Too vague |

### 🔗 Dependency Graph Issues
- task-3 → task-7: False dependency (task-3 doesn't produce anything task-7 needs)
- task-5 ← missing dep on task-2 (task-5 reads auth config created by task-2)
- Critical path: task-1 → task-2 → task-5 → task-8 → task-10 (5 sequential tasks)

### ✅ Passed Checks
- Check #1: Clarity ✅ (verified: 12/12 tasks have concrete descriptions)
- Check #6: Dependencies ✅ (verified: all 8 declared deps are genuine)
- ...

### Summary
- Total tasks: N
- Critical: N, Important: N, Minor: N
- Coverage gaps: [list missing design/req mappings]
- Recommendation: [ready to start / needs fixes first / needs major revision]
```

## Rules

- **Run ALL 22 checks.** Don't skip checks even if early ones find nothing.
- **Read the design document AND the tasks.** Tasks without design context can't be properly reviewed.
- **Trace every dependency.** Don't trust declared deps — verify by checking what each task reads/writes.
- **Build the coverage matrix.** This is the highest-value output.
- **Check for phantom tasks** — tasks that sound productive but produce nothing concrete. "Research best practices" without a deliverable → flag.
- **Count and quantify.** "Most tasks are clear" → useless. "10/12 tasks are clear, task-4 and task-9 need clarification" → actionable.
- **Think like a developer picking up task-N.** Could you start coding right now? What would you need to ask first?
- **Check for "and" in task descriptions.** "Implement X and Y" almost always should be two tasks.
