//! Anthropic OAuth token management.
//!
//! Reads tokens from macOS Keychain ("Claude Code-credentials"),
//! auto-refreshes when expired, and writes back updated tokens.
//! This mirrors Claude Code's own OAuth flow.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Anthropic OAuth client ID (same as Claude Code / OpenClaw SDK).
/// Base64 of "9d1c250a-e61b-44d9-88ed-5944d1962f5e".
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Anthropic OAuth token endpoint.
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";

/// Keychain service name used by Claude Code.
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Keychain account names used by Claude Code.
/// Claude Code may store credentials under different account names
/// depending on how auth was set up. We try the user's account first,
/// then fall back to the default "Claude Code" account.
const KEYCHAIN_ACCOUNTS: &[&str] = &["potato", "Claude Code"];

/// Refresh 5 minutes before actual expiry (same buffer as Claude Code SDK).
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;

/// Credentials stored in macOS Keychain by Claude Code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeychainCredentials {
    claude_ai_oauth: OAuthCredentials,
}

/// The OAuth credential block inside Keychain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: i64, // milliseconds since epoch
    #[serde(default)]
    scopes: Option<Vec<String>>,
    #[serde(default)]
    subscription_type: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
}

/// Anthropic OAuth token refresh response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64, // seconds
}

/// Thread-safe OAuth token manager.
///
/// Holds the current token in memory and refreshes it automatically
/// when expired or near-expiry.
#[derive(Clone)]
pub struct OAuthTokenManager {
    inner: Arc<RwLock<TokenState>>,
    http: reqwest::Client,
}

struct TokenState {
    access_token: String,
    refresh_token: String,
    expires_at_ms: i64,
    keychain_account: String,
}

impl OAuthTokenManager {
    /// Create a new token manager by reading from macOS Keychain.
    pub fn from_keychain() -> Result<Self> {
        let (creds, account) = read_keychain_credentials()
            .context("Failed to read OAuth credentials from macOS Keychain")?;

        tracing::info!(
            "Loaded OAuth token from Keychain (account='{}', expires_at={}, subscription={:?})",
            account,
            creds.claude_ai_oauth.expires_at,
            creds.claude_ai_oauth.subscription_type,
        );

        let state = TokenState {
            access_token: creds.claude_ai_oauth.access_token,
            refresh_token: creds.claude_ai_oauth.refresh_token,
            expires_at_ms: creds.claude_ai_oauth.expires_at,
            keychain_account: account.to_string(),
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(state)),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// Create from explicit tokens (for testing or fallback).
    pub fn from_tokens(access: String, refresh: String, expires_at_ms: i64) -> Result<Self> {
        let state = TokenState {
            access_token: access,
            refresh_token: refresh,
            expires_at_ms,
            keychain_account: KEYCHAIN_ACCOUNTS[0].to_string(),
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(state)),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// Get a valid access token, refreshing if expired.
    ///
    /// This is the main entry point — call before every API request.
    pub async fn get_token(&self) -> Result<String> {
        // Fast path: check if current token is still valid
        {
            let state = self.inner.read().await;
            if !Self::is_expired(&state) {
                return Ok(state.access_token.clone());
            }
            tracing::info!("OAuth token expired or near-expiry, refreshing...");
        }

        // Slow path: refresh the token
        self.refresh().await
    }

    /// Force-refresh the token (e.g., after a 401 response).
    pub async fn refresh(&self) -> Result<String> {
        let mut state = self.inner.write().await;

        // Double-check: another task might have refreshed while we waited for the lock
        if !Self::is_expired(&state) {
            return Ok(state.access_token.clone());
        }

        tracing::info!("Refreshing OAuth token via {}", TOKEN_URL);

        let resp = self
            .http
            .post(TOKEN_URL)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": CLIENT_ID,
                "refresh_token": state.refresh_token,
            }))
            .send()
            .await
            .context("Failed to reach Anthropic OAuth endpoint")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "OAuth token refresh failed (HTTP {}): {}",
                status,
                body
            );
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .context("Failed to parse OAuth token response")?;

        let now_ms = chrono::Utc::now().timestamp_millis();
        let new_expires_at = now_ms + token_resp.expires_in * 1000 - REFRESH_BUFFER_MS;

        tracing::info!(
            "OAuth token refreshed! Expires in {}s (at {})",
            token_resp.expires_in,
            new_expires_at,
        );

        // Update in-memory state
        state.access_token = token_resp.access_token.clone();
        state.refresh_token = token_resp.refresh_token.clone();
        state.expires_at_ms = new_expires_at;

        // Write back to Keychain (best-effort — don't fail the request if this fails)
        let new_access = state.access_token.clone();
        let new_refresh = state.refresh_token.clone();
        let new_expires = new_expires_at;
        let account = state.keychain_account.clone();
        drop(state); // Release write lock before Keychain I/O

        if let Err(e) = write_keychain_credentials(&account, &new_access, &new_refresh, new_expires) {
            tracing::warn!("Failed to update Keychain (non-fatal): {}", e);
        } else {
            tracing::info!("Updated Keychain with refreshed token");
        }

        Ok(new_access)
    }

    fn is_expired(state: &TokenState) -> bool {
        let now_ms = chrono::Utc::now().timestamp_millis();
        now_ms >= state.expires_at_ms
    }
}

