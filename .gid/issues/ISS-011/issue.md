---
id: "ISS-011"
title: "Streaming Telegram Output Edge Cases"
status: closed
priority: P1
created: 2026-04-15
closed: 2026-04-17
component: "src/channels/telegram.rs"
---
# ISS-011: Skill Sub-Agent Adaptation Layer

## Status: Done ✅
## Priority: P1
## Component: skills/*, src/agent.rs (skill injection), src/prompt/sections.rs, src/workspace.rs
## Closed: 2026-04-17

## Problem

Skills (draft-design, review-design 等) 是为**主 agent** 写的，有充足的 context window 和无限 iterations。但被原封不动注入给 sub-agent 时，指令产生冲突：

**冲突示例：**
- draft-design skill 说 "Read ALL requirements first — check both master and feature-level docs"
- 但 sub-agent 的 task 说 "DO NOT read any additional files, content is pre-loaded"
- Sub-agent 收到矛盾指令 → 倾向于遵守 skill（更长、更详细的指令赢）
- 结果：sub-agent 花大量 iterations 读文件，没有 iteration 写输出

**本质：** Skill 指令没有区分 "你是主 agent 有完整环境" 和 "你是 sub-agent 有预加载 context"。

## Root Cause

1. **Skill body 包含文件读取步骤** — 对主 agent 是必要的，对 sub-agent 是浪费
2. **Skill 注入不做任何适配** — `agent.rs` 里直接 `format!("# Skill Instructions\n\n{}", skill.prompt_content())`
3. **没有 sub-agent 专用指令** — skill frontmatter 没有这个概念

## Proposed Fix

### 方案 A: Skill Frontmatter 加 `subagent_preamble`

在 SKILL.md frontmatter 加可选字段：

```yaml
---
name: draft-design
description: ...
subagent_preamble: |
  You are a sub-agent with pre-loaded context. Key rules:
  - All input files are ALREADY in your context. Do NOT read_file for requirements or existing designs.
  - START WRITING the output file within your first 3 tool calls.
  - Budget: 20% reading (only if absolutely needed), 80% writing.
  - Write incrementally: skeleton first, then fill sections one by one.
---
```

**Injection 逻辑改动** (`src/agent.rs`):
```rust
// 现在:
format!("# Skill Instructions\n\n{}", skill.prompt_content())

// 改为:
if is_subagent && skill.has_subagent_preamble() {
    format!(
        "# Sub-Agent Mode\n\n{}\n\n# Skill Instructions\n\n{}",
        skill.subagent_preamble(),
        skill.prompt_content()
    )
} else {
    format!("# Skill Instructions\n\n{}", skill.prompt_content())
}
```

### 方案 B: 全局 SubagentSection 注入 output-first 规则

不改 skill，改 `src/prompt/sections.rs` 的 `SubagentSection`：

```
## Critical Rules (Sub-Agent)
- Pre-loaded files in your context ARE your input. Do NOT re-read them via read_file.
- If your task requires writing a file: START WRITING within your first 3 tool calls.
- Do NOT read additional files unless the task explicitly names them.
- Budget: max 20% of iterations for reading, 80% for writing.
- If the output file is large (>150 lines), write incrementally: structure first, then fill each section.
- If skill instructions say "read X first" but the content is already in your context → skip the read step.
```

### 推荐

**两个都做。** 方案 B 是防御层（所有 sub-agent 都受益），方案 A 是精细控制（per-skill 可以给 sub-agent 不同的执行策略）。

## Scope

- `src/prompt/sections.rs` — SubagentSection render 函数加 output-first 规则
- `src/agent.rs` — skill injection 逻辑支持 subagent_preamble
- `skills/draft-design/SKILL.md` — 加 subagent_preamble
- `skills/review-design/SKILL.md` — 加 subagent_preamble
- SKM frontmatter spec 更新（支持 `subagent_preamble` 字段）

## Verification

- 给 sub-agent 注入 draft-design skill + pre-loaded files → 观察它是否在前 3 iterations 就开始写
- 检查 sub-agent 不会无视 pre-loaded content 去重新 read_file
- 检查 SubagentSection prompt 包含 output-first 规则
