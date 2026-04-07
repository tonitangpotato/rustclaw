/// Error types for storage operations
use thiserror::Error;

/// Storage operation types for error context
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOp {
    Initialize,
    Close,
    AddNode,
    GetNode,
    UpdateNode,
    DeleteNode,
    ListNodes,
    SearchNodes,
    AddEdge,
    GetEdgesFrom,
    GetEdgesTo,
    DeleteEdge,
    ListEdges,
    AddNodesBatch,
    AddEdgesBatch,
    BeginTransaction,
    CommitTransaction,
    RollbackTransaction,
    SetMetadata,
    GetMetadata,
    DeleteMetadata,
    AddTag,
    RemoveTag,
    GetTags,
    FindByTag,
    SetConfig,
    GetConfig,
    LogChange,
    GetChangeHistory,
}

impl std::fmt::Display for StorageOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StorageOp::Initialize => "initialize",
            StorageOp::Close => "close",
            StorageOp::AddNode => "add_node",
            StorageOp::GetNode => "get_node",
            StorageOp::UpdateNode => "update_node",
            StorageOp::DeleteNode => "delete_node",
            StorageOp::ListNodes => "list_nodes",
            StorageOp::SearchNodes => "search_nodes",
            StorageOp::AddEdge => "add_edge",
            StorageOp::GetEdgesFrom => "get_edges_from",
            StorageOp::GetEdgesTo => "get_edges_to",
            StorageOp::DeleteEdge => "delete_edge",
            StorageOp::ListEdges => "list_edges",
            StorageOp::AddNodesBatch => "add_nodes_batch",
            StorageOp::AddEdgesBatch => "add_edges_batch",
            StorageOp::BeginTransaction => "begin_transaction",
            StorageOp::CommitTransaction => "commit_transaction",
            StorageOp::RollbackTransaction => "rollback_transaction",
            StorageOp::SetMetadata => "set_metadata",
            StorageOp::GetMetadata => "get_metadata",
            StorageOp::DeleteMetadata => "delete_metadata",
            StorageOp::AddTag => "add_tag",
            StorageOp::RemoveTag => "remove_tag",
            StorageOp::GetTags => "get_tags",
            StorageOp::FindByTag => "find_by_tag",
            StorageOp::SetConfig => "set_config",
            StorageOp::GetConfig => "get_config",
            StorageOp::LogChange => "log_change",
            StorageOp::GetChangeHistory => "get_change_history",
        };
        write!(f, "{}", s)
    }
}

/// Errors that can occur during storage operations
#[derive(Debug, Error)]
pub enum StorageError {
    /// Database-level error (connection, query execution, etc.)
    #[error("Database error during {op}: {message}")]
    Database { op: StorageOp, message: String },

    /// Requested node was not found
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    /// Requested edge was not found
    #[error("Edge not found: from={from} to={to} type={edge_type}")]
    EdgeNotFound {
        from: String,
        to: String,
        edge_type: String,
    },

    /// Serialization/deserialization error (JSON, etc.)
    #[error("Serialization error during {op}: {message}")]
    Serialization { op: StorageOp, message: String },

    /// Transaction-related error
    #[error("Transaction error during {op}: {message}")]
    Transaction { op: StorageOp, message: String },

    /// IO error (file access, etc.)
    #[error("IO error during {op}: {source}")]
    Io {
        op: StorageOp,
        #[source]
        source: std::io::Error,
    },

    /// Invalid query syntax or parameters
    #[error("Invalid query during {op}: {message}")]
    InvalidQuery { op: StorageOp, message: String },

    /// Constraint violation (foreign key, unique, etc.)
    #[error("Constraint violation during {op}: {message}")]
    ConstraintViolation { op: StorageOp, message: String },

    /// Resource already exists (duplicate insert, etc.)
    #[error("Resource already exists during {op}: {message}")]
    AlreadyExists { op: StorageOp, message: String },

    /// Generic error with context
    #[error("Storage error during {op}: {message}")]
    Other { op: StorageOp, message: String },
}

impl StorageError {
    /// Create a database error with operation context
    pub fn database(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::Database {
            op,
            message: message.into(),
        }
    }

    /// Create a serialization error with operation context
    pub fn serialization(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::Serialization {
            op,
            message: message.into(),
        }
    }

    /// Create a transaction error with operation context
    pub fn transaction(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::Transaction {
            op,
            message: message.into(),
        }
    }

    /// Create an IO error with operation context
    pub fn io(op: StorageOp, source: std::io::Error) -> Self {
        StorageError::Io { op, source }
    }

    /// Create an invalid query error with operation context
    pub fn invalid_query(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::InvalidQuery {
            op,
            message: message.into(),
        }
    }

    /// Create a constraint violation error with operation context
    pub fn constraint_violation(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::ConstraintViolation {
            op,
            message: message.into(),
        }
    }

    /// Create an already exists error with operation context
    pub fn already_exists(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::AlreadyExists {
            op,
            message: message.into(),
        }
    }

    /// Create a generic error with operation context
    pub fn other(op: StorageOp, message: impl Into<String>) -> Self {
        StorageError::Other {
            op,
            message: message.into(),
        }
    }
}
