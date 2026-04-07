# ISS-006 Design Review - Execution Summary

**Skill**: `review-design`  
**Executed**: April 6, 2024  
**Status**: ✅ COMPLETE  
**Result**: APPROVED FOR IMPLEMENTATION

---

## Quick Summary

The design review for **ISS-006: Incremental Updates for `gid extract`** has been completed successfully. The design is architecturally sound with 0 critical issues and all 4 important findings addressed.

### Design Approval Status

| Category | Count | Status |
|----------|-------|--------|
| 🔴 Critical (blocking) | 0 | ✅ None found |
| 🟡 Important (pre-impl) | 4 | ✅ All addressed |
| 🟢 Minor (during-impl) | 3 | ✅ Noted for future |

---

## Key Findings & Resolutions

### FINDING-1: Race Condition on mtime Check ✅
**Issue**: TOCTOU window between mtime read and hash computation  
**Resolution**: Documented in Edge Cases - acceptable for CLI tool  
**Impact**: Next extract catches missed changes

### FINDING-2: Dangling Edge Cleanup ✅
**Issue**: Edges from unchanged files to deleted nodes need cleanup  
**Resolution**: Added to design: "Scan ALL edges after Phase 1; remove if source/target node missing"  
**Implementation**: Post-stale-removal edge validation pass

### FINDING-3: edge_count in FileState ✅
**Issue**: Cannot identify which edges to remove with count alone  
**Resolution**: Clarified: "Edge removal uses node_ids; edge_count is reporting-only"  
**Algorithm**: Remove edges where `source ∈ file.node_ids`

### FINDING-4: Schema Versioning ✅
**Issue**: No version field to detect incompatible metadata changes  
**Resolution**: Added `pub version: u32` to ExtractMetadata  
**Behavior**: Version mismatch → full rebuild fallback

---

## Design Architecture Summary

### Core Structures (in `types.rs`)
```rust
pub struct ExtractMetadata {
    pub version: u32,                          // FINDING-4
    pub updated_at: String,
    pub files: HashMap<String, FileState>,
}

pub struct FileState {
    pub mtime: u64,
    pub content_hash: u64,                     // xxHash64
    pub node_ids: Vec<String>,                 // FINDING-3: For edge removal
    pub edge_count: usize,                     // Reporting only
}
```

### 5-Phase Pipeline
1. **SCAN FILES** → FileDelta (added/modified/deleted/unchanged)
2. **REMOVE STALE** → Delete nodes + edges from changed files + dangling edge cleanup (FINDING-2)
3. **PARSE CHANGED** → Tree-sitter parse only added/modified files
4. **RESOLVE REFS** → Rebuild name maps from ALL nodes, resolve changed edges only
5. **SAVE METADATA** → Serialize graph + extract-meta.json

---

## Performance Targets

| Scenario | Before | After | Speedup |
|----------|--------|-------|---------|
| No changes | ~3s | < 100ms | **30x** |
| 1 file (no LSP) | ~3s | < 500ms | **6x** |
| 1 file (warm LSP) | ~8 min | < 3s | **160x** |
| 5 files (warm LSP) | ~8 min | < 10s | **48x** |

---

## Implementation Checklist

### Must-Implement (MVP)
- [ ] Add ExtractMetadata with `version: u32` (FINDING-4)
- [ ] Add FileState with `node_ids` (FINDING-3)
- [ ] Implement FileDelta computation (mtime → hash)
- [ ] Implement remove_stale_nodes()
- [ ] Implement remove_dangling_edges() - scan ALL edges (FINDING-2)
- [ ] Implement extract_incremental() orchestration
- [ ] Add `--full` flag to CLI
- [ ] Atomic metadata save (temp + rename)
- [ ] Full rebuild fallback (corrupted/missing/version mismatch)
- [ ] Write 15 unit tests + 10 integration tests

### Critical Test Cases
- [ ] Version mismatch → full rebuild (FINDING-4)
- [ ] File deleted with incoming edges → dangling edges removed (FINDING-2)
- [ ] Edge removal uses node_ids correctly (FINDING-3)
- [ ] TOCTOU limitation documented (FINDING-1)
- [ ] Corrupted metadata → full rebuild
- [ ] No changes → early exit < 100ms

---

## Files to Modify

| File | Change | Lines |
|------|--------|-------|
| `crates/gid-core/src/code_graph/types.rs` | Add 3 structs | +50 |
| `crates/gid-core/src/code_graph/extract.rs` | Add incremental logic | +200 |
| `crates/gid-core/src/code_graph/build.rs` | LSP incremental variant | +50 |
| `crates/gid-core/src/code_graph/mod.rs` | Export new types | +5 |
| `gid-cli/src/main.rs` | Add --full flag | +10 |

**Total**: ~315 lines

---

## Edge Cases Covered

✅ No prior metadata → full rebuild  
✅ Corrupted metadata → full rebuild  
✅ Version mismatch → full rebuild (FINDING-4)  
✅ File modified during extract → next extract catches (FINDING-1)  
✅ File renamed → delete + add  
✅ Deleted file with incoming edges → dangling cleanup (FINDING-2)  
✅ Only non-source files changed → early exit  
✅ `--full` flag → ignore cache  

---

## Risk Assessment

### Mitigated ✅
- Schema evolution → version field (FINDING-4)
- Dangling edges → full edge scan (FINDING-2)
- Edge removal → use node_ids (FINDING-3)
- TOCTOU race → documented (FINDING-1)
- Corrupted cache → full rebuild fallback

### Remaining ⚠️
- Memory overhead for 10K+ files (defer to ISS-007: SQLite)
- Performance degradation > 1000 files (benchmark and document)

---

## Next Steps

1. ✅ Design review complete
2. ➡️ Begin Phase 1: Core implementation (2-3 days)
3. ➡️ Phase 2: LSP integration (1-2 days)
4. ➡️ Phase 3: Polish + tests (1 day)

**Timeline**: 4-6 days total

---

## Approval

**Status**: ✅ APPROVED FOR IMPLEMENTATION  
**Confidence**: HIGH (0 blockers, all findings addressed)  
**Reviewer**: RustClaw AI Design Reviewer  
**Date**: April 6, 2024

**Recommendation**: Proceed with implementation. All architectural questions resolved.

---

## Related Documents

- Full Review: `ISS-006-REVIEW-DESIGN-COMPLETE.md`
- Design Doc: `projects/gid-rs/.gid/features/incremental-extract/DESIGN.md`
- Review Doc: `projects/gid-rs/.gid/features/incremental-extract/DESIGN-review.md`
- Previous Summaries: `ISS-006-DESIGN-REVIEW-SUMMARY.md`, `ISS-006-review-design.md`
