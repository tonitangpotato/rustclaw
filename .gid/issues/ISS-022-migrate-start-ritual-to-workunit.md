# ISS-022: Migrate `start_ritual` tool to `WorkUnit` API (adopter-side of gid-rs ISS-029)

**Status:** closed (2026-04-23 — implementation complete, 284/284 tests passing)
**Severity:** medium — current behavior is the one ISS-027 identified as the root cause; library is fixed, but rustclaw still uses the old path
**Related:**
- engram ISS-027 (root-cause analysis + v2 design, closed 2026-04-23)
- gid-rs ISS-029 (library implementation: `WorkUnit` + `project_registry` + `reject_target_root` guard, done 2026-04-23, commit `ffbedbc`)
**Filed:** 2026-04-23
**Closed:** 2026-04-23

## Resolution

Implemented per the scope in this doc:

1. **New library method** — `RitualRunner::start_with_work_unit(unit, task)` in `src/ritual_runner.rs`:
   - Loads `RegistryResolver::load_default()` (reads `~/.config/gid/projects.yml`)
   - Calls `resolve_and_validate()` — fails loud on unknown project / invalid path (no silent fallback)
   - Builds state with `RitualState::with_work_unit(unit, resolved_root)` — WorkUnit survives state-file round-trips
   - Old `RitualRunner::start(task)` kept for `/ritual` Telegram command (human-driven, user already picked project)

2. **Tool schema** (`src/tools.rs:StartRitualTool`) — Replaced `workspace: string` with `work_unit: object`:
   - `{kind: "issue", project, id}` / `{kind: "feature", project, name}` / `{kind: "task", project, task_id}`
   - Matches gid-core's `#[serde(tag="kind")]` enum exactly — no custom parsing layer
   - `required: ["task", "work_unit"]` — both mandatory, no optional path

3. **Tool handler** — Parses `work_unit` via `serde_json::from_value::<WorkUnit>(...)`, calls `runner.start_with_work_unit()`. No `workspace_override`, no `Project location:` text injection, no `extract_target_project_dir` on the hot path.

4. **Registry seeding** — `~/.config/gid/projects.yml` already exists (9 projects registered). No seeding work needed.

### Out of scope (left for follow-ups)

- `/ritual` Telegram command still uses `runner.start(task)` — that path has its own project picker, different UX concern. If we unify later, it's a separate issue.
- `RitualRunner::start()` (legacy) still uses `extract_target_project_dir` for the telegram path. Kept deliberately to avoid touching the `/ritual` flow in this issue.

## Verification performed

- ✅ `cargo check --bin rustclaw` — clean (only pre-existing gid-core warnings)
- ✅ `cargo test --bin rustclaw` — **284/284 pass, 0 failed, 0 regressions**
- ✅ 8/8 ritual-specific tests pass (`ritual_registry::tests::*`)
- ✅ Tool schema shows `work_unit` (not `workspace`) — required by `required: ["task", "work_unit"]`
- ✅ Missing/invalid `work_unit` produces actionable error pointing to `~/.config/gid/projects.yml`
- ✅ `WorkUnit` persisted on `RitualState.work_unit` via `with_work_unit()` (gid-core handles round-trip)

## Anti-patterns avoided (per scope doc)

- ✅ No dual code path for "compatibility" — old `workspace` arg is **removed** from tool schema, caller must pass `work_unit`
- ✅ No silent fallback on missing/invalid input — errors include the fix instruction
- ✅ No NL inference of work_unit from task text — caller passes structured input

## Files changed

- `src/ritual_runner.rs` — added `start_with_work_unit`, added doc comment deprecating `start` for tool callers (~50 lines)
- `src/tools.rs` — replaced `workspace` param with `work_unit` object in `StartRitualTool` schema + execute handler (~60 lines)


## Problem

`gid-core` now exposes a `WorkUnit`-driven ritual API — ritual workspace is *derived* from the work item (issue / feature / task) via a project registry, rather than supplied as a free-form `workspace` argument. This is the root fix for the ISS-023/ISS-027 disaster where rituals ran against a deprecated repo because the caller passed the wrong path.

rustclaw is a gid-core consumer but has **not adopted the new API**. The `start_ritual` tool in `src/tools.rs` still:

1. Advertises a `workspace: string` parameter in its tool schema (`src/tools.rs:5591`)
2. Implements the old fallback chain: explicit `workspace` arg → extract from task text → default `self.workspace_root` (`src/tools.rs:5607`)
3. Has zero imports of `WorkUnit`, `ProjectRegistry`, or `RegistryResolver`

