---
name: restart-rustclaw
description: Rebuild RustClaw and rolling-restart the three launchd-managed agent instances (main, agent2, marketing)
triggers:
  patterns:
    - "(?i)rolling.?restart"
    - "(?i)apply.*(new )?binary"
  keywords:
    - "重启 rustclaw"
    - "重启agent"
    - "rolling restart"
    - "重新编译"
    - "重编译"
    - "滚动重启"
    - "restart all agents"
    - "pick up new binary"
priority: 85
always_load: false
---

# SKILL: Rolling Restart RustClaw Agents

> Three RustClaw instances share one binary at `/Users/potato/rustclaw/target/release/rustclaw`.
> After `cargo build --release`, each instance must be restarted separately — an already-running
> process keeps its old in-memory binary. This skill rebuilds + rolling-restarts all three.

## When to Use

- potato asks to "重启 RustClaw" / "rolling restart" / "apply 新 binary" / "重新编译"
- You (or another agent) just modified RustClaw source code and need the change live
- Coordinating updates across main, agent2, and marketing instances

## The Three Instances

All managed by launchd (`~/Library/LaunchAgents/`):

| Name | launchd label | Purpose |
|---|---|---|
| main | `com.rustclaw.agent` | RustClaw (this agent, @rustblawbot) |
| agent2 | `com.rustclaw.agent2` | RustClaw 2 |
| marketing | `com.rustclaw.agent-marketing` | Marketing / Grow agent |

## The Script

**Path:** `/Users/potato/rustclaw/scripts/rolling-restart.sh`

### Usage

```bash
# rebuild + restart all three (most common)
/Users/potato/rustclaw/scripts/rolling-restart.sh

# skip cargo build — just restart all (use when binary already fresh)
/Users/potato/rustclaw/scripts/rolling-restart.sh --no-build

# restart only one instance
/Users/potato/rustclaw/scripts/rolling-restart.sh --only main
/Users/potato/rustclaw/scripts/rolling-restart.sh --only agent2
/Users/potato/rustclaw/scripts/rolling-restart.sh --only marketing

# change gap between restarts (default 3 seconds)
/Users/potato/rustclaw/scripts/rolling-restart.sh --gap 5
```

### What It Does

1. Runs `cargo build --release` in `/Users/potato/rustclaw` (unless `--no-build`)
2. Verifies the binary exists and is executable
3. Prints binary mtime + sha256 prefix — so you can confirm it's actually the new version
4. For each agent: `launchctl kickstart -k gui/$UID/<label>` (falls back to stop+start)
5. Waits `--gap` seconds between restarts so they don't collide on shared resources (engram DB, session DB, log files)
6. Reports running pid for each after restart

## ⚠️ Self-Restart Caveat

**If you are the `main` agent, restarting yourself terminates this session.**

- To avoid disrupting the current conversation: use `--only agent2` or `--only marketing` first
- To restart self cleanly: use the `restart_self` tool instead — it exits cleanly and launchd respawns us
- If potato explicitly asks to restart everything including me, confirm first, then run the full script — my next session will pick up the new binary

## Cross-Instance Coordination

If another agent compiled a new binary while I'm still running the old one:

- I will **automatically pick up the new binary** on my next restart (launchd re-reads from disk)
- But during the gap between their restart and mine, we run different versions — this is fine for compatible changes, risky for schema/config breaking changes
- For breaking changes: rolling-restart all three together

## Verification After Restart

```bash
# check all three are running
launchctl list | grep rustclaw

# tail logs to confirm startup
tail -f /tmp/rustclaw-agent.log        # main (check plist for exact paths)
tail -f /tmp/rustclaw-agent2.log
tail -f /tmp/rustclaw-agent-marketing.log
```

## Related

- OpenClaw has its own restart script at `/Users/potato/clawd/projects/openclaw-latest/scripts/restart-mac.sh` — different project, different architecture, don't confuse them
- `restart_self` tool: clean self-restart for the calling agent only
