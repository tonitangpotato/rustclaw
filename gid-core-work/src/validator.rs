//! Graph validation: detect cycles, orphan nodes, missing references, etc.

use std::collections::{HashMap, HashSet, VecDeque};
use crate::graph::Graph;

/// Validation result with all issues found.
#[derive(Debug, Default)]
pub struct ValidationResult {
    pub orphan_nodes: Vec<String>,
    pub missing_refs: Vec<MissingRef>,
    pub cycles: Vec<Vec<String>>,
    pub duplicate_nodes: Vec<String>,
    pub duplicate_edges: Vec<DuplicateEdge>,
}

#[derive(Debug)]
pub struct MissingRef {
    pub edge_from: String,
    pub edge_to: String,
    pub missing_node: String,
}

#[derive(Debug)]
pub struct DuplicateEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.orphan_nodes.is_empty()
            && self.missing_refs.is_empty()
            && self.cycles.is_empty()
            && self.duplicate_nodes.is_empty()
            && self.duplicate_edges.is_empty()
    }

    pub fn issue_count(&self) -> usize {
        self.orphan_nodes.len()
            + self.missing_refs.len()
            + self.cycles.len()
            + self.duplicate_nodes.len()
            + self.duplicate_edges.len()
    }
}

impl std::fmt::Display for ValidationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_valid() {
            return write!(f, "✓ Graph is valid");
        }

        let mut lines = Vec::new();

        if !self.orphan_nodes.is_empty() {
            lines.push(format!(
                "Orphan nodes (no edges): {}",
                self.orphan_nodes.join(", ")
            ));
        }

        for mr in &self.missing_refs {
            lines.push(format!(
                "Missing node '{}' referenced by edge {} → {}",
                mr.missing_node, mr.edge_from, mr.edge_to
            ));
        }

        for cycle in &self.cycles {
            lines.push(format!("Cycle detected: {}", cycle.join(" → ")));
        }

        if !self.duplicate_nodes.is_empty() {
            lines.push(format!(
                "Duplicate node IDs: {}",
                self.duplicate_nodes.join(", ")
            ));
        }

        for de in &self.duplicate_edges {
            lines.push(format!(
                "Duplicate edge: {} → {} ({})",
                de.from, de.to, de.relation
            ));
        }

        write!(f, "✗ {} issues found:\n  {}", self.issue_count(), lines.join("\n  "))
    }
}

/// Validator for graph integrity.
pub struct Validator<'a> {
    graph: &'a Graph,
}

impl<'a> Validator<'a> {
    pub fn new(graph: &'a Graph) -> Self {
        Self { graph }
    }

    /// Run all validations and return combined result.
    pub fn validate(&self) -> ValidationResult {
        let mut result = ValidationResult::default();

        result.duplicate_nodes = self.find_duplicate_nodes();
        result.missing_refs = self.find_missing_refs();
        result.orphan_nodes = self.find_orphan_nodes();
        result.cycles = self.find_cycles();
        result.duplicate_edges = self.find_duplicate_edges();

        result
    }

    /// Find nodes that have no edges (neither incoming nor outgoing).
    pub fn find_orphan_nodes(&self) -> Vec<String> {
        let connected: HashSet<&str> = self.graph.edges.iter()
            .flat_map(|e| [e.from.as_str(), e.to.as_str()])
            .collect();

        self.graph.nodes.iter()
            .filter(|n| !connected.contains(n.id.as_str()))
            .map(|n| n.id.clone())
            .collect()
    }

    /// Find edges that reference non-existent nodes.
    pub fn find_missing_refs(&self) -> Vec<MissingRef> {
        let node_ids: HashSet<&str> = self.graph.nodes.iter()
            .map(|n| n.id.as_str())
            .collect();

        let mut missing = Vec::new();

        for edge in &self.graph.edges {
            if !node_ids.contains(edge.from.as_str()) {
                missing.push(MissingRef {
                    edge_from: edge.from.clone(),
                    edge_to: edge.to.clone(),
                    missing_node: edge.from.clone(),
                });
            }
            if !node_ids.contains(edge.to.as_str()) {
                missing.push(MissingRef {
                    edge_from: edge.from.clone(),
                    edge_to: edge.to.clone(),
                    missing_node: edge.to.clone(),
                });
            }
        }

        missing
    }

