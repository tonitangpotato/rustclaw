# ISS-006: Incremental Updates - Implementation Plan

**Date**: April 6, 2024
**Status**: Ready for Implementation

---

## Executive Summary

This document outlines the implementation strategy for ISS-006: Incremental Updates for `gid extract`. The goal is to avoid full re-parsing and LSP querying when only a few files have changed.

---

## Current Architecture Analysis

### Current Extraction Flow
1. **`CodeGraph::extract_from_dir()`** - Walks all files, parses with tree-sitter
2. **`CodeGraph::refine_with_lsp()`** - Opens ALL files in LSP, queries definitions for ALL call edges
3. **Cache system** - Based on repo_name + commit hash (all-or-nothing)

### Performance Bottlenecks
- **Full tree-sitter parsing**: ~500ms for medium project
- **LSP initialization**: ~8 minutes for large TypeScript projects
- **LSP queries**: 50-200ms per call edge × hundreds of edges = minutes

### Key Insight
The LSP daemon already persists, but we still:
1. Re-parse all files with tree-sitter
2. Re-open all files in LSP
3. Re-query all call edges

---

## Implementation Strategy

### Phase 1: File Metadata Tracking (ISS-006.1)

**New Module**: `crates/gid-core/src/code_graph/metadata.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

/// Metadata about extracted files for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub mtime: u64,  // Seconds since UNIX epoch
    pub content_hash: String,  // SHA-256 hex
    pub size: u64,
}

/// Stored alongside code graph for incremental detection
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphMetadata {
    pub files: HashMap<String, FileMetadata>,
    pub extraction_time: u64,
    pub lsp_refined: bool,
}

impl FileMetadata {
    pub fn from_path(path: &Path, rel_path: &str) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata.modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let content = std::fs::read_to_string(path)?;
        let content_hash = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
        
        Ok(Self {
            path: rel_path.to_string(),
            mtime,
            content_hash,
            size: metadata.len(),
        })
    }
    
    pub fn has_changed(&self, other: &FileMetadata) -> bool {
        self.mtime != other.mtime || self.content_hash != other.content_hash
    }
}
```

**Integration**: Store metadata alongside graph in cache:
- `.graph-cache/{repo}__{commit}.json` → graph data
- `.graph-cache/{repo}__{commit}.meta.json` → metadata

---

### Phase 2: Change Detection (ISS-006.2)

**New Module**: `crates/gid-core/src/code_graph/incremental.rs`

```rust
pub struct ChangeSet {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl ChangeSet {
    pub fn detect(old_meta: &GraphMetadata, dir: &Path) -> Self {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();
        
        // Scan current files
        let current_files = scan_source_files(dir);
        
        for (rel_path, current_meta) in &current_files {
            match old_meta.files.get(rel_path) {
                None => added.push(rel_path.clone()),
                Some(old) if old.has_changed(current_meta) => {
                    modified.push(rel_path.clone())
                }
                _ => {} // Unchanged
            }
        }
        
        // Find deleted files
        for old_path in old_meta.files.keys() {
            if !current_files.contains_key(old_path) {
                deleted.push(old_path.clone());
            }
        }
        
        ChangeSet { added, modified, deleted }
    }
    
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
    
    pub fn total(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }
}
```

---

### Phase 3: Incremental Extraction (ISS-006.3)

**Extend** `crates/gid-core/src/code_graph/extract.rs`

