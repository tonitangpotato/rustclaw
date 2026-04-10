---
name: draft-requirements
description: Draft structured requirements documents with goals and guards from discussion docs
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

**⚠️ The #1 failure mode is writing implementation details disguised as requirements.** This is especially common for infrastructure/framework features where WHAT and HOW feel intertwined. See "The WHAT/HOW Boundary" section below — read it before writing any GOAL.

## When to Use

- After idea intake (Phase 1) produces a clear project concept
- When extracting requirements from an existing design document
- When potato says "write requirements for X"
- Before writing a DESIGN.md (requirements come first), OR after design exists (extract from it)

## Document Size Rule: Feature-Level Splitting

**A single requirements document MUST NOT exceed ~15 GOALs.** If it does, split into feature-level documents.

**Why:** Review agents run 27+ checks across all GOALs. At 30+ GOALs, review quality degrades (context pressure, missed cross-references). Splitting at the source is the root fix — no need for complex multi-agent review architectures.

**Structure for large projects:**

```
.gid/
├── docs/
│   └── requirements.md          ← Master: overview + feature index + GUARDs only
└── features/
    ├── auth/requirements.md     ← 10-15 GOALs for auth
    ├── pipeline/requirements.md ← 10-15 GOALs for pipeline
    └── cli/requirements.md      ← 10-15 GOALs for CLI
```

- **Master requirements.md (in `.gid/docs/`) contains:**
- Project overview
- Feature index with brief descriptions and references to feature docs
- **GUARDs only** (cross-cutting constraints apply to all features)
- Out of Scope section
- NO GOALs — all GOALs live in feature-level docs

**Each feature requirements.md contains:**
- Feature overview (1 paragraph)
- GOALs for that feature only (10-15 max)
- Feature-specific dependencies
- Reference back to master for GUARDs

**When to split:**
- Writing a new project with >15 GOALs → split upfront
- Existing document growing past 15 GOALs → refactor into features
- If a single feature has 15+ GOALs → that feature should be split further

**GOAL numbering across features:**
- Each feature has its own namespace: `GOAL-auth.1`, `GOAL-pipe.1`, etc.
- OR use module-number format: `GOAL-1.1` (module 1), `GOAL-2.1` (module 2) — each feature doc owns a module number range

## Output Location

Depends on project structure:
- **Single feature (≤15 GOALs):** `.gid/features/{feature-name}/requirements.md`
- **Multi-feature project (>15 GOALs total):** Master at `.gid/docs/requirements.md` (GUARDs + index) + features at `.gid/features/{feature-name}/requirements.md`

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

**This is the #1 source of requirements bugs.** Three rounds of review can't fix a document that mixes requirements with design decisions — the contradictions are structural, not editorial.

Requirements describe **observable behavior from outside the system**. Design describes **how the system achieves that behavior internally**.

### The Substitution Test

> **If you could achieve the same observable result with a completely different implementation, the requirement is valid. If the requirement specifies the implementation itself, it belongs in DESIGN.md.**

Ask: "Could someone satisfy this GOAL using a different internal approach?" If yes → good requirement. If the GOAL *dictates* the approach → it's a design decision.

### ❌ Implementation details (belong in DESIGN.md)

```
BAD:  "Detects cycles using Kahn's algorithm"
      → Specifies the algorithm. Requirement = detect cycles. How = design.

BAD:  "Code nodes use `node_type = "code"` and `node_kind` for precise type"
      → Specifies field names and schema layout. Requirement = code nodes are distinguishable from task nodes. How fields are named = design.

BAD:  "Layer is derived from `source == "extract"`, not from `node_type`"
      → Specifies the derivation mechanism. Requirement = layers are deterministic. Which field drives it = design.

BAD:  "Bridge edges use `edge.metadata["source"] = "extract"`"
      → Specifies schema. Requirement = code-to-feature connections are queryable. What the edge looks like internally = design.

BAD:  "Uses SQLite for persistence"
      → Specifies technology. Requirement = state persists across crashes. Storage backend = design.

BAD:  "Implements retry with exponential backoff"
      → Specifies algorithm. Requirement = failed operations are retried before giving up. Retry strategy = design.

BAD:  "Merges are serialized via mutex/lock"
      → Specifies concurrency mechanism. Requirement = concurrent merges don't corrupt data. How = design.
```

### ✅ Observable behavior (belongs in requirements)

