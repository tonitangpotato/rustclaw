# HEARTBEAT.md

## 🚨 Polymarket MM Bot Monitor (ACTIVE — OVERNIGHT WATCH MODE)
**potato is sleeping. Check EVERY heartbeat. You have authority to STOP the bot if critical.**
- [ ] Run: `bash ~/clawd/projects/polymarket-mm/scripts/watchdog.sh`
- [ ] If exit code 2 (CRITICAL): **STOP the bot** via `ssh -i ~/.ssh/polytest.pem ec2-user@3.249.53.161 'pkill -f "^\./pmm"'` and alert potato
- [ ] If exit code 1 (WARNING): log it, alert potato if repeated 3x
- [ ] If bot NOT RUNNING unexpectedly: alert potato immediately
- [ ] If balance < $8: STOP the bot
- [ ] If 5+ consecutive losses: STOP the bot
- [ ] If SSH fails: retry once, then alert potato
- [ ] Log check result to memory/2026-03-18.md

## 🚨 SWE-bench Monitor (ACTIVE — until run completes)
- [ ] Check: are 4 swebench-agent processes running? (`ps aux | grep swebench-agent`)
- [ ] Check: is proxy alive? (`curl -s http://localhost:3456/v1/models`)
- [ ] Check: progress count (`ls results/v17-verified/*.json | wc -l`)
- [ ] If agents < 4: check logs, clean failed results, restart missing agents with latest binary
- [ ] If proxy down: restart it (`claude-max-api &`), agents will auto-retry
- [ ] If all 500 done: calculate final score, message potato with results

## 📊 Node Snapshot Monitor (ACTIVE)
- [ ] Check: `sqlite3 projects/causal-agent/replay.db "SELECT COUNT(*), MAX(timestamp) FROM node_snapshots;"` — should grow every 30min
- [ ] If no new snapshots in >1h during market hours: check if snapshot_collector.py is running in start.sh loop
- [ ] Weekly: `sqlite3 replay.db "SELECT node_id, COUNT(*) FROM node_snapshots GROUP BY node_id;"` — ensure all observable nodes being captured

## Memory Maintenance (每次 heartbeat)
- [ ] `engram --db ~/clawd/engram-memory.db consolidate` — 运行巩固
- [ ] `engram --db ~/clawd/engram-memory.db stats` — check stats, note any anomalies

## Proactive Habits (对话中)
When answering questions about history/preferences:
→ First: `engram --db ~/clawd/engram-memory.db recall "..." --limit 5`

When learning something important:
→ Store: `engram --db ~/clawd/engram-memory.db add --type factual --importance 0.8 "..."`

**When starting work on a project:**
→ Botcore auto-loads GID, tasks appear in context automatically
→ For NEW projects: Write DESIGN.md, then `gid.gid_design`
→ For projects WITHOUT botcore: Follow manual workflow (see TOOLS.md)

## Disk Health (每天一次)
- [ ] Check `df -h /` — if <15GB free, clean: `~/.claude/projects/`, `/tmp/*.log`, brew/npm/pip caches

## Periodic Checks (每天轮换)
- 早上: 检查日历、邮件
- 下午: 检查 mentions
- 晚上: 更新 memory files, consolidate engram
