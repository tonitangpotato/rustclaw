//! Semantify module for upgrading file-level graphs to semantic graphs.
//!
//! Generates prompts and parses LLM responses. Does NOT call LLM directly.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use crate::graph::{Graph, Node, Edge};
use std::collections::HashMap;

/// A proposed semantic enhancement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SemanticProposal {
    /// Assign a layer to a node
    AssignLayer {
        node_id: String,
        layer: String,
        reason: String,
        #[serde(default)]
        confidence: f32,
    },
    /// Upgrade a file node to a component
    UpgradeToComponent {
        node_id: String,
        component_name: String,
        description: String,
        #[serde(default)]
        confidence: f32,
    },
    /// Add a feature node
    AddFeature {
        name: String,
        description: String,
        implementing_nodes: Vec<String>,
        #[serde(default)]
        confidence: f32,
    },
    /// Add description to a node
    AddDescription {
        node_id: String,
        description: String,
        #[serde(default)]
        confidence: f32,
    },
    /// Group nodes into a module
    GroupIntoModule {
        module_name: String,
        node_ids: Vec<String>,
        #[serde(default)]
        confidence: f32,
    },
}

/// Result from parsing LLM semantify response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemantifyResult {
    pub proposals: Vec<SemanticProposal>,
    /// Optional: the full transformed graph
    pub graph: Option<Graph>,
}

/// Generate a prompt to upgrade a file-level graph to a semantic graph.
pub fn generate_semantify_prompt(graph: &Graph) -> String {
    // Build context from graph
    let node_summary = build_node_summary(graph);
    let edge_summary = build_edge_summary(graph);
    
    format!(r#"You are a software architect. Analyze this code graph and suggest semantic enhancements.

CURRENT GRAPH:

Nodes ({} total):
{}

Edges ({} total):
{}

TASK:
1. Assign architectural layers to nodes (interface, application, domain, infrastructure)
2. Identify features that nodes implement
3. Add meaningful descriptions to important nodes
4. Group related files into logical components

LAYER DEFINITIONS:
- interface: User-facing (CLI commands, API routes, UI components, handlers)
- application: Use cases, services, orchestration
- domain: Core business logic, types, entities
- infrastructure: External integrations (DB, filesystem, parsers, adapters)

Respond with a JSON object:
```json
{{
  "proposals": [
    {{
      "type": "assign_layer",
      "node_id": "src/commands/init.ts",
      "layer": "interface",
      "reason": "CLI command handler",
      "confidence": 0.9
    }},
    {{
      "type": "add_feature",
      "name": "graph_visualization",
      "description": "Visualize the graph in various formats",
      "implementing_nodes": ["src/visual.ts", "src/render.ts"],
      "confidence": 0.85
    }},
    {{
      "type": "add_description",
      "node_id": "src/core/query.ts",
      "description": "Graph traversal and query engine",
      "confidence": 0.8
    }}
  ]
}}
```

Only output valid JSON. No explanation before or after."#,
        graph.nodes.len(),
        node_summary,
        graph.edges.len(),
        edge_summary
    )
}

