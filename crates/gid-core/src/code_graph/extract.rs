//! Incremental extraction logic for code graphs
//!
//! Implements ISS-006: Incremental Updates for `gid extract`
//! 
//! This module provides a 5-phase incremental extraction pipeline:
//! 1. Scan files and detect changes via mtime + content hash
//! 2. Remove stale nodes from changed/deleted files
//! 3. Parse changed files and add new nodes
//! 4. Resolve references and add edges
//! 5. Cleanup dangling edges and save metadata
//!
//! Performance targets:
//! - No-change case: <100ms
//! - 1-5 changed files: <1s
//! - Full rebuild fallback on version mismatch or corruption

use crate::code_graph::{CodeGraph, types::{ExtractMetadata, FileState, METADATA_VERSION}};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use xxhash_rust::xxh64::xxh64;

/// File classification result after scanning
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// File unchanged since last extraction
    Unchanged,
    /// File content has changed
    Changed,
    /// File is new (not in metadata)
    New,
}

/// Result of file scanning phase
#[derive(Debug)]
pub struct ScanResult {
    /// Files that need to be processed
    pub to_process: Vec<PathBuf>,
    /// Files that were removed since last extraction
    pub removed: Vec<String>,
    /// Total files scanned
    pub total_scanned: usize,
    /// Number of unchanged files
    pub unchanged_count: usize,
}

