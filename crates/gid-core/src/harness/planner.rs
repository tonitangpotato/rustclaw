//! Execution planner: generate ExecutionPlan from graph topology.
//!
//! Pure function — no I/O, no LLM calls.

use anyhow::Result;
use crate::graph::Graph;
use super::topology::{compute_layers, critical_path};
use super::types::{ExecutionPlan, ExecutionLayer, TaskInfo};

/// Generate an execution plan from the graph.
///
/// - Groups tasks into parallelizable layers via topological sort
/// - Enriches each task with metadata from graph nodes
/// - Computes critical path and total estimated turns
/// - Idempotent: calling N times on same graph produces identical plan (GUARD-1)
/// - Skips tasks that are already `done` or `cancelled`
pub fn create_plan(graph: &Graph) -> Result<ExecutionPlan> {
    let layers = compute_layers(graph)?;
    let cp = critical_path(graph);

    let mut execution_layers = Vec::new();
    let mut total_tasks = 0;
    let mut estimated_total_turns: u32 = 0;

    for (index, task_ids) in layers.iter().enumerate() {
        let mut tasks = Vec::new();

        for id in task_ids {
            if let Some(node) = graph.get_node(id) {
                let task_info = extract_task_info(node, graph);
                estimated_total_turns = estimated_total_turns.saturating_add(task_info.estimated_turns);
                tasks.push(task_info);
                total_tasks += 1;
            }
        }

        // Detect checkpoint from project files (can be overridden by config)
        let checkpoint = detect_default_checkpoint(graph);

        execution_layers.push(ExecutionLayer {
            index,
            tasks,
            checkpoint,
        });
    }

    Ok(ExecutionPlan {
        layers: execution_layers,
        critical_path: cp,
        total_tasks,
        estimated_total_turns,
    })
}

/// Extract TaskInfo from a graph Node.
fn extract_task_info(node: &crate::graph::Node, graph: &Graph) -> TaskInfo {
    let description = node.description.clone().unwrap_or_default();

    // Extract metadata fields
    let verify = node.metadata.get("verify")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let estimated_turns = node.metadata.get("estimated_turns")
        .and_then(|v| v.as_u64())
        .unwrap_or(15) as u32;

    let design_ref = node.metadata.get("design_ref")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let satisfies = node.metadata.get("satisfies")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let goals = node.metadata.get("goals")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Get depends_on from edges
    let depends_on: Vec<String> = graph.edges.iter()
        .filter(|e| e.from == node.id && e.relation == "depends_on")
        .map(|e| e.to.clone())
        .collect();

    TaskInfo {
        id: node.id.clone(),
        title: node.title.clone(),
        description,
        goals,
        verify,
        estimated_turns,
        depends_on,
        design_ref,
        satisfies,
    }
}

/// Auto-detect checkpoint command from project files.
fn detect_default_checkpoint(graph: &Graph) -> Option<String> {
    // Look for project-level metadata hint
    if let Some(ref project) = graph.project {
        if let Some(ref desc) = project.description {
            let desc_lower = desc.to_lowercase();
            if desc_lower.contains("rust") || desc_lower.contains("cargo") {
                return Some("cargo check && cargo test".to_string());
            }
            if desc_lower.contains("node") || desc_lower.contains("typescript") || desc_lower.contains("javascript") {
                return Some("npm test".to_string());
            }
            if desc_lower.contains("python") {
                return Some("pytest".to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, Edge, NodeStatus};

    fn make_task(id: &str, title: &str) -> Node {
        let mut n = Node::new(id, title);
        n.node_type = Some("task".to_string());
        n
    }

    fn make_task_with_meta(id: &str, title: &str, turns: u32, verify: &str) -> Node {
        let mut n = make_task(id, title);
        n.metadata.insert("estimated_turns".to_string(), serde_json::json!(turns));
        n.metadata.insert("verify".to_string(), serde_json::json!(verify));
        n
    }

    #[test]
    fn test_create_plan_basic() {
        let mut graph = Graph::new();
        graph.add_node(make_task_with_meta("a", "Task A", 10, "cargo test"));
        graph.add_node(make_task_with_meta("b", "Task B", 20, "cargo test"));
        graph.add_edge(Edge::depends_on("b", "a"));

        let plan = create_plan(&graph).unwrap();
        assert_eq!(plan.total_tasks, 2);
        assert_eq!(plan.estimated_total_turns, 30);
        assert_eq!(plan.layers.len(), 2);
        assert_eq!(plan.layers[0].tasks[0].id, "a");
        assert_eq!(plan.layers[1].tasks[0].id, "b");
    }

    #[test]
    fn test_create_plan_skips_done() {
        let mut graph = Graph::new();
        let mut done = make_task("a", "Done Task");
        done.status = NodeStatus::Done;
        graph.add_node(done);
        graph.add_node(make_task("b", "Pending Task"));
        graph.add_edge(Edge::depends_on("b", "a"));

        let plan = create_plan(&graph).unwrap();
        assert_eq!(plan.total_tasks, 1);
        assert_eq!(plan.layers[0].tasks[0].id, "b");
    }

    #[test]
    fn test_create_plan_idempotent() {
        let mut graph = Graph::new();
        graph.add_node(make_task("a", "A"));
        graph.add_node(make_task("b", "B"));
        graph.add_node(make_task("c", "C"));
        graph.add_edge(Edge::depends_on("b", "a"));
        graph.add_edge(Edge::depends_on("c", "a"));

        let plan1 = serde_json::to_string(&create_plan(&graph).unwrap()).unwrap();
        let plan2 = serde_json::to_string(&create_plan(&graph).unwrap()).unwrap();
        assert_eq!(plan1, plan2, "create_plan must be idempotent (GUARD-1)");
    }

    #[test]
    fn test_create_plan_extracts_metadata() {
        let mut graph = Graph::new();
        let mut task = make_task("auth", "Implement auth");
        task.metadata.insert("verify".to_string(), serde_json::json!("cargo test --test auth"));
        task.metadata.insert("estimated_turns".to_string(), serde_json::json!(20));
        task.metadata.insert("design_ref".to_string(), serde_json::json!("3.2"));
        task.metadata.insert("satisfies".to_string(), serde_json::json!(["GOAL-1.1", "GOAL-1.2"]));
        graph.add_node(task);

        let plan = create_plan(&graph).unwrap();
        let info = &plan.layers[0].tasks[0];
        assert_eq!(info.verify, Some("cargo test --test auth".to_string()));
        assert_eq!(info.estimated_turns, 20);
        assert_eq!(info.design_ref, Some("3.2".to_string()));
        assert_eq!(info.satisfies, vec!["GOAL-1.1", "GOAL-1.2"]);
    }
}