```rust
impl CodeGraph {
    /// Extract with incremental updates - only re-parse changed files
    pub fn extract_incremental(
        dir: &Path,
        repo_name: &str,
        base_commit: &str,
        force_full: bool,
    ) -> (Self, IncrementalStats) {
        let cache_dir = dir.parent().unwrap_or(dir).join(".graph-cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        
        let safe_repo = repo_name.replace('/', "__");
        let short_commit = &base_commit[..base_commit.len().min(8)];
        let cache_file = cache_dir.join(format!("{}__{}.json", safe_repo, short_commit));
        let meta_file = cache_dir.join(format!("{}__{}.meta.json", safe_repo, short_commit));
        
        // Check if incremental update is possible
        if !force_full && cache_file.exists() && meta_file.exists() {
            if let (Ok(graph_data), Ok(meta_data)) = (
                std::fs::read_to_string(&cache_file),
                std::fs::read_to_string(&meta_file),
            ) {
                if let (Ok(mut graph), Ok(old_meta)) = (
                    serde_json::from_str::<CodeGraph>(&graph_data),
                    serde_json::from_str::<GraphMetadata>(&meta_data),
                ) {
                    // Detect changes
                    let changes = ChangeSet::detect(&old_meta, dir);
                    
                    if changes.is_empty() {
                        // No changes - return cached graph
                        graph.build_indexes();
                        return (graph, IncrementalStats::no_changes());
                    }
                    
                    // Apply incremental update
                    let stats = graph.apply_changes(dir, &changes, &old_meta);
                    
                    // Save updated graph and metadata
                    Self::save_cache(&graph, dir, &cache_file, &meta_file);
                    
                    return (graph, stats);
                }
            }
        }
        
        // Fall back to full extraction
        let graph = Self::extract_from_dir(dir);
        let meta = GraphMetadata::from_dir(dir);
        
        Self::save_cache(&graph, dir, &cache_file, &meta_file);
        
        (graph, IncrementalStats::full_extraction(graph.nodes.len()))
    }
    
    /// Apply changes to existing graph - core incremental logic
    fn apply_changes(
        &mut self,
        dir: &Path,
        changes: &ChangeSet,
        old_meta: &GraphMetadata,
    ) -> IncrementalStats {
        let mut stats = IncrementalStats::default();
        
        // Remove nodes from deleted files
        for deleted_path in &changes.deleted {
            let removed = self.remove_nodes_from_file(deleted_path);
            stats.nodes_removed += removed;
        }
        
        // Re-extract modified files
        for modified_path in &changes.modified {
            // Remove old nodes
            stats.nodes_removed += self.remove_nodes_from_file(modified_path);
            
            // Extract new nodes
            let full_path = dir.join(modified_path);
            let (nodes, edges) = Self::extract_single_file(&full_path, modified_path);
            stats.nodes_added += nodes.len();
            
            self.merge_nodes_and_edges(nodes, edges);
        }
        
        // Extract added files
        for added_path in &changes.added {
            let full_path = dir.join(added_path);
            let (nodes, edges) = Self::extract_single_file(&full_path, added_path);
            stats.nodes_added += nodes.len();
            
            self.merge_nodes_and_edges(nodes, edges);
        }
        
        // Rebuild indexes
        self.build_indexes();
        
        stats.files_changed = changes.total();
        stats
    }
    
    /// Remove all nodes from a specific file
    fn remove_nodes_from_file(&mut self, file_path: &str) -> usize {
        let initial_count = self.nodes.len();
        
        // Find nodes to remove
        let node_ids: Vec<String> = self.nodes.iter()
            .filter(|n| n.file_path == file_path)
            .map(|n| n.id.clone())
            .collect();
        
        // Remove edges referencing these nodes
        self.edges.retain(|e| {
            !node_ids.iter().any(|id| &e.from == id || &e.to == id)
        });
        
        // Remove nodes
        self.nodes.retain(|n| n.file_path != file_path);
        
        initial_count - self.nodes.len()
    }
}

#[derive(Debug, Default)]
pub struct IncrementalStats {
    pub files_changed: usize,
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub full_extraction: bool,
}
```

---

### Phase 4: Incremental LSP Refinement (ISS-006.4)

**Extend** `crates/gid-core/src/code_graph/build.rs`

```rust
impl CodeGraph {
    /// Refine only edges from changed files
    pub fn refine_with_lsp_incremental(
        &mut self,
        root_dir: &Path,
        changed_files: &[String],
    ) -> anyhow::Result<LspRefinementStats> {
        let mut stats = LspRefinementStats::default();
        
        // Only process call edges from changed files
        let changed_file_set: HashSet<&str> = 
            changed_files.iter().map(|s| s.as_str()).collect();
        
        let edge_indices: Vec<usize> = self.edges
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if e.relation != EdgeRelation::Calls {
                    return false;
                }
                
                // Check if caller is in a changed file
                if let Some(caller) = self.node_by_id(&e.from) {
                    changed_file_set.contains(caller.file_path.as_str())
                } else {
                    false
                }
            })
            .map(|(i, _)| i)
            .collect();
        
        stats.total_call_edges = edge_indices.len();
        
        // Rest of LSP refinement logic (same as full version)
        // but only for filtered edges
        
        // ... (reuse existing refine_with_lsp logic)
        
        Ok(stats)
    }
}
```

---

### Phase 5: CLI Integration (ISS-006.5)

**Update** `crates/gid-cli/src/main.rs`

