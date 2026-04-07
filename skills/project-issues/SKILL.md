---
name: project-issues
description: Track bugs, improvements, and issues discovered during project usage
version: "2.0.0"
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
  keywords:
    - "issue"
    - "bug report"
    - "improvement needed"
    - "需要改"
    - "记个issue"
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
├── ISSUES.md                ← 统一索引：记录所有 issue 的摘要
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
- `ISSUES.md` 是索引/目录 — 记录每个 issue 的摘要信息
- `ISS-NNN/` 是工作间 — 该 issue 的 fix 过程中产生的所有文档（requirements, design, task 等）
- 简单 issue 可能只需要 ISSUES.md 里的一条记录，不需要子目录
- 复杂 issue（需要 design review、多步实现等）才需要创建 `ISS-NNN/` 目录

## Storage Location

每个项目的 issues 放在 `{project_root}/.gid/issues/`：

```
/Users/potato/rustclaw/.gid/issues/ISSUES.md          ← RustClaw
/Users/potato/clawd/projects/gid-rs/.gid/issues/ISSUES.md  ← gid-rs
/Users/potato/clawd/projects/xinfluencer/.gid/issues/ISSUES.md
...etc
```

**项目路径映射**（已知项目）：
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

## ISSUES.md Format (索引)

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
- `in-progress` — 正在修复
- `blocked` — 被其他事情阻塞
- `done` — 已修复（保留记录，注明修复日期和方式）
- `wontfix` — 决定不修（注明原因）

## Pipeline

### Step 1: 识别项目

从 potato 的描述中判断是哪个项目的 issue。如果不明确，问一下。

### Step 2: 确认路径 & 读取现有 ISSUES.md

```
→ 先确认项目根目录存在（ls 检查），不存在就问 potato 正确路径
→ 创建 .gid/issues/ 目录（如果不存在）
→ 读取 .gid/issues/ISSUES.md（如果存在）
→ 找到最大的 ISS-NNN 编号，下一个 +1
→ 如果 ISSUES.md 不存在，用模板创建
```

### Step 3: 分析与分类

从 potato 的描述中提取：
- **类型**: 是 bug 还是 improvement？
- **优先级**: 根据影响范围和严重程度判断
- **组件**: 影响哪个模块/文件？
- **描述**: 用清晰的语言重述问题
- **建议方案**: 如果能想到方案就写，否则"待分析"

**主动补充上下文**——如果我知道相关的代码或架构细节，写进去。这样以后修的时候不用重新调查。

### Step 4: 写入 ISSUES.md

追加新 issue 到文件末尾。

### Step 5: 创建工作目录（按需）

**只在以下情况创建 `ISS-NNN/` 目录：**
- Issue 是 P0/P1 且需要 design 或 multi-step fix
- potato 明确要求做 design/requirements
- 修复过程中产生了文档需要存放

**简单 issue（小 bug fix、config change）不需要子目录。** ISSUES.md 里的记录就够了。

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
   - See {项目路径}/.gid/issues/ISSUES.md
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
       source: "ISSUES.md"
       priority: P0
   ```

### Step 7: 回复 potato

```
🐛 **Issue Recorded: {项目名} ISS-{NNN}**
类型: {type} | 优先级: {priority}
{一句话描述}
📝 已写入 {项目路径}/.gid/issues/ISSUES.md
{如果创建了工作目录: "📂 工作目录: .gid/issues/ISS-{NNN}/"}

{如果有建议方案: "💡 建议方案: {简述}"}
{如果关联到已有 issue: "🔗 关联: ISS-{XXX} ({描述})"}
```

## Batch Commands

potato 可能会用这些命令：

- **"看看 {项目} 有什么 issue"** → 读取并汇总该项目的 `.gid/issues/ISSUES.md`
- **"所有项目的 open issues"** → 扫描所有已知项目的 `.gid/issues/ISSUES.md`，汇总 open 状态的
- **"ISS-003 done"** → 更新状态为 done，加上修复日期
- **"清理 {项目} 的 issues"** → 把 done/wontfix 的归档到底部

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
- **保留 done 的 issue。** 不要删除，改状态为 done 并注明修复方式。这是项目历史。
- **编号永远递增。** 不复用已删除的编号。
- **优先级可以调整。** 发现时给个初始判断，后续 potato 可以改。
- **不确定是不是 issue？记下来。** 宁可多记一个 wontfix，也不要漏掉一个真正的问题。
- **ISS 文档放项目的 `.gid/issues/` 下。** 不要放根目录、不要放 `docs/`。
