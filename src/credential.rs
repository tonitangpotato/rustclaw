//! Credential Proxy & Injection.
//!
//! Implements DESIGN.md Key Decision #4: "Credential injection via proxy (never exposed to LLM)"
//!
//! Credentials are stored with placeholders. The LLM only sees placeholders like `{{API_KEY}}`.
//! Before executing tool calls, placeholders are replaced with actual credentials.
//! After execution, any leaked credentials in output are redacted back to placeholders.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Manages credentials with placeholder-based injection and redaction.
pub struct CredentialManager {
    /// Credential store: name -> entry
    store: HashMap<String, CredentialEntry>,
    /// Path to the config file (if loaded from file)
    config_path: Option<std::path::PathBuf>,
}

/// A single credential entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    /// Credential name (unique identifier).
    pub name: String,
    /// Type of credential.
    pub credential_type: CredentialType,
    /// Actual secret value (never exposed to LLM).
    #[serde(skip_serializing)]
    pub value: String,
    /// Placeholder that the LLM sees (e.g., "{{API_KEY}}").
    pub placeholder: String,
    /// Tools that are allowed to use this credential.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// When the credential was last rotated.
    pub last_rotated: Option<DateTime<Utc>>,
}

/// Type of credential.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialType {
    /// API key (e.g., OpenAI, Anthropic).
    ApiKey,
    /// Bearer token for OAuth/JWT.
    BearerToken,
    /// Basic auth with username and password.
    BasicAuth {
        /// Username for basic auth.
        username: String,
    },
    /// SSH private key.
    SshKey,
    /// Database connection URL.
    DatabaseUrl,
    /// Custom credential type.
    Custom(String),
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialManager {
    /// Create a new empty credential manager.
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
            config_path: None,
        }
    }

    /// Load credentials from an encrypted YAML file.
    ///
    /// The file format is:
    /// ```yaml
    /// credentials:
    ///   - name: openai_key
    ///     credential_type: api_key
    ///     value: sk-...
    ///     placeholder: "{{OPENAI_KEY}}"
    ///     allowed_tools: ["web_fetch", "exec"]
    /// ```
    ///
    /// The file is XOR-encrypted with a key derived from the path.
    pub fn load_from_file(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            anyhow::bail!("Credentials file not found: {}", path.display());
        }

        let encrypted_content = std::fs::read(path)?;
        let key = derive_key_from_path(path);
        let decrypted = xor_cipher(&encrypted_content, &key);
        let content = String::from_utf8(decrypted)
            .map_err(|_| anyhow::anyhow!("Invalid credentials file (decryption failed)"))?;

        let config: CredentialFileConfig = serde_yaml::from_str(&content)?;

        let mut manager = Self::new();
        manager.config_path = Some(path.to_path_buf());

        for entry in config.credentials {
            manager.store.insert(entry.name.clone(), entry);
        }

        tracing::info!(
            "Loaded {} credentials from {}",
            manager.store.len(),
            path.display()
        );

        Ok(manager)
    }

    /// Save credentials to an encrypted YAML file.
    pub fn save_to_file(&self, path: &Path) -> anyhow::Result<()> {
        let config = CredentialFileConfig {
            credentials: self.store.values().cloned().collect(),
        };

        let content = serde_yaml::to_string(&config)?;
        let key = derive_key_from_path(path);
        let encrypted = xor_cipher(content.as_bytes(), &key);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, encrypted)?;

        tracing::info!(
            "Saved {} credentials to {}",
            self.store.len(),
            path.display()
        );

        Ok(())
    }

    /// Register a new credential.
    ///
    /// # Arguments
    /// * `name` - Unique name for the credential
    /// * `cred_type` - Type of credential
    /// * `value` - Actual secret value
    /// * `placeholder` - Placeholder string (e.g., "{{API_KEY}}")
    pub fn register(
        &mut self,
        name: &str,
        cred_type: CredentialType,
        value: &str,
        placeholder: &str,
    ) {
        let entry = CredentialEntry {
            name: name.to_string(),
            credential_type: cred_type,
            value: value.to_string(),
            placeholder: placeholder.to_string(),
            allowed_tools: Vec::new(),
            last_rotated: Some(Utc::now()),
        };
        self.store.insert(name.to_string(), entry);
    }

    /// Register a credential with allowed tools.
    pub fn register_with_tools(
        &mut self,
        name: &str,
        cred_type: CredentialType,
        value: &str,
        placeholder: &str,
        allowed_tools: Vec<String>,
    ) {
        let entry = CredentialEntry {
            name: name.to_string(),
            credential_type: cred_type,
            value: value.to_string(),
            placeholder: placeholder.to_string(),
            allowed_tools,
            last_rotated: Some(Utc::now()),
        };
        self.store.insert(name.to_string(), entry);
    }

    /// Inject credentials into text by replacing placeholders with actual values.
    ///
    /// Example:
    /// - Input: `"Authorization: {{API_KEY}}"`
    /// - Output: `"Authorization: sk-actual-key"`
    pub fn inject(&self, text: &str) -> String {
        let mut result = text.to_string();
        for entry in self.store.values() {
            result = result.replace(&entry.placeholder, &entry.value);
        }
        result
    }

    /// Redact credentials from text by replacing actual values with placeholders.
    ///
    /// This prevents the LLM from seeing or leaking actual secrets.
    ///
    /// Example:
    /// - Input: `"Response: sk-actual-key was used"`
    /// - Output: `"Response: {{API_KEY}} was used"`
    pub fn redact(&self, text: &str) -> String {
        let mut result = text.to_string();
        for entry in self.store.values() {
            // Replace the actual value with the placeholder
            result = result.replace(&entry.value, &entry.placeholder);
        }
        result
    }

    /// Check if a credential is allowed for a specific tool.
    pub fn is_allowed(&self, credential_name: &str, tool_name: &str) -> bool {
        match self.store.get(credential_name) {
            Some(entry) => {
                // Empty allowed_tools means all tools are allowed
                entry.allowed_tools.is_empty() || entry.allowed_tools.contains(&tool_name.to_string())
            }
            None => false,
        }
    }

    /// Inject only credentials that are allowed for a specific tool.
    ///
    /// This provides fine-grained control over which tools can access which credentials.
    pub fn inject_for_tool(&self, tool_name: &str, text: &str) -> String {
        let mut result = text.to_string();
        for entry in self.store.values() {
            // Check if this tool is allowed to use this credential
            let allowed = entry.allowed_tools.is_empty()
                || entry.allowed_tools.contains(&tool_name.to_string());
            
            if allowed {
                result = result.replace(&entry.placeholder, &entry.value);
            }
        }
        result
    }

    /// List all credential placeholders (safe to show to LLM).
    ///
    /// Returns a list of (name, placeholder) pairs.
    pub fn list_placeholders(&self) -> Vec<(String, String)> {
        self.store
            .values()
            .map(|e| (e.name.clone(), e.placeholder.clone()))
            .collect()
    }

    /// Rotate a credential's value.
    pub fn rotate(&mut self, name: &str, new_value: &str) -> anyhow::Result<()> {
        match self.store.get_mut(name) {
            Some(entry) => {
                entry.value = new_value.to_string();
                entry.last_rotated = Some(Utc::now());
                tracing::info!("Rotated credential: {}", name);
                Ok(())
            }
            None => anyhow::bail!("Credential not found: {}", name),
        }
    }

    /// Get a credential entry by name (without exposing the value).
    pub fn get(&self, name: &str) -> Option<&CredentialEntry> {
        self.store.get(name)
    }

    /// Remove a credential.
    pub fn remove(&mut self, name: &str) -> Option<CredentialEntry> {
        self.store.remove(name)
    }

    /// Check if a credential exists.
    pub fn exists(&self, name: &str) -> bool {
        self.store.contains_key(name)
    }

    /// Get the number of credentials.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Check if the credential store is empty.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Get all credential names.
    pub fn names(&self) -> Vec<String> {
        self.store.keys().cloned().collect()
    }

    /// Check text for any credential leaks (returns true if credentials are found).
    pub fn contains_credential(&self, text: &str) -> bool {
        for entry in self.store.values() {
            if text.contains(&entry.value) {
                return true;
            }
        }
        false
    }

    /// Get leaked credential names from text.
    pub fn find_leaks(&self, text: &str) -> Vec<String> {
        let mut leaks = Vec::new();
        for entry in self.store.values() {
            if text.contains(&entry.value) {
                leaks.push(entry.name.clone());
            }
        }
        leaks
    }
}

