# ISS-006 Design Review Summary

**Date**: April 6, 2024  
**Skill**: review-design  
**Status**: ✅ Complete

---

## Overview

Completed comprehensive design review for **ISS-006: Incremental Updates for gid extract**. The design addresses a critical performance bottleneck where `gid extract --lsp` requires 8-minute full rebuilds even for single-file changes.

---

## Key Deliverables

### 1. Design Review Document ✅
- **File**: `ISS-006-review-design.md`
- **Size**: 20KB comprehensive review
- **Sections**: 
  - Executive Summary
  - Design Strengths Analysis
  - Critical Issues & Recommendations
  - Implementation Checklist (4 phases)
  - Testing Strategy
  - Performance Benchmarks
  - Risk Assessment
  - Open Questions
  - Reference Implementation

### 2. Design Approval ✅
**Status**: **APPROVED WITH CRITICAL RECOMMENDATIONS**

The overall design is excellent and ready for implementation, with three critical enhancements required:

1. **Schema Versioning** (CRITICAL)
2. **Reverse Dependency Invalidation** (CRITICAL)  
3. **Atomic Cache Writes** (CRITICAL)

---

## Key Findings

### Strengths 💪
- ✅ Well-defined problem with quantified impact (100x speedup)
- ✅ Solid three-way diff algorithm (add/modify/delete)
- ✅ Robust change detection (content hash + mtime)
- ✅ Clear implementation plan with phased approach
- ✅ Comprehensive testing strategy
- ✅ Realistic performance expectations

### Critical Issues Identified 🚨

#### 1. Schema Versioning Missing
**Risk**: Stale cache could cause silent bugs after code changes  
**Solution**: Add `schema_version` field to `CachedCodeGraph`  
```rust
pub struct CachedCodeGraph {
    pub schema_version: u32,  // Bump on breaking changes
    // ... rest of fields
}
```

#### 2. Cross-File Dependency Invalidation
**Risk**: Changing file A doesn't invalidate stale edges from file B → A  
**Solution**: Track reverse dependencies and re-query LSP for affected call sites  
**Example**:
```
file_a.rs: foo() → bar()    (rename bar → baz)
file_b.rs: calls foo()      (unchanged, but edge to bar is stale)
```

#### 3. Non-Atomic Cache Writes
**Risk**: Crash during save could corrupt cache  
**Solution**: Write to `.tmp` file, then atomic rename  

---

## Implementation Roadmap

### Phase 1: Core Functionality (MVP) - 2-3 days
- Define types: `FileMetadata`, `CachedCodeGraph`, `ChangedFiles`
- Implement change detection with xxHash (faster than SHA-256)
- Implement graph merge operations
- Add `--force` flag to CLI
- **✅ Include schema versioning**
- **✅ Include atomic cache writes**
- Write core unit tests

### Phase 2: Correctness & Robustness - 1-2 days
- **✅ Implement reverse dependency invalidation**
- Add cache corruption detection
- Handle LSP mode changes
- Write integration tests (10+ scenarios)
- Add `gid cache` subcommands (clear, info)

### Phase 3: Performance & UX - 1-2 days
- Concurrent LSP query batching with rayon
- Progress bars with indicatif
- Benchmark real-world speedups
- Memory profiling

### Phase 4: Future Enhancements (ISS-007+)
- Parallel tree-sitter parsing
- Cache compression (gzip/zstd)
- `gid extract --warmup` for CI
- SQLite backend for very large projects

**Total Estimated Time**: 4-7 days

---

## Performance Impact

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| 1 file changed | 8 min | 5 sec | **96x** |
| 10 files changed | 8 min | 20 sec | **24x** |
| 100 files changed | 8 min | 2 min | **4x** |
| File deleted | 8 min | 3 sec | **160x** |

---

## Testing Requirements

### Unit Tests (15+ test cases)
- `test_compute_metadata()` - Hash + mtime extraction
- `test_find_changed_files_*()` - Add/modify/delete detection
- `test_remove_deleted_files()` - Graph cleanup
- `test_merge_changes()` - Node/edge replacement
- `test_schema_version_mismatch()` - Cache invalidation
- `test_reverse_dependency_invalidation()` - Cross-file edges

### Integration Tests (10+ scenarios)
- Single file change (full cycle)
- Multiple files changed
- File added with imports
- File deleted with dependents
- Force flag ignores cache
- Corrupt cache fallback
- Branch switch cache miss
- LSP mode change invalidation

---

## Risk Assessment

| Risk | Severity | Mitigation | Status |
|------|----------|------------|--------|
| Schema versioning missing | 🚨 HIGH | Add schema_version field | ✅ Required in Phase 1 |
| Non-atomic cache writes | 🚨 HIGH | Temp file + rename | ✅ Required in Phase 1 |
| Cross-file dependency | ⚠️ MEDIUM | Reverse dependency tracking | ✅ Required in Phase 2 |
| Memory overhead | ⚠️ MEDIUM | Document limits, add profiling | ✅ Addressed in Phase 3 |
| LSP query correctness | ⚠️ MEDIUM | Comprehensive integration tests | ✅ Required in Phase 2 |

---

## Recommended Optimizations

