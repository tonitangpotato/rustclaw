//! Execution state — persistent state for harness execution.
//!
//! The execution state tracks the current status of a running or paused
//! execution, pending approvals, active tasks, and cancellation requests.
//! It's persisted to `.gid/execution-state.json` and read by CLI commands
//! like `gid approve` and `gid stop`.

use std::path::Path;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Path to the execution state file (relative to .gid directory).
pub const EXECUTION_STATE_FILENAME: &str = "execution-state.json";

/// Persistent execution state.
///
/// This is the source of truth for the current execution status,
/// read/written by both the scheduler and CLI commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionState {
    /// Current execution status.
    pub status: ExecutionStatus,
    /// IDs of currently executing tasks.
    pub active_tasks: Vec<String>,
    /// Pending approval requests (for manual approval mode).
    pub pending_approvals: Vec<ApprovalRequest>,
    /// Whether cancellation has been requested.
    pub cancel_requested: bool,
    /// When the state was last updated.
    pub last_updated: DateTime<Utc>,
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self {
            status: ExecutionStatus::Idle,
            active_tasks: Vec::new(),
            pending_approvals: Vec::new(),
            cancel_requested: false,
            last_updated: Utc::now(),
        }
    }
}

impl ExecutionState {
    /// Create a new execution state with the given status.
    pub fn new(status: ExecutionStatus) -> Self {
        Self {
            status,
            active_tasks: Vec::new(),
            pending_approvals: Vec::new(),
            cancel_requested: false,
            last_updated: Utc::now(),
        }
    }

    /// Load execution state from `.gid/execution-state.json`.
    ///
    /// Returns default state if the file doesn't exist.
    pub fn load(gid_dir: &Path) -> Result<Self> {
        let path = gid_dir.join(EXECUTION_STATE_FILENAME);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Save execution state to `.gid/execution-state.json`.
    pub fn save(&mut self, gid_dir: &Path) -> Result<()> {
        self.last_updated = Utc::now();
        let path = gid_dir.join(EXECUTION_STATE_FILENAME);

        // Ensure .gid directory exists
        std::fs::create_dir_all(gid_dir)
            .with_context(|| format!("Failed to create {}", gid_dir.display()))?;

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize execution state")?;

        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Set status to Running and clear cancel flag.
    pub fn start_running(&mut self) {
        self.status = ExecutionStatus::Running;
        self.cancel_requested = false;
    }

    /// Set active tasks list.
    pub fn set_active_tasks(&mut self, tasks: Vec<String>) {
        self.active_tasks = tasks;
    }

    /// Add a pending approval request.
    pub fn add_approval_request(&mut self, layer_index: usize, message: String) {
        self.pending_approvals.push(ApprovalRequest {
            layer_index,
            message,
            requested_at: Utc::now(),
        });
        self.status = ExecutionStatus::WaitingApproval;
    }

    /// Approve all pending requests and set status back to Running.
    ///
    /// Returns the list of approved requests.
    pub fn approve(&mut self) -> Vec<ApprovalRequest> {
        let approved = std::mem::take(&mut self.pending_approvals);
        if !approved.is_empty() {
            self.status = ExecutionStatus::Approved;
        }
        approved
    }

    /// Request cancellation.
    pub fn request_cancel(&mut self) {
        self.cancel_requested = true;
    }

    /// Check if cancellation was requested.
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested
    }

    /// Mark execution as completed.
    pub fn complete(&mut self) {
        self.status = ExecutionStatus::Completed;
        self.active_tasks.clear();
        self.pending_approvals.clear();
    }

    /// Mark execution as cancelled.
    pub fn mark_cancelled(&mut self) {
        self.status = ExecutionStatus::Cancelled;
        self.active_tasks.clear();
    }

    /// Mark execution as paused.
    pub fn pause(&mut self) {
        self.status = ExecutionStatus::Paused;
    }
}

/// Execution status enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// No execution in progress.
    Idle,
    /// Execution is running.
    Running,
    /// Waiting for user approval (manual mode).
    WaitingApproval,
    /// User approved, ready to continue.
    Approved,
    /// Execution paused (can be resumed).
    Paused,
    /// Execution completed successfully.
    Completed,
    /// Execution was cancelled by user.
    Cancelled,
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionStatus::Idle => write!(f, "idle"),
            ExecutionStatus::Running => write!(f, "running"),
            ExecutionStatus::WaitingApproval => write!(f, "waiting_approval"),
            ExecutionStatus::Approved => write!(f, "approved"),
            ExecutionStatus::Paused => write!(f, "paused"),
            ExecutionStatus::Completed => write!(f, "completed"),
            ExecutionStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A pending approval request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Layer index that requested approval.
    pub layer_index: usize,
    /// Human-readable message about what's being approved.
    pub message: String,
    /// When the request was created.
    pub requested_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_state() {
        let state = ExecutionState::default();
        assert_eq!(state.status, ExecutionStatus::Idle);
        assert!(state.active_tasks.is_empty());
        assert!(state.pending_approvals.is_empty());
        assert!(!state.cancel_requested);
    }

