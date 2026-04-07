# ISS-006: Incremental Updates for gid extract - Design Review

**Date**: 2024-04-06  
**Reviewer**: Claude (AI Design Review)  
**Status**: ✅ Design Approved with Recommendations

---

## Executive Summary

The design for incremental updates to `gid extract --lsp` is **well-architected** and addresses a critical performance bottleneck. The current 8-minute full rebuild for large projects will be reduced to ~5 seconds for single-file changes (100x speedup). The design is sound, the implementation plan is clear, and the tradeoffs are reasonable.

**Recommendation**: Proceed with implementation as documented in DESIGN.md with the enhancements and considerations outlined below.

---

## Design Strengths

### 1. **Clear Problem Definition**
- ✅ Well-defined pain point: 8-minute rebuilds for trivial changes
- ✅ Quantified impact: 100x speedup for single-file changes
- ✅ Real-world use case: LSP daemon mode where initial analysis is expensive

### 2. **Solid Architecture**
- ✅ Three-way diff algorithm (added/modified/deleted) is industry-standard
- ✅ Content hash + mtime for change detection is robust
- ✅ Cache keying by repo_name + commit hash handles branch switches
- ✅ Graph merge strategy (remove old + insert new) is sound

### 3. **Implementation Plan**
- ✅ Clear type definitions (`FileMetadata`, `CachedCodeGraph`)
- ✅ Well-defined API (`extract_incremental`, `merge_changes`, etc.)
- ✅ Testing strategy covers unit + integration tests
- ✅ CLI changes are minimal and intuitive (`--force` flag)

### 4. **Performance Analysis**
- ✅ Realistic performance expectations (8 min → 5 sec for 1 file)
- ✅ Understands that 10-file changes still get significant speedup (24x)
- ✅ Acknowledges memory tradeoff (5-20MB loaded graph is acceptable)

### 5. **Edge Case Handling**
- ✅ Covers file renames (delete + add)
- ✅ Handles corrupt cache (fallback to full rebuild)
- ✅ Branch switching via commit hash detection
- ✅ LSP mode change invalidation

---

## Design Concerns & Recommendations

### Critical Issues

#### 1. **Cache Invalidation Beyond Commit Hash**
**Issue**: Cache is keyed only by `repo_name + commit_hash`. This misses:
- Configuration changes (`.gitignore` patterns, LSP settings)
- Tree-sitter grammar version updates
- Code graph extraction logic changes (e.g., bug fixes in parser)

**Recommendation**: Add a **schema version** to `CachedCodeGraph`:
```rust
pub struct CachedCodeGraph {
    pub schema_version: u32,  // Bump on breaking changes
    pub graph: CodeGraph,
    pub metadata: HashMap<String, FileMetadata>,
    pub extracted_at: u64,
    pub lsp_enabled: bool,
    pub ignore_patterns_hash: String,  // Hash of .gitignore rules
}

const CURRENT_SCHEMA_VERSION: u32 = 1;
```

If schema versions mismatch → full rebuild. This prevents subtle bugs from stale caches.

#### 2. **Cross-File Dependency Invalidation**
**Issue**: Changing file A may affect edges pointing **to** entities in file A from unchanged files. Current design only re-extracts changed files, so stale edges may remain.

**Example**:
```rust
// file_a.rs
pub fn foo() { ... }

// file_b.rs (unchanged)
fn bar() { foo(); }  // Edge: file_b::bar → file_a::foo
```

If `foo` is renamed/deleted in `file_a.rs`, the edge from `file_b.rs` becomes stale, but file_b isn't re-extracted.

**Recommendation**: Add a **reverse dependency invalidation** phase:
1. After detecting changed files, identify all **inbound edges** to changed files
2. Mark source files of those edges as "needs edge re-extraction"
3. Re-run LSP queries for call sites in those files (without full tree-sitter parse)

This is a targeted fix that doesn't require re-parsing all files.

#### 3. **Memory Overhead for Large Graphs**
**Issue**: Must load entire cached graph into memory (~5-20MB). For very large projects (10K+ files), this could be 50-100MB.

