# Night Tasks — 2026-04-07

> **Priority order**: P0 bugs → infomap-rs → SQLite migration → remaining phases
> **Verified**: All file paths and line numbers confirmed against current codebase

---

## Phase 1: gid-rs P0 Bugs (CRITICAL)

> Target: `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/`
> Source: `docs/CODE-AUDIT-R2-20260407.md`

### BUG-R2-01: UTF-8 Truncation Panic — 8 sites

`&s[..N]` byte-slices on UTF-8 strings → panics if boundary falls inside multi-byte char.

**Existing fix**: `V2Executor::safe_truncate()` at `ritual/v2_executor.rs:803` already does it right.

**Fix**: Extract `safe_truncate()` into shared `utils.rs`, replace all 8 sites:

| # | File | Line | Pattern |
|---|------|------|---------|
| 1 | `harness/notifier.rs` | 289 | `&reason[..500]` |
| 2 | `harness/replanner.rs` | 306 | `&s[..max_len]` |
| 3 | `harness/scheduler.rs` | 468 | `&s[..max_len]` |
| 4 | `ritual/notifier.rs` | 238 | `&error[..500]` |
| 5 | `ritual/notifier.rs` | 309 | `&error[..500]` |
| 6 | `ritual/gating.rs` | 334 | `&s[..max]` |
| 7 | `code_graph/lang/typescript.rs` | 596 | `&first_line[..100]` |
| 8 | `code_graph/lang/rust_lang.rs` | 672 | `&first_line[..100]` |

Steps:
1. Create `crates/gid-core/src/utils.rs` with `pub fn safe_truncate(s: &str, max_bytes: usize) -> &str`
2. Copy implementation from `v2_executor.rs:803-811`
3. Replace all 8 sites with `utils::safe_truncate()`
4. Update `V2Executor` to call shared version too
5. Add `mod utils;` to `lib.rs`
6. Move tests from v2_executor to utils

---

### BUG-R2-02: RefCell\<Connection\> is !Send

`storage/sqlite.rs:54` — `RefCell<Connection>` prevents `SqliteStorage` from crossing thread boundaries.

**Fix**: Replace `RefCell<Connection>` with `Mutex<Connection>`.

```
File: storage/sqlite.rs
Line 6:  use std::cell::RefCell;  →  use std::sync::Mutex;
Line 54: conn: RefCell<Connection>,  →  conn: Mutex<Connection>,
Line 85: conn: RefCell::new(conn),  →  conn: Mutex::new(conn),
All method bodies: .borrow() → .lock().unwrap()
All method bodies: .borrow_mut() → .lock().unwrap()
```

---

### BUG-R2-03: SQL Injection in LIMIT/OFFSET

`storage/sqlite.rs:556,564` — `format!` string interpolation for LIMIT/OFFSET values.

**Fix**: Use parameterized queries.

```
Line 556: sql.push_str(&format!(" LIMIT {}", limit));
       →  sql.push_str(" LIMIT ?"); params.push(Box::new(limit as i64));

Line 564: sql.push_str(&format!(" OFFSET {}", offset));
       →  sql.push_str(" OFFSET ?"); params.push(Box::new(offset as i64));
```

Note: `limit` and `offset` are `usize` from NodeFilter, so injection risk is low (integer types), but parameterized queries are the correct pattern.

---

## Phase 2: gid-rs P1 Bugs

> Target: `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/`

### BUG-R2-04: TOCTOU Race in ExecutionState

`harness/execution_state.rs:61-89` — `load()` reads JSON, `save()` writes JSON. Concurrent scheduler + CLI can lose `cancel_requested` flag.

**Fix**: Atomic write via tmp+rename:
```rust
// In save():
let tmp = gid_dir.join("execution-state.json.tmp");
fs::write(&tmp, serde_json::to_string_pretty(&self)?)?;
fs::rename(&tmp, gid_dir.join("execution-state.json"))?;
```

---

### BUG-R2-05: Self-Referential Edges Accepted

`graph.rs:372,403` — `add_edge()` and `add_edge_dedup()` don't check `from == to`.

**Fix**: Add guard at top of both functions:
```rust
if edge.from == edge.to {
    return; // or return false for add_edge_dedup
}
```

---

