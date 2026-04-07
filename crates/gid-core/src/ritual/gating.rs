//! Tool Gating — configuration-driven access control for source code operations.
//!
//! When no ritual is active, gated operations (write to source paths, run build commands)
//! are blocked. This forces the developer/agent to go through the ritual pipeline
//! (design → implement → verify) instead of ad-hoc edits.
//!
//! Config lives in `.gid/config.yml` under `ritual.gating`.

use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Top-level gating configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatingConfig {
    /// Whether gating is enabled at all.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Paths that require an active ritual to write to (glob patterns).
    #[serde(default)]
    pub gated_paths: Vec<String>,
    /// Paths that are always allowed (overrides gated_paths).
    #[serde(default = "default_ungated_paths")]
    pub ungated_paths: Vec<String>,
    /// Shell command patterns that require an active ritual.
    #[serde(default)]
    pub gated_commands: Vec<CommandPattern>,
    /// Verify command (build + test).
    #[serde(default)]
    pub verify_command: Option<String>,
}

/// A command pattern — either glob or regex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPattern {
    pub pattern: String,
    #[serde(default = "default_regex")]
    pub r#type: PatternType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PatternType {
    Glob,
    Regex,
}

fn default_true() -> bool { true }
fn default_regex() -> PatternType { PatternType::Regex }

fn default_ungated_paths() -> Vec<String> {
    vec![
        "DESIGN.md".into(),
        ".gid/**".into(),
        "docs/**".into(),
        "memory/**".into(),
        "AGENTS.md".into(),
        "TOOLS.md".into(),
        "MEMORY.md".into(),
        "*.md".into(),
    ]
}

impl Default for GatingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            gated_paths: vec![],
            ungated_paths: default_ungated_paths(),
            gated_commands: vec![],
            verify_command: None,
        }
    }
}

