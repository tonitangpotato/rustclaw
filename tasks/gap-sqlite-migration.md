# Gap Analysis: sqlite-migration (4 sub-features)

**Date:** 2026-04-08
**Codebase:** `/Users/potato/clawd/projects/gid-rs/`

## Summary

| Sub-feature | GOALs | ✅ Done | ⚠️ Partial | ❌ Missing | Coverage |
|---|---|---|---|---|---|
| Storage (1.x) | 21 (+5 sub) | 20 | 1 | 1 | ~90% |
| Migration (2.x) | 11 | 11 | 0 | 0 | 100% |
| History (3.x) | 9 | 4 | 2 | 3 | ~55% |
| Context (4.x) | 13 | 11 | 1 | 1 | ~88% |

**Overall: ~83% implemented.** Core storage + migration + context are solid. History is the main gap — still YAML-based, not yet migrated to SQLite backup API.

---

## Sub-feature 1: Storage Layer (requirements-storage.md)

### Schema & Tables
| GOAL | Description | Status | Notes |
|---|---|---|---|
| 1.1 | Unified `nodes` table | ✅ | All types supported, `node_type` column |
| 1.2 | 21 dedicated columns | ✅ | Matches spec exactly + bonus columns (parent_id, depth, complexity, is_public, body) |
| 1.3 | `node_metadata` KV table | ✅ | PK (node_id, key), FK CASCADE |
| 1.4 | `node_tags` table | ✅ | PK (node_id, tag), FK CASCADE |
| 1.5 | `edges` with weight/confidence/metadata | ✅ | AUTOINCREMENT PK, all columns present |
| 1.6 | `knowledge` table (JSON blobs) | ✅ | findings, file_cache, tool_history columns |
| 1.7 | FTS5 `nodes_fts` | ✅ | id, title, description, signature, doc_comment + sync triggers |
| 1.8 | `change_log` audit table | ✅ | All columns match spec (batch_id, actor, operation, context) |

### GraphStorage Trait
| GOAL | Description | Status | Notes |
|---|---|---|---|
| 1.9 | Trait: sync, `&self`, object-safe | ✅ | All methods sync + `&self` via RefCell<Connection> |
| 1.9a | CRUD ops | ✅ | get_node, put_node, delete_node, get_edges, add_edge, remove_edge |
| 1.9b | query_nodes + search | ✅ | NodeFilter struct with all fields, FTS5 search |
| 1.9c | Tags & metadata | ✅ | get/set_tags, get/set_metadata |
| 1.9d | Project & knowledge | ✅ | get/set_project_meta, get/set_knowledge |
| 1.9e | Counts & enumeration | ✅ | get_node_count, get_edge_count, get_all_node_ids |
| 1.10 | SqliteStorage struct | ✅ | 2,120 lines, 76 tests |
| 1.11 | WAL + PRAGMAs | ✅ | journal_mode=WAL, foreign_keys=ON, synchronous=NORMAL, busy_timeout=5000 |

### Other
| GOAL | Description | Status | Notes |
|---|---|---|---|
| 1.12 | Indexes | ✅ | 12 indexes including node_type, file_path, status, edges, tags, metadata |
| 1.13 | All call sites updated | ❌ Missing | **scheduler.rs (6 calls), executor.rs (5 calls) still use load_graph/save_graph YAML.** CLI is migrated but harness/ritual internals are not. |
| 1.14 | Backend detection | ✅ | `detect_backend()` + `resolve_backend()` + CLI `--backend` flag |
| 1.15 | Batch operations | ✅ | `execute_batch(&self, ops: &[BatchOp])` with 7 op variants |
| 1.16 | `config` table | ✅ | schema_version initial row, project_meta read/write |
| 1.17 | SQLITE_BUSY handling | ⚠️ Partial | busy_timeout=5000 set, but no descriptive "database is locked" error wrapping — relies on rusqlite default error |
| 1.18 | Observability logging | ✅ | `tracing::debug!` on all write ops with node/edge IDs |

---

## Sub-feature 2: Migration (requirements-migration.md)

| GOAL | Description | Status | Notes |
|---|---|---|---|
| 2.1 | Auto-detect YAML + prompt | ✅ | `detect_backend()` returns Yaml when only .yml exists |
| 2.2 | Prefer graph.db | ✅ | SQLite takes precedence in detection |
| 2.3 | `gid migrate` — nodes + metadata promotion | ✅ | 5-phase pipeline: Parse→Validate→Transform→Insert→Verify |
| 2.4 | Edges with weight/confidence | ✅ | Edge.confidence written to dedicated column |
| 2.5 | Knowledge transfer | ✅ | JSON serialization of findings/file_cache/tool_history |
| 2.6 | Post-migration validation | ✅ | Count matching + verify phase |
| 2.7 | Backup to .yml.bak | ✅ | `backup_source()` function |
| 2.8a | Error: db already exists | ✅ | Config option + error handling |
| 2.8b | Error: no YAML found | ✅ | Parse phase checks |
| 2.9 | Parse errors + dangling edges + dupes | ✅ | Validate phase with diagnostics (DuplicateNodeId, DanglingEdgeRef, etc.) |
| 2.10 | Progress logging | ✅ | MigrationReport with all counts + elapsed time |

