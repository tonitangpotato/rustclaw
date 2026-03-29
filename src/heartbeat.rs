//! Heartbeat system — periodic autonomous agent actions.
//!
//! Runs at configurable intervals, reads HEARTBEAT.md for tasks.
//! Also handles memory consolidation.
//! Non-OK responses are routed to the configured Telegram channel.

use std::sync::Arc;

use crate::agent::AgentRunner;
use crate::config::Config;

/// Telegram API base URL.
const TELEGRAM_API: &str = "https://api.telegram.org";

/// Start the heartbeat loop.
pub async fn start_heartbeat(
    runner: Arc<AgentRunner>,
    interval_secs: u64,
    session_key: &str,
) -> anyhow::Result<()> {
    if interval_secs == 0 {
        tracing::info!("Heartbeat disabled");
        return Ok(());
    }

    let session_key = session_key.to_string();
    tracing::info!("Heartbeat started: every {}s", interval_secs);

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

            tracing::debug!("Heartbeat tick");

            let prompt = "Read HEARTBEAT.md if it exists (workspace context). \
                Follow it strictly. Do not infer or repeat old tasks from prior chats. \
                If nothing needs attention, reply HEARTBEAT_OK.";

            match runner
                .process_message_with_options(&session_key, prompt, None, None, true)
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

/// Route a heartbeat response to the first owner user in Telegram config.
async fn route_to_telegram(
    client: &reqwest::Client,
    config: &Config,
    message: &str,
) -> anyhow::Result<()> {
    let tg_config = match &config.channels.telegram {
        Some(c) => c,
        None => return Ok(()), // Telegram not configured
    };

    // Find the first allowed user (typically the owner)
    let chat_id = match tg_config.allowed_users.first() {
        Some(id) => *id,
        None => return Ok(()), // No users configured
    };

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

    tracing::debug!("Heartbeat routed to Telegram chat {}", chat_id);
    Ok(())
}
