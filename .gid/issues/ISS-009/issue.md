---
id: "ISS-009"
title: "Persona System — Multi-Instance Identity + Engram Namespace"
status: closed
priority: P2
created: 2026-04-14
severity: medium
component: "src/config.rs, src/workspace.rs, src/memory.rs"
---
# ISS-009: Persona System — Multi-Instance Identity + Engram Namespace

**Status**: Open  
**Severity**: Medium  
**Component**: `src/config.rs`, `src/workspace.rs`, `src/memory.rs`  
**Date Reported**: 2026-04-14  

## Problem

All rustclaw instances sharing the same workspace get the same personality (SOUL.md, AGENTS.md). There's no way to run multiple instances with different roles (e.g., coding assistant vs marketing advisor) from the same workspace without duplicating the entire directory.

Additionally, engram writes from all instances go into the same namespace, making it impossible to distinguish which persona stored a memory.

## Design

### Persona directory structure

```
rustclaw/
├── personas/
│   ├── default/        # main rustclaw
│   │   ├── SOUL.md
│   │   ├── AGENTS.md
│   │   └── HEARTBEAT.md
│   ├── marketing/
│   │   ├── SOUL.md
│   │   └── AGENTS.md
│   └── dev/            # rustclaw2
│       ├── SOUL.md
│       └── AGENTS.md
├── USER.md             # shared (user info, not persona-specific)
├── TOOLS.md            # shared (environment info, not persona-specific)
├── MEMORY.md           # shared
├── memory/             # shared
├── skills/             # shared
```

**Rule**: Persona-specific files (SOUL.md, AGENTS.md, HEARTBEAT.md) live in `personas/{name}/`. Shared files (USER.md, TOOLS.md, MEMORY.md) stay in workspace root.

### Config

```yaml
persona: marketing   # looks in {workspace}/personas/marketing/
```

No `persona` field → defaults to `personas/default/`.

### Engram namespace

The `persona` value is used as the engram write namespace:
- `persona: marketing` → writes with `namespace=marketing`
- `persona: default` → writes with `namespace=default`

Recall searches across all namespaces (shared memory).

## Changes Required

### 1. `src/config.rs`
- Add `persona: Option<String>` field to config struct

### 2. `src/workspace.rs` — `Workspace::load()`
- Persona files (SOUL.md, AGENTS.md, HEARTBEAT.md): read from `{workspace}/personas/{persona}/`, fallback to workspace root for backwards compatibility
- Shared files (USER.md, TOOLS.md, MEMORY.md, IDENTITY.md, BOOTSTRAP.md): always read from workspace root
- No `persona` set → use `personas/default/` if it exists, else workspace root

### 3. `src/memory.rs` — Engram initialization
- Pass `persona` value as namespace when writing to engram
- Recall: search across all namespaces (no namespace filter)

### 4. File migration
- Move existing `SOUL.md`, `AGENTS.md`, `HEARTBEAT.md` from workspace root to `personas/default/`
- Keep `USER.md`, `TOOLS.md`, `MEMORY.md` in workspace root

## Also addresses

- **Session busy stuck** (found during investigation): `ActiveSessionGuard` RAII pattern added to `telegram.rs` but not yet deployed — include in the same build.
- **`restart_self` tool**: Added to `tools.rs` but not yet deployed — include in the same build.
- **Engram `busy_timeout`**: Added to engram storage but not yet deployed to all instances.

## Test plan

- [ ] `persona: default` loads `personas/default/SOUL.md`
- [ ] `persona: marketing` loads `personas/marketing/SOUL.md`, falls back to root USER.md
- [ ] No `persona` field → backwards compatible (reads from root)
- [ ] Engram writes with correct namespace per persona
- [ ] Engram recall returns results across all namespaces
- [ ] Multiple instances with different personas can run simultaneously
