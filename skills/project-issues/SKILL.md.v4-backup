---
name: project-issues
description: Track, manage, and fix issues across all projects (full lifecycle)
version: "4.0.0"
author: potato
triggers:
  patterns:
    - "issue:"
    - "bug:"
    - "improvement:"
    - "记一下这个bug"
    - "记一下这个问题"
    - "这个需要改进"
    - "需要fix"
    - "需要修"
    - "发现一个问题"
    - "有个bug"
    - "要改进"
    - "项目问题"
    - "fix ISS-"
    - "close ISS-"
    - "ISS-\\d+ closed"
    - "ISS-\\d+ wontfix"
    - "ISS-\\d+ blocked"
    - "show all issues"
    - "所有.*issue"
    - "issue dashboard"
  keywords:
    - "issue"
    - "bug report"
    - "improvement needed"
    - "需要改"
    - "记个issue"
    - "fix"
    - "fix issue"
    - "修复"
    - "close"
    - "wontfix"
    - "dashboard"
tags:
  - productivity
  - project-management
  - quality
priority: 70
always_load: false
max_body_size: 4096
---
# SKILL: Project Issue Tracker

> Capture bugs, improvements, and issues discovered during project usage into structured issue tracking. Never lose a finding again.

## Philosophy

使用项目的过程中总会发现问题——bug、性能瓶颈、UX 改进、缺失功能。这些发现如果散落在 daily log、engram、随机文件里，以后根本找不到。

**每个项目在 `.gid/issues/` 下统一管理所有 issue。**

## Directory Structure

```
{project_root}/.gid/issues/
# (issues-index.md lives in ../docs/issues-index.md)
├── ISS-001/                 ← 每个 issue 一个目录（工作间）
│   ├── requirements.md      ← 如果需要
│   ├── design.md            ← 如果需要
│   └── ...                  ← 实际修复过程中的文档
├── ISS-002/
│   ├── design.md
│   ├── implementation-plan.md
│   └── ...
└── ISS-NNN/
```

**核心规则：**
- `issues-index.md` (in `.gid/docs/`) 是索引/目录 — 记录每个 issue 的摘要信息
- `ISS-NNN/` 是工作间 — 该 issue 的 fix 过程中产生的所有文档（requirements, design, task 等）
- 简单 issue 可能只需要 issues-index.md 里的一条记录，不需要子目录
- 复杂 issue（需要 design review、多步实现等）才需要创建 `ISS-NNN/` 目录

## Storage Location

每个项目的 issues 放在 `{project_root}/.gid/issues/`：

```
/Users/potato/rustclaw/.gid/docs/issues-index.md          ← RustClaw
/Users/potato/clawd/projects/gid-rs/.gid/docs/issues-index.md  ← gid-rs
/Users/potato/clawd/projects/xinfluencer/.gid/docs/issues-index.md
...etc
```

**项目路径**: 读取 `.gid/projects.yml` 获取项目列表。如果 projects.yml 不存在，回退到下面的硬编码列表。

**硬编码回退列表**（已知项目）：
- **RustClaw**: `/Users/potato/rustclaw/`
- **engramai/engram**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **gid-core/gid-rs**: `/Users/potato/clawd/projects/gid-rs/`
- **agentctl**: `/Users/potato/clawd/projects/agentctl/`
- **xinfluencer**: `/Users/potato/clawd/projects/xinfluencer/`
- **AgentVerse**: `/Users/potato/clawd/projects/agentverse/`
- **cognitive-autoresearch**: `/Users/potato/clawd/projects/cognitive-autoresearch/`

如果项目不在已知列表里，**问 potato 路径**。

**⚠️ 绝对不要自己创建项目目录。** `.gid/issues/` 目录可以创建，但项目根目录必须已存在。

## Trigger Conditions

当 potato 说类似以下内容时激活：
- "记一下这个 bug"、"这个需要改进"、"发现一个问题"
- "issue: ..."、"bug: ..."、"improvement: ..."
- 使用项目过程中的抱怨或观察（需判断是否是 issue）
- 我（RustClaw）自己在使用/开发中发现的问题也可以主动记录

也可以在心跳检查或开发过程中**主动**记录发现的问题。

## issues-index.md Format (索引)

