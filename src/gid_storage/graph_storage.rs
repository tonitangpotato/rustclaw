/// GraphStorage trait definition
/// 
/// This trait defines the interface for all storage backends (SQLite, PostgreSQL, in-memory, etc.)
use async_trait::async_trait;

use super::error::StorageError;
use super::types::{ChangeEntry, Edge, Node};

/// Storage backend trait for graph persistence
///
/// All storage implementations must be Send + Sync for use in async contexts.
/// Methods use &mut self for operations that modify state (transactions, writes)
/// and &self for read-only operations (queries, searches).
#[async_trait]
pub trait GraphStorage: Send + Sync {
    // ============================================================
    // Lifecycle Management
    // ============================================================

    /// Initialize the storage backend (create tables, indexes, etc.)
    ///
    /// This method is idempotent - calling it multiple times should be safe.
    async fn initialize(&mut self) -> Result<(), StorageError>;

    /// Close the storage backend and release resources
    ///
    /// Any pending transactions should be rolled back.
    async fn close(&mut self) -> Result<(), StorageError>;

    // ============================================================
    // Node Operations
    // ============================================================

    /// Add a new node to the graph
    ///
    /// Returns an error if a node with the same ID already exists.
    async fn add_node(&mut self, node: &Node) -> Result<(), StorageError>;

    /// Retrieve a node by ID
    ///
    /// Returns None if the node does not exist.
    async fn get_node(&self, id: &str) -> Result<Option<Node>, StorageError>;

    /// Update an existing node
    ///
    /// Returns an error if the node does not exist.
    async fn update_node(&mut self, node: &Node) -> Result<(), StorageError>;

    /// Delete a node by ID
    ///
    /// Cascades to delete associated edges, metadata, and tags.
    /// Returns an error if the node does not exist.
    async fn delete_node(&mut self, id: &str) -> Result<(), StorageError>;

    /// List all nodes in the graph
    ///
    /// Warning: This can be expensive for large graphs. Consider pagination.
    async fn list_nodes(&self) -> Result<Vec<Node>, StorageError>;

    /// Search nodes using full-text search
    ///
    /// Query syntax depends on the backend (FTS5 for SQLite, tsquery for PostgreSQL).
    /// Searches across name, description, and body fields.
    async fn search_nodes(&self, query: &str) -> Result<Vec<Node>, StorageError>;

    // ============================================================
    // Edge Operations
    // ============================================================

    /// Add a new edge to the graph
    ///
    /// Returns an error if an edge with the same (from, to, edge_type) already exists,
    /// or if either endpoint node does not exist.
    async fn add_edge(&mut self, edge: &Edge) -> Result<(), StorageError>;

    /// Get all outgoing edges from a node
    ///
    /// Returns empty vec if the node has no outgoing edges.
    async fn get_edges_from(&self, node_id: &str) -> Result<Vec<Edge>, StorageError>;

    /// Get all incoming edges to a node
    ///
    /// Returns empty vec if the node has no incoming edges.
    async fn get_edges_to(&self, node_id: &str) -> Result<Vec<Edge>, StorageError>;

    /// Delete a specific edge
    ///
    /// Returns an error if the edge does not exist.
    async fn delete_edge(
        &mut self,
        from: &str,
        to: &str,
        edge_type: &str,
    ) -> Result<(), StorageError>;

    /// List all edges in the graph
    ///
    /// Warning: This can be expensive for large graphs. Consider pagination.
    async fn list_edges(&self) -> Result<Vec<Edge>, StorageError>;

    // ============================================================
    // Batch Operations
    // ============================================================

    /// Add multiple nodes in a single transaction
    ///
    /// This is more efficient than calling add_node repeatedly.
    /// All nodes are added or none are (atomic operation).
    async fn add_nodes_batch(&mut self, nodes: &[Node]) -> Result<(), StorageError>;

    /// Add multiple edges in a single transaction
    ///
    /// This is more efficient than calling add_edge repeatedly.
    /// All edges are added or none are (atomic operation).
    async fn add_edges_batch(&mut self, edges: &[Edge]) -> Result<(), StorageError>;

    // ============================================================
    // Transaction Support
    // ============================================================

    /// Begin a new transaction
    ///
    /// All subsequent operations will be part of this transaction until
    /// commit_transaction or rollback_transaction is called.
    /// Nested transactions may not be supported by all backends.
    async fn begin_transaction(&mut self) -> Result<(), StorageError>;

