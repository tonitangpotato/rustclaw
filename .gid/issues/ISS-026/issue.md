---
id: "ISS-026"
title: "start_ritual tool misreports progress (duplicate of ISS-048)"
status: closed
priority: P2
created: 2026-04-24
closed: 2026-04-26
component: "src/tools.rs (start_ritual)"
superseded_by: "ISS-048"
note: "Merged into ISS-048 — same root cause, ISS-048 has the canonical fix."
---
# ISS-026: `start_ritual` tool misreports in-progress rituals as failed + lacks identity context

**Status:** ✅ Resolved (2026-04-26)
**Resolution:** Fixed in `src/tools.rs::StartRitualTool::execute` — split catch-all `_` arm into "healthy mid-ritual state" handler that emits an informational "🔧 Ritual started (id=...); running in background" message and sets `is_error: false`. Only `Escalated` and `Cancelled` now mark the ToolResult as an error. Closed together with ISS-048 (same root cause, single fix). 343 tests pass.
**Severity:** medium — causes agent to mis-trust the tool, fall back to redundant work, and confuse parallel rituals
**Filed:** 2026-04-26
**Reporter:** RustClaw (caught self in the act)
**Related:** rustclaw ISS-022 (work_unit migration, closed), gid-rs ISS-029 (workspace derivation, closed)

## Symptom (concrete incident, 2026-04-26)

Working on gid-rs ISS-032 (`validate cannot repair`). Sequence:

1. Main agent (this rustclaw instance) called `start_ritual({work_unit: {kind:"issue", project:"gid-rs", id:"ISS-032"}, task:"..."})`.
2. Tool returned **`is_error: true`**, message: `"Ritual ended in Initializing phase. ... This usually means project detection failed or the workspace path is wrong."`
3. Main agent interpreted this as "ritual failed" → abandoned the ritual flow, wrote the design + implementation directly.
4. **The ritual was actually running fine** in the background — telegram side reported "Ritual complete!" (potato confirmed).
5. Forensic check after the fact: `cargo check -p gid-core` clean, 706 tests pass — no actual file collision occurred this time. **But the agent could not tell that from the tool's return value alone**, and spent significant follow-up cycles trying to diagnose a non-existent race.

### Severity downgrade note

Original draft of this issue claimed "silent source-tree corruption (two writers on same files)". That was incorrect — forensic analysis (git diff, cargo check, test run) found no collision in this incident. The actual harm is:

- **False negative on the tool's success signal** — agent abandons the ritual flow even when it's working.
- **No identity context in returns** — when potato says "ritual complete" in telegram, agent can't correlate to its own tool call to verify which ritual completed where.
- **The corruption risk is real but latent** — if two writers had touched the same line of `lib.rs` in incompatible ways, this would have been a corruption incident. The tool contract makes that scenario reachable any time the agent doesn't trust the false-error signal but also doesn't realize the ritual is still running.

## Root cause

In `src/tools.rs` (`StartRitualTool::execute`, around line 5798):

```rust
let result = runner.start_with_work_unit(work_unit, task).await;

match result {
    Ok(state) => {
        let phase_name = state.phase.display_name();
        let output = match state.phase {
            RitualPhase::Done       => "✅ Ritual completed successfully!".to_string(),
            RitualPhase::Escalated  => format!("⚠️ Ritual escalated ..."),
            RitualPhase::Cancelled  => "🛑 Ritual was cancelled.".to_string(),
            RitualPhase::WaitingApproval     => format!("⏸️ Ritual paused ..."),
            RitualPhase::WaitingClarification => format!("⏸️ Ritual paused ..."),
            _ => {
                // ← THIS branch fires for Initializing / Researching / Designing / Graphing /
                //   Implementing / Verifying / etc. — i.e. every healthy mid-ritual state.
                let mut msg = format!("Ritual ended in {} phase.", phase_name);
                msg.push_str("\nThis usually means project detection failed ...");
                msg
            }
        };

        Ok(ToolResult {
            output,
            is_error: !matches!(state.phase, RitualPhase::Done),  // ← all non-Done = error
        })
    }
    ...
}
```

