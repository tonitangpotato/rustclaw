# Design: Issue Full-Cycle Management

> Based on: `.gid/features/issue-lifecycle/requirements.md` (R1 review-clean)
> Feature: End-to-end issue lifecycle across all projects

---

## §1 Architecture Overview

本 feature 是 **纯 agent 层实现**——所有逻辑在 skill markdown + heartbeat 规则中完成，不需要改 Rust 源代码。

```
┌─────────────────────────────────────────────────────┐
│                    Agent Session                     │
│                                                      │
│  ┌──────────────┐  ┌─────────────────────────────┐  │
│  │ project-     │  │ issue-fix skill (new)       │  │
│  │ issues skill │  │ fix → commit → verify →     │  │
│  │ (v3.0)       │  │ close → log (all-in-one)    │  │
│  └──────┬───────┘  └──────────────┬──────────────┘  │
│         │                         │                  │
│         ▼                         ▼                  │
│  ┌─────────────────────────────────────────────────┐ │
│  │              Shared State (files)               │ │
│  │  {project}/.gid/docs/issues-index.md            │ │
│  │  memory/YYYY-MM-DD.md (daily log)               │ │
│  │  engram-memory.db                               │ │
│  └─────────────────────────────────────────────────┘ │
│                                                      │
│  ┌──────────────────┐  ┌────────────────────┐       │
│  │ Heartbeat Logic  │  │ .gid/projects.yml  │       │
│  │ (P0 scan + fix)  │  │ (project registry) │       │
│  └──────────────────┘  └────────────────────┘       │
└─────────────────────────────────────────────────────┘
```

**关键设计决策**：不引入新的 Rust 代码。RustClaw 的 skill 机制已经足够表达整个流程。Issue 修复使用 skill-based workflow（agent 按步骤执行），不走 ritual engine——ritual 是为多阶段 feature 开发设计的，对单 bug 修复是 overkill。

---

## §2 Components

### §2.1 Project Registry（`.gid/projects.yml`）

**解决 FINDING-11**：项目路径不硬编码在 skill 中，改为统一配置文件。

```yaml
# .gid/projects.yml
projects:
  rustclaw:
    path: /Users/potato/rustclaw
    display_name: RustClaw
  engramai:
    path: /Users/potato/clawd/projects/engram-ai-rust
    display_name: engramai
  gid-rs:
    path: /Users/potato/clawd/projects/gid-rs
    display_name: gid-rs
  agentctl:
    path: /Users/potato/clawd/projects/agentctl
    display_name: agentctl
  xinfluencer:
    path: /Users/potato/clawd/projects/xinfluencer
    display_name: xinfluencer
  infomap-rs:
    path: /Users/potato/clawd/projects/infomap-rs
    display_name: infomap-rs
```

Skills 通过 `read_file(".gid/projects.yml")` 加载项目列表。新项目只需编辑此文件。

**注意**：`verify_command` 不在此文件中定义——各项目的 verify_command 已在各自的 `.gid/config.yml` 中配置（ritual runner 的现有机制）。如果项目没有 `.gid/config.yml`，修复流程使用语言默认检测（Cargo.toml → `cargo test`，package.json → `npm test`）。单一来源，不重复配置。

### §2.2 issues-index.md 格式

沿用现有 `project-issues` skill 格式，增加 lifecycle 字段：

```markdown
# Issues: {project_name}

> 格式: ISS-{NNN} [{type}] [{priority}] [{status}]
> Status: open | in_progress | closed | wontfix | blocked

---

## ISS-001 [bug] [P0] [closed]
**标题**: consolidate DB corruption
**发现日期**: 2026-03-29
**发现者**: RustClaw (heartbeat)
**状态**: closed
**关闭日期**: 2026-04-02
**修复 commit**: a1b2c3d
**修复摘要**: Fixed FK pragma inside transaction; rebuilt FTS5 index.

**描述**: Engram consolidate 命令导致 DB corruption，FTS5 索引损坏。

**跨项目引用**: 也影响 RustClaw (session recall)

---

## ISS-002 [performance] [P1] [open]
**标题**: recall recency bias
**发现日期**: 2026-04-05
**发现者**: potato
**状态**: open

**描述**: Engram recall 结果过度偏向最近的记忆，老记忆即使相关也排不上来。

---
```

