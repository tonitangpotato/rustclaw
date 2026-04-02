---
name: requirements
description: Generate structured requirements documents with goals and guards
version: "2.0.0"
author: potato
triggers:
  patterns:
    - "write requirements"
    - "写需求"
    - "requirements for"
    - "需求文档"
  regex:
    - "(?i)requirements?\\.md"
tags:
  - development
  - planning
priority: 60
always_load: false
max_body_size: 4096
---
# SKILL: Requirements Document Generator

> Transform project ideas into structured, verifiable requirements documents.

## Purpose

This is Phase 2 of the GID pipeline: Idea → **Requirements** → Design → Graph → Execute.

Requirements documents define **WHAT** the system must do (not HOW). Every criterion must be verifiable — if you can't test it, it's not a criterion.

## When to Use

- After idea intake (Phase 1) produces a clear project concept
- When extracting requirements from an existing design document
- When potato says "write requirements for X"
- Before writing a DESIGN.md (requirements come first), OR after design exists (extract from it)

## Output Location

Depends on project structure:
- **Simple project (single feature):** `.gid/requirements.md`
- **Multi-feature project:** `.gid/features/{feature-name}/requirements.md`

The `.gid/` location is canonical — `assemble_task_context()` resolves requirements from there via the feature node's `design_doc` metadata. Task nodes' `satisfies` references (e.g., `GOAL-1.1`) are resolved against their parent feature's requirements.md.

## Naming Convention

**ONLY use these names. No aliases, no alternatives:**
- **GOAL-X.Y** for functional requirements (goals)
- **GUARD-X** for system-wide invariants (guards)

❌ NEVER use: CR, INV, AC, CP, REQ, FR, NFR, or any other abbreviation.
Even if the task prompt uses different names, always use GOAL/GUARD.

## Template

```markdown
# Requirements: {Project Name}

## Overview

{One paragraph: what this project does and why it exists. No implementation details.
Include the core user problem being solved.}

## Priority Levels

- **P0**: Core — required for the system to function at all
- **P1**: Important — needed for production-quality operation
- **P2**: Enhancement — improves efficiency, UX, or observability

## Guard Severity

- **hard**: Violation = system is broken, execution must stop
- **soft**: Violation = degraded quality, should warn but can continue

## Goals

{Verifiable completion conditions. Each must be testable by a human or automated check.
Group by feature area. Use GOAL-{module}.{number} format.
Numbers must be sequential within each module — no gaps.}

### {Feature Area 1}
- **GOAL-1.1** [P0]: {Specific, verifiable condition} *(ref: {source})*
- **GOAL-1.2** [P1]: {Specific, verifiable condition} *(ref: {source})*

### {Feature Area 2}
- **GOAL-2.1** [P0]: {Specific, verifiable condition} *(ref: {source})*
- **GOAL-2.2** [P2]: {Specific, verifiable condition} *(ref: {source})*

## Guards

{System-level properties that must ALWAYS hold, regardless of feature.
These are cross-cutting constraints, not feature-specific.
Think: security, data integrity, performance, reliability.
Numbers must be sequential — GUARD-1, GUARD-2, etc.}

- **GUARD-1** [hard]: {Property that must never be violated} *(ref: {source})*
- **GUARD-2** [soft]: {Property that should hold} *(ref: {source})*

## Out of Scope

- {Thing explicitly not included}

## Dependencies

- {Dependency and why it's needed}

{Summary line: **N GOALs** (X P0 / Y P1 / Z P2) + **M GUARDs** (A hard / B soft)}
```

## Priority Assignment Guide

**P0 — "System doesn't work without this":**
- Core data flow (input → process → output)
- Safety/isolation guarantees (no data loss, no unauthorized access)
- Error handling that prevents crashes (rate limits, retries)
- State management (persistence, recovery)

**P1 — "System works but isn't production-ready":**
- Configuration flexibility (overridable defaults)
- Detailed observability (telemetry, stats)
- Graceful degradation (cancellation, cleanup)
- Optimization (skip unnecessary work)

