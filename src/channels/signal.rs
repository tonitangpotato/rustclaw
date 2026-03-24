//! Signal channel adapter using signal-cli.
//!
//! Uses signal-cli as a subprocess in JSON-RPC mode for sending and receiving messages.
//! This is the same approach OpenClaw uses.

use std::process::Stdio;
use std::sync::Arc;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::agent::AgentRunner;
use crate::config::SignalConfig;

/// Signal bot using signal-cli subprocess.
struct SignalBot {
    config: SignalConfig,
    runner: Arc<AgentRunner>,
    /// signal-cli subprocess handle
    process: Mutex<Option<Child>>,
    /// stdin writer for JSON-RPC commands
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    /// Request ID counter
    request_id: std::sync::atomic::AtomicU64,
}

impl SignalBot {
    fn new(config: SignalConfig, runner: Arc<AgentRunner>) -> Self {
        Self {
            config,
            runner,
            process: Mutex::new(None),
            stdin: Mutex::new(None),
            request_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Start the signal-cli subprocess in JSON-RPC mode.
    async fn start_subprocess(&self) -> anyhow::Result<tokio::io::Lines<BufReader<tokio::process::ChildStdout>>> {
        let mut cmd = Command::new(&self.config.signal_cli_path);
        cmd.args([
            "-a", &self.config.phone_number,
            "jsonRpc",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        tracing::info!(
            "Starting signal-cli: {} -a {} jsonRpc",
            self.config.signal_cli_path,
            self.config.phone_number
        );

        let mut child = cmd.spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("Failed to get stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to get stdout"))?;

        *self.stdin.lock().await = Some(stdin);
        *self.process.lock().await = Some(child);

        let reader = BufReader::new(stdout);
        Ok(reader.lines())
    }

    /// Check if a sender is allowed to message.
    fn is_allowed(&self, sender: &str) -> bool {
        self.config.allowed_numbers.is_empty()
            || self.config.allowed_numbers.iter().any(|n| n == sender)
    }

    /// Send a JSON-RPC request to signal-cli.
    async fn send_rpc(&self, method: &str, params: serde_json::Value) -> anyhow::Result<u64> {
        let id = self.request_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut stdin = self.stdin.lock().await;
        if let Some(ref mut stdin) = *stdin {
            let msg = format!("{}\n", serde_json::to_string(&request)?);
            stdin.write_all(msg.as_bytes()).await?;
            stdin.flush().await?;
            tracing::debug!("Sent RPC: {} (id={})", method, id);
        } else {
            anyhow::bail!("signal-cli stdin not available");
        }

        Ok(id)
    }

    /// Send a text message.
    async fn send_message(&self, recipient: &str, text: &str) -> anyhow::Result<()> {
        // Split long messages (Signal has a limit around 2000 chars)
        let chunks = split_message(text, 2000);

        for chunk in chunks {
            self.send_rpc("send", serde_json::json!({
                "recipient": [recipient],
                "message": chunk,
            })).await?;
        }

        Ok(())
    }

    /// Send a voice message (as attachment).
    async fn send_voice(&self, recipient: &str, ogg_path: &str) -> anyhow::Result<()> {
        self.send_rpc("send", serde_json::json!({
            "recipient": [recipient],
            "attachment": [ogg_path],
        })).await?;

        Ok(())
    }

    /// Send a response, handling file attachments.
    async fn send_response(&self, recipient: &str, response: &str) -> anyhow::Result<()> {
        // Check for FILE: patterns
        let file_re = regex::Regex::new(r"FILE:(/[^\s]+)").unwrap();
        let mut text_without_files = response.to_string();
        let mut files_to_send: Vec<String> = Vec::new();

        for cap in file_re.captures_iter(response) {
            let file_path = cap[1].to_string();
            files_to_send.push(file_path.clone());
            text_without_files = text_without_files.replace(&format!("FILE:{}", file_path), "");
        }

        let clean_text = text_without_files.trim();

        // Send text message
        if !clean_text.is_empty() {
            self.send_message(recipient, clean_text).await?;
        }

        // Send files as attachments
        for file_path in files_to_send {
            if std::path::Path::new(&file_path).exists() {
                self.send_rpc("send", serde_json::json!({
                    "recipient": [recipient],
                    "attachment": [file_path],
                })).await?;
            } else {
                self.send_message(recipient, &format!("⚠️ File not found: {}", file_path)).await?;
            }
        }

        Ok(())
    }

    /// Handle an incoming JSON-RPC message from signal-cli.
    async fn handle_rpc_message(&self, msg: &SignalRpcMessage) -> anyhow::Result<()> {
        // Check for incoming message envelope
        if let Some(envelope) = &msg.params {
            if let Some(data_message) = envelope.get("dataMessage") {
                self.handle_data_message(envelope, data_message).await?;
            }
        }

        Ok(())
    }

    /// Handle a data message (text, attachment, etc.).
    async fn handle_data_message(
        &self,
        envelope: &serde_json::Value,
        data_message: &serde_json::Value,
    ) -> anyhow::Result<()> {
        // Get sender
        let sender = envelope["sourceNumber"].as_str()
            .or_else(|| envelope["source"].as_str())
            .unwrap_or("");

        if sender.is_empty() {
            return Ok(());
        }

        // Check access
        if !self.is_allowed(sender) {
            tracing::warn!("Unauthorized Signal user: {}", sender);
            return Ok(());
        }

        // Get message text
        let text = data_message["message"].as_str().unwrap_or("");
        
        if text.is_empty() {
            // Check for voice note or other attachment
            if let Some(attachments) = data_message.get("attachments").and_then(|a| a.as_array()) {
                for attachment in attachments {
                    let content_type = attachment["contentType"].as_str().unwrap_or("");
                    if content_type.starts_with("audio/") {
                        // Voice message - could transcribe here if STT is available
                        tracing::info!("Received voice message from {}", sender);
                        self.send_message(sender, "🎤 Voice messages not yet supported. Please send text.").await?;
                        return Ok(());
                    }
                }
            }
            return Ok(()); // No text content
        }

        // Build session key
        let session_key = format!("signal:{}", sender);
        let user_id = sender.to_string();

        tracing::info!(
            "Signal message from {}: {}",
            sender,
            text.chars().take(50).collect::<String>()
        );

        // Process with agent
        match self
            .runner
            .process_message(&session_key, text, Some(&user_id), Some("signal"))
            .await
        {
            Ok(response) => {
                let trimmed = response.trim();
                if !trimmed.is_empty() && trimmed != "NO_REPLY" && trimmed != "HEARTBEAT_OK" {
                    self.send_response(sender, trimmed).await?;
                }
            }
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                self.send_message(sender, &format!("⚠️ Error: {}", e)).await?;
            }
        }

        Ok(())
    }

    /// Run the signal-cli event loop.
    async fn run(&self) -> anyhow::Result<()> {
        loop {
            tracing::info!("Starting signal-cli subprocess...");

            match self.start_subprocess().await {
                Ok(mut lines) => {
                    tracing::info!("signal-cli started successfully");

                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.is_empty() {
                            continue;
                        }

                        // Parse JSON-RPC message
                        match serde_json::from_str::<SignalRpcMessage>(&line) {
                            Ok(msg) => {
                                // Check if this is an incoming message notification
                                if msg.method.as_deref() == Some("receive") {
                                    if let Err(e) = self.handle_rpc_message(&msg).await {
                                        tracing::error!("Error handling message: {}", e);
                                    }
                                } else if let Some(error) = &msg.error {
                                    tracing::error!("signal-cli error: {:?}", error);
                                }
                            }
                            Err(e) => {
                                tracing::debug!("Failed to parse signal-cli output: {} (line: {})", e, { let _end = line.len().min(100); let _end = line.floor_char_boundary(_end); &line[.._end] });
                            }
                        }
                    }

                    tracing::warn!("signal-cli stdout closed");
                }
                Err(e) => {
                    tracing::error!("Failed to start signal-cli: {}", e);
                }
            }

            // Clean up
            *self.stdin.lock().await = None;
            if let Some(mut child) = self.process.lock().await.take() {
                let _ = child.kill().await;
            }

            // Reconnect delay
            tracing::info!("Restarting signal-cli in 5 seconds...");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }
}

/// Split a message into chunks respecting Signal's character limit.
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = std::cmp::min(start + max_len, text.len());
        let split_at = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .map(|pos| start + pos + 1)
                .unwrap_or(end)
        } else {
            end
        };
        chunks.push(&text[start..split_at]);
        start = split_at;
    }

    chunks
}

// --- signal-cli JSON-RPC types ---

#[derive(Debug, Deserialize)]
struct SignalRpcMessage {
    jsonrpc: Option<String>,
    id: Option<u64>,
    method: Option<String>,
    params: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

/// Start the Signal channel.
pub async fn start(config: SignalConfig, runner: Arc<AgentRunner>) -> anyhow::Result<()> {
    let bot = SignalBot::new(config, runner);
    bot.run().await
}
