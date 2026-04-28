# ISS-051 Graph Update Summary

## Issue
RitualRunner::run_skill bypasses gid-core V2Executor, causing file_snapshot
post-condition to never run. Full forensic detail in `.gid/issues/ISS-051/issue.md`.

## Root Cause
The RitualRunner has duplicate execution logic that doesn't go through V2Executor's
pipeline, which means post-conditions like file_snapshot are never triggered.

## Solution
Refactor RitualRunner::run_skill to delegate to V2Executor::run_skill, moving the
self-review loop and snapshot management into v2_executor where they belong.

## Tasks Added to Graph

Convention follows existing graph.yml pattern (e.g. ISS-029 / ISS-029-N): plain
`ISS-NNN` IDs, `node_kind: Task`, and `subtask_of` edges from child → parent.

### Main Task
- **ISS-051**: Parent task tracking the overall issue.

### Subtasks
1. **ISS-051-1**: Refactor RitualRunner::run_skill to delegate to V2Executor.
   - Remove duplicate execution logic from RitualRunner::run_skill
   - Replace with delegation to V2Executor::run_skill
   - Ensures all skill execution goes through proper v2_executor pipeline

2. **ISS-051-2**: Move self-review loop into V2Executor.
   - Extract self-review loop logic from RitualRunner
   - Integrate into V2Executor::run_skill
   - Centralizes review logic to work with post-conditions

3. **ISS-051-3**: Move snapshot management into V2Executor.
   - Move snapshot creation/management from RitualRunner to V2Executor
   - Ensures snapshots created at correct time in execution pipeline

## Edges
Each subtask has a `subtask_of` edge pointing to ISS-051, matching the established
convention used by ISS-029-1..4 → ISS-029.

## Expected Outcome
After this refactoring:
- Single execution path through V2Executor
- Post-conditions (including file_snapshot) will run correctly
- Self-review loop properly integrated
- Snapshot management centralized
- No duplicate execution logic
