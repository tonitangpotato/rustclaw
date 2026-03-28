//! CEO Multi-Agent Orchestration.
//!
//! The orchestrator manages specialist agents, assigns tasks based on roles,
//! tracks task lifecycle, and enforces per-agent token budgets.
//!
//! CEO Pattern:
//! - CEO (main agent) reads GID graph → finds unblocked tasks
//! - Assigns tasks to idle specialists based on role matching
//! - Specialists work in their own workspace/branch
//! - CEO merges results when complete

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::AgentRunner;
use crate::config::{AgentConfig, OrchestratorConfig, SpecialistConfig};

/// Status of a specialist agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agent is idle and ready for work.
    Idle,
    /// Agent is working on a specific task.
    Working(String), // task_id
    /// Agent is paused (manual intervention).
    Paused,
    /// Agent encountered an error.
    Error(String),
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Working(task_id) => write!(f, "working:{}", task_id),
            AgentStatus::Paused => write!(f, "paused"),
            AgentStatus::Error(msg) => write!(f, "error:{}", msg),
        }
    }
}

/// A specialist agent managed by the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialistAgent {
    /// Unique agent ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Agent role for task matching (e.g., "builder", "visibility", "trading").
    pub role: String,
    /// Optional git worktree path for isolated workspace.
    pub workspace: Option<String>,
    /// Model override (e.g., "claude-sonnet-4-5").
    pub model: Option<String>,
    /// Token budget for this agent (None = unlimited).
    pub budget_tokens: Option<u64>,
    /// Tokens used so far.
    pub budget_used: u64,
    /// Current status.
    pub status: AgentStatus,
    /// Maximum iterations for the agentic loop.
    pub max_iterations: u32,
}

impl SpecialistAgent {
    /// Create from config.
    pub fn from_config(cfg: &SpecialistConfig) -> Self {
        Self {
            id: cfg.id.clone(),
            name: cfg.name.clone().unwrap_or_else(|| cfg.id.clone()),
            role: cfg.role.clone(),
            workspace: cfg.workspace.clone(),
            model: cfg.model.clone(),
            budget_tokens: cfg.budget_tokens,
            budget_used: 0,
            status: AgentStatus::Idle,
            max_iterations: cfg.max_iterations,
        }
    }

    /// Check if agent can accept a task.
    pub fn can_accept_task(&self) -> bool {
        matches!(self.status, AgentStatus::Idle)
            && self.is_within_budget()
    }

    /// Check if agent is within token budget.
    pub fn is_within_budget(&self) -> bool {
        match self.budget_tokens {
            Some(budget) => self.budget_used < budget,
            None => true,
        }
    }

    /// Add token usage.
    pub fn add_usage(&mut self, tokens: u64) {
        self.budget_used += tokens;
    }

    /// Convert to AgentConfig for spawning.
    pub fn to_agent_config(&self) -> AgentConfig {
        AgentConfig {
            id: self.id.clone(),
            name: Some(self.name.clone()),
            workspace: self.workspace.clone(),
            model: self.model.clone(),
            default: false,
        }
    }
}

/// Status of a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task is waiting to be assigned.
    Pending,
    /// Task is being worked on.
    InProgress,
    /// Task completed successfully.
    Done,
    /// Task failed.
    Failed,
    /// Task was cancelled.
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Done => write!(f, "done"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A task to be executed by a specialist agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task ID.
    pub id: String,
    /// Task description (prompt for the agent).
    pub description: String,
    /// Assigned agent ID (None if pending).
    pub assigned_to: Option<String>,
    /// Priority (0 = highest).
    pub priority: u8,
    /// Current status.
    pub status: TaskStatus,
    /// Roles that can handle this task.
    pub roles: Vec<String>,
    /// Token budget for this task (None = unlimited).
    pub budget_tokens: Option<u64>,
    /// When the task was created.
    pub created_at: DateTime<Utc>,
    /// When the task was completed (if done).
    pub completed_at: Option<DateTime<Utc>>,
    /// Result from the agent.
    pub result: Option<String>,
    /// Error message (if failed).
    pub error: Option<String>,
}

