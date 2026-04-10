# Review: Issue Full-Cycle Management Requirements (R1)

**Document**: `.gid/features/issue-lifecycle/requirements.md`
**Reviewer**: RustClaw
**Date**: 2026-04-08
**Depth**: Full (Phase 0–6)

---

## 🔴 Critical (blocks implementation)

### FINDING-1: [Check #6] GOAL-4 — `close` phase 的 PhaseKind 不存在 ✅ Applied

GOAL-4 定义 `close` phase 要做：更新 issues-index.md 状态、记录 commit hash、写 daily log + engram。

但现有 `PhaseKind` 只有 4 种：`Skill`、`GidCommand`、`Harness`、`Shell`。`close` phase 需要做 markdown 文件编辑 + engram 写入 + 可能还有 git log 查询——这不是 shell 一行命令能干的事，也不是现有的任何 skill。

**需要明确**：close phase 用哪种 PhaseKind？

选项：
- (a) `Shell` — 写一个 shell 脚本做所有事（脆弱，markdown 编辑在 shell 里很难）
- (b) `Skill` — 写一个 `close-issue` skill，让 LLM 做编辑（消耗 token，但灵活）
- (c) 改 gid-core 加新的 `PhaseKind::Custom` 支持 agent 回调

**Suggested fix**: 明确 close phase 使用 `Skill` kind + 新建 `close-issue` skill。或者说明 close phase 是 ritual 完成后由 agent（而非 ritual engine）执行的 post-hook。

**✅ Applied**: Added to GOAL-4 — close phase uses `PhaseKind::Skill` with new `close-issue` skill. Clarified it's a post-ritual agent action, not a ritual engine phase. Added §4 scope entry for the new skill.

---

### FINDING-2: [Check #7] GOAL-5 — P0 自主修复触发机制未定义 ✅ Applied

GOAL-5 说 "RustClaw 发现 P0 issue 时可以直接启动 issue-fix ritual"，但没有定义**谁来触发**以及**怎么触发**。

当前 ritual 只能通过以下方式启动：
1. 用户发 `/ritual` 命令
2. `start_ritual` 工具调用

RustClaw 在 heartbeat 中发现 P0 → 需要 agent 自己调用 `start_ritual`。但 heartbeat 是一个 system prompt + 简单检查的轻量 session，不走完整的 orchestrator 对话循环。

**问题**：heartbeat session 能否调用 `start_ritual`？如果不能，P0 自主修复在架构上不可行。

**Suggested fix**: 增加一段说明触发路径：heartbeat 检测到 P0 open → 向主 Telegram chat 发送消息触发正常 session → 在该 session 中 agent 调用 `start_ritual`。或者说明 heartbeat 可以直接启动 ritual（需要确认 RustClaw 架构支持）。

**✅ Applied**: Added to GOAL-5 — defined 5-step trigger path: heartbeat detects P0 → sends Telegram message to main chat → triggers normal RustClaw session → agent calls `start_ritual` → ritual completes + notifies potato.

---

### FINDING-3: [Check #17] GOAL-4 — `extends: bugfix` 假设未验证 ✅ Applied

GOAL-4 说 `issue-fix` 可以 `extends: bugfix`，但代码审读发现 **`extends` 字段在 `RitualDefinition` 中定义了但从未被实现**——`template.rs` 中的 `load()` 方法不做 extends 解析/合并。

```rust
// definition.rs
pub struct RitualDefinition {
    pub extends: Option<String>,  // 定义了
    ...
}

// template.rs — load() 直接返回，不处理 extends
fn load_from_file(path: &Path) -> Result<RitualDefinition> {
    let content = std::fs::read_to_string(path)?;
    let def: RitualDefinition = serde_yaml::from_str(&content)?;
    Ok(def)  // 没有 extends resolution
}
```

**Suggested fix**: 两个选项：
- (a) 在 requirements 中删除 `extends: bugfix`，issue-fix 作为独立 template（推荐——3 个 phase 完全独立定义也不复杂）
- (b) 在 GOAL-4 中加 prerequisite：需要先实现 template extends 功能（scope 扩大）

**✅ Applied**: Deleted `extends: bugfix` from GOAL-4. Template is now independently defined with 3 full phases. Updated GUARD-6 to clarify YAML file + independent definition (no extends). No contradiction remains.

---

## 🟡 Important (should fix before starting)

### FINDING-4: [Check #8] 缺少 fix 失败时的 error path ✅ Applied

GOAL-4 定义了 happy path（fix → verify → close），但没有定义：
- fix phase 失败了怎么办？（代码改了但编译不过）
- verify phase 失败了怎么办？（修复引入了新 regression）
- 部分修复（fix 成功但 verify 发现 2/10 tests 仍然失败）

现有 bugfix template 的 `on_failure: FailureStrategy::Escalate`，但 GOAL-5 的 P0 场景是无人值守的——escalate 给谁？

