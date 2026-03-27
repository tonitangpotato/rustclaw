//! Auth profile rotation system for multi-token management.
//!
//! Matches OpenClaw's auth-profiles.json format for compatibility.
//! Provides round-robin rotation with cooldown tracking for rate-limited tokens.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Get current time in milliseconds since epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Auth profile credential types (matching OpenClaw's format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthProfileCredential {
    /// Static API key (x-api-key header).
    ApiKey {
        provider: String,
        key: String,
    },
    /// Static bearer token (often OAuth access token / PAT).
    Token {
        provider: String,
        token: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires: Option<u64>,
    },
    /// OAuth credentials with access and refresh tokens.
    #[serde(rename = "oauth")]
    OAuth {
        provider: String,
        access: String,
        refresh: String,
        expires: u64,
    },
}

impl AuthProfileCredential {
    /// Get the provider name for this credential.
    pub fn provider(&self) -> &str {
        match self {
            AuthProfileCredential::ApiKey { provider, .. } => provider,
            AuthProfileCredential::Token { provider, .. } => provider,
            AuthProfileCredential::OAuth { provider, .. } => provider,
        }
    }

    /// Extract the token/key string for authentication.
    /// Returns None if this is a special "keychain" reference.
    pub fn get_token(&self) -> Option<&str> {
        match self {
            AuthProfileCredential::ApiKey { key, .. } => Some(key),
            AuthProfileCredential::Token { token, .. } => {
                if token == "keychain" {
                    None
                } else {
                    Some(token)
                }
            }
            AuthProfileCredential::OAuth { access, .. } => {
                if access == "keychain" {
                    None
                } else {
                    Some(access)
                }
            }
        }
    }

    /// Check if this credential uses the macOS Keychain.
    pub fn is_keychain(&self) -> bool {
        match self {
            AuthProfileCredential::OAuth { access, .. } => access == "keychain",
            AuthProfileCredential::Token { token, .. } => token == "keychain",
            _ => false,
        }
    }
}

/// Failure reason categories for cooldown tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProfileFailureReason {
    Auth,
    AuthPermanent,
    Format,
    Overloaded,
    RateLimit,
    Billing,
    Timeout,
    ModelNotFound,
    SessionExpired,
    Unknown,
}

/// Per-profile usage statistics for round-robin and cooldown tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUsageStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_counts: Option<HashMap<String, u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_at: Option<u64>,
}

/// The auth profile store (matches OpenClaw's auth-profiles.json format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProfileStore {
    pub version: u32,
    pub profiles: HashMap<String, AuthProfileCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_good: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_stats: Option<HashMap<String, ProfileUsageStats>>,
}

impl Default for AuthProfileStore {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: HashMap::new(),
            order: None,
            last_good: None,
            usage_stats: None,
        }
    }
}