**Recommendation**: 
- Document memory requirements in CLI help text
- Add a `--memory-profile` flag to report memory usage
- Consider a future optimization: store graph in SQLite for partial loading (not for ISS-006)

### Medium-Priority Issues

#### 4. **Atomic Cache Updates**
**Issue**: If extraction fails mid-merge, cache could be left in inconsistent state.

**Recommendation**: Use atomic file operations:
```rust
// Write to temp file, then atomic rename
let temp_file = cache_file.with_extension(".tmp");
std::fs::write(&temp_file, json)?;
std::fs::rename(&temp_file, &cache_file)?;  // Atomic on POSIX
```

#### 5. **LSP Query Batching**
**Issue**: Each changed file triggers independent LSP queries. For 10 modified files with 50 call sites each → 500 sequential queries.

**Recommendation**: Implement **concurrent LSP query batching**:
```rust
use rayon::prelude::*;

changed_files.par_iter().for_each(|file| {
    // Query LSP for all call sites in file concurrently
});
```

This could reduce 10-file extraction from 20 sec → 8 sec.

#### 6. **Content Hash Algorithm Choice**
**Issue**: SHA-256 is secure but overkill for change detection. xxHash or BLAKE3 are 3-5x faster.

**Recommendation**: Use **xxHash** for file hashing:
```rust
use xxhash_rust::xxh3::xxh3_64;

fn compute_hash(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let hash = xxh3_64(&bytes);
    Ok(format!("{:016x}", hash))
}
```

This saves ~100ms per 1000 files on modern hardware.

#### 7. **Progress Reporting**
**Issue**: Users won't know if extraction is frozen or just slow during incremental updates.

**Recommendation**: Add progress bars via `indicatif`:
```rust
let pb = ProgressBar::new(changed_files.len() as u64);
pb.set_message("Extracting changed files...");
for file in changed_files {
    extract_file(file)?;
    pb.inc(1);
}
pb.finish_with_message("Extraction complete");
```

### Low-Priority Enhancements

#### 8. **Cache Compression**
**Issue**: JSON graphs are verbose (~5MB). Could compress to ~1MB with gzip.

**Recommendation**: Defer to ISS-007. Not worth complexity for 5MB.

#### 9. **Parallel Tree-Sitter Parsing**
**Issue**: Tree-sitter parsing is CPU-bound. Parsing 10 files sequentially is slower than parallel.

**Recommendation**: Use `rayon` for parallel parsing:
```rust
changed_files.par_iter().map(|file| {
    parse_with_tree_sitter(file)
}).collect()
```

#### 10. **Cache Warmup Command**
**Issue**: First run after git clone is slow. Could pre-compute cache.

**Recommendation**: Add `gid extract --warmup` to run in CI:
```bash
git clone repo
cd repo
gid extract --lsp --warmup  # Saves to cache
# Later: gid extract --lsp   # Instant (loads from cache)
```

---

## Implementation Checklist

Based on the design review, here's a prioritized implementation checklist:

### Phase 1: Core Functionality (ISS-006 MVP)
- [ ] Define `FileMetadata`, `CachedCodeGraph`, `ChangedFiles` types
- [ ] Implement `compute_metadata()` with xxHash (not SHA-256)
- [ ] Implement `find_changed_files()` three-way diff
- [ ] Implement `remove_deleted_files()` graph cleanup
- [ ] Implement `merge_changes()` graph merge
- [ ] Add `extract_incremental()` orchestration method
- [ ] Update CLI to support `--force` flag
- [ ] Add atomic cache file writes (temp + rename)
- [ ] Write unit tests for core functions

### Phase 2: Correctness & Robustness (ISS-006.1)
- [ ] **Add schema versioning** (CRITICAL)
- [ ] **Implement reverse dependency invalidation** (CRITICAL)
- [ ] Add cache corruption detection (JSON parse errors)
- [ ] Add LSP mode change detection
- [ ] Write integration tests (10+ scenarios)
- [ ] Add `gid cache clear` command
- [ ] Add `gid cache info` command

### Phase 3: Performance & UX (ISS-006.2)
- [ ] Implement concurrent LSP query batching
- [ ] Add progress bars for extraction steps
- [ ] Benchmark and document actual speedups
- [ ] Add `--memory-profile` flag
- [ ] Optimize for 100-file change scenarios

