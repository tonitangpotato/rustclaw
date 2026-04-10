# Requirements: Issue Full-Cycle Management

> Feature: End-to-end issue lifecycle — from discovery to resolution, across all projects.
> Location: RustClaw workspace (skills + ritual config + heartbeat integration)

---

## Context

Issue 管理目前是碎片化的：engram 和 gid-rs 用根目录 `ISSUES.md`（旧格式），`project-issues` skill 只管 create，没有 lifecycle 状态机，没有 cross-project 视图，没有心跳集成。

本 feature 的目标：**让 issue 从发现到关闭形成完整闭环，最小化人为干预。**

---

## §1 Goals

### GOAL-1: 统一 Issue 存储格式

所有项目的 issue 使用统一格式和位置：
- 索引文件：`{project_root}/.gid/docs/issues-index.md`
- 工作目录：`{project_root}/.gid/issues/ISS-{NNN}/`（复杂 issue 按需创建）
- 已有旧格式（根目录 `ISSUES.md`）必须迁移到新位置
- 迁移后旧文件替换为指向新位置的 redirect notice，不删除

**验证标准**：engram-ai-rust 和 gid-rs 的现有 issue 全部出现在新位置的 issues-index.md 中，内容完整，编号保留。

### GOAL-2: 双入口 Issue 记录

Issue 可以通过两种方式进入系统：
- **人工入口**：potato 通过消息告诉 RustClaw（`"issue: XXX"`, `"bug: XXX"`, `"发现一个问题"` 等）
- **自主入口**：RustClaw 在开发、heartbeat 检查、测试运行等过程中自主发现问题并记录

两种入口使用相同的记录流程，写入相同的存储位置。自主记录时 `发现者` 字段标注 `RustClaw`。

**Issue 项目归属规则**：自主发现的 issue 归属到问题源头所在的源代码项目。如果涉及多个项目的集成问题（如 RustClaw 调用 engram 时发现问题），记录到触发该问题的顶层项目（RustClaw），在 description 中注明涉及的其他项目并添加 cross-reference。

**验证标准**：(a) potato 说 "issue: engram recall 太慢" → 正确写入 engram 项目的 issues-index.md；(b) RustClaw heartbeat 发现测试失败 → 自动写入对应项目的 issues-index.md，无需人工触发。

### GOAL-3: 记录时自动分类与优先级

Issue 记录的同时完成分类，不作为独立步骤：
- **类型**自动判断：bug / improvement / performance / ux / missing / debt
- **优先级**自动判断，标准明确：
  - P0：数据丢失、功能完全不可用、安全漏洞
  - P1：功能有 bug 但有 workaround、性能严重退化
  - P2：改进、小瑕疵、技术债
- 优先级可以被 potato 事后覆盖

**验证标准**：记录 10 个不同严重程度的 issue，自动分配的优先级至少 8/10 符合预期。

### GOAL-4: Issue Ritual Template（轻量修复流水线）

提供名为 `issue-fix` 的 ritual template，phases：
- `fix`：执行代码修改（Harness，Opus model）
- `verify`：运行项目测试命令确认修复不引入回归
- `close`：更新 issues-index.md 状态为 closed，记录修复日期、commit hash、修复摘要；同步写入 daily log + engram

**触发方式**：`"fix ISS-003"` 或 `/ritual --template issue-fix "fix ISS-003 in engram"`

**Template 定义**：issue-fix 是独立定义的 YAML template，包含完整的 3 个 phase 定义（不使用 extends）。

**Close phase 实现**：close phase 使用 `PhaseKind::Skill`，调用新的 `close-issue` skill。该 skill 负责：
- 编辑 issues-index.md（更新状态为 closed + 记录修复日期）
- 获取 commit hash（运行 `git log -1 --format=%H`，如有未提交修改则先提交）
- 写入 daily log 条目
- 写入 engram 记忆

Close phase 是 ritual 完成后的 post-ritual agent action，而非 ritual engine 直接管理的 phase。