impl CommandPattern {
    pub fn regex(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            r#type: PatternType::Regex,
        }
    }

    pub fn glob(pattern: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            r#type: PatternType::Glob,
        }
    }

    /// Check if a command string matches this pattern.
    pub fn matches(&self, command: &str) -> bool {
        match self.r#type {
            PatternType::Regex => {
                match regex::Regex::new(&self.pattern) {
                    Ok(re) => re.is_match(command),
                    Err(e) => {
                        warn!(pattern = %self.pattern, error = %e, "Invalid gating regex");
                        false
                    }
                }
            }
            PatternType::Glob => {
                glob_match(&self.pattern, command)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Language defaults
// ═══════════════════════════════════════════════════════════════════════════════

use super::composer::ProjectLanguage;

impl GatingConfig {
    /// Generate a default gating config for a detected project language.
    pub fn for_language(lang: &ProjectLanguage) -> Self {
        match lang {
            ProjectLanguage::Rust => Self {
                enabled: true,
                gated_paths: vec![
                    "src/**".into(), "tests/**".into(), "crates/**".into(),
                    "Cargo.toml".into(), "build.rs".into(),
                ],
                ungated_paths: default_ungated_paths(),
                gated_commands: vec![
                    CommandPattern::regex(r"^cargo\s+(build|run|test|check)\b"),
                    CommandPattern::regex(r"^rustc\b"),
                ],
                verify_command: Some("cargo build 2>&1 && cargo test 2>&1".into()),
            },
            ProjectLanguage::TypeScript => Self {
                enabled: true,
                gated_paths: vec![
                    "src/**".into(), "lib/**".into(), "tests/**".into(),
                    "package.json".into(), "tsconfig.json".into(),
                ],
                ungated_paths: default_ungated_paths(),
                gated_commands: vec![
                    CommandPattern::regex(r"^npm\s+run\s+(build|start)\b"),
                    CommandPattern::regex(r"^tsc\b"),
                    CommandPattern::regex(r"^npx\b"),
                ],
                verify_command: Some("npm run build 2>&1 && npm test 2>&1".into()),
            },
            ProjectLanguage::Python => Self {
                enabled: true,
                gated_paths: vec![
                    "**/*.py".into(), "setup.py".into(), "pyproject.toml".into(),
                ],
                ungated_paths: default_ungated_paths(),
                gated_commands: vec![
                    CommandPattern::regex(r"^python\b"),
                    CommandPattern::regex(r"^pip\s+install"),
                ],
                verify_command: Some("python -m pytest 2>&1".into()),
            },
            ProjectLanguage::Go => Self {
                enabled: true,
                gated_paths: vec![
                    "**/*.go".into(), "go.mod".into(), "go.sum".into(),
                ],
                ungated_paths: default_ungated_paths(),
                gated_commands: vec![
                    CommandPattern::regex(r"^go\s+(build|run|test)\b"),
                ],
                verify_command: Some("go build ./... 2>&1 && go test ./... 2>&1".into()),
            },
            ProjectLanguage::Other(_) => Self {
                enabled: true,
                verify_command: None,
                ..Default::default()
            },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Config loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Wrapper for the full `.gid/config.yml` file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GidConfig {
    #[serde(default)]
    pub ritual: RitualConfigSection,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RitualConfigSection {
    #[serde(default)]
    pub gating: GatingConfig,
}

/// Load gating config from `.gid/config.yml`.
/// Returns default config if file doesn't exist or is malformed.
pub fn load_gating_config(project_root: &Path) -> GatingConfig {
    let config_path = project_root.join(".gid").join("config.yml");

    if !config_path.exists() {
        debug!("No .gid/config.yml, using default gating config");
        return GatingConfig::default();
    }

    match std::fs::read_to_string(&config_path) {
        Ok(content) => {
            match serde_yaml::from_str::<GidConfig>(&content) {
                Ok(config) => config.ritual.gating,
                Err(e) => {
                    warn!(error = %e, "Failed to parse .gid/config.yml, using defaults");
                    GatingConfig::default()
                }
            }
        }
        Err(e) => {
            warn!(error = %e, "Failed to read .gid/config.yml, using defaults");
            GatingConfig::default()
        }
    }
}

/// Save gating config to `.gid/config.yml`.
pub fn save_gating_config(project_root: &Path, config: &GatingConfig) -> anyhow::Result<()> {
    let config_path = project_root.join(".gid").join("config.yml");

    // Ensure .gid/ exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let full_config = GidConfig {
        ritual: RitualConfigSection {
            gating: config.clone(),
        },
    };

    let yaml = serde_yaml::to_string(&full_config)?;
    std::fs::write(&config_path, yaml)?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Gating check
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of a gating check.
#[derive(Debug)]
pub enum GatingResult {
    /// Operation is allowed.
    Allowed,
    /// Operation is blocked — must start a ritual first.
    Blocked { reason: String },
}

/// Check if a tool call is gated (requires active ritual).
///
/// Returns `Allowed` if:
/// - Gating is disabled
/// - A ritual is currently active (`ritual_active = true`)
/// - The operation doesn't match any gated pattern
/// - The path is in the ungated whitelist
///
/// Returns `Blocked` if the operation matches a gated pattern and no ritual is active.
pub fn check_gating(
    config: &GatingConfig,
    tool_name: &str,
    path: Option<&str>,
    command: Option<&str>,
    ritual_active: bool,
) -> GatingResult {
    // Gating disabled
    if !config.enabled {
        return GatingResult::Allowed;
    }

    // Active ritual → all tools allowed (inner ToolScope handles phase restrictions)
    if ritual_active {
        return GatingResult::Allowed;
    }

    // No gated paths configured → no restrictions
    if config.gated_paths.is_empty() && config.gated_commands.is_empty() {
        return GatingResult::Allowed;
    }

    // Check file write operations
    if matches!(tool_name, "write_file" | "edit_file" | "Write" | "Edit" | "create_file") {
        if let Some(file_path) = path {
            // Ungated whitelist takes priority
            if config.ungated_paths.iter().any(|p| glob_match(p, file_path)) {
                return GatingResult::Allowed;
            }
            // Check gated paths
            if config.gated_paths.iter().any(|p| glob_match(p, file_path)) {
                return GatingResult::Blocked {
                    reason: format!(
                        "⚠️ Writing to `{}` requires an active ritual.\n\
                         Use `/ritual <task description>` to start one.\n\
                         This ensures design → implement → verify quality gates.",
                        file_path
                    ),
                };
            }
        }
    }

    // Check shell commands
    if matches!(tool_name, "exec" | "Bash" | "bash" | "shell") {
        if let Some(cmd) = command {
            if config.gated_commands.iter().any(|gc| gc.matches(cmd)) {
                return GatingResult::Blocked {
                    reason: format!(
                        "⚠️ Running `{}` requires an active ritual.\n\
                         Use `/ritual <task description>` to start one.",
                        truncate_cmd(cmd, 50)
                    ),
                };
            }
        }
    }

    GatingResult::Allowed
}

fn truncate_cmd(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Simple glob matching (supports `*`, `**`, `?`).
fn glob_match(pattern: &str, path: &str) -> bool {
    // Normalize
    let pattern = pattern.trim();
    let path = path.trim().trim_start_matches("./");

    // Exact match
    if pattern == path {
        return true;
    }

    // ** matches any path depth
    if pattern == "**" {
        return true;
    }

    // pattern/**  matches any file under pattern/
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(prefix) && path.len() > prefix.len();
    }

    // **/pattern matches pattern at any depth (including root)
    if let Some(suffix) = pattern.strip_prefix("**/") {
        // suffix could be "*.py" — need to match it as a glob
        if suffix.starts_with("*.") {
            // **/*.ext — match extension at any depth
            let ext = &suffix[1..]; // ".py"
            return path.ends_with(ext);
        }
        return path == suffix || path.ends_with(&format!("/{}", suffix));
    }

    // *.ext matches any file with that extension (root level only)
    if let Some(ext) = pattern.strip_prefix("*.") {
        // Only match files without path separators (root level)
        return !path.contains('/') && path.ends_with(&format!(".{}", ext));
    }

    // pattern/* matches direct children
    if let Some(prefix) = pattern.strip_suffix("/*") {
        if let Some(rest) = path.strip_prefix(prefix) {
            let rest = rest.trim_start_matches('/');
            return !rest.contains('/');
        }
        return false;
    }

    false
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_config() -> GatingConfig {
        GatingConfig::for_language(&ProjectLanguage::Rust)
    }

    // ── Glob matching ──

    #[test]
    fn test_glob_exact() {
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("Cargo.toml", "cargo.toml"));
    }

    #[test]
    fn test_glob_dir_wildcard() {
        assert!(glob_match("src/**", "src/main.rs"));
        assert!(glob_match("src/**", "src/utils/helpers.rs"));
        assert!(!glob_match("src/**", "tests/test.rs"));
    }

    #[test]
    fn test_glob_recursive() {
        assert!(glob_match("**/*.py", "main.py"));
        assert!(glob_match("**/*.py", "src/utils/helper.py"));
    }

    #[test]
    fn test_glob_extension() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "src/main.rs")); // *.rs only matches root
    }

    #[test]
    fn test_glob_md() {
        assert!(glob_match("*.md", "DESIGN.md"));
        assert!(glob_match("*.md", "README.md"));
    }

    // ── Ungated paths ──

    #[test]
    fn test_ungated_design_md() {
        let config = rust_config();
        let result = check_gating(&config, "write_file", Some("DESIGN.md"), None, false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    #[test]
    fn test_ungated_gid_dir() {
        let config = rust_config();
        let result = check_gating(&config, "write_file", Some(".gid/graph.yml"), None, false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    #[test]
    fn test_ungated_docs() {
        let config = rust_config();
        let result = check_gating(&config, "write_file", Some("docs/api.md"), None, false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    // ── Gated paths ──

    #[test]
    fn test_gated_src() {
        let config = rust_config();
        let result = check_gating(&config, "write_file", Some("src/main.rs"), None, false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    #[test]
    fn test_gated_cargo_toml() {
        let config = rust_config();
        let result = check_gating(&config, "edit_file", Some("Cargo.toml"), None, false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    #[test]
    fn test_gated_tests() {
        let config = rust_config();
        let result = check_gating(&config, "Write", Some("tests/test_foo.rs"), None, false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    // ── Gated commands ──

    #[test]
    fn test_gated_cargo_build() {
        let config = rust_config();
        let result = check_gating(&config, "exec", None, Some("cargo build"), false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    #[test]
    fn test_gated_cargo_test() {
        let config = rust_config();
        let result = check_gating(&config, "Bash", None, Some("cargo test --lib"), false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    #[test]
    fn test_ungated_cargo_doc() {
        let config = rust_config();
        // cargo doc is NOT gated
        let result = check_gating(&config, "exec", None, Some("cargo doc"), false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    #[test]
    fn test_ungated_ls_command() {
        let config = rust_config();
        let result = check_gating(&config, "exec", None, Some("ls -la"), false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    // ── Ritual active bypasses gating ──

    #[test]
    fn test_ritual_active_allows_all() {
        let config = rust_config();
        let result = check_gating(&config, "write_file", Some("src/main.rs"), None, true);
        assert!(matches!(result, GatingResult::Allowed));

        let result = check_gating(&config, "exec", None, Some("cargo build"), true);
        assert!(matches!(result, GatingResult::Allowed));
    }

    // ── Gating disabled ──

    #[test]
    fn test_gating_disabled() {
        let mut config = rust_config();
        config.enabled = false;
        let result = check_gating(&config, "write_file", Some("src/main.rs"), None, false);
        assert!(matches!(result, GatingResult::Allowed));
    }

    // ── Command patterns ──

    #[test]
    fn test_command_pattern_regex() {
        let pat = CommandPattern::regex(r"^cargo\s+(build|run|test|check)\b");
        assert!(pat.matches("cargo build"));
        assert!(pat.matches("cargo test --lib"));
        assert!(pat.matches("cargo check"));
        assert!(!pat.matches("cargo doc"));
        assert!(!pat.matches("echo cargo build"));
    }

    // ── Language defaults ──

    #[test]
    fn test_typescript_defaults() {
        let config = GatingConfig::for_language(&ProjectLanguage::TypeScript);
        assert!(config.gated_paths.contains(&"src/**".to_string()));
        assert_eq!(
            config.verify_command.as_deref(),
            Some("npm run build 2>&1 && npm test 2>&1")
        );
    }

    #[test]
    fn test_python_defaults() {
        let config = GatingConfig::for_language(&ProjectLanguage::Python);
        let result = check_gating(&config, "exec", None, Some("python main.py"), false);
        assert!(matches!(result, GatingResult::Blocked { .. }));
    }

    // ── Serialization ──

    #[test]
    fn test_config_roundtrip() {
        let config = rust_config();
        let yaml = serde_yaml::to_string(&GidConfig {
            ritual: RitualConfigSection { gating: config.clone() },
        }).unwrap();

        let parsed: GidConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.ritual.gating.enabled, config.enabled);
        assert_eq!(parsed.ritual.gating.gated_paths.len(), config.gated_paths.len());
    }
}