**P2 — "Nice to have":**
- Smart defaults from historical data
- Cost estimation
- Advanced UI features
- Automatic routing/selection

**When unsure:** P1. Better to over-prioritize than under — P0 items gate implementation.

## The WHAT/HOW Boundary

**This is the most common mistake.** Requirements describe observable behavior, not implementation.

### ❌ Implementation details (belong in DESIGN.md)
```
BAD:  "Detects cycles using Kahn's algorithm"
BAD:  "Merges are serialized via mutex/lock"
BAD:  "Extracts sections via heading-level matching algorithm"
BAD:  "Uses SQLite for persistence"
BAD:  "Implements retry with exponential backoff"
```

### ✅ Observable behavior (belongs in requirements)
```
GOOD: "Detects dependency cycles and rejects cyclic graphs with cycle path details"
GOOD: "Merges within a layer are serialized — no two merges happen concurrently"
GOOD: "Extracts design section text matching the referenced section number"
GOOD: "Execution state persists across crashes"
GOOD: "Failed tasks are retried with enhanced context before marking blocked"
```

**Rule of thumb:** If you removed the requirement text and replaced it with a completely different implementation that achieves the same observable result, would the requirement still be valid? If yes, it's a good requirement. If the requirement specifies the implementation itself, it's too specific.

## What Is NOT a Requirement

Some statements look like requirements but aren't verifiable functional behaviors:

| Not a requirement | What it actually is | Where it belongs |
|---|---|---|
| "Task descriptions must be self-contained" | Writing guideline for graph creation | Skill template / gid_design prompt |
| "gidterm's backend is not used" | Architecture decision | DESIGN.md |
| "Use Opus for complex tasks" | Configuration preference | execution.yml |
| "Follow the 7-phase pipeline" | Process description | DESIGN.md overview |

**Test:** Can a test or human verify this by observing system behavior? If not, it's not a requirement.

## Numbering Rules

1. **Sequential within each module** — GOAL-2.1, 2.2, 2.3... never skip
2. **Never append out-of-order** — if you need to add GOAL-2.X after finishing module 2, renumber
3. **GUARD numbers are flat** — GUARD-1, GUARD-2, ... (not grouped by category in numbering, only in headings)
4. **Stable after approval** — once requirements.md is approved, IDs are frozen. New requirements get next available number.

## Reference Annotations

Every GOAL and GUARD must have a `*(ref: ...)*` pointing to its source:

**Good refs — specific enough to find:**
```
*(ref: DESIGN, Architecture/Topology Analyzer)*
*(ref: DESIGN, Context Assembly/Strategy 2)*
*(ref: DESIGN, Module 6/Failure Escalation)*
*(ref: conversation, 2026-04-01)*
```

**Bad refs — too vague to be useful:**
```
*(ref: DESIGN §2)*              — §2 is 60% of the doc
*(ref: DESIGN, Architecture)*   — too broad
*(ref: discussion)*             — when? about what?
```

**For extraction from design docs:** use the subsection heading, not the top-level section number.

## Extraction Mode (from existing Design Doc)

When requirements.md is written AFTER a design doc exists:

### Approach
1. Read the full design doc
2. For each component/feature: ask "what observable behavior does this produce?"
3. Write the WHAT, reference the design section for the HOW
4. Cross-check: every design section should map to at least one GOAL

### Common Extraction Mistakes
- **Copying design language verbatim** — design says "Kahn's topological sort", requirement should say "detects cycles and groups into layers"
- **Including internal architecture** — "Scheduler uses eager scheduling" → "Tasks can start as soon as their specific dependencies complete"
- **Missing implicit requirements** — design doc has error handling table but no explicit section = easy to miss
- **Forgetting configuration** — design doc defines config cascading but it's easy to only capture one level

### Extraction Checklist
After writing, verify against design doc:
- [ ] Every design component has at least one GOAL
- [ ] Error handling table entries are all captured
- [ ] Configuration options are captured (including cascading precedence)
- [ ] File formats and schemas mentioned in design are captured
- [ ] Sub-agent/external interface contracts are captured

