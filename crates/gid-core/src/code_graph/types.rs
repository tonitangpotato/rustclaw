//! Core types for code graph representation
//!
//! Defines the fundamental data structures:
//! - CodeGraph: the main graph structure with nodes and edges
//! - CodeNode: represents files, classes, and functions
//! - CodeEdge: represents relationships between nodes
//! - NodeKind, EdgeRelation: enumerations for node/edge types
//! - ImpactReport, CausalChain: types for impact analysis

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ═══ Graph Types ═══

/// A code dependency graph extracted from source files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeGraph {
    pub nodes: Vec<CodeNode>,
    pub edges: Vec<CodeEdge>,
    /// Adjacency list: node_id → indices into self.edges (outgoing)
    #[serde(skip)]
    pub outgoing: HashMap<String, Vec<usize>>,
    /// Reverse adjacency list: node_id → indices into self.edges (incoming)
    #[serde(skip)]
    pub incoming: HashMap<String, Vec<usize>>,
    /// Node lookup: node_id → index into self.nodes
    #[serde(skip)]
    pub node_index: HashMap<String, usize>,
}

/// Visibility level of a code entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Crate,
    Protected,
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Private
    }
}

/// A node in the code graph (file, class, function).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    pub id: String,
    pub kind: NodeKind,
    pub name: String,
    pub file_path: String,
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decorators: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
    #[serde(default)]
    pub line_count: usize,
    #[serde(default)]
    pub is_test: bool,
    #[serde(default)]
    pub visibility: Visibility,
    #[serde(default)]
    pub is_abstract: bool,
}

impl CodeNode {
    pub fn new_file(path: &str) -> Self {
        Self {
            id: format!("file:{}", path),
            kind: NodeKind::File,
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            file_path: path.to_string(),
            line: None,
            decorators: Vec::new(),
            signature: None,
            docstring: None,
            line_count: 0,
            is_test: path.contains("/test") || path.contains("_test."),
        }
    }

    pub fn new_class(path: &str, name: &str, line: usize) -> Self {
        Self {
            id: format!("class:{}:{}", path, name),
            kind: NodeKind::Class,
            name: name.to_string(),
            file_path: path.to_string(),
            line: Some(line),
            decorators: Vec::new(),
            signature: None,
            docstring: None,
            line_count: 0,
            is_test: name.starts_with("Test") || path.contains("/test"),
        }
    }

    pub fn new_function(path: &str, name: &str, line: usize, is_method: bool) -> Self {
        let prefix = if is_method { "method" } else { "func" };
        Self {
            id: format!("{}:{}:{}", prefix, path, name),
            kind: NodeKind::Function,
            name: name.to_string(),
            file_path: path.to_string(),
            line: Some(line),
            decorators: Vec::new(),
            signature: None,
            docstring: None,
            line_count: 0,
            is_test: name.starts_with("test_") || name.starts_with("test"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Class,
    Function,
}

/// An edge in the code graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEdge {
    pub from: String,
    pub to: String,
    pub relation: EdgeRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
}

impl CodeEdge {
    pub fn new(from: String, to: String, relation: EdgeRelation) -> Self {
        Self {
            from,
            to,
            relation,
            line: None,
            confidence: 1.0,
            context: Vec::new(),
        }
    }

    /// Builder method to add line information
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Builder method to set confidence
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Builder method to add context
    pub fn with_context(mut self, context: Vec<String>) -> Self {
        self.context = context;
        self
    }

    /// Convenience constructor for call edges
    pub fn call(from: String, to: String) -> Self {
        Self::new(from, to, EdgeRelation::Calls)
    }

    /// Convenience constructor for import edges
    pub fn import(from: String, to: String) -> Self {
        Self::new(from, to, EdgeRelation::Imports)
    }

    /// Convenience constructor for inherits edges
    pub fn inherits(from: String, to: String) -> Self {
        Self::new(from, to, EdgeRelation::Inherits)
    }

    /// Convenience constructor for defined_in edges
    pub fn defined_in(from: String, to: String) -> Self {
        Self::new(from, to, EdgeRelation::DefinedIn)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeRelation {
    Imports,
    Calls,
    Inherits,
    DefinedIn,
    /// References (e.g., uses but doesn't call)
    References,
}

impl std::fmt::Display for EdgeRelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeRelation::Imports => write!(f, "imports"),
            EdgeRelation::Calls => write!(f, "calls"),
            EdgeRelation::Inherits => write!(f, "inherits"),
            EdgeRelation::DefinedIn => write!(f, "defined_in"),
            EdgeRelation::References => write!(f, "references"),
        }
    }
}

// ═══ Impact Analysis Types ═══

/// Report of entities impacted by a change
#[derive(Debug, Clone)]
pub struct ImpactReport<'a> {
    pub node: &'a CodeNode,
    pub downstream: Vec<&'a CodeNode>,
    pub upstream: Vec<&'a CodeNode>,
}

/// A causal chain from symptom to potential root cause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChain {
    pub nodes: Vec<ChainNode>,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainNode {
    pub node_id: String,
    pub node_name: String,
    pub relation: String,
}

// ═══ Language Detection ═══

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Rust,
    TypeScript,
    JavaScript,
}

impl Language {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "py" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            _ => None,
        }
    }
}

// ═══ Unified Graph Types ═══

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedGraphResult {
    pub nodes: Vec<UnifiedNode>,
    pub edges: Vec<UnifiedEdge>,
}

/// A node in the unified graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
}

/// An edge in the unified graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

// ═══ Incremental Extraction Metadata ═══

/// Current metadata schema version
pub const METADATA_VERSION: u32 = 1;

/// Metadata tracking the state of the last extraction for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractMetadata {
    /// Schema version for compatibility checking
    pub version: u32,
    /// Map of file_path → FileState
    pub files: HashMap<String, FileState>,
}

impl Default for ExtractMetadata {
    fn default() -> Self {
        Self {
            version: METADATA_VERSION,
            files: HashMap::new(),
        }
    }
}

/// State of a single file from the last extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileState {
    /// xxHash64 of file contents (for fast change detection)
    pub content_hash: u64,
    /// Last modified time (seconds since epoch)
    pub mtime: u64,
    /// Node IDs created by parsing this file
    pub node_ids: Vec<String>,
    /// Count of edges where this file's nodes are source (for validation)
    pub edge_count: usize,
}