    /// Commit the current transaction
    ///
    /// Makes all changes since begin_transaction permanent.
    async fn commit_transaction(&mut self) -> Result<(), StorageError>;

    /// Rollback the current transaction
    ///
    /// Discards all changes since begin_transaction.
    async fn rollback_transaction(&mut self) -> Result<(), StorageError>;

    // ============================================================
    // Metadata Operations
    // ============================================================

    /// Set a metadata key-value pair for a node
    ///
    /// Overwrites existing value if the key already exists.
    /// The value can be any JSON-serializable data.
    async fn set_metadata(
        &mut self,
        node_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), StorageError>;

    /// Get a metadata value by key
    ///
    /// Returns None if the key does not exist for this node.
    async fn get_metadata(
        &self,
        node_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>, StorageError>;

    /// Delete a metadata key-value pair
    ///
    /// No-op if the key does not exist.
    async fn delete_metadata(&mut self, node_id: &str, key: &str) -> Result<(), StorageError>;

    // ============================================================
    // Tag Operations
    // ============================================================

    /// Add a tag to a node
    ///
    /// No-op if the node already has this tag.
    async fn add_tag(&mut self, node_id: &str, tag: &str) -> Result<(), StorageError>;

    /// Remove a tag from a node
    ///
    /// No-op if the node does not have this tag.
    async fn remove_tag(&mut self, node_id: &str, tag: &str) -> Result<(), StorageError>;

    /// Get all tags for a node
    ///
    /// Returns empty vec if the node has no tags.
    async fn get_tags(&self, node_id: &str) -> Result<Vec<String>, StorageError>;

    /// Find all nodes with a specific tag
    ///
    /// Returns empty vec if no nodes have this tag.
    async fn find_by_tag(&self, tag: &str) -> Result<Vec<Node>, StorageError>;

    // ============================================================
    // Configuration
    // ============================================================

    /// Set a configuration key-value pair
    ///
    /// Configuration is global (not per-node).
    /// Overwrites existing value if the key already exists.
    async fn set_config(&mut self, key: &str, value: &str) -> Result<(), StorageError>;

    /// Get a configuration value by key
    ///
    /// Returns None if the key does not exist.
    async fn get_config(&self, key: &str) -> Result<Option<String>, StorageError>;

    // ============================================================
    // Change Tracking
    // ============================================================

    /// Log a change event
    ///
    /// Used for audit trail and history tracking.
    /// The node_id can be None for system-wide changes.
    async fn log_change(
        &mut self,
        node_id: &str,
        change_type: &str,
        description: &str,
    ) -> Result<(), StorageError>;

    /// Get change history for a node
    ///
    /// Returns the most recent changes, limited by the limit parameter.
    /// Changes are returned in reverse chronological order (newest first).
    async fn get_change_history(
        &self,
        node_id: &str,
        limit: usize,
    ) -> Result<Vec<ChangeEntry>, StorageError>;
}

// ============================================================
// Helper Traits for Backend-Specific Features
// ============================================================

/// Optional trait for storage backends that support advanced querying
#[async_trait]
pub trait GraphStorageQuery: GraphStorage {
    /// Execute a raw query and return nodes
    ///
    /// Query syntax is backend-specific (SQL for SQLite/PostgreSQL, etc.)
    async fn query_nodes(&self, query: &str) -> Result<Vec<Node>, StorageError>;

    /// Execute a raw query and return edges
    ///
    /// Query syntax is backend-specific (SQL for SQLite/PostgreSQL, etc.)
    async fn query_edges(&self, query: &str) -> Result<Vec<Edge>, StorageError>;
}

/// Optional trait for storage backends that support schema migrations
#[async_trait]
pub trait GraphStorageMigration: GraphStorage {
    /// Get the current schema version
    async fn get_schema_version(&self) -> Result<i32, StorageError>;

    /// Migrate to a specific schema version
    async fn migrate_to(&mut self, version: i32) -> Result<(), StorageError>;
}

/// Optional trait for storage backends that support backup/restore
#[async_trait]
pub trait GraphStorageBackup: GraphStorage {
    /// Export the entire graph to a file
    async fn export_to_file(&self, path: &str) -> Result<(), StorageError>;

    /// Import a graph from a file
    async fn import_from_file(&mut self, path: &str) -> Result<(), StorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that the trait is object-safe (can be used as dyn GraphStorage)
    #[test]
    fn test_trait_object_safety() {
        fn _assert_object_safe(_: &dyn GraphStorage) {}
    }
}
