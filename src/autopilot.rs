//! Autopilot — autonomous task runner for RustClaw.
//!
//! Reads a markdown task file, finds uncompleted tasks, and drives the agent
//! through them one by one. Stops when all tasks are done, context is full,
//! user sends a message, or max turns exceeded.
//!
//! Usage:
//!   /autopilot memory/2026-04-07-overnight-plan.md
//!   /autopilot stop
//!   /autopilot status

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    /// Task description (without checkbox prefix).
    pub description: String,
    /// Whether the task is completed.
    pub completed: bool,
    /// Whether the task was skipped.
    pub skipped: bool,
}

/// Autopilot handle — use to stop or pause a running autopilot.
#[derive(Clone)]
pub struct AutopilotHandle {
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    tasks_completed: Arc<std::sync::atomic::AtomicU32>,
    total_turns: Arc<std::sync::atomic::AtomicU32>,
}

impl AutopilotHandle {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Pause autopilot (e.g., when user sends a message).
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    /// Resume after pause.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn stats(&self) -> (u32, u32) {
        (
            self.tasks_completed.load(Ordering::Relaxed),
            self.total_turns.load(Ordering::Relaxed),
        )
    }
}

/// Parse markdown task file and extract checkbox items.
///
/// Recognizes:
/// - `- [ ] description` → uncompleted
/// - `- [x] description` / `- [X] description` → completed
/// - Lines containing `⚠️ SKIPPED` → skipped
pub fn parse_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ] ") {
                let desc = trimmed["- [ ] ".len()..].to_string();
                let skipped = desc.contains("⚠️ SKIPPED");
                Some(Task {
                    line_number: i,
                    description: desc,
                    completed: false,
                    skipped,
                })
            } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
                Some(Task {
                    line_number: i,
                    description: trimmed["- [x] ".len()..].to_string(),
                    completed: true,
                    skipped: false,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Find a task by description content (robust against line number shifts).
fn find_task_by_description<'a>(tasks: &'a [Task], description: &str) -> Option<&'a Task> {
    // Exact match first
    if let Some(t) = tasks.iter().find(|t| t.description == description) {
        return Some(t);
    }
    // Prefix match (description might have been appended with status)
    tasks.iter().find(|t| t.description.starts_with(description) || description.starts_with(&t.description))
}

/// Mark a task as skipped in the file by appending ⚠️ SKIPPED.
/// Matches by exact description to avoid false matches on similar tasks.
fn mark_task_skipped(file_path: &Path, description: &str, reason: &str) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let mut result = String::with_capacity(content.len() + 50);
    let mut found = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if !found && trimmed.starts_with("- [ ] ") {
            let line_desc = &trimmed["- [ ] ".len()..];
            if line_desc == description || line_desc.starts_with(description) || description.starts_with(line_desc) {
                result.push_str(&format!("{} ⚠️ SKIPPED: {}", line, reason));
                found = true;
            } else {
                result.push_str(line);
            }
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    std::fs::write(file_path, result)?;
    Ok(())
}

/// Find the next actionable task (uncompleted and not skipped).
pub fn next_task(tasks: &[Task]) -> Option<&Task> {
    tasks.iter().find(|t| !t.completed && !t.skipped)
}

