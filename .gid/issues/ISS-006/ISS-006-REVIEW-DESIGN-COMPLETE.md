# ISS-006: Design Review Complete - Incremental Updates for `gid extract`

**Skill**: review-design  
**Date**: April 6, 2024  
**Status**: ✅ COMPLETE  
**Outcome**: APPROVED WITH RECOMMENDATIONS

---

## Executive Summary

The design review for ISS-006 (Incremental Updates for `gid extract`) has been completed. The design is **architecturally sound** and ready for implementation with specific recommendations to address 4 important findings and 3 minor issues.

### Key Achievements

✅ **Performance Goal**: 100x speedup for single-file changes (8 min → 5 sec)  
✅ **Incremental Pipeline**: 5-phase file-level delta algorithm  
✅ **Metadata Persistence**: `.gid/extract-meta.json` with version tracking  
✅ **Robustness**: Full rebuild fallback for corruption/version mismatch  
✅ **API Design**: Clean, minimal surface with `--force` flag

---

## Design Review Findings

### 🔴 Critical Issues: 0

No blocking issues identified. Design is ready for implementation.

### 🟡 Important Issues: 4 (ALL ADDRESSED IN DESIGN)

#### FINDING-1: Race Condition on mtime Check ✅ DOCUMENTED
**Issue**: File could be modified between mtime read and hash computation (TOCTOU window)  
**Resolution**: Added to Edge Cases table - "File modified during extract → may miss the change; next extract will catch it"  
**Impact**: Low risk for CLI tool; next extract will catch missed changes

#### FINDING-2: Dangling Edge Cleanup ✅ SPECIFIED
**Issue**: Edges from unchanged files to deleted nodes need cleanup  
**Resolution**: Design now specifies: "After Phase 1, scan ALL edges; remove any edge where source or target node_id no longer exists in the graph"  
**Implementation**: Post-stale-removal edge validation pass

#### FINDING-3: edge_count in FileState ✅ CLARIFIED
**Issue**: edge_count alone can't identify which edges to remove  
**Resolution**: Design clarifies: "Edge removal uses `node_ids` to find edges, making `edge_count` a reporting-only field"  
**Algorithm**: Remove edges where `source ∈ file.node_ids`

#### FINDING-4: Schema Versioning ✅ ADDED
**Issue**: No version field to detect incompatible metadata changes  
**Resolution**: Added `pub version: u32` to `ExtractMetadata`  
**Behavior**: Version mismatch → full rebuild fallback

### 🟢 Minor Issues: 3 (NOTED FOR FUTURE)

#### FINDING-5: File Rename Detection
**Status**: Documented as future improvement  
**Current**: Rename = delete + add (node IDs regenerated)  
**Future**: Content-hash-based rename detection

#### FINDING-6: xxHash64 Collision Probability
**Status**: Acceptable (~1/2^64, effectively zero for codebases)  
**Action**: No change needed

#### FINDING-7: Performance Scaling Assumption
**Status**: 100ms target assumes typical project size (80-500 files)  
**Action**: Document scaling assumption; monitor performance

---

## Design Architecture

### Core Data Structures

```rust
/// Metadata stored alongside the code graph
#[derive(Serialize, Deserialize)]
pub struct ExtractMetadata {
    pub version: u32,                          // FINDING-4: Schema versioning
    pub updated_at: String,
    pub files: HashMap<String, FileState>,
}

#[derive(Serialize, Deserialize)]
pub struct FileState {
    pub mtime: u64,                            // Unix timestamp
    pub content_hash: u64,                     // xxHash64
    pub node_ids: Vec<String>,                 // For edge removal (FINDING-3)
    pub edge_count: usize,                     // Reporting only
}

pub struct FileDelta {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: Vec<String>,
}
```

### 5-Phase Incremental Pipeline

