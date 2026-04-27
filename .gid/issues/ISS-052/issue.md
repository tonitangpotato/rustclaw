---
id: "ISS-052"
title: "Artifact↔Graph synchronization layer for gid_* native tools"
status: open
priority: P2
created: 2026-04-26
related: ["gid-rs/ISS-053"]
depends_on: ["gid-rs/ISS-053"]
---
# ISS-052 — Artifact↔Graph synchronization for gid_* native tools

**Status:** open (blocked on gid-rs ISS-053)
**Severity:** medium — chronic drift, not blocking
**Discovered:** 2026-04-26 — while scoping gid-rs ISS-053 (artifact-as-first-class), it became clear that issues/features now live in two places (artifact files + graph nodes) and rustclaw is the surface where users hit the desync.

## Why this is rustclaw scope, not gid-core scope

- **gid-core** owns: ArtifactStore (ISS-053), GraphStore (existing). Both are mechanism layers.
- **rustclaw** owns: the ~30 `gid_*` native tools that users actually call. The decision "when a user calls `gid_update_task`, do we also update the artifact file?" is a **policy** decision about the user-facing tool surface, not about the underlying stores.
- Putting the sync logic in gid-core would force every gid-core consumer (CLI, MCP, rustclaw) to adopt the same policy. Different consumers may want different policies (e.g., CLI in a script wants no auto-sync; rustclaw in interactive agent loop wants it).
- Therefore: gid-core exposes both stores cleanly; rustclaw decides how they relate at the tool layer.

## Problem

Today (post-ISS-028 backfill, post-ISS-053 artifact layer):

- `.gid/issues/ISS-022/issue.md` — frontmatter says `status: in_progress`
- graph node `ISS-022` in `graph.db` — `status: todo`

A user calling `gid_tasks` sees `todo`. A user reading the issue file sees `in_progress`. **Two truths.** Worse: agents (LLMs) see different states depending on which tool they happened to call.

This will compound. Every new artifact kind landing via ISS-053 (postmortems, ADRs, …) potentially gets a graph counterpart and another desync axis.

## Constraints

1. **Artifact files are authoritative for human workflows.** Humans edit them in vim. The graph is a derived index.
2. **Not all graph nodes have artifacts.** Code nodes (from `gid_extract`), inferred component nodes (from `gid_infer`), pure task nodes that never had a markdown file — these stay graph-only. Sync only applies where both exist.
3. **Don't break the 30 existing tools' contracts.** `gid_update_task` should still take the same input, return the same output. The sync is an internal upgrade, not an API change.
4. **Daemon mode and CLI mode have different needs.** Daemon can fs-watch; CLI invocation should be cheap (no background threads).
5. **Sync direction is artifact → graph by default** (artifact is authoritative). Graph → artifact only when explicitly written through a `gid_*` tool that historically writes graph nodes.

## Decisions

### D1 — Sync at tool-call boundary, not in storage layer

Synchronization runs **inside the rustclaw tool dispatch**, before/after gid-core calls. gid-core stays unaware. This keeps the policy in one place and out of CLI/MCP code paths that don't want it.

### D2 — Artifact wins on conflict; graph reconciles to artifact

When tool entry detects desync, **trust the artifact**. Update the graph node to match. Never silently rewrite the artifact based on graph state.

Exception: tools that have always been "graph-write" operations (`gid_add_task`, `gid_update_task`, `gid_complete`) **also write the artifact** if one exists. After write, both are consistent.

### D3 — Lazy detection on tool entry, optional fs-watch in daemon mode

- **CLI / one-shot tool call**: on entry to any `gid_*` tool that touches a node with an artifact counterpart, compare mtimes; if artifact is newer than graph row, reconcile graph from artifact.
- **Daemon mode**: optional fs-watch on `.gid/issues/`, `.gid/features/`, etc.; debounced; pushes artifact changes into graph index without waiting for next tool call. Off by default in v1.

### D4 — Artifact↔graph node mapping is path-driven

A graph node `ISS-022` corresponds to artifact at `.gid/issues/ISS-022/issue.md` via Layout (gid-core). Mapping function lives in gid-core (since both layers know about Layout). rustclaw consumes the mapping.

### D5 — Code/inferred graph nodes are exempt

`gid_extract` produces nodes like `func:src/foo.rs:bar` — no artifact, no sync. Same for `gid_infer` cluster nodes. The sync layer skips any node whose ID does not resolve to an artifact via Layout.

## Tools affected

Categorized:

### A. Need sync (read & write to nodes that have artifacts)
- `gid_tasks` — read; reconcile-on-read for issue/feature/design/review nodes.
- `gid_add_task` — write; if creating an issue/feature node, also create the artifact file.
- `gid_update_task` — write; mirror status/title/priority changes to artifact frontmatter.
- `gid_complete` — write; update both.
- `gid_read` — read; reconcile-on-read.
- `gid_query_impact` / `gid_query_deps` — read; reconcile-on-read for nodes in result.
- `gid_advise` / `gid_plan` — read; reconcile-on-read.
- `gid_visual` — read; reconcile-on-read.
- `gid_validate` — read; **also flag desync as a validation finding**.

### B. No sync needed (operate on code/inferred nodes only)
- `gid_extract` — code nodes only.
- `gid_schema` — code nodes only.
- `gid_semantify` — layer tags, no artifact.
- `gid_complexity` — code nodes only.
- `gid_infer` — cluster nodes, no artifact.
- `gid_working_memory` — file paths, no artifact.
- `gid_ignore` — gitignore-like, no nodes.

