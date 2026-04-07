//! Ritual Notifier — Telegram notifications for ritual phase transitions.
//!
//! Wraps `TelegramNotifier` from harness to provide ritual-specific notifications
//! for start, phase completion, approval required, failure, and completion events.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::harness::notifier::{
    escape_html, InlineButton, InlineKeyboard, NotifierConfig, TelegramNotifier,
};
use super::definition::PhaseDefinition;
use super::executor::PhaseResult;

/// Configuration for ritual notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RitualNotifyConfig {
    /// Events to notify on. If empty, all events are notified.
    #[serde(default)]
    pub events: Vec<RitualEvent>,
    /// Whether notifications are enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for RitualNotifyConfig {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            enabled: true,
        }
    }
}

impl RitualNotifyConfig {
    /// Check if a specific event should be notified.
    pub fn should_notify(&self, event: &RitualEvent) -> bool {
        if !self.enabled {
            return false;
        }
        // If events list is empty, notify all events (default behavior)
        if self.events.is_empty() {
            return true;
        }
        self.events.contains(event)
    }
}

/// Types of ritual events that can trigger notifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RitualEvent {
    /// Ritual has started.
    RitualStart,
    /// A phase completed successfully.
    PhaseComplete,
    /// Approval is required for a phase.
    ApprovalRequired,
    /// A phase failed.
    PhaseFailed,
    /// Ritual completed successfully.
    RitualComplete,
    /// Ritual failed.
    RitualFailed,
}

/// Notifier for ritual events.
///
/// Wraps `TelegramNotifier` and provides ritual-specific message formatting.
/// All notification methods are fire-and-forget — errors are logged but don't
/// fail the ritual.
pub struct RitualNotifier {
    telegram: TelegramNotifier,
    config: RitualNotifyConfig,
}

impl RitualNotifier {
    /// Create a new RitualNotifier from environment variables.
    ///
    /// Returns `None` if the required environment variables (`GID_TELEGRAM_BOT_TOKEN`,
    /// `GID_TELEGRAM_CHAT_ID`) are not set. This allows graceful degradation when
    /// notifications are not configured.
    pub fn from_env() -> Option<Self> {
        let telegram = TelegramNotifier::from_env()?;
        Some(Self {
            telegram,
            config: RitualNotifyConfig::default(),
        })
    }

    /// Create a new RitualNotifier with explicit config.
    pub fn new(telegram: TelegramNotifier, config: RitualNotifyConfig) -> Self {
        Self { telegram, config }
    }

    /// Create from NotifierConfig and RitualNotifyConfig.
    pub fn from_config(
        notifier_config: NotifierConfig,
        ritual_config: RitualNotifyConfig,
    ) -> Option<Self> {
        let telegram = TelegramNotifier::new(notifier_config)?;
        Some(Self {
            telegram,
            config: ritual_config,
        })
    }

    /// Notify that a ritual has started.
    pub async fn notify_ritual_start(&self, ritual_name: &str, total_phases: usize) -> Result<()> {
        if !self.config.should_notify(&RitualEvent::RitualStart) {
            return Ok(());
        }

        let html = format!(
            "🔄 <b>Ritual Started</b>\n\
             📋 {}\n\
             📊 {} phases",
            escape_html(ritual_name),
            total_phases
        );

        // Fire-and-forget: spawn background task to avoid blocking ritual
        let telegram = self.telegram.clone();
        tokio::spawn(async move {
            match telegram.send_html(&html).await {
                Ok(_) => debug!("Sent ritual start notification"),
                Err(e) => warn!("Failed to send ritual start notification: {}", e),
            }
        });

        Ok(())
    }

