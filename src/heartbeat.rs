//! Heartbeat system — periodic autonomous agent actions.
//!
//! Runs at configurable intervals, reads HEARTBEAT.md for tasks.
//! Also handles memory consolidation.

use std::sync::Arc;

use crate::agent::AgentRunner;

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

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        // Skip first tick (immediate)
        interval.tick().await;

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
                    if response.trim() == "HEARTBEAT_OK" {
                        tracing::debug!("Heartbeat: nothing to do");
                    } else {
                        tracing::info!("Heartbeat response: {}", { let _end = response.len().min(200); let _end = response.floor_char_boundary(_end); &response[.._end] });
                        // TODO: Route heartbeat responses to appropriate channel
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