### Phase 4: Future Enhancements (ISS-007+)
- [ ] Parallel tree-sitter parsing with rayon
- [ ] Cache compression (gzip or zstd)
- [ ] `gid extract --warmup` for CI
- [ ] SQLite-backed graph for very large projects

---

## Testing Strategy

### Unit Tests (Required for ISS-006)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_metadata() {
        // Create temp file with known content
        // Verify hash + mtime extraction
    }

    #[test]
    fn test_find_changed_files_added() {
        // Empty cached metadata, 1 file current
        // Assert: 1 added, 0 modified, 0 deleted
    }

    #[test]
    fn test_find_changed_files_modified() {
        // Same file in both, different hash
        // Assert: 0 added, 1 modified, 0 deleted
    }

    #[test]
    fn test_find_changed_files_deleted() {
        // 1 file in cached, 0 in current
        // Assert: 0 added, 0 modified, 1 deleted
    }

    #[test]
    fn test_remove_deleted_files() {
        // Graph with 3 files, delete 1
        // Assert: nodes/edges from deleted file removed
    }

    #[test]
    fn test_merge_changes_replaces_modified() {
        // Graph with file A (old version)
        // Extract file A (new version) and merge
        // Assert: old nodes/edges removed, new ones added
    }

    #[test]
    fn test_schema_version_mismatch() {
        // Load cache with old schema version
        // Assert: returns None (forces full rebuild)
    }

    #[test]
    fn test_reverse_dependency_invalidation() {
        // File A has function foo()
        // File B calls foo()
        // Change file A (rename foo → bar)
        // Assert: edge from B is detected as stale
    }
}
```

### Integration Tests (Required for ISS-006.1)

```rust
#[test]
fn test_incremental_single_file_change() {
    // Full extraction → save cache
    // Modify 1 file
    // Incremental extraction
    // Assert: only 1 file re-extracted, graph correct
}

#[test]
fn test_incremental_file_deleted() {
    // Full extraction with 3 files
    // Delete 1 file
    // Incremental extraction
    // Assert: nodes/edges removed
}

#[test]
fn test_incremental_file_added() {
    // Full extraction with 2 files
    // Add 1 file with new imports
    // Incremental extraction
    // Assert: new nodes + import edges added
}

#[test]
fn test_force_flag_ignores_cache() {
    // Full extraction → save cache
    // Run with --force
    // Assert: cache not loaded, full re-extraction
}

#[test]
fn test_corrupt_cache_fallback() {
    // Write invalid JSON to cache file
    // Run extraction
    // Assert: detects corruption, runs full rebuild
}

