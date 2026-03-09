//! Simple cron system for scheduled agent tasks.
//!
//! Supports:
//! - Interval-based jobs (every N seconds)
//! - One-shot jobs (at specific time)
//! - Jobs inject a message into the agent loop

use std::sync::Arc;

use chrono::{Local, NaiveDateTime};
use tokio::sync::mpsc;

use crate::agent::AgentRunner;

/// A cron job definition.
#[derive(Debug, Clone)]
pub struct CronJob {
    /// Unique job ID.
    pub id: String,
    /// Job type.
    pub job_type: CronJobType,
    /// Message to inject into agent.
    pub message: String,
    /// Session key for the job.
    pub session_key: String,
    /// Channel to deliver response (optional).
    pub channel: Option<String>,
    /// Whether the job is enabled.
    pub enabled: bool,
}

/// Type of cron job.
#[derive(Debug, Clone)]
pub enum CronJobType {
    /// Run every N seconds.
    Interval { seconds: u64 },
    /// Run once at a specific datetime.
    OneShot { at: NaiveDateTime },
}

/// Message sent when a cron job fires.
#[derive(Debug)]
pub struct CronTrigger {
    pub job_id: String,
    pub message: String,
    pub session_key: String,
    pub channel: Option<String>,
}

/// Cron scheduler.
pub struct CronScheduler {
    jobs: Vec<CronJob>,
    tx: mpsc::Sender<CronTrigger>,
}

impl CronScheduler {
    /// Create a new cron scheduler with the given jobs.
    pub fn new(jobs: Vec<CronJob>, tx: mpsc::Sender<CronTrigger>) -> Self {
        Self { jobs, tx }
    }

    /// Start the cron scheduler (spawns background tasks).
    pub async fn start(self) {
        for job in self.jobs {
            if !job.enabled {
                tracing::debug!("Cron job {} is disabled, skipping", job.id);
                continue;
            }

            let tx = self.tx.clone();
            let job_id = job.id.clone();

            match &job.job_type {
                CronJobType::Interval { seconds } => {
                    let interval_secs = *seconds;
                    tracing::info!(
                        "Cron job '{}' scheduled: every {}s",
                        job_id,
                        interval_secs
                    );

                    tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                        // Skip first tick
                        interval.tick().await;

                        loop {
                            interval.tick().await;
                            tracing::debug!("Cron job '{}' firing", job.id);

                            let trigger = CronTrigger {
                                job_id: job.id.clone(),
                                message: job.message.clone(),
                                session_key: job.session_key.clone(),
                                channel: job.channel.clone(),
                            };

                            if let Err(e) = tx.send(trigger).await {
                                tracing::error!("Failed to send cron trigger: {}", e);
                                break;
                            }
                        }
                    });
                }
                CronJobType::OneShot { at } => {
                    let target = *at;
                    tracing::info!("Cron job '{}' scheduled: one-shot at {}", job_id, target);

                    tokio::spawn(async move {
                        // Calculate delay until target time
                        let now = Local::now().naive_local();
                        if target <= now {
                            tracing::warn!(
                                "Cron job '{}' target time {} is in the past, skipping",
                                job.id,
                                target
                            );
                            return;
                        }

                        let duration = (target - now).to_std().unwrap_or_default();
                        tokio::time::sleep(duration).await;

                        tracing::debug!("Cron job '{}' firing (one-shot)", job.id);

                        let trigger = CronTrigger {
                            job_id: job.id.clone(),
                            message: job.message.clone(),
                            session_key: job.session_key.clone(),
                            channel: job.channel.clone(),
                        };

                        if let Err(e) = tx.send(trigger).await {
                            tracing::error!("Failed to send cron trigger: {}", e);
                        }
                    });
                }
            }
        }
    }
}

/// Process cron triggers by sending them to the agent.
pub async fn process_cron_triggers(
    mut rx: mpsc::Receiver<CronTrigger>,
    runner: Arc<AgentRunner>,
) {
    while let Some(trigger) = rx.recv().await {
        tracing::info!(
            "Processing cron job '{}': {}",
            trigger.job_id,
            { let _end = trigger.message.len().min(50); let _end = trigger.message.floor_char_boundary(_end); &trigger.message[.._end] }
        );

        match runner
            .process_message(
                &trigger.session_key,
                &trigger.message,
                None,
                trigger.channel.as_deref(),
            )
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty() && trimmed != "HEARTBEAT_OK" {
                    tracing::info!(
                        "Cron job '{}' response: {}",
                        trigger.job_id,
                        { let _end = trimmed.len().min(200); let _end = trimmed.floor_char_boundary(_end); &trimmed[.._end] }
                    );
                    // TODO: Route response to appropriate channel if configured
                }
            }
            Err(e) => {
                tracing::error!("Cron job '{}' error: {}", trigger.job_id, e);
            }
        }
    }
}

/// Start the cron system with jobs from config.
pub async fn start_cron(
    jobs: Vec<CronJob>,
    runner: Arc<AgentRunner>,
) -> anyhow::Result<()> {
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
pub fn parse_cron_jobs(config: &[crate::config::CronJobConfig]) -> Vec<CronJob> {
    config
        .iter()
        .filter_map(|c| {
            let job_type = if let Some(secs) = c.interval_seconds {
                CronJobType::Interval { seconds: secs }
            } else if let Some(at_str) = &c.at {
                match NaiveDateTime::parse_from_str(at_str, "%Y-%m-%d %H:%M:%S") {
                    Ok(dt) => CronJobType::OneShot { at: dt },
                    Err(e) => {
                        tracing::error!("Invalid cron 'at' format for job '{}': {}", c.id, e);
                        return None;
                    }
                }
            } else {
                tracing::error!(
                    "Cron job '{}' must have either 'interval_seconds' or 'at'",
                    c.id
                );
                return None;
            };

            Some(CronJob {
                id: c.id.clone(),
                job_type,
                message: c.message.clone(),
                session_key: c.session_key.clone().unwrap_or_else(|| format!("cron:{}", c.id)),
                channel: c.channel.clone(),
                enabled: c.enabled.unwrap_or(true),
            })
        })
        .collect()
}
