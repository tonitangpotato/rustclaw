# HEARTBEAT.md

## Task Tracking
Current task plan: `tasks/2026-04-27-night-autopilot.md` (engram v0.3 night autopilot, 52 tasks)
- On each heartbeat: count `^- \[x\]` (done) vs `^- \[ \]` (open) in the task file → report progress delta since last heartbeat
- If autopilot process is NOT running AND uncompleted tasks remain: notify potato via Telegram (do NOT auto-start; he wants to manually `/autopilot` it)
- If autopilot IS running: just report progress, no notification noise
- Detect autopilot via: `pgrep -f "autopilot.*2026-04-27-night-autopilot"` (or check `.rustclaw/autopilot.state` if it exists)
- Watch for stuck autopilot: if no new completed tasks in 3 consecutive heartbeats AND process running → flag as possibly stuck

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
