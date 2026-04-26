---
id: "ISS-012"
title: "Confidence Weighting in Code Graph Extraction"
status: closed
priority: P2
created: 2026-04-15
closed: 2026-04-17
component: "gid-core/code_graph"
---
# ISS-012: Sub-Agent Iteration Budget Monitor

## Status: Done ✅
## Priority: P2
## Component: src/agent.rs (sub-agent execution loop)
## Closed: 2026-04-17

## Problem

Sub-agent 可以花掉所有 iterations 读文件和思考，最后一个 iteration 才开始写输出——然后因为 max_iterations 到了被终止，产出为零。

当前没有任何运行时机制检测这种 "全读无写" 的反模式。

**典型失败模式：**
```
Iteration 1-5:   read_file × 5（读 requirements, existing code, etc）
Iteration 6-15:  read_file × 10（读更多 context）
Iteration 16-22: read_file + search_files（继续收集信息）
Iteration 23:    write_file（开始写输出）
Iteration 24:    write_file（继续写——但只写了一半）
Iteration 25:    MAX ITERATIONS → 强制终止 → 输出文件不完整或不存在
```

## Root Cause

`agent.rs` 的 sub-agent 循环只检查 `iteration < max_iterations`，不追踪工具调用模式。没有 "你在做无效工作" 的 early warning。

## Proposed Fix

### 在 sub-agent 执行循环中加入 iteration budget 监控

**核心逻辑：**

```rust
// 在 sub-agent loop 中追踪
struct IterationTracker {
    total: usize,
    max: usize,
    write_calls: usize,  // write_file + edit_file 调用次数
}

impl IterationTracker {
    fn should_warn(&self) -> Option<String> {
        let progress = self.total as f32 / self.max as f32;
        
        // 50% iterations 用完，还没有任何写操作
        if progress >= 0.5 && self.write_calls == 0 {
            return Some(format!(
                "⚠️ WARNING: You have used {}/{} iterations without writing any output files. \
                 START WRITING NOW. Pre-loaded files in your context are your input — \
                 do not read more files. You have {} iterations remaining.",
                self.total, self.max, self.max - self.total
            ));
        }
        
        // 75% iterations 用完，仍然没有写操作 → 更强的警告
        if progress >= 0.75 && self.write_calls == 0 {
            return Some(format!(
                "🚨 CRITICAL: {}/{} iterations used, ZERO output files written. \
                 You MUST write your output file in the next tool call or this task will fail. \
                 Write what you have NOW — partial output is better than no output.",
                self.total, self.max
            ));
        }
        
        None
    }
}
```

**注入方式：** 在 sub-agent 的 tool loop 中，每次 iteration 后检查 `should_warn()`。如果返回 Some，将 warning message 作为 system message 注入到下一轮对话的 messages 中。

### 实现位置

`src/agent.rs` — `run_subagent()` 或 `run_specialist()` 方法中：

```rust
// 现有循环大致结构:
for iteration in 0..max_iterations {
    let response = llm.call(messages).await?;
    let tool_calls = extract_tool_calls(&response);
    
    // === 新增：追踪写操作 ===
    for call in &tool_calls {
        if call.name == "write_file" || call.name == "edit_file" {
            tracker.write_calls += 1;
        }
    }
    tracker.total = iteration + 1;
    
    // === 新增：注入警告 ===
    if let Some(warning) = tracker.should_warn() {
        messages.push(Message::system(warning));
    }
    
    // 执行 tool calls...
}
```

### 额外：transcript 里记录 budget 使用

```
[iteration 12/25] tools: read_file | writes: 0 | WARNING: no output at 50%
[iteration 18/25] tools: read_file | writes: 0 | CRITICAL: no output at 75%
[iteration 19/25] tools: write_file | writes: 1 | Started writing
```

这让调试 sub-agent 失败变得容易——看 transcript 就知道它在干什么。

## Scope

- `src/agent.rs` — sub-agent 执行循环加 IterationTracker
- 不影响主 agent 循环（主 agent 没有 max_iterations 限制）
- 不影响 tool 定义或 skill 系统

## Risk

- **Low risk**: 只是注入额外的 system message，不改变执行流程
- **Edge case**: 有些合法任务确实需要大量读（比如 review）→ 50% warning 不是 "stop reading"，是 "start writing"
- 如果任务纯粹是分析（不需要写文件），warning 会是噪音 → 可以在 task description 加 `[no-write-expected]` 标记来 suppress

## Verification

- 给 sub-agent 一个需要写文件的任务 + 足够的 pre-loaded context
- 验证：50% iterations 时如果没有 write → warning 出现在 transcript
- 验证：warning 后 sub-agent 开始写（或至少尝试写）
- 验证：纯分析任务（不需要写文件）不会被 warning 干扰

## 与 ISS-010, ISS-011 的关系

- **ISS-010** 解决 "不该 delegate 的别 delegate"（决策层）
- **ISS-011** 解决 "delegate 了给对的指令"（指令层）
- **ISS-012** 解决 "指令不听也有后手"（运行时监控层）
- 三层防御：决策 → 指令 → 监控
