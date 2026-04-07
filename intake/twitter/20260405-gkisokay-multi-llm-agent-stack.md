# Multi-LLM Agent Stack — Cost-Optimized Architecture

- **URL**: https://x.com/gkisokay/status/2040352080196755780
- **Author**: Graeme (@gkisokay)
- **Date**: 2026-04-04
- **Platform**: Twitter/X
- **Captured**: 2026-04-05
- **Domain**: 🔧 tech + 💰 trading
- **learning_priority**: high

## Summary

Anthropic 封禁了用 Claude 订阅来跑 OpenClaw 的账号。作者展示了他早就准备好的多模型 agent stack，证明不应该把整个 agent 绑死在单一 LLM 上。本地模型跑量（$0），便宜 API 跑执行，高端模型只做判断。

## Key Points

- **Qwen3.5 9B** — 本地运行，$0，7×24 跑 "subconscious ideation loop"（后台持续思考），比 GPT-OSS-120B 快 13x
- **MiniMax M2.7** — agent backbone，97% skill adherence，$0.30/M tokens，$10 plan = 1500 calls/5hrs
- **GPT-5.4 mini** — orchestration brain，~$0.075/次，做辩论+判断+输出
- **Claude Opus 4.6** — 仅用于 Claude Code 里的复杂外部开发，不走订阅
- **24小时成本**: subconscious 跑了 15 次，总计 $1.58
- **核心原则**: "Build your agent stack on a multiple LLM stack" — 本地模型处理量，订阅模型处理执行和判断，你掌控成本结构

## Category
tech/infrastructure

## Tags
multi-llm, agent-stack, cost-optimization, local-models, qwen, minimax, openclaw, subconscious-loop

## Engagement
749 likes, 70 RTs, 81 replies — 高互动，说明多 LLM stack 是当前热门话题

## Raw Content

Anthropic just banned Claude subscriptions from powering OpenClaw.

Here's why my stack was already built for this.

I never ran Opus 4.6 through a subscription for OpenClaw or Hermes. It runs in Claude Code for complex external dev only. Same with GPT-5.4 in Codex.

The internal agent runtime is a completely different stack:

1. Qwen3.5 9B runs locally. $0. Always on. Feeds the subconscious ideation loop 24/7. Beats GPT-OSS-120B by 13x. Awesome.

2. MiniMax M2.7 is the agent's backbone. 97% skill adherence, built for agents, $0.30/M tokens. The $10 plan allows for 1500 calls every 5 hours. Amazing.

3. GPT-5.4 mini is the Hermes brain. debates ideas with the subconscious, builds output, ~$0.075 avg per run. It's smart enough to orchestrate your entire system, and you can actually use your subscription plan here via OAuth. Incredible!

Over the last 24 hours, the subconscious ran 15 times, for a total of $1.58. Not too shabby for an always-improving agentic system.

The lesson is to build your agent stack on a multiple LLM stack.

Local models handle volume. Generous subscription models handle execution and judgment. You own the cost structure.