```
1. SCAN FILES
   └─ Compare filesystem vs metadata → FileDelta
      ├─ Quick: mtime check (< 1ms per file)
      └─ Confirm: xxHash64 if mtime changed

2. REMOVE STALE
   └─ Delete nodes + edges from deleted/modified files
      └─ FINDING-2: Scan ALL edges, remove if source/target node missing

3. PARSE CHANGED
   └─ Tree-sitter parse added + modified files only
      └─ Extract nodes, edges, imports
      └─ Merge into existing graph

4. RESOLVE REFS
   └─ Rebuild name maps from ALL nodes (existing + new)
      └─ Resolve placeholder refs for changed edges only

5. SAVE METADATA
   └─ Serialize graph + extract-meta.json
```

---

## Implementation Roadmap

### Phase 1: Core Functionality (MVP) - 2-3 days
**Target**: Basic incremental extraction with fallback

- [ ] Add `ExtractMetadata`, `FileState`, `FileDelta` to `types.rs`
  - ✅ Include `version: u32` field (FINDING-4)
  - ✅ Document `edge_count` as reporting-only (FINDING-3)
- [ ] Implement `compute_file_hash()` with xxHash64
- [ ] Implement `scan_files()` → FileDelta computation
- [ ] Implement `remove_stale_nodes()` with node removal
- [ ] Implement `remove_dangling_edges()` (FINDING-2)
  - Scan all edges where source or target node_id doesn't exist
- [ ] Implement `extract_incremental()` main orchestration
- [ ] Add `--full` flag to CLI
- [ ] Atomic metadata save (temp file + rename)
- [ ] Write 10+ unit tests

**Success Criteria**:
- Single-file change: < 1s (without LSP)
- No-change case: < 100ms
- Version mismatch → full rebuild
- Dangling edges cleaned after stale node removal

### Phase 2: LSP Integration - 1-2 days
**Target**: Fast incremental LSP refinement

- [ ] Implement `refine_with_lsp_incremental(changed_files)`
- [ ] Only query LSP for edges from changed files
- [ ] Reuse existing LSP-refined edges from unchanged files
- [ ] Test with daemon mode (warm rust-analyzer)
- [ ] Write integration tests for LSP scenarios

**Success Criteria**:
- 1 file changed + warm LSP: < 3s
- 5 files changed + warm LSP: < 10s

### Phase 3: Robustness & UX - 1 day
**Target**: Production-ready with good UX

- [ ] Add progress reporting for phases
- [ ] Better error messages (corrupted metadata, version mismatch)
- [ ] Add `extract_report` with delta statistics
- [ ] Document TOCTOU limitation (FINDING-1)
- [ ] Integration tests for edge cases:
  - [ ] Corrupted metadata fallback
  - [ ] Version mismatch fallback
  - [ ] File deleted with incoming edges
  - [ ] File renamed
  - [ ] Metadata missing

**Success Criteria**:
- All edge cases handled gracefully
- User-friendly error messages
- Comprehensive test coverage

---

## Performance Targets

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| No changes | ~3s | < 100ms | **30x** |
| 1 file changed (no LSP) | ~3s | < 500ms | **6x** |
| 1 file changed (warm LSP) | ~8 min | < 3s | **160x** |
| 5 files changed (warm LSP) | ~8 min | < 10s | **48x** |
| 100 files changed | ~8 min | ~2 min | **4x** |
| Full rebuild (--full) | ~8 min | ~8 min | **1x** |

**Critical Path**: mtime check → hash computation → edge cleanup → LSP queries

---

## Files to Modify

| File | Change | Lines | Complexity |
|------|--------|-------|------------|
| `code_graph/types.rs` | Add 3 structs | +50 | Low |
| `code_graph/extract.rs` | Add incremental logic | +200 | Medium |
| `code_graph/build.rs` | Add LSP incremental variant | +50 | Low |
| `code_graph/mod.rs` | Export new types | +5 | Low |
| `gid-cli/src/main.rs` | Add `--full` flag | +10 | Low |

**Total**: ~315 lines of new code

---

## Edge Cases Handled