### BUG-R2-06: project_nodes() Includes Code Nodes

`graph.rs:704` — Nodes without `source` field default to project layer. Code nodes extracted without `source: "code_extract"` leak through.

**Fix**: Add secondary check on `node_type`:
```rust
pub fn project_nodes(&self) -> Vec<&Node> {
    self.nodes.values().filter(|n| {
        let is_code = matches!(n.node_type.as_deref(),
            Some("file") | Some("class") | Some("function") | Some("method") | Some("module"));
        !is_code && n.source.as_deref() != Some("code_extract")
    }).collect()
}
```

---

### BUG-R2-07: topological_sort() False Positive Cycle

`query.rs:164` — If duplicate node IDs exist, `sorted.len() != nodes.len()` triggers false cycle detection.

**Fix**: Compare against unique count:
```rust
let unique_count = nodes.iter().map(|n| &n.id).collect::<HashSet<_>>().len();
if sorted.len() != unique_count {
    bail!("Cycle detected");
}
```

---

### BUG-R2-08: save_graph() Non-Atomic Write

`parser.rs:15` — Direct `fs::write()` to `graph.yml`. Crash mid-write = corrupt file.

**Fix**: Write to `.yml.tmp` then `fs::rename()`:
```rust
pub fn save_graph(graph: &Graph, path: &Path) -> Result<()> {
    let tmp = path.with_extension("yml.tmp");
    let yaml = serde_yaml::to_string(graph)?;
    fs::write(&tmp, &yaml)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
```

---

### BUG-R2-09: Merge Functions Drop Done/In-Progress Status ⬆️ Upgraded from P1

**Two affected functions, both lose task progress on re-merge:**

**Site 1**: `graph.rs:538` — `merge_feature_nodes()`
- Step 2 calls `remove_node()` on all old feature tasks (including `done` ones)
- Step 4 calls `add_node()` with incoming nodes (from design YAML, default `todo`)
- Result: all done/in_progress status reset to todo

**Site 2**: `unify.rs:180` — `merge_project_layer()` ← **MORE SEVERE**
- Called by `design --parse` (the main design→graph pipeline)
- `drain(..)` removes **ALL** project-layer nodes, then adds new ones from LLM output
- Result: **every** task in the graph loses its status on any `design --parse` re-run
- This is the primary entry point — affects every project using the design workflow

**Trigger conditions:**
- Re-running `gid design --parse` on a project with in-progress work
- Re-running `merge_feature_nodes()` for a feature with completed tasks
- Any ritual that regenerates the graph from design YAML

**Fix — both functions need status preservation:**

```rust
// Before removing old nodes, snapshot their statuses:
let status_map: HashMap<String, NodeStatus> = old_nodes.iter()
    .filter(|n| matches!(n.status, NodeStatus::Done | NodeStatus::InProgress))
    .map(|n| (n.id.clone(), n.status.clone()))
    .collect();

// After adding new nodes, restore preserved statuses:
for (id, status) in &status_map {
    if let Some(node) = graph.get_node_mut(id) {
        node.status = status.clone();
    }
}
```

Apply this pattern in:
1. `graph.rs:538` — `merge_feature_nodes()` before Step 2 / after Step 4
2. `unify.rs:180` — `merge_project_layer()` before drain / after insert
3. Add tests: merge with done nodes → verify status preserved

---

## Phase 3: gid-rs P2 Risks

> Target: `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/`

- [x] **RISK-R2-01**: No self-edge detection in Validator — `validator.rs`. Add `find_self_edges()`.
- [x] **RISK-R2-02**: `execution_state.json` no schema version — `harness/execution_state.rs`. Add `version: u32` with `serde(default)`.
- [x] **RISK-R2-03**: Regex recompilation in extract — `code_graph/lang/rust_lang.rs:672-697`. 5× `Regex::new().unwrap()` per file. Use `OnceLock`.
- [x] **RISK-R2-04**: `ready_tasks()` is O(n×m×n) — `graph.rs:753`. Build adjacency HashMap for O(1) dep lookup.
- [x] **RISK-R2-05**: History snapshots not pruned on save — `history.rs`. ✅ Already correct — `cleanup()` is called in `save_snapshot()` (line 151), `list_snapshots()` is read-only. No change needed.

