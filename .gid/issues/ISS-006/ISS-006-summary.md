# ISS-006: Incremental Updates Implementation Plan

## Overview
Successfully added ISS-006 and 8 sub-tasks to .gid/graph.yml for implementing incremental updates in gid extract.

## Feature Structure

### Main Feature Node: ISS-006
- **Title**: Incremental Updates for gid extract
- **Status**: todo
- **Type**: feature
- **Priority**: high
- **Estimated Effort**: large

### Sub-tasks (8 total):

1. **ISS-006-1**: Add file metadata storage to code graph
   - Extend CodeGraph to track file path, mtime, and content hash
   - Location: `crates/gid-core/src/code_graph/types.rs`

2. **ISS-006-2**: Implement change detection logic
   - Create functions to detect added/modified/deleted files
   - Location: `crates/gid-core/src/code_graph/extract.rs`

3. **ISS-006-3**: Implement selective re-extraction
   - Modify extraction pipeline to only process changed files
   - Location: `crates/gid-core/src/code_graph/build.rs`

4. **ISS-006-4**: Implement graph merging logic
   - Merge new nodes with existing graph, handle deletions
   - Location: `crates/gid-core/src/code_graph/build.rs`

5. **ISS-006-5**: Update metadata after extraction
   - Persist updated file metadata after successful extraction
   - Location: `crates/gid-core/src/code_graph/build.rs`

6. **ISS-006-6**: Add CLI flag for incremental mode
   - Add --force/--full flags, make incremental default
   - Location: Main CLI crate

7. **ISS-006-7**: Add tests for incremental extraction
   - Comprehensive unit and integration tests
   - Location: `tests/` directory and `code_graph/tests.rs`

8. **ISS-006-8**: Update documentation
   - Document incremental mode, flags, and performance

## Dependency Graph

```
ISS-006 (main feature)
├── ISS-006-1 (file metadata storage)
│   ├── ISS-006-2 (change detection) ← depends on
│   └── ISS-006-3 (selective extraction) ← depends on
├── ISS-006-3 (selective extraction)
│   ├── ISS-006-4 (graph merging) ← depends on
│   └── ISS-006-6 (CLI flags) ← depends on
├── ISS-006-4 (graph merging)
│   └── ISS-006-5 (metadata updates) ← depends on
└── ISS-006-5 (metadata updates)
    └── ISS-006-7 (tests) ← depends on
        └── ISS-006-8 (documentation) ← depends on
```

## Implementation Order (based on dependencies)

1. **ISS-006-1** - File metadata storage (foundational)
2. **ISS-006-2** - Change detection (depends on 1)
3. **ISS-006-3** - Selective re-extraction (depends on 1, 2)
4. **ISS-006-4** - Graph merging (depends on 3)
5. **ISS-006-5** - Metadata updates (depends on 4)
6. **ISS-006-6** - CLI flags (depends on 3, can be done in parallel with 4-5)
7. **ISS-006-7** - Tests (depends on 5)
8. **ISS-006-8** - Documentation (depends on 7)

## Key Technical Decisions

1. **File Change Detection**: Use both mtime and content hash (SHA-256) for robust change detection
2. **Metadata Storage**: Persist metadata alongside the graph in the same file
3. **Default Behavior**: Make incremental the default when a graph exists
4. **Fallback**: Always provide --force flag for full rebuild
5. **Graph Merging**: Handle node removal, replacement, and edge cleanup carefully

## Expected Performance Impact

- **Initial extraction**: Same as current (8 minutes with LSP daemon)
- **Incremental extraction**: Expected ~10-100x faster for small changes
  - Single file change: seconds instead of minutes
  - No LSP re-query for unchanged files
  - No tree-sitter re-parsing for unchanged files

## Files Modified (anticipated)

- `crates/gid-core/src/code_graph/types.rs` - Add FileMetadata struct
- `crates/gid-core/src/code_graph/extract.rs` - Add change detection
- `crates/gid-core/src/code_graph/build.rs` - Add incremental extraction and merging
- `crates/gid-core/src/code_graph/tests.rs` - Add tests
- CLI argument parsing - Add flags
- README and docs - Update documentation

## Next Steps

1. Start with ISS-006-1 to add the foundational file metadata storage
2. Implement in order following the dependency chain
3. Test thoroughly at each stage before moving to next task
4. Consider edge cases: concurrent modifications, symlinks, moved files

