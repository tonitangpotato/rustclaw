//! Agent event types for streaming intermediate output to consumers.
//!
//! Replaces the old `process_message -> String` pattern with an event channel
//! that delivers text, tool progress, and final response as they happen.

use tokio::sync::mpsc;

/// Sub-agent lifecycle events — broadcast to all listeners (e.g., telegram.rs).
/// These events trigger proactive agent turns when sub-agents complete.
#[derive(Debug, Clone)]
pub enum SubAgentEvent {
    /// A fire-and-forget sub-agent completed successfully.
    Completed {
        task_id: String,
        parent_session_key: String,
        task_summary: String,
        result_preview: String,
        duration_secs: f64,
    },
    /// A fire-and-forget sub-agent failed.
    Failed {
        task_id: String,
        parent_session_key: String,
        task_summary: String,
        error: String,
        duration_secs: f64,
    },
}

/// Events emitted by the agent during message processing.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Intermediate text — the LLM's text response before tool execution.
    /// Consumer should send this to the user immediately as an acknowledgment.
    Text(String),

    /// A tool execution is starting.
    ToolStart {
        name: String,
        /// Tool call ID for correlation.
        id: String,
    },

    /// A tool execution completed (for verbose/progress display).
    ToolDone {
        name: String,
        id: String,
        /// First N chars of output for preview.
        preview: String,
        is_error: bool,
    },

    /// Final response text — the last LLM response with no more tool calls.
    Response(String),

    /// An error occurred during processing.
    Error(String),
}

impl AgentEvent {
    /// Returns true if this is the terminal event (Response or Error).
    pub fn is_terminal(&self) -> bool {
        matches!(self, AgentEvent::Response(_) | AgentEvent::Error(_))
    }

    /// Extract the final response text, if this is a Response event.
    pub fn as_response(&self) -> Option<&str> {
        match self {
            AgentEvent::Response(text) => Some(text),
            _ => None,
        }
    }
}

/// Create an event channel with a reasonable buffer.
pub fn event_channel() -> (mpsc::Sender<AgentEvent>, mpsc::Receiver<AgentEvent>) {
    mpsc::channel(32)
}

/// Helper: collect all events from a receiver and return the final response.
/// Useful for callers that don't need streaming (heartbeat, simple process_message).
pub async fn collect_response(mut rx: mpsc::Receiver<AgentEvent>) -> anyhow::Result<String> {
    let mut last_response = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Response(text) => {
                last_response = text;
            }
            AgentEvent::Error(e) => {
                return Err(anyhow::anyhow!("{}", e));
            }
            // Ignore intermediate events
            _ => {}
        }
    }
    Ok(last_response)
}