---

## Phase 4: infomap-rs

> Target: `/Users/potato/clawd/projects/infomap-rs/`
> Git: `95b192e` (initial) + `88e8055` (.gitignore)
> Status: 17 tests pass, core algorithm working

### ✅ Done
- [x] Network representation (directed, weighted graph)
- [x] PageRank flow calculation (power iteration + teleportation)
- [x] Map equation L(M) computation
- [x] Optimization with local moves (multi-trial, deterministic seed)
- [x] Two-triangle benchmark (2 communities detected correctly)
- [x] Design doc (`.gid/features/infomap-core/design.md`)
- [x] Benchmark suite (`benches/infomap_bench.rs`)

### Remaining

- [x] **Hierarchical recursive decomposition** — currently `hierarchy.rs` returns flat tree. Implement recursive sub-module detection: take each community → treat as sub-network → run Infomap again → repeat until no improvement. ✅ Implemented with MAX_DEPTH=10, MIN_RECURSIVE_SIZE=4, subgraph extraction on Network, 5 new tests (22 total).
- [x] **Coarsening phase** — Collapse modules into super-nodes, run local moves on coarsened graph, map back. Validates against original graph codelength to prevent teleportation artifacts. ✅
- [x] **Incremental ΔL optimization** — No-clone delta computation: builds temporary modules for only the two affected modules. Eliminated O(n) partition clone per move evaluation. ✅
- [x] **Benchmark graphs** — Added Zachary karate club (34 nodes, detects faction leaders in different modules) and large ring-of-cliques (50 nodes, detects ~10 modules). ✅
- [x] **gid-rs integration** — Wire into `advise` command at `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/advise.rs`. Add `detect_modules()` function that runs Infomap on the code dependency graph and suggests module groupings. ✅ Commit `2f635a4`. Feature-gated under `infomap` feature. 7 new tests, 500 total pass.
- [x] **Publish to crates.io** — Clean API, docs, README, `cargo publish`. *(Deferred: needs review of public API surface)* ✅ Published v0.1.0. Commit `625fb3c`.

---

## Phase 5: Graph Cleanup

> Target: `/Users/potato/clawd/projects/gid-rs/.gid/graph.yml`

- [x] Mark `iss006-lsp-incremental` as done (commit `e5a98bf`) ✅
- [x] Mark `iss006-tests` as done (25 tests in `e5a98bf`) ✅
- [x] Mark `feat-unified-graph` as done (umbrella) ✅
- [x] Mark `ug-t4.1-code-graph-migration` as done (commit `3a1f7fd`) ✅
- [x] Mark `ug-t4.2-rustclaw-tools` as done (commit `c183a46`) ✅
- [x] Mark `ritual-v2-triage` as done (skip_design implemented) ✅

---

## Phase 6: SQLite Migration — Storage Layer

> Target: `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/storage/`

- [x] **storage-neighbors**: BFS neighbor query using recursive CTE in `sqlite.rs`. Add `fn neighbors(&self, id, depth, direction) -> Vec<Node>`. ✅ Implemented with Direction enum (Outgoing/Incoming/Both), recursive CTE, depth cap at 10. 17 new tests.
- [x] **storage-tests**: Comprehensive tests for SqliteStorage — CRUD, FTS search, query_nodes filtering, metadata/tags, batch operations. ✅ 46 new tests (76 total sqlite tests). Found bug: `execute_migration_batch` FK-disable is no-op inside transaction.

---

## Phase 7: SQLite Migration — Migration Pipeline

> Target: `/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/storage/migration.rs`

- [x] **migration-metadata**: Verify PROMOTED_KEYS logic handles all metadata fields from design.
- [x] **migration-tests**: Tests for YAML→SQLite pipeline — parse, validate, deduplicate, transform, insert, verify roundtrip.

---

## Phase 8: SQLite Migration — History + Context

### History (`history.rs`)
- [x] **history-tests**: Snapshot save/list/diff/restore tests. Roundtrip correctness, diff accuracy, restore overwrites. ✅ 51 new tests (56 total history tests). Coverage: all-fields roundtrip, unicode, multiple sequential snapshots, message preservation, list ordering/filtering, diff (added/removed/modified nodes, edges, symmetric property, display format, truncation), restore (overwrite, auto-snapshot, data preservation, nonexistent version), cleanup/pruning, large graph stress test (100 nodes), all 7 NodeStatus variants, KnowledgeNode roundtrip, edge cases. Total gid-core: 599 tests, 0 failures.

