# gid_storage - SQLite Storage Foundation for gid-rs

This module contains the Layer 1 foundation implementation for SQLite-based persistent storage in gid-rs.

## Overview

The `gid_storage` module provides:

1. **StorageError** - Comprehensive error types with operation context
2. **StorageOp** - Enum of all storage operation types
3. **Extended Node/Edge Types** - Structures with 14+ new metadata fields
4. **GraphStorage Trait** - Complete interface for storage backends
5. **SQLite Schema** - DDL for tables, indexes, FTS5, and triggers

## Module Structure

```
src/gid_storage/
├── mod.rs              # Module exports
├── error.rs            # StorageError and StorageOp
├── types.rs            # Node, Edge, ChangeEntry
├── graph_storage.rs    # GraphStorage trait
└── schema.rs           # SQLite DDL
```

## Usage Example

```rust
use gid_storage::{Node, Edge, GraphStorage, StorageError};

// Create a node with extended metadata
let node = Node::new("fn:parse", "parse")
    .with_file_path("src/parser.rs")
    .with_lang("rust")
    .with_node_type("function")
    .with_line_range(10, 50)
    .with_complexity(8)
    .with_is_public(true);

// Create an edge with metadata
let edge = Edge::new("fn:main", "fn:parse", "calls")
    .with_metadata(serde_json::json!({
        "call_type": "direct",
        "args_count": 2
    }));

// Use any GraphStorage implementation
async fn example<S: GraphStorage>(storage: &mut S) -> Result<(), StorageError> {
    storage.initialize().await?;
    storage.add_node(&node).await?;
    storage.add_edge(&edge).await?;
    
    let found = storage.search_nodes("parse").await?;
    println!("Found {} nodes", found.len());
    
    Ok(())
}
```

## Node Extensions

The `Node` struct includes 14 new optional fields:

- `file_path` - Source file path
- `lang` - Programming language
- `signature` - Function/class signature
- `node_type` - Type classification
- `description` - Human-readable description
- `priority` - Task priority (1-100)
- `assigned_to` - Task assignee
- `line_start` / `line_end` - Source location
- `body` - Full content
- `parent_id` - Hierarchical parent
- `depth` - Hierarchy depth
- `complexity` - Complexity metric
- `is_public` - Visibility flag

## Edge Extensions

The `Edge` struct includes:

- `metadata` - Arbitrary JSON metadata

## GraphStorage Trait

The trait defines 30+ async methods organized into categories:

- **Lifecycle**: initialize, close
- **Nodes**: add, get, update, delete, list, search
- **Edges**: add, get (from/to), delete, list
- **Batch**: add_nodes_batch, add_edges_batch
- **Transactions**: begin, commit, rollback
- **Metadata**: set, get, delete
- **Tags**: add, remove, get, find_by_tag
- **Config**: set, get
- **Change Tracking**: log_change, get_change_history

## SQLite Schema

The schema includes:

### Core Tables
- `nodes` - Graph nodes with 14 extended fields
- `edges` - Directed edges with metadata
- `node_metadata` - Flexible key-value store
- `node_tags` - Node categorization
- `knowledge` - User-added context
- `config` - System configuration
- `change_log` - Audit trail

### Performance
- 13+ indexes on frequently queried columns
- FTS5 virtual table for full-text search
- Automatic triggers to keep FTS in sync
- Foreign key constraints with CASCADE

## Error Handling

All errors include operation context:

```rust
match storage.add_node(&node).await {
    Err(StorageError::Database { op, message }) => {
        eprintln!("Database error during {}: {}", op, message);
    }
    Err(StorageError::AlreadyExists { op, message }) => {
        eprintln!("Node already exists during {}: {}", op, message);
    }
    Ok(()) => println!("Node added successfully"),
    _ => {}
}
```

## Next Steps (Layer 2+)

This Layer 1 foundation enables:

1. **Layer 2**: SQLite backend implementation
   - Implement GraphStorage trait with sqlx
   - Connection pooling
   - Prepared statements
   - Transaction management

2. **Layer 3**: Integration & testing
   - Unit tests for each operation
   - Integration tests with real SQLite
   - Benchmarks for large graphs

3. **Future**: Advanced features
   - PostgreSQL backend
   - Graph versioning
   - Real-time subscriptions
   - Collaborative editing

## Design Documents

See `.gid/features/sqlite-migration/design-storage.md` for detailed specifications.

## Testing

Run tests with:

```bash
cargo test --lib gid_storage
```

## License

Same as parent project (MIT).
