# ISS-006 Graph Update Summary

**Date**: April 6, 2024  
**Status**: Completed  
**File Updated**: `.gid/graph.yml`

## Overview

Updated the ISS-006 (Incremental Updates for gid extract) task breakdown in the project graph to reflect the detailed design requirements. The update transformed the original high-level tasks into a comprehensive, implementation-ready task breakdown.

## Key Changes

### 1. Main Feature Node (ISS-006)
**Enhanced with specific requirements:**
- Use xxHash64 (not SHA-256) for content hashing - 20x faster
- Track node_ids per file for efficient edge cleanup
- Store metadata in `.gid/extract-meta.json` (separate from graph.yml)
- Implement 5-phase pipeline: scan → remove stale → parse changed → resolve refs → save
- Dangling edge cleanup: remove edges where source OR target no longer exists
- Version field in metadata for schema evolution
- Fallback to full rebuild on version mismatch or corruption
- **Performance targets**: <100ms for no-change, <1s for 1-5 changed files

### 2. Task Breakdown (9 Tasks Total)

#### ISS-006-1: Add ExtractMetadata and FileState structs
**Changed from**: Generic "file metadata storage"  
**Changed to**: Specific struct definitions with:
- ExtractMetadata: version (u32), files (HashMap)
- FileState: content_hash (u64 via xxHash64), mtime (u64), node_ids (Vec<String>), edge_count (usize)
- JSON persistence to `.gid/extract-meta.json`
- Rationale for xxHash64 vs SHA-256
- Version field for schema evolution

#### ISS-006-2: Implement 5-phase incremental pipeline
**Changed from**: Generic "selective re-extraction"  
**Changed to**: Detailed 5-phase algorithm:
1. **Phase 1 (Scan)**: Walk directory, compute xxHash64 + mtime, load previous metadata
2. **Phase 2 (Remove stale)**: Detect deleted/modified files, remove nodes, collect node_ids
3. **Phase 3 (Edge cleanup)**: Scan ALL edges, remove where source OR target missing
4. **Phase 4 (Parse)**: Parse only changed files with tree-sitter
5. **Phase 5 (Resolve & save)**: LSP queries for new edges only, save metadata

**Critical detail**: Edge cleanup uses node existence check, NOT edge_count.

#### ISS-006-3: Implement change detection and xxHash64 computation
**Changed from**: Generic "change detection logic"  
**Changed to**: Specific functions:
- `compute_file_hash(path) -> Result<u64>` using xxHash64
- `compute_current_state(dir) -> HashMap<String, FileState>`
- `detect_changes(current, previous) -> ChangeSet`
- Dependency: xxhash-rust = "0.8"

#### ISS-006-4: Implement graph merging with node tracking
**Changed from**: Generic "graph merging"  
**Changed to**: Specific merge operations:
- `remove_file_nodes(graph, file_path) -> Vec<String>` - returns removed node IDs
- `cleanup_dangling_edges(graph, removed_node_ids)` - scans ALL edges
- `merge_extracted_nodes(graph, new_nodes, new_edges)` - updates node_ids

**Critical detail**: `cleanup_dangling_edges` builds valid node set and checks ALL edges.

#### ISS-006-5: Add extract-meta.json persistence
**Changed from**: Part of "update metadata after extraction"  
**Changed to**: Dedicated metadata file operations:
- `load_metadata(repo_dir) -> Result<Option<ExtractMetadata>>`
- `save_metadata(repo_dir, metadata) -> Result<()>`
- Version checking with fallback on mismatch
- Atomic write (temp + rename) for crash safety
- Current version: 1

#### ISS-006-6: Add --full flag and fallback logic
**Changed from**: Generic "CLI flag for incremental mode"  
**Changed to**: Comprehensive fallback behavior:
- `--full` flag to force full rebuild
- Auto-fallback triggers: missing metadata, corrupt JSON, version mismatch, errors
- User-friendly messages for each fallback reason
- Show timing stats

#### ISS-006-7: Add performance monitoring and stats
**NEW TASK** - not in original breakdown  
**Added**: Comprehensive instrumentation:
- IncrementalStats struct with file/node/edge counts
- Timing checkpoints for each phase
- Performance targets: <100ms no-change, <1s small change
- Optional detailed timing export to `.gid/extract-perf.json`

