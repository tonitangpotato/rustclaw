# ISS-020: Project Path Discovery Friction

**Status**: superseded-by-feature (2026-04-23)
**Created**: 2026-04-22
**Reporter**: potato
**Severity**: medium (not blocking, but wastes 3-5 tool calls every cross-project session)

> **2026-04-23 update**: This issue is being resolved by **feature-project-registry** in gid-rs.
> Tracked as a 3-part effort:
> - `gid-rs/.gid/issues/ISS-028-project-registry-cli.md` — gid CLI + `~/.config/gid/projects.yml` (directly resolves ISS-020)
> - `gid-rs/.gid/issues/ISS-029-ritual-launcher-work-unit.md` — gid-core ritual API accepts work_unit only
> - `gid-rs/.gid/issues/ISS-030-rustclaw-start-ritual-tool.md` — rustclaw tool adapter
>
> ISS-020 stays open until ISS-028 ships, then close with a reference.

---

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
