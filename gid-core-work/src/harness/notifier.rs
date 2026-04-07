//! Telegram notifier for harness execution events.
//!
//! Provides optional Telegram notifications for:
//! - Task completion/failure (escalation tier 3)
//! - Approval gate requests (with inline keyboard buttons)
//! - Execution milestones (start, complete, cancelled)
//!
//! Uses reqwest to call the Telegram Bot API directly — no dependency on agentctl.
//!
//! # Configuration
//!
//! Set via `HarnessConfig::notifier` or environment variables:
//! - `GID_TELEGRAM_BOT_TOKEN` — Bot token from @BotFather
//! - `GID_TELEGRAM_CHAT_ID` — Chat/user ID for notifications
//!
//! # Example
//!
//! ```ignore
//! let notifier = TelegramNotifier::from_env()?;
//! notifier.send_task_done("auth", 12, 5000).await?;
//! notifier.send_approval_request("graph", "12 tasks in 4 layers. Confirm?").await?;
//! ```

use std::time::Duration;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Telegram Bot API base URL.
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// Default timeout for Telegram API requests.
const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Notifier configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotifierConfig {
    /// Telegram bot token (from @BotFather).
    pub telegram_bot_token: Option<String>,
    /// Telegram chat ID for notifications.
    pub telegram_chat_id: Option<String>,
    /// Whether to send notifications (default: true if configured).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Notify on task completion.
    #[serde(default = "default_true")]
    pub notify_task_done: bool,
    /// Notify on task failure.
    #[serde(default = "default_true")]
    pub notify_task_failed: bool,
    /// Notify on approval requests.
    #[serde(default = "default_true")]
    pub notify_approval: bool,
    /// Notify on execution start/complete.
    #[serde(default = "default_true")]
    pub notify_milestones: bool,
}

fn default_enabled() -> bool { true }
fn default_true() -> bool { true }

impl NotifierConfig {
    /// Load from environment variables.
    pub fn from_env() -> Self {
        Self {
            telegram_bot_token: std::env::var("GID_TELEGRAM_BOT_TOKEN").ok(),
            telegram_chat_id: std::env::var("GID_TELEGRAM_CHAT_ID").ok(),
            enabled: true,
            notify_task_done: true,
            notify_task_failed: true,
            notify_approval: true,
            notify_milestones: true,
        }
    }

    /// Check if the notifier is properly configured.
    pub fn is_configured(&self) -> bool {
        self.enabled
            && self.telegram_bot_token.is_some()
            && self.telegram_chat_id.is_some()
    }
}

/// Telegram notifier for harness events.
///
/// Sends notifications via the Telegram Bot API when important events
/// occur during harness execution. Supports inline keyboards for
/// approval workflows.
#[derive(Clone)]
pub struct TelegramNotifier {
    /// HTTP client for API requests.
    client: reqwest::Client,
    /// Bot token.
    bot_token: String,
    /// Chat ID for notifications.
    chat_id: String,
    /// Configuration flags.
    config: NotifierConfig,
}