### Context Assembly (`harness/context.rs`)
- [x] **context-scoring**: Verify edge-relation 5-tier ranking per design. ✅ Implemented `relation_rank()`, `relation_score()`, `score_candidate()`, `score_candidates()` from design §5 + `Candidate`/`ScoredCandidate` types. 45 new tests (56 total context tests). All GOAL-4.4 tiers verified: tier completeness, monotonicity, case sensitivity, composite scoring with exact values, NaN guard, hop-distance decay, realistic scenarios. gid-core: 644 tests, 0 failures.
- [x] **context-truncation**: Verify category-based truncation priority (targets kept, deps trimmed first).
- [x] **context-source**: Verify source code loading from disk.
- [x] **context-tests**: Tests for assemble_task_context — scoring, truncation, source loading, edge traversal.

### Context Partitioning (moved from Phase 13 — this is gid-rs, not Engram)
> Feature dir: `.gid/features/context-partitioning/` (design.md + requirements.md + reviews/design-r1.md)
> Docs migrated from sqlite-migration/ subdocs to independent feature directory.

- [x] **requirements**: 13 GOALs (GOAL-4.1 through GOAL-4.13), P0/P1/P2
- [x] **design**: 793-line design doc covering query, traversal, scoring, truncation, source loading
- [x] **design review**: design-r1.md
- [x] **GOAL-4.1** [P0]: Structured output (target details + source + deps + callers + tests) ✅
- [x] **GOAL-4.2** [P0]: Token budget (bytes/4 estimation) ✅
- [x] **GOAL-4.3** [P0]: Category-based truncation priority ✅ `budget_fit_by_category()`
- [x] **GOAL-4.4** [P0]: 5-tier edge scoring ✅ `relation_rank()`, `relation_score()`, `score_candidate()`
- [x] **GOAL-4.5** [P1]: Relevance score visible in output ✅
- [x] **GOAL-4.6** [P0]: Multi-target support ✅ (via `assemble_task_context`)
- [ ] **GOAL-4.7** [P1]: `--depth` parameter ❌ NOT IMPLEMENTED — no `ContextQuery` struct, no depth control
- [ ] **GOAL-4.8** [P1]: `--include <pattern>` filter ❌ NOT IMPLEMENTED — no `ContextFilters`
- [ ] **GOAL-4.9** [P1]: `--format json|yaml|markdown` ❌ NOT IMPLEMENTED — no CLI command yet
- [x] **GOAL-4.10** [P0]: `estimated_tokens` field ✅ `estimate_tokens_for_candidate()`
- [x] **GOAL-4.11** [P1]: Node details (id, file_path, signature, doc_comment, relation) ✅
- [ ] **GOAL-4.12** [P2]: Library function + thin CLI wrapper ❌ PARTIAL — library exists, CLI `gid context` command does NOT exist
- [ ] **GOAL-4.13** [P1]: Traversal stats logging ❌ NOT IMPLEMENTED
- **Implementation**: `harness/context.rs` 2,826 lines, 189 functions, 119 tests
- **Status**: Core pipeline done (P0 GOALs ✅). CLI surface + query parameters (4 P1/P2 GOALs) not yet wired.

---

## Phase 9: SQLite Migration — CLI & Integration

- [x] **cli-wiring**: Wire CLI commands to SqliteStorage backend. Add `--backend sqlite` flag or auto-detect.
- [x] **backend-detect**: Auto-detect YAML vs SQLite — if `graph.db` exists use SQLite, else YAML. In `storage/mod.rs`.
- [x] **integration-tests**: ✅ 12 end-to-end tests in `storage/integration_tests.rs` (migration roundtrip, CRUD, FTS, query_nodes, tags/metadata, batch, context assembly, budget truncation, history snapshot/diff/restore, neighbor queries, edge add/remove, all_node_ids). 28 total integration tests pass.

---

## Phase 10: Frictionless Graph