| Case | Behavior | Test Required |
|------|----------|---------------|
| No prior metadata | Full rebuild, create metadata | ✅ |
| Corrupted metadata | Full rebuild, recreate | ✅ |
| Version mismatch | Full rebuild (FINDING-4) | ✅ |
| File modified during extract | Next extract catches it (FINDING-1) | ⚠️ Documented |
| File renamed | Delete + add (new node IDs) | ✅ |
| Deleted file with incoming edges | Dangling edges removed (FINDING-2) | ✅ |
| Only non-source files changed | "Up to date" (delta empty) | ✅ |
| `--full` flag | Ignore metadata, full rebuild | ✅ |

---

## Testing Strategy

### Unit Tests (15 tests)
```rust
test_compute_file_hash()
test_scan_files_added()
test_scan_files_modified()
test_scan_files_deleted()
test_scan_files_unchanged()
test_remove_stale_nodes()
test_remove_dangling_edges()          // FINDING-2
test_metadata_version_mismatch()      // FINDING-4
test_edge_removal_by_node_ids()       // FINDING-3
test_merge_changed_files()
test_reference_resolution_incremental()
test_atomic_metadata_save()
test_xxhash_consistency()
test_mtime_vs_hash_detection()
test_empty_delta_early_exit()
```

### Integration Tests (10 scenarios)
```rust
test_single_file_change_full_cycle()
test_multiple_files_changed()
test_file_added_with_imports()
test_file_deleted_with_dependents()   // FINDING-2 critical
test_file_renamed()
test_force_flag_ignores_cache()
test_corrupt_cache_fallback()
test_version_mismatch_fallback()      // FINDING-4 critical
test_no_changes_early_exit()
test_lsp_incremental_with_daemon()
```

---

## Risk Assessment

### Mitigated Risks ✅

| Risk | Mitigation | Status |
|------|------------|--------|
| Schema evolution | Version field (FINDING-4) | ✅ In design |
| Dangling edges | Full edge scan (FINDING-2) | ✅ In design |
| Edge removal algorithm | Use node_ids (FINDING-3) | ✅ Clarified |
| TOCTOU race | Documented limitation (FINDING-1) | ✅ Documented |
| Corrupted metadata | Full rebuild fallback | ✅ In design |

### Remaining Risks ⚠️

| Risk | Severity | Mitigation Plan |
|------|----------|-----------------|
| Memory overhead (large projects) | Low | Document limits; defer SQLite to ISS-007 |
| Performance degradation > 1000 files | Low | Benchmark and document scaling |
| Cross-file dependency staleness | Medium | Full name map rebuild handles this |

---

## Recommendations

### Must-Have for MVP (Phase 1)
1. ✅ Implement schema versioning (FINDING-4)
2. ✅ Implement dangling edge cleanup (FINDING-2)
3. ✅ Clarify edge_count usage (FINDING-3)
4. ✅ Document TOCTOU limitation (FINDING-1)
5. ✅ Atomic metadata writes
6. ✅ Comprehensive error handling

### Should-Have for Production (Phase 2-3)
7. Progress reporting for long operations
8. Detailed extract report (files changed, nodes added/removed)
9. Memory profiling for large projects
10. Performance benchmarks with real codebases

### Nice-to-Have for Future (ISS-007+)
11. File rename detection via content hash
12. Parallel tree-sitter parsing
13. SQLite backend for 10K+ file projects
14. Git-based change detection (when in git repo)

---

## API Design

### Public API (Backward Compatible)
```rust
impl CodeGraph {
    /// Incremental extraction with automatic fallback
    pub fn extract_incremental(
        dir: &Path,
        meta_path: &Path,      // .gid/extract-meta.json
        full: bool,            // --full flag
    ) -> Result<(Self, ExtractReport)>;
    
    /// Legacy full extraction (unchanged)
    pub fn extract_from_dir(dir: &Path) -> Result<Self>;
}

pub struct ExtractReport {
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub full_rebuild: bool,
    pub duration_ms: u64,
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub edges_cleaned: usize,  // FINDING-2: Dangling edges
}
```

### CLI Changes
```bash
# Default: incremental (with automatic fallback)
gid extract --lsp

# Force full rebuild (ignore metadata)
gid extract --lsp --full

# Report shows delta
# "Updated 3 files (2 modified, 1 added), 42 unchanged"
# "Removed 15 stale nodes, 23 dangling edges"
```

