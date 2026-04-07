use std::path::Path;
use anyhow::{Context, Result};
use crate::graph::Graph;

/// Load a graph from a YAML file.
pub fn load_graph(path: &Path) -> Result<Graph> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read graph file: {}", path.display()))?;
    let graph: Graph = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse graph YAML: {}", path.display()))?;
    Ok(graph)
}

/// Save a graph to a YAML file.
pub fn save_graph(graph: &Graph, path: &Path) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(graph)?;
    std::fs::write(path, yaml)
        .with_context(|| format!("Failed to write graph file: {}", path.display()))?;
    Ok(())
}

/// Find the graph file in a project directory.
/// Searches: .gid/graph.yml, .gid/graph.yaml, graph.yml, graph.yaml
pub fn find_graph_file(project_dir: &Path) -> Option<std::path::PathBuf> {
    let candidates = [
        project_dir.join(".gid/graph.yml"),
        project_dir.join(".gid/graph.yaml"),
        project_dir.join("graph.yml"),
        project_dir.join("graph.yaml"),
    ];
    candidates.into_iter().find(|p| p.exists())
}
