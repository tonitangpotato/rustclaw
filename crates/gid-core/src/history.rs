//! History tracking for GID graphs.
//!
//! Save snapshots with timestamps, list/diff/restore versions.

use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use crate::graph::Graph;
use crate::parser::{load_graph, save_graph};

/// Maximum number of history entries to keep.
const MAX_HISTORY_ENTRIES: usize = 50;

/// A history snapshot entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Filename of the snapshot (e.g., "2024-03-25T12-30-00Z.yml")
    pub filename: String,
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Optional commit-like message
    pub message: Option<String>,
    /// Number of nodes in this snapshot
    pub node_count: usize,
    /// Number of edges in this snapshot
    pub edge_count: usize,
    /// Git commit hash if available
    pub git_commit: Option<String>,
}

/// Diff result between two graph versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDiff {
    /// Nodes added in the newer version
    pub added_nodes: Vec<String>,
    /// Nodes removed from the older version
    pub removed_nodes: Vec<String>,
    /// Nodes that changed (status, title, etc.)
    pub modified_nodes: Vec<String>,
    /// Number of edges added
    pub added_edges: usize,
    /// Number of edges removed
    pub removed_edges: usize,
}

impl std::fmt::Display for GraphDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "No differences found.");
        }
        
        let mut lines = Vec::new();
        
        if !self.added_nodes.is_empty() {
            lines.push(format!("+ Added nodes ({}):", self.added_nodes.len()));
            for node in self.added_nodes.iter().take(10) {
                lines.push(format!("    + {}", node));
            }
            if self.added_nodes.len() > 10 {
                lines.push(format!("    ... and {} more", self.added_nodes.len() - 10));
            }
        }
        
        if !self.removed_nodes.is_empty() {
            lines.push(format!("- Removed nodes ({}):", self.removed_nodes.len()));
            for node in self.removed_nodes.iter().take(10) {
                lines.push(format!("    - {}", node));
            }
            if self.removed_nodes.len() > 10 {
                lines.push(format!("    ... and {} more", self.removed_nodes.len() - 10));
            }
        }
        
        if !self.modified_nodes.is_empty() {
            lines.push(format!("~ Modified nodes ({}):", self.modified_nodes.len()));
            for node in self.modified_nodes.iter().take(10) {
                lines.push(format!("    ~ {}", node));
            }
            if self.modified_nodes.len() > 10 {
                lines.push(format!("    ... and {} more", self.modified_nodes.len() - 10));
            }
        }
        
        if self.added_edges > 0 || self.removed_edges > 0 {
            lines.push("Edge changes:".to_string());
            if self.added_edges > 0 {
                lines.push(format!("    + {} edges added", self.added_edges));
            }
            if self.removed_edges > 0 {
                lines.push(format!("    - {} edges removed", self.removed_edges));
            }
        }
        
        write!(f, "{}", lines.join("\n"))
    }
}

impl GraphDiff {
    pub fn is_empty(&self) -> bool {
        self.added_nodes.is_empty()
            && self.removed_nodes.is_empty()
            && self.modified_nodes.is_empty()
            && self.added_edges == 0
            && self.removed_edges == 0
    }
}

/// History manager for a GID project.
pub struct HistoryManager {
    history_dir: PathBuf,
}

impl HistoryManager {
    /// Create a new history manager for the given .gid directory.
    pub fn new(gid_dir: &Path) -> Self {
        Self {
            history_dir: gid_dir.join("history"),
        }
    }
    
    /// Ensure the history directory exists.
    fn ensure_dir(&self) -> Result<()> {
        if !self.history_dir.exists() {
            fs::create_dir_all(&self.history_dir)
                .with_context(|| format!("Failed to create history directory: {}", self.history_dir.display()))?;
        }
        Ok(())
    }
    
    /// Save a snapshot of the current graph.
    pub fn save_snapshot(&self, graph: &Graph, message: Option<&str>) -> Result<String> {
        self.ensure_dir()?;
        
        let timestamp = Utc::now();
        let filename = format!("{}.yml", timestamp.format("%Y-%m-%dT%H-%M-%SZ"));
        let filepath = self.history_dir.join(&filename);
        
        // Add message as a comment at the top if provided
        let yaml = if let Some(msg) = message {
            format!("# {}\n{}", msg, serde_yaml::to_string(graph)?)
        } else {
            serde_yaml::to_string(graph)?
        };
        
        fs::write(&filepath, yaml)
            .with_context(|| format!("Failed to save snapshot: {}", filepath.display()))?;
        
        // Clean up old history entries
        self.cleanup()?;
        
        Ok(filename)
    }
    
    /// List all history snapshots.
    pub fn list_snapshots(&self) -> Result<Vec<HistoryEntry>> {
        if !self.history_dir.exists() {
            return Ok(Vec::new());
        }
        
        let mut entries = Vec::new();
        
        let mut files: Vec<_> = fs::read_dir(&self.history_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map_or(false, |ext| ext == "yml" || ext == "yaml")
            })
            .collect();
        
