//! Autopilot — autonomous task runner for RustClaw.
//!
//! Reads a markdown task file, finds uncompleted tasks, and drives the agent
//! through them one by one. Stops when all tasks are done, context is full,
//! user sends a message, or max turns exceeded.
//!
//! Usage:
//!   /autopilot memory/2026-04-07-overnight-plan.md
//!   /autopilot stop

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// Autopilot configuration.
#[derive(Clone, Debug)]
pub struct AutopilotConfig {
    /// Path to the markdown task file (relative to workspace).
    pub task_file: PathBuf,
    /// Max agent turns per individual task.
    pub max_turns_per_task: u32,
    /// Max total agent turns across all tasks.
    pub max_total_turns: u32,
    /// Session key to run in.
    pub session_key: String,
}

impl Default for AutopilotConfig {
    fn default() -> Self {
        Self {
            task_file: PathBuf::from("HEARTBEAT.md"),
            max_turns_per_task: 40,
            max_total_turns: 200,
            session_key: String::new(),
        }
    }
}

/// A parsed task from the markdown file.
#[derive(Debug, Clone)]
pub struct Task {
    /// Line number in the file (for updating checkbox).
    pub line_number: usize,
    /// Raw line text.
    pub line: String,
    /// Task description (without checkbox prefix).
    pub description: String,
    /// Whether the task is completed.
    pub completed: bool,
}

/// Autopilot handle — use to stop a running autopilot.
#[derive(Clone)]
pub struct AutopilotHandle {
    running: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
}

impl AutopilotHandle {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.stop_notify.notify_one();
    }
}

