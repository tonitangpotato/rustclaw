## Incremental Updates for gid extract (ISS-006)

**Problem**: Currently `gid extract --lsp` rebuilds the entire code graph every time. Even changing one file triggers full re-parsing of all files + all LSP queries. This is especially painful with the LSP daemon where initial analysis takes ~8 minutes for large projects.

**Solution**: Detect file changes (mtime/content hash) and only re-extract + re-query LSP for modified files, merging results into the existing graph.

### Architecture

```
┌──────────────────────────────────────────────────────┐
│         Incremental Extraction Flow                  │
│                                                      │
│  1. Load existing graph + metadata from cache       │
│     ↓                                                │
│  2. Scan filesystem → compute file hashes           │
│     ↓                                                │
│  3. Compare current vs cached metadata              │
│     ↓                                                │
│  4. Identify: added / modified / deleted files      │
│     ↓                                                │
│  5. Extract only changed files (tree-sitter + LSP)  │
│     ↓                                                │
│  6. Merge new nodes/edges into existing graph       │
│     ↓                                                │
│  7. Remove nodes/edges from deleted files           │
│     ↓                                                │
│  8. Update metadata cache + save graph              │
└──────────────────────────────────────────────────────┘
```

### Implementation Details

**New Types** in `gid-core/src/code_graph.rs`:

```rust
/// File metadata for incremental extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub mtime: u64,           // Unix timestamp
    pub content_hash: String, // SHA-256 hex digest
    pub size: u64,
}

/// Extended graph with metadata for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCodeGraph {
    pub graph: CodeGraph,
    pub metadata: HashMap<String, FileMetadata>,
    pub extracted_at: u64,    // Unix timestamp
    pub lsp_enabled: bool,
}
```

**New Methods**:

```rust
impl CodeGraph {
    /// Incremental extraction with change detection
    pub fn extract_incremental(
        repo_dir: &Path,
        cache_dir: &Path,
        force: bool,
    ) -> Result<(Self, IncrementalStats)>;

    /// Compute file metadata for change detection
    fn compute_metadata(path: &Path) -> Result<FileMetadata>;

    /// Find changed files by comparing current vs cached metadata
    fn find_changed_files(
        current: &HashMap<String, FileMetadata>,
        cached: &HashMap<String, FileMetadata>,
    ) -> ChangedFiles;

    /// Extract only changed files and merge into existing graph
    fn merge_changes(
        &mut self,
        changed: &ChangedFiles,
        repo_dir: &Path,
        lsp_enabled: bool,
    ) -> Result<()>;

    /// Remove nodes and edges associated with deleted files
    fn remove_deleted_files(&mut self, deleted: &[String]);
}

#[derive(Debug)]
pub struct ChangedFiles {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

#[derive(Debug, Default)]
pub struct IncrementalStats {
    pub total_files: usize,
    pub unchanged_files: usize,
    pub added_files: usize,
    pub modified_files: usize,
    pub deleted_files: usize,
    pub nodes_added: usize,
    pub nodes_updated: usize,
    pub nodes_removed: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    pub extraction_time_ms: u64,
}
```

**Cache Storage**:
- Location: `{repo_dir}/.graph-cache/{repo_name}__{commit}.json`
- Format: JSON serialization of `CachedCodeGraph`
- Keyed by repo name + git commit hash (or timestamp if not git repo)
- Cache invalidation: automatic on commit change, manual via `--force` flag

**Change Detection Algorithm**:

1. **Load cached graph** from `.graph-cache/` (if exists)
2. **Scan filesystem** to build current file list + compute hashes
3. **Three-way diff**:
   - Added: in current, not in cached
   - Modified: in both, but hash differs
   - Deleted: in cached, not in current
4. **Partial extraction**:
   - Parse only added/modified files with tree-sitter
   - Query LSP only for changed call sites
   - Keep existing nodes/edges for unchanged files
5. **Graph merge**:
   - Remove old nodes/edges from modified files
   - Insert new nodes/edges from extraction
   - Remove nodes/edges from deleted files
   - Rebuild adjacency indexes
6. **Save updated graph** + metadata to cache

**Content Hashing**:
- Algorithm: SHA-256 (fast enough for incremental use)
- Fallback: mtime comparison if hashing fails
- Skip: large binary files (>10MB) use mtime only

**CLI Changes**:

```bash
# Default: incremental extraction (if cache exists)
gid extract --lsp

# Force full rebuild (ignore cache)
gid extract --lsp --force

# Clear cache directory
gid cache clear

# Show cache stats
gid cache info
```

### Benefits

- **Performance**: 10-100x faster for single-file changes (8 minutes → 5 seconds)
- **LSP efficiency**: Only query changed call sites, reuse previous results
- **Disk I/O**: Skip reading/parsing unchanged files
- **Developer ergonomics**: Near-instant updates during development

### Tradeoffs

- **Cache storage**: ~2-5MB per cached graph (acceptable)
- **Complexity**: Change detection + merge logic (~300 LOC)
- **Cache invalidation**: Must detect when cache is stale (commit change, config change)
- **Memory**: Must load existing graph into memory (typical: 5-20MB for large projects)

### Edge Cases

1. **File renamed**: Detected as delete + add → nodes re-created (acceptable, rare)
2. **Cross-file refactor**: Changing multiple files works correctly (all updated)
3. **LSP mode change**: Cache stores `lsp_enabled` flag, invalidates if changed
4. **Partial extraction failure**: Rollback to cached graph, log warning
5. **Corrupt cache**: Detect via JSON parse error → full rebuild
6. **Git branch switch**: Commit hash change → cache miss → full rebuild

### Performance Expectations

**Initial extraction** (no cache):
- Large TypeScript project (~1000 files): ~8 minutes with LSP
- Same project without LSP: ~30 seconds

**Incremental update** (1 file changed):
- With cache: ~5 seconds (parse 1 file + ~10 LSP queries + merge)
- Speedup: ~100x

**Incremental update** (10 files changed):
- With cache: ~20 seconds
- Speedup: ~24x

### Testing Strategy

**Unit tests**:
- `test_compute_metadata()`: File hash + mtime computation
- `test_find_changed_files()`: Three-way diff logic
- `test_remove_deleted_files()`: Node/edge removal
- `test_merge_changes()`: Graph merge correctness

**Integration tests**:
- Create fixture repo, extract, modify file, extract again → verify incremental
- Test all change types: add, modify, delete, rename
- Verify cache invalidation on commit change
- Test `--force` flag bypasses cache

**Performance benchmarks**:
- Measure extraction time: full vs incremental (1 file, 10 files, 100 files)
- Compare LSP query count: full vs incremental
- Memory usage: cached graph loading

### Integration with LSP Daemon

**Synergy**: Incremental extraction + LSP daemon = optimal developer experience
- Daemon keeps LSP server alive across extractions
- Incremental mode only queries changed files
- Result: ~5 second graph updates instead of 8 minutes

**Daemon awareness**: Daemon tracks file watchers, can trigger incremental extraction on file change events (future enhancement).
