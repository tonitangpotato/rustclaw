---
id: "ISS-027"
title: "Ritual observer needs context injection"
status: closed
priority: P2
created: 2026-04-26
component: "src/ritual.rs"
related: ["ISS-025"]
---
# ISS-027: Ritual Observer + LLM Context Injection — make ritual state changes visible to the agent without asking

**Status:** open
**Severity:** medium — root-fix for "agent describes stale ritual state" class of bugs; current pull-only model causes mis-reporting to user
**Filed:** 2026-04-26
**Reporter:** RustClaw (caught self mis-describing live ritual phase to potato across multiple turns)
**Depends on:** gid-rs ISS-041 (ritual event bus — producer side)
**Related:**
- rustclaw ISS-026 (start_ritual tool misreports — single-turn symptom of same root)
- gid-rs ISS-040 (paths-as-SSOT — observer must use canonical paths)

---

## Symptom (concrete incident, 2026-04-26)

While discussing ritual `r-9a1bb9` (gid-rs ISS-040 implementation) with potato over Telegram:

1. T+0: agent read `.gid/runtime/rituals/r-9a1bb9.json`, saw phase=WaitingApproval, described it to potato.
2. T+0 to T+6min: agent did unrelated analysis (graph diff inspection, paths.rs review) without re-reading the state file.
3. T+~7min: potato pasted Telegram messages showing ritual had progressed through "review round 2" and entered "Implementing".
4. Agent had to walk back its claim that the ritual was waiting for approval. Visible in transcript as the agent saying "等等. 这个 ritual 实际上没失败" then later "状态早变了".
5. Potato: **"如何避免这个问题之后再发生呢？"**

### Why this is RustClaw's problem (not just gid-rs's)

gid-rs ISS-041 establishes a producer-side event bus — ritual phase transitions get emitted as durable events. But emission alone doesn't fix anything. **Someone has to consume those events and put them in front of the agent before the agent describes ritual state.** That consumer lives in RustClaw, because:

- RustClaw is what runs the agent loop.
- RustClaw is what assembles the LLM context for each turn.
- RustClaw already has hooks for similar concerns (engram auto-recall injects relevant memories pre-call; somatic state injects interoceptive trends).

This issue is the RustClaw counterpart that closes the loop.

---

## Root cause

RustClaw currently has zero awareness of long-running external processes the agent has touched. When the agent calls `start_ritual` or any tool that kicks off a backgrounded operation:

- The tool returns once at call time.
- The state of the operation continues to evolve.
- **Nothing in the framework re-surfaces that evolving state into the agent's context.**