impl Task {
    /// Create a new task.
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            assigned_to: None,
            priority: 100, // Default middle priority
            status: TaskStatus::Pending,
            roles: Vec::new(),
            budget_tokens: None,
            created_at: Utc::now(),
            completed_at: None,
            result: None,
            error: None,
        }
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Set roles that can handle this task.
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    /// Set token budget.
    pub fn with_budget(mut self, budget: u64) -> Self {
        self.budget_tokens = Some(budget);
        self
    }
}

/// Result of a completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub agent_id: String,
    pub success: bool,
    pub output: String,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

/// The CEO orchestrator that manages specialist agents and tasks.
pub struct Orchestrator {
    /// Registered specialist agents.
    agents: HashMap<String, SpecialistAgent>,
    /// Task queue (priority-sorted).
    task_queue: VecDeque<Task>,
    /// All tasks (including completed).
    tasks: HashMap<String, Task>,
    /// In-flight tasks (task_id -> start_time).
    in_flight: HashMap<String, std::time::Instant>,
    /// Configuration.
    config: OrchestratorConfig,
}

impl Orchestrator {
    /// Create a new orchestrator.
    pub fn new(config: OrchestratorConfig) -> Self {
        let mut orchestrator = Self {
            agents: HashMap::new(),
            task_queue: VecDeque::new(),
            tasks: HashMap::new(),
            in_flight: HashMap::new(),
            config,
        };

        // Load specialists from config
        for spec_cfg in &orchestrator.config.specialists.clone() {
            let agent = SpecialistAgent::from_config(spec_cfg);
            orchestrator.agents.insert(agent.id.clone(), agent);
        }

        tracing::info!(
            "Orchestrator initialized with {} specialists",
            orchestrator.agents.len()
        );

        orchestrator
    }

    /// Add a specialist agent.
    pub fn add_agent(&mut self, agent: SpecialistAgent) {
        tracing::info!("Adding specialist agent: {} (role: {})", agent.id, agent.role);
        self.agents.insert(agent.id.clone(), agent);
    }

    /// Remove a specialist agent.
    pub fn remove_agent(&mut self, agent_id: &str) -> Option<SpecialistAgent> {
        self.agents.remove(agent_id)
    }

    /// Get a specialist agent.
    pub fn get_agent(&self, agent_id: &str) -> Option<&SpecialistAgent> {
        self.agents.get(agent_id)
    }

    /// Get a mutable specialist agent.
    pub fn get_agent_mut(&mut self, agent_id: &str) -> Option<&mut SpecialistAgent> {
        self.agents.get_mut(agent_id)
    }

    /// Submit a task to the queue.
    pub fn submit_task(&mut self, task: Task) {
        tracing::info!(
            "Task submitted: {} (priority: {}, roles: {:?})",
            task.id,
            task.priority,
            task.roles
        );

        let id = task.id.clone();
        self.tasks.insert(id.clone(), task.clone());
        self.task_queue.push_back(task);

        // Sort by priority (0 = highest)
        self.task_queue
            .make_contiguous()
            .sort_by_key(|t| t.priority);
    }

    /// Cancel a pending task.
    pub fn cancel_task(&mut self, task_id: &str) -> bool {
        if let Some(task) = self.tasks.get_mut(task_id) {
            if task.status == TaskStatus::Pending {
                task.status = TaskStatus::Cancelled;
                // Remove from queue
                self.task_queue.retain(|t| t.id != task_id);
                tracing::info!("Task cancelled: {}", task_id);
                return true;
            }
        }
        false
    }

    /// Get a task by ID.
    pub fn get_task(&self, task_id: &str) -> Option<&Task> {
        self.tasks.get(task_id)
    }

    /// Find an idle agent that can handle a task.
    fn find_agent_for_task(&self, task: &Task) -> Option<&SpecialistAgent> {
        // If task has specific roles, match them
        if !task.roles.is_empty() {
            self.agents
                .values()
                .filter(|a| a.can_accept_task())
                .find(|a| task.roles.contains(&a.role))
        } else {
            // No role constraint - pick any idle agent
            self.agents.values().find(|a| a.can_accept_task())
        }
    }

