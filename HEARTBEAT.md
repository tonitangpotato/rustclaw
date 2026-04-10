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

## P0 Issue Scan (每次 heartbeat)
- [ ] Read `.gid/projects.yml` → get project list
- [ ] For each project, read `{path}/.gid/docs/issues-index.md` (skip if missing)
- [ ] Regex extract all `[P0] [open]` issues: `## ISS-(\d+) \[\w+\] \[P0\] \[open\]`
- [ ] If any P0 open found:
  - Check if `[P0] [in_progress]` already exists → skip (already being fixed)
  - Add 🚨 P0 highlight to heartbeat report
  - **直接执行 issue-fix skill workflow** (Step 1-5 from `skills/issue-fix/SKILL.md`):
    1. Read issue details, mark `in_progress` (mutex lock)
    2. Analyze root cause, implement fix via `edit_file`
    3. Run verify_command from `.gid/config.yml` (or language default: `cargo test` / `npm test`)
    4. If verify passes → close issue, record commit hash + summary
    5. If verify fails → revert status to `open`, keep changes, report failure
  - Report result: `✅ P0 ISS-NNN auto-fixed` or `❌ P0 ISS-NNN fix failed: {reason}`
- [ ] Include issue summary in heartbeat report (P0 count, open count across all projects)

## Disk Health (每天一次)
- [ ] Check `df -h /` — if <15GB free, alert potato
