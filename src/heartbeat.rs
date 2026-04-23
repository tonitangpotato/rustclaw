//! Heartbeat system — periodic autonomous agent actions.
//!
//! Runs at configurable intervals, reads HEARTBEAT.md for tasks.
//! Also handles memory consolidation.
//! Non-OK responses are routed to the configured Telegram channel.
//!
//! Configuration (rustclaw.yaml):
//! ```yaml
//! heartbeat:
//!   enabled: true
//!   interval: 3600          # seconds (default: 1 hour)
//!   model: claude-haiku-4-5 # cheaper model for heartbeats (optional)
//!   prompt: "Read HEARTBEAT.md..." # custom prompt (optional)
//!   quiet_hours: [23, 8]    # no heartbeats 23:00-08:00 (optional)
//! ```

use std::sync::Arc;

use chrono::Timelike;

use crate::agent::AgentRunner;
use crate::config::{Config, HeartbeatConfig};

/// Telegram API base URL.
const TELEGRAM_API: &str = "https://api.telegram.org";

/// Start the heartbeat loop.
pub async fn start_heartbeat(
    runner: Arc<AgentRunner>,
    hb_config: &HeartbeatConfig,
    session_key: &str,
) -> anyhow::Result<()> {
    if !hb_config.enabled || hb_config.interval == 0 {
        tracing::info!("Heartbeat disabled");
        return Ok(());
    }

    let session_key = session_key.to_string();
    let interval_secs = hb_config.interval;
    let prompt = hb_config.prompt.clone();
    let model = hb_config.model.clone();
    let quiet_hours = hb_config.quiet_hours;

    tracing::info!(
        "Heartbeat started: every {}s, model={}, quiet_hours={:?}",
        interval_secs,
        model.as_deref().unwrap_or("(default)"),
        quiet_hours
    );

    // Clone config for Telegram routing
    let config = runner.config().clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        // Skip first tick (immediate)
        interval.tick().await;

        // Create HTTP client for Telegram routing
        let client = reqwest::Client::new();

        loop {
            interval.tick().await;

            // Check quiet hours
            if let Some([start, end]) = quiet_hours {
                let now = chrono::Local::now();
                let hour = now.hour() as u8;
                let in_quiet = if start <= end {
                    hour >= start && hour < end
                } else {
                    // Wraps midnight: e.g., [23, 8] means 23:00-08:00
                    hour >= start || hour < end
                };
                if in_quiet {
                    tracing::debug!("Heartbeat skipped: quiet hours ({:02}:00-{:02}:00)", start, end);
                    continue;
                }
            }

            tracing::debug!("Heartbeat tick");

            // Run interoceptive cycle: tick (pull signals) + evaluate (generate actions)
            if let Some(ref mem) = runner.memory() {
                match mem.interoceptive_cycle() {
                    Ok(actions) => {
                        if !actions.is_empty() {
                            tracing::info!("Interoceptive regulation: {} actions", actions.len());
                            for action in &actions {
                                match action {
                                    engramai::interoceptive::RegulationAction::SoulUpdateSuggestion { domain, reason, .. } => {
                                        tracing::info!("🧠 Soul suggestion [{}]: {}", domain, reason);
                                    }
                                    engramai::interoceptive::RegulationAction::RetrievalAdjustment { reason, .. } => {
                                        tracing::info!("🔍 Retrieval adjustment: {}", reason);
                                    }
                                    engramai::interoceptive::RegulationAction::BehaviorShift { action, recommendation, .. } => {
                                        tracing::info!("⚡ Behavior shift [{}]: {}", action, recommendation);
                                    }
                                    engramai::interoceptive::RegulationAction::Alert { severity, message, .. } => {
                                        tracing::warn!("🚨 Alert [{}]: {}", severity, message);
                                        // High-severity alerts route to Telegram
                                        if matches!(severity, engramai::interoceptive::AlertSeverity::High) {
                                            let alert_msg = format!("🧠 Interoceptive Alert [{}]: {}", severity, message);
                                            if let Err(e) = route_to_telegram(&client, &config, &alert_msg).await {
                                                tracing::warn!("Failed to route interoceptive alert: {}", e);
                                            }
                                        }
                                    }
                                    engramai::interoceptive::RegulationAction::IdentityEvolutionSuggestion { observation, suggestion, .. } => {
                                        tracing::info!("🪞 Identity evolution: {} → {}", observation, suggestion);
                                    }
                                    engramai::interoceptive::RegulationAction::HeartbeatFrequencyAdjustment { direction, interval_multiplier, reason, .. } => {
                                        let dir_str = match direction {
                                            engramai::interoceptive::HeartbeatAdjustDirection::Increase => "⬆️ increase",
                                            engramai::interoceptive::HeartbeatAdjustDirection::Decrease => "⬇️ decrease",
                                        };
                                        tracing::info!("💓 Heartbeat {} (×{:.2}): {}", dir_str, interval_multiplier, reason);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Interoceptive cycle failed (non-fatal): {}", e);
                    }
                }
            }

            // TODO: If model override is set, temporarily switch the agent's model
            // For now, the model override is logged but not yet wired into process_message
            if let Some(ref m) = model {
                tracing::debug!("Heartbeat using model override: {}", m);
            }

            match runner
                .process_message_with_options(&session_key, &prompt, None, None, true)
                .await
            {
                Ok(response) => {
                    let trimmed = response.trim();
                    if trimmed == "HEARTBEAT_OK" {
                        tracing::debug!("Heartbeat: nothing to do");
                    } else {
                        tracing::info!("Heartbeat response: {}", {
                            let end = trimmed.len().min(200);
                            let end = trimmed.floor_char_boundary(end);
                            &trimmed[..end]
                        });

                        // Route non-OK responses to Telegram (if configured)
                        if let Err(e) = route_to_telegram(&client, &config, trimmed).await {
                            tracing::warn!("Failed to route heartbeat to Telegram: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Heartbeat error: {}", e);
                }
            }
        }
    });

    Ok(())
}

/// Route a heartbeat response to the configured operational recipients.
/// Uses the same three-tier resolver as lifecycle notifications
/// (notify_chat_ids → autodiscovered → allowed_users). See
/// `src/notify_targets.rs` for the rationale.
async fn route_to_telegram(
    client: &reqwest::Client,
    config: &Config,
    message: &str,
) -> anyhow::Result<()> {
    let tg_config = match &config.channels.telegram {
        Some(c) => c,
        None => return Ok(()), // Telegram not configured
    };

    let autodisc = crate::notify_targets::load_autodiscovered();
    let (chat_ids, src) = crate::notify_targets::resolve_recipients(
        &tg_config.notify_chat_ids,
        &autodisc,
        &tg_config.allowed_users,
    );
    if chat_ids.is_empty() {
        tracing::warn!(
            "Heartbeat alert SKIPPED: 0 recipients (source: {}). \
             Set telegram.notify_chat_ids or message the bot once.",
            src.as_str()
        );
        return Ok(());
    }
    // Only send to the primary (most-recent / first-listed) recipient for
    // heartbeat alerts — they can be noisy, don't want to fan out.
    let chat_id = chat_ids[0];

    let url = format!("{}/bot{}/sendMessage", TELEGRAM_API, tg_config.bot_token);

    let payload = serde_json::json!({
        "chat_id": chat_id,
        "text": format!("🔔 *Heartbeat Alert*\n\n{}", message),
        "parse_mode": "Markdown",
    });

    let resp = client.post(&url).json(&payload).send().await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Telegram API error: {}", body);
    }

    tracing::debug!(
        "Heartbeat routed to Telegram chat {} (via {})",
        chat_id,
        src.as_str()
    );
    Ok(())
}