**Suggested fix**: 增加 failure handling 说明：
- fix 失败 → revert changes + 标记 issue 为 `fix-failed` + 通知 potato
- verify 失败 → 保留修改但标记 `verify-failed` + 通知 potato（不自动 revert，因为可能是部分修复）
- P0 无人值守场景 → failure 必须通知 + 不自动重试（避免循环）

**✅ Applied**: Added failure handling section to GOAL-4 — fix fails → revert + mark fix-failed + notify; verify fails → keep changes + mark verify-failed + notify; P0 → no auto-retry. Updated verification standard to include verify-failed case.

---

### FINDING-5: [Check #12] GOAL-2 + GOAL-3 — 自主记录的 "项目归属" 判断未定义 ✅ Applied

GOAL-2 说 "RustClaw 在 heartbeat 中发现测试失败 → 自动写入对应项目的 issues-index.md"，GOAL-3 说自动判断类型和优先级。

但**没有定义 agent 如何判断 issue 属于哪个项目**。比如：
- heartbeat 跑 `cargo test` 在 gid-rs 目录发现失败 → 属于 gid-rs ✅ 明确
- 开发 RustClaw 时发现 engram recall 结果差 → 属于 engramai？属于 RustClaw？两者都有？

**Suggested fix**: 增加一条规则："自主发现的 issue 归属到问题源头所在的代码库。如果涉及多个项目（如集成问题），记录到触发该问题的顶层项目，description 中注明涉及的其他项目。"

**✅ Applied**: Added project attribution rule to GOAL-2 — issues belong to source code project. Integration issues → record to top-level project with cross-reference in description.

---

### FINDING-6: [Check #13] 术语不一致 — "done" vs "closed" ✅ Applied

文档中混用：
- GOAL-4: `状态为 done`
- GOAL-7: `close ISS-003` → `标记 done`
- GOAL-6 示例: `Recently Closed`
- 现有 ISSUES.md 格式: `[closed] ✅`

"done" 和 "closed" 是同一个状态还是两个？现有 ISSUES.md 用 `[closed]`，新格式应该用哪个？

**Suggested fix**: 统一为一个状态名。建议用 `closed`（与现有格式一致），定义 status enum: `open | in_progress | closed | wontfix | blocked`

**✅ Applied**: Unified to `closed` throughout document. GOAL-4 now uses "状态为 closed". GOAL-7 defines status enum: `open | in_progress | closed | wontfix | blocked`. Matches existing ISSUES.md format.

---

### FINDING-7: [Check #15] GUARD-6 与 FINDING-3 冲突 ✅ Applied

GUARD-6 说 "issue-fix ritual template 以 YAML 文件形式存放在 `.gid/rituals/issue-fix.yml`（或作为 builtin）"。但 GOAL-4 说 `extends: bugfix`。

如果做 YAML 文件方案 → extends 不可用（如 FINDING-3 所述）
如果做 builtin 方案 → 不需要 extends（直接在代码里组合 phases）

这两个约束相互矛盾，需要在 design 阶段之前解决。

**Suggested fix**: 二选一并明确：(a) YAML 文件 + 独立定义（不用 extends）；(b) builtin + 可以复用 bugfix phases。推荐 (a)，因为不需要改 gid-core 代码。

**✅ Applied**: Updated GUARD-6 to clarify YAML file + independent definition (no extends mechanism). Removed "(或作为 builtin)" option. Aligns with FINDING-3 fix. No contradiction remains.

---

### FINDING-8: [Check #21] GOAL-6 — "5 秒内返回" 验证标准缺少约束条件 ✅ Applied

"show all issues" 扫描 6 个项目，验证标准说 "5 秒内返回"。但 5 秒取决于：
- 6 个 issues-index.md 的大小（每个 100 issue vs 每个 10 issue）
- 是否需要 LLM 解析 markdown 还是纯文本处理
- 文件系统延迟

如果是纯 `read_file` + 文本格式化（GUARD-2 定义的），6 个文件应该在 1 秒内完成。5 秒太宽松了。

**Suggested fix**: 明确 "6 个项目各 ≤50 open issues 时，扫描 + 格式化 < 2 秒"。或者删掉时间限制（纯文件操作本来就快，没必要指定）。

**✅ Applied**: Changed GOAL-6 verification to "< 2 秒内返回（6 个项目，每个 ≤50 open issues）". More precise constraint based on expected data size.

---

## 🟢 Minor (can fix during implementation)

### FINDING-9: [Check #4] GOAL-7 — "ISS-003 done" 命令歧义 ✅ Applied

`"ISS-003 done"` — 如果有多个项目都有 ISS-003 怎么办？编号是 per-project 的（engram 有 ISS-003，gid-rs 也有 ISS-003）。

**Suggested fix**: 命令应该包含项目标识：`"engram ISS-003 done"` 或者 agent 基于上下文推断（如果当前对话上下文明确在讨论某个项目）。建议在 GOAL-7 中说明："命令中需指定项目名（如 `engram ISS-003 done`），或 agent 根据对话上下文推断项目。歧义时主动询问。"