### Immediate (Phase 1)
1. **Use xxHash instead of SHA-256** - 3-5x faster hashing
2. **Atomic cache writes** - Prevent corruption
3. **Schema versioning** - Prevent stale cache bugs

### Short-term (Phase 2-3)
4. **Concurrent LSP queries** - Batch with rayon
5. **Progress bars** - Better UX with indicatif
6. **Reverse dependency invalidation** - Correctness for cross-file edges

### Future (Phase 4+)
7. **Git-based change detection** - Faster than hashing when in git repo
8. **Cache compression** - Reduce 5MB → 1MB with gzip
9. **SQLite backend** - Scale to 100K+ files

---

## Open Questions for Implementation

1. **Should we cache LSP query results separately?**  
   → Defer to ISS-008 (adds complexity)

2. **How to handle language server crashes?**  
   → Log warning, skip LSP for that file, keep tree-sitter edges

3. **Should `--force` clear cache or just ignore it?**  
   → Ignore (fast testing), add `gid cache clear` for deletion

4. **What's the UX for "cache outdated"?**  
   → Log: "Cache outdated (schema v0, current v1). Running full extraction..."

5. **Should we support manual cache invalidation?**  
   → `gid extract --invalidate file_a.rs` - Defer to ISS-009

---

## Documentation Requirements

### User-Facing
- [ ] Update `gid extract --help` text
- [ ] Add "Performance: Incremental Extraction" section to user guide
- [ ] Document `--force` flag behavior
- [ ] Document cache location (`.graph-cache/`)
- [ ] Add FAQ: "How do I clear the cache?"

### Developer
- [ ] Architecture diagram in DESIGN.md (already done ✅)
- [ ] Document `FileMetadata` and `CachedCodeGraph` schemas
- [ ] Add code comments for `merge_changes()` algorithm
- [ ] Document reverse dependency invalidation logic

### Changelog
- [ ] Add ISS-006 entry to CHANGELOG.md

---

## Comparison with Alternatives

### Alternative 1: File-watcher Daemon ❌
**Rejected**: Too complex (daemon management, OS-specific APIs) for current use case

### Alternative 2: SQLite Backend ⏭️
**Deferred**: Good for very large projects (100K+ files), but HashMap approach works for 95% of cases

### Alternative 3: Git-based Change Detection ⚠️
**Consider**: Use `git diff` when in git repo (faster than hashing), fall back to hash-based detection

---

## Implementation Checklist

### Phase 1: Core (MVP) ✅ Ready to Start
- [ ] Define `FileMetadata` struct with path, mtime, hash, size
- [ ] Define `CachedCodeGraph` struct with **schema_version**, graph, metadata, timestamps
- [ ] Define `ChangedFiles` struct with added, modified, deleted vectors
- [ ] Implement `compute_metadata()` with **xxHash** (not SHA-256)
- [ ] Implement `find_changed_files()` three-way diff
- [ ] Implement `remove_deleted_files()` graph cleanup
- [ ] Implement `merge_changes()` graph merge
- [ ] Implement `extract_incremental()` orchestration
- [ ] Add `--force` flag to CLI
- [ ] Use **atomic cache writes** (temp file + rename)
- [ ] Write 15+ unit tests

### Phase 2: Correctness ✅ Ready After Phase 1
- [ ] **Implement reverse dependency invalidation** (CRITICAL)
- [ ] Add cache corruption detection (JSON parse errors)
- [ ] Add LSP mode change detection
- [ ] Write 10+ integration tests
- [ ] Add `gid cache clear` command
- [ ] Add `gid cache info` command

### Phase 3: Performance ✅ Ready After Phase 2
- [ ] Implement concurrent LSP query batching (rayon)
- [ ] Add progress bars (indicatif)
- [ ] Benchmark actual speedups
- [ ] Add `--memory-profile` flag
- [ ] Optimize 100-file change scenarios

---

## Reference Implementation

A complete reference implementation sketch is provided in `ISS-006-review-design.md` Appendix A, including:

- `FileMetadata` struct definition
- `CachedCodeGraph` struct with schema versioning
- `ChangedFiles` struct
- `extract_incremental()` main orchestration method
- `compute_hash()` with xxHash
- `save_cached_graph()` with atomic writes

---

## Final Recommendation

**✅ APPROVED FOR IMPLEMENTATION**

The ISS-006 design is **well-architected** and ready to proceed. The three critical enhancements (schema versioning, reverse dependency invalidation, atomic writes) must be included in Phases 1-2.

**Next Steps**:
1. Review this design document with team
2. Begin Phase 1 implementation (2-3 days)
3. Complete Phases 2-3 for production readiness (2-4 days)
4. Document and benchmark results

**Expected Outcome**: 100x speedup for single-file changes, transforming developer experience with LSP-enabled extraction.

---

## Artifacts Generated

1. **ISS-006-review-design.md** (20KB) - Comprehensive design review
2. **ISS-006-DESIGN-REVIEW-SUMMARY.md** (this file) - Executive summary

---

**Review completed**: April 6, 2024  
**Reviewer**: Claude (AI Design Reviewer)  
**Status**: ✅ Complete and approved