#### ISS-006-8: Add comprehensive tests
**Changed from**: Generic "add tests"  
**Changed to**: Specific test coverage:
- Unit tests: hash computation, change detection, node removal, edge cleanup, version handling
- Integration tests: no-change, modify, delete, add, edge cleanup, --full flag
- Performance benchmarks for targets
- Test fixture: small repo with cross-file references

#### ISS-006-9: Update documentation
**Changed from**: ISS-006-8 (renumbered)  
**Changed to**: Detailed documentation plan:
- README section on incremental extraction
- Detailed design doc
- Performance comparison table
- Code comments on critical sections (edge cleanup, pipeline phases)
- Usage examples with --full flag

### 3. Edge Relationships

**Updated dependency graph:**
```
ISS-006 (root feature)
  ├─ ISS-006-1 (metadata structs)
  │    ├─ ISS-006-2 (pipeline) → depends on
  │    ├─ ISS-006-3 (change detection) → depends on
  │    └─ ISS-006-4 (graph merging) → depends on
  ├─ ISS-006-2 (pipeline)
  │    ├─ ISS-006-6 (CLI flag) → depends on
  │    ├─ ISS-006-7 (perf monitoring) → depends on
  │    └─ ISS-006-8 (tests) → depends on
  ├─ ISS-006-3 (change detection)
  │    ├─ ISS-006-2 (pipeline) → depends on
  │    └─ ISS-006-8 (tests) → depends on
  ├─ ISS-006-4 (graph merging)
  │    ├─ ISS-006-2 (pipeline) → depends on
  │    └─ ISS-006-8 (tests) → depends on
  ├─ ISS-006-5 (metadata persistence)
  │    └─ ISS-006-2 (pipeline) → depends on
  ├─ ISS-006-6 (CLI flag)
  ├─ ISS-006-7 (perf monitoring)
  ├─ ISS-006-8 (tests)
  └─ ISS-006-9 (docs)
       └─ ISS-006-2 (pipeline) → depends on
```

## Critical Implementation Details Captured

### 1. Edge Cleanup Strategy
**Problem**: Original design didn't specify how to handle dangling edges.  
**Solution**: Phase 3 explicitly scans ALL edges and removes those where source OR target node no longer exists. Edge_count is metadata only, not used for cleanup.

### 2. Hash Algorithm Choice
**Problem**: SHA-256 mentioned in original, but slow for incremental case.  
**Solution**: xxHash64 chosen - 20x faster, still good collision resistance for file content.

### 3. Metadata Storage Location
**Problem**: Original design embedded in graph.yml.  
**Solution**: Separate `.gid/extract-meta.json` file allows independent evolution of graph format.

### 4. Fallback Safety
**Problem**: What happens when metadata is corrupt or version mismatches?  
**Solution**: Multiple fallback triggers all lead to safe full rebuild with user notification.

### 5. Performance Targets
**Problem**: No quantitative performance goals.  
**Solution**: Specific targets: <100ms no-change, <1s small change (1-5 files).

## Files Modified

- `.gid/graph.yml` - Updated ISS-006 section (lines 79350-79555)

## Validation

✅ ISS-006 section found  
✅ xxHash64 mentioned  
✅ extract-meta.json mentioned  
✅ 5-phase pipeline mentioned  
✅ Dangling edge cleanup mentioned  
✅ Version field mentioned  
✅ Performance targets mentioned  
✅ node_ids tracking mentioned  
✅ Found 9 ISS-006 subtasks  
✅ All part_of edges present  
✅ All depends_on edges present  

## Next Steps

The graph is now ready for implementation. Developers can:
1. Start with ISS-006-1 (metadata structs) - no dependencies
2. Proceed to ISS-006-3, ISS-006-4, ISS-006-5 in parallel after ISS-006-1
3. Implement ISS-006-2 (pipeline) after dependencies are complete
4. Add ISS-006-6 (CLI), ISS-006-7 (monitoring), ISS-006-8 (tests) in parallel
5. Complete with ISS-006-9 (documentation)

## References

- Original design: `.gid/features/incremental-extract/DESIGN.md`
- Implementation plan: `ISS-006-implementation-plan.md`
- Requirements doc: `ISS-006-incremental-updates.md`
