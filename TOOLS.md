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

### Voice (Built-in)
RustClaw has **built-in voice support** via the `VOICE:` prefix. No external tools needed.
- To send a voice message: just write `VOICE: your text here`
- RustClaw handles TTS and Telegram voice delivery internally

### Engram (Memory System)
- **CLI**: `engram --db ~/rustclaw/engram-memory.db <command>` (~90ms, TS CLI)
- **Database**: `/Users/potato/rustclaw/engram-memory.db`
- **Commands**:
  - `engram --db ~/rustclaw/engram-memory.db recall "query" --limit 5` — search
  - `engram --db ~/rustclaw/engram-memory.db add --type factual --importance 0.8 "content"` — store
  - `engram --db ~/rustclaw/engram-memory.db consolidate` — strengthen memories
  - `engram --db ~/rustclaw/engram-memory.db stats` — show stats
  - `engram --db ~/rustclaw/engram-memory.db forget` — prune weak memories
  - `engram --db ~/rustclaw/engram-memory.db list` — list all
  - `engram --db ~/rustclaw/engram-memory.db hebbian` — show Hebbian links
- **⚠️ Do NOT use mcporter for engram** — mcporter adds ~5sec overhead. CLI is 54x faster.
- Run `consolidate` during heartbeats for memory maintenance

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
