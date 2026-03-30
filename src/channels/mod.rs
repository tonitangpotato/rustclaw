//! Channel adapters for messaging platforms.
//!
//! Each channel implements the `Channel` trait for a unified interface.

pub mod discord;
pub mod matrix;
pub mod signal;
pub mod slack;
pub mod telegram;
pub mod whatsapp;

use std::sync::Arc;

use async_trait::async_trait;

use crate::agent::AgentRunner;
use crate::config::Config;
use crate::context::ChannelCapabilities;

/// Common trait for all messaging channels.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Returns the channel name (e.g., "telegram", "discord").
    fn name(&self) -> &str;

    /// Declare what this channel supports (formatting, reactions, voice, etc.).
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::default()
    }

    /// Start the channel's event loop (long-poll, gateway, etc.).
    /// This should run indefinitely until an error occurs.
    async fn start(&self, runner: Arc<AgentRunner>) -> anyhow::Result<()>;
}

/// Start the messaging gateway with all configured channels.
/// Spawns each channel as a separate task.
pub async fn start_gateway(
    config: Config,
    runner: Arc<AgentRunner>,
) -> anyhow::Result<()> {
    let mut handles = Vec::new();
    let mut any_channel = false;

    // Start Telegram if configured (auto-restart on failure)
    if let Some(tg_config) = &config.channels.telegram {
        tracing::info!("Starting Telegram channel...");
        any_channel = true;
        let tg_config = tg_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            loop {
                match telegram::start(tg_config.clone(), runner.clone()).await {
                    Ok(()) => {
                        tracing::warn!("Telegram channel exited normally (unexpected). Restarting in 5s...");
                    }
                    Err(e) => {
                        tracing::error!("Telegram channel error: {}. Restarting in 5s...", e);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }));
    }

    // Start Discord if configured
    if let Some(discord_config) = &config.channels.discord {
        tracing::info!("Starting Discord channel...");
        any_channel = true;
        let discord_config = discord_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = discord::start(discord_config, runner).await {
                tracing::error!("Discord channel error: {}", e);
            }
        }));
    }

    // Start Slack if configured
    if let Some(slack_config) = &config.channels.slack {
        tracing::info!("Starting Slack channel...");
        any_channel = true;
        let slack_config = slack_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = slack::start(slack_config, runner).await {
                tracing::error!("Slack channel error: {}", e);
            }
        }));
    }

    // Start Signal if configured
    if let Some(signal_config) = &config.channels.signal {
        tracing::info!("Starting Signal channel...");
        any_channel = true;
        let signal_config = signal_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = signal::start(signal_config, runner).await {
                tracing::error!("Signal channel error: {}", e);
            }
        }));
    }

    // Start WhatsApp if configured
    if let Some(wa_config) = &config.channels.whatsapp {
        tracing::info!("Starting WhatsApp channel...");
        any_channel = true;
        let wa_config = wa_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = whatsapp::start(wa_config, runner).await {
                tracing::error!("WhatsApp channel error: {}", e);
            }
        }));
    }

    // Start Matrix if configured
    if let Some(matrix_config) = &config.channels.matrix {
        tracing::info!("Starting Matrix channel...");
        any_channel = true;
        let matrix_config = matrix_config.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = matrix::start(matrix_config, runner).await {
                tracing::error!("Matrix channel error: {}", e);
            }
        }));
    }

    if !any_channel {
        tracing::warn!("No channels configured. Add a channel in rustclaw.yaml.");
        return Ok(());
    }

    // Wait for all channels (they should run forever unless error)
    for handle in handles {
        if let Err(e) = handle.await {
            tracing::error!("Channel task panicked: {:?}", e);
        }
    }

    Ok(())
}