- [x] **about**: ✅ `gid about` command — `cmd_about_ctx()` at main.rs:1014. Project stats, node/edge counts.
- [ ] **watch**: File watcher + auto-extract on source changes. ❌ Not implemented (optional, high complexity — deferred).
- [x] **type-inference**: ✅ `infer_node_type()` at graph.rs:342. Auto-called in `add_node()` (graph.rs:381). Supports: file, fn/func, struct/class, mod/module, method, trait/interface, enum, const/static, test, impl.
- [x] **compact-output**: ✅ `--compact` flag at main.rs:103, used in `cmd_tasks_ctx()`. Dense one-line-per-task format.
- [ ] **frictionless-tests**: Partial — about/compact/type-inference tested via full test suite. No dedicated test file. watch not implemented.

---

## Phase 11: ISS-009 Phase 2 — Cross-Layer Auto-Linking

> `link_tasks_to_code()` at `graph.rs:229` exists but is never called automatically.

- [x] Auto-call `link_tasks_to_code()` in `build_unified_graph()` — ✅ unified.rs:147
- [x] `parse_design_yaml()` generate `implements` edges — ✅ Working as designed: `generate_scoped_graph_prompt()` instructs LLM to produce `implements` edges (design.rs:225), parsed by `parse_llm_response()`. Prompt-driven, not hardcoded — correct for LLM workflow.
- [x] `gid extract` auto-trigger cross-layer linking — ✅ main.rs:2331 calls `link_tasks_to_code(&code_graph, &mut graph)` after extract + semantify.
- [x] Tests: ✅ `test_cross_layer_query_traversal` in unify.rs
- [x] Close ISS-010 in ISSUES.md — ✅ already closed

---

## Phase 12: Ritual Fixes

> Target: gid-rs (`crates/gid-core/src/ritual/`) + RustClaw (`/Users/potato/rustclaw/src/`)

- [x] **verify-after-implement**: ✅ Implement phase prompt includes `cargo check` as final step (v2_executor.rs:724). Prompt-enforced, not code-enforced.
- [x] **post-edit compilation hook**: ✅ `WriteFileTool` syntax validation at tools.rs:637. Runs after write for Rust, Python, TS, JS, JSON, YAML.
- [x] **ritual project selector**: ✅ Full implementation in telegram.rs: `discover_projects()` (line 1343), inline keyboard with `__ritual_project:` callback (line 1271), handler (line 1619), `pending_ritual_tasks` HashMap.
- [x] **sub-agent wait rules**: ✅ Added to AGENTS.md — `wait: false` fire-and-forget warning.

---

## Phase 13: Engram

> Target: `/Users/potato/clawd/projects/engram-ai-rust/`

- [ ] T4.1 Business Plan: Read ENGRAM-V2-DESIGN.md + IDEAS.md → write BUSINESS-PLAN.md ⚠️ SKIPPED: agent error or stopped
- [ ] T4.2 ISS-001 P0 consolidate corruption: Read INVESTIGATION-2026-03-31.md → fix → test → commit ⚠️ SKIPPED: agent error or stopped
- [ ] T4.3 Bracket Resolution Skill: Write `skills/bracket-resolution/SKILL.md` ⚠️ SKIPPED: agent error or stopped
- [ ] T4.4 Engram Hub: Write requirements (`.gid/features/engram-hub/requirements.md`) ⚠️ SKIPPED: agent error or stopped
- [ ] T4.5 Engram Share Memory: Write requirements (`.gid/features/share-memory/requirements.md`) ⚠️ SKIPPED: agent error or stopped
- ~~T4.6 Context Partitioning~~ → Moved to Phase 8 (gid-rs, not Engram)

---

## Phase 14: Engram 梳理

> Target: `/Users/potato/clawd/projects/engram-ai-rust/`

- [ ] T5.1 文档汇总: Collect all TODO/FIXME/design docs → write TODO-MASTER.md ⚠️ SKIPPED: agent error or stopped
- [ ] T5.2 Feature Dirs: Create `.gid/features/<name>/requirements.md` for each P0/P1 item ⚠️ SKIPPED: agent error or stopped
- [ ] T5.3 ISS-002~007 简单修复: Read each issue → fix → test → commit ⚠️ SKIPPED: agent error or stopped