// ─── macOS Keychain operations ──────────────────────────────────

/// Read OAuth credentials from macOS Keychain using the `security` CLI.
///
/// We use `security find-generic-password -w` instead of the `security-framework`
/// crate because programmatic Keychain access from ad-hoc signed binaries triggers
/// macOS authorization dialogs. The `security` CLI is pre-authorized and doesn't block.
fn read_keychain_credentials() -> Result<(KeychainCredentials, String)> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut best: Option<(KeychainCredentials, String, i64)> = None;

    for account in KEYCHAIN_ACCOUNTS {
        let output = std::process::Command::new("security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account, "-w"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let json_str = String::from_utf8_lossy(&out.stdout);
                let json_str = json_str.trim();
                if let Ok(creds) = serde_json::from_str::<KeychainCredentials>(json_str) {
                    let expires = creds.claude_ai_oauth.expires_at;
                    tracing::debug!(
                        "Found Keychain entry for account '{}': expires_at={}, expired={}",
                        account, expires, now_ms >= expires
                    );

                    // Prefer non-expired tokens; among expired, prefer the most recent
                    let dominated = best.as_ref().map_or(false, |(_, _, best_exp)| {
                        (*best_exp > now_ms) && (expires <= now_ms)
                    });

                    if !dominated {
                        let dominated_best = best.as_ref().map_or(true, |(_, _, best_exp)| {
                            expires > *best_exp
                        });
                        if dominated_best {
                            best = Some((creds, account.to_string(), expires));
                        }
                    }
                }
            }
            Ok(out) => {
                tracing::debug!(
                    "Keychain account '{}' not found: {}",
                    account,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Err(e) => {
                tracing::debug!("Failed to run security CLI for account '{}': {}", account, e);
            }
        }
    }

    match best {
        Some((creds, account, _)) => {
            tracing::info!("Using Keychain account '{}'", account);
            Ok((creds, account))
        }
        None => anyhow::bail!(
            "No OAuth credentials found in macOS Keychain (service='{}'). Run `claude` CLI first to set up OAuth.",
            KEYCHAIN_SERVICE
        ),
    }
}

/// Write updated OAuth credentials back to macOS Keychain using `security` CLI.
fn write_keychain_credentials(
    account: &str,
    access_token: &str,
    refresh_token: &str,
    expires_at_ms: i64,
) -> Result<()> {
    // Read existing credentials to preserve non-OAuth fields
    let mut creds = {
        let output = std::process::Command::new("security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account, "-w"])
            .output();
        output.ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout);
                serde_json::from_str::<KeychainCredentials>(s.trim()).ok()
            })
            .unwrap_or_else(|| KeychainCredentials {
                claude_ai_oauth: OAuthCredentials {
                    access_token: String::new(),
                    refresh_token: String::new(),
                    expires_at: 0,
                    scopes: Some(vec![
                        "user:inference".into(),
                        "user:mcp_servers".into(),
                        "user:profile".into(),
                        "user:sessions:claude_code".into(),
                    ]),
                    subscription_type: Some("max".into()),
                    rate_limit_tier: Some("default_claude_max_20x".into()),
                },
            })
    };

    creds.claude_ai_oauth.access_token = access_token.to_string();
    creds.claude_ai_oauth.refresh_token = refresh_token.to_string();
    creds.claude_ai_oauth.expires_at = expires_at_ms;

    let json = serde_json::to_string(&creds)
        .context("Failed to serialize credentials")?;

    // Delete old entry (ignore errors — might not exist)
    let _ = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account])
        .output();

    // Add new entry
    let output = std::process::Command::new("security")
        .args(["add-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account, "-w", &json])
        .output()
        .context("Failed to run security CLI for Keychain write")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Keychain write failed: {}", stderr.trim());
    }

    Ok(())
}