impl AuthProfileStore {
    /// Load auth profiles from a JSON file.
    /// Returns an empty store if the file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            tracing::debug!("Auth profiles file not found, using empty store: {}", path.display());
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let store: AuthProfileStore = serde_json::from_str(&content)?;
        tracing::info!(
            "Loaded {} auth profile(s) from {}",
            store.profiles.len(),
            path.display()
        );
        Ok(store)
    }

    /// Save auth profiles to a JSON file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        tracing::debug!("Saved auth profiles to {}", path.display());
        Ok(())
    }

    /// List all profile IDs for a given provider.
    pub fn list_profiles_for_provider(&self, provider: &str) -> Vec<String> {
        let provider_lower = provider.to_lowercase();
        self.profiles
            .iter()
            .filter(|(_, cred)| cred.provider().to_lowercase() == provider_lower)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get the configured order for a provider, or fall back to listing all profiles.
    fn get_base_order(&self, provider: &str) -> Vec<String> {
        let provider_lower = provider.to_lowercase();

        // Check explicit order first
        if let Some(order_map) = &self.order {
            for (key, order) in order_map {
                if key.to_lowercase() == provider_lower {
                    return order.clone();
                }
            }
        }

        // Fall back to listing all profiles for this provider
        self.list_profiles_for_provider(provider)
    }

    /// Check if a profile is currently in cooldown.
    pub fn is_in_cooldown(&self, profile_id: &str) -> bool {
        let now = now_ms();
        if let Some(stats_map) = &self.usage_stats {
            if let Some(stats) = stats_map.get(profile_id) {
                if let Some(until) = stats.cooldown_until {
                    return now < until;
                }
            }
        }
        false
    }

    /// Clear expired cooldowns from all profiles.
    /// Returns true if any profile was modified.
    pub fn clear_expired_cooldowns(&mut self) -> bool {
        let now = now_ms();
        let mut mutated = false;

        if let Some(stats_map) = &mut self.usage_stats {
            for stats in stats_map.values_mut() {
                if let Some(until) = stats.cooldown_until {
                    if now >= until {
                        // Cooldown has expired - clear it and reset error counts
                        stats.cooldown_until = None;
                        stats.error_count = Some(0);
                        stats.failure_counts = None;
                        mutated = true;
                    }
                }
            }
        }

        mutated
    }

    /// Resolve the auth profile order for a provider.
    /// Returns profile IDs sorted by:
    /// 1. Available profiles (not in cooldown), sorted by lastUsed (oldest first = round-robin)
    /// 2. Cooldown profiles, sorted by cooldown expiry (soonest first)
    pub fn resolve_auth_order(&mut self, provider: &str) -> Vec<String> {
        // Clear any expired cooldowns first
        self.clear_expired_cooldowns();

        let base_order = self.get_base_order(provider);
        if base_order.is_empty() {
            return Vec::new();
        }

        // Filter to valid profiles that exist
        let valid: Vec<String> = base_order
            .into_iter()
            .filter(|id| self.profiles.contains_key(id))
            .collect();

        // Partition into available and in-cooldown
        let mut available: Vec<(String, u64)> = Vec::new();
        let mut in_cooldown: Vec<(String, u64)> = Vec::new();

        for profile_id in valid {
            if self.is_in_cooldown(&profile_id) {
                let cooldown_until = self
                    .usage_stats
                    .as_ref()
                    .and_then(|m| m.get(&profile_id))
                    .and_then(|s| s.cooldown_until)
                    .unwrap_or(0);
                in_cooldown.push((profile_id, cooldown_until));
            } else {
                let last_used = self
                    .usage_stats
                    .as_ref()
                    .and_then(|m| m.get(&profile_id))
                    .and_then(|s| s.last_used)
                    .unwrap_or(0);
                available.push((profile_id, last_used));
            }
        }

        // Sort available by lastUsed (oldest first = round-robin)
        available.sort_by_key(|(_, last_used)| *last_used);

        // Sort cooldown profiles by cooldown expiry (soonest first)
        in_cooldown.sort_by_key(|(_, until)| *until);

        // Combine: available first, then cooldown
        let mut result: Vec<String> = available.into_iter().map(|(id, _)| id).collect();
        result.extend(in_cooldown.into_iter().map(|(id, _)| id));

        result
    }

    /// Mark a profile as successfully used.
    /// Resets error count and updates lastUsed timestamp.
    pub fn mark_used(&mut self, profile_id: &str) {
        if !self.profiles.contains_key(profile_id) {
            return;
        }

        let stats_map = self.usage_stats.get_or_insert_with(HashMap::new);
        let stats = stats_map.entry(profile_id.to_string()).or_default();

        stats.last_used = Some(now_ms());
        stats.error_count = Some(0);
        stats.cooldown_until = None;
        stats.failure_counts = None;

        tracing::debug!("Marked profile {} as used", profile_id);
    }

    /// Mark a profile as failed with a specific reason.
    /// Applies exponential backoff cooldown: 1min, 5min, 25min, max 1 hour.
    pub fn mark_failure(&mut self, profile_id: &str, reason: AuthProfileFailureReason) {
        if !self.profiles.contains_key(profile_id) {
            return;
        }

        let now = now_ms();
        let stats_map = self.usage_stats.get_or_insert_with(HashMap::new);
        let stats = stats_map.entry(profile_id.to_string()).or_default();

        // Check if previous cooldown has expired - if so, reset counters
        if let Some(until) = stats.cooldown_until {
            if now >= until {
                stats.error_count = Some(0);
                stats.failure_counts = None;
                stats.cooldown_until = None;
            }
        }

        // Increment error count
        let error_count = stats.error_count.unwrap_or(0) + 1;
        stats.error_count = Some(error_count);
        stats.last_failure_at = Some(now);

        // Track failure by reason
        let reason_key = format!("{:?}", reason).to_lowercase();
        let failure_counts = stats.failure_counts.get_or_insert_with(HashMap::new);
        *failure_counts.entry(reason_key).or_insert(0) += 1;

        // Calculate cooldown: 1min * 5^(errorCount-1), max 1 hour
        // Error 1 → 1min, Error 2 → 5min, Error 3 → 25min, Error 4+ → 60min
        let cooldown_ms = calculate_cooldown_ms(error_count);
        stats.cooldown_until = Some(now + cooldown_ms);

        tracing::warn!(
            "Profile {} failed ({:?}), error_count={}, cooldown={}ms",
            profile_id,
            reason,
            error_count,
            cooldown_ms
        );
    }

    /// Get a credential by profile ID.
    pub fn get_credential(&self, profile_id: &str) -> Option<&AuthProfileCredential> {
        self.profiles.get(profile_id)
    }
}

/// Calculate cooldown duration in milliseconds based on error count.
/// Formula: min(1 hour, 1 minute * 5^(error_count - 1))
/// Results: 1min, 5min, 25min, 1h max
fn calculate_cooldown_ms(error_count: u32) -> u64 {
    const ONE_MINUTE_MS: u64 = 60 * 1000;
    const ONE_HOUR_MS: u64 = 60 * 60 * 1000;

    let count = error_count.max(1);
    let exponent = (count - 1).min(3); // Cap at 5^3 = 125, but we'll hit 1h cap at 5^2

    let cooldown = ONE_MINUTE_MS * 5u64.pow(exponent);
    cooldown.min(ONE_HOUR_MS)
}