**✅ Applied**: Added project qualifier section to GOAL-7 — commands should include project name (`"engram ISS-003 closed"`), or agent infers from context and asks when ambiguous.

---

### FINDING-10: [Check #9] 缺少 "issue 关联 commit" 的 non-functional 考虑 ✅ Applied

GOAL-4 和 GOAL-5 要求记录 commit hash，但没有定义：
- 如果 fix 改了代码但没 commit（agent 改完代码没有 git commit 权限/流程）
- 如果一个 fix 涉及多个 commit
- commit hash 从哪来（agent 自动 `git log` 还是手动提供）

**Suggested fix**: 增加说明："fix ritual 的 close phase 自动运行 `git log -1 --format=%H` 获取最新 commit hash。如果修复涉及多个 commit，记录最后一个。如果工作区有未提交修改，close phase 先执行 `git add . && git commit -m 'fix: ISS-NNN ...'`。"——或者明确说 commit 是 agent 的责任，不是 ritual 的责任。

**✅ Applied**: Added to GOAL-4 close phase implementation — runs `git log -1 --format=%H`, or agent commits first if uncommitted changes exist. Clarified commit is agent responsibility during fix phase; close phase only reads hash.

---

### FINDING-11: [Check #22] §5 项目路径硬编码 ✅ Applied

已知项目列表硬编码在 requirements 的 §5 中，并且说 "新项目加入时更新此列表和 skill 中的路径映射"。

这不是 requirements 的问题（已有 NONGOAL-3 说不做自动扫描），但值得在 design 时考虑一个 config 文件存储项目列表而非硬编码在 skill markdown 中。

**Suggested fix**: 无需改 requirements（已在 scope 内），但建议在 design 中把项目列表放到一个 config 文件（如 `.gid/projects.yml`）而非散在 skill 的 markdown 里。

**✅ Applied**: Added design note to §5 — project list should be moved to config file like `.gid/projects.yml` during design phase for easier maintenance and extension.

---

## ✅ Passed Checks

| Check | Description | Result |
|---|---|---|
| #0 | Document size (7 GOALs ≤ 15) | ✅ |
| #1 | Specificity — all GOALs are concrete | ✅ |
| #2 | Testability — all GOALs have verification criteria | ✅ |
| #3 | Measurability — P0/P1/P2 criteria quantified | ✅ |
| #5 | Completeness — actor/trigger/outcome present | ✅ |
| #10 | Boundary conditions (P0/P1/P2 thresholds clear) | ✅ |
| #11 | State transitions (open → in_progress → closed/wontfix/blocked) | ✅ implied |
| #14 | Priority consistency | ✅ |
| #16 | GUARDs vs GOALs alignment | ✅ |
| #18 | External dependencies named (ritual engine, engram, gid-core) | ✅ |
| #19 | Data requirements (issues-index.md format from skill) | ✅ |
| #20 | Migration path (old ISSUES.md → new location) | ✅ |
| #23 | Grouping (Goals → Non-Goals → Guards → Scope) | ✅ |
| #24 | Dependency graph (GOAL-4 needs GOAL-1; GOAL-5 needs GOAL-4) | ✅ implicit |
| #25 | Acceptance criteria present for all GOALs | ✅ |
| #26 | User perspective (GOAL-2, GOAL-6, GOAL-7 user-centric) | ✅ |
| #27 | Success metrics (verification standards are measurable) | ✅ |

---

## Summary

- **Total GOALs**: 7
- **Critical**: 3 (FINDING-1 ✅, FINDING-2 ✅, FINDING-3 ✅) — all resolved
- **Important**: 5 (FINDING-4 ✅, FINDING-5 ✅, FINDING-6 ✅, FINDING-7 ✅, FINDING-8 ✅) — all resolved
- **Minor**: 3 (FINDING-9 ✅, FINDING-10 ✅, FINDING-11 ✅) — all resolved
- **Passed checks**: 17/28

### Changes Applied

**Critical fixes**:
1. FINDING-1: Close phase uses `PhaseKind::Skill` with new `close-issue` skill
2. FINDING-2: P0 trigger path defined (heartbeat → Telegram → normal session → start_ritual)
3. FINDING-3: Deleted `extends: bugfix`, issue-fix is independent YAML template

**Important fixes**:
4. FINDING-4: Added failure handling for fix/verify phases
5. FINDING-5: Added project attribution rule for autonomous issue recording
6. FINDING-6: Unified to `closed` status (not "done")
7. FINDING-7: Resolved GUARD-6 contradiction — YAML file + independent definition
8. FINDING-8: Changed verification to "< 2 秒" with explicit constraints

**Minor fixes**:
9. FINDING-9: Added project qualifier to commands
10. FINDING-10: Added commit hash handling (git log + agent commits if needed)
11. FINDING-11: Added design note for config file

### Recommendation
**✅ Ready for design.** All 11 findings applied. The 3 critical architecture-level questions are resolved:
- Close phase mechanism: Skill-based post-ritual action
- P0 trigger: Heartbeat → message → normal session flow
- Template structure: Independent YAML definition (no extends)
