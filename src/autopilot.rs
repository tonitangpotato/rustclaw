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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
            max_turns_per_task: 3,
            max_total_turns: 300,
            session_key: String::new(),
        }
    }
}

/// Why a task is in a non-pending, non-completed terminal state.
///
/// Tracked separately from `completed` so the loop can distinguish "agent
/// gave up after reflection" from "task was auto-skipped" from "external
/// blocker exists". All four are treated as "do not retry" by `next_task`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskTerminal {
    /// Legacy auto-skip (`⚠️ SKIPPED: ...`). Kept for back-compat with old task files.
    Skipped,
    /// Agent reflected and decided the task is too large; needs manual decomposition.
    NeedsSplit,
    /// External blocker (missing dep, unclear req, upstream task incomplete).
    Blocked,
    /// Final attempt failed; agent flagged for human review.
    NeedsHumanTriage,
}

impl TaskTerminal {
    /// Marker substring written into the task line. Order matters: more
    /// specific markers must be checked first (e.g. `NEEDS_HUMAN_TRIAGE`
    /// before `NEEDS_SPLIT`, which doesn't actually overlap, but kept
    /// explicit for safety).
    fn marker(&self) -> &'static str {
        match self {
            TaskTerminal::Skipped => "⚠️ SKIPPED",
            TaskTerminal::NeedsSplit => "⚠️ NEEDS_SPLIT",
            TaskTerminal::Blocked => "⚠️ BLOCKED",
            TaskTerminal::NeedsHumanTriage => "⚠️ NEEDS_HUMAN_TRIAGE",
        }
    }

    /// Detect the terminal state from a task description line. Checks the
    /// most specific marker first to avoid `NEEDS_HUMAN_TRIAGE` being
    /// shadowed by a hypothetical broader prefix.
    fn detect(desc: &str) -> Option<Self> {
        if desc.contains("⚠️ NEEDS_HUMAN_TRIAGE") {
            Some(TaskTerminal::NeedsHumanTriage)
        } else if desc.contains("⚠️ NEEDS_SPLIT") {
            Some(TaskTerminal::NeedsSplit)
        } else if desc.contains("⚠️ BLOCKED") {
            Some(TaskTerminal::Blocked)
        } else if desc.contains("⚠️ SKIPPED") {
            Some(TaskTerminal::Skipped)
        } else {
            None
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
    /// If set, the task is in a terminal non-pending state and `next_task`
    /// must skip it. `None` means the task is pending (or completed; check
    /// `completed` first).
    pub terminal: Option<TaskTerminal>,
}

impl Task {
    /// Back-compat alias: any non-pending terminal state was historically
    /// called "skipped". Keep this getter so existing callers don't break.
    pub fn skipped(&self) -> bool {
        self.terminal.is_some()
    }
}

/// Autopilot handle — use to stop or pause a running autopilot.
#[derive(Clone)]
pub struct AutopilotHandle {
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    tasks_completed: Arc<AtomicU32>,
    total_turns: Arc<AtomicU32>,
    total_tokens: Arc<AtomicU64>,
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

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens.load(Ordering::Relaxed)
    }
}