        // Sort by filename (which includes timestamp) in descending order
        files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        
        for entry in files {
            let filepath = entry.path();
            let filename = entry.file_name().to_string_lossy().to_string();
            
            // Extract timestamp from filename
            let timestamp = filename
                .trim_end_matches(".yml")
                .trim_end_matches(".yaml")
                .replace('T', " ")
                .replace('-', ":");
            
            // Try to load the graph to get stats
            if let Ok(content) = fs::read_to_string(&filepath) {
                // Extract message from first line if it's a comment
                let message = content.lines().next()
                    .filter(|l| l.starts_with("# "))
                    .map(|l| l[2..].to_string());
                
                // Parse the graph
                if let Ok(graph) = serde_yaml::from_str::<Graph>(&content) {
                    entries.push(HistoryEntry {
                        filename,
                        timestamp,
                        message,
                        node_count: graph.nodes.len(),
                        edge_count: graph.edges.len(),
                        git_commit: None, // TODO: Extract from metadata
                    });
                }
            }
        }
        
        Ok(entries)
    }
    
    /// Load a historical version by filename.
    pub fn load_version(&self, filename: &str) -> Result<Graph> {
        let filepath = self.history_dir.join(filename);
        
        if !filepath.exists() {
            bail!("History version not found: {}", filename);
        }
        
        load_graph(&filepath)
    }
    
    /// Compute diff between two graphs.
    pub fn diff(older: &Graph, newer: &Graph) -> GraphDiff {
        use std::collections::{HashMap, HashSet};
        
        let old_nodes: HashSet<&str> = older.nodes.iter().map(|n| n.id.as_str()).collect();
        let new_nodes: HashSet<&str> = newer.nodes.iter().map(|n| n.id.as_str()).collect();
        
        let added_nodes: Vec<String> = new_nodes.difference(&old_nodes)
            .map(|s| s.to_string())
            .collect();
        
        let removed_nodes: Vec<String> = old_nodes.difference(&new_nodes)
            .map(|s| s.to_string())
            .collect();
        
        // Find modified nodes (same ID but different content)
        let old_node_map: HashMap<&str, &crate::graph::Node> = 
            older.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        let new_node_map: HashMap<&str, &crate::graph::Node> = 
            newer.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        
        let mut modified_nodes = Vec::new();
        for id in old_nodes.intersection(&new_nodes) {
            if let (Some(old), Some(new)) = (old_node_map.get(id), new_node_map.get(id)) {
                if old.status != new.status || old.title != new.title || old.description != new.description {
                    modified_nodes.push(id.to_string());
                }
            }
        }
        
        // Edge comparison
        let old_edges: HashSet<(&str, &str, &str)> = older.edges.iter()
            .map(|e| (e.from.as_str(), e.to.as_str(), e.relation.as_str()))
            .collect();
        let new_edges: HashSet<(&str, &str, &str)> = newer.edges.iter()
            .map(|e| (e.from.as_str(), e.to.as_str(), e.relation.as_str()))
            .collect();
        
        let added_edges = new_edges.difference(&old_edges).count();
        let removed_edges = old_edges.difference(&new_edges).count();
        
        GraphDiff {
            added_nodes,
            removed_nodes,
            modified_nodes,
            added_edges,
            removed_edges,
        }
    }
    
    /// Diff current graph against a historical version.
    pub fn diff_against(&self, version: &str, current: &Graph) -> Result<GraphDiff> {
        let historical = self.load_version(version)?;
        Ok(Self::diff(&historical, current))
    }
    
    /// Restore a historical version to the main graph file.
    pub fn restore(&self, version: &str, graph_path: &Path) -> Result<()> {
        let historical = self.load_version(version)?;
        
        // Save current state to history first
        if graph_path.exists() {
            if let Ok(current) = load_graph(graph_path) {
                self.save_snapshot(&current, Some("Auto-snapshot before restore"))?;
            }
        }
        
        // Write the historical version as the current graph
        save_graph(&historical, graph_path)?;
        
        Ok(())
    }
    
    /// Clean up old history entries, keeping only the most recent N.
    fn cleanup(&self) -> Result<()> {
        let mut files: Vec<_> = fs::read_dir(&self.history_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().map_or(false, |ext| ext == "yml" || ext == "yaml")
            })
            .collect();
        
        // Sort by filename in ascending order (oldest first)
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        
        // Remove oldest files if we have too many
        while files.len() > MAX_HISTORY_ENTRIES {
            if let Some(oldest) = files.first() {
                fs::remove_file(oldest.path()).ok();
                files.remove(0);
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Node;
    use tempfile::TempDir;
    
    #[test]
    fn test_diff_empty_graphs() {
        let g1 = Graph::new();
        let g2 = Graph::new();
        let diff = HistoryManager::diff(&g1, &g2);
        assert!(diff.is_empty());
    }
    
    #[test]
    fn test_diff_added_nodes() {
        let g1 = Graph::new();
        let mut g2 = Graph::new();
        g2.add_node(Node::new("a", "Node A"));
        
        let diff = HistoryManager::diff(&g1, &g2);
        assert_eq!(diff.added_nodes, vec!["a"]);
        assert!(diff.removed_nodes.is_empty());
    }
    
    #[test]
    fn test_save_and_load_snapshot() {
        let temp = TempDir::new().unwrap();
        let gid_dir = temp.path().join(".gid");
        fs::create_dir_all(&gid_dir).unwrap();
        
        let mgr = HistoryManager::new(&gid_dir);
        
        let mut graph = Graph::new();
        graph.add_node(Node::new("test", "Test Node"));
        
        let filename = mgr.save_snapshot(&graph, Some("Test snapshot")).unwrap();
        
        let loaded = mgr.load_version(&filename).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].id, "test");
    }
}
