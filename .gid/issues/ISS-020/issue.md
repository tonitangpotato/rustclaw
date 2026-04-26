---
id: "ISS-020"
title: "Project path discovery friction in cross-project tools"
status: open
priority: P2
created: 2026-04-23
component: "src/tools.rs (gid_* tools)"
---
# ISS-020: Project Path Discovery Friction

**Status**: closed (2026-04-23 — resolved by feature-project-registry)
**Created**: 2026-04-22
**Closed**: 2026-04-23
**Reporter**: potato
**Severity**: medium (not blocking, but wastes 3-5 tool calls every cross-project session)

## Resolution (2026-04-23)

The underlying friction is resolved by the three-part **feature-project-registry** rollout:

- ✅ **`~/.config/gid/projects.yml`** exists with 9 canonical project mappings (engram, rustclaw, agentctl, causal-agent, swebench, engram-ai-rust, gid-rs, xinfluencer, autoalpha). Registry entries include `aliases` (e.g., `gid-rs` → alias `gid`, `engram-ai-rust` → alias `old-engram`).
- ✅ **gid-rs ISS-029** — gid-core `WorkUnit` + `RegistryResolver::load_default()` + `resolve_and_validate()`. Library-level resolution with loud failure on registry miss.
- ✅ **rustclaw ISS-022** (this repo) — `start_ritual` tool now requires structured `work_unit`, resolves path via registry, no more text-grep inheritance. 284/284 tests pass.
- ✅ **MEMORY.md** — Canonical Project Roots section (added 2026-04-23) is loaded every main session, gives the agent the registry contents in-context even before tool calls.

**What's left** (tracked as gid-rs ISS-028, not blocking closure of this issue):
- `gid project <list|add|resolve|remove>` CLI subcommands. `projects.yml` exists and the library can read it, but the CLI management commands aren't implemented yet. Current workflow: edit `~/.config/gid/projects.yml` manually. This is workable — the issue's acceptance criteria (agent finds right path on first try) are already met via registry + MEMORY.md + WorkUnit API.

**Verification**:
- Cross-project ritual launches now require explicit `work_unit` (enforced by tool schema) — impossible to silently land in wrong workspace.
- Agent's in-context `MEMORY.md` table lists canonical paths for all 9 projects — no trial-and-error probing needed.

---

## Original Report (kept for history)

## Symptom

When working on an engram task, the agent repeatedly fails to locate the correct project root on the first try. Typical session wastes 3-6 tool calls before finding the right `.gid/` directory.

Observed sequence (2026-04-22 session):

1. `gid_tasks(project="/Users/potato/clawd/projects/engram")` → returns ISS-001 graph (wrong, only 8 nodes)
2. `gid_tasks(project="/Users/potato/clawd/projects/engram/crates/engramai")` → same 8-node ISS-001 graph (also wrong)
3. `find ... ISS-024 ...` searches (timed out once)
4. Eventually discovers real project is `/Users/potato/clawd/projects/engram-ai-rust/` (114 nodes, 88 done, active work)

The agent narrates "let me try another path" each time, which is both slow and visible friction to the user.

---

## Root Causes

### 1. Ambiguous workspace structure (the real problem)

There are **four** engram-shaped directories under `projects/`:

- `autoresearch-engram/` — (unknown purpose, not touched)
- `engram/` — appears to be a release/publish mirror (has `.gid/` but only design docs, no active graph)
- `engram-ai-rust/` — **the actual working repo** (full `.gid/graph.yml` with 114 nodes, all ISS-* issue docs)
- `hermes-engram/` — (unknown, maybe fork)

Only `engram-ai-rust/` has the live task graph. `engram/` has a shadow `.gid/` that looks authoritative but isn't.

Additionally, `engram/crates/engramai/.gid/graph.yml` exists as a second stale graph (ISS-001 era), making the "walk up from source file" heuristic also fail.

### 2. No project registry in the agent's context

The agent has no stable mapping from issue IDs → project roots. Each session rediscovers this by trial and error. MEMORY.md mentions projects by name but not by canonical path.

### 3. Partial memory of the right path

Engram recall eventually surfaces memories mentioning ISS-024, but those memories don't embed the project path. The "real path" is implicit knowledge the agent has to rebuild each time.

---

## Proposed Fixes (ranked by effort/value)

### Fix A (low effort, high value): Project registry file

Create `/Users/potato/rustclaw/PROJECTS.md` or append to `MEMORY.md`:

```
## Canonical Project Roots

- **engram** (active development): /Users/potato/clawd/projects/engram-ai-rust
  - Issues: ISS-001..ISS-024 at .gid/issues/
  - Graph: .gid/graph.yml (114 nodes as of 2026-04-22)
- **engram (release mirror, do not modify .gid/)**: /Users/potato/clawd/projects/engram
- **rustclaw**: /Users/potato/rustclaw
- **agentctl**: /Users/potato/clawd/projects/agentctl
- **xinfluencer**: /Users/potato/clawd/projects/xinfluencer
- **gid-rs**: /Users/potato/clawd/projects/gid-rs
```

This file gets loaded into context every session (via AGENTS.md rule). Cost: ~10 lines of context, saves 3-6 tool calls per cross-project session.

### Fix B (medium effort): Consolidate/clean up stale .gid directories

- Move `engram/crates/engramai/.gid/` somewhere less confusing, or delete if it's truly dead
- Make `engram/` (release mirror) not have a `.gid/` at all, or put a `.gid/README.md` saying "the live graph is in engram-ai-rust/"

This prevents future confusion even if the registry file drifts.

### Fix C (highest effort): Finish the monorepo consolidation from ISS-023

ISS-023 (`repo-consolidation-monorepo.md` in engram-ai-rust) was meant to merge `engram/` and `engram-ai-rust/`. Apparently not complete — finishing that work removes the ambiguity at the source.

### Fix D (tooling): `gid_tasks` fuzzy project resolution

Enhance gid CLI so that `gid_tasks(project="engram")` resolves by name via a registry, not raw path. Requires gid-core change. Worth considering if this friction recurs with other projects.

---

## Recommended Order

1. **Fix A now** (5-minute edit to MEMORY.md) — immediate relief
2. **Fix B this week** — prevent the stale-graph trap from tricking future sessions
3. **Fix C** as part of the existing ISS-023 when it's prioritized
4. **Fix D** only if the problem keeps happening to other projects

---

## Acceptance

- [ ] Next cross-project session finds the right path on first try
- [ ] MEMORY.md (or a linked file) contains canonical project roots
- [ ] Stale `.gid/` directories either removed or marked as shadow
