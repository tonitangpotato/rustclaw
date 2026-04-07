# HEARTBEAT.md

## Task Tracking
Current task plan: `tasks/2026-04-07.md`
If autopilot is not running and there are uncompleted tasks, remind potato or start `/autopilot tasks/2026-04-07.md`.

---

## Memory Maintenance (每次 heartbeat)
- [ ] `engram --db ~/rustclaw/engram-memory.db consolidate`
- [ ] `engram --db ~/rustclaw/engram-memory.db stats`

## Proactive Habits (对话中)
When answering questions about history/preferences:
→ First: `engram --db ~/rustclaw/engram-memory.db recall "..." --limit 5`

When learning something important:
→ Store: `engram --db ~/rustclaw/engram-memory.db add --type factual --importance 0.8 "..."`

## Auto-Triage (每次 heartbeat)
- [ ] Read `.gid/meta-graph.yml` — check for `status: new` action_item nodes
- [ ] For P0 items: read intake source file + target project code → generate epic + task nodes in target project's `.gid/graph.yml` → update meta-graph status to `triaged`
- [ ] For P1 items (if time permits): same as P0 but lower priority
- [ ] P2 items: leave as-is until manually requested
- [ ] Report any newly triaged items to potato via Telegram

## Disk Health (每天一次)
- [ ] Check `df -h /` — if <15GB free, alert potato