/// Generate a prompt for full graph transformation.
pub fn generate_full_transform_prompt(graph: &Graph) -> String {
    let yaml = serde_yaml::to_string(graph).unwrap_or_default();
    
    format!(r#"You are a software architect. Transform this file-level graph into a semantic architecture graph.

CURRENT GRAPH (YAML):
```yaml
{}
```

Transform the graph by:
1. Adding a `layer` field to each node (interface, application, domain, infrastructure)
2. Adding meaningful `description` fields
3. Creating Feature nodes for logical feature groupings
4. Adding `implements` edges from components to features
5. Keeping all existing `depends_on` edges

Output the complete transformed graph as YAML:
```yaml
project:
  name: project-name
  description: Semantic architecture graph

nodes:
  - id: feature_visualization
    title: Graph Visualization
    type: feature
    description: Visualize graphs in multiple formats
    
  - id: src/visual.ts
    title: Visual Renderer
    type: component
    layer: interface
    description: Renders graph in ASCII, DOT, and Mermaid formats
    
edges:
  - from: src/visual.ts
    to: feature_visualization
    relation: implements
    
  - from: src/visual.ts
    to: src/core/graph.ts
    relation: depends_on
```

Only output valid YAML. Start with "```yaml" and end with "```"."#, yaml)
}

/// Parse an LLM response containing semantic proposals.
pub fn parse_semantify_response(response: &str) -> Result<SemantifyResult> {
    let json_str = extract_json(response)?;
    
    #[derive(Deserialize)]
    struct ProposalsResponse {
        proposals: Vec<SemanticProposal>,
    }
    
    let parsed: ProposalsResponse = serde_json::from_str(&json_str)
        .context("Failed to parse proposals JSON")?;
    
    Ok(SemantifyResult {
        proposals: parsed.proposals,
        graph: None,
    })
}

/// Parse an LLM response containing a full transformed graph.
pub fn parse_full_transform_response(response: &str) -> Result<Graph> {
    let yaml_str = extract_yaml(response)?;
    
    let graph: Graph = serde_yaml::from_str(&yaml_str)
        .context("Failed to parse graph YAML")?;
    
    Ok(graph)
}

/// Apply semantic proposals to a graph.
pub fn apply_proposals(graph: &mut Graph, proposals: &[SemanticProposal]) -> usize {
    let mut applied_count = 0;
    
    for proposal in proposals {
        match proposal {
            SemanticProposal::AssignLayer { node_id, layer, .. } => {
                if let Some(node) = graph.get_node_mut(node_id) {
                    node.metadata.insert("layer".to_string(), serde_json::json!(layer));
                    applied_count += 1;
                }
            }
            
            SemanticProposal::UpgradeToComponent { node_id, component_name, description, .. } => {
                if let Some(node) = graph.get_node_mut(node_id) {
                    node.node_type = Some("component".to_string());
                    node.title = component_name.clone();
                    node.description = Some(description.clone());
                    applied_count += 1;
                }
            }
            
            SemanticProposal::AddFeature { name, description, implementing_nodes, .. } => {
                // Create feature node
                let feature_id = format!("feature_{}", name.to_lowercase().replace(' ', "_"));
                let mut feature_node = Node::new(&feature_id, name);
                feature_node.node_type = Some("feature".to_string());
                feature_node.description = Some(description.clone());
                graph.add_node(feature_node);
                applied_count += 1;
                
                // Add implements edges
                for impl_node in implementing_nodes {
                    if graph.get_node(impl_node).is_some() {
                        graph.add_edge(Edge::new(impl_node, &feature_id, "implements"));
                        applied_count += 1;
                    }
                }
            }
            
            SemanticProposal::AddDescription { node_id, description, .. } => {
                if let Some(node) = graph.get_node_mut(node_id) {
                    if node.description.is_none() {
                        node.description = Some(description.clone());
                        applied_count += 1;
                    }
                }
            }
            
            SemanticProposal::GroupIntoModule { module_name, node_ids, .. } => {
                // Create module node
                let module_id = format!("module_{}", module_name.to_lowercase().replace(' ', "_"));
                let mut module_node = Node::new(&module_id, module_name);
                module_node.node_type = Some("module".to_string());
                graph.add_node(module_node);
                applied_count += 1;
                
                // Add contains edges
                for node_id in node_ids {
                    if graph.get_node(node_id).is_some() {
                        graph.add_edge(Edge::new(&module_id, node_id, "contains"));
                        applied_count += 1;
                    }
                }
            }
        }
    }
    
    applied_count
}

/// Build a summary of nodes for the prompt.
fn build_node_summary(graph: &Graph) -> String {
    let mut lines = Vec::new();
    
    for node in &graph.nodes {
        let node_type = node.node_type.as_deref().unwrap_or("unknown");
        let desc = node.description.as_deref().unwrap_or("");
        let layer = node.metadata.get("layer")
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        
        lines.push(format!(
            "  - {} (type: {}, layer: {}) {}",
            node.id,
            node_type,
            layer,
            if desc.is_empty() { String::new() } else { format!("// {}", desc) }
        ));
    }
    
    lines.join("\n")
}

/// Build a summary of edges for the prompt.
fn build_edge_summary(graph: &Graph) -> String {
    // Group edges by relation
    let mut by_relation: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    
    for edge in &graph.edges {
        by_relation.entry(&edge.relation)
            .or_default()
            .push((&edge.from, &edge.to));
    }
    
    let mut lines = Vec::new();
    
    for (relation, edges) in &by_relation {
        lines.push(format!("  {} edges ({}):", relation, edges.len()));
        for (from, to) in edges.iter().take(10) {
            lines.push(format!("    {} -> {}", from, to));
        }
        if edges.len() > 10 {
            lines.push(format!("    ... and {} more", edges.len() - 10));
        }
    }
    
    lines.join("\n")
}

/// Extract JSON from response with markdown code blocks.
fn extract_json(response: &str) -> Result<String> {
    // Try to find JSON in code block
    if let Some(start) = response.find("```json") {
        let content = &response[start + 7..];
        if let Some(end) = content.find("```") {
            return Ok(content[..end].trim().to_string());
        }
    }
    
    // Try plain code block
    if let Some(start) = response.find("```") {
        let content = &response[start + 3..];
        if let Some(end) = content.find("```") {
            let inner = content[..end].trim();
            if let Some(newline) = inner.find('\n') {
                let first_line = &inner[..newline];
                if !first_line.starts_with('{') && !first_line.starts_with('[') {
                    return Ok(inner[newline..].trim().to_string());
                }
            }
            return Ok(inner.to_string());
        }
    }
    
    // Try raw JSON
    let trimmed = response.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Ok(trimmed.to_string());
    }
    
    bail!("No JSON found in response")
}