Two compounding mistakes:

1. **`start_with_work_unit().await` does not await the terminal phase.** It awaits a single state-machine transition (or whatever the runner's await contract is) and returns the *current* phase, which is almost always a mid-flight phase like `Initializing` or `Researching`.
2. **The fallthrough arm reports any non-terminal phase as a failure** — both via `is_error: true` and via a misleading message that names a specific cause ("project detection failed or workspace path is wrong") that has nothing to do with what actually happened.

Combined effect: every healthy `start_ritual` call returns a result that **looks identical to a real failure**. The agent's only safe options are (a) ignore the tool's `is_error` flag entirely (bad — can't distinguish real failures), or (b) hard-code knowledge that "Initializing phase" actually means "running" (brittle, leaks ritual internals into agent prompt).

## Why this matters

**Direct harms (observed):**
- Agent abandons ritual flow on healthy in-progress rituals → wasted parallel work, divergent implementations.
- No identity in returns → agent cannot correlate `start_ritual` calls to telegram completion signals or to `.gid/rituals/r-*.json` state files. When multiple rituals could be in flight, the agent has no way to disambiguate.
- Misleading error message ("project detection failed or workspace path is wrong") sends agent down a debugging path orthogonal to the actual situation.

**Latent harms (one bad pairing away):**
- If two writers had landed on the same line of the same file with incompatible changes, this would have been a real source-tree corruption incident, caught only when the next cargo build / git review fired. The tool contract makes that scenario reachable any time the false-error fires.

The same failure mode would fire if the agent, on seeing the false error, decided to spawn `spawn_specialist` for the implementation — sub-agent and ritual would race.

## What the contract should be

Pick one — both are defensible, but the current "neither" is the worst option:

### Option A: Tool blocks until terminal phase

`start_ritual` only returns when `state.phase ∈ {Done, Escalated, Cancelled, WaitingApproval, WaitingClarification}`. Mid-flight phases are not observable to the caller.

- Pros: simple contract, agent can trust `is_error`, no race possible because agent is blocked.
- Cons: tool call may take many minutes; agent loses interactivity; if runner crashes mid-phase the tool hangs.
- Mitigation: timeout + status query tool (`/ritual status` already exists for human use; expose as `ritual_status` tool for the agent).

### Option B: Tool returns immediately with "started" semantics

Tool returns within seconds with one of:

- `Started { ritual_id, current_phase }` — `is_error: false`, output explicitly says **"ritual is running in background — DO NOT write source code in this project until /ritual status shows Done. Use ritual_status tool to poll."**
- `LaunchFailed { reason }` — `is_error: true`, message names the actual cause (registry miss, work-unit invalid, runner panic).
- `Done`/`Escalated`/etc. — same as today, terminal phase known.

- Pros: agent can do other work; matches the actual async nature of ritual execution.
- Cons: requires explicit `ritual_status` tool; requires a hard agent-side rule "don't write source while a ritual is active for that project".
- This is closer to how the system actually behaves today, so probably less invasive.

### Either way: fix the message

The fallthrough message **must not invent a diagnosis** ("project detection failed or workspace path is wrong"). If the tool truly doesn't know what state the ritual is in, the message must say so. Lying about the cause sends the agent down a debugging path that has nothing to do with the real situation.

### Every return message must carry ritual identity context

Right now the tool returns generic strings like `"✅ Ritual completed successfully! Final phase: Done"` or `"Ritual ended in Initializing phase."` — **the agent has no way to tell which ritual this is talking about** if more than one is in flight, or to correlate with telegram notifications, or to look up the state file later.

Every return message (success, in-progress, error, terminal) must include:

- **Ritual ID** — the `r-XXXXXX` short id from `RitualState.id`
- **Work unit identity** — `kind` + `project` + (`id` | `name` | `task_id`), e.g. `issue gid-rs/ISS-032` or `feature engram/auth`
- **Target root** — `state.target_root` (resolved absolute path)
- **Current phase** — already there
- **State file path** — `.gid/rituals/r-XXXXXX.json` under the target root, so the agent can read details if needed

Example of the **new** message format:

```
✅ Ritual r-a3f2c1 complete (issue gid-rs/ISS-032)
   Target: /Users/potato/clawd/projects/gid-rs
   Phase:  Done
   State:  /Users/potato/clawd/projects/gid-rs/.gid/rituals/r-a3f2c1.json
```

```
⏳ Ritual r-a3f2c1 in progress (issue gid-rs/ISS-032)
   Target:        /Users/potato/clawd/projects/gid-rs
   Current phase: Researching
   State:         /Users/potato/clawd/projects/gid-rs/.gid/rituals/r-a3f2c1.json
   ⚠️ DO NOT write source code in this project — ritual is actively modifying it.
   Use ritual_status tool with ritual_id="r-a3f2c1" to poll, or wait for telegram notification.
```

```
⚠️ Ritual r-a3f2c1 escalated at Implementing phase (issue gid-rs/ISS-032)
   Target: /Users/potato/clawd/projects/gid-rs
   State:  /Users/potato/clawd/projects/gid-rs/.gid/rituals/r-a3f2c1.json
   Error:  test failure: 3 tests failed in repair::tests
   Use /ritual retry from inside the gid-rs project to retry.
```

Why this matters: when potato says "ritual just finished" in telegram, the main agent should be able to look at its own conversation log, find the matching `start_ritual` return value, and know exactly which ritual + which work unit + which file to read for the verification report. Generic "Ritual complete!" with no identity is a debug black hole.

**This applies to ALL return paths**, including the early-return error paths (`No LLM client available`, `Missing 'work_unit' parameter`, `Invalid 'work_unit' structure`, registry-resolve failure). Those should still echo back what the caller passed (`work_unit`) so the agent's log shows the input → output pair clearly.

## Acceptance criteria

1. Tool no longer returns `is_error: true` for healthy in-progress phases.
2. Tool's output text either (a) accurately reports "ritual still running, do not write source code", or (b) blocks until the ritual reaches a terminal phase.
3. Misleading "project detection failed" boilerplate removed from the fallthrough arm.
4. **Every return message** (success, in-progress, terminal-error, early-error) **carries**: ritual ID (`r-XXXXXX`), work unit identity (`kind` + `project` + id/name/task_id), target root, current phase, and state file path. Generic "Ritual complete!" with no identity is rejected.
5. If Option B is chosen: a companion `ritual_status` tool exists so the agent can poll without re-reading raw state files. The status tool also returns the same identity block.
6. Test added: spawn a ritual that's known to take >5s in early phases, assert the tool's return contract matches whichever option was chosen, AND assert the return text contains the work unit identity (e.g. `"issue gid-rs/ISS-032"`).
7. Cross-process behavior documented: it must be impossible for the main agent to be misled into writing source code in a project where a ritual is actively executing.

## Defense in depth (out of scope but related)

A per-project ritual lock (file or sqlite advisory lock under `.gid/runtime/`) would catch any caller that ignores the tool contract — including human direct edits during a ritual, or two agents on the same machine. Not strictly required to close this issue, but worth filing as a follow-up if Option B is chosen.

## Files likely involved in fix

- `src/tools.rs` — `StartRitualTool::execute` (the buggy match)
- `src/ritual_runner.rs` — `RitualRunner::start_with_work_unit` (the await contract)
- `gid-core/src/ritual/state_machine.rs` — verify which phase the runner actually returns at (terminal vs first-transition)

## Self-review note

I (RustClaw) caused this incident. The right behavior on seeing the misleading error would have been: (a) check `.gid/rituals/r-*.json` for an active state file before assuming failure, (b) ask potato whether a ritual was actually running. Both would have caught the race. Adding a procedural rule to MEMORY.md is a patch — fixing the tool contract is the root fix. This issue is the root fix.
