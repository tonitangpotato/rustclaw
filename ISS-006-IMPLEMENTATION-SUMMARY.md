# ISS-006 Implementation Summary

## Task Completed
✅ Added comprehensive design documentation for **ISS-006: Incremental Updates for gid extract**

## Changes Made

### 1. Updated DESIGN.md
Added a new major section after the LSP integration documentation:

**Section: "Incremental Updates for gid extract (ISS-006)"**

This section includes:

#### Architecture Overview
- Visual flow diagram showing 8-step incremental extraction process
- Clear illustration of cache loading → change detection → partial extraction → merge

#### Implementation Details

**New Types**:
- `FileMetadata`: Stores path, mtime, content hash, and size for change detection
- `CachedCodeGraph`: Extended graph structure with metadata map, timestamp, and LSP flag
- `ChangedFiles`: Categorizes files as added/modified/deleted
- `IncrementalStats`: Comprehensive statistics for extraction performance tracking

**New Methods**:
- `extract_incremental()`: Main entry point for incremental extraction
- `compute_metadata()`: File hashing and metadata computation
- `find_changed_files()`: Three-way diff between current and cached state
- `merge_changes()`: Merge new/updated nodes into existing graph
- `remove_deleted_files()`: Clean up nodes from deleted files

**Cache Storage**:
- Location: `{repo_dir}/.graph-cache/{repo_name}__{commit}.json`
- Format: JSON serialization
- Keying: Git commit hash (or timestamp for non-git repos)
- Invalidation: Automatic on commit change, manual via `--force`

**Change Detection Algorithm**:
1. Load cached graph from disk
2. Scan filesystem and compute current file hashes
3. Three-way diff (added/modified/deleted)
4. Partial extraction (only changed files)
5. Graph merge with adjacency index rebuild
6. Save updated cache

**Content Hashing**:
- SHA-256 for files <10MB
- Fallback to mtime for large files
- Graceful degradation on hash failures

**CLI Changes**:
```bash
gid extract --lsp           # Incremental (if cache exists)
gid extract --lsp --force   # Force full rebuild
gid cache clear             # Clear cache directory
gid cache info              # Show cache statistics
```

#### Benefits
- **Performance**: 10-100x faster for single-file changes (8 minutes → 5 seconds)
- **LSP efficiency**: Only query changed call sites
- **Developer ergonomics**: Near-instant updates during development

#### Tradeoffs
- Cache storage: ~2-5MB per cached graph
- Complexity: ~300 LOC for change detection + merge
- Cache invalidation logic required
- Must load existing graph into memory

#### Edge Cases Covered
1. File renamed (delete + add)
2. Cross-file refactor (multiple files)
3. LSP mode change (cache invalidation)
4. Partial extraction failure (rollback)
5. Corrupt cache (full rebuild)
6. Git branch switch (commit hash change)

#### Performance Expectations
- Initial extraction: ~8 minutes (1000 files with LSP)
- 1 file changed: ~5 seconds (100x speedup)
- 10 files changed: ~20 seconds (24x speedup)

#### Testing Strategy
- Unit tests for metadata computation, diffing, merging
- Integration tests for all change types
- Performance benchmarks for various file counts
- Cache invalidation tests

#### Integration with LSP Daemon
- Synergy with existing LSP daemon (ISS-003)
- Daemon keeps server alive, incremental mode only queries changes
- Combined result: ~5 second updates vs 8 minutes

## Files Modified

1. **DESIGN.md** - Added ISS-006 section with comprehensive design documentation

## Files Created

1. **ISS-006-incremental-updates.md** - Standalone copy of the design section (for reference)
2. **.verify-iss006.sh** - Verification script to confirm section was added correctly

## Next Steps for Implementation

To actually implement this design, developers should:

1. **Add new types** to `crates/gid-core/src/code_graph.rs`:
   - `FileMetadata`, `CachedCodeGraph`, `ChangedFiles`, `IncrementalStats`

2. **Implement core methods** in `CodeGraph`:
   - File hashing with SHA-256
   - Three-way diff logic
   - Graph merge with node/edge replacement
   - Cache load/save with JSON serialization

3. **Add CLI flags** in the gid binary:
   - `--force` flag to bypass cache
   - `gid cache clear` subcommand
   - `gid cache info` subcommand

4. **Write tests**:
   - Unit tests for each core method
   - Integration tests with fixture repos
   - Performance benchmarks

5. **Document cache format**:
   - JSON schema for `CachedCodeGraph`
   - Cache directory structure
   - Invalidation rules

## Design Rationale

The design focuses on:
- **Minimal changes**: Extends existing `CodeGraph` rather than rewriting
- **Backward compatibility**: Incremental mode is opt-in, full extraction still default
- **Fail-safe**: Corrupt cache or errors trigger full rebuild automatically
- **Performance**: SHA-256 hashing is fast enough for incremental use (<1ms per file)
- **Developer experience**: Cache is transparent, ~100x speedup for typical workflows

## Verification

Run `.verify-iss006.sh` to confirm all sections were added to DESIGN.md.

---
**Implementation Status**: Design complete, ready for development
**Estimated Implementation Effort**: ~3-5 days for core functionality + tests
**Priority**: High (significant developer experience improvement)
