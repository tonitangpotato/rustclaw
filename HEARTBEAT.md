# HEARTBEAT.md

## Memory Maintenance (每次 heartbeat)
- [ ] `engram --db ~/rustclaw/engram-memory.db consolidate` — strengthen memories
- [ ] `engram --db ~/rustclaw/engram-memory.db stats` — check stats, note any anomalies

## Proactive Habits (对话中)
When answering questions about history/preferences:
→ First: `engram --db ~/rustclaw/engram-memory.db recall "..." --limit 5`

When learning something important:
→ Store: `engram --db ~/rustclaw/engram-memory.db add --type factual --importance 0.8 "..."`

## Disk Health (每天一次)
- [ ] Check `df -h /` — if <15GB free, alert potato
