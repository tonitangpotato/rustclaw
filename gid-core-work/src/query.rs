use std::collections::{HashMap, HashSet, VecDeque};
use crate::graph::{Graph, Node};

/// Query engine for graph traversal and analysis.
pub struct QueryEngine<'a> {
    graph: &'a Graph,
}

impl<'a> QueryEngine<'a> {
    pub fn new(graph: &'a Graph) -> Self {
        Self { graph }
    }

    /// Impact analysis: what nodes are affected if `node_id` changes?
    /// Follows reverse dependency edges (who depends on this node?).
    pub fn impact(&self, node_id: &str) -> Vec<&'a Node> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(node_id.to_string());
        visited.insert(node_id.to_string());

        while let Some(current) = queue.pop_front() {
            // Find nodes that depend_on current (edges where to == current)
            for edge in &self.graph.edges {
                if edge.to == current && edge.relation == "depends_on" {
                    if visited.insert(edge.from.clone()) {
                        queue.push_back(edge.from.clone());
                    }
                }
            }
        }

        visited.remove(node_id);
        self.graph.nodes.iter()
            .filter(|n| visited.contains(&n.id))
            .collect()
    }

    /// Dependencies: what does `node_id` depend on? (transitive)
    pub fn deps(&self, node_id: &str, transitive: bool) -> Vec<&'a Node> {
        if !transitive {
            // Direct deps only
            let dep_ids: HashSet<&str> = self.graph.edges.iter()
                .filter(|e| e.from == node_id && e.relation == "depends_on")
                .map(|e| e.to.as_str())
                .collect();
            return self.graph.nodes.iter()
                .filter(|n| dep_ids.contains(n.id.as_str()))
                .collect();
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(node_id.to_string());
        visited.insert(node_id.to_string());

        while let Some(current) = queue.pop_front() {
            for edge in &self.graph.edges {
                if edge.from == current && edge.relation == "depends_on" {
                    if visited.insert(edge.to.clone()) {
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }

        visited.remove(node_id);
        self.graph.nodes.iter()
            .filter(|n| visited.contains(&n.id))
            .collect()
    }

    /// Find shortest path between two nodes (any edge direction).
    pub fn path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        queue.push_back(from.to_string());
        visited.insert(from.to_string());

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstruct path
                let mut path = vec![to.to_string()];
                let mut cur = to.to_string();
                while let Some(p) = parent.get(&cur) {
                    path.push(p.clone());
                    cur = p.clone();
                }
                path.reverse();
                return Some(path);
            }

            // Follow edges in both directions
            for edge in &self.graph.edges {
                let neighbor = if edge.from == current {
                    &edge.to
                } else if edge.to == current {
                    &edge.from
                } else {
                    continue;
                };
                if visited.insert(neighbor.clone()) {
                    parent.insert(neighbor.clone(), current.clone());
                    queue.push_back(neighbor.clone());
                }
            }
        }

        None
    }

    /// Common cause: find shared dependencies of two nodes.
    pub fn common_cause(&self, node_a: &str, node_b: &str) -> Vec<&'a Node> {
        let deps_a: HashSet<String> = self.deps(node_a, true)
            .iter().map(|n| n.id.clone()).collect();
        let deps_b: HashSet<String> = self.deps(node_b, true)
            .iter().map(|n| n.id.clone()).collect();
        let common: HashSet<&String> = deps_a.intersection(&deps_b).collect();

        self.graph.nodes.iter()
            .filter(|n| common.contains(&n.id))
            .collect()
    }

    /// Topological sort (returns error if cycle detected).
    pub fn topological_sort(&self) -> anyhow::Result<Vec<String>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for node in &self.graph.nodes {
            in_degree.entry(&node.id).or_insert(0);
        }
        for edge in &self.graph.edges {
            if edge.relation == "depends_on" {
                *in_degree.entry(&edge.from).or_insert(0) += 1;
            }
        }

        let mut queue: VecDeque<&str> = in_degree.iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = Vec::new();
        while let Some(node) = queue.pop_front() {
            sorted.push(node.to_string());
            for edge in &self.graph.edges {
                if edge.to == node && edge.relation == "depends_on" {
                    if let Some(deg) = in_degree.get_mut(edge.from.as_str()) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(&edge.from);
                        }
                    }
                }
            }
        }

        if sorted.len() != self.graph.nodes.len() {
            anyhow::bail!("Cycle detected in graph");
        }

        Ok(sorted)
    }
}