**与旧格式的差异**：
- 新增：`发现者`、`关闭日期`、`修复 commit`、`修复摘要`、`跨项目引用` 字段
- header 增加 status 枚举说明
- 其余字段与现有 `project-issues` skill 定义的格式完全兼容

### §2.3 project-issues skill v3.0

现有 v2.0 只处理 issue 创建。v3.0 扩展为完整 lifecycle 管理。

**⚠️ 写入路径变更**：v3.0 将写入路径从 `{project_root}/ISSUES.md`（根目录）改为 `{project_root}/.gid/docs/issues-index.md`。现有 `project-issues` skill 中的写入路径需同步更新。

**新增 trigger patterns**：
```yaml
triggers:
  patterns:
    # 原有
    - "issue:"
    - "bug:"
    - "improvement:"
    - "问题:"
    - "发现一个"
    # 新增 - lifecycle
    - "fix ISS-"
    - "close ISS-"
    - "ISS-\\d+ closed"
    - "ISS-\\d+ wontfix"
    - "ISS-\\d+ blocked"
    - "show all issues"
    - "所有.*issue"
    - "issue dashboard"
  keywords:
    # 原有
    - "issue"
    - "bug"
    - "问题"
    # 新增
    - "fix"
    - "close"
    - "wontfix"
    - "dashboard"
```

**新增流程步骤**：

#### Step A: Issue 状态变更

当匹配到 `close ISS-NNN` / `ISS-NNN wontfix` / `ISS-NNN blocked by` / `ISS-NNN P0` 等命令时：

1. **项目解析**：从命令中提取项目名（如 `"engram ISS-003 closed"`）。如未指定，检查当前对话上下文是否有明确项目。如有歧义（多项目存在同编号），询问 potato。
2. **读取** `{project_path}/.gid/docs/issues-index.md`
3. **用 edit_file 修改** issue 的 `**状态**` 行和相关字段：
   - `closed` → 增加 `**关闭日期**` 行
   - `wontfix` → 增加 `**关闭日期**` + `**Wontfix 原因**` 行
   - `blocked` → 增加 `**Blocked by**` 行
   - 优先级变更 → 修改 header 行的 `[P?]` 标记
4. **双写**：daily log 记录 `## Issue 状态变更: {project} ISS-NNN → {new_status}`，engram store

#### Step B: Dashboard 扫描

当匹配到 `show all issues` / `issue dashboard` / `所有.*issue`：

1. **读取** `.gid/projects.yml` 获取项目列表
2. **逐项目** `read_file("{path}/.gid/docs/issues-index.md")`，跳过不存在的文件
3. **解析**每个 issue 的 header 行提取：编号、类型、优先级、状态、标题、日期
4. **按优先级分组**，按状态过滤（默认只显示 open + in_progress + 最近 7 天 closed）
5. **格式化输出**：Telegram bullet list 格式（见 GOAL-6 示例）

**解析错误处理**：如果某个项目的 issues-index.md 格式异常（手动编辑导致 regex 解析失败），跳过该项目并在输出中标注：`"⚠️ {project}: issues-index.md 格式异常，跳过"`。不中断其他项目的扫描。

**解析逻辑**（pure regex，不需要 LLM）：
```
Header: ## ISS-(\d+) \[(\w+)\] \[(P\d)\] \[(\w+)\]
Title:  \*\*标题\*\*: (.+)
Date:   \*\*发现日期\*\*: (\d{4}-\d{2}-\d{2})
Close:  \*\*关闭日期\*\*: (\d{4}-\d{2}-\d{2})
```

这是字符串处理，agent 在 exec 中用 grep/awk 或直接在 LLM 推理中做。零额外 API call。

#### Step C: Fix 触发

当匹配到 `fix ISS-NNN`：

1. **解析** issue 编号和项目（同 Step A 的项目解析逻辑）
2. **读取** issue 内容，确认状态为 open 或 in_progress
3. **触发 issue-fix skill**：agent 将 issue 上下文传入 issue-fix skill workflow（见 §2.4）