```
GOOD: "Detects dependency cycles and rejects cyclic graphs with cycle path in error message"
GOOD: "Code nodes are distinguishable from project nodes in all query and display operations"
GOOD: "Node layer classification is deterministic — same input always produces same layer"
GOOD: "Code-to-feature connections are queryable in both directions (feature→code, code→feature)"
GOOD: "Execution state persists across process crashes — no data loss on restart"
GOOD: "Failed tasks are retried with enhanced context before marking as blocked"
GOOD: "Concurrent graph modifications never corrupt data or lose writes"
```

### 🔥 The Infrastructure Trap

**Infrastructure and framework features are where this boundary is hardest.** When the "product" is a schema, an API, or a data pipeline, it feels like the schema IS the requirement. It's not.

Examples from a graph engine:

| ❌ Disguised design decision | ✅ Actual requirement |
|---|---|
| "Node struct has `source`, `node_type`, `node_kind` fields" | "Each node carries enough metadata to determine its origin (extract vs manual) and category (code vs task)" |
| "`ready_tasks()` filters by `status == Todo && source != extract`" | "Task readiness queries never return code-only nodes" |
| "Edge relations include `BelongsTo`, `TestsFor`, `Implements`" | "The graph supports structural (containment), testing, and implementation relationships between nodes" |
| "FTS5 index covers `title` and `description` columns" | "Full-text search covers node titles and descriptions" |

**The pattern:** Strip field names, function signatures, enum variants, and schema details. What's left is the requirement. The stripped parts go to DESIGN.md.

## What Is NOT a Requirement

Some statements look like requirements but aren't verifiable functional behaviors:

| Not a requirement | What it actually is | Where it belongs |
|---|---|---|
| "Task descriptions must be self-contained" | Writing guideline for graph creation | Skill template / gid_design prompt |
| "gidterm's backend is not used" | Architecture decision | DESIGN.md |
| "Use Opus for complex tasks" | Configuration preference | execution.yml |
| "Follow the 7-phase pipeline" | Process description | DESIGN.md overview |
| "`node_type` is for coarse layer, `node_kind` for precise type" | Field-level schema design | DESIGN.md |
| "deprecated `code_node_to_task_id()` is not called" | Implementation cleanup detail | Task description |

**Test:** Can a test or human verify this by observing **system output/behavior** (not by reading source code)? If you'd need to read the struct definition to verify it → it's not a requirement.

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
- [ ] **WHAT not HOW**: No algorithms, data structures, field names, schema layouts, or implementation patterns in requirement text. Run the Substitution Test on every GOAL: "Could this be satisfied with a different implementation?" If the GOAL dictates the implementation → rewrite it.
- [ ] **No infrastructure trap**: For framework/infra features, verify GOALs describe observable behavior, not internal schema. Strip field names and function signatures — what's left should still be a valid requirement.
- [ ] **All functional**: Every GOAL describes observable/testable behavior (not guidelines, architecture decisions, or preferences)
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
- **Count GOALs first.** If total exceeds 15 → split into feature-level docs immediately
- Small project: single doc, ≤15 GOALs
- Large project: master doc (GUARDs + feature index) + feature docs (10-15 GOALs each)
- Aim for 3-10 GUARDs (cross-cutting only, always in master doc)
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
- **GID graph** feature nodes link to their requirements/design docs via metadata
- **GID graph** task nodes have `satisfies: ["GOAL-1.1", "GOAL-1.2"]` metadata
- **Verification** checks goals after task completion
- **Guards** become cross-cutting test cases or checkpoint constraints

### Feature Node ↔ Document Mapping

When the Graph phase generates feature nodes, each feature node MUST include metadata linking to its documents:

```yaml
- id: feat-auth
  title: Authentication
  type: feature
  metadata:
    requirements_doc: ".gid/features/auth/requirements.md"
    design_doc: ".gid/features/auth/design.md"
    goal_prefix: "GOAL-1"  # namespace for this feature's GOALs
```

Task nodes under a feature MUST reference their parent and the GOALs they satisfy:

```yaml
- id: task-auth-profile-list
  title: "Implement auth profile listing"
  type: task
  metadata:
    parent_feature: "feat-auth"
    satisfies: ["GOAL-1.1", "GOAL-1.2"]
```

This chain ensures nothing is implemented without a reason, and nothing is required without being implemented.
