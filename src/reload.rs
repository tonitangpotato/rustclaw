//! Config hot-reload via file watching + SIGHUP signal.
//!
//! Watches rustclaw.yaml for changes and reloads safe-to-update fields:
//! - Heartbeat interval
//! - Cron jobs
//! - Safety/hook settings
//! - Telegram allowed_users, group_policy, dm_policy
//! - Orchestrator specialists (add/remove/modify)
//!
//! NOT reloaded (requires restart):
//! - LLM provider/model/auth
//! - Memory/engram DB path
//! - Bot token (Telegram long-poll is bound at startup)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::Config;

/// Sender half — main holds this, receivers get updates.
pub type ConfigSender = watch::Sender<Arc<Config>>;
/// Receiver half — components subscribe to config changes.
pub type ConfigReceiver = watch::Receiver<Arc<Config>>;

/// Start watching config file for changes.
/// Returns a receiver that components can clone to get notified of reloads.
pub fn start_config_watcher(
    config_path: &str,
    initial_config: Config,
) -> anyhow::Result<(ConfigSender, ConfigReceiver, RecommendedWatcher)> {
    let (tx, rx) = watch::channel(Arc::new(initial_config));

    let path = PathBuf::from(config_path);
    let tx_clone = tx.clone();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    reload_config(&path, &tx_clone);
                }
            }
            Err(e) => warn!(error = %e, "Config watcher error"),
        }
    })?;

    let watch_path = Path::new(config_path);
    // Watch the parent directory (some editors do atomic save = delete + create)
    let watch_dir = watch_path.parent().unwrap_or(Path::new("."));
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    info!(path = config_path, "Config hot-reload watcher started");

    Ok((tx, rx, watcher))
}

/// Start SIGHUP listener for manual reload trigger.
pub async fn start_sighup_listener(config_path: String, tx: ConfigSender) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "Failed to register SIGHUP handler");
                return;
            }
        };

        let path = PathBuf::from(config_path);
        tokio::spawn(async move {
            loop {
                sighup.recv().await;
                info!("SIGHUP received — reloading config");
                reload_config(&path, &tx);
            }
        });
    }
}

fn reload_config(path: &Path, tx: &ConfigSender) {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            match serde_yaml::from_str::<Config>(&content) {
                Ok(new_config) => {
                    let old = tx.borrow().clone();
                    let changes = diff_config(&old, &new_config);
                    if changes.is_empty() {
                        return; // No meaningful changes
                    }
                    info!(changes = ?changes, "Config reloaded");
                    let _ = tx.send(Arc::new(new_config));
                }
                Err(e) => {
                    error!(error = %e, "Failed to parse config — keeping current");
                }
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to read config file");
        }
    }
}

/// Detect what changed between two configs (human-readable summary).
fn diff_config(old: &Config, new: &Config) -> Vec<String> {
    let mut changes = Vec::new();

    if old.heartbeat_interval != new.heartbeat_interval {
        changes.push(format!(
            "heartbeat_interval: {}s → {}s",
            old.heartbeat_interval, new.heartbeat_interval
        ));
    }

    if old.max_session_messages != new.max_session_messages {
        changes.push(format!(
            "max_session_messages: {} → {}",
            old.max_session_messages, new.max_session_messages
        ));
    }

    if old.cron.jobs.len() != new.cron.jobs.len() || old.cron.timezone != new.cron.timezone {
        changes.push(format!(
            "cron: {} jobs ({}) → {} jobs ({})",
            old.cron.jobs.len(),
            old.cron.timezone,
            new.cron.jobs.len(),
            new.cron.timezone
        ));
    }

    if old.llm.model != new.llm.model {
        changes.push(format!("llm.model: {} → {}", old.llm.model, new.llm.model));
    }

    if old.llm.temperature != new.llm.temperature {
        changes.push(format!(
            "llm.temperature: {} → {}",
            old.llm.temperature, new.llm.temperature
        ));
    }

    if old.llm.max_tokens != new.llm.max_tokens {
        changes.push(format!(
            "llm.max_tokens: {} → {}",
            old.llm.max_tokens, new.llm.max_tokens
        ));
    }

    // Check orchestrator/specialist changes
    if old.orchestrator.enabled != new.orchestrator.enabled {
        changes.push(format!(
            "orchestrator.enabled: {} → {}",
            old.orchestrator.enabled, new.orchestrator.enabled
        ));
    }
    if old.orchestrator.specialists.len() != new.orchestrator.specialists.len() {
        changes.push(format!(
            "orchestrator.specialists: {} → {}",
            old.orchestrator.specialists.len(),
            new.orchestrator.specialists.len()
        ));
    }

    // Check telegram config changes
    match (&old.channels.telegram, &new.channels.telegram) {
        (Some(old_tg), Some(new_tg)) => {
            if old_tg.dm_policy != new_tg.dm_policy {
                changes.push(format!("telegram.dm_policy: {} → {}", old_tg.dm_policy, new_tg.dm_policy));
            }
            if old_tg.group_policy != new_tg.group_policy {
                changes.push(format!("telegram.group_policy: {} → {}", old_tg.group_policy, new_tg.group_policy));
            }
            if old_tg.allowed_users != new_tg.allowed_users {
                changes.push("telegram.allowed_users changed".to_string());
            }
        }
        _ => {}
    }

    changes
}
