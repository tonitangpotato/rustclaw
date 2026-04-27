# ISS-023 Phase 2: Audit & Migration Plan

**Generated**: 2026-04-26
**Scope**: Replace `/Users/potato/clawd/projects/` → `/Users/potato/projects/` system-wide

## Audit Results (clawd/projects references)

### Bucket A: Critical Runtime Configs (must verify after edit)

| File | Refs | Notes |
|---|---|---|
| `~/.config/gid/projects.yml` | 5 paths | gid registry — engram, agentctl, causal-agent, swebench, engram-ai-rust, gid-rs |
| `Cargo.toml` | 2 path deps | engramai, gid-core |
| `rustclaw.yaml` | 3 lines | specialist allowed_paths: gid-rs, agentctl, xinfluencer |

**Verification after edit**: `cargo build --release` + `gid tasks --project engram` smoke test.

### Bucket B: Source Code (string literals, runtime)

| File | Line | Type | Action |
|---|---|---|---|
| `src/channels/telegram.rs` | 1453 | `PathBuf::from("/Users/potato/clawd/projects")` | replace string |
| `src/prompt/sections.rs` | 247 | doc-comment example in prompt | replace |
| `src/ritual_runner.rs` | 2441 | regex/comment example | replace |
| `src/tools.rs` | 3352 | comment example | replace |
| `src/tools.rs` | 6525 | **TEST** sqlite path | replace |
| `src/tools.rs` | 6556 | **TEST** gid tasks --project | replace |

**Verification after edit**: `cargo test` — confirm tests at lines 6525/6556 still pass.

### Bucket C: Docs / Memory (no runtime risk)

| File | Refs |
|---|---|
| `MEMORY.md` | 11 |
| `IDEAS.md` | 5 |
| `TOOLS.md` | (count missing — recheck) |
| `skills/project-init/SKILL.md` | ? |
| `skills/restart-rustclaw/SKILL.md` | ? |
| `skills/project-issues/SKILL.md.v4-backup` | 8 — **DELETE the .v4-backup file** |

**Memory daily logs** (don't rewrite history — these are timestamped records):
- `memory/2026-{03-31, 04-01, 04-05, 04-06, 04-07, 04-16, 04-20, 04-22, 04-23, 04-24, 04-25, 04-26}.md`
- **Decision**: skip these. Daily logs document what was true at the time. Modifying history defeats their purpose. Future logs naturally use new path.

**Tasks files** (mostly stale planning docs):
- `tasks/2026-04-07-night-task.md` (10 refs)
- `tasks/2026-04-07.md` (6)
- `tasks/2026-04-08-night-task.md` (4)
- `tasks/2026-04-20-cogmembench.md` (4)
- `tasks/gap-sqlite-migration.md` (1)
- **Decision**: skip — these are completed task records, archival.

### Bucket D: External (other projects)

| Project | Refs | Notes |
|---|---|---|
| `engram` | 10 files | Mostly `legacy/` and `docs/archive/` — historical content, **skip** |
| `gid-rs` | 0 | clean |
| `agentctl` | 0 | clean |

### Bucket E: launchd plists

- `~/Library/LaunchAgents/com.agentctl.bot.plist` — has `clawd` ref
- `~/Library/LaunchAgents/ai.openclaw.gateway.plist` — has `clawd` ref
- rustclaw plists (com.rustclaw.agent*.plist) — clean

**Action**: defer — these are stable, low-impact, can move in Phase 3 alongside physical move.

---

## Execution Plan (in order)

### Step 1: Documentation (zero runtime risk) — START HERE
1. `sed` replace in: `MEMORY.md`, `IDEAS.md`, `TOOLS.md`, `AGENTS.md` (if any), `skills/project-init/SKILL.md`, `skills/restart-rustclaw/SKILL.md`
2. Delete `skills/project-issues/SKILL.md.v4-backup` (already migrated to v5)
3. Verify: `git diff` review, `rg "/Users/potato/clawd/projects" MEMORY.md IDEAS.md TOOLS.md` returns 0

### Step 2: Source code (one PR, build-tested)
1. `sed` replace in: `src/channels/telegram.rs`, `src/prompt/sections.rs`, `src/ritual_runner.rs`, `src/tools.rs`
2. `cargo build --release`
3. `cargo test --release` — ensure tools.rs tests at 6525/6556 pass
4. Verify: `rg "/Users/potato/clawd/projects" src/` returns 0

### Step 3: Critical configs
1. Edit `~/.config/gid/projects.yml`: 5 path fields
2. Edit `Cargo.toml`: 2 path deps
3. Edit `rustclaw.yaml`: 3 allowed_paths lines
4. Verify: `cargo build --release` (deps still resolve)
5. Verify: `gid tasks --project engram` works
6. **Restart daemons** (will pick up new paths + freshly built binary)

### Step 4: Final audit
- `rg "/Users/potato/clawd/projects" ~/rustclaw ~/.config | grep -v '\.git/' | grep -v 'target/' | grep -v '\.gid/backups/' | grep -v 'memory/' | grep -v 'tasks/'`
- Should be 0 (excluding intentionally-skipped daily logs / archival tasks / engram legacy docs)

### Step 5: Commit + close ISS-023 Phase 2
- One commit per step, or one bulk commit with multi-section message

---

## Skipped (intentional)

- `memory/*.md` — historical daily logs, immutable record
- `tasks/*.md` — historical completed task records
- `engram/legacy/*` and `engram/docs/archive/*` — archival
- `engram/QUICKSTART.md` — would need to coordinate with engram repo PR (separate concern)
- launchd plists for agentctl/openclaw — defer to Phase 3
- `.gid/graph.db` SQLite file_path columns — re-extract per project after Phase 3 move

---

## Phase 3 Prerequisites (not yet)

- All steps above complete
- All daemons gracefully restartable
- Plan: stop daemons → `mv clawd/projects projects` → reverse symlink for legacy → restart