    /// Find cycles in the graph (specifically in depends_on edges).
    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        // Build adjacency list for depends_on edges
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &self.graph.nodes {
            adj.entry(&node.id).or_default();
        }
        for edge in &self.graph.edges {
            if edge.relation == "depends_on" {
                adj.entry(&edge.from).or_default().push(&edge.to);
            }
        }

        fn dfs<'a>(
            node: &'a str,
            adj: &HashMap<&'a str, Vec<&'a str>>,
            visited: &mut HashSet<&'a str>,
            rec_stack: &mut HashSet<&'a str>,
            path: &mut Vec<String>,
            cycles: &mut Vec<Vec<String>>,
        ) {
            visited.insert(node);
            rec_stack.insert(node);
            path.push(node.to_string());

            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        dfs(neighbor, adj, visited, rec_stack, path, cycles);
                    } else if rec_stack.contains(neighbor) {
                        // Found a cycle - extract the cycle portion
                        if let Some(start_idx) = path.iter().position(|x| x == neighbor) {
                            let mut cycle: Vec<String> = path[start_idx..].to_vec();
                            cycle.push(neighbor.to_string()); // Close the cycle
                            cycles.push(cycle);
                        }
                    }
                }
            }

            path.pop();
            rec_stack.remove(node);
        }

        for node in &self.graph.nodes {
            if !visited.contains(node.id.as_str()) {
                let mut path = Vec::new();
                dfs(&node.id, &adj, &mut visited, &mut rec_stack, &mut path, &mut cycles);
            }
        }

        cycles
    }

    /// Find duplicate node IDs.
    pub fn find_duplicate_nodes(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();

        for node in &self.graph.nodes {
            if !seen.insert(&node.id) {
                duplicates.push(node.id.clone());
            }
        }

        duplicates
    }

    /// Find duplicate edges (same from, to, relation).
    pub fn find_duplicate_edges(&self) -> Vec<DuplicateEdge> {
        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();

        for edge in &self.graph.edges {
            let key = (&edge.from, &edge.to, &edge.relation);
            if !seen.insert(key) {
                duplicates.push(DuplicateEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    relation: edge.relation.clone(),
                });
            }
        }

        duplicates
    }

    /// Check if adding an edge would create a cycle.
    pub fn would_create_cycle(&self, from: &str, to: &str) -> bool {
        // Adding from -> to creates a cycle if there's already a path from to -> from
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(to);
        visited.insert(to);

        while let Some(current) = queue.pop_front() {
            if current == from {
                return true;
            }
            for edge in &self.graph.edges {
                if edge.from == current && edge.relation == "depends_on" {
                    if visited.insert(&edge.to) {
                        queue.push_back(&edge.to);
                    }
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Node};

    #[test]
    fn test_orphan_detection() {
        let mut graph = Graph::new();
        graph.add_node(Node::new("a", "A"));
        graph.add_node(Node::new("b", "B"));
        graph.add_node(Node::new("c", "C"));
        graph.add_edge(Edge::depends_on("a", "b"));
        
        let validator = Validator::new(&graph);
        let orphans = validator.find_orphan_nodes();
        assert_eq!(orphans, vec!["c"]);
    }

    #[test]
    fn test_missing_refs() {
        let mut graph = Graph::new();
        graph.add_node(Node::new("a", "A"));
        graph.edges.push(Edge::depends_on("a", "missing"));

        let validator = Validator::new(&graph);
        let missing = validator.find_missing_refs();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].missing_node, "missing");
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = Graph::new();
        graph.add_node(Node::new("a", "A"));
        graph.add_node(Node::new("b", "B"));
        graph.add_node(Node::new("c", "C"));
        graph.add_edge(Edge::depends_on("a", "b"));
        graph.add_edge(Edge::depends_on("b", "c"));
        graph.add_edge(Edge::depends_on("c", "a")); // cycle!

        let validator = Validator::new(&graph);
        let cycles = validator.find_cycles();
        assert!(!cycles.is_empty());
    }
}