### §2.4 issue-fix Skill（综合修复 workflow）

**文件位置**：`skills/issue-fix/SKILL.md`

**设计决策**：issue 修复使用 **skill-based workflow**，不走 ritual engine。理由：
- RustClaw 的 `RitualPhase` 是硬编码枚举（Idle → Triaging → ... → Verifying → Done），不支持自定义 YAML template
- Ritual engine 是为多阶段 feature 开发设计的（requirements → design → implement → verify），对单 bug 修复是 overkill
- Skill 方式让 agent 按步骤执行，保持完整的 session context，更灵活

**close-issue 不作为独立 skill**——合并在 issue-fix 内部。原因：独立 skill 如果没有 trigger patterns 就不会被 skill engine 匹配到，而且 close 逻辑只有 5 步，不值得独立文件。

```yaml
---
name: issue-fix
description: Fix a known issue end-to-end (analyze → fix → commit → verify → close)
triggers:
  patterns:
    - "fix ISS-"
  keywords:
    - "fix issue"
    - "修复"
always_load: false
---
```

**Skill 步骤**：

#### Step 1: 理解 Issue

1. 从 `{project_path}/.gid/docs/issues-index.md` 读取 issue 完整描述
2. 如果 issue 状态不是 `open` 或 `in_progress` → 停止，告知 potato
3. **将 issue 状态更新为 `in_progress`**（防止重复触发——heartbeat 再次扫描时会跳过）
4. 分析 issue 描述，确定涉及的文件和代码区域
5. 使用 `gid_query_impact` 或 `search_files` 定位相关代码

#### Step 2: 实现修复

1. 阅读相关源文件，理解 root cause
2. 使用 `edit_file` 实施修复（优先最小改动，root fix 不是 patch）
3. 如果修复复杂（涉及多文件、架构变更），可以 `spawn_specialist` 给 builder
4. 完成后 commit：
   ```bash
   cd {project_path} && git add -A && git commit -m "fix({project}): ISS-NNN {brief description}"
   ```

#### Step 3: 验证

1. 获取项目的 verify_command：读取 `{project_path}/.gid/config.yml`，如无则语言默认（`cargo test` / `npm test`）
2. 运行 verify_command：
   ```bash
   cd {project_path} && {verify_command}
   ```
3. **如果验证通过** → 继续 Step 4
4. **如果验证失败** → 保留修改（不 revert），更新 issue 状态为 `open`（回退 in_progress），通知 potato：
   ```
   ⚠️ Issue fix failed: {project} ISS-NNN — verify phase: {error summary}
   修改已保留在工作目录，需要手动检查。
   ```
   写入 daily log 记录失败尝试，**停止**。

#### Step 4: 关闭 Issue

1. 获取 commit hash：`cd {project_path} && git log -1 --format=%H`
2. 生成修复摘要（一句话）
3. 更新 `{project_path}/.gid/docs/issues-index.md`：
   - `edit_file`：修改 issue header `[open]` → `[closed]`（或 `[in_progress]` → `[closed]`）
   - `edit_file`：修改 `**状态**` 行 → `closed`
   - `edit_file`：在 `**描述**` 之前插入：
     ```
     **关闭日期**: YYYY-MM-DD
     **修复 commit**: {hash}
     **修复摘要**: {one-line summary}
     ```

#### Step 5: 记录

1. **Daily log 双写**：
   ```markdown
   ## Issue Closed: {project} ISS-NNN
   - **标题**: {title}
   - **修复 commit**: {hash}
   - **摘要**: {summary}
   ```

2. **Engram 写入**：
   ```
   engram_store(type=factual, importance=0.6, 
     content="Issue closed: {project} ISS-NNN ({title}) — fix: {summary}, commit: {hash}")
   ```

3. 通知 potato（如果是自主修复）：`"✅ {project} ISS-NNN fixed and closed — {one-line summary}"`

**Failure handling 总结**：
- Step 1 失败（issue 不存在/已关闭）→ 停止 + 告知
- Step 2 失败（无法理解/修复太复杂）→ 停止 + 告知 + 建议手动处理
- Step 3 失败（验证不通过）→ 保留修改 + 回退状态 + 告知
- 任何步骤失败都写入 daily log，记录失败原因

