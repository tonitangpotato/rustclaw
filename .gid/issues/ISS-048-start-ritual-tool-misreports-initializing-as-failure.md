# ISS-048: `start_ritual` tool reports "Ritual ended in Initializing" while ritual is still running

**Status**: ✅ Resolved (2026-04-26)
**Resolution**: Applied Option B (fix the report only) in `src/tools.rs::StartRitualTool::execute`. The previous catch-all `_` arm conflated healthy mid-ritual states (Initializing/Triaging/Designing/Implementing/etc.) with terminal failure. Now those states emit a "🔧 Ritual started; running in background" message with the ritual id and target root, and `is_error` is set only for `Escalated`/`Cancelled`. Option A (await terminal/paused state) deferred — the new message correctly tells callers to use `/ritual status` or read the JSON file. Closed together with ISS-026. 343 tests pass.
**Severity**: medium (UX bug, ritual still works in background but caller cannot tell)
**Filed**: 2026-04-26
**Related**: ritual rewrite c1d8b98 (2026-04-06)

## Symptom

Calling the `start_ritual` MCP tool returns:

```
Ritual ended in Initializing phase.
Target root: /Users/potato/clawd/projects/gid-rs
This usually means project detection failed or the workspace path is wrong.
```

This is **misleading**. The ritual has not ended; project detection has not failed; the workspace path is correct. The ritual continues running in the background and reaches a real terminal/paused phase (often `WaitingClarification` from triage, or `Done` if it completes).

## Root cause

In `src/ritual_runner.rs::advance()` (post-rewrite c1d8b98), event-producing actions (`DetectProject`, `RunTriage`, `RunSkill`, ...) are spawned via `tokio::spawn` and the call returns the *intermediate* state immediately. The recursive `advance()` chain that drives the ritual to a paused/terminal state runs entirely in background tasks.

`start_with_work_unit().await` therefore returns a `RitualState { phase: Initializing, ... }` to the caller — the state right after `(Idle, Start) → Initializing + DetectProject`.

In `src/tools.rs::run_ritual_v2_with_work_unit`, the `match state.phase` block has explicit arms for `Done | Escalated | Cancelled | WaitingApproval | WaitingClarification`, then a catch-all `_` arm that emits the "Ritual ended in Initializing phase. ... project detection failed" message — assuming the state is always terminal. It is not.

Verified by:
- Calling `start_ritual` for ISS-045 → tool returned the misleading message
- 8 seconds later, `.gid/rituals/r-4cc542.json` showed `phase: WaitingClarification` (triage decided the probe task was ambiguous)
- The state file was written to `/Users/potato/rustclaw/.gid/rituals/`, not the target project's `.gid/rituals/` (separate concern, see "Related findings")

## Why this didn't break older rituals

Older successful rituals (e.g. `r-e07fa7.json`, 2026-04-20, ISS-015 `Done`) reached terminal phase the same way — in background tasks. The `start_with_work_unit` caller still saw the misleading intermediate state but the ritual itself completed. Users only notice the bug now because:
- Recent rituals more often pause at `WaitingClarification` (triage tightening)
- `start_ritual` was added/exposed as a callable tool (rather than CLI `gid ritual run`)

## Fix options

**Option A (recommended): make `start_with_work_unit` await terminal/paused state.**
Add an event-channel synchronization so the initial `advance` call waits until the ritual reaches `is_terminal() || is_paused()`. Match the existing CLI `gid ritual run` semantics.

**Option B: fix the report only.** If the returned phase is non-terminal/non-paused, emit a different message:
```
🔧 Ritual started (id=…); running in background. Current phase: Initializing.
Use /ritual status or check .gid/rituals/<id>.json for progress.
```
This is a 5-line change in `src/tools.rs` and is the **minimum** fix.

**Option C: both.** Do B now (correctness of report), then A as a follow-up (better UX — caller blocks until paused/done).

## Related findings (separate issues, not blocking)

1. **State file location** — `RitualRunner::new(project_root=…)` sets `rituals_dir = project_root.join(".gid/rituals")`. When called from RustClaw, `project_root` is RustClaw's workspace (`/Users/potato/rustclaw`), not the target project. State files for cross-project rituals end up in RustClaw's `.gid/rituals/` instead of the target project's. This is harmless for execution (state still loads) but breaks discoverability — `gid ritual status` run from the target project sees nothing. Track separately.

2. **Triage over-sensitivity** — A short probe task ("Tracking-only ritual probe to capture init failure mode") was classified as `WaitingClarification`. May be intentional (good triage = ask when unsure) but worth a calibration pass.

## Acceptance criteria

- Calling `start_ritual` for a real task returns one of:
  - `✅ Ritual completed`
  - `⚠️ Ritual escalated`
  - `🛑 Ritual cancelled`
  - `⏸️ Ritual paused — waiting for approval/clarification`
  - `🔧 Ritual started in background (phase: <X>)` — for non-terminal returns
- No path produces "Ritual ended in Initializing phase" with the false "project detection failed" hint when project detection actually succeeded.
- Regression test in `src/tools.rs` (or `src/ritual_runner.rs`) covers a Start → Initializing return and asserts the report message format.