## Self-Review Checklist

**Before submitting, verify ALL of these:**

- [ ] **Naming**: Only GOAL-X.Y and GUARD-X used (no CR, INV, AC, CP)
- [ ] **Numbering**: Sequential within each module, no gaps
- [ ] **WHAT not HOW**: No algorithms, data structures, or implementation patterns in requirement text
- [ ] **All functional**: Every GOAL describes observable/testable behavior (not guidelines, decisions, or preferences)
- [ ] **Priority assigned**: Every GOAL has [P0], [P1], or [P2]
- [ ] **Severity assigned**: Every GUARD has [hard] or [soft]
- [ ] **Refs specific**: Every ref points to a specific subsection, not a top-level section
- [ ] **No duplicates**: No two GOALs describe the same behavior (merge overlapping ones)
- [ ] **Guards are cross-cutting**: Every GUARD applies to multiple features (feature-specific → GOAL)
- [ ] **Summary accurate**: Final count line matches actual counts

## Writing Guidelines

### Goals

**Good goals are:**
- **Specific**: "User can list all auth profiles showing name, status, and token prefix" — not "auth works"
- **Verifiable**: Can be checked by running a command, reading output, or running a test
- **Independent**: Each criterion tests one thing. Don't combine multiple conditions.
- **Complete**: Cover all user-facing behaviors. If a feature exists, it has goals.

**Examples:**
```
✅ GOAL-1.1 [P0]: Running `agentctl auth list` displays all profiles with name, status (active/cooldown), and token prefix (first 8 chars)
✅ GOAL-1.2 [P0]: Running `agentctl auth use <name>` switches the active profile and confirms with "Switched to <name>"
✅ GOAL-2.1 [P1]: Config file is written atomically so partial writes never corrupt state

❌ "Auth module works correctly" — not specific
❌ "Good performance" — not verifiable
❌ "Handle errors properly" — not specific or verifiable
❌ "Uses Kahn's algorithm for cycle detection" — implementation detail
```

### Guards

**Guards are system-wide, not feature-specific:**
- Security: "Auth tokens never appear in logs or error messages"
- Data integrity: "All file writes are atomic — partial writes never corrupt state"
- Reliability: "No operation leaves the system in an inconsistent state on crash"
- Performance: "CLI commands respond within 500ms for local operations"

**If it only applies to one feature, it's a goal, not a guard.**

### Out of Scope

Be explicit. Sub-agents will read this. If something is ambiguous, an agent might waste turns implementing it.

```
✅ "GUI interface — CLI only for v1"
✅ "Multi-user support — single user only"
✅ "Cloud sync — local storage only"
```

## Process

### Step 1: Understand the Project
- Read any existing idea intake notes, conversation context, or prior documents
- If a design doc exists, use Extraction Mode (see above)
- If an idea intake (Phase 1) exists, use it as primary input
- `engram recall "{project topic} requirements decisions"` for past context

### Step 2: Draft Requirements
- Write the full document following the template above
- Typical size: 15-30 GOALs for small projects, 50-120 for large systems
- Aim for 3-10 GUARDs (cross-cutting only)
- At least 3 out-of-scope items

### Step 3: Self-Review
- Run through the Self-Review Checklist above
- Fix any issues before presenting

### Step 4: Present for Review
- Show the complete document
- Ask specifically: "Are these goals complete? Anything missing or wrong?"
- Iterate until approved

### Step 5: Save
- Write to `requirements.md` in project root
- Store in engram: `engram add --type factual --importance 0.7 "Requirements written for {project}: {GOAL count} goals, {GUARD count} guards"`
- Log in daily memory file

## Traceability

After this document is written, the next phases reference it:
- **DESIGN.md** addresses how to satisfy each GOAL
- **GID graph** tasks have `satisfies: ["GOAL-1.1", "GOAL-1.2"]` metadata
- **Verification** checks goals after task completion
- **Guards** become cross-cutting test cases or checkpoint constraints

This chain ensures nothing is implemented without a reason, and nothing is required without being implemented.
