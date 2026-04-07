//! Execution log reader — read and analyze execution-log.jsonl.
//!
//! Provides incremental reading (since timestamp) and summary statistics
//! for the harness execution telemetry log.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::types::ExecutionEvent;

/// Default log filename within .gid directory.
pub const EXECUTION_LOG_FILENAME: &str = "execution-log.jsonl";

/// Reader for execution-log.jsonl telemetry files.
///
/// Supports reading all events, incremental reads since a timestamp,
/// and computing summary statistics.
pub struct ExecutionLogReader {
    /// Path to the JSONL log file.
    pub log_path: PathBuf,
}

impl ExecutionLogReader {
    /// Create a new log reader for the given path.
    pub fn new(log_path: impl Into<PathBuf>) -> Self {
        Self {
            log_path: log_path.into(),
        }
    }

    /// Create a log reader for the default path within a .gid directory.
    pub fn for_gid_dir(gid_dir: &Path) -> Self {
        Self::new(gid_dir.join(EXECUTION_LOG_FILENAME))
    }

    /// Check if the log file exists.
    pub fn exists(&self) -> bool {
        self.log_path.exists()
    }

    /// Read all events from the log file.
    ///
    /// Returns an empty vec if the file doesn't exist.
    pub fn read_all(&self) -> Result<Vec<ExecutionEvent>> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&self.log_path)
            .with_context(|| format!("Failed to read {}", self.log_path.display()))?;

        let mut events = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<ExecutionEvent>(line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    tracing::warn!(
                        line = line_num + 1,
                        error = %e,
                        "Skipping malformed log line"
                    );
                }
            }
        }

        Ok(events)
    }

    /// Read events since a given timestamp (for incremental watching).
    ///
    /// Returns all events with timestamp >= since.
    pub fn read_since(&self, since: DateTime<Utc>) -> Result<Vec<ExecutionEvent>> {
        let all_events = self.read_all()?;
        
        let filtered: Vec<ExecutionEvent> = all_events
            .into_iter()
            .filter(|event| event_timestamp(event).map(|ts| ts >= since).unwrap_or(false))
            .collect();

        Ok(filtered)
    }

    /// Compute summary statistics from the log file.
    pub fn summary(&self) -> Result<ExecutionSummary> {
        let events = self.read_all()?;
        
        let mut total_tasks: usize = 0;
        let mut completed: usize = 0;
        let mut failed: usize = 0;
        let mut total_input_tokens: u64 = 0;
        let total_output_tokens: u64 = 0;
        let mut total_duration_secs: f64 = 0.0;
        let mut tasks: Vec<TaskSummary> = Vec::new();

        for event in &events {
            match event {
                ExecutionEvent::Plan { total_tasks: t, .. } => {
                    total_tasks = *t;
                }
                ExecutionEvent::TaskDone { task_id, turns, tokens, duration_s, verify, .. } => {
                    completed += 1;
                    total_input_tokens += tokens; // We only have total tokens, not split
                    total_duration_secs += *duration_s as f64;
                    
                    tasks.push(TaskSummary {
                        task_id: task_id.clone(),
                        status: TaskStatus::Completed,
                        turns_used: *turns,
                        tokens_used: *tokens,
                        duration_secs: *duration_s as f64,
                        verify_result: Some(verify.clone()),
                        failure_reason: None,
                    });
                }
                ExecutionEvent::TaskFailed { task_id, reason, turns, .. } => {
                    failed += 1;
                    
                    tasks.push(TaskSummary {
                        task_id: task_id.clone(),
                        status: TaskStatus::Failed,
                        turns_used: *turns,
                        tokens_used: 0,
                        duration_secs: 0.0,
                        verify_result: None,
                        failure_reason: Some(reason.clone()),
                    });
                }
                ExecutionEvent::Complete { duration_s, total_tokens, .. } => {
                    // Use the final duration from complete event
                    total_duration_secs = *duration_s as f64;
                    // Use total tokens if we haven't summed them
                    if total_input_tokens == 0 {
                        total_input_tokens = *total_tokens;
                    }
                }
                _ => {}
            }
        }

        Ok(ExecutionSummary {
            total_tasks,
            completed,
            failed,
            total_input_tokens,
            total_output_tokens,
            total_duration_secs,
            tasks,
        })
    }

    /// Get the timestamp of the last event in the log.
    pub fn last_event_time(&self) -> Result<Option<DateTime<Utc>>> {
        let events = self.read_all()?;
        Ok(events.last().and_then(event_timestamp))
    }
}

/// Summary of execution statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Total number of tasks in the plan.
    pub total_tasks: usize,
    /// Number of tasks completed successfully.
    pub completed: usize,
    /// Number of tasks that failed.
    pub failed: usize,
    /// Total input tokens consumed (approximation from total tokens).
    pub total_input_tokens: u64,
    /// Total output tokens consumed (currently 0, for future use).
    pub total_output_tokens: u64,
    /// Total execution duration in seconds.
    pub total_duration_secs: f64,
    /// Per-task summaries.
    pub tasks: Vec<TaskSummary>,
}