/// Run autopilot: continuously execute tasks from the task file.
///
/// Returns the number of tasks completed.
pub async fn run(
    runner: Arc<crate::agent::AgentRunner>,
    config: AutopilotConfig,
    workspace: &Path,
    notify_fn: Option<Box<dyn Fn(&str) + Send + Sync + 'static>>,
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
        paused: Arc::new(AtomicBool::new(false)),
        tasks_completed: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        total_turns: Arc::new(std::sync::atomic::AtomicU32::new(0)),
    };
    let handle_clone = handle.clone();
    let session_key = config.session_key.clone();

    let join = tokio::spawn(async move {
        let mut completed_count: u32 = 0;
        let mut total_turns: u32 = 0;
        let notify = |msg: &str| {
            tracing::info!("Autopilot: {}", msg);
            if let Some(ref f) = notify_fn {
                f(msg);
            }
        };

        notify(&format!(
            "Started: file={} max_turns_per_task={} max_total={}",
            task_file.display(),
            config.max_turns_per_task,
            config.max_total_turns,
        ));

        loop {
            // Check stop
            if !handle_clone.running.load(Ordering::Relaxed) {
                notify("Stopped by user");
                break;
            }

            // Wait while paused (user is chatting)
            while handle_clone.paused.load(Ordering::Relaxed) {
                if !handle_clone.running.load(Ordering::Relaxed) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }

            if total_turns >= config.max_total_turns {
                notify(&format!("Reached max total turns ({})", config.max_total_turns));
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
            let next = next_task(&tasks);

            if next.is_none() {
                notify(&format!("All tasks done! ({} completed)", completed_count));
                break;
            }
            let task = next.unwrap();
            let task_desc = task.description.clone();

            notify(&format!(
                "Starting task [{}/{}]: {}",
                completed_count + 1,
                tasks.len(),
                task_desc,
            ));

            // Build prompt
            let prompt = format!(
                "You are in autopilot mode. Execute this task:\n\n\
                 **Task**: {}\n\n\
                 Read the full task file at `{}` for context.\n\
                 When done, update the checkbox in the task file from `- [ ]` to `- [x]`.\n\
                 If stuck after 3 attempts, update the daily log with why and stop.",
                task_desc,
                task_file.display(),
            );

            let mut task_turns: u32 = 0;
            let mut task_completed = false;

            loop {
                if !handle_clone.running.load(Ordering::Relaxed) {
                    break;
                }
                // Pause check — yield to user interaction
                if handle_clone.paused.load(Ordering::Relaxed) {
                    tracing::info!("Autopilot: paused mid-task (user interaction)");
                    while handle_clone.paused.load(Ordering::Relaxed) {
                        if !handle_clone.running.load(Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    tracing::info!("Autopilot: resumed");
                }
                if task_turns >= config.max_turns_per_task {
                    tracing::warn!(
                        "Autopilot: task hit max turns ({}): {}",
                        config.max_turns_per_task,
                        task_desc,
                    );
                    break;
                }

                let msg = if task_turns == 0 {
                    prompt.clone()
                } else {
                    format!(
                        "Continue working on: **{}**\n\
                         Update the checkbox when done.",
                        task_desc
                    )
                };

                match runner
                    .process_message(&session_key, &msg, None, None)
                    .await
                {
                    Ok(response) => {
                        task_turns += 1;
                        total_turns += 1;
                        handle_clone.total_turns.store(total_turns, Ordering::Relaxed);

                        // Check if task was marked done — match by description, not line number
                        if let Ok(updated_content) = std::fs::read_to_string(&task_file) {
                            let updated_tasks = parse_tasks(&updated_content);
                            if let Some(updated) = find_task_by_description(&updated_tasks, &task_desc) {
                                if updated.completed {
                                    notify(&format!(
                                        "Task completed in {} turns: {}",
                                        task_turns, task_desc,
                                    ));
                                    task_completed = true;
                                    completed_count += 1;
                                    handle_clone.tasks_completed.store(completed_count, Ordering::Relaxed);
                                    break;
                                }
                            }
                        }

                        // Check if agent explicitly says done/stuck
                        let lower = response.to_lowercase();
                        if lower.contains("all tasks completed") || lower.contains("task completed") {
                            task_completed = true;
                            completed_count += 1;
                            handle_clone.tasks_completed.store(completed_count, Ordering::Relaxed);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Autopilot: agent error on '{}': {}", task_desc, e);
                        break;
                    }
                }
            }

            if !task_completed {
                let reason = if task_turns >= config.max_turns_per_task {
                    format!("hit max turns ({})", config.max_turns_per_task)
                } else {
                    "agent error or stopped".to_string()
                };
                notify(&format!("Skipping task: {} ({})", task_desc, reason));
                if let Err(e) = mark_task_skipped(&task_file, &task_desc, &reason) {
                    tracing::error!("Autopilot: failed to mark skipped: {}", e);
                }
            }

            // Brief pause between tasks
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        notify(&format!(
            "Finished: {} tasks completed, {} total turns",
            completed_count, total_turns,
        ));
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
    fn test_parse_skipped() {
        let content = "- [ ] Normal task\n\
            - [ ] Stuck task ⚠️ SKIPPED: hit max turns\n";
        let tasks = parse_tasks(content);
        assert!(!tasks[0].skipped);
        assert!(tasks[1].skipped);
    }

    #[test]
    fn test_next_task_skips_skipped() {
        let tasks = vec![
            Task { line_number: 0, description: "done".into(), completed: true, skipped: false },
            Task { line_number: 1, description: "skipped".into(), completed: false, skipped: true },
            Task { line_number: 2, description: "pending".into(), completed: false, skipped: false },
        ];
        assert_eq!(next_task(&tasks).unwrap().description, "pending");
    }

    #[test]
    fn test_next_task_all_done() {
        let tasks = vec![
            Task { line_number: 0, description: "done".into(), completed: true, skipped: false },
        ];
        assert!(next_task(&tasks).is_none());
    }

    #[test]
    fn test_find_task_by_description() {
        let tasks = vec![
            Task { line_number: 0, description: "T2.3 ISS-009 Cross-Layer".into(), completed: false, skipped: false },
            Task { line_number: 5, description: "T2.4 ISS-006 Incremental".into(), completed: false, skipped: false },
        ];
        assert!(find_task_by_description(&tasks, "T2.3 ISS-009 Cross-Layer").is_some());
        assert!(find_task_by_description(&tasks, "nonexistent").is_none());
    }
}