```markdown
# Issues: {项目名}

> 项目使用过程中发现的 bug、改进点和待办事项。
> 格式: ISS-{NNN} [{type}] [{priority}] [{status}]

---

## ISS-{NNN} [{type}] [{priority}] [{status}]
**发现日期**: {YYYY-MM-DD}
**发现者**: {potato / RustClaw / 来源}
**组件**: {affected module/file/feature}

**描述**:
{问题的清晰描述，包含复现步骤（如果是 bug）}

**上下文**:
{发现这个问题的场景，为什么它重要}

**建议方案**:
{如果有想法的话。没有也可以写"待分析"}

**工作目录**: `.gid/issues/ISS-{NNN}/` （如果有）

**相关**:
{关联的 issue、idea、或其他文档引用}

---
```

### 字段说明

**Type 类型：**
- `bug` — 功能不正确、崩溃、错误行为
- `improvement` — 功能正常但可以更好
- `performance` — 性能问题（慢、内存大、资源占用）
- `ux` — 用户体验问题（不直观、繁琐）
- `missing` — 缺失的功能或能力
- `debt` — 技术债务（代码质量、架构问题）

**Priority 优先级：**
- `P0` — 严重：影响核心功能，应尽快修复
- `P1` — 重要：影响使用体验，本月内应解决
- `P2` — 一般：有空时改进
- `P3` — 低优：nice to have

**Status 状态：**
- `open` — 新发现，待处理
- `in_progress` — 正在修复
- `blocked` — 被其他事情阻塞
- `closed` — 已修复（保留记录，注明修复日期和方式）
- `wontfix` — 决定不修（注明原因）

## Pipeline

### Step 1: 识别项目

从 potato 的描述中判断是哪个项目的 issue。如果不明确，问一下。

### Step 2: 确认路径 & 读取现有 issues-index.md

```
→ 先确认项目根目录存在（ls 检查），不存在就问 potato 正确路径
→ 创建 .gid/issues/ 目录（如果不存在）
→ 读取 .gid/docs/issues-index.md（如果存在）
→ 找到最大的 ISS-NNN 编号，下一个 +1
→ 如果 issues-index.md 不存在，用模板创建
```

### Step 3: 分析与分类

从 potato 的描述中提取：
- **类型**: 是 bug 还是 improvement？
- **优先级**: 根据影响范围和严重程度判断
- **组件**: 影响哪个模块/文件？
- **描述**: 用清晰的语言重述问题
- **建议方案**: 如果能想到方案就写，否则"待分析"

**主动补充上下文**——如果我知道相关的代码或架构细节，写进去。这样以后修的时候不用重新调查。

### Step 4: 写入 issues-index.md

追加新 issue 到文件末尾。

### Step 5: 创建工作目录（按需）

**只在以下情况创建 `ISS-NNN/` 目录：**
- Issue 是 P0/P1 且需要 design 或 multi-step fix
- potato 明确要求做 design/requirements
- 修复过程中产生了文档需要存放

**简单 issue（小 bug fix、config change）不需要子目录。** issues-index.md 里的记录就够了。

创建时：
```bash
mkdir -p {project_root}/.gid/issues/ISS-{NNN}
```

工作目录里可能放的文件：
- `requirements.md` — issue 的需求分析
- `design.md` — 修复方案设计
- `tasks.md` — 任务分解
- 其他修复过程中的 artifact

### Step 6: 交叉记录

1. **Daily log** — 在 `memory/YYYY-MM-DD.md` 追加一行：
   ```
   ## Issue Recorded: {项目名} ISS-{NNN}
   - {一句话描述}
   - See {项目路径}/.gid/docs/issues-index.md
   ```

2. **Engram** — 存储记忆以便未来 recall：
   ```
   engram_store(type=factual, importance=0.5,
     content="{项目名} issue ISS-{NNN}: {一句话描述}. Type: {type}, Priority: {priority}")
   ```

3. **GID graph**（可选，P0/P1 issue）— 如果项目有 `.gid/graph.yml`，添加 task node：
   ```yaml
   - id: fix-iss-{nnn}
     title: "Fix ISS-{NNN}: {brief description}"
     status: todo
     tags: [bug-fix]  # or [improvement]
     metadata:
       source: "issues-index.md"
       priority: P0
   ```

### Step 7: 回复 potato