impl ExecutionSummary {
    /// Check if execution is complete (all tasks done or failed).
    pub fn is_complete(&self) -> bool {
        self.completed + self.failed >= self.total_tasks && self.total_tasks > 0
    }

    /// Get success rate as a percentage.
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        (self.completed as f64 / self.total_tasks as f64) * 100.0
    }

    /// Average turns per completed task.
    pub fn avg_turns_per_task(&self) -> f64 {
        let completed_tasks: Vec<_> = self.tasks.iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .collect();
        
        if completed_tasks.is_empty() {
            return 0.0;
        }

        let total_turns: u32 = completed_tasks.iter().map(|t| t.turns_used).sum();
        total_turns as f64 / completed_tasks.len() as f64
    }
}

/// Summary for a single task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    /// Task ID.
    pub task_id: String,
    /// Final status.
    pub status: TaskStatus,
    /// Number of turns used.
    pub turns_used: u32,
    /// Tokens consumed.
    pub tokens_used: u64,
    /// Duration in seconds.
    pub duration_secs: f64,
    /// Verify command result (for completed tasks).
    pub verify_result: Option<String>,
    /// Failure reason (for failed tasks).
    pub failure_reason: Option<String>,
}

/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
}

/// Extract timestamp from an execution event.
fn event_timestamp(event: &ExecutionEvent) -> Option<DateTime<Utc>> {
    match event {
        ExecutionEvent::Plan { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::TaskStart { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::TaskDone { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::TaskFailed { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::Checkpoint { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::Replan { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::Cancel { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::Advise { timestamp, .. } => Some(*timestamp),
        ExecutionEvent::Complete { timestamp, .. } => Some(*timestamp),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase Approval Messages
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate human-readable approval message for a phase gate.
///
/// Called by the scheduler when reaching an approval gate in mixed mode.
pub fn generate_approval_message(
    phase: ApprovalPhase,
    context: &ApprovalContext,
) -> String {
    match phase {
        ApprovalPhase::Requirements => {
            format!(
                "📋 Requirements Complete\n\n\
                {} goals defined in requirements.md.\n\n\
                Review the requirements and confirm to proceed to design.\n\n\
                Run `gid approve` to continue.",
                context.goal_count
            )
        }
        ApprovalPhase::Design => {
            format!(
                "🎨 Design Complete\n\n\
                Design documented in design.md.\n\
                {} sections covering architecture and implementation details.\n\n\
                Review the design and confirm to proceed to task decomposition.\n\n\
                Run `gid approve` to continue.",
                context.design_section_count
            )
        }
        ApprovalPhase::Graph => {
            format!(
                "📊 Task Graph Ready\n\n\
                {} tasks organized in {} parallel layers.\n\
                Critical path: {} tasks.\n\
                Estimated total turns: {}.\n\n\
                Review the execution plan:\n\
                  `gid plan`\n\n\
                Confirm to start automated execution.\n\n\
                Run `gid approve` to continue.",
                context.task_count,
                context.layer_count,
                context.critical_path_length,
                context.estimated_turns
            )
        }
    }
}

/// Phase that requires approval in mixed mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPhase {
    /// After Phase 2: requirements.md written.
    Requirements,
    /// After Phase 3: design.md written.
    Design,
    /// After Phase 4: graph decomposed into tasks.
    Graph,
}

/// Context for generating approval messages.
#[derive(Debug, Clone, Default)]
pub struct ApprovalContext {
    /// Number of goals in requirements (for Requirements phase).
    pub goal_count: usize,
    /// Number of sections in design doc (for Design phase).
    pub design_section_count: usize,
    /// Number of tasks in the graph (for Graph phase).
    pub task_count: usize,
    /// Number of parallel layers (for Graph phase).
    pub layer_count: usize,
    /// Length of critical path (for Graph phase).
    pub critical_path_length: usize,
    /// Estimated total turns (for Graph phase).
    pub estimated_turns: u32,
}

impl ApprovalContext {
    /// Create context for requirements phase.
    pub fn for_requirements(goal_count: usize) -> Self {
        Self {
            goal_count,
            ..Default::default()
        }
    }

    /// Create context for design phase.
    pub fn for_design(section_count: usize) -> Self {
        Self {
            design_section_count: section_count,
            ..Default::default()
        }
    }

    /// Create context for graph phase from execution plan.
    pub fn for_graph(
        task_count: usize,
        layer_count: usize,
        critical_path_length: usize,
        estimated_turns: u32,
    ) -> Self {
        Self {
            task_count,
            layer_count,
            critical_path_length,
            estimated_turns,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    fn write_test_log(path: &Path, events: &[ExecutionEvent]) {
        let content: String = events.iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_read_all_empty() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("empty.jsonl");
        std::fs::write(&log_path, "").unwrap();

        let reader = ExecutionLogReader::new(&log_path);
        let events = reader.read_all().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_read_all_nonexistent() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("nonexistent.jsonl");

        let reader = ExecutionLogReader::new(&log_path);
        let events = reader.read_all().unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_read_all_with_events() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("test.jsonl");

        let events = vec![
            ExecutionEvent::Plan {
                total_tasks: 5,
                layers: 3,
                timestamp: Utc::now(),
            },
            ExecutionEvent::TaskStart {
                task_id: "task-1".to_string(),
                layer: 0,
                timestamp: Utc::now(),
            },
            ExecutionEvent::TaskDone {
                task_id: "task-1".to_string(),
                turns: 10,
                tokens: 5000,
                duration_s: 60,
                verify: "pass".to_string(),
                timestamp: Utc::now(),
            },
        ];

        write_test_log(&log_path, &events);

        let reader = ExecutionLogReader::new(&log_path);
        let read_events = reader.read_all().unwrap();
        assert_eq!(read_events.len(), 3);
    }

    #[test]
    fn test_read_since() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("since.jsonl");

        let now = Utc::now();
        let earlier = now - chrono::Duration::hours(1);
        let later = now + chrono::Duration::hours(1);

        let events = vec![
            ExecutionEvent::Plan {
                total_tasks: 2,
                layers: 1,
                timestamp: earlier,
            },
            ExecutionEvent::TaskDone {
                task_id: "task-1".to_string(),
                turns: 5,
                tokens: 1000,
                duration_s: 30,
                verify: "pass".to_string(),
                timestamp: later,
            },
        ];

        write_test_log(&log_path, &events);

        let reader = ExecutionLogReader::new(&log_path);
        
        // Read since now - should only get the later event
        let recent = reader.read_since(now).unwrap();
        assert_eq!(recent.len(), 1);
        
        // Read since earlier - should get both
        let all = reader.read_since(earlier).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_summary_computation() {
        let tmp = tempdir().unwrap();
        let log_path = tmp.path().join("summary.jsonl");

        let events = vec![
            ExecutionEvent::Plan {
                total_tasks: 3,
                layers: 2,
                timestamp: Utc::now(),
            },
            ExecutionEvent::TaskDone {
                task_id: "a".to_string(),
                turns: 10,
                tokens: 5000,
                duration_s: 60,
                verify: "pass".to_string(),
                timestamp: Utc::now(),
            },
            ExecutionEvent::TaskDone {
                task_id: "b".to_string(),
                turns: 20,
                tokens: 8000,
                duration_s: 120,
                verify: "pass".to_string(),
                timestamp: Utc::now(),
            },
            ExecutionEvent::TaskFailed {
                task_id: "c".to_string(),
                reason: "verify failed".to_string(),
                turns: 5,
                timestamp: Utc::now(),
            },
            ExecutionEvent::Complete {
                total_turns: 35,
                total_tokens: 13000,
                duration_s: 200,
                failed: 1,
                timestamp: Utc::now(),
            },
        ];

        write_test_log(&log_path, &events);

        let reader = ExecutionLogReader::new(&log_path);
        let summary = reader.summary().unwrap();

        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.completed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.tasks.len(), 3);
        assert!((summary.total_duration_secs - 200.0).abs() < 0.1);
        assert!(summary.is_complete());
        assert!((summary.success_rate() - 66.67).abs() < 0.1);
        assert!((summary.avg_turns_per_task() - 15.0).abs() < 0.1);
    }

    #[test]
    fn test_approval_message_requirements() {
        let ctx = ApprovalContext::for_requirements(5);
        let msg = generate_approval_message(ApprovalPhase::Requirements, &ctx);
        
        assert!(msg.contains("Requirements Complete"));
        assert!(msg.contains("5 goals"));
        assert!(msg.contains("gid approve"));
    }

    #[test]
    fn test_approval_message_design() {
        let ctx = ApprovalContext::for_design(8);
        let msg = generate_approval_message(ApprovalPhase::Design, &ctx);
        
        assert!(msg.contains("Design Complete"));
        assert!(msg.contains("8 sections"));
        assert!(msg.contains("gid approve"));
    }

    #[test]
    fn test_approval_message_graph() {
        let ctx = ApprovalContext::for_graph(12, 4, 6, 180);
        let msg = generate_approval_message(ApprovalPhase::Graph, &ctx);
        
        assert!(msg.contains("Task Graph Ready"));
        assert!(msg.contains("12 tasks"));
        assert!(msg.contains("4 parallel layers"));
        assert!(msg.contains("6 tasks")); // critical path
        assert!(msg.contains("180")); // estimated turns
        assert!(msg.contains("gid approve"));
    }

    #[test]
    fn test_for_gid_dir() {
        let tmp = tempdir().unwrap();
        let gid_dir = tmp.path().join(".gid");
        std::fs::create_dir_all(&gid_dir).unwrap();

        let reader = ExecutionLogReader::for_gid_dir(&gid_dir);
        assert_eq!(
            reader.log_path,
            gid_dir.join("execution-log.jsonl")
        );
    }
}