    /// Notify that a phase completed successfully.
    pub async fn notify_phase_complete(
        &self,
        phase: &PhaseDefinition,
        result: &PhaseResult,
        phase_idx: usize,
        total: usize,
    ) -> Result<()> {
        if !self.config.should_notify(&RitualEvent::PhaseComplete) {
            return Ok(());
        }

        let artifact_count = result.artifacts.len();
        let html = format!(
            "✅ <b>Phase {}/{}</b>: {}\n\
             ⏱ {}s | 📁 {} artifact{}",
            phase_idx + 1,
            total,
            escape_html(&phase.id),
            result.duration_secs,
            artifact_count,
            if artifact_count == 1 { "" } else { "s" }
        );

        // Fire-and-forget: spawn background task
        let telegram = self.telegram.clone();
        let phase_id = phase.id.clone();
        tokio::spawn(async move {
            match telegram.send_html(&html).await {
                Ok(_) => debug!("Sent phase complete notification for '{}'", phase_id),
                Err(e) => warn!("Failed to send phase complete notification: {}", e),
            }
        });

        Ok(())
    }

    /// Notify that approval is required for a phase.
    ///
    /// Sends an inline keyboard with Approve/Skip/Reject buttons.
    /// Returns the message_id so the message can be edited after the user responds.
    pub async fn notify_approval_required(
        &self,
        phase: &PhaseDefinition,
        artifacts: &[String],
    ) -> Result<i64> {
        if !self.config.should_notify(&RitualEvent::ApprovalRequired) {
            return Ok(0);
        }

        let mut html = format!(
            "⏸ <b>Approval Required</b>\n\
             📋 Phase: {}\n",
            escape_html(&phase.id)
        );

        if !artifacts.is_empty() {
            html.push_str("📁 Artifacts:\n");
            for artifact in artifacts.iter().take(10) {
                html.push_str(&format!("  • {}\n", escape_html(artifact)));
            }
            if artifacts.len() > 10 {
                html.push_str(&format!("  ... and {} more\n", artifacts.len() - 10));
            }
        }

        let keyboard = InlineKeyboard::new(vec![vec![
            InlineButton::callback("Approve ✅", &format!("ritual_approve:{}", phase.id)),
            InlineButton::callback("Skip ⊘", &format!("ritual_skip:{}", phase.id)),
            InlineButton::callback("Reject ✗", &format!("ritual_reject:{}", phase.id)),
        ]]);

        match self.telegram.send_with_keyboard(&html, &keyboard).await {
            Ok(resp) => {
                debug!(
                    "Sent approval required notification for '{}', message_id={}",
                    phase.id, resp.message_id
                );
                Ok(resp.message_id)
            }
            Err(e) => {
                warn!("Failed to send approval required notification: {}", e);
                Ok(0)
            }
        }
    }

    /// Notify that a phase failed.
    pub async fn notify_phase_failed(
        &self,
        phase: &PhaseDefinition,
        error: &str,
    ) -> Result<()> {
        if !self.config.should_notify(&RitualEvent::PhaseFailed) {
            return Ok(());
        }

        // Truncate error message if too long
        let error_display = if error.len() > 500 {
            format!("{}...", &error[..500])
        } else {
            error.to_string()
        };

        let html = format!(
            "❌ <b>Phase Failed</b>\n\
             📋 Phase: {}\n\
             📝 {}\n",
            escape_html(&phase.id),
            escape_html(&error_display)
        );

        // Fire-and-forget: spawn background task
        let telegram = self.telegram.clone();
        let phase_id = phase.id.clone();
        tokio::spawn(async move {
            match telegram.send_html(&html).await {
                Ok(_) => debug!("Sent phase failed notification for '{}'", phase_id),
                Err(e) => warn!("Failed to send phase failed notification: {}", e),
            }
        });

        Ok(())
    }

    /// Notify that the ritual completed successfully.
    pub async fn notify_ritual_complete(
        &self,
        ritual_name: &str,
        total_duration_secs: u64,
    ) -> Result<()> {
        if !self.config.should_notify(&RitualEvent::RitualComplete) {
            return Ok(());
        }

        let duration_str = format_duration(total_duration_secs);

        let html = format!(
            "🎉 <b>Ritual Completed!</b>\n\
             📋 {}\n\
             ⏱ Total: {}",
            escape_html(ritual_name),
            duration_str
        );

        // Fire-and-forget: spawn background task
        let telegram = self.telegram.clone();
        tokio::spawn(async move {
            match telegram.send_html(&html).await {
                Ok(_) => debug!("Sent ritual complete notification"),
                Err(e) => warn!("Failed to send ritual complete notification: {}", e),
            }
        });

        Ok(())
    }

