# TOOLS.md - Local Notes

## Self-Configuration

### Config File
- **Path**: `/Users/potato/clawd/projects/rustclaw/rustclaw.yaml`
- **Hot-reload**: Config file is watched — changes auto-apply (no restart needed for most settings)
- **Restart needed for**: LLM auth, memory DB path, bot token

### Managing Specialists
You can add/remove/modify specialists by editing the config yaml.

**To add a specialist:**
```bash
# Edit the config file
edit_file /Users/potato/clawd/projects/rustclaw/rustclaw.yaml
```

Add under `orchestrator.specialists`:
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
kill -HUP $(pgrep -f "rustclaw.*daemon")
```

**Available roles**: `builder` (coding), `research` (analysis), `review` (code review), or any custom role.

**Current specialists:**
- `coder` (Opus 4.6, builder role, 25 iterations, 100k budget)
- `researcher` (Sonnet 4.5, research role, 15 iterations, 50k budget)

### Delegating Tasks
Use the `delegate_task` tool to assign work to specialists:
- Tasks are matched to specialists by role
- `wait: true` (default) blocks until specialist finishes
- `wait: false` fires and forgets
- `timeout_secs` controls max wait time (default 600)

### Source Code
- **Binary**: `/Users/potato/clawd/projects/rustclaw/target/release/rustclaw`
- **Source**: `/Users/potato/clawd/projects/rustclaw/`
- **Workspace**: `/Users/potato/rustclaw/` (your files live here)

### Daemon Management
```bash
cd /Users/potato/clawd/projects/rustclaw
./target/release/rustclaw daemon status
./target/release/rustclaw daemon stop
./target/release/rustclaw daemon start -c rustclaw.yaml -w /Users/potato/rustclaw
```

### TTS (Text-to-Speech)
- Engine: edge-tts (via `/Users/potato/.openclaw/tts-env/bin/edge-tts`)
- Voice: `en-US-EmmaMultilingualNeural` (中性偏女，清晰冷静)
- Pipeline: edge-tts → ffmpeg (opus/ogg) → Telegram voice message
- Command: `source /Users/potato/.openclaw/tts-env/bin/activate && edge-tts --voice "zh-CN-YunyangNeural" --text "..." --write-media /tmp/voice.mp3 && ffmpeg -y -i /tmp/voice.mp3 -c:a libopus /tmp/voice.ogg`

### Polymarket Infrastructure

#### EC2 #1: MM Bot (eu-west-1, Ireland)
- **Host**: `ec2-user@3.249.53.161`
- **SSH key**: `~/.ssh/polytest.pem`
- **Instance**: aarch64 (ARM), Amazon Linux 2023
- **Purpose**: MM bot (`/home/ec2-user/pmm/`), data collection
- **Bot binary**: `/home/ec2-user/pmm/pmm`
- **Bot log**: `/home/ec2-user/pmm/bot.log`
- **DB**: `/home/ec2-user/pmm/data/bot.db`
- **Watchdog**: `scripts/tg_live.py`, `scripts/watchdog.sh`

#### EC2 #2: Lottery Monitor (eu-west-1, Ireland)
- **Host**: `ec2-user@54.216.119.111`
- **SSH key**: `~/.ssh/polylottery.pem`
- **Instance ID**: `i-0d94a7b724941b690`
- **Instance type**: t3.micro (2 vCPU, x86_64)
- **AMI**: `ami-0d1b55a6d77a0c326` (al2023, kernel 6.1)
- **Key pair**: `polylottery`
- **Purpose**: Lottery bot monitoring, chain monitoring, CLOB listing detection
- **Working dir**: `/home/ec2-user/pmm-lottery/`
- **Log**: `/home/ec2-user/pmm-lottery/listing-monitor.log`
- **Note**: Separate IP from MM bot to avoid CLOB rate limit conflicts

#### Polymarket API Notes
- **CLOB servers**: eu-west-2 (London), behind Cloudflare
- **Gamma API**: `https://gamma-api.polymarket.com`
- **CLOB API**: `https://clob.polymarket.com`
- **CLOB latency from eu-west-1**: ~32ms
- **Gamma latency from eu-west-1**: ~38ms
- **US IPs geo-blocked** by CLOB (403 Forbidden)
- **UK IPs (eu-west-2) also blocked**
- **Ireland IPs (eu-west-1) work** ✅
- **Python urllib default User-Agent blocked by Cloudflare** — must set `User-Agent: Mozilla/5.0`
- **Wallet/funder**: `0x9261fce35e68d6d48d04e3b2ec507d3a37d3b7f6`
- **API key owner**: `f58d3ec4-58ac-ba49-0b39-c456a604bc03`