```
🐛 **Issue Recorded: {项目名} ISS-{NNN}**
类型: {type} | 优先级: {priority}
{一句话描述}
📝 已写入 {项目路径}/.gid/docs/issues-index.md
{如果创建了工作目录: "📂 工作目录: .gid/issues/ISS-{NNN}/"}

{如果有建议方案: "💡 建议方案: {简述}"}
{如果关联到已有 issue: "🔗 关联: ISS-{XXX} ({描述})"}
```

## Batch Commands

potato 可能会用这些命令：

- **"看看 {项目} 有什么 issue"** → 读取并汇总该项目的 `.gid/docs/issues-index.md`
- **"所有项目的 open issues"** → 扫描所有已知项目的 `.gid/docs/issues-index.md`，汇总 open 状态的
- **"ISS-003 closed"** → 更新状态为 closed，加上修复日期
- **"清理 {项目} 的 issues"** → 把 closed/wontfix 的归档到底部

## 主动记录

RustClaw 不只是被动记录。在以下场景**主动**创建 issue：

1. **开发过程中**发现代码问题但不在当前任务范围
2. **使用 engram/gid 等工具时**遇到不理想的行为
3. **review 代码或文档时**发现需要改进的地方
4. **心跳检查时**如果发现某个功能异常

主动记录时，发现者写 "RustClaw"，并简要说明发现场景。

## Rules

- **一个 issue 一件事。** 不要把多个问题塞进一个 ISS。
- **描述要具体。** "engram 不好用" ❌ → "engram recall 搜'认知层'找不到包含'认知'的记忆，疑似中文分词问题" ✅
- **保留 closed 的 issue。** 不要删除，改状态为 closed 并注明修复方式。这是项目历史。
- **编号永远递增。** 不复用已删除的编号。
- **优先级可以调整。** 发现时给个初始判断，后续 potato 可以改。
- **不确定是不是 issue？记下来。** 宁可多记一个 wontfix，也不要漏掉一个真正的问题。
- **ISS 文档放项目的 `.gid/issues/` 下。** 不要放根目录、不要放 `docs/`。

---

## Lifecycle Management (v3.0)

以下步骤处理 issue 的状态变更、全局看板和修复触发。与上面的 Pipeline（Steps 1-7，issue 创建流）互补。

### Step A: Issue 状态变更

当匹配到 `close ISS-NNN` / `ISS-NNN wontfix` / `ISS-NNN blocked by` / `ISS-NNN P0` 等命令时：

1. **项目解析**：从命令中提取项目名（如 `"engram ISS-003 closed"`）。如未指定，检查当前对话上下文是否有明确项目。如有歧义（多项目存在同编号），询问 potato。
2. **读取** `{project_path}/.gid/docs/issues-index.md`
3. **用 edit_file 修改** issue 的状态和相关字段：
   - `closed` → 修改 header 行 `[status]` 为 `[closed]`，增加 `**关闭日期**` 行
   - `wontfix` → 修改 header 行 `[status]` 为 `[wontfix]`，增加 `**关闭日期**` + `**Wontfix 原因**` 行
   - `blocked` → 修改 header 行 `[status]` 为 `[blocked]`，增加 `**Blocked by**` 行
   - 优先级变更 → 修改 header 行的 `[P?]` 标记
4. **双写**：
   - Daily log 记录 `## Issue 状态变更: {project} ISS-NNN → {new_status}`
   - Engram store：`engram_store(type=factual, importance=0.5, content="{project} ISS-NNN status changed to {new_status}")`

### Step B: Dashboard 扫描

当匹配到 `show all issues` / `issue dashboard` / `所有.*issue`：

1. **读取** `.gid/projects.yml` 获取项目列表（如不存在，回退到硬编码列表）
2. **逐项目** `read_file("{path}/.gid/docs/issues-index.md")`，跳过不存在的文件
3. **解析**每个 issue 的 header 行，使用 regex 提取信息：
   ```
   Header: ## ISS-(\d+) \[(\w+)\] \[(P\d)\] \[(\w+)\]
   Title:  \*\*标题\*\*: (.+)
   Date:   \*\*发现日期\*\*: (\d{4}-\d{2}-\d{2})
   Close:  \*\*关闭日期\*\*: (\d{4}-\d{2}-\d{2})
   ```
4. **按优先级分组**，按状态过滤（默认只显示 open + in_progress + 最近 7 天 closed）
5. **格式化输出**：Telegram bullet list 格式