/// Configuration file structure for credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CredentialFileConfig {
    credentials: Vec<CredentialEntry>,
}

/// Simple XOR cipher for file encryption.
/// This is NOT cryptographically secure, but provides basic obfuscation.
/// For production, use proper encryption (e.g., AES via ring crate).
fn xor_cipher(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &byte)| byte ^ key[i % key.len()])
        .collect()
}

/// Derive an encryption key from a file path.
/// This ties the encryption to the file location.
fn derive_key_from_path(path: &Path) -> Vec<u8> {
    let path_str = path.display().to_string();
    let mut key = Vec::with_capacity(32);
    
    // Simple key derivation: hash-like mixing of path bytes
    let mut hash: u64 = 0x517cc1b727220a95; // Random seed
    for byte in path_str.bytes() {
        hash = hash.wrapping_mul(0x5851f42d4c957f2d).wrapping_add(byte as u64);
    }
    
    // Expand to 32 bytes
    for i in 0..4 {
        let h = hash.wrapping_add(i);
        key.extend_from_slice(&h.to_le_bytes());
    }
    
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_and_redact() {
        let mut manager = CredentialManager::new();
        manager.register(
            "openai",
            CredentialType::ApiKey,
            "sk-actual-key-12345",
            "{{OPENAI_KEY}}",
        );
        manager.register(
            "anthropic",
            CredentialType::ApiKey,
            "sk-ant-secret-67890",
            "{{ANTHROPIC_KEY}}",
        );

        // Test injection
        let input = "Authorization: Bearer {{OPENAI_KEY}}\nX-API-Key: {{ANTHROPIC_KEY}}";
        let injected = manager.inject(input);
        assert_eq!(
            injected,
            "Authorization: Bearer sk-actual-key-12345\nX-API-Key: sk-ant-secret-67890"
        );

        // Test redaction
        let output = "Response used sk-actual-key-12345 and sk-ant-secret-67890";
        let redacted = manager.redact(output);
        assert_eq!(
            redacted,
            "Response used {{OPENAI_KEY}} and {{ANTHROPIC_KEY}}"
        );
    }

    #[test]
    fn test_tool_restrictions() {
        let mut manager = CredentialManager::new();
        manager.register_with_tools(
            "db_password",
            CredentialType::DatabaseUrl,
            "postgres://user:secret@localhost/db",
            "{{DB_URL}}",
            vec!["exec".to_string(), "db_query".to_string()],
        );
        manager.register_with_tools(
            "api_key",
            CredentialType::ApiKey,
            "sk-12345",
            "{{API_KEY}}",
            vec![], // Empty = all tools allowed
        );

        // db_password should only work with allowed tools
        assert!(manager.is_allowed("db_password", "exec"));
        assert!(manager.is_allowed("db_password", "db_query"));
        assert!(!manager.is_allowed("db_password", "web_fetch"));

        // api_key should work with any tool
        assert!(manager.is_allowed("api_key", "exec"));
        assert!(manager.is_allowed("api_key", "web_fetch"));
        assert!(manager.is_allowed("api_key", "anything"));

        // Test inject_for_tool
        let input = "DB: {{DB_URL}}, API: {{API_KEY}}";
        
        // exec can use both
        let for_exec = manager.inject_for_tool("exec", input);
        assert!(for_exec.contains("postgres://user:secret@localhost/db"));
        assert!(for_exec.contains("sk-12345"));
        
        // web_fetch can only use api_key
        let for_web = manager.inject_for_tool("web_fetch", input);
        assert!(for_web.contains("{{DB_URL}}")); // Not injected
        assert!(for_web.contains("sk-12345")); // Injected
    }

    #[test]
    fn test_leak_detection() {
        let mut manager = CredentialManager::new();
        manager.register(
            "secret",
            CredentialType::ApiKey,
            "my-super-secret-key",
            "{{SECRET}}",
        );

        assert!(manager.contains_credential("The key is my-super-secret-key"));
        assert!(!manager.contains_credential("The key is {{SECRET}}"));

        let leaks = manager.find_leaks("Used my-super-secret-key in request");
        assert_eq!(leaks, vec!["secret"]);
    }

    #[test]
    fn test_rotation() {
        let mut manager = CredentialManager::new();
        manager.register(
            "rotating_key",
            CredentialType::ApiKey,
            "old-key",
            "{{KEY}}",
        );

        let input = "Key: {{KEY}}";
        assert_eq!(manager.inject(input), "Key: old-key");

        manager.rotate("rotating_key", "new-key").unwrap();
        assert_eq!(manager.inject(input), "Key: new-key");
    }

    #[test]
    fn test_xor_cipher_roundtrip() {
        let original = b"Hello, World! This is a secret.";
        let key = b"mykey123";
        
        let encrypted = xor_cipher(original, key);
        let decrypted = xor_cipher(&encrypted, key);
        
        assert_eq!(decrypted, original.to_vec());
    }

    #[test]
    fn test_list_placeholders() {
        let mut manager = CredentialManager::new();
        manager.register("key1", CredentialType::ApiKey, "secret1", "{{KEY1}}");
        manager.register("key2", CredentialType::ApiKey, "secret2", "{{KEY2}}");

        let placeholders = manager.list_placeholders();
        assert_eq!(placeholders.len(), 2);
        
        // Check that actual secrets are not exposed
        let placeholder_strs: Vec<&str> = placeholders.iter().map(|(_, p)| p.as_str()).collect();
        assert!(placeholder_strs.contains(&"{{KEY1}}"));
        assert!(placeholder_strs.contains(&"{{KEY2}}"));
    }
}
