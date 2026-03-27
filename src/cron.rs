//! Enhanced cron system for scheduled agent tasks.
//!
//! Supports:
//! - Standard cron expressions (e.g., "0 9 * * *" = 9AM daily)
//! - Interval-based jobs (every N seconds)
//! - One-shot jobs (at specific time)
//! - Multiple task types: Shell, AgentMessage, Script
//! - Timezone-aware scheduling
//! - Non-blocking concurrent execution

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{Local, NaiveDateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::agent::AgentRunner;
use crate::config::{CronConfig, CronJobConfig, CronTaskConfig};

/// A parsed cron job ready for scheduling.
#[derive(Debug, Clone)]
pub struct CronJob {
    /// Unique job name.
    pub name: String,
    /// The schedule type.
    pub schedule: CronScheduleType,
    /// The task to execute.
    pub task: CronTask,
    /// Whether the job is enabled.
    pub enabled: bool,
    /// Session key for AgentMessage tasks.
    pub session_key: String,
    /// Channel for AgentMessage task responses.
    pub channel: Option<String>,
}

/// Schedule type for cron jobs.
#[derive(Debug, Clone)]
pub enum CronScheduleType {
    /// Standard cron expression with timezone.
    Cron {
        schedule: Schedule,
        timezone: Tz,
    },
    /// Run every N seconds.
    Interval {
        seconds: u64,
    },
    /// Run once at a specific datetime.
    OneShot {
        at: NaiveDateTime,
    },
}

/// Task to execute when a cron job fires.
#[derive(Debug, Clone)]
pub enum CronTask {
    /// Run a shell command.
    Shell { command: String },
    /// Send a message to the agent (triggers agent loop).
    AgentMessage { message: String },
    /// Execute a script file.
    Script { path: PathBuf },
}

/// Message sent when a cron job fires.
#[derive(Debug)]
pub struct CronTrigger {
    pub job_name: String,
    pub task: CronTask,
    pub session_key: String,
    pub channel: Option<String>,
}

/// Cron scheduler that manages all jobs.
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    tx: mpsc::Sender<CronTrigger>,
}

impl CronScheduler {
    /// Create a new cron scheduler with the given jobs.
    pub fn new(jobs: Vec<CronJob>, tx: mpsc::Sender<CronTrigger>) -> Self {
        Self { jobs, tx }
    }

    /// Start the cron scheduler (spawns background tasks for each job).
    pub async fn start(self) {
        for job in self.jobs {
            if !job.enabled {
                tracing::debug!("Cron job '{}' is disabled, skipping", job.name);
                continue;
            }

            let tx = self.tx.clone();
            let job_name = job.name.clone();

            match job.schedule.clone() {
                CronScheduleType::Cron { schedule, timezone } => {
                    tracing::info!(
                        "Cron job '{}' scheduled: {} ({})",
                        job_name,
                        schedule,
                        timezone
                    );

                    tokio::spawn(async move {
                        run_cron_schedule_loop(job, schedule, timezone, tx).await;
                    });
                }
                CronScheduleType::Interval { seconds } => {
                    tracing::info!("Cron job '{}' scheduled: every {}s", job_name, seconds);

                    tokio::spawn(async move {
                        run_interval_loop(job, seconds, tx).await;
                    });
                }
                CronScheduleType::OneShot { at } => {
                    tracing::info!("Cron job '{}' scheduled: one-shot at {}", job_name, at);

                    tokio::spawn(async move {
                        run_oneshot(job, at, tx).await;
                    });
                }
            }
        }
    }
}