impl TelegramNotifier {
    /// Create a new notifier from configuration.
    ///
    /// Returns `None` if the configuration is incomplete.
    pub fn new(config: NotifierConfig) -> Option<Self> {
        if !config.is_configured() {
            return None;
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .build()
            .ok()?;

        Some(Self {
            client,
            bot_token: config.telegram_bot_token.clone()?,
            chat_id: config.telegram_chat_id.clone()?,
            config,
        })
    }

    /// Create a notifier from environment variables.
    pub fn from_env() -> Option<Self> {
        Self::new(NotifierConfig::from_env())
    }

    /// Build the API URL for a method.
    fn api_url(&self, method: &str) -> String {
        format!("{}{}/{}", TELEGRAM_API_BASE, self.bot_token, method)
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Basic Message Sending
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Send a plain text message.
    pub async fn send_text(&self, text: &str) -> Result<MessageResponse> {
        let body = serde_json::json!({
            "chat_id": &self.chat_id,
            "text": text,
        });

        self.send_request("sendMessage", &body).await
    }

    /// Send an HTML-formatted message.
    pub async fn send_html(&self, html: &str) -> Result<MessageResponse> {
        let body = serde_json::json!({
            "chat_id": &self.chat_id,
            "text": html,
            "parse_mode": "HTML",
        });

        self.send_request("sendMessage", &body).await
    }

    /// Send a message with an inline keyboard.
    pub async fn send_with_keyboard(
        &self,
        html: &str,
        keyboard: &InlineKeyboard,
    ) -> Result<MessageResponse> {
        let body = serde_json::json!({
            "chat_id": &self.chat_id,
            "text": html,
            "parse_mode": "HTML",
            "reply_markup": keyboard,
        });

        self.send_request("sendMessage", &body).await
    }

    /// Edit an existing message.
    pub async fn edit_message(&self, message_id: i64, html: &str) -> Result<()> {
        let body = serde_json::json!({
            "chat_id": &self.chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
        });

        let _: serde_json::Value = self.send_request("editMessageText", &body).await?;
        Ok(())
    }

    /// Answer a callback query (button press acknowledgment).
    pub async fn answer_callback(&self, callback_id: &str, text: &str) -> Result<()> {
        let body = serde_json::json!({
            "callback_query_id": callback_id,
            "text": text,
        });

        let _: serde_json::Value = self.send_request("answerCallbackQuery", &body).await?;
        Ok(())
    }

    /// Send an API request and parse the response.
    async fn send_request<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let url = self.api_url(method);

        debug!(method, "Sending Telegram API request");

        let response = self.client
            .post(&url)
            .json(body)
            .send()
            .await
            .context("Failed to connect to Telegram API")?;

        let status = response.status();
        let text = response.text().await.context("Failed to read response")?;

        if !status.is_success() {
            anyhow::bail!("Telegram API error ({}): {}", status, text);
        }

        let api_response: TelegramResponse<T> = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse Telegram response: {}", text))?;

        if !api_response.ok {
            let desc = api_response.description.unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Telegram API error: {}", desc);
        }

        api_response.result.context("Missing result in Telegram response")
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Harness Event Notifications
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Notify that execution has started.
    pub async fn send_execution_start(&self, total_tasks: usize, layers: usize) -> Result<()> {
        if !self.config.notify_milestones {
            return Ok(());
        }

        let msg = format!(
            "🚀 <b>GID Harness Started</b>\n\n\
             Tasks: {}\n\
             Layers: {}",
            total_tasks, layers
        );

        self.send_html(&msg).await?;
        Ok(())
    }

    /// Notify that a task completed successfully.
    pub async fn send_task_done(
        &self,
        task_id: &str,
        turns: u32,
        tokens: u64,
    ) -> Result<()> {
        if !self.config.notify_task_done {
            return Ok(());
        }

        let msg = format!(
            "✅ <b>Task Complete:</b> <code>{}</code>\n\
             Turns: {} | Tokens: {}",
            escape_html(task_id),
            turns,
            format_tokens(tokens)
        );

        self.send_html(&msg).await?;
        Ok(())
    }

    /// Notify that a task failed (escalation tier 3).
    pub async fn send_task_failed(
        &self,
        task_id: &str,
        reason: &str,
        turns: u32,
    ) -> Result<()> {
        if !self.config.notify_task_failed {
            return Ok(());
        }

        let truncated_reason = if reason.len() > 500 {
            format!("{}...", &reason[..500])
        } else {
            reason.to_string()
        };

        let msg = format!(
            "❌ <b>Task Failed:</b> <code>{}</code>\n\n\
             <b>Reason:</b>\n<pre>{}</pre>\n\n\
             Turns used: {}",
            escape_html(task_id),
            escape_html(&truncated_reason),
            turns
        );

        self.send_html(&msg).await?;
        Ok(())
    }

    /// Notify that an approval is needed (with inline keyboard).
    ///
    /// Returns the message ID so it can be edited after approval.
    pub async fn send_approval_request(
        &self,
        phase: &str,
        message: &str,
        callback_prefix: &str,
    ) -> Result<i64> {
        if !self.config.notify_approval {
            return Ok(0);
        }

        let html = format!(
            "⏸ <b>Approval Required: {}</b>\n\n{}",
            escape_html(phase),
            escape_html(message)
        );

        let keyboard = InlineKeyboard::new(vec![
            vec![
                InlineButton::callback("✅ Approve", &format!("{}:approve", callback_prefix)),
                InlineButton::callback("⏭ Skip", &format!("{}:skip", callback_prefix)),
            ],
            vec![
                InlineButton::callback("🛑 Cancel", &format!("{}:cancel", callback_prefix)),
            ],
        ]);

        let resp = self.send_with_keyboard(&html, &keyboard).await?;
        Ok(resp.message_id)
    }

    /// Update an approval message after the user responds.
    pub async fn update_approval_status(
        &self,
        message_id: i64,
        phase: &str,
        status: ApprovalStatus,
    ) -> Result<()> {
        let (emoji, action) = match status {
            ApprovalStatus::Approved => ("✅", "Approved"),
            ApprovalStatus::Skipped => ("⏭", "Skipped"),
            ApprovalStatus::Cancelled => ("🛑", "Cancelled"),
        };

        let html = format!(
            "{} <b>{}</b> — {}",
            emoji,
            escape_html(phase),
            action
        );

        self.edit_message(message_id, &html).await
    }

    /// Notify that execution completed.
    pub async fn send_execution_complete(
        &self,
        completed: usize,
        failed: usize,
        total_turns: u32,
        total_tokens: u64,
        duration_secs: u64,
    ) -> Result<()> {
        if !self.config.notify_milestones {
            return Ok(());
        }

        let status_emoji = if failed == 0 { "🎉" } else { "⚠️" };
        let duration_str = if duration_secs > 60 {
            format!("{}m {}s", duration_secs / 60, duration_secs % 60)
        } else {
            format!("{}s", duration_secs)
        };

        let msg = format!(
            "{} <b>GID Harness Complete</b>\n\n\
             ✅ Completed: {}\n\
             ❌ Failed: {}\n\
             🔄 Total turns: {}\n\
             💰 Total tokens: {}\n\
             ⏱ Duration: {}",
            status_emoji,
            completed,
            failed,
            total_turns,
            format_tokens(total_tokens),
            duration_str
        );

        self.send_html(&msg).await?;
        Ok(())
    }

    /// Notify that execution was cancelled.
    pub async fn send_execution_cancelled(
        &self,
        completed: usize,
        remaining: usize,
    ) -> Result<()> {
        if !self.config.notify_milestones {
            return Ok(());
        }

        let msg = format!(
            "🛑 <b>GID Harness Cancelled</b>\n\n\
             Completed: {}\n\
             Remaining: {}",
            completed, remaining
        );

        self.send_html(&msg).await?;
        Ok(())
    }

    /// Notify about a replan event.
    pub async fn send_replan(&self, new_tasks: &[String]) -> Result<()> {
        if !self.config.notify_milestones {
            return Ok(());
        }

        let tasks_str = new_tasks.iter()
            .take(5)
            .map(|t| format!("• <code>{}</code>", escape_html(t)))
            .collect::<Vec<_>>()
            .join("\n");

        let more = if new_tasks.len() > 5 {
            format!("\n... and {} more", new_tasks.len() - 5)
        } else {
            String::new()
        };

        let msg = format!(
            "🔄 <b>Replan: {} new tasks added</b>\n\n{}{}",
            new_tasks.len(),
            tasks_str,
            more
        );

        self.send_html(&msg).await?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Inline Keyboard Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Inline keyboard for Telegram messages.
#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboard {
    inline_keyboard: Vec<Vec<InlineButton>>,
}

impl InlineKeyboard {
    /// Create a new inline keyboard with the given button rows.
    pub fn new(rows: Vec<Vec<InlineButton>>) -> Self {
        Self { inline_keyboard: rows }
    }
}

/// A single inline keyboard button.
#[derive(Debug, Clone, Serialize)]
pub struct InlineButton {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

impl InlineButton {
    /// Create a callback button.
    pub fn callback(text: &str, data: &str) -> Self {
        Self {
            text: text.to_string(),
            callback_data: Some(data.to_string()),
            url: None,
        }
    }

    /// Create a URL button.
    pub fn url(text: &str, url: &str) -> Self {
        Self {
            text: text.to_string(),
            callback_data: None,
            url: Some(url.to_string()),
        }
    }
}

/// Approval status for updating messages.
#[derive(Debug, Clone, Copy)]
pub enum ApprovalStatus {
    Approved,
    Skipped,
    Cancelled,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Response Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Generic Telegram API response wrapper.
#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

/// Response from sendMessage.
#[derive(Debug, Deserialize)]
pub struct MessageResponse {
    pub message_id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Escape HTML special characters for Telegram HTML mode.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Format token count for display.
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>alert('xss')</script>"), "&lt;script&gt;alert('xss')&lt;/script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("hello"), "hello");
    }

    #[test]
    fn test_format_tokens() {
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(5_000), "5.0K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn test_config_from_env() {
        // Clear any existing env vars
        std::env::remove_var("GID_TELEGRAM_BOT_TOKEN");
        std::env::remove_var("GID_TELEGRAM_CHAT_ID");

        let config = NotifierConfig::from_env();
        assert!(!config.is_configured());

        // Set env vars
        std::env::set_var("GID_TELEGRAM_BOT_TOKEN", "test-token");
        std::env::set_var("GID_TELEGRAM_CHAT_ID", "12345");

        let config = NotifierConfig::from_env();
        assert!(config.is_configured());
        assert_eq!(config.telegram_bot_token, Some("test-token".to_string()));
        assert_eq!(config.telegram_chat_id, Some("12345".to_string()));

        // Clean up
        std::env::remove_var("GID_TELEGRAM_BOT_TOKEN");
        std::env::remove_var("GID_TELEGRAM_CHAT_ID");
    }

    #[test]
    fn test_inline_keyboard_serialization() {
        let keyboard = InlineKeyboard::new(vec![
            vec![
                InlineButton::callback("Approve", "approve:1"),
                InlineButton::callback("Skip", "skip:1"),
            ],
        ]);

        let json = serde_json::to_value(&keyboard).unwrap();
        assert!(json["inline_keyboard"].is_array());
        assert_eq!(json["inline_keyboard"][0][0]["text"], "Approve");
        assert_eq!(json["inline_keyboard"][0][0]["callback_data"], "approve:1");
    }

    #[test]
    fn test_notifier_not_created_without_config() {
        let config = NotifierConfig::default();
        assert!(TelegramNotifier::new(config).is_none());
    }
}