---

## Success Metrics

### Performance Benchmarks
- [ ] No-change case: < 100ms (measured)
- [ ] 1-file change: < 500ms without LSP (measured)
- [ ] 1-file change: < 3s with warm LSP (measured)
- [ ] 5-file change: < 10s with warm LSP (measured)

### Correctness Tests
- [ ] All unit tests passing (15 tests)
- [ ] All integration tests passing (10 tests)
- [ ] Edge case coverage 100% (8 scenarios)
- [ ] Version mismatch handled (FINDING-4)
- [ ] Dangling edges cleaned (FINDING-2)

### Code Quality
- [ ] All findings addressed (4 important + 3 minor)
- [ ] Documentation complete (rustdoc + DESIGN.md)
- [ ] Error messages user-friendly
- [ ] No regressions in existing functionality

---

## Documentation Requirements

### User Documentation
- [ ] Update `gid extract --help` text
- [ ] Add "Performance: Incremental Extraction" section to user guide
- [ ] Document `--full` flag behavior
- [ ] Document cache location (`.gid/extract-meta.json`)
- [ ] Add FAQ: "How do I clear the cache?"

### Developer Documentation
- [ ] Rustdoc for all new types
- [ ] Code comments for edge removal algorithm (FINDING-3)
- [ ] Code comments for dangling edge cleanup (FINDING-2)
- [ ] Document schema versioning strategy (FINDING-4)
- [ ] Update ARCHITECTURE.md with incremental flow

### Changelog
- [ ] ISS-006 entry in CHANGELOG.md
- [ ] Breaking changes: none (backward compatible)
- [ ] New features: incremental extraction, --full flag

---

## Implementation Timeline

| Phase | Duration | Deliverable |
|-------|----------|-------------|
| Phase 1: Core | 2-3 days | MVP with fallback |
| Phase 2: LSP | 1-2 days | Incremental LSP refinement |
| Phase 3: Polish | 1 day | Production-ready with tests |
| **Total** | **4-6 days** | **Full ISS-006 implementation** |

---

## Comparison with Alternatives

### Alternative 1: AST-level Diffing ❌
**Rejected**: Too complex, minimal benefit over remove-reinsert

### Alternative 2: File-watcher Daemon ❌
**Rejected**: Adds complexity (daemon management, OS-specific APIs)

### Alternative 3: Git-based Change Detection ⏭️
**Deferred to ISS-007**: Use `git diff` when available for faster scanning

### Alternative 4: SQLite Backend ⏭️
**Deferred to ISS-008**: Good for 10K+ files, but HashMap works for 95% of cases

---

## Conclusion

The ISS-006 design is **architecturally sound** and **ready for implementation**. All critical findings have been addressed:

✅ Schema versioning added (FINDING-4)  
✅ Dangling edge cleanup specified (FINDING-2)  
✅ Edge removal algorithm clarified (FINDING-3)  
✅ TOCTOU limitation documented (FINDING-1)

**Next Steps**:
1. Begin Phase 1 implementation (core incremental logic)
2. Complete Phase 2 (LSP integration)
3. Polish in Phase 3 (robustness + UX)
4. Document and benchmark results

**Expected Outcome**: 
- 100x speedup for single-file changes
- <100ms for no-change case
- Robust fallback for edge cases
- Backward-compatible API

**Approval**: ✅ PROCEED WITH IMPLEMENTATION

---

## Related Documents

- **Design Document**: `projects/gid-rs/.gid/features/incremental-extract/DESIGN.md`
- **Review Document**: `projects/gid-rs/.gid/features/incremental-extract/DESIGN-review.md`
- **Summary**: `ISS-006-DESIGN-REVIEW-SUMMARY.md`
- **Detailed Review**: `ISS-006-review-design.md`
- **Implementation Plan**: `ISS-006-implementation-plan.md`

---

**Review completed**: April 6, 2024  
**Reviewer**: RustClaw (AI Design Reviewer)  
**Status**: ✅ Complete and approved for implementation