#[test]
fn test_branch_switch_invalidates_cache() {
    // Extract on branch A → cache keyed to commit_a
    // Switch to branch B (commit_b)
    // Run extraction
    // Assert: cache miss, full rebuild
}
```

---

## Performance Benchmarks

Document these metrics in the implementation PR:

| Scenario | Before (full) | After (incremental) | Speedup |
|----------|---------------|---------------------|---------|
| 1 file changed (TS project, 1000 files) | 8 min | 5 sec | 96x |
| 10 files changed | 8 min | 20 sec | 24x |
| 100 files changed | 8 min | 2 min | 4x |
| First run (no cache) | 8 min | 8 min | 1x |
| File deleted | 8 min | 3 sec | 160x |
| File added (no imports) | 8 min | 4 sec | 120x |

---

## Risk Assessment

### Low Risk ✅
- Cache corruption (handled with fallback)
- File renames (handled as delete + add)
- Branch switches (cache miss is acceptable)

### Medium Risk ⚠️
- **Cross-file dependency invalidation**: Needs careful implementation
- **Memory overhead**: May hit limits on very large projects (>10K files)
- **LSP query correctness**: Must ensure changed call sites are re-queried

### High Risk 🚨
- **Schema versioning missing**: Could cause silent bugs from stale caches
  - **Mitigation**: Add schema version in ISS-006 MVP (Phase 1)
- **Non-atomic cache writes**: Could corrupt cache on crash
  - **Mitigation**: Use temp file + atomic rename pattern

---

## Comparison with Alternatives

### Alternative 1: File-watcher Daemon
**Approach**: Run `gid watch` daemon that incrementally updates on file changes.

**Pros**:
- Near-instant updates (100ms latency)
- No manual `gid extract` runs

**Cons**:
- Complex daemon management (crashes, restarts)
- Requires OS-specific file watching (inotify, FSEvents)
- Memory overhead (daemon always running)

**Verdict**: ❌ Overkill for current use case. Incremental extraction on-demand is simpler.

### Alternative 2: Lazy Loading from SQLite
**Approach**: Store graph in SQLite, query subsets on demand.

**Pros**:
- No memory overhead (load only needed nodes)
- Scales to very large projects (100K+ files)

**Cons**:
- Complex query planning (which nodes to load?)
- SQLite schema design overhead
- Slower for small projects (disk I/O)

**Verdict**: ⏭️ Defer to future (ISS-020). Current HashMap approach works for 95% of projects.

### Alternative 3: Git-based Change Detection
**Approach**: Use `git diff` to find changed files instead of content hashing.

**Pros**:
- Faster than hashing (git already tracks changes)
- Handles renames correctly

**Cons**:
- Only works in git repos (not for non-git projects)
- Doesn't detect unstaged changes
- Requires shelling out to `git` (platform-specific)

**Verdict**: ⚠️ Consider as optimization in ISS-006.2 (if repo is git, use `git diff`, else hash).

---

## Open Questions

1. **Should we cache LSP query results separately?**
   - LSP queries are slow (50-100ms each). Could cache (call_site, line, col) → definition mappings.
   - **Recommendation**: Defer to ISS-008. Adds complexity.

2. **How to handle language server crashes?**
   - LSP daemon may crash mid-extraction. Should we retry or fallback to tree-sitter?
   - **Recommendation**: Log warning, skip LSP for that file, keep tree-sitter edges.

3. **Should `--force` clear the cache or just ignore it?**
   - Ignoring: Fast for testing (re-run with/without cache).
   - Clearing: Prevents stale cache buildup.
   - **Recommendation**: `--force` ignores cache but doesn't delete. Add `gid cache clear` for deletion.

4. **What's the UX for "cache outdated" scenarios?**
   - If schema version mismatches, should we print a message?
   - **Recommendation**: Log: "Cache outdated (schema v0, current v1). Running full extraction..."

5. **Should we support manual cache invalidation?**
   - E.g., `gid extract --invalidate file_a.rs` to mark specific files as stale.
   - **Recommendation**: Defer to ISS-009. Niche use case.

---

## Documentation Requirements

### User-Facing Docs
- [ ] Update `gid extract --help` text to mention incremental mode
- [ ] Add section to user guide: "Performance: Incremental Extraction"
- [ ] Document `--force` flag behavior
- [ ] Document cache location (`.graph-cache/`)
- [ ] Add FAQ: "How do I clear the cache?"

### Developer Docs
- [ ] Add architecture diagram to DESIGN.md (done ✅)
- [ ] Document `FileMetadata` and `CachedCodeGraph` schemas
- [ ] Add code comments for `merge_changes()` algorithm
- [ ] Document reverse dependency invalidation logic

### Changelog
- [ ] Add entry to CHANGELOG.md for ISS-006

---

## Conclusion

**Overall Assessment**: ✅ **Excellent Design**

The ISS-006 design is well-thought-out and addresses a real pain point with a pragmatic solution. The architecture is sound, the performance gains are significant, and the implementation plan is clear.

**Critical Action Items**:
1. ✅ Add schema versioning (prevents stale cache bugs)
2. ✅ Implement reverse dependency invalidation (ensures correctness)
3. ✅ Use atomic cache writes (prevents corruption)

**Recommendation**: **Approve for implementation** with the critical enhancements noted above. Prioritize Phase 1 (core functionality) and Phase 2 (correctness) before optimizing in Phase 3.

**Estimated Implementation Time**:
- Phase 1: 2-3 days
- Phase 2: 1-2 days
- Phase 3: 1-2 days
- **Total**: 4-7 days for full ISS-006 completion

---

## Approval

**Design Review Status**: ✅ **APPROVED WITH RECOMMENDATIONS**

**Reviewer**: Claude (AI Design Reviewer)  
**Date**: 2024-04-06  
**Next Step**: Proceed to implementation with Phase 1 checklist

---

## Appendix A: Reference Implementation Sketch

```rust
// gid-core/src/code_graph/incremental.rs