The agent's only options are:
- Remember to poll (fails — discipline alone doesn't survive attention shifts)
- Get told by potato (works but inverts the help direction — potato shouldn't have to inform the agent about the agent's own work)

There is no inbox mechanism for "things that happened in the background since your last turn".

---

## Goals (WHAT)

- **GOAL-1**: When ritual phase transitions occur between agent turns, the agent's next turn must see those transitions as part of its system context — without the agent having to ask for them.
- **GOAL-2**: Notification surface is **scoped per turn** — only transitions that occurred *since the last LLM call ended* are surfaced; older transitions are not re-spammed.
- **GOAL-3**: Notifications include enough identity context for the agent to correlate to its own prior tool calls (ritual_id, project, work_unit, current phase, time-since-transition).
- **GOAL-4**: Observer is **per-project subscribable** — agent can be working with multiple projects simultaneously and gets transitions from all of them.
- **GOAL-5**: Observer must **discover projects automatically** from the canonical project registry (`~/.config/gid/projects.yml` once it exists post-ISS-040, falling back to known paths from rustclaw config in the interim).
- **GOAL-6**: Observer must **survive RustClaw daemon restart** — on restart, replay events since `last_seen_offset` per project so no transitions are silently lost across restart boundaries.
- **GOAL-7**: Notification format is optimized for LLM consumption — short, structured, prefixed (e.g. `## 🔔 Ritual Updates (since last turn)`) so the agent reliably notices and references them.
- **GOAL-8**: Observer is configurable — potato can disable it per channel, set staleness thresholds, or filter to specific projects.

## Guards (CONSTRAINTS)

- **GUARD-1**: Observer must not block or slow LLM call dispatch. If the bus is unreachable or slow, observer skips the injection for that turn and logs a warning — never delays the user-facing turn.
- **GUARD-2**: Observer must not inject into shared/group contexts (Discord groups, Telegram groups with non-potato participants) — ritual state is workflow-private. Same security boundary as MEMORY.md (main-session-only).
- **GUARD-3**: Notification volume must be bounded. If >10 transitions occurred since last turn, summarize ("12 transitions across 3 rituals — most recent: r-9a1bb9 → Done") rather than dump all 12.
- **GUARD-4**: Observer must not require additional LLM calls to format notifications. Pure deterministic formatting from event data.
- **GUARD-5**: Per-project event log paths must be derived via gid-core's path module (post-ISS-040), not hardcoded `.gid/runtime/events.jsonl` literals in RustClaw.
- **GUARD-6**: Observer's offset tracking (per-project `last_seen` markers) must be persisted in RustClaw state, not in the gid project — gid produces events, RustClaw tracks its own consumption.
- **GUARD-7**: No part of this changes existing tool behavior. `start_ritual` still works the same; `gid_tasks` still works the same. The observer is purely additive context.

---

## Acceptance criteria

- [ ] `RitualObserver` background task spawned at RustClaw daemon startup. Logs "ritual observer started, watching N projects" on boot.
- [ ] Project discovery: reads from `~/.config/gid/projects.yml` if present (post-ISS-040); falls back to `rustclaw.yaml` known-projects list during transition period.
- [ ] Per-project file watcher on `<project>/.gid/runtime/events.jsonl` using `notify` crate. Verified that touching the file triggers an event in <500ms.
- [ ] Per-project offset persisted in `~/.local/share/rustclaw/ritual_observer_offsets.json` (or sqlite, decide in design). Verified survives daemon restart.
- [ ] Pre-LLM-call hook: assembles `RitualUpdate` block from events-since-last-turn-per-channel and injects into system prompt. Verified by capturing a system prompt during a turn after a transition occurred.
- [ ] Inject format: markdown section starting with `## 🔔 Ritual Updates`, listing each transition with ritual_id, project, from→to phase, age. Format finalized in design doc.
- [ ] Volume cap: >10 transitions → summarized form. Verified by emitting 15 events between turns and checking inject is summarized.
- [ ] Group-chat suppression: in chat contexts marked as group/shared, observer block is omitted. Verified for Telegram group, Discord channel.
- [ ] Channel-level config: `rustclaw.yaml` accepts `observer.ritual.enabled`, `observer.ritual.staleness_threshold_secs`, `observer.ritual.project_filter`. Verified each setting respected.
- [ ] Restart replay: kill daemon mid-ritual, restart, verify next agent turn surfaces transitions that occurred during downtime.
- [ ] Failure mode: corrupt events.jsonl line — observer logs warning, skips line, continues. Does not crash.
- [ ] Failure mode: events.jsonl missing — observer treats as "no transitions", does not error.
- [ ] No new clippy warnings, all new code unit-tested, no behavior change for existing tests.

---

## Design sketch (non-binding)

### Architecture

```
┌─────────────────┐         ┌─────────────────────────┐
│ gid ritual      │ writes  │ <project>/.gid/runtime/ │
│ engine (gid-rs) │────────▶│ events.jsonl            │
└─────────────────┘         └─────────────────────────┘
                                       │
                                       │ notify watcher
                                       ▼
                            ┌──────────────────────┐
                            │ RitualObserver task  │
                            │ (rustclaw background)│
                            └──────────────────────┘
                                       │
                                       │ events queue per channel
                                       ▼
                            ┌──────────────────────┐
                            │ Pre-LLM-call hook    │
                            │ (system prompt inj.) │
                            └──────────────────────┘
                                       │
                                       ▼
                            agent sees transitions
                            in next turn's context
```

### Inject example

```markdown
## 🔔 Ritual Updates (since last turn)

- **r-9a1bb9** [gid-rs / ISS-040 paths-as-SSOT]
  - Reviewing → WaitingApproval (3m ago)
  - WaitingApproval → Implementing (1m ago, current)
  - Tasks: 4/16 complete
- **r-7c2ff1** [engram / ISS-021 phase 2]
  - Implementing → Done (8m ago, terminal)

If you describe these rituals, use this state — earlier readings of state files are stale.
```

The trailing nudge is critical — without it the LLM may still describe stale info from earlier in conversation history. With it, the LLM is explicitly anchored to fresh state.

### Per-project offset state

```json
// ~/.local/share/rustclaw/ritual_observer_offsets.json
{
  "version": 1,
  "projects": {
    "/Users/potato/clawd/projects/gid-rs": {"last_seen_offset": 8420, "last_event_ts": "2026-04-26T16:35:57.632971Z"},
    "/Users/potato/clawd/projects/engram": {"last_seen_offset": 1240, "last_event_ts": "2026-04-26T15:12:03.001Z"}
  }
}
```

Per-channel "since last turn" is computed from `(global_offset - channel_last_turn_offset)` rather than persisting per-channel offsets — channels are ephemeral, the per-project file offset is the source of truth.

---

## Phasing

### Phase 1: Background observer + offset persistence (no injection yet)
- Spawn observer task at daemon start
- Watch project event files (use rustclaw.yaml project list initially)
- Persist offsets, log events to rustclaw daemon log
- **Deliverable**: observer running, events flowing into rustclaw, but agent doesn't see them

### Phase 2: Pre-LLM-call injection hook
- Hook into existing system-prompt assembly path
- Inject formatted update block when transitions exist
- Group-chat suppression
- Volume cap
- **Deliverable**: agent sees ritual updates in context

### Phase 3: Project registry integration (post-ISS-040)
- Switch project discovery from rustclaw.yaml to `~/.config/gid/projects.yml`
- Handle dynamic project add/remove via registry watch
- **Deliverable**: zero rustclaw config required for new projects

### Phase 4: Configuration + polish
- Channel-level enable/disable
- Staleness thresholds
- Project filters
- Documentation
- **Deliverable**: feature ready for wider use

---

## Out of scope (deferred)

- **Other long-running operation observers** (background coding tasks, swebench runs, etc.) — same pattern will apply, but each producer needs its own bus first. This issue establishes the pattern for ritual; others copy it when needed.
- **Human-facing dashboard view of ritual events** — could subscribe to same bus, but is a dashboard concern, separate issue.
- **Cross-machine observation** (agent on machine A observing ritual on machine B) — single-machine first; networked observation is a future concern that needs the bus to be network-transparent first (gid-rs ISS-041 out-of-scope).
- **Smart filtering** (only notify on "significant" transitions like Done/Failed) — v1 surfaces all transitions; smart filters added later if signal-to-noise becomes an issue.

---

## Risk

- **R1**: notify-crate file watch unreliable across all OSes (kqueue on macOS has had issues — see prior fd-leak fix in 2026-03-29 notes). **Mitigation**: fall back to 5s polling if notify watcher errors; covered by integration test.
- **R2**: Inject block ignored by LLM if poorly formatted or buried. **Mitigation**: place inject near top of system context (above skill injections), use distinctive emoji prefix, terminal nudge sentence.
- **R3**: Inject block too large during long approval-waiting cycles. **Mitigation**: GUARD-3 volume cap + dedup (same transition seen twice across restart shouldn't double-list).
- **R4**: Race between observer reading events.jsonl and producer writing. **Mitigation**: append-only file + line-based reads handle this naturally; producer-side guarantees in ISS-041 GUARD-2.

---

## Why this is the root fix (not a patch)

L1 (agent discipline rule "always re-read state before describing"): tried in AGENTS.md, **failed in the originating incident**. Discipline cannot survive attention shifts.

L2 (faster polling tool / more ergonomic state command): treats friction, not the missing-event problem. Agent still has to remember to poll.

L3 (per-tool stale-state guards inside RustClaw): would patch one specific tool family at a time. Doesn't generalize to dashboard, MCP, or future tools.

**L4 (this issue + gid-rs ISS-041): event-driven architecture eliminates polling entirely.** Once events flow on a bus and the framework injects them into context, the agent **physically cannot** describe stale state — fresh state is in the prompt. The fix lives at the framework layer where it should: the agent loop manages the agent's awareness of the world, not the agent's discipline.

This pair (ISS-041 + ISS-027) generalizes: any future long-running operation (background sub-agent task, batch swebench run, etc.) just needs to emit on a similar bus, and the same observer pattern surfaces it. We're building a generic "agent inbox for background activity" abstraction, with ritual as the first user.