/// Extract YAML from response.
fn extract_yaml(response: &str) -> Result<String> {
    if let Some(start) = response.find("```yaml") {
        let content = &response[start + 7..];
        if let Some(end) = content.find("```") {
            return Ok(content[..end].trim().to_string());
        }
    }
    
    if let Some(start) = response.find("```yml") {
        let content = &response[start + 6..];
        if let Some(end) = content.find("```") {
            return Ok(content[..end].trim().to_string());
        }
    }
    
    // Assume raw YAML
    let trimmed = response.trim();
    if trimmed.contains(':') {
        return Ok(trimmed.to_string());
    }
    
    bail!("No YAML found in response")
}

/// Heuristic layer assignment based on file paths.
pub fn heuristic_assign_layer(file_path: &str) -> Option<&'static str> {
    let path_lower = file_path.to_lowercase();
    
    // Interface layer patterns
    if path_lower.contains("/commands/") 
        || path_lower.contains("/cmd/")
        || path_lower.contains("/api/")
        || path_lower.contains("/routes/")
        || path_lower.contains("/controllers/")
        || path_lower.contains("/handlers/")
        || path_lower.contains("/web/")
        || path_lower.contains("/ui/")
        || path_lower.contains("/views/")
        || path_lower.contains("/pages/")
        || path_lower.contains("/components/")
    {
        return Some("interface");
    }
    
    // Application layer patterns
    if path_lower.contains("/services/")
        || path_lower.contains("/usecases/")
        || path_lower.contains("/use_cases/")
        || path_lower.contains("/orchestrators/")
        || path_lower.contains("/workflows/")
        || path_lower.contains("/ai/")
        || path_lower.contains("/llm/")
    {
        return Some("application");
    }
    
    // Domain layer patterns
    if path_lower.contains("/core/")
        || path_lower.contains("/domain/")
        || path_lower.contains("/entities/")
        || path_lower.contains("/models/")
        || path_lower.contains("/types/")
        || path_lower.contains("/lib/")
        || path_lower.ends_with("types.ts")
        || path_lower.ends_with("types.rs")
    {
        return Some("domain");
    }
    
    // Infrastructure layer patterns
    if path_lower.contains("/infrastructure/")
        || path_lower.contains("/db/")
        || path_lower.contains("/database/")
        || path_lower.contains("/repositories/")
        || path_lower.contains("/adapters/")
        || path_lower.contains("/clients/")
        || path_lower.contains("/extractors/")
        || path_lower.contains("/parsers/")
        || path_lower.contains("/config/")
    {
        return Some("infrastructure");
    }
    
    None
}

/// Apply heuristic layer assignments to a graph.
pub fn apply_heuristic_layers(graph: &mut Graph) -> usize {
    let mut assigned = 0;
    
    for node in &mut graph.nodes {
        // Skip if already has layer
        if node.metadata.contains_key("layer") {
            continue;
        }
        
        // Try to infer from path (stored in id for file nodes)
        if let Some(layer) = heuristic_assign_layer(&node.id) {
            node.metadata.insert("layer".to_string(), serde_json::json!(layer));
            assigned += 1;
        }
    }
    
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_heuristic_layer_assignment() {
        assert_eq!(heuristic_assign_layer("src/commands/init.ts"), Some("interface"));
        assert_eq!(heuristic_assign_layer("src/services/auth.ts"), Some("application"));
        assert_eq!(heuristic_assign_layer("src/core/graph.ts"), Some("domain"));
        assert_eq!(heuristic_assign_layer("src/extractors/typescript.ts"), Some("infrastructure"));
        assert_eq!(heuristic_assign_layer("src/utils.ts"), None);
    }
    
    #[test]
    fn test_parse_proposals() {
        let response = r#"```json
{
  "proposals": [
    {
      "type": "assign_layer",
      "node_id": "src/cli.ts",
      "layer": "interface",
      "reason": "CLI entry point",
      "confidence": 0.9
    }
  ]
}
```"#;
        
        let result = parse_semantify_response(response).unwrap();
        assert_eq!(result.proposals.len(), 1);
    }
    
    #[test]
    fn test_apply_proposals() {
        let mut graph = Graph::new();
        graph.add_node(Node::new("src/cli.ts", "CLI"));
        
        let proposals = vec![
            SemanticProposal::AssignLayer {
                node_id: "src/cli.ts".to_string(),
                layer: "interface".to_string(),
                reason: "CLI".to_string(),
                confidence: 0.9,
            },
        ];
        
        let applied = apply_proposals(&mut graph, &proposals);
        assert_eq!(applied, 1);
        
        let node = graph.get_node("src/cli.ts").unwrap();
        assert_eq!(node.metadata.get("layer").and_then(|v| v.as_str()), Some("interface"));
    }
}
