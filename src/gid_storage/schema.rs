/// SQLite schema definition for graph storage
///
/// This module contains the DDL (Data Definition Language) statements
/// for creating tables, indexes, triggers, and FTS5 virtual tables.

/// Complete SQLite schema initialization SQL
pub const SCHEMA_SQL: &str = r#"
-- ============================================================
-- Core Tables
-- ============================================================

-- Nodes table (primary graph nodes)
CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    
    -- Extended metadata fields
    file_path TEXT,
    lang TEXT,
    signature TEXT,
    node_type TEXT,
    description TEXT,
    priority INTEGER,
    assigned_to TEXT,
    line_start INTEGER,
    line_end INTEGER,
    body TEXT,
    parent_id TEXT,
    depth INTEGER DEFAULT 0,
    complexity INTEGER,
    is_public BOOLEAN DEFAULT 0,
    
    -- Timestamps
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    
    -- Foreign key constraint
    FOREIGN KEY (parent_id) REFERENCES nodes(id) ON DELETE SET NULL
);

-- Edges table (relationships between nodes)
CREATE TABLE IF NOT EXISTS edges (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    metadata TEXT, -- JSON
    created_at INTEGER NOT NULL,
    
    PRIMARY KEY (from_id, to_id, edge_type),
    FOREIGN KEY (from_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (to_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Node metadata (flexible key-value store)
CREATE TABLE IF NOT EXISTS node_metadata (
    node_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL, -- JSON
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    
    PRIMARY KEY (node_id, key),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Node tags (categorization)
CREATE TABLE IF NOT EXISTS node_tags (
    node_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    
    PRIMARY KEY (node_id, tag),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Knowledge base (user-added context)
CREATE TABLE IF NOT EXISTS knowledge (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    tags TEXT, -- JSON array
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Configuration (system settings)
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Change log (audit trail)
CREATE TABLE IF NOT EXISTS change_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT,
    change_type TEXT NOT NULL, -- 'create', 'update', 'delete'
    description TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE SET NULL
);

-- ============================================================
-- Indexes for Performance
-- ============================================================

-- Node lookup optimization
CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
CREATE INDEX IF NOT EXISTS idx_nodes_file_path ON nodes(file_path);
CREATE INDEX IF NOT EXISTS idx_nodes_node_type ON nodes(node_type);
CREATE INDEX IF NOT EXISTS idx_nodes_parent_id ON nodes(parent_id);
CREATE INDEX IF NOT EXISTS idx_nodes_priority ON nodes(priority);
CREATE INDEX IF NOT EXISTS idx_nodes_assigned_to ON nodes(assigned_to);
CREATE INDEX IF NOT EXISTS idx_nodes_lang ON nodes(lang);

-- Edge lookup optimization
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);

-- Tag lookup optimization
CREATE INDEX IF NOT EXISTS idx_node_tags_tag ON node_tags(tag);

-- Metadata lookup optimization
CREATE INDEX IF NOT EXISTS idx_node_metadata_key ON node_metadata(key);

-- Change log optimization
CREATE INDEX IF NOT EXISTS idx_change_log_node ON change_log(node_id);
CREATE INDEX IF NOT EXISTS idx_change_log_time ON change_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_change_log_type ON change_log(change_type);

-- ============================================================
-- Full-Text Search (FTS5)
-- ============================================================

-- FTS5 virtual table for node content search
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
    id UNINDEXED,
    name,
    description,
    body,
    content=nodes,
    content_rowid=rowid
);

-- Triggers to keep FTS in sync with nodes table

-- Insert trigger
CREATE TRIGGER IF NOT EXISTS nodes_fts_insert AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, id, name, description, body)
    VALUES (new.rowid, new.id, new.name, new.description, new.body);
END;

-- Delete trigger
CREATE TRIGGER IF NOT EXISTS nodes_fts_delete AFTER DELETE ON nodes BEGIN
    DELETE FROM nodes_fts WHERE rowid = old.rowid;
END;

-- Update trigger
CREATE TRIGGER IF NOT EXISTS nodes_fts_update AFTER UPDATE ON nodes BEGIN
    UPDATE nodes_fts SET
        name = new.name,
        description = new.description,
        body = new.body
    WHERE rowid = new.rowid;
END;

-- ============================================================
-- Schema Version Tracking
-- ============================================================

-- Insert initial schema version if not exists
INSERT OR IGNORE INTO config (key, value, updated_at) 
VALUES ('schema_version', '1', strftime('%s', 'now'));
"#;

/// Schema version for migration tracking
pub const SCHEMA_VERSION: i32 = 1;

/// Individual table creation statements (for reference/testing)
pub mod tables {
    pub const NODES: &str = r#"
CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    file_path TEXT,
    lang TEXT,
    signature TEXT,
    node_type TEXT,
    description TEXT,
    priority INTEGER,
    assigned_to TEXT,
    line_start INTEGER,
    line_end INTEGER,
    body TEXT,
    parent_id TEXT,
    depth INTEGER DEFAULT 0,
    complexity INTEGER,
    is_public BOOLEAN DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (parent_id) REFERENCES nodes(id) ON DELETE SET NULL
)
"#;

    pub const EDGES: &str = r#"
CREATE TABLE IF NOT EXISTS edges (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    metadata TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (from_id, to_id, edge_type),
    FOREIGN KEY (from_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (to_id) REFERENCES nodes(id) ON DELETE CASCADE
)
"#;

    pub const NODE_METADATA: &str = r#"
CREATE TABLE IF NOT EXISTS node_metadata (
    node_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, key),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
)
"#;

    pub const NODE_TAGS: &str = r#"
CREATE TABLE IF NOT EXISTS node_tags (
    node_id TEXT NOT NULL,
    tag TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (node_id, tag),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
)
"#;

    pub const KNOWLEDGE: &str = r#"
CREATE TABLE IF NOT EXISTS knowledge (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    tags TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)
"#;

    pub const CONFIG: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
)
"#;

    pub const CHANGE_LOG: &str = r#"
CREATE TABLE IF NOT EXISTS change_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT,
    change_type TEXT NOT NULL,
    description TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE SET NULL
)
"#;
}