/// Run a job on a cron schedule.
async fn run_cron_schedule_loop(
    job: CronJob,
    schedule: Schedule,
    timezone: Tz,
    tx: mpsc::Sender<CronTrigger>,
) {
    loop {
        // Get current time in the configured timezone
        let now_utc = Utc::now();
        let now_tz = now_utc.with_timezone(&timezone);

        // Find the next scheduled time
        let next = match schedule.after(&now_tz).next() {
            Some(next) => next,
            None => {
                tracing::warn!("Cron job '{}' has no future occurrences", job.name);
                return;
            }
        };

        // Calculate sleep duration
        let duration = (next - now_tz).to_std().unwrap_or_default();
        tracing::debug!(
            "Cron job '{}' next run at {} (in {:?})",
            job.name,
            next,
            duration
        );

        tokio::time::sleep(duration).await;

        // Fire the job
        tracing::info!("Cron job '{}' firing", job.name);
        let trigger = CronTrigger {
            job_name: job.name.clone(),
            task: job.task.clone(),
            session_key: job.session_key.clone(),
            channel: job.channel.clone(),
        };

        if let Err(e) = tx.send(trigger).await {
            tracing::error!("Failed to send cron trigger for '{}': {}", job.name, e);
            break;
        }

        // Small delay to prevent firing multiple times for the same minute
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Run a job at a fixed interval.
async fn run_interval_loop(job: CronJob, seconds: u64, tx: mpsc::Sender<CronTrigger>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(seconds));
    // Skip first tick (fires immediately)
    interval.tick().await;

    loop {
        interval.tick().await;
        tracing::info!("Cron job '{}' firing (interval)", job.name);

        let trigger = CronTrigger {
            job_name: job.name.clone(),
            task: job.task.clone(),
            session_key: job.session_key.clone(),
            channel: job.channel.clone(),
        };

        if let Err(e) = tx.send(trigger).await {
            tracing::error!("Failed to send cron trigger for '{}': {}", job.name, e);
            break;
        }
    }
}

/// Run a one-shot job at a specific time.
async fn run_oneshot(job: CronJob, at: NaiveDateTime, tx: mpsc::Sender<CronTrigger>) {
    let now = Local::now().naive_local();
    if at <= now {
        tracing::warn!(
            "Cron job '{}' target time {} is in the past, skipping",
            job.name,
            at
        );
        return;
    }

    let duration = (at - now).to_std().unwrap_or_default();
    tokio::time::sleep(duration).await;

    tracing::info!("Cron job '{}' firing (one-shot)", job.name);

    let trigger = CronTrigger {
        job_name: job.name.clone(),
        task: job.task.clone(),
        session_key: job.session_key.clone(),
        channel: job.channel.clone(),
    };

    if let Err(e) = tx.send(trigger).await {
        tracing::error!("Failed to send cron trigger for '{}': {}", job.name, e);
    }
}

/// Execute a shell command and return (exit_code, stdout, stderr).
async fn execute_shell_command(command: &str) -> anyhow::Result<(i32, String, String)> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok((exit_code, stdout, stderr))
}

/// Execute a script file and return (exit_code, stdout, stderr).
async fn execute_script(path: &PathBuf) -> anyhow::Result<(i32, String, String)> {
    // Check if file exists
    if !path.exists() {
        anyhow::bail!("Script not found: {}", path.display());
    }

    let output = Command::new(path).output().await?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    Ok((exit_code, stdout, stderr))
}

