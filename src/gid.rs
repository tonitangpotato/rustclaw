//! GID Native Integration — Task Graph.
//!
//! Native Rust implementation of GID (Graph-Indexed Development) for task management.
//! Replaces the TypeScript MCP server with a native Rust module.
//!
//! Features:
//! - Task graph with nodes (tasks) and edges (dependencies)
//! - YAML persistence (compatible with existing graph.yml files)
//! - Dependency tracking and resolution
//! - Topological sort and critical path analysis
//! - Ready task detection (tasks with no unmet dependencies)
//!
//! Note: Requires `serde_yaml` dependency. Add to Cargo.toml:
//! ```toml
//! serde_yaml = "0.9"
//! ```

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A task graph with nodes and edges.
#[derive(Debug, Clone, Default)]
pub struct TaskGraph {
    nodes: HashMap<String, TaskNode>,
    edges: Vec<TaskEdge>,
}

/// A task node in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    /// Unique task ID.
    pub id: String,
    /// Task title.
    pub title: String,
    /// Current status.
    pub status: GidTaskStatus,
    /// Detailed description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Agent assigned to this task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Priority (0 = highest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    /// Creation timestamp (Unix milliseconds).
    pub created_at: i64,
    /// Last update timestamp (Unix milliseconds).
    pub updated_at: i64,
    /// Additional metadata.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl TaskNode {
    /// Create a new task node.
    pub fn new(id: &str, title: &str) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            id: id.to_string(),
            title: title.to_string(),
            status: GidTaskStatus::Todo,
            description: None,
            assigned_to: None,
            tags: Vec::new(),
            priority: None,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Builder: set priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Builder: set tags.
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Builder: set status.
    pub fn with_status(mut self, status: GidTaskStatus) -> Self {
        self.status = status;
        self
    }
}

/// Status of a GID task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GidTaskStatus {
    Todo,
    #[serde(alias = "in_progress")]
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

impl Default for GidTaskStatus {
    fn default() -> Self {
        Self::Todo
    }
}

impl std::fmt::Display for GidTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GidTaskStatus::Todo => write!(f, "todo"),
            GidTaskStatus::InProgress => write!(f, "in_progress"),
            GidTaskStatus::Done => write!(f, "done"),
            GidTaskStatus::Blocked => write!(f, "blocked"),
            GidTaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// An edge (dependency) in the task graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    /// Source task ID.
    pub from: String,
    /// Target task ID.
    pub to: String,
    /// Relationship type: "depends_on", "blocks", "relates_to", "subtask_of".
    pub relation: String,
}

impl TaskEdge {
    /// Create a new edge.
    pub fn new(from: &str, to: &str, relation: &str) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            relation: relation.to_string(),
        }
    }

    /// Create a "depends_on" edge.
    pub fn depends_on(from: &str, to: &str) -> Self {
        Self::new(from, to, "depends_on")
    }

    /// Create a "blocks" edge (inverse of depends_on).
    pub fn blocks(from: &str, to: &str) -> Self {
        Self::new(from, to, "blocks")
    }

    /// Create a "subtask_of" edge.
    pub fn subtask_of(from: &str, to: &str) -> Self {
        Self::new(from, to, "subtask_of")
    }
}

/// Summary statistics for a task graph.
#[derive(Debug, Clone, Default)]
pub struct GraphSummary {
    pub total_tasks: usize,
    pub todo: usize,
    pub in_progress: usize,
    pub done: usize,
    pub blocked: usize,
    pub cancelled: usize,
    pub total_edges: usize,
    pub ready_tasks: usize,
}

/// YAML file format for graph persistence.
#[derive(Debug, Serialize, Deserialize)]
struct GraphFile {
    #[serde(default)]
    nodes: Vec<TaskNode>,
    #[serde(default)]
    edges: Vec<TaskEdge>,
}

impl TaskGraph {
    /// Create a new empty task graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a task graph from a YAML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let file: GraphFile = serde_yaml::from_str(&content)?;

        let mut graph = Self::new();
        for node in file.nodes {
            graph.nodes.insert(node.id.clone(), node);
        }
        graph.edges = file.edges;