### §2.5 Heartbeat P0 扫描

在现有 heartbeat 检查流程中增加一步：

**位置**：HEARTBEAT.md 中的检查项列表。

**设计决策**：Heartbeat session **直接执行**修复 workflow，不走间接消息触发。理由：
- Heartbeat session 就是完整的 agent session——有 engram、有工具集、有完整 system prompt。不存在"精简模式"。
- 间接触发（发消息 → 新 session → 重新发现 P0）增加延迟且新 session 需要重新扫描才能找到 P0，没有好处。
- `in_progress` 状态作为互斥锁：heartbeat 在开始修复前将 issue 标记为 `in_progress`，下一次 heartbeat 看到 `in_progress` 就跳过，防止重复触发。

**逻辑**：
1. 读取 `.gid/projects.yml`
2. 对每个项目，读取 `.gid/docs/issues-index.md`（如存在）
3. 用 regex 提取所有 `[P0] [open]` 的 issue
4. 如果发现任何 P0 open issue：
   - **检查是否已有 `[P0] [in_progress]`** → 如果有，跳过（已在修复中）
   - 在心跳报告中添加 🚨 P0 高亮区域
   - 直接执行 issue-fix skill workflow（§2.4 的 Step 1-5）
   - 如果修复成功，心跳报告中标注 `✅ P0 ISS-NNN auto-fixed`
   - 如果修复失败，心跳报告中标注失败原因 + 通知 potato

**观测性**：修复尝试的完整过程记录在 `memory/YYYY-MM-DD.md`（daily log）中。失败时包含错误上下文。LLM 的推理过程可通过 Telegram 通知追溯。

### §2.6 旧数据迁移

一次性任务，不是 recurring 流程。

**迁移步骤**：

1. **读取旧文件**：
   - `/Users/potato/clawd/projects/engram-ai-rust/ISSUES.md`
   - `/Users/potato/clawd/projects/gid-rs/ISSUES.md`

2. **格式转换**：旧格式已经很接近新格式。需要补充的字段：
   - `发现者`：默认标注 `potato`（旧 issue 都是 potato 或手动记录的）
   - `跨项目引用`：根据描述内容判断是否需要
   - 保留所有已有字段值（编号、类型、优先级、状态、描述）

3. **写入新位置**：
   - `{project}/.gid/docs/issues-index.md`

4. **替换旧文件**：
   ```markdown
   # ⚠️ Issues have moved
   
   Issue tracking for this project has moved to:
   `.gid/docs/issues-index.md`
   
   This file is kept for reference only. Do not edit.
   ```

5. **验证**：逐条对比旧文件和新文件，确保零丢失。

---

## §3 Data Flow

### 3.1 Issue 创建流（GOAL-2 + GOAL-3）

```
Input (human msg / agent discovery)
  │
  ▼
project-issues skill 匹配
  │
  ├─ 判断项目归属（从消息提取 / 从工作上下文推断）
  ├─ 自动分类：type + priority
  ├─ 分配下一个 ISS 编号（读取 issues-index.md 最后一个编号 +1）
  │
  ▼
write: {project}/.gid/docs/issues-index.md (append new issue)
write: memory/YYYY-MM-DD.md (daily log)
write: engram (factual memory)
  │
  ▼
如果 P0 → 触发 P0 修复流程（GOAL-5）
如果 P1/P2 → 仅记录，等待手动触发
```

### 3.2 Issue 修复流（GOAL-4 + GOAL-5）

```
Input: "fix ISS-NNN" / P0 auto-trigger (heartbeat 直接执行)
  │
  ▼
project-issues skill → 解析 issue + 项目
  │
  ▼
issue-fix skill workflow (§2.4):
  │
  ├─ Step 1: 理解 issue
  │   ├─ 读取 issue 描述 + 相关代码
  │   └─ 标记状态 in_progress（互斥锁）
  │
  ├─ Step 2: 实现修复
  │   ├─ 分析 root cause
  │   ├─ edit_file 修改代码
  │   └─ git commit
  │
  ├─ Step 3: 验证
  │   ├─ 读取 .gid/config.yml verify_command
  │   ├─ 成功 → 继续
  │   └─ 失败 → 回退状态 + 通知 + 停止
  │
  ├─ Step 4: 关闭 issue
  │   ├─ 更新 issues-index.md → closed
  │   └─ 记录 commit hash + 修复摘要
  │
  └─ Step 5: 记录
      ├─ 写 daily log
      ├─ 写 engram
      └─ 通知 potato（如为自主修复）
```