    /// Count currently in-progress tasks.
    fn in_progress_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count()
    }

    /// Run one tick of the orchestrator loop.
    ///
    /// This:
    /// 1. Assigns pending tasks to idle agents
    /// 2. Checks completed tasks
    /// 3. Returns completed task results
    pub async fn tick(&mut self, runner: &AgentRunner) -> Vec<TaskResult> {
        let mut results = Vec::new();

        // Don't exceed max concurrent
        let current_in_progress = self.in_progress_count();
        let slots_available = self
            .config
            .max_concurrent
            .saturating_sub(current_in_progress as u32) as usize;

        if slots_available == 0 {
            tracing::debug!("Orchestrator: max concurrent reached, skipping assignment");
            return results;
        }

        // Collect tasks to assign (avoid borrowing issues)
        let tasks_to_assign: Vec<String> = self
            .task_queue
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .take(slots_available)
            .map(|t| t.id.clone())
            .collect();

        for task_id in tasks_to_assign {
            // Find task and matching agent
            let task_roles = self
                .tasks
                .get(&task_id)
                .map(|t| t.roles.clone())
                .unwrap_or_default();

            let agent_id = {
                let dummy_task = Task {
                    id: task_id.clone(),
                    description: String::new(),
                    assigned_to: None,
                    priority: 0,
                    status: TaskStatus::Pending,
                    roles: task_roles,
                    budget_tokens: None,
                    created_at: Utc::now(),
                    completed_at: None,
                    result: None,
                    error: None,
                };

                self.find_agent_for_task(&dummy_task).map(|a| a.id.clone())
            };

            if let Some(agent_id) = agent_id {
                // Assign task to agent
                if let Some(task) = self.tasks.get_mut(&task_id) {
                    task.assigned_to = Some(agent_id.clone());
                    task.status = TaskStatus::InProgress;
                }

                if let Some(agent) = self.agents.get_mut(&agent_id) {
                    agent.status = AgentStatus::Working(task_id.clone());
                }

                // Remove from pending queue
                self.task_queue.retain(|t| t.id != task_id);
                self.in_flight
                    .insert(task_id.clone(), std::time::Instant::now());

                tracing::info!("Task {} assigned to agent {}", task_id, agent_id);

                // Execute task
                let task_description = self
                    .tasks
                    .get(&task_id)
                    .map(|t| t.description.clone())
                    .unwrap_or_default();

                let (agent_config, max_iterations) = self
                    .agents
                    .get(&agent_id)
                    .map(|a| (a.to_agent_config(), a.max_iterations))
                    .unwrap();

                let result = self
                    .execute_task(runner, &agent_config, &task_id, &task_description, max_iterations)
                    .await;

                results.push(result);
            }
        }

        results
    }

    /// Execute a task using a specialist agent.
    async fn execute_task(
        &mut self,
        runner: &AgentRunner,
        agent_config: &AgentConfig,
        task_id: &str,
        description: &str,
        max_iterations: u32,
    ) -> TaskResult {
        let start = std::time::Instant::now();
        let agent_id = agent_config.id.clone();

        tracing::info!(
            "Executing task {} with agent {} (workspace: {:?}, model: {:?}, max_iterations: {})",
            task_id,
            agent_id,
            agent_config.workspace,
            agent_config.model,
            max_iterations
        );

        // Spawn sub-agent with max_iterations and process
        let result = match runner.spawn_agent_with_options(agent_config, max_iterations) {
            Ok(subagent) => {
                match runner
                    .process_with_subagent(&subagent, description, Some(task_id))
                    .await
                {
                    Ok(output) => {
                        tracing::info!("Task {} completed successfully", task_id);
                        TaskResult {
                            task_id: task_id.to_string(),
                            agent_id: agent_id.clone(),
                            success: true,
                            output,
                            tokens_used: 0, // TODO: track actual usage
                            duration_ms: start.elapsed().as_millis() as u64,
                        }
                    }
                    Err(e) => {
                        tracing::error!("Task {} failed: {}", task_id, e);
                        TaskResult {
                            task_id: task_id.to_string(),
                            agent_id: agent_id.clone(),
                            success: false,
                            output: format!("Error: {}", e),
                            tokens_used: 0,
                            duration_ms: start.elapsed().as_millis() as u64,
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to spawn agent for task {}: {}", task_id, e);
                TaskResult {
                    task_id: task_id.to_string(),
                    agent_id: agent_id.clone(),
                    success: false,
                    output: format!("Spawn error: {}", e),
                    tokens_used: 0,
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
        };

        // Update task state
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.completed_at = Some(Utc::now());
            if result.success {
                task.status = TaskStatus::Done;
                task.result = Some(result.output.clone());
            } else {
                task.status = TaskStatus::Failed;
                task.error = Some(result.output.clone());
            }
        }

        // Update agent state
        if let Some(agent) = self.agents.get_mut(&agent_id) {
            agent.status = AgentStatus::Idle;
            agent.add_usage(result.tokens_used);
        }

        // Remove from in-flight
        self.in_flight.remove(task_id);

        result
    }

    /// Get status of all agents.
    pub fn agent_status(&self) -> Vec<(String, AgentStatus)> {
        self.agents
            .iter()
            .map(|(id, a)| (id.clone(), a.status.clone()))
            .collect()
    }

    /// Get status of all tasks.
    pub fn task_status(&self) -> Vec<(String, TaskStatus)> {
        self.tasks
            .iter()
            .map(|(id, t)| (id.clone(), t.status.clone()))
            .collect()
    }

    /// Get pending task count.
    pub fn pending_count(&self) -> usize {
        self.task_queue.len()
    }

    /// Get completed task count.
    pub fn completed_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.status == TaskStatus::Done)
            .count()
    }

    /// Get failed task count.
    pub fn failed_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|t| t.status == TaskStatus::Failed)
            .count()
    }
}