**Failure handling**：
- fix phase 失败 → revert changes + 标记 issue 为 `fix-failed` + 通知 potato
- verify phase 失败 → 保留修改 + 标记 issue 为 `verify-failed` + 通知 potato（不自动 revert，可能是部分修复）
- P0 issue 修复失败 → 必须通知 + 不自动重试（避免循环）

**验证标准**：(a) 通过 ritual 触发修复一个 P1 issue，全流程自动完成（fix → verify → close）；(b) 修复完成后 issues-index.md 状态更新为 closed，包含修复日期和 commit hash；(c) verify 失败时不自动 revert 但正确标记 verify-failed 状态。

### GOAL-5: P0 自主修复权限

RustClaw 发现或被告知 P0 issue 时：
- 可以**不等 potato 确认**直接启动 issue-fix ritual
- 修复全过程必须完整记录：改了哪些文件、修复思路、commit hash、测试结果
- 修复完成后立即通知 potato（Telegram 消息），包含完整修复报告
- P1/P2 issue 不自动修复，等 potato 手动触发或心跳报告后决定

**P0 自主修复触发路径**：
1. Heartbeat 检测到 P0 open issue（测试失败、关键功能故障等）
2. Heartbeat 向主 Telegram chat 发送消息（"发现 P0 issue: ISS-NNN，需要立即修复"）
3. 消息触发正常 RustClaw session
4. 在该 session 中 agent 调用 `start_ritual` 工具启动 issue-fix ritual
5. Ritual 完成后通知 potato

这个流程确保 P0 修复在完整的 agent context 中进行，而非在 heartbeat 的简化 session 中。

**验证标准**：RustClaw 发现一个 P0 issue（如测试全部失败），自动记录 + 启动修复 + 完成后通知，无需人工介入。通知内容包含 diff 摘要和测试结果。

### GOAL-6: Cross-Project Issue Dashboard

提供统一视图查看所有已知项目的 issue 状态：
- **手动触发**：potato 说 `"show all issues"` / `"所有项目的 issue"` → 扫描所有已知项目的 `.gid/docs/issues-index.md`，按优先级 × 状态汇总输出
- **心跳自动报告**：heartbeat 检查时扫描所有项目，如果有 P0 open 的 issue → 在心跳报告中高亮提醒
- 扫描是 read_file 操作，不消耗 LLM token

**输出格式**（Telegram 友好，用 bullet list 不用 table）：

```
📋 **Issue Dashboard** (2026-04-08)

🔴 **P0 Open** (1)
• engram ISS-001: consolidate DB corruption — open since 03-29

🟡 **P1 Open** (3)
• engram ISS-002: recall recency bias — open since 04-05
• engram ISS-003: memory extractor 问题 — open since 03-31
• gid-rs ISS-003: semantify 路径启发式 — open since 04-05

🟢 **P2 Open** (1)
• gid-rs ISS-005: ... — open since ...

✅ **Recently Closed** (last 7 days)
• engram ISS-010: embedding model_id 格式不一致 — closed 04-08
```

**验证标准**：`"show all issues"` 正确扫描 engram + gid-rs + RustClaw 三个项目，输出格式清晰，P0 排最前，< 2 秒内返回（6 个项目，每个 ≤50 open issues）。

### GOAL-7: Issue 状态手动管理命令

支持以下手动状态变更命令：
- `"ISS-003 closed"` / `"close ISS-003"` → 标记 closed，记录关闭日期
- `"ISS-003 wontfix: 原因"` → 标记 wontfix，记录原因
- `"ISS-003 blocked by ISS-001"` → 标记 blocked，记录阻塞原因
- `"ISS-003 P0"` → 修改优先级
- 所有状态变更同步写入 daily log + engram

**状态枚举定义**：`open | in_progress | closed | wontfix | blocked`（与现有 ISSUES.md 格式统一）

**项目限定**：命令中需指定项目名（如 `"engram ISS-003 closed"`），或 agent 根据对话上下文推断项目。当存在歧义时（多个项目都有同编号 issue），主动询问 potato。

**验证标准**：每种命令至少成功执行一次，issues-index.md 中状态正确更新，daily log 有对应记录。

---

## §2 Non-Goals