/// Parse markdown task file and extract checkbox items.
///
/// Recognizes:
/// - `- [ ] description` → uncompleted (pending unless a terminal marker is present)
/// - `- [x] description` / `- [X] description` → completed
/// - Lines containing `⚠️ SKIPPED` / `⚠️ NEEDS_SPLIT` / `⚠️ BLOCKED` /
///   `⚠️ NEEDS_HUMAN_TRIAGE` → terminal non-pending state (skipped by `next_task`)
pub fn parse_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("- [ ] ") {
                let desc = trimmed["- [ ] ".len()..].to_string();
                let terminal = TaskTerminal::detect(&desc);
                Some(Task {
                    line_number: i,
                    description: desc,
                    completed: false,
                    terminal,
                })
            } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
                Some(Task {
                    line_number: i,
                    description: trimmed["- [x] ".len()..].to_string(),
                    completed: true,
                    terminal: None,
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

/// Mark a task with a terminal state by appending the appropriate marker.
/// Matches by description prefix to avoid false matches on similar tasks.
///
/// Idempotent: if the task already has a terminal marker, leaves it alone
/// (avoids duplicating `⚠️ SKIPPED ⚠️ NEEDS_HUMAN_TRIAGE` when the loop
/// re-marks a line).
fn mark_task_terminal(
    file_path: &Path,
    description: &str,
    state: TaskTerminal,
    reason: &str,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let mut result = String::with_capacity(content.len() + 80);
    let mut found = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if !found && trimmed.starts_with("- [ ] ") {
            let line_desc = &trimmed["- [ ] ".len()..];
            let matches = line_desc == description
                || line_desc.starts_with(description)
                || description.starts_with(line_desc);
            if matches {
                if TaskTerminal::detect(line_desc).is_some() {
                    // Already terminal — don't double-mark.
                    result.push_str(line);
                } else {
                    result.push_str(&format!("{} {}: {}", line, state.marker(), reason));
                }
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

/// Mark a task as auto-skipped (legacy `⚠️ SKIPPED`). New code should prefer
/// `mark_task_needs_triage`, `mark_task_needs_split`, or `mark_task_blocked`.
fn mark_task_skipped(file_path: &Path, description: &str, reason: &str) -> anyhow::Result<()> {
    mark_task_terminal(file_path, description, TaskTerminal::Skipped, reason)
}

/// Flag a task as needing human triage (final-attempt fallback).
fn mark_task_needs_triage(
    file_path: &Path,
    description: &str,
    reason: &str,
) -> anyhow::Result<()> {
    mark_task_terminal(file_path, description, TaskTerminal::NeedsHumanTriage, reason)
}

/// Find the next actionable task (uncompleted and in no terminal state).
pub fn next_task(tasks: &[Task]) -> Option<&Task> {
    tasks.iter().find(|t| !t.completed && t.terminal.is_none())
}

/// Parsed prior-failure record. Populated from engram by recalling
/// `autopilot_failure` and parsing each entry's quoted task / reason / etc.
#[derive(Debug, Clone)]
struct PriorFailure {
    task_prefix: String, // first 60 chars of failed task description, lowercased
    reason: String,
    attempts: String, // kept as string — display only
    date: String,
}

/// Parse engram-stored failure lines back into structured records.
///
/// Source format (see Change 5 store):
///   `autopilot_failure: task="..." file=... reason="..." attempts=N date=YYYY-MM-DD`
fn parse_failure_record(content: &str) -> Option<PriorFailure> {
    if !content.starts_with("autopilot_failure:") {
        return None;
    }
    // Extract first quoted segment after `task=`
    let task = extract_quoted(content, "task=")?;
    let reason = extract_quoted(content, "reason=").unwrap_or_default();
    let attempts = extract_unquoted(content, "attempts=").unwrap_or_default();
    let date = extract_unquoted(content, "date=").unwrap_or_default();
    let prefix: String = task
        .chars()
        .take(60)
        .collect::<String>()
        .to_lowercase();
    Some(PriorFailure {
        task_prefix: prefix,
        reason,
        attempts,
        date,
    })
}

fn extract_quoted(s: &str, key: &str) -> Option<String> {
    let i = s.find(key)?;
    let rest = &s[i + key.len()..];
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_unquoted(s: &str, key: &str) -> Option<String> {
    let i = s.find(key)?;
    let rest = &s[i + key.len()..];
    let end = rest
        .find(|c: char| c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Match a pending task description against the set of prior failures using
/// case-insensitive 60-char prefix substring match. Returns the most recent
/// matching failure (chosen by raw date string ordering, which is correct for
/// `YYYY-MM-DD`).
fn match_prior_failure<'a>(
    task_desc: &str,
    priors: &'a [PriorFailure],
) -> Option<&'a PriorFailure> {
    let needle = task_desc
        .chars()
        .take(60)
        .collect::<String>()
        .to_lowercase();
    priors
        .iter()
        .filter(|p| p.task_prefix.contains(&needle) || needle.contains(&p.task_prefix))
        .max_by(|a, b| a.date.cmp(&b.date))
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
        tasks_completed: Arc::new(AtomicU32::new(0)),
        total_turns: Arc::new(AtomicU32::new(0)),
        total_tokens: Arc::new(AtomicU64::new(0)),
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

        // ISS-053 Change 5: recall prior autopilot failures so we can inject
        // context into attempt-1 prompts and break the "fresh session retries
        // the same dead task" cycle.
        let prior_failures: Vec<PriorFailure> = if let Some(mem) = runner.memory() {
            match mem.recall_explicit("autopilot_failure", 20) {
                Ok(records) => records
                    .iter()
                    .filter_map(|r| parse_failure_record(&r.content))
                    .collect(),
                Err(e) => {
                    tracing::warn!("Autopilot: prior-failure recall failed: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        if !prior_failures.is_empty() {
            notify(&format!(
                "Loaded {} prior-failure record(s) from engram for context injection",
                prior_failures.len()
            ));
        }

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

            // Build prompt — attempt-1 base. ISS-053 Change 5: prepend a
            // prior-failure note when this task description matches a record
            // recalled from engram. The agent then knows it's hitting a
            // previously dead task and can short-circuit to NEEDS_SPLIT /
            // NEEDS_HUMAN_TRIAGE without burning another retry budget.
            let prior_note = match_prior_failure(&task_desc, &prior_failures).map(|p| {
                format!(
                    "⚠️ Prior failure on this task: {} (attempts: {}, date: {}).\n\
                     Consider this before starting. If the prior failure mode still applies, \
                     go straight to NEEDS_SPLIT or NEEDS_HUMAN_TRIAGE without retrying.\n\n",
                    p.reason, p.attempts, p.date,
                )
            });
            let prompt = format!(
                "{}You are in autopilot mode. Execute this task:\n\n\
                 **Task**: {}\n\n\
                 Read the full task file at `{}` for context.\n\
                 When done, update the checkbox in the task file from `- [ ]` to `- [x]`.\n\
                 If stuck after 3 attempts, update the daily log with why and stop.",
                prior_note.as_deref().unwrap_or(""),
                task_desc,
                task_file.display(),
            );

            let mut task_turns: u32 = 0;
            let mut task_completed = false;
            let max_attempts = config.max_turns_per_task;

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
                if task_turns >= max_attempts {
                    tracing::warn!(
                        "Autopilot: task hit max attempts ({}): {}",
                        max_attempts,
                        task_desc,
                    );
                    break;
                }

                let attempt = task_turns + 1;
                notify(&format!("Attempt {}/{} for task: {}", attempt, max_attempts, task_desc));

                let msg = if task_turns == 0 {
                    // First attempt: full prompt, let the agent's internal tool loop handle multi-step work
                    prompt.clone()
                } else if task_turns == 1 {
                    // Second attempt: force self-reflection + offer A/B/C escape hatches
                    // (retry / NEEDS_SPLIT / BLOCKED). See ISS-053 Change 2.
                    format!(
                        "Your previous attempt did not complete the task (checkbox is still `[ ]`).\n\n\
                         Before retrying, briefly self-reflect:\n\
                         1. What did you actually do in the previous attempt?\n\
                         2. What blocked you? (context exhaustion / unclear scope / failed sub-agent / \
                         wrong file path / build error / something else)\n\
                         3. Is the task too large for one attempt?\n\n\
                         Then choose ONE path:\n\
                         (A) Retry with a different approach. State the change in approach BEFORE acting.\n\
                         (B) If task is too large: append `⚠️ NEEDS_SPLIT: <reason; suggested sub-tasks>` \
                         to the task line in `{file}`, then stop. Do NOT mark `[x]`.\n\
                         (C) If blocked by external (missing dep, unclear req, upstream task incomplete): \
                         append `⚠️ BLOCKED: <what's missing>` to the task line, then stop.\n\n\
                         Task: {desc}",
                        file = task_file.display(),
                        desc = task_desc,
                    )
                } else {
                    // Third (final) attempt: mandate triage marker, no further retries.
                    // See ISS-053 Change 2.
                    format!(
                        "Final attempt failed twice. STOP retrying.\n\n\
                         Append `⚠️ NEEDS_HUMAN_TRIAGE: <one-line: what you tried, why it failed, \
                         suggested next step>` to the task line in `{file}`.\n\
                         Do NOT mark `[x]`. Do NOT mark as `SKIPPED`.\n\n\
                         Task: {desc}",
                        file = task_file.display(),
                        desc = task_desc,
                    )
                };

                // Snapshot session tokens before the call
                let tokens_before = runner
                    .sessions()
                    .get_session(&session_key)
                    .await
                    .map(|s| s.total_tokens)
                    .unwrap_or(0);

                match runner
                    .process_message(&session_key, &msg, None, None)
                    .await
                {
                    Ok(_response) => {
                        task_turns += 1;
                        total_turns += 1;
                        handle_clone.total_turns.store(total_turns, Ordering::Relaxed);

                        // Track token delta from this turn
                        let tokens_after = runner
                            .sessions()
                            .get_session(&session_key)
                            .await
                            .map(|s| s.total_tokens)
                            .unwrap_or(0);
                        let delta = tokens_after.saturating_sub(tokens_before);
                        handle_clone.total_tokens.fetch_add(delta, Ordering::Relaxed);

                        // ISS-053 Change 4: the ONLY truth that a task is done is
                        // the checkbox flipping from `[ ]` to `[x]`. The previous
                        // `response.contains("task completed")` shortcut was a
                        // false-positive magnet — agent prose is unreliable.
                        //
                        // ISS-053 attempt-2/3 escape hatches: also check for the
                        // agent having written a NEEDS_SPLIT / BLOCKED /
                        // NEEDS_HUMAN_TRIAGE marker on the task line. Any of
                        // those should treat the task as resolved-non-completion
                        // — break the retry loop without further marking.
                        if let Ok(updated_content) = std::fs::read_to_string(&task_file) {
                            let updated_tasks = parse_tasks(&updated_content);
                            if let Some(updated) = find_task_by_description(&updated_tasks, &task_desc) {
                                if updated.completed {
                                    notify(&format!(
                                        "Task completed in {} attempt(s): {}",
                                        task_turns, task_desc,
                                    ));
                                    task_completed = true;
                                    completed_count += 1;
                                    handle_clone
                                        .tasks_completed
                                        .store(completed_count, Ordering::Relaxed);
                                    break;
                                }
                                // Agent self-marked a terminal state (B/C path
                                // from attempt 2, or NEEDS_HUMAN_TRIAGE from
                                // attempt 3). Stop retrying; outer loop will
                                // skip on next iteration via `next_task`.
                                if let Some(state) = updated.terminal {
                                    notify(&format!(
                                        "⚠️ Task needs human review: {}\n   State: {:?}\n   File: {}:{}",
                                        task_desc,
                                        state,
                                        task_file.display(),
                                        updated.line_number + 1,
                                    ));
                                    // Treat as resolved-non-completion: don't
                                    // double-mark, don't count as completed.
                                    task_completed = true; // suppress mark_task_skipped fallback
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Autopilot: agent error on '{}': {}", task_desc, e);
                        break;
                    }
                }
            }

            if !task_completed {
                // ISS-053 Change 6: prefer NEEDS_HUMAN_TRIAGE over legacy SKIPPED
                // wording. The agent should self-mark via attempt-3, but if it
                // doesn't (agent error / runaway prose / network drop), the
                // outer loop falls back to triage so the next autopilot run
                // doesn't retry the same dead task.
                let reason = if task_turns >= config.max_turns_per_task {
                    format!("hit max attempts ({}); agent did not self-mark", task_turns)
                } else {
                    "agent error or stopped".to_string()
                };
                notify(&format!(
                    "⚠️ Task needs human review: {}\n   State: NeedsHumanTriage\n   Reason: {}",
                    task_desc, reason,
                ));
                if let Err(e) = mark_task_needs_triage(&task_file, &task_desc, &reason) {
                    tracing::error!("Autopilot: failed to mark needs-triage: {}", e);
                }
                // ISS-053 Change 5: cross-session failure memory. Record so
                // future autopilot runs can recall and pre-warn on the same task.
                if let Some(mem) = runner.memory() {
                    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
                    let content = format!(
                        "autopilot_failure: task=\"{}\" file={} reason=\"{}\" attempts={} date={}",
                        task_desc,
                        task_file.display(),
                        reason,
                        task_turns,
                        date,
                    );
                    if let Err(e) = mem.store_explicit(
                        &content,
                        engramai::MemoryType::Factual,
                        0.7,
                        None,
                    ) {
                        tracing::warn!("Autopilot: failed to store failure memory: {}", e);
                    }
                }
            }

            // Brief pause between tasks
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        let final_tokens = handle_clone.total_tokens.load(Ordering::Relaxed);
        notify(&format!(
            "Finished: {} tasks completed, {} total turns, {} tokens used",
            completed_count, total_turns, final_tokens,
        ));
        Ok(completed_count)
    });

    Ok((handle, join))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(line: usize, desc: &str) -> Task {
        Task {
            line_number: line,
            description: desc.into(),
            completed: false,
            terminal: None,
        }
    }

    fn done(line: usize, desc: &str) -> Task {
        Task {
            line_number: line,
            description: desc.into(),
            completed: true,
            terminal: None,
        }
    }

    fn terminal(line: usize, desc: &str, state: TaskTerminal) -> Task {
        Task {
            line_number: line,
            description: desc.into(),
            completed: false,
            terminal: Some(state),
        }
    }

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
    fn test_parse_skipped_legacy() {
        let content = "- [ ] Normal task\n\
            - [ ] Stuck task ⚠️ SKIPPED: hit max turns\n";
        let tasks = parse_tasks(content);
        assert!(!tasks[0].skipped());
        assert_eq!(tasks[0].terminal, None);
        assert!(tasks[1].skipped());
        assert_eq!(tasks[1].terminal, Some(TaskTerminal::Skipped));
    }

    /// ISS-053 AC: parse_tasks recognizes all four non-pending markers.
    #[test]
    fn test_parse_all_terminal_markers() {
        let content = "- [ ] T1 ⚠️ SKIPPED: legacy reason\n\
            - [ ] T2 ⚠️ NEEDS_SPLIT: too big; suggest sub-tasks\n\
            - [ ] T3 ⚠️ BLOCKED: waiting on upstream\n\
            - [ ] T4 ⚠️ NEEDS_HUMAN_TRIAGE: tried X, failed because Y\n\
            - [ ] T5 actually pending\n";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 5);
        assert_eq!(tasks[0].terminal, Some(TaskTerminal::Skipped));
        assert_eq!(tasks[1].terminal, Some(TaskTerminal::NeedsSplit));
        assert_eq!(tasks[2].terminal, Some(TaskTerminal::Blocked));
        assert_eq!(tasks[3].terminal, Some(TaskTerminal::NeedsHumanTriage));
        assert_eq!(tasks[4].terminal, None);
    }

    /// ISS-053 AC: next_task skips all four non-pending states.
    #[test]
    fn test_next_task_skips_all_terminal_states() {
        let tasks = vec![
            done(0, "done"),
            terminal(1, "skipped legacy", TaskTerminal::Skipped),
            terminal(2, "needs split", TaskTerminal::NeedsSplit),
            terminal(3, "blocked", TaskTerminal::Blocked),
            terminal(4, "needs triage", TaskTerminal::NeedsHumanTriage),
            pending(5, "pending"),
        ];
        assert_eq!(next_task(&tasks).unwrap().description, "pending");
    }

    #[test]
    fn test_next_task_all_done() {
        let tasks = vec![done(0, "done")];
        assert!(next_task(&tasks).is_none());
    }

    #[test]
    fn test_next_task_all_terminal_returns_none() {
        let tasks = vec![
            terminal(0, "a", TaskTerminal::Skipped),
            terminal(1, "b", TaskTerminal::NeedsHumanTriage),
        ];
        assert!(next_task(&tasks).is_none());
    }

    /// ISS-053 AC: find_task_by_description still works after a marker is
    /// appended (since the description stored in `Task.description` includes
    /// the marker, the prefix-match branch takes over).
    #[test]
    fn test_find_task_by_description_after_marker() {
        let tasks = vec![
            pending(0, "T2.3 ISS-009 Cross-Layer"),
            terminal(
                5,
                "T2.4 ISS-006 Incremental ⚠️ NEEDS_HUMAN_TRIAGE: ran out of context",
                TaskTerminal::NeedsHumanTriage,
            ),
        ];
        assert!(find_task_by_description(&tasks, "T2.3 ISS-009 Cross-Layer").is_some());
        // After marker: prefix match must still resolve when given the
        // original description (the autopilot loop searches by the desc it
        // recorded *before* the marker was appended).
        assert!(find_task_by_description(&tasks, "T2.4 ISS-006 Incremental").is_some());
        assert!(find_task_by_description(&tasks, "nonexistent").is_none());
    }

    /// ISS-053 AC: marking helpers append correctly without duplicating.
    #[test]
    fn test_mark_task_terminal_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("tasks.md");
        std::fs::write(
            &path,
            "# Tasks\n- [ ] alpha\n- [ ] beta task\n- [x] gamma\n",
        )
        .unwrap();

        // First mark
        mark_task_needs_triage(&path, "alpha", "first failure")
            .expect("mark1");
        let after1 = std::fs::read_to_string(&path).unwrap();
        assert!(after1.contains("- [ ] alpha ⚠️ NEEDS_HUMAN_TRIAGE: first failure"));

        // Second mark on same description must NOT double-append
        mark_task_needs_triage(&path, "alpha", "second failure")
            .expect("mark2");
        let after2 = std::fs::read_to_string(&path).unwrap();
        let count = after2.matches("⚠️ NEEDS_HUMAN_TRIAGE").count();
        assert_eq!(count, 1, "must not duplicate marker; got: {}", after2);
        // Reason from first call is preserved (idempotent = no-op on
        // already-terminal lines).
        assert!(after2.contains("first failure"));
        assert!(!after2.contains("second failure"));

        // Marking a different task still works (not blocked by the first).
        mark_task_skipped(&path, "beta task", "different reason").expect("mark3");
        let after3 = std::fs::read_to_string(&path).unwrap();
        assert!(after3.contains("- [ ] beta task ⚠️ SKIPPED: different reason"));
    }

    #[test]
    fn test_find_task_by_description() {
        let tasks = vec![
            pending(0, "T2.3 ISS-009 Cross-Layer"),
            pending(5, "T2.4 ISS-006 Incremental"),
        ];
        assert!(find_task_by_description(&tasks, "T2.3 ISS-009 Cross-Layer").is_some());
        assert!(find_task_by_description(&tasks, "nonexistent").is_none());
    }

    /// ISS-053 Change 5: parse_failure_record round-trips a stored failure
    /// memory so the startup recall path can extract task / reason / date.
    #[test]
    fn test_parse_failure_record_roundtrip() {
        let stored = r#"autopilot_failure: task="T2.4 ISS-006 Incremental sub-dim extraction" file=memory/2026-04-27-night.md reason="hit max attempts (3); agent did not self-mark" attempts=3 date=2026-04-27"#;
        let p = parse_failure_record(stored).expect("must parse");
        assert!(p.task_prefix.starts_with("t2.4 iss-006 incremental"));
        assert_eq!(p.attempts, "3");
        assert_eq!(p.date, "2026-04-27");
        assert!(p.reason.contains("hit max attempts"));
    }

    #[test]
    fn test_parse_failure_record_rejects_other_lines() {
        assert!(parse_failure_record("some other memory").is_none());
        assert!(parse_failure_record("").is_none());
    }

    #[test]
    fn test_match_prior_failure_picks_most_recent() {
        let priors = vec![
            PriorFailure {
                task_prefix: "t2.4 iss-006 incremental sub-dim extraction".into(),
                reason: "old".into(),
                attempts: "3".into(),
                date: "2026-04-25".into(),
            },
            PriorFailure {
                task_prefix: "t2.4 iss-006 incremental sub-dim extraction".into(),
                reason: "new".into(),
                attempts: "3".into(),
                date: "2026-04-27".into(),
            },
            PriorFailure {
                task_prefix: "totally unrelated task description".into(),
                reason: "noise".into(),
                attempts: "1".into(),
                date: "2026-04-26".into(),
            },
        ];
        let m = match_prior_failure("T2.4 ISS-006 Incremental sub-dim extraction", &priors)
            .expect("must match");
        assert_eq!(m.reason, "new");
        // Unrelated task should not match anything.
        assert!(match_prior_failure("Wholly different work", &priors).is_none());
    }
}