        tracing::info!(
            "Loaded task graph from {:?}: {} nodes, {} edges",
            path,
            graph.nodes.len(),
            graph.edges.len()
        );

        Ok(graph)
    }

    /// Save the task graph to a YAML file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let file = GraphFile {
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.clone(),
        };

        let content = serde_yaml::to_string(&file)?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;

        tracing::info!(
            "Saved task graph to {:?}: {} nodes, {} edges",
            path,
            self.nodes.len(),
            self.edges.len()
        );

        Ok(())
    }

    /// Add a task to the graph.
    pub fn add_task(&mut self, task: TaskNode) {
        tracing::debug!("Adding task: {} ({})", task.id, task.title);
        self.nodes.insert(task.id.clone(), task);
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<&TaskNode> {
        self.nodes.get(task_id)
    }

    /// Get a mutable task by ID.
    pub fn get_task_mut(&mut self, task_id: &str) -> Option<&mut TaskNode> {
        self.nodes.get_mut(task_id)
    }

    /// Remove a task from the graph.
    pub fn remove_task(&mut self, task_id: &str) -> Option<TaskNode> {
        // Remove associated edges
        self.edges.retain(|e| e.from != task_id && e.to != task_id);
        self.nodes.remove(task_id)
    }

    /// Update a task's status.
    pub fn update_task_status(
        &mut self,
        task_id: &str,
        status: GidTaskStatus,
    ) -> anyhow::Result<()> {
        let task = self
            .nodes
            .get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

        tracing::info!(
            "Updating task {} status: {} → {}",
            task_id,
            task.status,
            status
        );

        task.status = status;
        task.updated_at = chrono::Utc::now().timestamp_millis();

        Ok(())
    }

    /// Add an edge to the graph.
    pub fn add_edge(&mut self, edge: TaskEdge) {
        tracing::debug!(
            "Adding edge: {} --[{}]--> {}",
            edge.from,
            edge.relation,
            edge.to
        );
        self.edges.push(edge);
    }

    /// Remove edges matching criteria.
    pub fn remove_edges(&mut self, from: Option<&str>, to: Option<&str>, relation: Option<&str>) {
        self.edges.retain(|e| {
            let from_match = from.map_or(true, |f| e.from != f);
            let to_match = to.map_or(true, |t| e.to != t);
            let rel_match = relation.map_or(true, |r| e.relation != r);
            from_match || to_match || rel_match
        });
    }

    /// Get all edges from a task.
    pub fn get_edges_from(&self, task_id: &str) -> Vec<&TaskEdge> {
        self.edges.iter().filter(|e| e.from == task_id).collect()
    }

    /// Get all edges to a task.
    pub fn get_edges_to(&self, task_id: &str) -> Vec<&TaskEdge> {
        self.edges.iter().filter(|e| e.to == task_id).collect()
    }

    /// Get tasks that are ready to work on.
    ///
    /// A task is ready if:
    /// - Status is Todo
    /// - All dependencies (tasks it depends_on) are Done
    pub fn get_ready_tasks(&self) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|task| {
                if task.status != GidTaskStatus::Todo {
                    return false;
                }

                // Find all dependencies
                let dependencies: Vec<&str> = self
                    .edges
                    .iter()
                    .filter(|e| e.from == task.id && e.relation == "depends_on")
                    .map(|e| e.to.as_str())
                    .collect();

                // Check all dependencies are done
                dependencies.iter().all(|dep_id| {
                    self.nodes
                        .get(*dep_id)
                        .map(|t| t.status == GidTaskStatus::Done)
                        .unwrap_or(true) // Missing dependency = satisfied
                })
            })
            .collect()
    }

    /// Get tasks that block a given task.
    pub fn get_blocked_by(&self, task_id: &str) -> Vec<&TaskNode> {
        self.edges
            .iter()
            .filter(|e| e.from == task_id && e.relation == "depends_on")
            .filter_map(|e| self.nodes.get(&e.to))
            .filter(|t| t.status != GidTaskStatus::Done)
            .collect()
    }

    /// Get tasks that depend on a given task.
    pub fn get_dependents(&self, task_id: &str) -> Vec<&TaskNode> {
        self.edges
            .iter()
            .filter(|e| e.to == task_id && e.relation == "depends_on")
            .filter_map(|e| self.nodes.get(&e.from))
            .collect()
    }

    /// Get subtasks of a given task.
    pub fn get_subtasks(&self, task_id: &str) -> Vec<&TaskNode> {
        self.edges
            .iter()
            .filter(|e| e.to == task_id && e.relation == "subtask_of")
            .filter_map(|e| self.nodes.get(&e.from))
            .collect()
    }

    /// Topological sort of task IDs.
    ///
    /// Returns task IDs in dependency order (dependencies before dependents).
    /// Returns an error if there's a cycle.
    pub fn topological_sort(&self) -> anyhow::Result<Vec<String>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize
        for id in self.nodes.keys() {
            in_degree.insert(id.as_str(), 0);
            adj.insert(id.as_str(), Vec::new());
        }

        // Build adjacency list from depends_on edges
        // depends_on: A depends on B → B must come before A → edge B→A
        for edge in &self.edges {
            if edge.relation == "depends_on" {
                if let Some(adj_list) = adj.get_mut(edge.to.as_str()) {
                    adj_list.push(&edge.from);
                }
                if let Some(deg) = in_degree.get_mut(edge.from.as_str()) {
                    *deg += 1;
                }
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(node) = queue.pop_front() {
            result.push(node.to_string());

            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if result.len() != self.nodes.len() {
            anyhow::bail!("Cycle detected in task graph");
        }

        Ok(result)
    }

    /// Find the critical path (longest dependency chain).
    ///
    /// Returns task IDs in the critical path, from first to last.
    pub fn critical_path(&self) -> Vec<String> {
        if self.nodes.is_empty() {
            return Vec::new();
        }

        // Get topological order
        let topo_order = match self.topological_sort() {
            Ok(order) => order,
            Err(_) => return Vec::new(), // Cycle = no critical path
        };

        // Build adjacency for depends_on (reversed for path finding)
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for id in self.nodes.keys() {
            adj.insert(id.as_str(), Vec::new());
        }
        for edge in &self.edges {
            if edge.relation == "depends_on" {
                if let Some(list) = adj.get_mut(edge.to.as_str()) {
                    list.push(&edge.from);
                }
            }
        }

        // Compute longest path from each node
        let mut dist: HashMap<&str, usize> = HashMap::new();
        let mut parent: HashMap<&str, Option<&str>> = HashMap::new();

        for id in &topo_order {
            dist.insert(id.as_str(), 0);
            parent.insert(id.as_str(), None);
        }

        for id in &topo_order {
            let current_dist = dist[id.as_str()];
            if let Some(neighbors) = adj.get(id.as_str()) {
                for &neighbor in neighbors {
                    let new_dist = current_dist + 1;
                    if new_dist > dist[neighbor] {
                        dist.insert(neighbor, new_dist);
                        parent.insert(neighbor, Some(id.as_str()));
                    }
                }
            }
        }

        // Find node with maximum distance
        let (&end_node, _) = dist.iter().max_by_key(|(_, &d)| d).unwrap_or((&"", &0));

        if end_node.is_empty() {
            return Vec::new();
        }

        // Reconstruct path
        let mut path = Vec::new();
        let mut current = Some(end_node);
        while let Some(node) = current {
            path.push(node.to_string());
            current = parent.get(node).and_then(|&p| p);
        }

        path.reverse();
        path
    }

    /// Get summary statistics for the graph.
    pub fn summary(&self) -> GraphSummary {
        let mut summary = GraphSummary {
            total_tasks: self.nodes.len(),
            total_edges: self.edges.len(),
            ..Default::default()
        };

        for task in self.nodes.values() {
            match task.status {
                GidTaskStatus::Todo => summary.todo += 1,
                GidTaskStatus::InProgress => summary.in_progress += 1,
                GidTaskStatus::Done => summary.done += 1,
                GidTaskStatus::Blocked => summary.blocked += 1,
                GidTaskStatus::Cancelled => summary.cancelled += 1,
            }
        }

        summary.ready_tasks = self.get_ready_tasks().len();

        summary
    }

    /// Get all tasks.
    pub fn tasks(&self) -> impl Iterator<Item = &TaskNode> {
        self.nodes.values()
    }

    /// Get all edges.
    pub fn edges(&self) -> &[TaskEdge] {
        &self.edges
    }

    /// Get task count.
    pub fn task_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get edge count.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Check if graph has cycles.
    pub fn has_cycles(&self) -> bool {
        self.topological_sort().is_err()
    }

    /// Find tasks by tag.
    pub fn find_by_tag(&self, tag: &str) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|t| t.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Find tasks by status.
    pub fn find_by_status(&self, status: GidTaskStatus) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|t| t.status == status)
            .collect()
    }

    /// Find tasks assigned to an agent.
    pub fn find_by_assignee(&self, assignee: &str) -> Vec<&TaskNode> {
        self.nodes
            .values()
            .filter(|t| t.assigned_to.as_deref() == Some(assignee))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_node_new() {
        let task = TaskNode::new("task1", "Build feature");

        assert_eq!(task.id, "task1");
        assert_eq!(task.title, "Build feature");
        assert_eq!(task.status, GidTaskStatus::Todo);
        assert!(task.description.is_none());
    }

    #[test]
    fn test_task_node_builder() {
        let task = TaskNode::new("task1", "Build feature")
            .with_description("Detailed desc")
            .with_priority(10)
            .with_tags(vec!["backend".to_string()])
            .with_status(GidTaskStatus::InProgress);

        assert_eq!(task.description, Some("Detailed desc".to_string()));
        assert_eq!(task.priority, Some(10));
        assert_eq!(task.tags, vec!["backend"]);
        assert_eq!(task.status, GidTaskStatus::InProgress);
    }

    #[test]
    fn test_gid_status_display() {
        assert_eq!(format!("{}", GidTaskStatus::Todo), "todo");
        assert_eq!(format!("{}", GidTaskStatus::InProgress), "in_progress");
        assert_eq!(format!("{}", GidTaskStatus::Done), "done");
        assert_eq!(format!("{}", GidTaskStatus::Blocked), "blocked");
        assert_eq!(format!("{}", GidTaskStatus::Cancelled), "cancelled");
    }

    #[test]
    fn test_task_edge_new() {
        let edge = TaskEdge::new("task1", "task2", "depends_on");

        assert_eq!(edge.from, "task1");
        assert_eq!(edge.to, "task2");
        assert_eq!(edge.relation, "depends_on");
    }

    #[test]
    fn test_task_edge_helpers() {
        let dep = TaskEdge::depends_on("a", "b");
        assert_eq!(dep.relation, "depends_on");

        let blocks = TaskEdge::blocks("a", "b");
        assert_eq!(blocks.relation, "blocks");

        let subtask = TaskEdge::subtask_of("a", "b");
        assert_eq!(subtask.relation, "subtask_of");
    }

    #[test]
    fn test_graph_add_and_get() {
        let mut graph = TaskGraph::new();

        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));

        assert_eq!(graph.task_count(), 2);
        assert_eq!(graph.get_task("t1").unwrap().title, "Task 1");
        assert!(graph.get_task("t3").is_none());
    }

    #[test]
    fn test_graph_update_status() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));

        assert_eq!(graph.get_task("t1").unwrap().status, GidTaskStatus::Todo);

        graph
            .update_task_status("t1", GidTaskStatus::Done)
            .unwrap();

        assert_eq!(graph.get_task("t1").unwrap().status, GidTaskStatus::Done);
    }

    #[test]
    fn test_graph_edges() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));
        graph.add_task(TaskNode::new("t3", "Task 3"));

        graph.add_edge(TaskEdge::depends_on("t2", "t1")); // t2 depends on t1
        graph.add_edge(TaskEdge::depends_on("t3", "t2")); // t3 depends on t2

        assert_eq!(graph.edge_count(), 2);

        let edges_from_t2 = graph.get_edges_from("t2");
        assert_eq!(edges_from_t2.len(), 1);
        assert_eq!(edges_from_t2[0].to, "t1");
    }

    #[test]
    fn test_get_ready_tasks() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));
        graph.add_task(TaskNode::new("t3", "Task 3"));

        graph.add_edge(TaskEdge::depends_on("t2", "t1")); // t2 depends on t1
        graph.add_edge(TaskEdge::depends_on("t3", "t2")); // t3 depends on t2

        // Only t1 is ready (no dependencies)
        let ready = graph.get_ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t1");

        // Complete t1 → t2 becomes ready
        graph.update_task_status("t1", GidTaskStatus::Done).unwrap();
        let ready = graph.get_ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t2");

        // Complete t2 → t3 becomes ready
        graph.update_task_status("t2", GidTaskStatus::Done).unwrap();
        let ready = graph.get_ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t3");
    }

    #[test]
    fn test_get_blocked_by() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));

        graph.add_edge(TaskEdge::depends_on("t2", "t1")); // t2 depends on t1

        // t2 is blocked by t1
        let blockers = graph.get_blocked_by("t2");
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].id, "t1");

        // t1 has no blockers
        let blockers = graph.get_blocked_by("t1");
        assert!(blockers.is_empty());
    }

    #[test]
    fn test_get_dependents() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));
        graph.add_task(TaskNode::new("t3", "Task 3"));

        graph.add_edge(TaskEdge::depends_on("t2", "t1")); // t2 depends on t1
        graph.add_edge(TaskEdge::depends_on("t3", "t1")); // t3 depends on t1

        // t2 and t3 depend on t1
        let dependents = graph.get_dependents("t1");
        assert_eq!(dependents.len(), 2);
    }

    #[test]
    fn test_topological_sort() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("a", "A"));
        graph.add_task(TaskNode::new("b", "B"));
        graph.add_task(TaskNode::new("c", "C"));
        graph.add_task(TaskNode::new("d", "D"));

        // d depends on b and c
        // b depends on a
        // c depends on a
        graph.add_edge(TaskEdge::depends_on("b", "a"));
        graph.add_edge(TaskEdge::depends_on("c", "a"));
        graph.add_edge(TaskEdge::depends_on("d", "b"));
        graph.add_edge(TaskEdge::depends_on("d", "c"));

        let order = graph.topological_sort().unwrap();

        // a must come before b and c
        // b and c must come before d
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        let pos_d = order.iter().position(|x| x == "d").unwrap();

        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);
    }

    #[test]
    fn test_topological_sort_cycle() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("a", "A"));
        graph.add_task(TaskNode::new("b", "B"));

        // Cycle: a depends on b, b depends on a
        graph.add_edge(TaskEdge::depends_on("a", "b"));
        graph.add_edge(TaskEdge::depends_on("b", "a"));

        assert!(graph.topological_sort().is_err());
        assert!(graph.has_cycles());
    }

    #[test]
    fn test_critical_path() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("a", "A"));
        graph.add_task(TaskNode::new("b", "B"));
        graph.add_task(TaskNode::new("c", "C"));
        graph.add_task(TaskNode::new("d", "D"));

        // Chain: a → b → c → d (longest)
        // Also: a → d (shorter)
        graph.add_edge(TaskEdge::depends_on("b", "a"));
        graph.add_edge(TaskEdge::depends_on("c", "b"));
        graph.add_edge(TaskEdge::depends_on("d", "c"));

        let path = graph.critical_path();
        assert_eq!(path, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_summary() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1").with_status(GidTaskStatus::Done));
        graph.add_task(TaskNode::new("t2", "Task 2").with_status(GidTaskStatus::InProgress));
        graph.add_task(TaskNode::new("t3", "Task 3").with_status(GidTaskStatus::Todo));
        graph.add_task(TaskNode::new("t4", "Task 4").with_status(GidTaskStatus::Todo));

        graph.add_edge(TaskEdge::depends_on("t3", "t1"));

        let summary = graph.summary();

        assert_eq!(summary.total_tasks, 4);
        assert_eq!(summary.done, 1);
        assert_eq!(summary.in_progress, 1);
        assert_eq!(summary.todo, 2);
        assert_eq!(summary.total_edges, 1);
        // t3 depends on done t1, so t3 is ready; t4 has no deps, so also ready
        assert_eq!(summary.ready_tasks, 2);
    }

    #[test]
    fn test_find_by_tag() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1").with_tags(vec!["backend".to_string()]));
        graph.add_task(
            TaskNode::new("t2", "Task 2")
                .with_tags(vec!["frontend".to_string(), "backend".to_string()]),
        );
        graph.add_task(TaskNode::new("t3", "Task 3").with_tags(vec!["frontend".to_string()]));

        let backend_tasks = graph.find_by_tag("backend");
        assert_eq!(backend_tasks.len(), 2);

        let frontend_tasks = graph.find_by_tag("frontend");
        assert_eq!(frontend_tasks.len(), 2);

        let infra_tasks = graph.find_by_tag("infra");
        assert!(infra_tasks.is_empty());
    }

    #[test]
    fn test_find_by_status() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1").with_status(GidTaskStatus::Done));
        graph.add_task(TaskNode::new("t2", "Task 2").with_status(GidTaskStatus::Done));
        graph.add_task(TaskNode::new("t3", "Task 3").with_status(GidTaskStatus::InProgress));

        let done = graph.find_by_status(GidTaskStatus::Done);
        assert_eq!(done.len(), 2);

        let in_progress = graph.find_by_status(GidTaskStatus::InProgress);
        assert_eq!(in_progress.len(), 1);
    }

    #[test]
    fn test_find_by_assignee() {
        let mut graph = TaskGraph::new();

        let mut t1 = TaskNode::new("t1", "Task 1");
        t1.assigned_to = Some("agent-1".to_string());
        graph.add_task(t1);

        let mut t2 = TaskNode::new("t2", "Task 2");
        t2.assigned_to = Some("agent-1".to_string());
        graph.add_task(t2);

        graph.add_task(TaskNode::new("t3", "Task 3")); // Unassigned

        let agent1_tasks = graph.find_by_assignee("agent-1");
        assert_eq!(agent1_tasks.len(), 2);

        let agent2_tasks = graph.find_by_assignee("agent-2");
        assert!(agent2_tasks.is_empty());
    }

    #[test]
    fn test_remove_task() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1"));
        graph.add_task(TaskNode::new("t2", "Task 2"));
        graph.add_edge(TaskEdge::depends_on("t2", "t1"));

        assert_eq!(graph.task_count(), 2);
        assert_eq!(graph.edge_count(), 1);

        // Remove t1 - should also remove the edge
        let removed = graph.remove_task("t1");
        assert!(removed.is_some());
        assert_eq!(graph.task_count(), 1);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_yaml_serialization() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("t1", "Task 1").with_description("First task"));
        graph.add_task(TaskNode::new("t2", "Task 2"));
        graph.add_edge(TaskEdge::depends_on("t2", "t1"));

        // Serialize to YAML format
        let file = GraphFile {
            nodes: graph.nodes.values().cloned().collect(),
            edges: graph.edges.clone(),
        };

        let yaml = serde_yaml::to_string(&file).unwrap();
        assert!(yaml.contains("t1"));
        assert!(yaml.contains("Task 1"));
        assert!(yaml.contains("depends_on"));

        // Deserialize back
        let parsed: GraphFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.nodes.len(), 2);
        assert_eq!(parsed.edges.len(), 1);
    }

    #[test]
    fn test_get_subtasks() {
        let mut graph = TaskGraph::new();
        graph.add_task(TaskNode::new("parent", "Parent Task"));
        graph.add_task(TaskNode::new("sub1", "Subtask 1"));
        graph.add_task(TaskNode::new("sub2", "Subtask 2"));

        graph.add_edge(TaskEdge::subtask_of("sub1", "parent"));
        graph.add_edge(TaskEdge::subtask_of("sub2", "parent"));

        let subtasks = graph.get_subtasks("parent");
        assert_eq!(subtasks.len(), 2);
    }
}