    #[test]
    fn test_load_nonexistent() {
        let tmp = tempdir().unwrap();
        let state = ExecutionState::load(tmp.path()).unwrap();
        assert_eq!(state.status, ExecutionStatus::Idle);
    }

    #[test]
    fn test_save_and_load() {
        let tmp = tempdir().unwrap();
        let gid_dir = tmp.path().join(".gid");

        let mut state = ExecutionState::new(ExecutionStatus::Running);
        state.active_tasks = vec!["task-1".to_string(), "task-2".to_string()];
        state.save(&gid_dir).unwrap();

        let loaded = ExecutionState::load(&gid_dir).unwrap();
        assert_eq!(loaded.status, ExecutionStatus::Running);
        assert_eq!(loaded.active_tasks, vec!["task-1", "task-2"]);
    }

    #[test]
    fn test_approval_workflow() {
        let mut state = ExecutionState::new(ExecutionStatus::Running);
        
        // Add approval request
        state.add_approval_request(1, "Review layer 1 results".to_string());
        assert_eq!(state.status, ExecutionStatus::WaitingApproval);
        assert_eq!(state.pending_approvals.len(), 1);

        // Approve
        let approved = state.approve();
        assert_eq!(approved.len(), 1);
        assert_eq!(approved[0].layer_index, 1);
        assert_eq!(state.status, ExecutionStatus::Approved);
        assert!(state.pending_approvals.is_empty());
    }

    #[test]
    fn test_cancel_workflow() {
        let mut state = ExecutionState::new(ExecutionStatus::Running);
        state.active_tasks = vec!["task-1".to_string()];
        
        assert!(!state.is_cancel_requested());
        
        state.request_cancel();
        assert!(state.is_cancel_requested());
        
        state.mark_cancelled();
        assert_eq!(state.status, ExecutionStatus::Cancelled);
        assert!(state.active_tasks.is_empty());
    }

    #[test]
    fn test_complete_workflow() {
        let mut state = ExecutionState::new(ExecutionStatus::Running);
        state.active_tasks = vec!["task-1".to_string()];
        state.add_approval_request(0, "test".to_string());

        state.complete();
        assert_eq!(state.status, ExecutionStatus::Completed);
        assert!(state.active_tasks.is_empty());
        assert!(state.pending_approvals.is_empty());
    }

    #[test]
    fn test_status_display() {
        assert_eq!(ExecutionStatus::Idle.to_string(), "idle");
        assert_eq!(ExecutionStatus::Running.to_string(), "running");
        assert_eq!(ExecutionStatus::WaitingApproval.to_string(), "waiting_approval");
        assert_eq!(ExecutionStatus::Approved.to_string(), "approved");
        assert_eq!(ExecutionStatus::Cancelled.to_string(), "cancelled");
    }
}