### C. Mixed (decide per-call)
- `gid_design` — DESIGN.md → graph; if a feature/design artifact exists at the parsed location, update its frontmatter from parse.
- `gid_context` / `gid_task_context` — reads from graph, but task targets often have artifacts; reconcile-on-read.
- `gid_history` — graph snapshots; out of scope (snapshots are immutable).
- `gid_refactor` — rename/merge; **must rename artifact file too** when the renamed node has one.
- `gid_stats` — last harness run; no sync.

## Implementation outline

### New module: `src/gid_storage/sync.rs`

```rust
pub struct ArtifactGraphSync<'a> {
    project: &'a str,
    artifact_store: &'a ArtifactStore,
    graph: &'a mut Graph,
    layout: &'a Layout,
}

impl<'a> ArtifactGraphSync<'a> {
    /// Called on tool entry for read-style tools.
    /// For each node ID about to be touched, check if it has an artifact and if mtime says drift.
    pub fn reconcile_for_read(&mut self, node_ids: &[&str]) -> Result<ReconcileReport>;

    /// Called inside write-style tools after the graph mutation succeeds.
    /// Mirrors changes into the artifact's frontmatter.
    pub fn mirror_write_to_artifact(&self, node_id: &str, fields: &FieldChanges) -> Result<()>;

    /// Called inside gid_refactor when renaming a node with an artifact.
    pub fn rename_artifact(&self, old_id: &str, new_id: &str) -> Result<()>;
}

pub struct ReconcileReport {
    pub reconciled: Vec<String>,    // node IDs updated from artifact
    pub artifact_only: Vec<String>, // artifact exists but no graph node — surfaced as warning
    pub graph_only: Vec<String>,    // graph node exists but artifact deleted — surfaced as warning
}
```

### Tool dispatch wrappers

Each affected tool (Category A and C) gets a thin wrapper:

```rust
async fn handle_gid_tasks(&self, input: Value) -> Result<Value> {
    let mut sync = self.make_sync()?;
    let candidate_ids = self.peek_candidate_node_ids(&input)?;
    let report = sync.reconcile_for_read(&candidate_ids)?;
    let result = self.handle_gid_tasks_inner(input).await?;
    Ok(merge_warnings(result, report))
}
```

For write tools:

```rust
async fn handle_gid_update_task(&self, input: Value) -> Result<Value> {
    // ... existing graph write ...
    if let Some(field_changes) = extract_field_changes(&input) {
        self.make_sync()?.mirror_write_to_artifact(&node_id, &field_changes)?;
    }
    // ... return ...
}
```

### Daemon-mode fs-watch (optional, v2 of this issue)

`notify`-based watcher on `<project_root>/.gid/{issues,features}/`, debounced (500ms), feeds into the same `reconcile_for_read` codepath. Off by default; enabled via config.

## Acceptance criteria

- [ ] `ArtifactGraphSync` module exists with the API above.
- [ ] Reconcile-on-read works: edit `issues/ISS-022/issue.md` frontmatter `status: blocked` by hand; `gid_tasks` returns `blocked` for ISS-022 (graph reconciled silently before query).
- [ ] Mirror-on-write works: `gid_update_task --status done ISS-022`; the artifact file's frontmatter `status:` is now `done`; `Metadata::set_field` round-trip preserves all other lines byte-exact.
- [ ] `gid_refactor rename ISS-022 → ISS-099` also moves `.gid/issues/ISS-022/` to `.gid/issues/ISS-099/`. (Or refuses if the artifact has internal references that would break.)
- [ ] `gid_validate` reports artifact↔graph desync as a finding (severity: warning).
- [ ] Code nodes (`func:src/foo.rs:bar`, `class:...`) skip sync — verified by unit test.
- [ ] CLI mode adds <5ms per tool call when no desync exists (mtime check is cheap).
- [ ] Daemon-mode fs-watch is gated behind a config flag (off by default in v1 of this issue).

## Out of scope

- Two-way realtime sync (e.g., WebSocket-style notifications back to artifact-aware tools). Lazy reconcile is sufficient.
- GitHub Issues mirror. Hard no.
- Conflict resolution UI. If a real conflict appears (artifact and graph both edited since last sync), the policy is "artifact wins" — surface a warning, not a prompt.
- Sync for skill-/ritual-/runtime-state files. Those are not artifacts in ISS-053 sense.

## Risks

- **Mirror-write can corrupt user-edited frontmatter** if `Metadata::set_field` round-trip has bugs. Mitigation: depend on ISS-053's byte-exact round-trip guarantee (its acceptance criterion). Add rustclaw-side regression tests with real artifact files.
- **mtime-based detection is racy** under filesystem clock skew or rapid edits. Mitigation: also compare a content hash of the metadata block; if mismatch but mtime <1s old, retry once.
- **Refactor rename of artifact** may invalidate cross-project markdown links pointing at the old path. Mitigation: surface a warning listing affected files; refuse to rename if the artifact has incoming refs unless `--force` is set.
- **Daemon fs-watch is off by default** but if turned on, may flood the graph rebuild on bulk operations (e.g., git checkout of a feature branch). Mitigation: debounce + batch reconcile.
- **Adoption order**: this issue cannot start until gid-rs ISS-053 lands. Tracked via `depends_on`.

## Notes

- This issue exists because gid-rs ISS-053 chose to keep artifact and graph layers separate. The alternative (artifact = graph node's physical representation) would have eliminated this issue entirely but required a much larger gid-core refactor and a graph storage migration. The trade-off was made consciously in ISS-053 §10.
- After this issue lands, agents (Claude in rustclaw) can edit issue frontmatter directly via `gid_artifact_update` and trust that subsequent `gid_tasks` calls reflect the change. This closes the "two truths" gap.
- Future thought: when desync warnings become rare in practice, consider promoting reconcile-on-read to a graph-storage-layer concern (push back into gid-core). v1 deliberately keeps it in rustclaw to avoid premature abstraction.
