//! Instance Manager — manages per-user Telegram bot polling loops.
//!
//! For MVP: each user gets a tokio task that long-polls their Telegram bot
//! via the Bot API (getUpdates). Messages are logged but not yet routed
//! to a shared AgentRunner (that integration comes later).

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::platform::db::PlatformDb;

// ─── Types ──────────────────────────────────────────────────

pub struct InstanceManager {
    instances: Arc<RwLock<HashMap<i64, UserInstance>>>,
    db: Arc<PlatformDb>,
}

pub struct UserInstance {
    pub user_id: i64,
    pub bot_token: String,
    pub status: String,
    /// Handle to the running polling task; dropping it won't cancel — we use a flag.
    pub handle: Option<tokio::task::JoinHandle<()>>,
    /// Shared shutdown signal.
    pub shutdown: Arc<tokio::sync::Notify>,
}

// Telegram API response types (minimal)
#[derive(Debug, Deserialize)]
struct TgResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    message_id: i64,
    chat: TgChat,
    text: Option<String>,
    from: Option<TgUser>,
}

#[derive(Debug, Deserialize)]
struct TgChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
    first_name: String,
    username: Option<String>,
}

// ─── Implementation ─────────────────────────────────────────

impl InstanceManager {
    pub fn new(db: Arc<PlatformDb>) -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            db,
        }
    }

    /// Start a Telegram polling loop for a user's bot.
    pub async fn start_instance(&self, user_id: i64, bot_token: &str) -> Result<()> {
        // Stop any existing instance first.
        self.stop_instance(user_id).await.ok();

        // Validate the bot token by calling getMe.
        let client = reqwest::Client::new();
        let url = format!("https://api.telegram.org/bot{}/getMe", bot_token);
        let resp: TgResponse<serde_json::Value> = client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to reach Telegram API: {}", e))?
            .json()
            .await
            .map_err(|e| anyhow!("Invalid Telegram response: {}", e))?;

        if !resp.ok {
            return Err(anyhow!(
                "Invalid bot token: {}",
                resp.description.unwrap_or_default()
            ));
        }

        let bot_name = resp
            .result
            .and_then(|v| v.get("username").and_then(|u| u.as_str().map(String::from)))
            .unwrap_or_else(|| "unknown".into());

        tracing::info!(
            "Starting Telegram instance for user {} (bot: @{})",
            user_id,
            bot_name
        );

        let shutdown = Arc::new(tokio::sync::Notify::new());
        let shutdown_rx = Arc::clone(&shutdown);
        let token = bot_token.to_string();
        let uid = user_id;

        let handle = tokio::spawn(async move {
            telegram_poll_loop(uid, &token, shutdown_rx).await;
        });

        let mut map = self.instances.write().await;
        map.insert(
            user_id,
            UserInstance {
                user_id,
                bot_token: bot_token.to_string(),
                status: "active".into(),
                handle: Some(handle),
                shutdown,
            },
        );

        Ok(())
    }

    /// Stop a user's polling loop.
    pub async fn stop_instance(&self, user_id: i64) -> Result<()> {
        let mut map = self.instances.write().await;
        if let Some(inst) = map.remove(&user_id) {
            inst.shutdown.notify_one();
            if let Some(h) = inst.handle {
                h.abort();
            }
            tracing::info!("Stopped instance for user {}", user_id);
        }
        // Update DB status.
        if let Ok(Some(db_inst)) = self.db.get_instance(user_id).await {
            self.db
                .update_instance_status(db_inst.id, "stopped")
                .await
                .ok();
        }
        Ok(())
    }

    /// On startup, restart all active instances from the database.
    pub async fn restart_all(&self) -> Result<()> {
        let active = self.db.list_active_instances().await?;
        tracing::info!("Restoring {} active instance(s)", active.len());

        for inst in active {
            if let Err(e) = self.start_instance(inst.user_id, &inst.bot_token).await {
                tracing::warn!(
                    "Failed to restart instance for user {}: {}",
                    inst.user_id,
                    e
                );
                // Mark it as error in DB.
                self.db
                    .update_instance_status(inst.id, "error")
                    .await
                    .ok();
            }
        }

        Ok(())
    }

    /// Get the runtime status of a user's instance.
    pub async fn get_status(&self, user_id: i64) -> Option<String> {
        let map = self.instances.read().await;
        map.get(&user_id).map(|i| i.status.clone())
    }
}

// ─── Telegram Long-Polling Loop ─────────────────────────────

async fn telegram_poll_loop(user_id: i64, bot_token: &str, shutdown: Arc<tokio::sync::Notify>) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(35))
        .build()
        .unwrap_or_default();

    let base = format!("https://api.telegram.org/bot{}", bot_token);
    let mut offset: i64 = 0;

    loop {
        // Check shutdown.
        tokio::select! {
            _ = shutdown.notified() => {
                tracing::info!("Instance for user {} received shutdown signal", user_id);
                return;
            }
            result = fetch_updates(&client, &base, offset) => {
                match result {
                    Ok(updates) => {
                        for update in updates {
                            offset = update.update_id + 1;
                            handle_update(user_id, &client, &base, &update).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Polling error for user {}: {}", user_id, e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }
}

async fn fetch_updates(
    client: &reqwest::Client,
    base: &str,
    offset: i64,
) -> Result<Vec<TgUpdate>> {
    let url = format!(
        "{}/getUpdates?offset={}&timeout=30&allowed_updates=[\"message\"]",
        base, offset
    );

    let resp: TgResponse<Vec<TgUpdate>> = client.get(&url).send().await?.json().await?;

    if resp.ok {
        Ok(resp.result.unwrap_or_default())
    } else {
        Err(anyhow!(
            "Telegram API error: {}",
            resp.description.unwrap_or_default()
        ))
    }
}

/// Handle a single Telegram update — for MVP, echo-acknowledge and log it.
/// TODO: Route to shared AgentRunner for real AI responses.
async fn handle_update(user_id: i64, client: &reqwest::Client, base: &str, update: &TgUpdate) {
    let msg = match &update.message {
        Some(m) => m,
        None => return,
    };

    let text = msg.text.as_deref().unwrap_or("");
    let from_name = msg
        .from
        .as_ref()
        .map(|u| u.first_name.as_str())
        .unwrap_or("unknown");

    tracing::info!(
        "[user:{}] Telegram message from {}: {}",
        user_id,
        from_name,
        text
    );

    // MVP: Send an acknowledgment reply.
    let reply = format!(
        "✅ Your AI assistant received your message. Full agent integration coming soon!\n\nYou said: \"{}\"",
        text.chars().take(200).collect::<String>()
    );

    let send_url = format!("{}/sendMessage", base);
    let _ = client
        .post(&send_url)
        .json(&serde_json::json!({
            "chat_id": msg.chat.id,
            "text": reply,
        }))
        .send()
        .await;
}