```rust
/// Extract code graph from a directory
Extract {
    /// Directory to extract from (default: current directory)
    #[arg(default_value = ".")]
    dir: PathBuf,
    /// Output format (yaml, json, summary)
    #[arg(short, long, default_value = "summary")]
    format: String,
    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Use LSP servers for precise call edge resolution
    #[arg(long)]
    lsp: bool,
    /// Force full re-extraction (skip incremental)
    #[arg(long)]
    force: bool,
    /// Repository name for cache key
    #[arg(long)]
    repo: Option<String>,
    /// Base commit for cache key
    #[arg(long, default_value = "HEAD")]
    commit: String,
},
```

```rust
fn cmd_extract(
    dir: &PathBuf,
    format: &str,
    output: Option<&std::path::Path>,
    json_flag: bool,
    lsp: bool,
    force: bool,
    repo: Option<String>,
    commit: &str,
) -> Result<()> {
    let dir = if dir.is_absolute() {
        dir.clone()
    } else {
        std::env::current_dir()?.join(dir)
    };
    
    if !dir.exists() {
        bail!("Directory not found: {}", dir.display());
    }
    
    // Determine repo name
    let repo_name = repo.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    
    if !json_flag {
        eprintln!("Extracting code graph from {}...", dir.display());
        if force {
            eprintln!("  Mode: full extraction (--force)");
        } else {
            eprintln!("  Mode: incremental (use --force for full rebuild)");
        }
    }
    
    // Extract with incremental support
    let (mut code_graph, inc_stats) = CodeGraph::extract_incremental(
        &dir,
        &repo_name,
        commit,
        force,
    );
    
    if !json_flag {
        if inc_stats.full_extraction {
            eprintln!("  Performed full extraction: {} nodes", code_graph.nodes.len());
        } else {
            eprintln!(
                "  Incremental update: {} files changed, {} nodes added, {} removed",
                inc_stats.files_changed,
                inc_stats.nodes_added,
                inc_stats.nodes_removed,
            );
        }
    }
    
    // LSP refinement
    if lsp {
        if !json_flag {
            eprintln!("Refining call edges with LSP...");
        }
        
        let changed_files = if inc_stats.full_extraction {
            Vec::new() // Empty = refine all
        } else {
            // Collect changed file paths from stats
            // (need to enhance IncrementalStats to include file list)
            Vec::new()
        };
        
        let lsp_stats = if changed_files.is_empty() {
            code_graph.refine_with_lsp(&dir)?
        } else {
            code_graph.refine_with_lsp_incremental(&dir, &changed_files)?
        };
        
        if !json_flag {
            eprintln!(
                "  LSP: {} refined, {} removed, {} failed, {} skipped",
                lsp_stats.refined,
                lsp_stats.removed,
                lsp_stats.failed,
                lsp_stats.skipped,
            );
        }
    }
    
    // ... rest of output formatting
}
```

---

## Testing Strategy

### Unit Tests

**File**: `crates/gid-core/src/code_graph/incremental.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_detect_added_files() {
        // Create temp dir, add files, verify detection
    }
    
    #[test]
    fn test_detect_modified_files() {
        // Modify file mtime/content, verify detection
    }
    
    #[test]
    fn test_detect_deleted_files() {
        // Remove file, verify detection
    }
    
    #[test]
    fn test_remove_nodes_from_file() {
        // Create graph with nodes from multiple files
        // Remove one file's nodes, verify edges also removed
    }
    
    #[test]
    fn test_incremental_extraction_no_changes() {
        // Extract once, extract again without changes
        // Verify second extraction is instant
    }
}
```

### Integration Tests

**File**: `tests/incremental_extraction.rs`

```rust
#[test]
fn test_incremental_add_file() {
    // 1. Create temp project with 2 files
    // 2. Extract full graph
    // 3. Add a third file
    // 4. Extract incremental
    // 5. Verify only third file was parsed
}

#[test]
fn test_incremental_modify_file() {
    // 1. Extract full graph
    // 2. Modify one file (add a function)
    // 3. Extract incremental
    // 4. Verify new function appears in graph
}

#[test]
fn test_incremental_delete_file() {
    // 1. Extract full graph
    // 2. Delete a file
    // 3. Extract incremental
    // 4. Verify nodes from deleted file are gone
}

#[test]
fn test_incremental_lsp_refinement() {
    // 1. Extract with LSP
    // 2. Modify one file
    // 3. Extract incremental with LSP
    // 4. Verify only modified file's edges were re-queried
}
```

---

## Performance Expectations

### Before (Current)
- **Initial extraction**: 8 minutes (LSP initialization)
- **Change one file**: 8 minutes (full re-extraction)
- **No changes**: 8 minutes (no cache hit if commit changed)

