# TOOLS.md - Local Notes

## Self-Configuration

### Config File
- **Path**: `/Users/potato/rustclaw/rustclaw.yaml`
- **Hot-reload**: Config file is watched — changes auto-apply (no restart needed for most settings)
- **Restart needed for**: LLM auth, memory DB path, bot token

### Managing Specialists
Edit `rustclaw.yaml` under `orchestrator.specialists`:

```yaml
    - id: unique-id        # Required: unique identifier
      name: Display Name   # Optional: human-friendly name
      role: builder        # Required: role for task matching
      model: claude-opus-4-6  # Optional: model override
      workspace: /Users/potato/rustclaw  # Optional: working directory
      max_iterations: 25   # Optional: tool loop limit (default 25)
      budget_tokens: 100000  # Optional: token budget
```

After editing, send SIGHUP to reload:
```bash
kill -HUP $(pgrep -f "rustclaw.*run")
```

**Available roles**: `builder` (coding), `research` (analysis), `review` (code review), or any custom role.

**Current specialists:**
- `coder` (Opus 4.6, builder role, 25 iterations, 100k budget)
- `researcher` (Sonnet 4.5, research role, 15 iterations, 50k budget)

### Shared Workspace (两个agent共享)
- **OpenClaw workspace**: `/Users/potato/clawd/`
- **RustClaw workspace**: `/Users/potato/rustclaw/`
- **Global IDEAS.md**: `/Users/potato/rustclaw/IDEAS.md` — 跨项目idea收集
- **OpenClaw projects**: `/Users/potato/clawd/projects/` — gid-rs, agentctl, swebench, autoalpha, causal-agent, xinfluencer, interview-prep
- **RustClaw projects**: `/Users/potato/rustclaw/` — RustClaw本身就是项目
- **Engram DB (shared)**: `/Users/potato/rustclaw/engram-memory.db`
- 两个agent都可以直接读写对方路径下的文件和项目

### Source Code & Binary
- **Binary**: `/Users/potato/rustclaw/target/release/rustclaw` (35MB, v0.1.0)
- **Source**: `/Users/potato/rustclaw/` (same as workspace)
- **Tests**: 140 pass, 0 warnings

### Daemon Management
```bash
cd /Users/potato/rustclaw
./target/release/rustclaw run --config rustclaw.yaml --workspace .
# or with daemon:
./target/release/rustclaw daemon status
./target/release/rustclaw daemon stop
./target/release/rustclaw daemon start -c rustclaw.yaml -w /Users/potato/rustclaw
```

### X/Twitter (bird CLI)

#### Accounts
- **@horseonedragon** (potato's personal/主号): `~/bin/bird-potato` wrapper (auth_token + ct0 baked in)
- **@Toni1161947** (小号/xinfluencer): `~/bin/bird-alt` wrapper (auth_token + ct0 baked in)
- **@salty_hall_bots** (SaltyHall/Developer App): default `bird` command (uses env AUTH_TOKEN)

#### Usage Rules
- **@horseonedragon bird CLI: 完全不碰** — 避免任何风险
- 所有 bird CLI 读操作用 **bird-alt**（小号 @Toni1161947）
- 读主号互动: `~/bin/bird-alt search "to:horseonedragon" -n 20`
- 读某条推回复: `~/bin/bird-alt replies <tweet-url>`
- 所有写操作（发推/follow/reply）用 **X API**（SaltyHall App + OAuth）
- Cookie tokens expire periodically — if auth fails, get new auth_token + ct0 from browser DevTools

### Voice (Built-in)
RustClaw has **built-in voice support** via the `VOICE:` prefix. No external tools needed.
- To send a voice message: just write `VOICE: your text here`
- RustClaw handles TTS and Telegram voice delivery internally

### Engram (Memory System)
- **Integration**: Native Rust crate (`engramai`), built into RustClaw. No CLI needed.
- **Database**: `/Users/potato/rustclaw/engram-memory.db`
- **Auto-recall**: Framework hooks automatically recall relevant memories before every LLM call
- **Auto-store**: Framework hooks automatically store significant content after every LLM call
- **Native tools** (for explicit use when needed):
  - `engram_recall` — search memories by query
  - `engram_store` — store with type + importance
  - `engram_recall_associated` — Hebbian-linked memories
  - `engram_trends` — emotional trends per domain
  - `engram_behavior_stats` — tool success/failure rates
  - `engram_soul_suggestions` — SOUL.md update suggestions
- Consolidation runs during heartbeats automatically

### GID (Graph Indexed Development)
- **Built-in**: GID is integrated into RustClaw (gid-core crate)
- Graph path: `.gid/graph.yml`
- **Core capabilities**:
  - **Code intelligence**: dependency analysis, impact queries, architecture visualization
  - **Task management**: status tracking, dependency DAGs, blockers
  - **Design-to-code**: DESIGN.md → graph → tasks → implementation
- Use GID for: understanding codebase structure, tracking what depends on what, finding impact of changes, managing tasks with dependencies
- **`gid design` workflow**: `gid design <file>` outputs a *prompt*, not a graph. You (the agent/LLM) generate the YAML yourself based on that prompt, then pipe it back: `echo "<yaml>" | gid design --parse` to merge into the graph. Two-step loop.
- 39 commands: `gid tasks`, `gid query impact <node>`, `gid query deps <node>`, `gid visual`, `gid analyze`, etc.

### Dashboard
- **URL**: http://localhost:8081
- **Endpoints**: `/api/tokens`, `/api/orchestrator`
- Agent name read from IDENTITY.md

---

Add whatever helps you do your job. This is your cheat sheet.

### Skills (SKM)
- **Engine**: [skm](https://crates.io/crates/skm) v0.1 (Agent Skill Engine)
- **Skills directory**: `/Users/potato/rustclaw/skills/`
- **Format**: agentskills.io standard SKILL.md (YAML frontmatter + markdown body)
- **Selection**: `skm-select` TriggerStrategy (regex pattern matching, µs latency)
- **always_load**: Skills with `always_load: true` in frontmatter are always injected into system prompt
- **Matched skills**: Triggered skills are injected when user message matches patterns/keywords
- **Auto-skill generation**: `src/skills.rs` — learns from agent experience, generates new SKILL.md files

#### Adding a new skill:
1. Create `skills/my-skill/SKILL.md`
2. Add YAML frontmatter with `name`, `description`, `triggers` (patterns + keywords)
3. Write instructions in markdown body
4. Skill is auto-loaded on next message (no restart needed)

#### Example SKILL.md:
```yaml
---
name: web-scraping
description: Extract content from web pages
triggers:
  patterns: ["https?://"]
  keywords: ["scrape", "fetch page"]
priority: 80
always_load: false
---

# Web Scraping
Instructions for the agent...
```
