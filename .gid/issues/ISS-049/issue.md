---
id: "ISS-049"
title: "Skills directory has no hot-reload"
status: open
priority: P3
created: 2026-04-26
component: "src/skills.rs"
---
# ISS-049: Skills directory has no hot-reload — new/edited SKILL.md files require RustClaw restart

**Status**: open
**Severity**: low–medium (UX friction; blocks "drop in a skill, use immediately" flow)
**Filed**: 2026-04-26
**Discovered while**: installing `humanizer` skill (ported from blader/humanizer) into `/Users/potato/rustclaw/skills/humanizer/SKILL.md`

## Symptom

After dropping a new skill into `/Users/potato/rustclaw/skills/<name>/SKILL.md`:

- The file exists on disk.
- The currently running RustClaw process **does not see it** — `matched_skills` for any message containing the skill's trigger keywords stays empty.
- The skill only becomes available after a process restart (manual kill + relaunch, `restart_self`, or launchd respawn).

Same issue applies to **edits** of existing SKILL.md files — frontmatter changes (new triggers, priority bumps, description updates) and body changes are not picked up live.

## Expected behavior

Either:

1. **Live**: a file watcher on `skills/` (analogous to the existing config hot-reload via `notify`/`fsevent`) re-scans the directory on add/modify/remove and rebuilds `SkillRegistry` + `TriggerStrategy`.
2. **At minimum**: a `/reload-skills` admin command or tool that triggers re-scan without full process restart (cheap escape hatch if full hot-reload is too risky to add right now).

## Root cause

Code path verified 2026-04-26 in `src/workspace.rs`:

- `SkillRegistry::load(&skills_dir)` is called **only at Workspace construction** (`workspace.rs:388-389`, also `:933`, `:1110`).
- `TriggerStrategy::from_metadata(...)` is built once from the loaded registry (`workspace.rs:398-400`) and stored.
- `grep "watch\|reload\|notify" src/skills.rs` returns **zero matches** — there is no watcher infrastructure for the skills dir.
- Compare with `src/config.rs`, which **does** have a hot-reload watcher for the config file (proven pattern in this codebase, can be reused).

So the registry + trigger index are baked into the running Workspace and never refreshed. `match_skills(user_message, 5)` (called per inbound message at `workspace.rs:485` and `:856`) consults the stale in-memory registry only.

## Repro

1. Start RustClaw (`./target/release/rustclaw run --config rustclaw.yaml --workspace .`).
2. `mkdir -p skills/test-hot-reload && cat > skills/test-hot-reload/SKILL.md` with valid frontmatter containing trigger keyword `xyzzy-test`.
3. Send a message containing `xyzzy-test` to the bot. → Skill does **not** match, no skill section injected into prompt.
4. Restart the process. Repeat step 3. → Skill matches.

## Fix options

### Option A (recommended): file watcher on `skills/`

- Reuse the `notify` + debounce pattern from `src/config.rs`.
- On `Create`/`Modify`/`Remove` events under `skills/**/SKILL.md`:
  - Re-run `SkillRegistry::load(&skills_dir)`.
  - Rebuild `TriggerStrategy::from_metadata(...)`.
  - Atomic-swap into the live `Workspace` (likely behind `Arc<RwLock<...>>` or similar — check current ownership of `skill_registry` / `trigger_strategy` fields).
- Edge cases to handle:
  - Partial writes (file appearing without complete frontmatter) → log + skip, don't crash.
  - SKILL.md with parse errors → keep previous version of that skill, log the error.
  - Removed skill → drop from registry; if it was `always_load`, regenerate the always-load section on next prompt build.

### Option B (cheaper): `reload_skills` admin tool

- Expose a tool (or CLI command on the running daemon) that calls `SkillRegistry::load` + rebuild on demand.
- No watcher, no async file events. Manual trigger only.
- Good as a **stepping stone** — ship this first, add Option A later if it's worth it.

## Why this matters now

- Skills are increasingly the unit of behavior extension (12 skills in `skills/` as of today).
- The "drop a SKILL.md and it works" workflow is what makes the skill system feel lightweight. Requiring a restart breaks that mental model.
- I (RustClaw) just hit this myself when installing `humanizer` — told the user "hot reload, works immediately", which was **false**. Cost trust + a round-trip to clarify.

## Out of scope

- The auto-skill-generation pipeline in `src/skills.rs` (separate concern; that writes new skills but presumably also requires restart to use them — same root cause, same fix benefits both).
- SKM upstream changes — the fix lives entirely in RustClaw's `Workspace` integration.

## Related

- `src/config.rs` — existing hot-reload watcher to model after.
- `src/workspace.rs` — owner of `skill_registry` and `trigger_strategy`, primary edit site.
- `src/skills.rs` — skill loading helpers.
- TOOLS.md skills section currently claims "Skill is auto-loaded on next message (no restart needed)" — **this is incorrect** and should be updated either when this issue is fixed or now (whichever ships first).