    /// Notify that the ritual failed.
    pub async fn notify_ritual_failed(
        &self,
        ritual_name: &str,
        phase_id: &str,
        error: &str,
    ) -> Result<()> {
        if !self.config.should_notify(&RitualEvent::RitualFailed) {
            return Ok(());
        }

        // Truncate error message if too long
        let error_display = if error.len() > 500 {
            format!("{}...", &error[..500])
        } else {
            error.to_string()
        };

        let html = format!(
            "❌ <b>Ritual Failed</b>\n\
             📋 {}\n\
             💥 Failed at: {}\n\
             📝 {}",
            escape_html(ritual_name),
            escape_html(phase_id),
            escape_html(&error_display)
        );

        // Fire-and-forget: spawn background task
        let telegram = self.telegram.clone();
        tokio::spawn(async move {
            match telegram.send_html(&html).await {
                Ok(_) => debug!("Sent ritual failed notification"),
                Err(e) => warn!("Failed to send ritual failed notification: {}", e),
            }
        });

        Ok(())
    }

    /// Edit a previous message (e.g., to update approval status).
    pub async fn edit_message(&self, message_id: i64, html: &str) -> Result<()> {
        if message_id == 0 {
            return Ok(());
        }

        match self.telegram.edit_message(message_id, html).await {
            Ok(_) => debug!("Edited message {}", message_id),
            Err(e) => warn!("Failed to edit message {}: {}", message_id, e),
        }

        Ok(())
    }

    /// Answer a callback query (acknowledge button press).
    pub async fn answer_callback(&self, callback_id: &str, text: &str) -> Result<()> {
        match self.telegram.answer_callback(callback_id, text).await {
            Ok(_) => debug!("Answered callback {}", callback_id),
            Err(e) => warn!("Failed to answer callback {}: {}", callback_id, e),
        }

        Ok(())
    }
}

/// Format duration in human-readable form.
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ritual_notify_config_default() {
        let config = RitualNotifyConfig::default();
        assert!(config.enabled);
        assert!(config.events.is_empty());
    }

    #[test]
    fn test_should_notify_all_when_empty() {
        let config = RitualNotifyConfig {
            enabled: true,
            events: vec![],
        };

        assert!(config.should_notify(&RitualEvent::RitualStart));
        assert!(config.should_notify(&RitualEvent::PhaseComplete));
        assert!(config.should_notify(&RitualEvent::ApprovalRequired));
        assert!(config.should_notify(&RitualEvent::PhaseFailed));
        assert!(config.should_notify(&RitualEvent::RitualComplete));
        assert!(config.should_notify(&RitualEvent::RitualFailed));
    }

    #[test]
    fn test_should_notify_filtered() {
        let config = RitualNotifyConfig {
            enabled: true,
            events: vec![
                RitualEvent::RitualStart,
                RitualEvent::RitualComplete,
                RitualEvent::RitualFailed,
            ],
        };

        assert!(config.should_notify(&RitualEvent::RitualStart));
        assert!(!config.should_notify(&RitualEvent::PhaseComplete));
        assert!(!config.should_notify(&RitualEvent::ApprovalRequired));
        assert!(config.should_notify(&RitualEvent::RitualComplete));
        assert!(config.should_notify(&RitualEvent::RitualFailed));
    }

    #[test]
    fn test_should_notify_disabled() {
        let config = RitualNotifyConfig {
            enabled: false,
            events: vec![],
        };

        assert!(!config.should_notify(&RitualEvent::RitualStart));
        assert!(!config.should_notify(&RitualEvent::PhaseComplete));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
    }

    #[test]
    fn test_ritual_event_serialization() {
        let event = RitualEvent::ApprovalRequired;
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, "\"approval_required\"");

        let parsed: RitualEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, event);
    }

    #[test]
    fn test_ritual_notify_config_yaml() {
        let yaml = r#"
enabled: true
events:
  - ritual_start
  - ritual_complete
"#;
        let config: RitualNotifyConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.events.len(), 2);
        assert!(config.events.contains(&RitualEvent::RitualStart));
        assert!(config.events.contains(&RitualEvent::RitualComplete));
    }
}
