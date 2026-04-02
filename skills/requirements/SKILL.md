---
name: requirements
description: Generate structured requirements documents with goals and guards
version: "1.0.0"
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

Requirements documents define WHAT the system must do (not HOW). Every criterion must be verifiable — if you can't test it, it's not a criterion.

## When to Use

- After idea intake (Phase 1) produces a clear project concept
- When potato says "write requirements for X"
- Before writing a DESIGN.md (requirements come first)

## Output Location

`requirements.md` in the project root (or specified path).

## Template

```markdown
# Requirements: {Project Name}

## Overview

{One paragraph: what this project does and why it exists. No implementation details.
Include the core user problem being solved.}

## Goals

{Verifiable completion conditions. Each must be testable by a human or automated check.
Group by feature area. Use GOAL-{feature}.{number} format.}

### {Feature Area 1}
- GOAL-1.1 [P0]: {Specific, verifiable condition}
- GOAL-1.2 [P1]: {Specific, verifiable condition}

### {Feature Area 2}
- GOAL-2.1 [P0]: {Specific, verifiable condition}
- GOAL-2.2 [P2]: {Specific, verifiable condition}

## Guards

{System-level properties that must ALWAYS hold, regardless of feature.
These are cross-cutting constraints, not feature-specific.
Think: security, data integrity, performance, reliability.}

- GUARD-1 [hard]: {Property that must never be violated — hard = execution stops}
- GUARD-2 [soft]: {Property that should hold — soft = warn but continue}

## Out of Scope

{Explicit boundaries. What this project does NOT do.
Prevents scope creep and gives sub-agents clear guardrails.}

- {Thing explicitly not included}
- {Thing explicitly not included}

## Dependencies

{External systems, libraries, or preconditions this project requires.
Optional section — include only if relevant.}

- {Dependency and why it's needed}
```

## Priority & Severity

### Goal Priority
- **P0**: Core — required for the system to function at all
- **P1**: Important — needed for production-quality operation
- **P2**: Enhancement — improves efficiency, UX, or observability

### Guard Severity
- **hard**: Violation = system is broken, execution must stop
- **soft**: Violation = degraded quality, should warn but can continue

## Writing Guidelines

### Goals

**Good goals are:**
- **Specific**: "User can list all auth profiles showing name, status, and token prefix" — not "auth works"
- **Verifiable**: Can be checked by running a command, reading output, or running a test
- **Independent**: Each criterion tests one thing. Don't combine multiple conditions.
- **Complete**: Cover all user-facing behaviors. If a feature exists, it has goals.

**Numbering:**
- `GOAL-{feature_number}.{item}` — e.g., GOAL-1.1, GOAL-1.2, GOAL-2.1
- Feature numbers correspond to feature areas (groups)
- This numbering maps directly to `satisfies` references in GID task nodes

**Examples:**
```
✅ GOAL-1.1 [P0]: Running `agentctl auth list` displays all profiles with name, status (active/cooldown), and token prefix (first 8 chars)
✅ GOAL-1.2 [P0]: Running `agentctl auth use <name>` switches the active profile and confirms with "Switched to <name>"
✅ GOAL-2.1 [P1]: Config file is written atomically (temp file + rename) so partial writes never corrupt state

❌ "Auth module works correctly" — not specific
❌ "Good performance" — not verifiable
❌ "Handle errors properly" — not specific or verifiable
```

### Guards (INV)

**Guards are system-wide, not feature-specific:**
- Security: "Auth tokens never appear in logs or error messages"
- Data integrity: "All file writes are atomic (temp + rename)"
- Reliability: "No operation leaves the system in an inconsistent state on crash"
- Performance: "CLI commands respond within 500ms for local operations"

**If it only applies to one feature, it's a criterion, not an invariant.**

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
- If an idea intake (Phase 1) exists, use it as primary input
- `engram recall "{project topic} requirements decisions"` for past context

### Step 2: Draft Requirements
- Write the full document following the template above
- Aim for 5-15 goals (fewer = project is simple, more = might need splitting)
- Aim for 2-5 guards
- At least 3 out-of-scope items

### Step 3: Present for Review
- Show the complete document to potato
- Ask specifically: "Are these goals complete? Anything missing or wrong?"
- Iterate until approved

### Step 4: Save
- Write to `requirements.md` in project root
- Store in engram: `engram add --type factual --importance 0.7 "Requirements written for {project}: {CR count} goals, {INV count} guards"`
- Log in daily memory file

## Traceability

After this document is written, the next phases reference it:
- **DESIGN.md** addresses how to satisfy each CR
- **GID graph** tasks have `satisfies: ["GOAL-1.1", "GOAL-1.2"]` metadata
- **Verification** checks goals after task completion
- **Guards** become cross-cutting test cases

This chain ensures nothing is implemented without a reason, and nothing is required without being implemented.