/// Index creation statements
pub mod indexes {
    pub const NODES_NAME: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name)";
    pub const NODES_FILE_PATH: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_file_path ON nodes(file_path)";
    pub const NODES_NODE_TYPE: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_node_type ON nodes(node_type)";
    pub const NODES_PARENT_ID: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_parent_id ON nodes(parent_id)";
    pub const NODES_PRIORITY: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_priority ON nodes(priority)";
    pub const NODES_ASSIGNED_TO: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_assigned_to ON nodes(assigned_to)";
    pub const NODES_LANG: &str = "CREATE INDEX IF NOT EXISTS idx_nodes_lang ON nodes(lang)";
    
    pub const EDGES_FROM: &str = "CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id)";
    pub const EDGES_TO: &str = "CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id)";
    pub const EDGES_TYPE: &str = "CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type)";
    
    pub const NODE_TAGS_TAG: &str = "CREATE INDEX IF NOT EXISTS idx_node_tags_tag ON node_tags(tag)";
    pub const NODE_METADATA_KEY: &str = "CREATE INDEX IF NOT EXISTS idx_node_metadata_key ON node_metadata(key)";
    
    pub const CHANGE_LOG_NODE: &str = "CREATE INDEX IF NOT EXISTS idx_change_log_node ON change_log(node_id)";
    pub const CHANGE_LOG_TIME: &str = "CREATE INDEX IF NOT EXISTS idx_change_log_time ON change_log(timestamp)";
    pub const CHANGE_LOG_TYPE: &str = "CREATE INDEX IF NOT EXISTS idx_change_log_type ON change_log(change_type)";
}

/// FTS5 virtual table and triggers
pub mod fts {
    pub const CREATE_VIRTUAL_TABLE: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
    id UNINDEXED,
    name,
    description,
    body,
    content=nodes,
    content_rowid=rowid
)
"#;

    pub const TRIGGER_INSERT: &str = r#"
CREATE TRIGGER IF NOT EXISTS nodes_fts_insert AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, id, name, description, body)
    VALUES (new.rowid, new.id, new.name, new.description, new.body);
END
"#;

    pub const TRIGGER_DELETE: &str = r#"
CREATE TRIGGER IF NOT EXISTS nodes_fts_delete AFTER DELETE ON nodes BEGIN
    DELETE FROM nodes_fts WHERE rowid = old.rowid;
END
"#;

    pub const TRIGGER_UPDATE: &str = r#"
CREATE TRIGGER IF NOT EXISTS nodes_fts_update AFTER UPDATE ON nodes BEGIN
    UPDATE nodes_fts SET
        name = new.name,
        description = new.description,
        body = new.body
    WHERE rowid = new.rowid;
END
"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_sql_not_empty() {
        assert!(!SCHEMA_SQL.is_empty());
        assert!(SCHEMA_SQL.contains("CREATE TABLE"));
        assert!(SCHEMA_SQL.contains("nodes"));
        assert!(SCHEMA_SQL.contains("edges"));
    }

    #[test]
    fn test_schema_version() {
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn test_all_tables_defined() {
        assert!(SCHEMA_SQL.contains("nodes"));
        assert!(SCHEMA_SQL.contains("edges"));
        assert!(SCHEMA_SQL.contains("node_metadata"));
        assert!(SCHEMA_SQL.contains("node_tags"));
        assert!(SCHEMA_SQL.contains("knowledge"));
        assert!(SCHEMA_SQL.contains("config"));
        assert!(SCHEMA_SQL.contains("change_log"));
    }

    #[test]
    fn test_fts5_defined() {
        assert!(SCHEMA_SQL.contains("nodes_fts"));
        assert!(SCHEMA_SQL.contains("fts5"));
        assert!(SCHEMA_SQL.contains("nodes_fts_insert"));
        assert!(SCHEMA_SQL.contains("nodes_fts_update"));
        assert!(SCHEMA_SQL.contains("nodes_fts_delete"));
    }

    #[test]
    fn test_indexes_defined() {
        assert!(SCHEMA_SQL.contains("idx_nodes_name"));
        assert!(SCHEMA_SQL.contains("idx_edges_from"));
        assert!(SCHEMA_SQL.contains("idx_node_tags_tag"));
    }
}
