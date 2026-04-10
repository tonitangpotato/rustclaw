# Applied Changes: Requirements Review R1

**Date**: 2026-04-08
**Document**: `.gid/features/issue-lifecycle/requirements.md`
**Review**: `.gid/features/issue-lifecycle/reviews/requirements-r1.md`
**Status**: ✅ All 11 findings applied

---

## Applied Changes Summary

### FINDING-1 ✅ (Critical)
**Section**: GOAL-4
**Change**: Clarified close phase implementation
- Added: close phase uses `PhaseKind::Skill` with new `close-issue` skill
- Added: Clarification that it's a post-ritual agent action, not ritual engine phase
- Added: New §4.2.1 scope entry for `close-issue` skill with detailed responsibilities

### FINDING-2 ✅ (Critical)
**Section**: GOAL-5
**Change**: Defined P0 自主修复触发路径
- Added: 5-step trigger mechanism:
  1. Heartbeat detects P0 open issue
  2. Sends Telegram message to main chat
  3. Triggers normal RustClaw session
  4. Agent calls `start_ritual` in that session
  5. Ritual completes + notifies potato
- Clarification: P0 fix runs in full agent context, not in heartbeat's simplified session

### FINDING-3 ✅ (Critical)
**Section**: GOAL-4, GUARD-6
**Change**: Removed `extends: bugfix` assumption
- Deleted: "可以 `extends: bugfix` 并追加 close phase" from GOAL-4
- Changed: "与现有 bugfix template 的关系" → "Template 定义"
- Added: "issue-fix 是独立定义的 YAML template，包含完整的 3 个 phase 定义（不使用 extends）"
- Updated GUARD-6: Changed from "(或作为 builtin)" to "作为独立完整定义（不使用 extends 机制）"

### FINDING-4 ✅ (Important)
**Section**: GOAL-4
**Change**: Added failure handling
- Added complete **Failure handling** section:
  - fix phase fails → revert changes + mark `fix-failed` + notify potato
  - verify phase fails → keep changes + mark `verify-failed` + notify (no auto-revert, might be partial fix)
  - P0 fix fails → must notify + no auto-retry (avoid loops)
- Updated verification standard to include verify-failed case

### FINDING-5 ✅ (Important)
**Section**: GOAL-2
**Change**: Added project attribution rule
- Added **Issue 项目归属规则**: Issues belong to source code project
- Integration issues → record to top-level project with cross-reference in description
- Example: RustClaw using engram → record to RustClaw, note engram in description

### FINDING-6 ✅ (Important)
**Section**: GOAL-4, GOAL-6, GOAL-7
**Change**: Unified terminology to `closed`
- Changed all instances of "done" to "closed"
- GOAL-4: "状态为 closed" (was "状态为 done")
- GOAL-6 example: "closed 04-08" (was "done 04-08")
- GOAL-7: Added **状态枚举定义**: `open | in_progress | closed | wontfix | blocked`

### FINDING-7 ✅ (Important)
**Section**: GUARD-6
**Change**: Resolved contradiction with FINDING-3
- Changed from "以 YAML 文件形式存放在 `.gid/rituals/issue-fix.yml`（或作为 builtin）"
- To: "以 YAML 文件形式存放在 `.gid/rituals/issue-fix.yml`，作为独立完整定义（不使用 extends 机制）"
- Removed builtin option
- Clarified template contains 3 fully defined phases, not hardcoded in Rust

### FINDING-8 ✅ (Important)
**Section**: GOAL-6
**Change**: Made verification standard more precise
- Changed from "5 秒内返回"
- To: "< 2 秒内返回（6 个项目，每个 ≤50 open issues）"
- Added explicit constraint on data size

### FINDING-9 ✅ (Minor)
**Section**: GOAL-7
**Change**: Added project qualifier handling
- Added **项目限定** section:
  - Commands should include project name: `"engram ISS-003 closed"`
  - Or agent infers from conversation context
  - When ambiguous (multiple projects have same ISS number), agent asks potato

### FINDING-10 ✅ (Minor)
**Section**: GOAL-4, §4.2.1
**Change**: Added commit hash handling details
- Added to close phase implementation:
  - Runs `git log -1 --format=%H` to get latest commit hash
  - If uncommitted changes exist, agent commits first
- Added to §4.2.1: "Commit 是 agent 在 fix phase 中的责任；close phase 只读取最新 commit hash"

### FINDING-11 ✅ (Minor)
**Section**: §5
**Change**: Added design note for config file
- Added **设计说明**: "项目列表应在 design 阶段考虑移至配置文件（如 `.gid/projects.yml`），而非硬编码在 skill markdown 中，以便于维护和扩展。"

---

## Summary Statistics

- **Total findings**: 11
- **Applied**: 11 (100%)
- **Skipped**: 0
- **Critical fixed**: 3/3
- **Important fixed**: 5/5
- **Minor fixed**: 3/3

---

## Document Status

**Before**: Requirements had 3 critical architecture-level questions unresolved
**After**: All architecture decisions made, ready for design phase

### Key Decisions Made

1. **Close phase mechanism**: Skill-based (`close-issue` skill), post-ritual agent action
2. **P0 trigger path**: Heartbeat → Telegram message → normal session → start_ritual
3. **Template structure**: Independent YAML definition (no extends, 3 full phases)
4. **Status terminology**: Unified to `closed` (matching existing ISSUES.md format)
5. **Failure handling**: Defined for both fix and verify phases
6. **Project attribution**: Source code project owns the issue
7. **Commit responsibility**: Agent commits during fix phase; close phase reads hash

### Next Steps

✅ **Ready for design phase**

All blocking issues resolved. Design can proceed with:
- `issue-fix.yml` ritual template definition
- `close-issue` skill specification
- Heartbeat integration details
- Dashboard implementation plan