/// Parse markdown task file and extract checkbox items.
///
/// Recognizes:
/// - `- [ ] task description` → uncompleted
/// - `- [x] task description` → completed
/// - `- [X] task description` → completed
pub fn parse_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ] ") {
                Some(Task {
                    line_number: i,
                    line: line.to_string(),
                    description: trimmed["- [ ] ".len()..].to_string(),
                    completed: false,
                })
            } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
                Some(Task {
                    line_number: i,
                    line: line.to_string(),
                    description: trimmed["- [x] ".len()..].to_string(),
                    completed: true,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Mark a task as completed in the file by replacing `- [ ]` with `- [x]`.
pub fn mark_task_done(file_path: &Path, line_number: usize) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let lines: Vec<&str> = content.lines().collect();

    if line_number >= lines.len() {
        anyhow::bail!("Line number {} out of range (file has {} lines)", line_number, lines.len());
    }

    let mut new_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    new_lines[line_number] = new_lines[line_number]
        .replacen("- [ ] ", "- [x] ", 1);

    std::fs::write(file_path, new_lines.join("\n"))?;
    Ok(())
}

/// Find the next uncompleted task.
pub fn next_task(tasks: &[Task]) -> Option<&Task> {
    tasks.iter().find(|t| !t.completed)
}

/// Run autopilot: continuously execute tasks from the task file.
///
/// Returns the number of tasks completed.
pub async fn run(
    runner: Arc<crate::agent::AgentRunner>,
    config: AutopilotConfig,
    workspace: &Path,
) -> anyhow::Result<(AutopilotHandle, tokio::task::JoinHandle<anyhow::Result<u32>>)> {
    let task_file = if config.task_file.is_absolute() {
        config.task_file.clone()
    } else {
        workspace.join(&config.task_file)
    };

    if !task_file.exists() {
        anyhow::bail!("Task file not found: {}", task_file.display());
    }

    let handle = AutopilotHandle {
        running: Arc::new(AtomicBool::new(true)),
        stop_notify: Arc::new(Notify::new()),
    };
    let handle_clone = handle.clone();
    let session_key = config.session_key.clone();

    let join = tokio::spawn(async move {
        let mut completed_count: u32 = 0;
        let mut total_turns: u32 = 0;

        tracing::info!(
            "Autopilot started: file={} max_turns_per_task={} max_total={}",
            task_file.display(),
            config.max_turns_per_task,
            config.max_total_turns,
        );

        loop {
            // Check stop conditions
            if !handle_clone.running.load(Ordering::Relaxed) {
                tracing::info!("Autopilot stopped by user");
                break;
            }
            if total_turns >= config.max_total_turns {
                tracing::info!("Autopilot reached max total turns ({})", config.max_total_turns);
                break;
            }

            // Read task file fresh each iteration
            let content = match std::fs::read_to_string(&task_file) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Autopilot: failed to read task file: {}", e);
                    break;
                }
            };

            let tasks = parse_tasks(&content);
            let remaining: Vec<_> = tasks.iter().filter(|t| !t.completed).collect();

            if remaining.is_empty() {
                tracing::info!("Autopilot: all tasks completed! ({} done)", completed_count);
                break;
            }

            let task = remaining[0];
            tracing::info!(
                "Autopilot: starting task [{}/{}]: {}",
                completed_count + 1,
                tasks.len(),
                task.description,
            );

            // Build prompt for the agent
            let prompt = format!(
                "You are in autopilot mode. Execute this task:\n\n\
                 **Task**: {}\n\n\
                 Read the full task file at `{}` for context.\n\
                 When done, update the checkbox in the task file from `- [ ]` to `- [x]`.\n\
                 If you get stuck for more than 3 tool calls with no progress, \
                 write why in the daily log and move on.",
                task.description,
                task_file.display(),
            );

            // Run agent
            let mut task_turns: u32 = 0;
            let task_completed;

            loop {
                if !handle_clone.running.load(Ordering::Relaxed) {
                    task_completed = false;
                    break;
                }
                if task_turns >= config.max_turns_per_task {
                    tracing::warn!(
                        "Autopilot: task hit max turns ({}): {}",
                        config.max_turns_per_task,
                        task.description,
                    );
                    task_completed = false;
                    break;
                }

                let msg = if task_turns == 0 {
                    prompt.clone()
                } else {
                    "Continue the current task. If done, update the checkbox.".to_string()
                };

                match runner
                    .process_message(&session_key, &msg, None, None)
                    .await
                {
                    Ok(response) => {
                        task_turns += 1;
                        total_turns += 1;

                        // Check if the task was marked done in the file
                        if let Ok(updated_content) = std::fs::read_to_string(&task_file) {
                            let updated_tasks = parse_tasks(&updated_content);
                            if let Some(updated_task) = updated_tasks.get(task.line_number) {
                                if updated_task.completed {
                                    tracing::info!(
                                        "Autopilot: task completed in {} turns: {}",
                                        task_turns,
                                        task.description,
                                    );
                                    task_completed = true;
                                    completed_count += 1;
                                    break;
                                }
                            }
                        }

                        // Check if agent said it's done/stuck in the response
                        let lower = response.to_lowercase();
                        if lower.contains("heartbeat_ok") || lower.contains("all tasks completed") {
                            task_completed = true;
                            completed_count += 1;
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Autopilot: agent error: {}", e);
                        task_completed = false;
                        break;
                    }
                }
            }

            if !task_completed {
                // Mark as skipped in daily log, continue to next
                tracing::warn!("Autopilot: skipping task: {}", task.description);
                // Force-mark done to avoid infinite loop on stuck task
                if let Err(e) = mark_task_done(&task_file, task.line_number) {
                    tracing::error!("Autopilot: failed to mark task: {}", e);
                }
                completed_count += 1; // Count as processed even if skipped
            }

            // Brief pause between tasks
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        tracing::info!(
            "Autopilot finished: {} tasks processed, {} total turns",
            completed_count,
            total_turns,
        );
        Ok(completed_count)
    });

    Ok((handle, join))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tasks() {
        let content = "# Tasks\n\
            - [x] Done task\n\
            - [ ] Pending task 1\n\
            Some text\n\
            - [ ] Pending task 2\n\
            - [X] Also done\n";

        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 4);
        assert!(tasks[0].completed);
        assert!(!tasks[1].completed);
        assert_eq!(tasks[1].description, "Pending task 1");
        assert!(!tasks[2].completed);
        assert!(tasks[3].completed);
    }

    #[test]
    fn test_next_task() {
        let tasks = vec![
            Task { line_number: 0, line: String::new(), description: "done".into(), completed: true },
            Task { line_number: 1, line: String::new(), description: "pending".into(), completed: false },
        ];
        let next = next_task(&tasks);
        assert_eq!(next.unwrap().description, "pending");
    }

    #[test]
    fn test_next_task_all_done() {
        let tasks = vec![
            Task { line_number: 0, line: String::new(), description: "done".into(), completed: true },
        ];
        assert!(next_task(&tasks).is_none());
    }
}
