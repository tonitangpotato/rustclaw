//! Channel adapters for messaging platforms.

pub mod telegram;

use crate::agent::AgentRunner;
use crate::config::Config;

/// Start the messaging gateway with all configured channels.
pub async fn start_gateway(config: Config, runner: AgentRunner) -> anyhow::Result<()> {
    let runner = std::sync::Arc::new(runner);

    // Start Telegram if configured
    if let Some(tg_config) = &config.channels.telegram {
        tracing::info!("Starting Telegram channel...");
        telegram::start(tg_config.clone(), runner.clone()).await?;
    } else {
        tracing::warn!("No channels configured. Add a channel in rustclaw.yaml.");
    }

    Ok(())
}
