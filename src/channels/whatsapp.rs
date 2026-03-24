//! WhatsApp Channel via Cloud API.
//!
//! Uses the WhatsApp Business Cloud API for sending and receiving messages.
//! Incoming messages are received via webhook, outgoing via HTTP POST.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::agent::AgentRunner;

/// WhatsApp Cloud API base URL.
const WHATSAPP_API_URL: &str = "https://graph.facebook.com/v18.0";

/// WhatsApp channel using the Cloud API.
pub struct WhatsAppChannel {
    config: WhatsAppConfig,
    runner: Arc<AgentRunner>,
    client: reqwest::Client,
}

/// Configuration for the WhatsApp channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    /// WhatsApp Cloud API base URL (defaults to graph.facebook.com).
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// Access token from Meta Developer Portal.
    pub access_token: String,
    /// Phone number ID from WhatsApp Business Account.
    pub phone_number_id: String,
    /// Verify token for webhook verification.
    pub verify_token: String,
    /// Allowed phone numbers (empty = allow all).
    #[serde(default)]
    pub allowed_numbers: Vec<String>,
    /// Webhook server port.
    #[serde(default = "default_webhook_port")]
    pub webhook_port: u16,
}

fn default_api_url() -> String {
    WHATSAPP_API_URL.to_string()
}

fn default_webhook_port() -> u16 {
    8080
}

/// Shared state for webhook handlers.
struct AppState {
    channel: Arc<WhatsAppChannel>,
}