use std::collections::HashMap;
use std::path::Path;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub mtime: u64,
    pub content_hash: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedCodeGraph {
    pub schema_version: u32,
    pub graph: CodeGraph,
    pub metadata: HashMap<String, FileMetadata>,
    pub extracted_at: u64,
    pub lsp_enabled: bool,
    pub ignore_patterns_hash: String,
}

#[derive(Debug)]
pub struct ChangedFiles {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl CodeGraph {
    pub fn extract_incremental(
        repo_dir: &Path,
        cache_dir: &Path,
        force: bool,
        lsp_enabled: bool,
    ) -> Result<(Self, IncrementalStats)> {
        let mut stats = IncrementalStats::default();
        let start = Instant::now();

        // Load cached graph (if exists and not forced)
        let cache_file = cache_dir.join("graph_cache.json");
        let cached = if !force && cache_file.exists() {
            match load_cached_graph(&cache_file) {
                Ok(cached) if cached.schema_version == SCHEMA_VERSION => Some(cached),
                Ok(_) => {
                    tracing::info!("Cache schema outdated, running full extraction");
                    None
                }
                Err(e) => {
                    tracing::warn!("Cache corrupted: {}, running full extraction", e);
                    None
                }
            }
        } else {
            None
        };

        // Scan filesystem and compute current metadata
        let current_metadata = scan_and_hash(repo_dir)?;
        stats.total_files = current_metadata.len();

        // Find changed files
        let changed = if let Some(ref cached) = cached {
            find_changed_files(&current_metadata, &cached.metadata)
        } else {
            // No cache → treat all as added
            ChangedFiles {
                added: current_metadata.keys().cloned().collect(),
                modified: Vec::new(),
                deleted: Vec::new(),
            }
        };

        stats.added_files = changed.added.len();
        stats.modified_files = changed.modified.len();
        stats.deleted_files = changed.deleted.len();
        stats.unchanged_files = stats.total_files - stats.added_files - stats.modified_files;

        // Start with cached graph or empty
        let mut graph = cached.as_ref().map(|c| c.graph.clone()).unwrap_or_default();

        // Remove deleted files
        if !changed.deleted.is_empty() {
            graph.remove_deleted_files(&changed.deleted);
            stats.nodes_removed = changed.deleted.len(); // Approximate
        }

        // Extract and merge changed files
        if !changed.added.is_empty() || !changed.modified.is_empty() {
            let mut changed_files = changed.added.clone();
            changed_files.extend(changed.modified.clone());
            
            graph.merge_changes(&changed_files, repo_dir, lsp_enabled)?;
            stats.nodes_added = changed_files.len(); // Approximate
        }

        // Rebuild indexes
        graph.build_indexes();

        // Save cache atomically
        let cached_graph = CachedCodeGraph {
            schema_version: SCHEMA_VERSION,
            graph: graph.clone(),
            metadata: current_metadata,
            extracted_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
            lsp_enabled,
            ignore_patterns_hash: compute_ignore_hash()?,
        };

        save_cached_graph(&cache_file, &cached_graph)?;

        stats.extraction_time_ms = start.elapsed().as_millis() as u64;
        Ok((graph, stats))
    }
}

fn compute_hash(path: &Path) -> Result<String> {
    use xxhash_rust::xxh3::xxh3_64;
    let bytes = std::fs::read(path)?;
    Ok(format!("{:016x}", xxh3_64(&bytes)))
}

fn save_cached_graph(path: &Path, cached: &CachedCodeGraph) -> Result<()> {
    let temp_path = path.with_extension(".tmp");
    let json = serde_json::to_string(cached)?;
    std::fs::write(&temp_path, json)?;
    std::fs::rename(&temp_path, path)?; // Atomic
    Ok(())
}
```

---

**End of Design Review**
