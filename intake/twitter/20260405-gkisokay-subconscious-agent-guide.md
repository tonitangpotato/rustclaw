# Subconscious Self-Improving Agent Loop — Full Guide

- **URL**: https://x.com/gkisokay/status/2040044476060864598
- **Author**: Graeme (@gkisokay)
- **Date**: 2026-04-03
- **Platform**: Twitter/X (Article)
- **Captured**: 2026-04-05
- **Domain**: 🔧 tech
- **learning_priority**: high
- **Engagement**: 444 likes, 31 RTs, 1379 bookmarks, 194K views

## Summary

完整的 "subconscious agent" 构建指南。核心概念：给 AI agent 加一个后台持续运行的自我改进循环。系统不断从历史中学习：brainstorm → debate → refine → 写回状态 → 下一次运行从更新后的状态开始。不是模糊记忆，是结构化的 compounding。

## Architecture — 7 个必要组件

1. **Runner** — 控制平面：加载 brief → 获取状态 → ideation → critique → synthesis → 写回
2. **Persistent State** — JSON/JSONL/Markdown 持久化，进程重启后能继续
3. **Scheduler** — cron/metrics trigger/manual，控制每天跑几次（太多会 diverge）
4. **Transport** — 输出通道（Discord/Telegram/file），与推理层解耦
5. **Model Router** — 不同 phase 用不同模型：便宜本地→ideation，强模型→critique+synthesis
6. **Review Gate** — 人类审批门，防止 autopilot 失控
7. **Artifact Writers** — 预测性地写回文件系统：ideas/, debate/, winning-concept.md, improvement-backlog.md

## Core Loop (Pseudocode)

```
1. inspect state (load previous run results)
2. generate options (ideation, cheap model)
3. challenge weak ideas (debate, strong model)
4. choose one strong direction
5. persist the result (write artifacts)
6. make the next run smarter (update state for next iteration)
```

**关键：如果 artifacts 不 feed 下一次运行，就只是一次性生成链，不是 compounding。**

## File Structure

```
runner/          # orchestration code
state/           # persistent state (JSON, JSONL)
artifacts/       # per-run outputs
  ideas/         # candidate directions
  debate/        # challenge + defence turns
  winning-concept.md
  improvement-backlog.md
targets/         # target definitions
briefs/          # human-readable briefs
```

## Guardrails

- Evidence first (不是 fuzzy opinions)
- Explicit states (不是模糊标签)
- 人类审批门在最后
- 零确认的 cluster 不能自动晋升
- 下一次运行必须把 learning 写回状态

## Model Stack

- **Qwen3.5 9B** (local) — fast ideation, cheap volume
- **GPT-5.4 mini** — challenge, defence, synthesis
- **MiniMax M2.7** — $10/month, sophisticated alternative
- 原则：right model for right phase

## Key Insight

> "The difference between agents that guess improvements and agents that actually compound"

每次运行留下完整 trail：系统想了什么、抵制了什么、什么经过了 critique 存活、下一步该做什么。

## Raw Content

[Full article text preserved in extraction]