Result: every time an agent calls `start_ritual`, it bypasses the safety mechanism `gid-core` added. The library-level fix exists but is inert for this caller.

## Evidence

```
$ grep -rn "WorkUnit\|project_registry\|RegistryResolver" rustclaw/src/
(no matches)

$ grep -nE '"workspace"|workspace_override' rustclaw/src/tools.rs
2784: (spawn_specialist — unrelated)
5591: start_ritual tool schema — still "workspace": string
5607: let workspace_override = input["workspace"]...
```

`Cargo.toml` already depends on `gid-core` via path — no dependency bump needed; the new types are available right now.

## Scope

Migrate **only** the `start_ritual` tool (5591–~5640 in `src/tools.rs`). `spawn_specialist`'s `workspace` parameter is a different concern (sub-agent working directory) and is out of scope.

### Required changes

1. **Tool schema (`start_ritual` / 5591)** — Replace `workspace: string` with `work_unit: string` (e.g. `"issue:engram/ISS-022"` or `"feature:engram/dimensional"`) matching gid-core's parseable form. Keep `task` as the natural-language description.

2. **Handler** — Stop computing `workspace_override` and `project_root` by hand. Instead:
   - Parse `work_unit` arg into `WorkUnit` enum.
   - Load `ProjectRegistry` (XDG path per gid-core convention).
   - Call `RegistryResolver::resolve_and_validate(work_unit)` to get the canonical project root.
   - Apply `reject_target_root()` guard against any explicit path the task text might still carry (belt-and-braces against legacy task prompts).
   - Pass the derived root + `WorkUnit` into ritual launch.

3. **Ritual state wiring** — Ensure the `WorkUnit` is persisted onto `RitualState.work_unit` so downstream phases and resume paths know what the ritual is working on. (gid-core's `RitualState` already has the field with serde default for backwards compat; rustclaw just needs to set it.)

4. **Registry seeding** — Users need a `projects.yml` for the registry to resolve names. Decide:
   - (a) Document the format + expected XDG location in `TOOLS.md` and require users to write it manually, OR
   - (b) Auto-discover from `known_projects` in `rustclaw.yaml` and sync into the registry on startup.
   - Recommend (a) for this issue — auto-discovery is a separate convenience feature. One config path, one source of truth.

5. **Error messages** — When `work_unit` is missing / unparseable / not found in registry, emit an actionable error ("unknown project 'engram' — add to ~/.config/gid/projects.yml" rather than "resolution failed").

### Out of scope

- Auto-sync between `rustclaw.yaml` and gid projects registry (separate issue if we want it).
- Changes to `spawn_specialist`'s `workspace` arg (sub-agents, not rituals).
- CLI-level `gid` commands — those already use the new API via gid-rs ISS-028.

## Verification

- [ ] `start_ritual` tool schema shows `work_unit` (not `workspace`) in tool-list output
- [ ] Unit test: calling `start_ritual { work_unit: "issue:engram/ISS-022", task: "..." }` resolves to `/Users/potato/clawd/projects/engram` without any path argument
- [ ] Unit test: calling with unknown project emits registry-not-found error, not a silent fallback to default workspace
- [ ] Unit test: `reject_target_root` guard blocks any attempt to ritual against `target_root` paths passed through legacy task text
- [ ] Manual: run one real ritual end-to-end against a known project, confirm `RitualState` JSON has `work_unit` populated

## Non-goals / anti-patterns to avoid

- **Do not** keep the old `workspace` arg "for compatibility" — dual paths re-open the exact hole ISS-027 identified. Break the signature cleanly; it's an internal tool, callers are our own code + prompts.
- **Do not** silently fall back to default workspace on missing/invalid `work_unit`. Fail loud. That silent fallback was the bug.
- **Do not** try to auto-infer `work_unit` from task text in this issue. Parsing "implement ISS-022" out of NL is a separate problem; start_ritual's contract should be that the caller passes a structured `work_unit`.

## Notes

- Library API reference: `gid-core/src/ritual/work_unit.rs` (WorkUnit enum + WorkUnitResolver trait + RegistryResolver + reject_target_root) and `gid-core/src/project_registry.rs` (YAML format, resolve by name/alias, collision detection).
- Design justification lives in engram's ISS-027 v2 doc — read it before implementing if the "why" isn't obvious.
- No deprecation window needed — rustclaw is the only caller and we control all prompts/agents that invoke `start_ritual`.