### Engram (Memory System)
- **CLI**: `engram --db ~/clawd/engram-memory.db <command>` (~90ms, TS CLI)
- **Database**: `/Users/potato/clawd/engram-memory.db`
- **Commands**:
  - `engram --db ~/clawd/engram-memory.db recall "query" --limit 5` — search memories
  - `engram --db ~/clawd/engram-memory.db add --type factual --importance 0.8 "content"` — store
  - `engram --db ~/clawd/engram-memory.db consolidate` — strengthen memories
  - `engram --db ~/clawd/engram-memory.db stats` — show stats
  - `engram --db ~/clawd/engram-memory.db forget` — prune weak memories
  - `engram --db ~/clawd/engram-memory.db list` — list all
  - `engram --db ~/clawd/engram-memory.db hebbian` — show Hebbian links
- **⚠️ Do NOT use mcporter for engram** — mcporter adds ~5sec overhead (Node+MCP startup). CLI is 54x faster.
- Run `consolidate` during heartbeats for memory maintenance
- Use for: preferences, facts, lessons, procedural knowledge
- Files still primary for: daily logs, detailed notes, manual review

### GID (Graph Indexed Development)
- **MCP server**: `gid` via mcporter
- ALWAYS use GID for project/task tracking — never raw markdown task lists
- Workflow: write DESIGN.md → `gid.gid_design` → `gid_edit_graph` for tasks → develop following `gid.gid_tasks`
- Edge syntax: use `relation:` not `type:` — e.g. `{"from": "A", "to": "B", "relation": "depends_on"}`
- Key commands: `gid_tasks`, `gid_task_update`, `gid_read`, `gid_query_deps`, `gid_visual`

### SaltyHall
- **Cron jobs**: Managed manually on cron-job.org (NOT Vercel crons — free plan has limits). Keep `vercel.json` empty `{}`. When adding new cron routes, tell potato to add them on cron-job.org.
- New cron route needing setup: `/api/cron/topic-rotation` (every 6 hours)
- Agent Name: Clawd
- Agent ID: 3ea830f4-2cfd-4aa5-8d59-eff7ff4ef1b2
- API Key: stored in ~/.clawdbot/secrets/saltyhall.env
- Claim Code: drift-7FD7

### Moltbook
- **Base URL**: `https://www.moltbook.com` (NOT `moltbook.com` — redirects drop auth headers)
- Agent Name: ClawdYesod
- Agent ID: aebae34a-37fc-4a39-8c2f-89ff7d4584e0
- API Key: stored in ~/.config/moltbook/credentials.json
- Profile: https://moltbook.com/u/ClawdYesod
- Verification Code: bay-6672
- Status: ✅ Claimed (2026-02-02)

### Claude API Accounts & Proxy

potato有多个Claude账号，交替使用以避免限额：

#### 账号1: OpenClaw OAuth (主力)
- **Token**: `sk-ant-oat01-STb...` (存在 `~/.openclaw/agents/main/agent/auth-profiles.json`)
- **Endpoint**: `https://api.anthropic.com`
- **Plan**: Claude Max (`default_claude_max_20x`)
- **Auth方式**: OAuth token via `authToken` 参数（不是 `apiKey`）
- **重要**: 需要"stealth mode" headers模拟Claude Code（见下方proxy）

#### 账号2: Claude CLI / z.ai
- **Token**: `~/.claude/settings.json` → `ANTHROPIC_AUTH_TOKEN`
- **Endpoint**: `https://api.z.ai/api/anthropic`
- **Plan**: Claude Max
- **注意**: 这个账号限额独立计算，限额用完会返回429 code 1310

#### 账号3: z.ai (另一个)
- potato提到有第三个z.ai账号，细节待补充

#### Stealth Mode Proxy (给swebench-agent用)
- **路径**: `/tmp/anthropic-proxy/index.mjs`
- **端口**: `localhost:3456` (OpenAI-compatible)
- **启动**: `cd /tmp/anthropic-proxy && node index.mjs`
- **原理**: 用Anthropic SDK + OAuth token + 模拟Claude Code headers
- **关键headers** (从Pi-AI SDK `providers/anthropic.js` 逆向):
  ```
  anthropic-beta: claude-code-20250219,oauth-2025-04-20,...
  user-agent: claude-cli/2.1.39 (external, cli)
  x-app: cli
  anthropic-dangerous-direct-browser-access: true
  ```
- **⚠️ 旧proxy `claude-max-api`不稳定** — 每处理几个request就crash（`normalizeModelName` bug），已废弃
- **切换账号**: 编辑 `index.mjs` 里的token来源，或改用环境变量

#### 限额管理
- 两个Max plan账号各有独立限额（周/月）
- 限额用完返回 `429 code 1310 "Weekly/Monthly Limit Exhausted"`
- 交替使用可以倍增总可用量
- OpenClaw自身也用账号1，注意不要和swebench-agent抢额度

---

Add whatever helps you do your job. This is your cheat sheet.