/// Process cron triggers by executing them.
pub async fn process_cron_triggers(mut rx: mpsc::Receiver<CronTrigger>, runner: Arc<AgentRunner>) {
    while let Some(trigger) = rx.recv().await {
        let runner = runner.clone();
        let job_name = trigger.job_name.clone();

        // Spawn each task execution so they don't block each other
        tokio::spawn(async move {
            let start = std::time::Instant::now();

            match trigger.task {
                CronTask::Shell { ref command } => {
                    tracing::info!("Cron job '{}' executing shell: {}", job_name, command);

                    match execute_shell_command(command).await {
                        Ok((code, stdout, stderr)) => {
                            let elapsed = start.elapsed();
                            if code == 0 {
                                tracing::info!(
                                    "Cron job '{}' shell completed in {:?} (exit 0)",
                                    job_name,
                                    elapsed
                                );
                                if !stdout.is_empty() {
                                    tracing::debug!("stdout: {}", stdout.trim());
                                }
                            } else {
                                tracing::warn!(
                                    "Cron job '{}' shell exited with code {} in {:?}",
                                    job_name,
                                    code,
                                    elapsed
                                );
                                if !stderr.is_empty() {
                                    tracing::warn!("stderr: {}", stderr.trim());
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Cron job '{}' shell failed: {}", job_name, e);
                        }
                    }
                }
                CronTask::Script { ref path } => {
                    tracing::info!(
                        "Cron job '{}' executing script: {}",
                        job_name,
                        path.display()
                    );

                    match execute_script(path).await {
                        Ok((code, stdout, stderr)) => {
                            let elapsed = start.elapsed();
                            if code == 0 {
                                tracing::info!(
                                    "Cron job '{}' script completed in {:?} (exit 0)",
                                    job_name,
                                    elapsed
                                );
                                if !stdout.is_empty() {
                                    tracing::debug!("stdout: {}", stdout.trim());
                                }
                            } else {
                                tracing::warn!(
                                    "Cron job '{}' script exited with code {} in {:?}",
                                    job_name,
                                    code,
                                    elapsed
                                );
                                if !stderr.is_empty() {
                                    tracing::warn!("stderr: {}", stderr.trim());
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Cron job '{}' script failed: {}", job_name, e);
                        }
                    }
                }
                CronTask::AgentMessage { ref message } => {
                    tracing::info!(
                        "Cron job '{}' sending agent message: {}",
                        job_name,
                        truncate_str(message, 50)
                    );

                    match runner
                        .process_message(
                            &trigger.session_key,
                            message,
                            None,
                            trigger.channel.as_deref(),
                        )
                        .await
                    {
                        Ok(response) => {
                            let elapsed = start.elapsed();
                            let trimmed = response.trim();
                            if !trimmed.is_empty() && trimmed != "HEARTBEAT_OK" {
                                tracing::info!(
                                    "Cron job '{}' response in {:?}: {}",
                                    job_name,
                                    elapsed,
                                    truncate_str(trimmed, 200)
                                );
                            } else {
                                tracing::debug!(
                                    "Cron job '{}' completed in {:?} (no output)",
                                    job_name,
                                    elapsed
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("Cron job '{}' agent error: {}", job_name, e);
                        }
                    }
                }
            }
        });
    }
}

/// Helper to truncate a string for logging.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let end = s.floor_char_boundary(max_len);
        &s[..end]
    }
}

/// Start the cron system with jobs from config.
pub async fn start_cron(jobs: Vec<CronJob>, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    if jobs.is_empty() {
        tracing::debug!("No cron jobs configured");
        return Ok(());
    }

    let (tx, rx) = mpsc::channel(32);

    // Start the scheduler
    let scheduler = CronScheduler::new(jobs, tx);
    scheduler.start().await;

    // Start the processor
    tokio::spawn(async move {
        process_cron_triggers(rx, runner).await;
    });

    Ok(())
}

/// Parse cron jobs from config.
pub fn parse_cron_jobs(config: &CronConfig) -> Vec<CronJob> {
    let default_tz: Tz = config
        .timezone
        .parse()
        .unwrap_or_else(|_| {
            tracing::warn!(
                "Invalid timezone '{}', defaulting to UTC",
                config.timezone
            );
            Tz::UTC
        });

    config
        .jobs
        .iter()
        .filter_map(|c| {
            let schedule = parse_schedule_type(c, default_tz)?;
            let task = parse_task(&c.task);
            let session_key = c
                .session_key
                .clone()
                .unwrap_or_else(|| format!("cron:{}", c.name));

            Some(CronJob {
                name: c.name.clone(),
                schedule,
                task,
                enabled: c.enabled,
                session_key,
                channel: c.channel.clone(),
            })
        })
        .collect()
}

/// Parse the schedule type from config.
fn parse_schedule_type(config: &CronJobConfig, default_tz: Tz) -> Option<CronScheduleType> {
    // Priority: schedule > interval_seconds > at
    if let Some(expr) = &config.schedule {
        // The cron crate expects 6-field expressions (with seconds)
        // Standard 5-field cron → prepend "0 " for seconds
        let expr_with_seconds = if expr.split_whitespace().count() == 5 {
            format!("0 {}", expr)
        } else {
            expr.clone()
        };

        match Schedule::from_str(&expr_with_seconds) {
            Ok(schedule) => Some(CronScheduleType::Cron {
                schedule,
                timezone: default_tz,
            }),
            Err(e) => {
                tracing::error!(
                    "Invalid cron expression '{}' for job '{}': {}",
                    expr,
                    config.name,
                    e
                );
                None
            }
        }
    } else if let Some(secs) = config.interval_seconds {
        Some(CronScheduleType::Interval { seconds: secs })
    } else if let Some(at_str) = &config.at {
        match NaiveDateTime::parse_from_str(at_str, "%Y-%m-%d %H:%M:%S") {
            Ok(dt) => Some(CronScheduleType::OneShot { at: dt }),
            Err(e) => {
                tracing::error!(
                    "Invalid datetime '{}' for job '{}': {}",
                    at_str,
                    config.name,
                    e
                );
                None
            }
        }
    } else {
        tracing::error!(
            "Cron job '{}' must have 'schedule', 'interval_seconds', or 'at'",
            config.name
        );
        None
    }
}

/// Parse the task from config.
fn parse_task(config: &CronTaskConfig) -> CronTask {
    match config {
        CronTaskConfig::Shell { command } => CronTask::Shell {
            command: command.clone(),
        },
        CronTaskConfig::AgentMessage { message } => CronTask::AgentMessage {
            message: message.clone(),
        },
        CronTaskConfig::Script { path } => CronTask::Script {
            path: PathBuf::from(path),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_parse_cron_expression_5_field() {
        // Standard 5-field cron: minute hour day month weekday
        let expr = "0 9 * * *"; // 9AM daily
        let expr_with_seconds = format!("0 {}", expr);
        let schedule = Schedule::from_str(&expr_with_seconds);
        assert!(schedule.is_ok(), "Failed to parse: {:?}", schedule.err());
    }

    #[test]
    fn test_parse_cron_expression_6_field() {
        // 6-field cron: second minute hour day month weekday
        let expr = "0 0 9 * * *"; // 9AM daily
        let schedule = Schedule::from_str(expr);
        assert!(schedule.is_ok(), "Failed to parse: {:?}", schedule.err());
    }

    #[test]
    fn test_parse_cron_weekday() {
        // Weekdays only
        let expr = "0 0 9 * * 1-5"; // 9AM Mon-Fri
        let schedule = Schedule::from_str(expr);
        assert!(schedule.is_ok(), "Failed to parse: {:?}", schedule.err());
    }

    #[test]
    fn test_parse_cron_every_6_hours() {
        let expr = "0 0 */6 * * *"; // Every 6 hours
        let schedule = Schedule::from_str(expr);
        assert!(schedule.is_ok(), "Failed to parse: {:?}", schedule.err());
    }

    #[test]
    fn test_parse_timezone() {
        let tz: Result<Tz, _> = "America/New_York".parse();
        assert!(tz.is_ok());

        let tz: Result<Tz, _> = "UTC".parse();
        assert!(tz.is_ok());

        let tz: Result<Tz, _> = "Invalid/Zone".parse();
        assert!(tz.is_err());
    }

    #[test]
    fn test_cron_next_occurrence() {
        let expr = "0 0 9 * * *"; // 9AM daily
        let schedule = Schedule::from_str(expr).unwrap();
        let tz: Tz = "America/New_York".parse().unwrap();

        let now = Utc::now().with_timezone(&tz);
        let next = schedule.after(&now).next();
        assert!(next.is_some());

        let next = next.unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn test_task_config_parsing() {
        // Shell task
        let shell_config = CronTaskConfig::Shell {
            command: "echo hello".to_string(),
        };
        let task = parse_task(&shell_config);
        assert!(matches!(task, CronTask::Shell { .. }));

        // AgentMessage task
        let msg_config = CronTaskConfig::AgentMessage {
            message: "Check email".to_string(),
        };
        let task = parse_task(&msg_config);
        assert!(matches!(task, CronTask::AgentMessage { .. }));

        // Script task
        let script_config = CronTaskConfig::Script {
            path: "/path/to/script.sh".to_string(),
        };
        let task = parse_task(&script_config);
        assert!(matches!(task, CronTask::Script { .. }));
    }

    #[test]
    fn test_parse_cron_jobs_from_config() {
        let config = CronConfig {
            timezone: "America/New_York".to_string(),
            jobs: vec![
                CronJobConfig {
                    name: "morning-briefing".to_string(),
                    schedule: Some("0 9 * * *".to_string()),
                    interval_seconds: None,
                    at: None,
                    task: CronTaskConfig::AgentMessage {
                        message: "Good morning!".to_string(),
                    },
                    enabled: true,
                    session_key: None,
                    channel: None,
                },
                CronJobConfig {
                    name: "memory-consolidate".to_string(),
                    schedule: Some("0 */6 * * *".to_string()),
                    interval_seconds: None,
                    at: None,
                    task: CronTaskConfig::Shell {
                        command: "engram consolidate".to_string(),
                    },
                    enabled: true,
                    session_key: None,
                    channel: None,
                },
            ],
        };

        let jobs = parse_cron_jobs(&config);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].name, "morning-briefing");
        assert_eq!(jobs[1].name, "memory-consolidate");
        assert!(matches!(jobs[0].schedule, CronScheduleType::Cron { .. }));
        assert!(matches!(jobs[0].task, CronTask::AgentMessage { .. }));
        assert!(matches!(jobs[1].task, CronTask::Shell { .. }));
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
        // Test with unicode
        let unicode = "你好世界";
        let truncated = truncate_str(unicode, 6);
        // Should truncate at char boundary
        assert!(truncated.len() <= 6 || truncated == "你好");
    }
}
