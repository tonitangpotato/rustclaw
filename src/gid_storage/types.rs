/// Extended Node and Edge types for SQLite storage
use serde::{Deserialize, Serialize};

/// Graph node with extended metadata fields
/// 
/// This structure supports both code graph nodes (functions, classes, modules)
/// and task graph nodes (requirements, features, tasks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    /// Unique identifier for the node
    pub id: String,

    /// Human-readable name (function name, task title, etc.)
    pub name: String,

    // ============================================================
    // Extended fields (14 new fields per design-storage.md §12)
    // ============================================================
    
    /// Source file path (relative or absolute)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Programming language (rust, typescript, python, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    /// Function/class signature (e.g., "fn parse(input: &str) -> Result<Node>")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    /// Node type classification (function, class, module, task, requirement, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,

    /// Human-readable description or documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Task priority (1-100, higher = more important)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,

    /// Task assignee (username, email, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,

    /// Starting line number in source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,

    /// Ending line number in source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,

    /// Full source code or content body
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,

    /// Parent node ID (for hierarchical structures)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Depth in hierarchy (0 = root)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,

    /// Cyclomatic complexity or complexity estimate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<i32>,

    /// Public vs private visibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
}

impl Node {
    /// Create a new node with minimal required fields
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Node {
            id: id.into(),
            name: name.into(),
            file_path: None,
            lang: None,
            signature: None,
            node_type: None,
            description: None,
            priority: None,
            assigned_to: None,
            line_start: None,
            line_end: None,
            body: None,
            parent_id: None,
            depth: None,
            complexity: None,
            is_public: None,
        }
    }

    /// Builder pattern: set file path
    pub fn with_file_path(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Builder pattern: set language
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = Some(lang.into());
        self
    }

    /// Builder pattern: set signature
    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(sig.into());
        self
    }

    /// Builder pattern: set node type
    pub fn with_node_type(mut self, node_type: impl Into<String>) -> Self {
        self.node_type = Some(node_type.into());
        self
    }

    /// Builder pattern: set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Builder pattern: set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Builder pattern: set assignee
    pub fn with_assigned_to(mut self, assignee: impl Into<String>) -> Self {
        self.assigned_to = Some(assignee.into());
        self
    }

    /// Builder pattern: set line range
    pub fn with_line_range(mut self, start: u32, end: u32) -> Self {
        self.line_start = Some(start);
        self.line_end = Some(end);
        self
    }

    /// Builder pattern: set body
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Builder pattern: set parent
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Builder pattern: set depth
    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = Some(depth);
        self
    }

    /// Builder pattern: set complexity
    pub fn with_complexity(mut self, complexity: i32) -> Self {
        self.complexity = Some(complexity);
        self
    }

    /// Builder pattern: set visibility
    pub fn with_is_public(mut self, is_public: bool) -> Self {
        self.is_public = Some(is_public);
        self
    }
}

/// Graph edge with metadata support
///
/// Represents a directed relationship between two nodes.
/// Examples: calls, imports, depends_on, blocks, parent_of, etc.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    /// Source node ID
    pub from: String,

    /// Target node ID
    pub to: String,

    /// Edge type/relationship (calls, imports, depends_on, blocks, etc.)
    pub edge_type: String,

    // ============================================================
    // Extended field (per design-storage.md §12)
    // ============================================================
    
    /// Arbitrary JSON metadata for edge-specific attributes
    /// 
    /// Examples:
    /// - Call edges: {"call_type": "direct", "args_count": 3}
    /// - Import edges: {"import_type": "default", "alias": "React"}
    /// - Dependency edges: {"version": "^1.2.3", "dev": false}
    /// - Task edges: {"blocking": true, "effort_hours": 8}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl Edge {
    /// Create a new edge with minimal required fields
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        edge_type: impl Into<String>,
    ) -> Self {
        Edge {
            from: from.into(),
            to: to.into(),
            edge_type: edge_type.into(),
            metadata: None,
        }
    }

    /// Builder pattern: set metadata
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Builder pattern: set metadata from serializable value
    pub fn with_metadata_from<T: Serialize>(mut self, value: &T) -> Result<Self, serde_json::Error> {
        self.metadata = Some(serde_json::to_value(value)?);
        Ok(self)
    }
}

/// Change log entry for audit trail
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChangeEntry {
    /// Auto-incrementing change ID
    pub id: i64,

    /// Node ID affected by this change (None for system-wide changes)
    pub node_id: Option<String>,

    /// Type of change (create, update, delete, etc.)
    pub change_type: String,

    /// Human-readable description of the change
    pub description: String,

    /// Unix timestamp (seconds since epoch)
    pub timestamp: i64,
}

impl ChangeEntry {
    /// Create a new change entry
    pub fn new(
        id: i64,
        node_id: Option<String>,
        change_type: impl Into<String>,
        description: impl Into<String>,
        timestamp: i64,
    ) -> Self {
        ChangeEntry {
            id,
            node_id,
            change_type: change_type.into(),
            description: description.into(),
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_builder() {
        let node = Node::new("fn:parse", "parse")
            .with_file_path("src/parser.rs")
            .with_lang("rust")
            .with_node_type("function")
            .with_line_range(10, 50)
            .with_complexity(8)
            .with_is_public(true);

        assert_eq!(node.id, "fn:parse");
        assert_eq!(node.name, "parse");
        assert_eq!(node.file_path, Some("src/parser.rs".to_string()));
        assert_eq!(node.lang, Some("rust".to_string()));
        assert_eq!(node.node_type, Some("function".to_string()));
        assert_eq!(node.line_start, Some(10));
        assert_eq!(node.line_end, Some(50));
        assert_eq!(node.complexity, Some(8));
        assert_eq!(node.is_public, Some(true));
    }

    #[test]
    fn test_edge_builder() {
        let edge = Edge::new("fn:main", "fn:parse", "calls")
            .with_metadata(serde_json::json!({
                "call_type": "direct",
                "args_count": 2
            }));

        assert_eq!(edge.from, "fn:main");
        assert_eq!(edge.to, "fn:parse");
        assert_eq!(edge.edge_type, "calls");
        assert!(edge.metadata.is_some());

        let metadata = edge.metadata.unwrap();
        assert_eq!(metadata["call_type"], "direct");
        assert_eq!(metadata["args_count"], 2);
    }

    #[test]
    fn test_node_serialization() {
        let node = Node::new("test:1", "Test Node")
            .with_priority(50)
            .with_assigned_to("alice");

        let json = serde_json::to_string(&node).unwrap();
        let deserialized: Node = serde_json::from_str(&json).unwrap();

        assert_eq!(node, deserialized);
    }

    #[test]
    fn test_edge_serialization() {
        let edge = Edge::new("a", "b", "depends_on")
            .with_metadata(serde_json::json!({"version": "1.0"}));

        let json = serde_json::to_string(&edge).unwrap();
        let deserialized: Edge = serde_json::from_str(&json).unwrap();

        assert_eq!(edge, deserialized);
    }
}