---

## Sub-feature 3: History (requirements-history.md)

| GOAL | Description | Status | Notes |
|---|---|---|---|
| 3.1 | Save via SQLite backup API | ❌ Missing | **Still YAML-based** — copies .yml files, not .db snapshots. No `rusqlite::backup` usage. |
| 3.2 | ISO 8601 filename + index.json | ⚠️ Partial | Has timestamp filenames + HistoryEntry struct with metadata. But uses .yml extension, not .db |
| 3.3 | Max 50 + pruning | ✅ | MAX_HISTORY_ENTRIES = 50, prune_old_entries exists |
| 3.4 | `gid history list` | ✅ | CLI command exists, reverse chronological |
| 3.5 | `gid history restore` + auto-save | ⚠️ Partial | restore exists, auto-save exists, but no 50-limit edge case protection |
| 3.6 | Error on invalid timestamp | ✅ | Error handling exists |
| 3.7 | `gid history diff` | ✅ | GraphDiff with added/removed/modified + 10-item truncation in display |
| 3.8 | Diff between two historical snapshots | ❌ Missing | Only current-vs-snapshot, not snapshot-vs-snapshot |
| 3.9 | Observability logging | ❌ Missing | No elapsed time or file size logging |

---

## Sub-feature 4: Context (requirements-context.md)

| GOAL | Description | Status | Notes |
|---|---|---|---|
| 4.1 | `gid context --targets --max-tokens` | ✅ | CLI command exists, `assemble_context()` in gid-core |
| 4.2 | Token budget (bytes/4) | ✅ | `estimate_tokens_str()` uses bytes/4 |
| 4.3 | Priority truncation | ✅ | `budget_fit_by_category()` — targets→direct deps→callers→tests→transitive |
| 4.4 | 5-tier relevance ranking | ✅ | `relation_rank()` + `score_candidate()` with exact tier mapping |
| 4.5 | Relevance score in output | ✅ | ScoredCandidate.score visible in output |
| 4.6 | `--targets` parameter | ✅ | Comma-separated, at least one required |
| 4.7 | `--depth` parameter | ✅ | Default 2 (req says 3 — minor deviation) |
| 4.8 | `--include` pattern filter | ⚠️ Partial | ContextFilters struct exists, CLI flag exists. Need to verify glob matching |
| 4.9 | `--format json|yaml|markdown` | ✅ | OutputFormat enum, CLI flag |
| 4.10 | `estimated_tokens` in output | ✅ | TraversalStats includes estimated_tokens |
| 4.11 | Node details for AI consumption | ✅ | id, file_path, signature, doc_comment, edge relation all included |
| 4.12 | Library-first architecture | ✅ | `assemble_context()` in gid-core, CLI is thin wrapper |
| 4.13 | Observability logging | ❌ Missing | TraversalStats struct exists but no stderr logging in CLI |

---

## GUARD Compliance

| GUARD | Description | Status |
|---|---|---|
| GUARD-3 | No data loss during migration | ✅ Validated (5-phase pipeline + verify) |
| GUARD-5 | YAML still works | ✅ `load_graph_auto()` falls back to YAML |
| GUARD-8 | `&self` for concurrent reads | ✅ RefCell<Connection> with `&self` |
| GUARD-10 | Object-safe trait | ✅ Sync trait, BatchOp command pattern |

---

## Remaining Gaps (Priority Order)

### P0 — Blockers

1. **GOAL-1.13: Call site migration** — `scheduler.rs` (6 calls) and `executor.rs` (5 calls) still use `load_graph`/`save_graph` YAML pattern. These are the harness and ritual internals. All CLI commands are already migrated to GraphContext.

### P1 — Important but not blocking

2. **GOAL-3.1: History → SQLite backup API** — History is still YAML-based. Need to add `rusqlite::backup` for SQLite snapshots. This is a significant rewrite of history.rs.
3. **GOAL-3.8: Diff between two historical snapshots** — Only current-vs-snapshot exists.
4. **GOAL-3.9: History observability** — No elapsed time/file size logging.
5. **GOAL-4.13: Context observability** — TraversalStats exists but not logged to stderr.
6. **GOAL-1.17: SQLITE_BUSY descriptive error** — Currently relies on raw rusqlite error message.

### P2 — Nice to have

7. **GOAL-4.7: Default depth** — Spec says 3, implementation uses 2. Minor.
8. **GOAL-3.5: 50-limit edge case** — No protection when restore target would be pruned.