### After (With Incremental)
- **Initial extraction**: 8 minutes (same - one-time cost)
- **Change one file**: ~5-10 seconds (parse 1 file + LSP query ~10 edges)
- **No changes**: <100ms (metadata comparison only)

### Expected Speedup
- **Typical development cycle**: 50-100x faster
- **Large projects**: More dramatic (1000+ files → 1 file)

---

## Implementation Checklist

### ISS-006.1: File Metadata Tracking
- [ ] Create `metadata.rs` module
- [ ] Implement `FileMetadata::from_path()`
- [ ] Implement `GraphMetadata` serialization
- [ ] Update cache system to store `.meta.json`
- [ ] Unit tests for metadata tracking

### ISS-006.2: Change Detection
- [ ] Create `incremental.rs` module
- [ ] Implement `ChangeSet::detect()`
- [ ] Add `scan_source_files()` helper
- [ ] Unit tests for change detection

### ISS-006.3: Incremental Extraction
- [ ] Implement `CodeGraph::extract_incremental()`
- [ ] Implement `CodeGraph::apply_changes()`
- [ ] Implement `CodeGraph::remove_nodes_from_file()`
- [ ] Extract `extract_single_file()` from `extract_from_dir()`
- [ ] Unit tests for incremental extraction

### ISS-006.4: Incremental LSP Refinement
- [ ] Implement `CodeGraph::refine_with_lsp_incremental()`
- [ ] Update LSP daemon to handle partial file updates
- [ ] Unit tests for incremental refinement

### ISS-006.5: CLI Integration
- [ ] Add `--force` flag to `extract` command
- [ ] Add `--repo` and `--commit` flags
- [ ] Update `cmd_extract()` to use incremental extraction
- [ ] Add progress reporting for incremental updates
- [ ] Integration tests

### ISS-006.6: Documentation
- [ ] Update README with incremental extraction usage
- [ ] Add performance benchmarks
- [ ] Document cache file format
- [ ] Add troubleshooting guide

### ISS-006.7: Edge Cases & Polish
- [ ] Handle concurrent extraction attempts
- [ ] Add cache size limits / cleanup
- [ ] Handle corrupted cache gracefully
- [ ] Add metrics/telemetry for incremental vs full

---

## Migration Strategy

### Backward Compatibility
- Old cache files (without `.meta.json`) trigger full extraction
- `--force` flag allows explicit full rebuild
- Default behavior unchanged (uses incremental if available)

### Rollout Plan
1. **Phase 1**: Internal testing with existing projects
2. **Phase 2**: Beta release with opt-in `--incremental` flag
3. **Phase 3**: Make incremental default, add `--no-incremental` to disable
4. **Phase 4**: Remove old cache format support (breaking change)

---

## Success Metrics

### Functional
- ✅ Incremental extraction produces identical graph to full extraction
- ✅ No regressions in existing test suite
- ✅ Handles all file change scenarios (add/modify/delete)

### Performance
- ✅ Single file change: <10 seconds (vs 8 minutes)
- ✅ No changes: <100ms
- ✅ Memory usage: no regression

### User Experience
- ✅ Clear progress messages
- ✅ Graceful fallback on errors
- ✅ Cache cleanup doesn't require manual intervention

---

## Dependencies

### New Crates
```toml
[dependencies]
sha2 = "0.10"  # For content hashing
```

### Existing Crates (already used)
- `serde` / `serde_json` - Metadata serialization
- `walkdir` - File scanning
- `anyhow` - Error handling

---

## Future Enhancements (Out of Scope)

- **Watch mode**: Auto-reextract on file changes
- **Distributed cache**: Share cache across machines
- **Partial graph queries**: Query without loading full graph
- **Incremental formatting**: Only format changed portions

---

## Risk Assessment

### Low Risk
- ✅ Metadata tracking (isolated module)
- ✅ Change detection (pure function)
- ✅ CLI flags (additive)

### Medium Risk
- ⚠️ Graph merging logic (complex edge cases)
- ⚠️ Cache format changes (migration needed)

### High Risk
- ❌ LSP daemon state management (concurrent access)
- ❌ Correctness under concurrent extractions

### Mitigation
- Extensive testing with real projects
- Conservative rollout with opt-in flag
- Clear documentation on `--force` fallback

---

## Conclusion

This implementation provides a solid foundation for incremental extraction while maintaining backward compatibility and graceful degradation. The expected performance improvement (50-100x for typical workflows) will dramatically improve the developer experience, especially for large projects.

**Next Steps**: Begin implementation with ISS-006.1 (File Metadata Tracking).