- **NONGOAL-1**：不做 GitHub/GitLab issue 同步。这是内部工作流，不需要和外部 issue tracker 集成。
- **NONGOAL-2**：不做 issue 模板自定义。使用 `project-issues` skill 中定义的固定格式。
- **NONGOAL-3**：不做 issue 的自动发现扫描（如定期扫描代码里的 TODO/FIXME）。自主记录依赖于 agent 在工作过程中的判断，不是定时任务。
- **NONGOAL-4**：不做 issue 之间的复杂依赖图。`blocked by` 是简单标注，不做传递性 block 分析。
- **NONGOAL-5**：不做 issue 统计报表（burn-down chart 等）。Dashboard 够用。

---

## §3 Guards (约束)

### GUARD-1: 数据完整性
迁移旧格式时不得丢失任何 issue 内容。旧文件保留 redirect notice，不删除。

### GUARD-2: 零额外 token 消耗
Dashboard 扫描和心跳 issue 检查通过 `read_file` 实现，不调用 LLM。分类使用 agent 当前 session 的推理能力，不额外调用 API。

### GUARD-3: 已有 project-issues skill 格式兼容
新 lifecycle 功能是对现有 skill 的扩展，不破坏已有格式。issues-index.md 格式沿用 skill 中定义的结构。

### GUARD-4: 跨项目路径安全
Dashboard 扫描只读取已知项目列表中的路径（在 skill 中硬编码）。不递归扫描文件系统。

### GUARD-5: P0 自主修复必须可追溯
每次自主修复必须产出：(a) issues-index.md 更新, (b) daily log 条目, (c) engram 记忆, (d) commit hash, (e) Telegram 通知。缺任何一项视为修复不完整。

### GUARD-6: Ritual template 定义方式
issue-fix ritual template 以 YAML 文件形式存放在 `.gid/rituals/issue-fix.yml`，作为独立完整定义（不使用 extends 机制）。Template 包含 3 个完整定义的 phases（fix, verify, close），不硬编码在 Rust 源代码中。

---

## §4 Scope

### 需要改动的组件

1. **`project-issues` skill**（`skills/project-issues/SKILL.md`）
   - 扩展 trigger patterns 支持 lifecycle 命令（fix, close, wontfix, show all issues）
   - 增加状态变更流程步骤
   - 增加 dashboard 扫描步骤

2. **Ritual template**
   - 新增 `issue-fix` template（YAML 独立定义）
   - 3 phases: fix → verify → close
   - close phase 调用新的 `close-issue` skill

2.1. **close-issue skill**（新增）
   - 编辑 issues-index.md 更新状态
   - 获取 commit hash（`git log -1 --format=%H`，如有未提交修改则先 commit）
   - 写入 daily log + engram
   - Commit 是 agent 在 fix phase 中的责任；close phase 只读取最新 commit hash

3. **Heartbeat 集成**
   - 心跳检查增加 P0 open issue 扫描
   - HEARTBEAT.md 或心跳逻辑中加入 issue 检查步骤

4. **旧数据迁移**（一次性）
   - engram-ai-rust `ISSUES.md` → `.gid/docs/issues-index.md`
   - gid-rs `ISSUES.md` → `.gid/docs/issues-index.md`

### 不需要改动的组件
- gid-core Rust 源代码（除非选择 builtin template 方案）
- RustClaw Rust 源代码（ritual runner 已支持自定义 template）
- 其他 skills

---

## §5 Known Project Paths

Dashboard 扫描的项目列表：
- **RustClaw**: `/Users/potato/rustclaw/`
- **engramai**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **gid-rs**: `/Users/potato/clawd/projects/gid-rs/`
- **agentctl**: `/Users/potato/clawd/projects/agentctl/`
- **xinfluencer**: `/Users/potato/clawd/projects/xinfluencer/`
- **infomap-rs**: `/Users/potato/clawd/projects/infomap-rs/`

新项目加入时更新此列表和 skill 中的路径映射。

**设计说明**：项目列表应在 design 阶段考虑移至配置文件（如 `.gid/projects.yml`），而非硬编码在 skill markdown 中，以便于维护和扩展。
