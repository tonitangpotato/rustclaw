# ISS-024: gid_* built-in tools cannot target arbitrary graph.db paths

**Status:** todo
**Priority:** P1
**Filed:** 2026-04-24
**Related:** ISS-020 (project path discovery friction)

## Problem

RustClaw's built-in `gid_*` tools (gid_read, gid_tasks, gid_add_task, gid_update_task, gid_complete, gid_query_impact, gid_query_deps, gid_validate, gid_advise, gid_visual, gid_history, gid_refactor, gid_extract, gid_schema, gid_design, gid_plan, gid_stats, gid_semantify, gid_complexity, gid_working_memory, gid_ignore, gid_infer, gid_context, gid_task_context — 24 tools) call `gid-core` directly via static linking. Path resolution is centralized in `GraphManager::resolve_external` (`src/tools.rs:3205`).

Current behavior with `project: <dir>` argument:

1. If `<dir>/.gid/` exists → use `<dir>/.gid/graph.{db,yml}`
2. Else → `find_project_root(<dir>)` walks upward to nearest `.git` marker → use `<that root>/.gid/graph.db`

**Limitation:** there is no way to specify an arbitrary graph file path (e.g., `.gid-v03-context/graph.db` or any non-`.gid/`-named directory). The `find_project_root` upward walk silently routes the call to the main project DB, and the silent fallback to main DB is *worse* than an error — it produces tool results that look successful but operate on the wrong graph.

The CLI (`gid --graph <path>`) supports this via the `--graph` flag. Built-in tools do not. This forces shell-out to CLI for any non-standard graph location, which:
- Defeats the performance benefit of crate static linking (function call vs fork+exec)
- Inconsistent UX: agent must remember which tool flavor supports which paths
- No type/schema validation on CLI args

## Root Fix

Add a `graph_path` parameter to all 24 `gid_*` tools. When provided, it bypasses `.gid/` directory inference and loads the graph file directly via `gid_core::load_graph_auto` with explicit backend detection from file extension.

Precedence: `graph_path` > `project` > workspace default.

## Proposed Changes

**File: `src/tools.rs`**

1. **`gid_project_property()` (~line 3286)** — extend helper to return both `project` and `graph_path` schema entries so all tools pick them up automatically. Either rename to `gid_graph_properties()` returning `Vec<(String, Value)>`, or add a sibling `gid_graph_path_property()` and call both at each tool's `input_schema`.

2. **`GraphManager::resolve()` (~line 3184)** — add `graph_path` short-circuit before existing `project` branch:
   ```rust
   if let Some(gp) = input.get("graph_path").and_then(|v| v.as_str()) {
       return self.resolve_by_graph_path(gp).await;
   }
   match input.get("project").and_then(|v| v.as_str()) {
       None => Ok((self.workspace_graph.clone(), self.workspace_path.clone())),
       Some(d) => self.resolve_external(d).await,
   }
   ```

3. **New method `resolve_by_graph_path(&self, path: &str)`** — accepts an absolute or relative `.db`/`.yml` path. Detects backend from extension. Loads via `load_graph_auto` with parent dir as `gid_dir`. Caches under the canonical path key. Returns `(SharedGraph, SharedPath)`.

4. **Cache key normalization** — use `canonicalize()` so `./graph.db`, `graph.db`, and absolute path map to the same cache entry. Avoid double-loading.

5. **Schema doc** — describe precedence: "If `graph_path` is set, it overrides `project`. Use `graph_path` for non-standard graph file locations (e.g., parallel working DBs not in `.gid/`)."

## Acceptance Criteria

- [ ] All 24 `gid_*` tools accept optional `graph_path` parameter in their JSON schema
- [ ] `gid_read(graph_path=".gid-v03-context/graph.db")` correctly returns the v03 graph contents (not main DB)
- [ ] `gid_add_task(id="X", graph_path="...")` writes to the specified file, not the inferred path
- [ ] When `graph_path` and `project` are both provided, `graph_path` wins; this is documented in the schema description
- [ ] When `graph_path` points to a non-existent file, error is explicit (not silent fallback to workspace)
- [ ] Cache works correctly: two calls with same `graph_path` reuse the loaded graph; different paths get isolated `Arc<RwLock<Graph>>`
- [ ] Existing behavior preserved: calls without `graph_path` work exactly as before (workspace + `project` modes unchanged)
- [ ] Unit tests: precedence, cache reuse, cache isolation, error on missing file, both `.db` and `.yml` extensions

## Out of Scope

- CLI changes (already has `--graph`)
- Refactoring `resolve_external` path inference (that stays for `project` mode)
- Migrating `.gid-v03-context/graph.db` content (separate question — see end of conversation today)
- Auto-discovering parallel graph DBs in a project (no listing API)

## Notes

- Issue surfaced 2026-04-24 while trying to operate on `.gid-v03-context/graph.db` (engram v03 retrieval working DB)
- Root cause analysis logged in today's daily note + engram memories (memory IDs around 04-25 02:27 / 02:35 / 03:01)
- The conversation that produced this issue also surfaced a separate question about whether `.gid-v03-context/graph.db` should even exist as a separate working DB — that is deferred and not part of this issue
