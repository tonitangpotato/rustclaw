//! Impact analysis and graph analysis utilities
//!
//! Provides functions for analyzing code dependencies and impacts:
//! - Impact reports: what depends on what
//! - Causal chains: tracing dependencies
//! - Keyword search: finding nodes by name/path
//! - Schema validation

use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use crate::code_graph::types::{CodeGraph, CodeNode, CodeEdge, ImpactReport, CausalChain, ChainNode, EdgeRelation};

impl CodeGraph {
    /// Build adjacency lists for fast lookup
    pub fn build_index(&mut self) {
        self.outgoing.clear();
        self.incoming.clear();
        self.node_index.clear();

        for (i, node) in self.nodes.iter().enumerate() {
            self.node_index.insert(node.id.clone(), i);
        }

        for (i, edge) in self.edges.iter().enumerate() {
            self.outgoing
                .entry(edge.from.clone())
                .or_insert_with(Vec::new)
                .push(i);
            self.incoming
                .entry(edge.to.clone())
                .or_insert_with(Vec::new)
                .push(i);
        }
    }

    /// Analyze the impact of changing a node
    pub fn analyze_impact(&self, node_id: &str) -> Option<ImpactReport> {
        let node = self.nodes.iter().find(|n| n.id == node_id)?;
        
        let mut downstream = Vec::new();
        let mut upstream = Vec::new();
        let mut visited = HashSet::new();
        
        if let Some(out_edges) = self.outgoing.get(node_id) {
            for &edge_idx in out_edges {
                let edge = &self.edges[edge_idx];
                if let Some(&node_idx) = self.node_index.get(&edge.to) {
                    if visited.insert(edge.to.clone()) {
                        downstream.push(&self.nodes[node_idx]);
                    }
                }
            }
        }
        
        if let Some(in_edges) = self.incoming.get(node_id) {
            for &edge_idx in in_edges {
                let edge = &self.edges[edge_idx];
                if let Some(&node_idx) = self.node_index.get(&edge.from) {
                    if visited.insert(edge.from.clone()) {
                        upstream.push(&self.nodes[node_idx]);
                    }
                }
            }
        }
        
        Some(ImpactReport {
            node,
            downstream,
            upstream,
        })
    }

    /// Find all transitive dependencies of a node (BFS)
    pub fn transitive_dependencies(&self, node_id: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        
        queue.push_back(node_id.to_string());
        visited.insert(node_id.to_string());
        
        while let Some(current) = queue.pop_front() {
            if let Some(out_edges) = self.outgoing.get(&current) {
                for &edge_idx in out_edges {
                    let edge = &self.edges[edge_idx];
                    if visited.insert(edge.to.clone()) {
                        result.push(edge.to.clone());
                        queue.push_back(edge.to.clone());
                    }
                }
            }
        }
        
        result
    }

    /// Find nodes that have keyword in their name or path (case-insensitive)
    pub fn search_by_keyword(&self, keyword: &str) -> Vec<&CodeNode> {
        let kw = keyword.to_lowercase();
        self.nodes
            .iter()
            .filter(|n| {
                n.name.to_lowercase().contains(&kw)
                    || n.file_path.to_lowercase().contains(&kw)
            })
            .collect()
    }

    /// Find the shortest causal chain from symptom to root cause
    pub fn find_causal_chain(&self, symptom_id: &str, root_id: &str) -> Option<CausalChain> {
        // BFS to find shortest path
        let mut queue = VecDeque::new();
        let mut parent: HashMap<String, (String, String)> = HashMap::new();
        
        queue.push_back(symptom_id.to_string());
        
        while let Some(current) = queue.pop_front() {
            if current == root_id {
                // Reconstruct path
                let mut chain_nodes = Vec::new();
                let mut node_id = root_id.to_string();
                
                while node_id != symptom_id {
                    if let Some(node) = self.nodes.iter().find(|n| n.id == node_id) {
                        let (prev_id, rel) = parent.get(&node_id).unwrap();
                        chain_nodes.push(ChainNode {
                            node_id: node_id.clone(),
                            node_name: node.name.clone(),
                            relation: rel.clone(),
                        });
                        node_id = prev_id.clone();
                    } else {
                        break;
                    }
                }
                
                // Add symptom node
                if let Some(node) = self.nodes.iter().find(|n| n.id == symptom_id) {
                    chain_nodes.push(ChainNode {
                        node_id: symptom_id.to_string(),
                        node_name: node.name.clone(),
                        relation: "symptom".to_string(),
                    });
                }
                
                chain_nodes.reverse();
                
                return Some(CausalChain {
                    nodes: chain_nodes,
                    score: 1.0 / (chain_nodes.len() as f32),
                });
            }
            
            if let Some(in_edges) = self.incoming.get(&current) {
                for &edge_idx in in_edges {
                    let edge = &self.edges[edge_idx];
                    if !parent.contains_key(&edge.from) {
                        parent.insert(
                            edge.from.clone(),
                            (current.clone(), edge.relation.to_string()),
                        );
                        queue.push_back(edge.from.clone());
                    }
                }
            }
        }
        
        None
    }

    /// Validate graph schema: check for dangling edges
    pub fn validate_schema(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let node_ids: HashSet<_> = self.nodes.iter().map(|n| &n.id).collect();
        
        for (i, edge) in self.edges.iter().enumerate() {
            if !node_ids.contains(&edge.from) {
                errors.push(format!("Edge {} has dangling 'from': {}", i, edge.from));
            }
            if !node_ids.contains(&edge.to) {
                errors.push(format!("Edge {} has dangling 'to': {}", i, edge.to));
            }
        }
        
        errors
    }

    /// Get all nodes of a specific kind
    pub fn nodes_by_kind(&self, kind: crate::code_graph::types::NodeKind) -> Vec<&CodeNode> {
        self.nodes.iter().filter(|n| n.kind == kind).collect()
    }

    /// Find all call paths between two nodes (DFS with depth limit)
    pub fn find_call_paths(
        &self,
        from_id: &str,
        to_id: &str,
        max_depth: usize,
    ) -> Vec<Vec<String>> {
        let mut paths = Vec::new();
        let mut current_path = vec![from_id.to_string()];
        let mut visited = HashSet::new();
        
        self.dfs_find_paths(
            from_id,
            to_id,
            &mut current_path,
            &mut visited,
            &mut paths,
            max_depth,
        );
        
        paths
    }

    fn dfs_find_paths(
        &self,
        current: &str,
        target: &str,
        path: &mut Vec<String>,
        visited: &mut HashSet<String>,
        paths: &mut Vec<Vec<String>>,
        max_depth: usize,
    ) {
        if path.len() > max_depth {
            return;
        }
        
        if current == target {
            paths.push(path.clone());
            return;
        }
        
        visited.insert(current.to_string());
        
        if let Some(out_edges) = self.outgoing.get(current) {
            for &edge_idx in out_edges {
                let edge = &self.edges[edge_idx];
                if edge.relation == EdgeRelation::Calls && !visited.contains(&edge.to) {
                    path.push(edge.to.clone());
                    self.dfs_find_paths(&edge.to, target, path, visited, paths, max_depth);
                    path.pop();
                }
            }
        }
        
        visited.remove(current);
    }
}
