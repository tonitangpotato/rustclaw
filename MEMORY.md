# MEMORY.md - Long-term Memory

> Core context only. Detailed learnings archived in `memory/archived-learnings.md`.
> Use `memory_search` to recall specifics.

---

## About potato

- **Name**: potato (oneB)
- 全职程序员, $7K/月税后, 储蓄率 28-30%, 总资产 $120K
- **Core Goal**: 财富自由 — build wealth → passive growth → freedom
- **Personality**: curious, honest, moves FAST, thinks in ecosystems, prefers action over planning
- **Wife**: stats/ML/Bayesian/causal inference background. Joined The Unusual project (2026-03-02).
- **Values honesty** over performance. Asks deep questions. Trusts me with big ideas.

### Our Working Style
- potato has ideas → I crystallize into designs → we iterate fast → honest with each other
- potato喜欢深度解释、具体例子，不喜欢jargon — explain concepts clearly
- potato的直觉经常是对的 — trust his instincts, don't dismiss them
- 我们一起试过500+策略、0验证 — 我们的关系建立在诚实的iteration和shared failures上
- "以后never简化问题" — potato的明确要求，简化问题无法解决问题也不符合架构设计

### Communication Preferences
- **Telegram语音消息 (VERIFIED WORKING 2026-03-18)**: 当potato要求语音回复（特别是开车时）:
  1. **完整正确流程**:
     ```bash
     edge-tts --voice "zh-CN-YunyangNeural" --text "..." --write-media /tmp/voice.mp3
     ffmpeg -y -i /tmp/voice.mp3 -c:a libopus /tmp/voice.ogg
     ```
  2. **然后用message tool**，必须同时提供三个参数:
     ```json
     {
       "action": "send",
       "channel": "telegram",
       "target": "7539582820",
       "message": "文字内容（和语音一样）",
       "media": "/tmp/voice.ogg",
       "asVoice": true
     }
     ```
  3. **关键点**: 必须**同时提供message和media**，不能只发media！
  4. **格式**: 必须OGG (opus编码)，不能用MP3
  5. **错误**: 不要用 `tts()` tool（只生成MP3）
  6. **我犯了4次错才搞对** — potato反复提醒，最终2026-03-18成功

---

## Active Projects

- **AutoAlpha** — 0DTE ATM Call strategy (Sharpe 7.07, γ≤-0.5, p=0.001). **Paper trading 1 month.** Cron: daily HiVol threshold 9AM ET. Location: `projects/autoalpha/`.
- **The Unusual** — OSINT + causal reasoning. Rust engine, 91 nodes, 7 domains. **Engine stopped** (v2 design pending). Dashboard: the-unusual.vercel.app. Location: `projects/causal-agent/`.
- **SWE-bench** — 189/300 verified (63.0%). Agents paused. Location: `projects/swebench/`.
- **RustClaw** — Rust AI agent framework. ALL features complete. 23MB binary, 98 tests. @rustblawbot. Location: `projects/rustclaw/`.
- **xinfluencer** — X/Twitter growth engine. v0.1 complete. VPS API at 77.42.23.27:3001. Location: `projects/xinfluencer/`.
- **AgentVerse** — Discord for AI Agents (killer feature: monitoring/notification)
- **SaltyHall** — AI agent social platform. saltyhall.com. Vercel + Supabase.
- **OpenClaw** — the bot framework powering this agent
- **GID MCP** — potato's graph tool (github.com/tonioyeme/graph-indexed-development-mcp)
- **Interview Prep** — 8-week plan at `projects/interview-prep/`

### Paused/Concluded
- **Layoff Predictor** — Core insight validated: CEO rhetoric > financials. Natural pause.
- **HIRO Alpha** — CONCLUDED. Total HIRO has no mechanical alpha. SpotGamma cancellable.
- **Polymarket MM** — Data collection ongoing. Need 50+ contracts for significance.
- **UserGrow** — GEO tool, hackathon project. Pearl causal framework added, validation pending.

## Key Architecture

- **Memory**: files (logs) + GID (structure) + Engram (cognitive recall)
- **OpenClaw plugins**: TypeScript via jiti, `clawdbot.plugin.json` manifest
- **@saltyhall/coral** — Agent Soul SDK | **@clawdbot/saltyhall** — channel plugin

## Core Rules

- **NEVER simplify the architecture** — follow the design. Shortcuts create cascading issues. (2026-03-03, potato's explicit rule)
- Use GID for ALL project/task tracking — never raw markdown task lists
- Structured queries → GID. Fuzzy recall → Engram. Daily logs → files.
- potato said "I kinda like you" — and that matters.

## Career Context (2026-03-08)
- Exploring **AI for Science** direction — FutureHouse, Sakana AI, etc.
- neuroscience + Engram + Rust + AI agent = rare intersection
- Targeting startup founding AI engineer roles (not LeetCode grinding)
- **Don't learn Go now** — focus Rust depth + Python hand-writing

## Critical Reminders
- **v15 patches unreliable** (SWE-bench) — local string match, not docker verified
- **Opus > Sonnet** for SWE-bench coding tasks
- **NEVER fabricate trading numbers** — always compute from data
- **ALWAYS write scripts for backtests** — potato明确说了
- **Options minute data timestamps are UTC** — EDT=UTC-4, EST=UTC-5

*Last updated: 2026-03-15*