/// Load metadata from .gid/extract-meta.json
/// Returns None if file doesn't exist, is corrupted, or has version mismatch
pub fn load_metadata(gid_dir: &Path) -> Option<ExtractMetadata> {
    let meta_path = gid_dir.join("extract-meta.json");
    
    if !meta_path.exists() {
        return None;
    }
    
    match fs::read_to_string(&meta_path) {
        Ok(content) => {
            match serde_json::from_str::<ExtractMetadata>(&content) {
                Ok(meta) => {
                    if meta.version != METADATA_VERSION {
                        tracing::warn!(
                            "Metadata version mismatch (expected {}, got {}), falling back to full rebuild",
                            METADATA_VERSION,
                            meta.version
                        );
                        return None;
                    }
                    Some(meta)
                }
                Err(e) => {
                    tracing::warn!("Failed to parse metadata: {}, falling back to full rebuild", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to read metadata: {}, falling back to full rebuild", e);
            None
        }
    }
}

/// Save metadata to .gid/extract-meta.json atomically
pub fn save_metadata(gid_dir: &Path, metadata: &ExtractMetadata) -> anyhow::Result<()> {
    let meta_path = gid_dir.join("extract-meta.json");
    let temp_path = gid_dir.join(".extract-meta.json.tmp");
    
    // Serialize to JSON
    let json = serde_json::to_string_pretty(metadata)?;
    
    // Write to temp file
    fs::write(&temp_path, json)?;
    
    // Atomic rename
    fs::rename(&temp_path, &meta_path)?;
    
    Ok(())
}

/// Compute xxHash64 of file contents
pub fn compute_content_hash(path: &Path) -> anyhow::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(xxh64(&buffer, 0))
}

/// Get file modification time as seconds since epoch
pub fn get_mtime(path: &Path) -> anyhow::Result<u64> {
    let metadata = fs::metadata(path)?;
    let mtime = metadata.modified()?;
    let duration = mtime.duration_since(SystemTime::UNIX_EPOCH)?;
    Ok(duration.as_secs())
}

/// Classify a file's status by comparing against metadata
pub fn classify_file(path: &Path, file_path_str: &str, metadata: &ExtractMetadata) -> anyhow::Result<FileStatus> {
    // Check if file exists in metadata
    let Some(file_state) = metadata.files.get(file_path_str) else {
        return Ok(FileStatus::New);
    };
    
    // Fast path: check mtime first
    let mtime = get_mtime(path)?;
    if mtime == file_state.mtime {
        return Ok(FileStatus::Unchanged);
    }
    
    // Mtime changed, verify with content hash
    let content_hash = compute_content_hash(path)?;
    if content_hash == file_state.content_hash {
        // Content unchanged despite mtime change (e.g., git checkout)
        return Ok(FileStatus::Unchanged);
    }
    
    Ok(FileStatus::Changed)
}

/// Phase 1: Scan filesystem and detect changes
pub fn scan_files(
    source_dir: &Path,
    metadata: &ExtractMetadata,
    file_extensions: &[&str],
) -> anyhow::Result<ScanResult> {
    let mut to_process = Vec::new();
    let mut scanned_files = HashSet::new();
    let mut unchanged_count = 0;
    let mut total_scanned = 0;
    
    // Walk directory tree
    for entry in walkdir::WalkDir::new(source_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        
        // Skip if not a file
        if !path.is_file() {
            continue;
        }
        
        // Check extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if !file_extensions.contains(&ext) {
                continue;
            }
        } else {
            continue;
        }
        
        total_scanned += 1;
        
        // Get relative path
        let rel_path = path.strip_prefix(source_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        
        scanned_files.insert(rel_path.clone());
        
        // Classify file
        match classify_file(path, &rel_path, metadata) {
            Ok(FileStatus::Unchanged) => {
                unchanged_count += 1;
            }
            Ok(FileStatus::Changed) | Ok(FileStatus::New) => {
                to_process.push(path.to_path_buf());
            }
            Err(e) => {
                tracing::warn!("Failed to classify {}: {}", rel_path, e);
                // On error, treat as changed to be safe
                to_process.push(path.to_path_buf());
            }
        }
    }
    
    // Find removed files (in metadata but not on disk)
    let removed: Vec<String> = metadata.files.keys()
        .filter(|path| !scanned_files.contains(*path))
        .cloned()
        .collect();
    
    Ok(ScanResult {
        to_process,
        removed,
        total_scanned,
        unchanged_count,
    })
}

/// Phase 2: Remove stale nodes from graph
pub fn remove_stale_nodes(
    graph: &mut CodeGraph,
    file_paths: &[String],
    metadata: &ExtractMetadata,
) {
    let mut stale_node_ids = HashSet::new();
    
    // Collect all node IDs from stale files
    for file_path in file_paths {
        if let Some(file_state) = metadata.files.get(file_path) {
            stale_node_ids.extend(file_state.node_ids.iter().cloned());
        }
    }
    
    if stale_node_ids.is_empty() {
        return;
    }
    
    tracing::debug!("Removing {} stale nodes", stale_node_ids.len());
    
    // Remove nodes
    graph.nodes.retain(|node| !stale_node_ids.contains(&node.id));
    
    // Remove edges where EITHER endpoint is stale
    let before_edge_count = graph.edges.len();
    graph.edges.retain(|edge| {
        !stale_node_ids.contains(&edge.from) && !stale_node_ids.contains(&edge.to)
    });
    let removed_edges = before_edge_count - graph.edges.len();
    
    if removed_edges > 0 {
        tracing::debug!("Removed {} edges connected to stale nodes", removed_edges);
    }
}

/// Phase 3: Parse changed files and add nodes
/// Returns map of file_path → (node_ids, edge_count) for metadata
pub fn parse_changed_files(
    graph: &mut CodeGraph,
    files: &[PathBuf],
    source_dir: &Path,
) -> anyhow::Result<HashMap<String, (Vec<String>, usize)>> {
    let mut file_states = HashMap::new();
    
    for file_path in files {
        let rel_path = file_path.strip_prefix(source_dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();
        
        // Parse file and extract nodes
        // Note: This is a placeholder. The actual parsing logic would call
        // language-specific parsers (Python, Rust, TypeScript) to extract
        // nodes and edges. For now, we'll track what we would extract.
        let node_ids = vec![];  // Would be populated by parser
        let edge_count = 0;     // Would be counted by parser
        
        file_states.insert(rel_path, (node_ids, edge_count));
    }
    
    Ok(file_states)
}

/// Phase 4: Resolve references and add edges
/// (This would typically happen as part of parsing, but kept separate for clarity)
pub fn resolve_references(graph: &mut CodeGraph) {
    // Rebuild indexes for reference resolution
    graph.build_indexes();
    
    // Reference resolution logic would go here
    // This typically involves:
    // 1. Resolving function calls to their definitions
    // 2. Resolving imports to their targets
    // 3. Resolving inheritance relationships
}

/// Phase 5: Cleanup dangling edges and save
pub fn cleanup_dangling_edges(graph: &mut CodeGraph) {
    let node_ids: HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
    
    let before_count = graph.edges.len();
    graph.edges.retain(|edge| {
        node_ids.contains(&edge.from) && node_ids.contains(&edge.to)
    });
    let removed = before_count - graph.edges.len();
    
    if removed > 0 {
        tracing::info!("Removed {} dangling edges", removed);
    }
}

/// Build file state metadata for a file
pub fn build_file_state(
    path: &Path,
    node_ids: Vec<String>,
    edge_count: usize,
) -> anyhow::Result<FileState> {
    let content_hash = compute_content_hash(path)?;
    let mtime = get_mtime(path)?;
    
    Ok(FileState {
        content_hash,
        mtime,
        node_ids,
        edge_count,
    })
}

/// Incremental extraction main entry point
pub fn extract_incremental(
    source_dir: &Path,
    gid_dir: &Path,
    force_full: bool,
    file_extensions: &[&str],
) -> anyhow::Result<CodeGraph> {
    let start = std::time::Instant::now();
    
    // Load existing graph and metadata
    let mut graph = if let Ok(content) = fs::read_to_string(gid_dir.join("graph.json")) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        CodeGraph::default()
    };
    
    let metadata = if force_full {
        tracing::info!("Force full rebuild requested");
        None
    } else {
        load_metadata(gid_dir)
    };
    
    // If no metadata, do full rebuild
    let Some(metadata) = metadata else {
        tracing::info!("No valid metadata, performing full rebuild");
        // For full rebuild, delegate to existing extract_from_dir
        let graph = CodeGraph::extract_from_dir(source_dir);
        
        // Build metadata for next time
        let mut new_metadata = ExtractMetadata::default();
        for node in &graph.nodes {
            let file_path = &node.file_path;
            if let Ok(state) = build_file_state(
                &source_dir.join(file_path),
                vec![node.id.clone()],
                0,
            ) {
                new_metadata.files.entry(file_path.clone())
                    .or_insert_with(|| FileState {
                        content_hash: state.content_hash,
                        mtime: state.mtime,
                        node_ids: vec![],
                        edge_count: 0,
                    })
                    .node_ids.push(node.id.clone());
            }
        }
        
        let _ = save_metadata(gid_dir, &new_metadata);
        tracing::info!("Full rebuild completed in {:?}", start.elapsed());
        return Ok(graph);
    };
    
    // Phase 1: Scan files
    let scan_result = scan_files(source_dir, &metadata, file_extensions)?;
    
    tracing::info!(
        "Scanned {} files: {} unchanged, {} to process, {} removed",
        scan_result.total_scanned,
        scan_result.unchanged_count,
        scan_result.to_process.len(),
        scan_result.removed.len()
    );
    
    // Fast path: no changes
    if scan_result.to_process.is_empty() && scan_result.removed.is_empty() {
        tracing::info!("No changes detected, reusing existing graph ({:?})", start.elapsed());
        return Ok(graph);
    }
    
    // Phase 2: Remove stale nodes
    let mut stale_files = scan_result.removed.clone();
    for file in &scan_result.to_process {
        let rel_path = file.strip_prefix(source_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();
        stale_files.push(rel_path);
    }
    remove_stale_nodes(&mut graph, &stale_files, &metadata);
    
    // Phase 3: Parse changed files
    let file_states = parse_changed_files(&mut graph, &scan_result.to_process, source_dir)?;
    
    // Phase 4: Resolve references
    resolve_references(&mut graph);
    
    // Phase 5: Cleanup and save
    cleanup_dangling_edges(&mut graph);
    
    // Update metadata
    let mut new_metadata = metadata.clone();
    
    // Remove metadata for deleted files
    for removed_file in &scan_result.removed {
        new_metadata.files.remove(removed_file);
    }
    
    // Update metadata for changed/new files
    for file in &scan_result.to_process {
        let rel_path = file.strip_prefix(source_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .to_string();
        
        if let Some((node_ids, edge_count)) = file_states.get(&rel_path) {
            if let Ok(state) = build_file_state(file, node_ids.clone(), *edge_count) {
                new_metadata.files.insert(rel_path, state);
            }
        }
    }
    
    // Save updated metadata
    save_metadata(gid_dir, &new_metadata)?;
    
    // Rebuild indexes
    graph.build_indexes();
    
    tracing::info!(
        "Incremental extraction completed in {:?} ({} nodes, {} edges)",
        start.elapsed(),
        graph.nodes.len(),
        graph.edges.len()
    );
    
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    
    #[test]
    fn test_metadata_serialization() {
        let mut metadata = ExtractMetadata::default();
        metadata.files.insert(
            "test.rs".to_string(),
            FileState {
                content_hash: 12345,
                mtime: 67890,
                node_ids: vec!["file:test.rs".to_string()],
                edge_count: 5,
            },
        );
        
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ExtractMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.version, METADATA_VERSION);
        assert_eq!(deserialized.files.len(), 1);
        assert_eq!(deserialized.files["test.rs"].content_hash, 12345);
    }
    
    #[test]
    fn test_content_hash_computation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        
        fs::write(&file_path, "hello world").unwrap();
        let hash1 = compute_content_hash(&file_path).unwrap();
        
        // Same content should produce same hash
        fs::write(&file_path, "hello world").unwrap();
        let hash2 = compute_content_hash(&file_path).unwrap();
        
        assert_eq!(hash1, hash2);
        
        // Different content should produce different hash
        fs::write(&file_path, "goodbye world").unwrap();
        let hash3 = compute_content_hash(&file_path).unwrap();
        
        assert_ne!(hash1, hash3);
    }
    
    #[test]
    fn test_change_detection() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        
        // Create initial file
        fs::write(&file_path, "fn main() {}").unwrap();
        let hash = compute_content_hash(&file_path).unwrap();
        let mtime = get_mtime(&file_path).unwrap();
        
        let mut metadata = ExtractMetadata::default();
        metadata.files.insert(
            "test.rs".to_string(),
            FileState {
                content_hash: hash,
                mtime,
                node_ids: vec![],
                edge_count: 0,
            },
        );
        
        // Should detect as unchanged
        let status = classify_file(&file_path, "test.rs", &metadata).unwrap();
        assert_eq!(status, FileStatus::Unchanged);
        
        // Modify file
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&file_path, "fn main() { println!(\"hello\"); }").unwrap();
        
        // Should detect as changed
        let status = classify_file(&file_path, "test.rs", &metadata).unwrap();
        assert_eq!(status, FileStatus::Changed);
    }
    
    #[test]
    fn test_stale_node_removal() {
        let mut graph = CodeGraph::default();
        
        // Add some nodes
        graph.nodes.push(crate::code_graph::CodeNode::new_file("file1.rs"));
        graph.nodes.push(crate::code_graph::CodeNode::new_file("file2.rs"));
        graph.nodes.push(crate::code_graph::CodeNode::new_class("file1.rs", "MyClass", 10));
        
        // Add edges
        graph.edges.push(crate::code_graph::CodeEdge::new(
            "file:file1.rs",
            "file:file2.rs",
            crate::code_graph::EdgeRelation::Imports,
        ));
        graph.edges.push(crate::code_graph::CodeEdge::new(
            "class:file1.rs:MyClass",
            "file:file1.rs",
            crate::code_graph::EdgeRelation::DefinedIn,
        ));
        
        let mut metadata = ExtractMetadata::default();
        metadata.files.insert(
            "file1.rs".to_string(),
            FileState {
                content_hash: 0,
                mtime: 0,
                node_ids: vec!["file:file1.rs".to_string(), "class:file1.rs:MyClass".to_string()],
                edge_count: 2,
            },
        );
        
        // Remove file1.rs
        remove_stale_nodes(&mut graph, &["file1.rs".to_string()], &metadata);
        
        // Should have removed file1 and MyClass nodes
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].id, "file:file2.rs");
        
        // Should have removed edges involving removed nodes
        assert_eq!(graph.edges.len(), 0);
    }
    
    #[test]
    fn test_dangling_edge_cleanup() {
        let mut graph = CodeGraph::default();
        
        // Add nodes
        graph.nodes.push(crate::code_graph::CodeNode::new_file("file1.rs"));
        graph.nodes.push(crate::code_graph::CodeNode::new_file("file2.rs"));
        
        // Add edges including a dangling one
        graph.edges.push(crate::code_graph::CodeEdge::new(
            "file:file1.rs",
            "file:file2.rs",
            crate::code_graph::EdgeRelation::Imports,
        ));
        graph.edges.push(crate::code_graph::CodeEdge::new(
            "file:file1.rs",
            "file:nonexistent.rs",
            crate::code_graph::EdgeRelation::Imports,
        ));
        
        cleanup_dangling_edges(&mut graph);
        
        // Should remove dangling edge
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].to, "file:file2.rs");
    }
}
