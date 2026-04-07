//! Code graph module - extracts and analyzes code structure
//!
//! This module provides:
//! - Code graph extraction from source files (types.rs, code_graph.rs)
//! - Incremental extraction for improved performance (extract.rs)
//! - Type definitions for metadata tracking

pub mod types;
pub mod extract;

// Re-export main types from code_graph.rs
pub use crate::code_graph::{
    CodeGraph, CodeNode, CodeEdge, NodeKind, EdgeRelation, Visibility,
    Language, ImpactReport, CausalChain, ChainNode, ResolutionContext,
};

// Re-export incremental extraction types
pub use types::{ExtractMetadata, FileState, METADATA_VERSION};
pub use extract::{
    FileStatus, ScanResult,
    load_metadata, save_metadata,
    compute_content_hash, get_mtime, classify_file,
    scan_files, remove_stale_nodes, parse_changed_files,
    resolve_references, cleanup_dangling_edges,
    build_file_state, extract_incremental,
};