/// Shared orchestrator handle (thread-safe).
pub type SharedOrchestrator = Arc<RwLock<Orchestrator>>;

/// Create a shared orchestrator.
pub fn create_orchestrator(config: OrchestratorConfig) -> SharedOrchestrator {
    Arc::new(RwLock::new(Orchestrator::new(config)))
}

/// Start the orchestrator tick loop.
pub async fn start_orchestrator_loop(
    orchestrator: SharedOrchestrator,
    runner: Arc<AgentRunner>,
    tick_interval_secs: u64,
) {
    if tick_interval_secs == 0 {
        tracing::info!("Orchestrator tick loop disabled (interval=0)");
        return;
    }

    tracing::info!(
        "Starting orchestrator tick loop (interval: {}s)",
        tick_interval_secs
    );

    let interval = std::time::Duration::from_secs(tick_interval_secs);

    loop {
        tokio::time::sleep(interval).await;

        let results = {
            let mut orch = orchestrator.write().await;
            orch.tick(&runner).await
        };

        if !results.is_empty() {
            tracing::info!(
                "Orchestrator tick: {} task(s) completed",
                results.len()
            );
            for result in &results {
                tracing::info!(
                    "  Task {} (agent: {}): {} in {}ms",
                    result.task_id,
                    result.agent_id,
                    if result.success { "success" } else { "failed" },
                    result.duration_ms
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_priority_sorting() {
        let cfg = OrchestratorConfig {
            enabled: true,
            tick_interval: 60,
            max_concurrent: 3,
            specialists: vec![],
        };
        let mut orch = Orchestrator::new(cfg);

        orch.submit_task(Task::new("t1", "low priority").with_priority(100));
        orch.submit_task(Task::new("t2", "high priority").with_priority(0));
        orch.submit_task(Task::new("t3", "medium priority").with_priority(50));

        let ids: Vec<&str> = orch.task_queue.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["t2", "t3", "t1"]);
    }

    #[test]
    fn test_agent_budget() {
        let mut agent = SpecialistAgent {
            id: "test".into(),
            name: "Test".into(),
            role: "builder".into(),
            workspace: None,
            model: None,
            budget_tokens: Some(1000),
            budget_used: 0,
            status: AgentStatus::Idle,
            max_iterations: 25,
        };

        assert!(agent.is_within_budget());
        assert!(agent.can_accept_task());

        agent.add_usage(1000);
        assert!(!agent.is_within_budget());
        assert!(!agent.can_accept_task());
    }
}