### 3.3 Dashboard 扫描流（GOAL-6）

```
Input: "show all issues" / heartbeat trigger
  │
  ▼
read: .gid/projects.yml → 项目列表
  │
  ▼
for each project:
  read: {path}/.gid/docs/issues-index.md
  parse: regex extract (编号, 类型, 优先级, 状态, 标题, 日期)
  │
  ▼
aggregate by priority × status
  │
  ▼
format: Telegram bullet list
output: send to chat
```

---

## §4 Issue 编号分配策略

每个项目独立编号，从 ISS-001 开始。新 issue 编号 = 当前最大编号 + 1。

**获取当前最大编号**：
```bash
grep -oE 'ISS-[0-9]+' {project}/.gid/docs/issues-index.md | sort -t- -k2 -n | tail -1
```

**注意**：使用 POSIX ERE (`-oE`)，兼容 macOS BSD grep 和 Linux GNU grep。不使用 `-oP`（PCRE），macOS 默认不支持。实际上 agent 多数情况下在 LLM 推理中直接读取文件并提取编号，不需要 shell grep。

如果 issues-index.md 不存在（新项目），从 ISS-001 开始，并创建文件头。

**跨项目歧义**：当用户说 `"ISS-003"` 而多个项目都有 ISS-003 时，agent 询问。当项目上下文明确（如刚讨论过 engram），直接推断。

---

## §5 实现计划

### 任务分解

| 序号 | 任务 | 依赖 | 产出 |
|------|------|------|------|
| T1 | 创建 `.gid/projects.yml` | — | 配置文件 |
| T2 | 迁移 engram ISSUES.md | T1 | `.gid/docs/issues-index.md` |
| T3 | 迁移 gid-rs ISSUES.md | T1 | `.gid/docs/issues-index.md` |
| T4 | 升级 project-issues skill 到 v3.0 | T1 | `skills/project-issues/SKILL.md` |
| T5 | 创建 issue-fix skill | — | `skills/issue-fix/SKILL.md` |
| T6 | 更新 HEARTBEAT.md（P0 扫描规则） | T1, T4, T5 | HEARTBEAT.md |
| T7 | 端到端验证 | T2-T6 | 测试报告 |

**并行机会**：T1 先做（其他都依赖它），然后 T2+T3+T5 并行，T4 可与 T5 并行，T6 在 T4+T5 之后，T7 最后。

---

## §6 Trade-offs

### 为什么 skill 而不是 Rust 代码？

**选择**：所有逻辑在 skill markdown 中。
**替代方案**：写 Rust 代码实现 issue 管理（struct, trait, parser）。
**Trade-off**：skill 方式更快迭代（改 markdown 不需要 compile），但解析逻辑依赖 LLM 推理和 regex，没有类型安全。对于 issue 管理这种低频操作（每天个位数次），灵活性 > 类型安全。

### 为什么 projects.yml 而不是自动发现？

**选择**：显式项目列表。
**替代方案**：扫描 `/Users/potato/clawd/projects/` 下所有目录。
**Trade-off**：显式列表避免意外扫描到临时目录或不想管理的项目。代价是新项目需要手动加。但新项目频率低（每月 0-2 个），手动维护成本可忽略。

### 为什么 P0 自动修复由 heartbeat 直接执行而不是间接触发？

**选择**：Heartbeat session 直接执行 issue-fix skill workflow。
**替代方案**：Heartbeat 发消息 → 触发新 session → 新 session 重新发现并修复 P0。
**Trade-off**：直接执行更可靠——heartbeat session 本身就是完整 agent session（有 engram、有工具集、有 system prompt），不存在“精简模式”。间接触发增加延迟且新 session 需要重新扫描才能发现 P0，浪费 tokens。`in_progress` 状态作为互斥锁防止重复触发。
