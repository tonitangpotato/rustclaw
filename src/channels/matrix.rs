//! Matrix Channel via Client-Server API.
//!
//! Uses the Matrix client-server API (no SDK dependency).
//! Long-polling /sync for incoming messages, HTTP POST for sending.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::AgentRunner;

/// Matrix channel using the client-server API.
pub struct MatrixChannel {
    config: MatrixConfig,
    runner: Arc<AgentRunner>,
    client: reqwest::Client,
    /// Since token for sync (persisted across requests).
    since_token: Arc<RwLock<Option<String>>>,
}

/// Configuration for the Matrix channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    /// Matrix homeserver URL (e.g., "https://matrix.org").
    pub homeserver_url: String,
    /// Access token from login or registration.
    pub access_token: String,
    /// Bot's user ID (e.g., "@bot:matrix.org").
    pub user_id: String,
    /// Allowed room IDs (empty = allow all joined rooms).
    #[serde(default)]
    pub allowed_rooms: Vec<String>,
    /// Group policy: "mention" (only respond to mentions), "open" (all messages), "off".
    #[serde(default = "default_group_policy")]
    pub group_policy: String,
    /// Sync timeout in milliseconds (for long-polling).
    #[serde(default = "default_sync_timeout")]
    pub sync_timeout_ms: u64,
}

fn default_group_policy() -> String {
    "mention".to_string()
}

fn default_sync_timeout() -> u64 {
    30000 // 30 seconds
}

