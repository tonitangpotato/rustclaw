// Module for gid-rs SQLite storage foundation (Layer 1)
// This implementation can be extracted and contributed to gid-rs

pub mod error;
pub mod graph_storage;
pub mod schema;
pub mod types;

pub use error::{StorageError, StorageOp};
pub use graph_storage::GraphStorage;
pub use types::{ChangeEntry, Edge, Node};