/// Get the default path for auth-profiles.json.
pub fn default_auth_profiles_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rustclaw")
        .join("auth-profiles.json")
}

/// Auth profile manager for the LLM client.
/// Wraps the store with path tracking for persistence.
pub struct AuthProfileManager {
    store: AuthProfileStore,
    path: PathBuf,
}

impl AuthProfileManager {
    /// Create a new manager, loading from the given path.
    pub fn new(path: Option<&str>) -> anyhow::Result<Self> {
        let path = path
            .map(PathBuf::from)
            .unwrap_or_else(default_auth_profiles_path);

        let store = AuthProfileStore::load(&path)?;

        Ok(Self { store, path })
    }

    /// Get the next available profile for a provider.
    /// Returns the profile ID or None if no profiles are available.
    pub fn next_profile(&mut self, provider: &str) -> Option<String> {
        let order = self.store.resolve_auth_order(provider);
        order.into_iter().next()
    }

    /// Get a credential by profile ID.
    pub fn get_credential(&self, profile_id: &str) -> Option<&AuthProfileCredential> {
        self.store.get_credential(profile_id)
    }

    /// Mark a profile as successfully used.
    pub fn mark_used(&mut self, profile_id: &str) {
        self.store.mark_used(profile_id);
        if let Err(e) = self.store.save(&self.path) {
            tracing::error!("Failed to save auth profiles: {}", e);
        }
    }

    /// Mark a profile as failed.
    pub fn mark_failure(&mut self, profile_id: &str, reason: AuthProfileFailureReason) {
        self.store.mark_failure(profile_id, reason);
        if let Err(e) = self.store.save(&self.path) {
            tracing::error!("Failed to save auth profiles: {}", e);
        }
    }

    /// Check if a profile is in cooldown.
    pub fn is_in_cooldown(&self, profile_id: &str) -> bool {
        self.store.is_in_cooldown(profile_id)
    }

    /// Get the underlying store (for testing/debugging).
    pub fn store(&self) -> &AuthProfileStore {
        &self.store
    }

    /// Get mutable access to the underlying store (for resolve_auth_order which clears expired cooldowns).
    pub fn store_mut(&mut self) -> &mut AuthProfileStore {
        &mut self.store
    }

    /// Check if the manager has any profiles for a provider.
    pub fn has_profiles(&self, provider: &str) -> bool {
        !self.store.list_profiles_for_provider(provider).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooldown_calculation() {
        assert_eq!(calculate_cooldown_ms(1), 60_000); // 1 min
        assert_eq!(calculate_cooldown_ms(2), 300_000); // 5 min
        assert_eq!(calculate_cooldown_ms(3), 1_500_000); // 25 min
        assert_eq!(calculate_cooldown_ms(4), 3_600_000); // 1 hour (capped)
        assert_eq!(calculate_cooldown_ms(5), 3_600_000); // 1 hour (capped)
    }

    #[test]
    fn test_store_round_robin() {
        let mut store = AuthProfileStore {
            version: 1,
            profiles: [
                (
                    "a".to_string(),
                    AuthProfileCredential::Token {
                        provider: "anthropic".to_string(),
                        token: "token_a".to_string(),
                        expires: None,
                    },
                ),
                (
                    "b".to_string(),
                    AuthProfileCredential::Token {
                        provider: "anthropic".to_string(),
                        token: "token_b".to_string(),
                        expires: None,
                    },
                ),
            ]
            .into(),
            order: Some([("anthropic".to_string(), vec!["a".to_string(), "b".to_string()])].into()),
            last_good: None,
            usage_stats: None,
        };

        // First call should return "a" (both unused, follows order)
        let order = store.resolve_auth_order("anthropic");
        assert_eq!(order, vec!["a", "b"]);

        // Mark "a" as used
        store.mark_used("a");

        // Now "b" should come first (older lastUsed)
        let order = store.resolve_auth_order("anthropic");
        assert_eq!(order, vec!["b", "a"]);
    }

    #[test]
    fn test_cooldown_ordering() {
        let now = now_ms();
        let mut store = AuthProfileStore {
            version: 1,
            profiles: [
                (
                    "a".to_string(),
                    AuthProfileCredential::Token {
                        provider: "anthropic".to_string(),
                        token: "token_a".to_string(),
                        expires: None,
                    },
                ),
                (
                    "b".to_string(),
                    AuthProfileCredential::Token {
                        provider: "anthropic".to_string(),
                        token: "token_b".to_string(),
                        expires: None,
                    },
                ),
            ]
            .into(),
            order: None,
            last_good: None,
            usage_stats: Some(
                [(
                    "a".to_string(),
                    ProfileUsageStats {
                        cooldown_until: Some(now + 60_000), // 1 min in future
                        ..Default::default()
                    },
                )]
                .into(),
            ),
        };

        // "b" should come first (not in cooldown)
        let order = store.resolve_auth_order("anthropic");
        assert_eq!(order[0], "b");
        assert!(order.contains(&"a".to_string())); // "a" should be at end
    }
}