impl WhatsAppChannel {
    /// Create a new WhatsApp channel.
    pub fn new(config: WhatsAppConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            config,
            runner,
            client: reqwest::Client::new(),
        }
    }

    /// Start the webhook server for incoming messages.
    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        let state = Arc::new(AppState {
            channel: self.clone(),
        });

        let app = Router::new()
            .route("/webhook", get(verify_webhook))
            .route("/webhook", post(handle_webhook))
            .with_state(state);

        let addr = format!("0.0.0.0:{}", self.config.webhook_port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        
        tracing::info!("WhatsApp webhook server listening on {}", addr);
        
        axum::serve(listener, app).await?;
        
        Ok(())
    }

    /// Send a text message to a phone number.
    pub async fn send_message(&self, to: &str, text: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}/messages",
            self.config.api_url, self.config.phone_number_id
        );

        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": "text",
            "text": {
                "preview_url": false,
                "body": text
            }
        });

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("Failed to send WhatsApp message: {}", error_body);
        }

        tracing::debug!("Sent WhatsApp message to {}", to);
        Ok(())
    }

    /// Send a media message (image, video, document, audio).
    pub async fn send_media(
        &self,
        to: &str,
        media_type: &str,
        url: &str,
        caption: Option<&str>,
    ) -> anyhow::Result<()> {
        let api_url = format!(
            "{}/{}/messages",
            self.config.api_url, self.config.phone_number_id
        );

        let media_object = match caption {
            Some(cap) => serde_json::json!({
                "link": url,
                "caption": cap
            }),
            None => serde_json::json!({
                "link": url
            }),
        };

        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": to,
            "type": media_type,
            media_type: media_object
        });

        let response = self
            .client
            .post(&api_url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("Failed to send WhatsApp media: {}", error_body);
        }

        tracing::debug!("Sent WhatsApp {} to {}", media_type, to);
        Ok(())
    }

    /// Mark a message as read.
    pub async fn mark_read(&self, message_id: &str) -> anyhow::Result<()> {
        let url = format!(
            "{}/{}/messages",
            self.config.api_url, self.config.phone_number_id
        );

        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "status": "read",
            "message_id": message_id
        });

        let _ = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.access_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        Ok(())
    }

    /// Handle an incoming webhook payload.
    async fn handle_webhook(&self, payload: serde_json::Value) -> anyhow::Result<()> {
        // Parse the webhook payload
        // WhatsApp Cloud API structure:
        // {
        //   "entry": [{
        //     "changes": [{
        //       "value": {
        //         "messages": [{...}],
        //         "metadata": {...}
        //       }
        //     }]
        //   }]
        // }

        let empty_vec: Vec<serde_json::Value> = vec![];
        let entries = payload["entry"].as_array().unwrap_or(&empty_vec);

        for entry in entries {
            let changes = entry["changes"].as_array().unwrap_or(&empty_vec);

            for change in changes {
                let value = &change["value"];

                // Check for messages
                if let Some(messages) = value["messages"].as_array() {
                    for message in messages {
                        self.process_message(message).await?;
                    }
                }

                // Check for status updates (optional logging)
                if let Some(statuses) = value["statuses"].as_array() {
                    for status in statuses {
                        let msg_id = status["id"].as_str().unwrap_or("");
                        let status_val = status["status"].as_str().unwrap_or("");
                        tracing::debug!("Message {} status: {}", msg_id, status_val);
                    }
                }
            }
        }

        Ok(())
    }

    /// Process a single incoming message.
    async fn process_message(&self, message: &serde_json::Value) -> anyhow::Result<()> {
        let from = message["from"].as_str().unwrap_or("");
        let msg_id = message["id"].as_str().unwrap_or("");
        let msg_type = message["type"].as_str().unwrap_or("text");
        let timestamp = message["timestamp"].as_str().unwrap_or("");

        // Check if sender is allowed
        if !self.config.allowed_numbers.is_empty()
            && !self.config.allowed_numbers.contains(&from.to_string())
        {
            tracing::warn!("Message from unauthorized number: {}", from);
            return Ok(());
        }

        // Extract message text
        let text = match msg_type {
            "text" => message["text"]["body"].as_str().unwrap_or("").to_string(),
            "image" | "video" | "document" | "audio" => {
                let caption = message[msg_type]["caption"].as_str().unwrap_or("");
                let media_id = message[msg_type]["id"].as_str().unwrap_or("");
                format!("[{} received: {}] {}", msg_type, media_id, caption)
            }
            "location" => {
                let lat = message["location"]["latitude"].as_f64().unwrap_or(0.0);
                let lon = message["location"]["longitude"].as_f64().unwrap_or(0.0);
                format!("[Location: {}, {}]", lat, lon)
            }
            "contacts" => "[Contacts shared]".to_string(),
            "sticker" => "[Sticker]".to_string(),
            _ => format!("[Unsupported message type: {}]", msg_type),
        };

        if text.is_empty() {
            return Ok(());
        }

        tracing::info!(
            "WhatsApp message from {}: {} ({})",
            from,
            text.chars().take(50).collect::<String>(),
            timestamp
        );

        // Mark message as read
        let _ = self.mark_read(msg_id).await;

        // Build session key
        let session_key = format!("whatsapp:{}", from);

        // Process with agent
        match self
            .runner
            .process_message(&session_key, &text, Some(from), Some("whatsapp"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty()
                    && trimmed != "NO_REPLY"
                    && trimmed != "HEARTBEAT_OK"
                {
                    // Split long messages (WhatsApp limit: 4096 chars)
                    for chunk in split_message(trimmed, 4096) {
                        self.send_message(from, chunk).await?;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Agent error for WhatsApp message: {}", e);
                self.send_message(from, &format!("⚠️ Error: {}", e)).await?;
            }
        }

        Ok(())
    }
}

/// Query parameters for webhook verification.
#[derive(Deserialize)]
struct VerifyParams {
    #[serde(rename = "hub.mode")]
    mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

/// Webhook verification handler (GET /webhook).
async fn verify_webhook(
    State(state): State<Arc<AppState>>,
    Query(params): Query<VerifyParams>,
) -> impl IntoResponse {
    let mode = params.mode.as_deref().unwrap_or("");
    let token = params.token.as_deref().unwrap_or("");
    let challenge = params.challenge.as_deref().unwrap_or("");

    if mode == "subscribe" && token == state.channel.config.verify_token {
        tracing::info!("WhatsApp webhook verified successfully");
        (StatusCode::OK, challenge.to_string())
    } else {
        tracing::warn!("WhatsApp webhook verification failed");
        (StatusCode::FORBIDDEN, "Verification failed".to_string())
    }
}

/// Webhook handler (POST /webhook).
async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Always respond with 200 OK quickly to avoid timeouts
    tokio::spawn({
        let channel = state.channel.clone();
        async move {
            if let Err(e) = channel.handle_webhook(payload).await {
                tracing::error!("Error handling WhatsApp webhook: {}", e);
            }
        }
    });

    StatusCode::OK
}

/// Split a message into chunks respecting WhatsApp's character limit.
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let mut end = std::cmp::min(start + max_len, text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        
        // Try to split at a newline or space
        let split_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .or_else(|| text[start..end].rfind(' '))
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        if split_at <= start {
            start = text.ceil_char_boundary(start + 1);
            continue;
        }
        chunks.push(&text[start..split_at]);
        start = split_at;
    }

    chunks
}

/// Start the WhatsApp channel (convenience function).
pub async fn start(config: WhatsAppConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let channel = Arc::new(WhatsAppChannel::new(config, runner));
    channel.start().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_message() {
        let short = "Hello, World!";
        assert_eq!(split_message(short, 4096), vec!["Hello, World!"]);

        let long = "a".repeat(5000);
        let chunks = split_message(&long, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 4096);
    }

    #[test]
    fn test_split_message_at_boundary() {
        let text = "Line 1\nLine 2\nLine 3";
        let chunks = split_message(text, 10);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "Line 1\n");
    }
}