impl MatrixChannel {
    /// Create a new Matrix channel.
    pub fn new(config: MatrixConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            config,
            runner,
            client: reqwest::Client::new(),
            since_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the sync loop for incoming messages.
    pub async fn start(&self) -> anyhow::Result<()> {
        tracing::info!(
            "Starting Matrix channel for {} on {}",
            self.config.user_id,
            self.config.homeserver_url
        );

        // Initial sync to get the since token (no timeout, just get current state)
        self.initial_sync().await?;

        // Start the main sync loop
        self.sync_loop().await
    }

    /// Perform initial sync to get the since token.
    async fn initial_sync(&self) -> anyhow::Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/sync?filter={{\"room\":{{\"timeline\":{{\"limit\":0}}}}}}",
            self.config.homeserver_url
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Initial sync failed: {}", error);
        }

        let sync_response: SyncResponse = response.json().await?;
        
        let mut token = self.since_token.write().await;
        *token = Some(sync_response.next_batch);

        tracing::debug!("Initial sync complete, got since token");
        Ok(())
    }

    /// Main sync loop - long-poll for new events.
    async fn sync_loop(&self) -> anyhow::Result<()> {
        loop {
            if let Err(e) = self.do_sync().await {
                tracing::error!("Sync error: {}. Retrying in 5s...", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }

    /// Perform a single sync request.
    async fn do_sync(&self) -> anyhow::Result<()> {
        let since = self.since_token.read().await.clone();

        let url = match &since {
            Some(token) => format!(
                "{}/_matrix/client/v3/sync?since={}&timeout={}",
                self.config.homeserver_url, token, self.config.sync_timeout_ms
            ),
            None => format!(
                "{}/_matrix/client/v3/sync?timeout={}",
                self.config.homeserver_url, self.config.sync_timeout_ms
            ),
        };

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .timeout(std::time::Duration::from_millis(
                self.config.sync_timeout_ms + 10000,
            ))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Sync request failed: {}", error);
        }

        let sync_response: SyncResponse = response.json().await?;

        // Update since token
        {
            let mut token = self.since_token.write().await;
            *token = Some(sync_response.next_batch);
        }

        // Process room events
        if let Some(rooms) = sync_response.rooms {
            // Process joined rooms
            if let Some(joined) = rooms.join {
                for (room_id, room_data) in joined {
                    if let Some(timeline) = room_data.timeline {
                        for event in timeline.events {
                            self.handle_event(&room_id, &event).await?;
                        }
                    }
                }
            }

            // Auto-join invited rooms
            if let Some(invited) = rooms.invite {
                for (room_id, _) in invited {
                    tracing::info!("Received invite to room {}", room_id);
                    if let Err(e) = self.join_room(&room_id).await {
                        tracing::warn!("Failed to auto-join room {}: {}", room_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle a room event.
    async fn handle_event(&self, room_id: &str, event: &serde_json::Value) -> anyhow::Result<()> {
        // Only handle m.room.message events
        let event_type = event["type"].as_str().unwrap_or("");
        if event_type != "m.room.message" {
            return Ok(());
        }

        // Skip our own messages
        let sender = event["sender"].as_str().unwrap_or("");
        if sender == self.config.user_id {
            return Ok(());
        }

        // Check if room is allowed
        if !self.config.allowed_rooms.is_empty()
            && !self.config.allowed_rooms.contains(&room_id.to_string())
        {
            tracing::debug!("Ignoring message from non-allowed room: {}", room_id);
            return Ok(());
        }

        // Get message content
        let content = &event["content"];
        let msgtype = content["msgtype"].as_str().unwrap_or("");
        let body = content["body"].as_str().unwrap_or("");

        if body.is_empty() {
            return Ok(());
        }

        // Check group policy
        let is_direct = self.is_direct_message(room_id).await;
        
        if !is_direct {
            match self.config.group_policy.as_str() {
                "off" => {
                    tracing::debug!("Ignoring group message (policy: off)");
                    return Ok(());
                }
                "mention" => {
                    // Check if bot is mentioned
                    let mention_patterns = [
                        self.config.user_id.as_str(),
                        // Also check display name (localpart)
                        self.config.user_id.split(':').next().unwrap_or("").trim_start_matches('@'),
                    ];
                    
                    let is_mentioned = mention_patterns.iter().any(|p| {
                        !p.is_empty() && body.to_lowercase().contains(&p.to_lowercase())
                    });

                    if !is_mentioned {
                        tracing::debug!("Ignoring group message (no mention)");
                        return Ok(());
                    }
                }
                "open" => {
                    // Respond to all messages
                }
                _ => {
                    // Default to mention policy
                    return Ok(());
                }
            }
        }

        // Build message text based on type
        let text = match msgtype {
            "m.text" => body.to_string(),
            "m.image" => format!("[Image] {}", body),
            "m.video" => format!("[Video] {}", body),
            "m.audio" => format!("[Audio] {}", body),
            "m.file" => format!("[File] {}", body),
            "m.location" => format!("[Location] {}", body),
            _ => format!("[{}] {}", msgtype, body),
        };

        tracing::info!(
            "Matrix message from {} in {}: {}",
            sender,
            room_id,
            text.chars().take(50).collect::<String>()
        );

        // Build session key
        let session_key = format!("matrix:{}:{}", room_id, sender);

        // Process with agent
        match self
            .runner
            .process_message(&session_key, &text, Some(sender), Some("matrix"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty()
                    && trimmed != "NO_REPLY"
                    && trimmed != "HEARTBEAT_OK"
                {
                    self.send_message(room_id, trimmed).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error for Matrix message: {}", e);
                self.send_message(room_id, &format!("⚠️ Error: {}", e)).await?;
            }
        }

        Ok(())
    }

    /// Check if a room is a direct message (DM) room.
    async fn is_direct_message(&self, _room_id: &str) -> bool {
        // For simplicity, assume rooms with 2 members are DMs
        // A more accurate check would query room state
        // TODO: Implement proper DM detection via m.direct account data
        false
    }

    /// Send a text message to a room.
    pub async fn send_message(&self, room_id: &str, text: &str) -> anyhow::Result<()> {
        let txn_id = uuid::Uuid::new_v4().to_string();
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.config.homeserver_url,
            urlencoding::encode(room_id),
            txn_id
        );

        let payload = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
            // Support basic markdown
            "format": "org.matrix.custom.html",
            "formatted_body": markdown_to_html(text)
        });

        let response = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to send Matrix message: {}", error);
        }

        tracing::debug!("Sent Matrix message to {}", room_id);
        Ok(())
    }

    /// Send a file to a room.
    pub async fn send_file(&self, room_id: &str, file_path: &str) -> anyhow::Result<()> {
        // First, upload the file
        let file_bytes = tokio::fs::read(file_path).await?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        // Detect content type
        let content_type = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();

        // Upload to media server
        let upload_url = format!(
            "{}/_matrix/media/v3/upload?filename={}",
            self.config.homeserver_url,
            urlencoding::encode(file_name)
        );

        let upload_response = self
            .client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", &content_type)
            .body(file_bytes)
            .send()
            .await?;

        if !upload_response.status().is_success() {
            let error = upload_response.text().await?;
            anyhow::bail!("Failed to upload file: {}", error);
        }

        let upload_result: UploadResponse = upload_response.json().await?;
        let mxc_uri = upload_result.content_uri;

        // Determine message type based on content type
        let msgtype = if content_type.starts_with("image/") {
            "m.image"
        } else if content_type.starts_with("video/") {
            "m.video"
        } else if content_type.starts_with("audio/") {
            "m.audio"
        } else {
            "m.file"
        };

        // Send the file message
        let txn_id = uuid::Uuid::new_v4().to_string();
        let msg_url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.config.homeserver_url,
            urlencoding::encode(room_id),
            txn_id
        );

        let payload = serde_json::json!({
            "msgtype": msgtype,
            "body": file_name,
            "url": mxc_uri,
            "info": {
                "mimetype": content_type
            }
        });

        let response = self
            .client
            .put(&msg_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to send file message: {}", error);
        }

        tracing::debug!("Sent file {} to {}", file_name, room_id);
        Ok(())
    }

    /// Join a room by room ID or alias.
    pub async fn join_room(&self, room_id: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/join/{}",
            self.config.homeserver_url,
            urlencoding::encode(room_id)
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to join room: {}", error);
        }

        tracing::info!("Joined room {}", room_id);
        Ok(())
    }

    /// Leave a room.
    pub async fn leave_room(&self, room_id: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/leave",
            self.config.homeserver_url,
            urlencoding::encode(room_id)
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("Failed to leave room: {}", error);
        }

        tracing::info!("Left room {}", room_id);
        Ok(())
    }

    /// Set typing indicator in a room.
    pub async fn set_typing(&self, room_id: &str, typing: bool) -> anyhow::Result<()> {
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/typing/{}",
            self.config.homeserver_url,
            urlencoding::encode(room_id),
            urlencoding::encode(&self.config.user_id)
        );

        let payload = serde_json::json!({
            "typing": typing,
            "timeout": 30000
        });

        let _ = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await;

        Ok(())
    }
}

/// Sync response structure.
#[derive(Debug, Deserialize)]
struct SyncResponse {
    next_batch: String,
    rooms: Option<RoomsResponse>,
}

#[derive(Debug, Deserialize)]
struct RoomsResponse {
    join: Option<std::collections::HashMap<String, JoinedRoom>>,
    invite: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct JoinedRoom {
    timeline: Option<Timeline>,
}

#[derive(Debug, Deserialize)]
struct Timeline {
    events: Vec<serde_json::Value>,
}

/// Upload response structure.
#[derive(Debug, Deserialize)]
struct UploadResponse {
    content_uri: String,
}

/// Simple markdown to HTML conversion.
fn markdown_to_html(text: &str) -> String {
    // Very basic conversion - just handle bold, italic, code
    let mut html = text.to_string();
    
    // Code blocks (must come before inline code)
    html = regex::Regex::new(r"```(\w*)\n([\s\S]*?)```")
        .unwrap()
        .replace_all(&html, "<pre><code>$2</code></pre>")
        .to_string();
    
    // Inline code
    html = regex::Regex::new(r"`([^`]+)`")
        .unwrap()
        .replace_all(&html, "<code>$1</code>")
        .to_string();
    
    // Bold
    html = regex::Regex::new(r"\*\*([^*]+)\*\*")
        .unwrap()
        .replace_all(&html, "<strong>$1</strong>")
        .to_string();
    
    // Italic
    html = regex::Regex::new(r"\*([^*]+)\*")
        .unwrap()
        .replace_all(&html, "<em>$1</em>")
        .to_string();
    
    // Links
    html = regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)")
        .unwrap()
        .replace_all(&html, "<a href=\"$2\">$1</a>")
        .to_string();
    
    html
}

/// Start the Matrix channel (convenience function).
pub async fn start(config: MatrixConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let channel = MatrixChannel::new(config, runner);
    channel.start().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_html() {
        assert_eq!(
            markdown_to_html("**bold** and *italic*"),
            "<strong>bold</strong> and <em>italic</em>"
        );
        
        assert_eq!(
            markdown_to_html("inline `code` here"),
            "inline <code>code</code> here"
        );
        
        assert_eq!(
            markdown_to_html("[link](https://example.com)"),
            "<a href=\"https://example.com\">link</a>"
        );
    }

    #[test]
    fn test_default_config() {
        assert_eq!(default_group_policy(), "mention");
        assert_eq!(default_sync_timeout(), 30000);
    }
}
