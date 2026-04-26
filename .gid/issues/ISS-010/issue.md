---
id: "ISS-010"
title: "Sub-agent Delegation Hard Rules"
status: closed
priority: P1
created: 2026-04-15
closed: 2026-04-17
component: "AGENTS.md, src/orchestrator.rs"
---
# ISS-010: Sub-Agent Delegation Decision Framework

## Status: Done ✅
## Priority: P1
## Component: AGENTS.md, orchestration logic
## Closed: 2026-04-17

## Problem

主 agent 缺少**硬性决策规则**来判断一个任务是否适合委托给 sub-agent。当前 AGENTS.md 里的 "Sub-Agent Task Fitness" 是模糊指导（"P(success) > 80%"），没有可执行的 checklist。

**结果：**
- 需要写 750 行设计文档的任务被委托给 sub-agent
- Sub-agent 失败后又重试了一次（同样失败）
- 最终主 agent 自己拆分写才成功
- 浪费 ~200k tokens + 大量时间

## Root Cause

1. **没有基于输出大小的决策规则** — 没有人检查 "预期输出多大" 再决定是否 delegate
2. **没有同类失败阻断** — sub-agent 失败后的默认反应是重试，而不是换策略
3. **大文件写入没有拆分策略** — 不管多大的文件都尝试一次 write_file 搞定

## Proposed Fix

在 AGENTS.md 的 "Sub-Agent Task Fitness" 章节加入以下**硬规则**：

### 规则 1: 输出大小预估（Delegation Gate）
```
在 spawn_specialist 之前，预估输出文件行数：
- 预期输出 > 300 行 → 不 delegate，主 agent 拆分写
- 预期输出 100-300 行 → delegate 但设 max_iterations ≥ 35
- 预期输出 < 100 行 → 正常 delegate (max_iterations=25)
```

### 规则 2: 同类失败阻断（No Retry Same Strategy）
```
Sub-agent 某类任务失败一次 → 不重试同样的 delegation 方式
必须换策略：
  a) 主 agent 自己做
  b) 拆分成更小的子任务再 delegate
  c) 减少 pre-loaded files，增加 max_iterations
同一个 session 内不应该在同一个任务上失败两次
```

### 规则 3: 大文件写入策略（Incremental Write Pattern）
```
任何预期 > 200 行的输出：
  - 主 agent 分段写（每段一个 write_file/edit_file call）
  - 先写骨架（headings + structure），再逐段填充内容
  - 每段 50-150 行，不要一次写 500+ 行
这不是 sub-agent 特有的——主 agent 也应该遵守
```

## Scope

- 修改 `/Users/potato/rustclaw/AGENTS.md` 的 "Sub-Agent Task Fitness" 章节
- 这是纯 prompt/约定层面的改动，不涉及 Rust 代码
- 改完后所有 session（主 agent + 未来的 sub-agent）都会遵守

## Verification

- 下次遇到大文件写入任务时，主 agent 应该自动拆分写而不是尝试 delegate
- 下次 sub-agent 失败时，不应该用相同方式重试