**解析错误处理**：如果某个项目的 issues-index.md 格式异常（手动编辑导致 regex 解析失败），跳过该项目并在输出中标注：`"⚠️ {project}: issues-index.md 格式异常，跳过"`。不中断其他项目的扫描。

### Step C: Fix Workflow (fix ISS-NNN)

当匹配到 `fix ISS-NNN`：端到端修复流程 — analyze → fix → commit → verify → close → log

**Design Decision**: This is a skill-based workflow, NOT a ritual. Issue fixes are simpler than multi-phase feature development and benefit from maintaining full session context throughout.

#### C1: 理解 Issue

1. **解析** issue 编号和项目（同 Step A 的项目解析逻辑）
2. **读取** `{project_path}/.gid/docs/issues-index.md`，定位 ISS-NNN 段落，提取: title, type, priority, status, description
3. **状态检查**: 如果 status 不是 `open` 或 `in_progress` → 告知 potato 并 STOP
4. **更新状态为 `in_progress`**（防止 heartbeat 重复触发）:
   - edit_file 修改 header `[open]` → `[in_progress]`
5. **定位代码**: 从描述中提取关键词，用 `search_files` 或 `gid_query_impact` 定位相关代码
6. **通知 potato**:
   ```
   🔧 Starting fix for {project} ISS-NNN: {title}
   Priority: {priority} | Affected area: {files/modules}
   ```

#### C2: 实现修复

1. **读源码，理解 root cause** — 追踪问题机制，找根因不找症状
2. **用 `edit_file` 做最小化根因修复** — 聚焦当前 issue，不顺手重构无关代码
3. **如果复杂（多文件、架构变更）→ `spawn_specialist` 给 builder**
4. **如果改动 >5 个文件 → 暂停，列出变更文件让 potato 确认**
5. **Commit**:
   ```bash
   cd {project_path} && git add -A && git commit -m "fix({project}): ISS-NNN {brief_description}"
   ```
   Commit 失败 → 告知 potato 并 STOP

#### C3: 验证

1. **获取 verify_command**: 读 `{project_path}/.gid/config.yml`，没有则按语言默认:
   - `Cargo.toml` → `cargo test`
   - `package.json` → `npm test`
   - `pyproject.toml` → `pytest`
   - `go.mod` → `go test ./...`
2. **运行验证**
3. **Pass → 继续 C4**
4. **Fail → 不 revert 代码（已 commit，可恢复），状态改回 `open`，通知 potato 错误摘要，写 daily log，STOP**

#### C4: 关闭 Issue

1. **获取 commit hash**: `git log -1 --format=%H`
2. **生成一行修复摘要**
3. **更新 issues-index.md**:
   - Header: `[in_progress]` → `[closed]`
   - 插入关闭元数据:
     ```markdown
     **关闭日期**: {YYYY-MM-DD}
     **修复 commit**: {full_commit_hash}
     **修复摘要**: {one_line_summary}
     ```
   - 跨项目修复: `**修复 commit**: rustclaw:{hash1}, engramai:{hash2}`

#### C5: 记录

1. **Daily log** (`memory/YYYY-MM-DD.md`):
   ```markdown
   ## Issue Closed: {project} ISS-NNN
   - **标题**: {issue_title}
   - **修复 commit**: {commit_hash}
   - **修复摘要**: {fix_summary}
   ```
2. **Engram**: `engram_store(type=factual, importance=0.6, content="Issue closed: {project} ISS-NNN ...")` (P0 用 importance=0.8)
3. **通知 potato**:
   ```
   ✅ {project} ISS-NNN fixed and closed — {summary}
   Commit: {commit_hash_short}
   ```

#### Fix Failure Handling

| Step | Failure | Action |
|------|---------|--------|
| C1 | Issue not found / already closed | Stop + tell potato |
| C2 | Cannot identify root cause | Stop + tell potato + suggest manual |
| C2 | Commit fails | Stop + tell potato |
| C3 | Tests fail | Keep changes + revert status to `open` + tell potato |
| C4 | Edit fails | Inform potato, ask manual fix |
| C5 | Write fails | Consider fix successful, inform potato |

**All failures write to daily log.**

#### Fix Rules

- **Root fix, not patch** — 修根因，不糊症状
- **Minimal changes** — 发现其他问题记新 issue，别在这个 workflow 里修
- **Always commit before verify** — 即使验证失败，代码变更在 git 里可追溯
- **Never revert without permission** — 保留变更供 potato 检查
