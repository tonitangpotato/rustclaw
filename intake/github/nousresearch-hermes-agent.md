# Hermes Agent — Nous Research

- **URL**: https://github.com/NousResearch/hermes-agent
- **Stars**: 23.9k | Forks: 3.1k | Commits: 3,181 | Tests: ~3,000
- **Date Captured**: 2026-04-03
- **Domain**: 🔧 tech + 📦 product
- **License**: MIT

## Summary

Nous Research 的 self-improving AI agent。关键卖点是 "the only agent with a built-in learning loop"。Python 写的，支持多 LLM provider，多平台（Telegram/Discord/Slack/WhatsApp/Signal），有 skill 系统、memory、cron、sub-agent delegation。

## Architecture

- **Core loop**: `run_agent.py` (AIAgent class) — 同步 while loop，OpenAI format messages
- **CLI**: Rich + prompt_toolkit TUI
- **Gateway**: messaging platform adapters (telegram, discord, slack, whatsapp, signal)
- **Tools**: 40+ tools, registry pattern (`tools/registry.py`)
- **Skills**: `skills/` directory, SKILL.md format (agentskills.io standard)
- **Memory**: FTS5 session search + LLM summarization + Honcho dialectic user modeling
- **Sessions**: SQLite (SessionDB)
- **Terminal backends**: local, Docker, SSH, Daytona, Singularity, Modal
- **Profiles**: Multi-instance support via HERMES_HOME
- **Config**: `~/.hermes/config.yaml`

## Key Capabilities

1. **Self-improving learning loop**: skills from experience → improve during use → persist knowledge
2. **Cross-session recall**: FTS5 session search with LLM summarization
3. **Sub-agent delegation**: isolated subagents for parallel workstreams
4. **Cron scheduling**: natural language scheduled tasks with platform delivery
5. **Context compression**: auto-compress with lineage-aware persistence
6. **Prompt caching**: Anthropic prompt caching, strict cache-stability policy
7. **Batch runner**: parallel trajectory generation for RL/benchmarks
8. **RL integration**: Atropos environments, trajectory compression for training

## Self-Evolution System (separate repo)

- **Repo**: NousResearch/hermes-agent-self-evolution (600 stars)
- **Engine**: DSPy + GEPA (Genetic-Pareto Prompt Evolution) — ICLR 2026 Oral
- **Cost**: ~$2-10 per optimization run, no GPU needed
- **What it evolves**:
  - Phase 1 ✅: Skill files (SKILL.md) — wrap as DSPy module, eval via batch_runner, evolve with GEPA
  - Phase 2 🔲: Tool descriptions — improve tool selection accuracy
  - Phase 3 🔲: System prompt sections — optimize offline, deploy as new versions
  - Phase 4 🔲: Code evolution — Darwinian Evolver with Git-based organisms
  - Phase 5 🔲: Continuous improvement loop — automated pipeline
- **GEPA**: reads execution traces to understand WHY things fail, works with as few as 3 examples
- **Eval data**: synthetic generation, SessionDB mining, hand-curated golden sets, skill-specific auto-eval
- **Scoring**: LLM-as-judge with rubrics (procedure, correctness, conciseness)
- **Guardrails**: full test suite, size limits (Skills ≤15KB), caching compatibility, semantic preservation, PR review
- **Deployment**: Git branch + PR, human review before merge

## Comparison with RustClaw

| Feature | Hermes Agent | RustClaw |
|---------|-------------|----------|
| Language | Python | Rust |
| Binary | pip install | 35MB single binary |
| Stars | 23.9k | — |
| Tests | ~3,000 | 166 |
| Channels | 6 (Telegram, Discord, Slack, WhatsApp, Signal, HA) | 6 (Telegram, Discord, Slack, WhatsApp, Signal, Matrix) |
| Skills | agentskills.io standard, SKILL.md | SKM, SKILL.md (compatible) |
| Memory | FTS5 + Honcho user modeling | Engram (ACT-R + Hebbian) |
| Task mgmt | — | GID (graph-indexed) |
| Self-improvement | DSPy + GEPA evolution | IDEA stage |
| Terminal backends | 6 (local, Docker, SSH, Daytona, Singularity, Modal) | local only |
| Context management | compression + prompt caching | — |
| RL integration | Atropos environments | — |
| Sub-agents | Yes (delegate_tool) | Yes (orchestrator) |
| Cron | Built-in | Heartbeat only |

## Key Takeaways for RustClaw

### 1. Self-Evolution via GEPA (Directly Applicable)
- GEPA is MIT licensed, works via API calls only
- Their approach: wrap skill as DSPy module → generate eval dataset → run GEPA → compare → PR
- We can adapt this for our Skill + Harness self-optimization (IDEA-20260403-01 & 03)
- Key insight: GEPA reads execution traces to understand WHY failures happen
- We already have execution-log.jsonl (gid-harness) as our trace source

### 2. Prompt Caching Discipline
- Strict rule: NEVER alter past context mid-conversation
- We should adopt this for token efficiency

### 3. Multi-Source Eval Data
- SessionDB mining (real usage) + synthetic generation + golden sets + auto-eval
- We have engram + session DB + execution logs — richer data sources

### 4. Continuous Learning Loop Architecture
- Skills created from experience → improved during use → nudge to persist
- Our skill system + engram behavioral stats already has the building blocks
- Missing: the automatic improvement loop (GEPA/DSPy integration)

### 5. agentskills.io Standard
- They use it, we use it — our skills are format-compatible
- Their Skills Hub could be a distribution channel for our skills too
