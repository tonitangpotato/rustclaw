//! Skill metadata schema.
//!
//! Defines the YAML frontmatter structure for skill files.
//! Skills are markdown documents with structured metadata that
//! enables discovery, matching, and management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A loaded skill with metadata and content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Parsed metadata from YAML frontmatter.
    pub metadata: SkillMetadata,
    /// Raw markdown body (after frontmatter).
    pub body: String,
    /// Source file path (if loaded from disk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
}

impl Skill {
    /// Get the skill's name.
    pub fn name(&self) -> &str {
        &self.metadata.name
    }

    /// Get the skill's description.
    pub fn description(&self) -> &str {
        &self.metadata.description
    }

    /// Get the skill's priority (0-100, higher = more important).
    pub fn priority(&self) -> u8 {
        self.metadata.priority
    }

    /// Whether this skill should always be loaded into context.
    pub fn always_load(&self) -> bool {
        self.metadata.always_load
    }

    /// Whether this skill is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.metadata.status == SkillStatus::Enabled
    }

    /// Get the full content (frontmatter + body) as it would appear in a prompt.
    /// Returns just the body — the frontmatter is metadata, not prompt content.
    pub fn prompt_content(&self) -> &str {
        &self.body
    }

    /// Get the full content with metadata header for display.
    pub fn display_content(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n", self.metadata.name));
        if !self.metadata.description.is_empty() {
            out.push_str(&format!("> {}\n\n", self.metadata.description));
        }
        out.push_str(&self.body);
        out
    }

    /// Get tags as a slice.
    pub fn tags(&self) -> &[String] {
        &self.metadata.tags
    }

    /// Get the directory name (last component of source path's parent).
    pub fn dir_name(&self) -> Option<&str> {
        self.source_path
            .as_ref()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
    }
}

/// Structured metadata from a skill's YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Unique name (kebab-case recommended). Required.
    pub name: String,

    /// Human-readable description. Required.
    #[serde(default)]
    pub description: String,

    /// Version string (semver recommended).
    #[serde(default = "default_version")]
    pub version: String,

    /// Trigger configuration for input matching.
    #[serde(default)]
    pub triggers: TriggerConfig,

    /// Priority (0-100, higher = more important). Default: 50.
    #[serde(default = "default_priority")]
    pub priority: u8,

    /// Whether this skill should always be injected into the system prompt.
    #[serde(default)]
    pub always_load: bool,

    /// Tags for categorization and search.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Current status (enabled/disabled).
    #[serde(default)]
    pub status: SkillStatus,

    /// Author name or identifier.
    #[serde(default)]
    pub author: String,

    /// Maximum body size in bytes to inject (0 = no limit). Default: 4096.
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    /// When this skill was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,

    /// When this skill was last modified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Trigger configuration — how a skill matches user input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Substring patterns to match (case-insensitive).
    /// If *any* pattern matches, the skill triggers.
    /// Example: `["http://", "https://"]`
    #[serde(default)]
    pub patterns: Vec<String>,

    /// Keywords/phrases to match (case-insensitive, word boundary).
    /// Matched with some fuzziness — "scrape" matches "scraping".
    /// Example: `["scrape", "fetch page", "download"]`
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Regex patterns for advanced matching.
    /// Example: `["\\bhttps?://[^\\s]+"]`
    #[serde(default)]
    pub regex: Vec<String>,

    /// Glob patterns for matching (shell-style).
    /// Example: `["*.py", "*.rs"]`
    #[serde(default)]
    pub globs: Vec<String>,
}

impl TriggerConfig {
    /// Check if any triggers are defined.
    pub fn has_triggers(&self) -> bool {
        !self.patterns.is_empty()
            || !self.keywords.is_empty()
            || !self.regex.is_empty()
            || !self.globs.is_empty()
    }
}

/// Skill status — whether it's active or disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillStatus {
    Enabled,
    Disabled,
}

impl Default for SkillStatus {
    fn default() -> Self {
        Self::Enabled
    }
}

impl std::fmt::Display for SkillStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enabled => write!(f, "enabled"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_priority() -> u8 {
    50
}

fn default_max_body_size() -> usize {
    4096
}

/// Validation errors for skill metadata.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("skill name is empty")]
    EmptyName,

    #[error("skill name contains invalid characters: {0} (use kebab-case)")]
    InvalidName(String),

    #[error("priority must be 0-100, got {0}")]
    InvalidPriority(u8),

    #[error("invalid regex pattern '{pattern}': {reason}")]
    InvalidRegex { pattern: String, reason: String },

    #[error("invalid glob pattern '{pattern}': {reason}")]
    InvalidGlob { pattern: String, reason: String },
}

impl SkillMetadata {
    /// Validate this metadata, returning all errors found.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Name validation
        if self.name.is_empty() {
            errors.push(ValidationError::EmptyName);
        } else if !self.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            errors.push(ValidationError::InvalidName(self.name.clone()));
        }

        // Priority validation
        if self.priority > 100 {
            errors.push(ValidationError::InvalidPriority(self.priority));
        }

        // Regex validation
        for pattern in &self.triggers.regex {
            if let Err(e) = regex::Regex::new(pattern) {
                errors.push(ValidationError::InvalidRegex {
                    pattern: pattern.clone(),
                    reason: e.to_string(),
                });
            }
        }

        // Glob validation
        for pattern in &self.triggers.globs {
            if let Err(e) = glob::Pattern::new(pattern) {
                errors.push(ValidationError::InvalidGlob {
                    pattern: pattern.clone(),
                    reason: e.to_string(),
                });
            }
        }

        errors
    }

    /// Check if this metadata is valid.
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_metadata() {
        let yaml = r#"
name: test-skill
description: A test skill
"#;
        let meta: SkillMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.name, "test-skill");
        assert_eq!(meta.description, "A test skill");
        assert_eq!(meta.version, "0.1.0");
        assert_eq!(meta.priority, 50);
        assert!(!meta.always_load);
        assert!(meta.tags.is_empty());
        assert_eq!(meta.status, SkillStatus::Enabled);
        assert_eq!(meta.max_body_size, 4096);
    }

    #[test]
    fn test_full_metadata() {
        let yaml = r#"
name: web-scraping
description: Extract content from web pages
version: "1.0.0"
triggers:
  patterns: ["http://", "https://"]
  keywords: ["scrape", "fetch page"]
  regex: ["\\bhttps?://"]
priority: 80
always_load: false
tags: [web, extraction]
author: potato
max_body_size: 8192
"#;
        let meta: SkillMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(meta.name, "web-scraping");
        assert_eq!(meta.priority, 80);
        assert_eq!(meta.triggers.patterns.len(), 2);
        assert_eq!(meta.triggers.keywords.len(), 2);
        assert_eq!(meta.triggers.regex.len(), 1);
        assert_eq!(meta.tags, vec!["web", "extraction"]);
        assert_eq!(meta.max_body_size, 8192);
    }

    #[test]
    fn test_validation_valid() {
        let meta = SkillMetadata {
            name: "valid-skill".to_string(),
            description: "A valid skill".to_string(),
            version: "1.0.0".to_string(),
            triggers: TriggerConfig::default(),
            priority: 50,
            always_load: false,
            tags: vec![],
            status: SkillStatus::Enabled,
            author: String::new(),
            max_body_size: 4096,
            created_at: None,
            updated_at: None,
        };
        assert!(meta.is_valid());
        assert!(meta.validate().is_empty());
    }

    #[test]
    fn test_validation_empty_name() {
        let meta = SkillMetadata {
            name: String::new(),
            description: "No name".to_string(),
            version: "1.0.0".to_string(),
            triggers: TriggerConfig::default(),
            priority: 50,
            always_load: false,
            tags: vec![],
            status: SkillStatus::Enabled,
            author: String::new(),
            max_body_size: 4096,
            created_at: None,
            updated_at: None,
        };
        let errors = meta.validate();
        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], ValidationError::EmptyName));
    }

    #[test]
    fn test_validation_invalid_name() {
        let meta = SkillMetadata {
            name: "bad skill name!".to_string(),
            description: "Spaces and special chars".to_string(),
            version: "1.0.0".to_string(),
            triggers: TriggerConfig::default(),
            priority: 50,
            always_load: false,
            tags: vec![],
            status: SkillStatus::Enabled,
            author: String::new(),
            max_body_size: 4096,
            created_at: None,
            updated_at: None,
        };
        assert!(!meta.is_valid());
    }

    #[test]
    fn test_validation_invalid_regex() {
        let meta = SkillMetadata {
            name: "test".to_string(),
            description: String::new(),
            version: "1.0.0".to_string(),
            triggers: TriggerConfig {
                regex: vec!["[invalid".to_string()],
                ..Default::default()
            },
            priority: 50,
            always_load: false,
            tags: vec![],
            status: SkillStatus::Enabled,
            author: String::new(),
            max_body_size: 4096,
            created_at: None,
            updated_at: None,
        };
        let errors = meta.validate();
        assert!(errors.iter().any(|e| matches!(e, ValidationError::InvalidRegex { .. })));
    }

    #[test]
    fn test_trigger_config_has_triggers() {
        let empty = TriggerConfig::default();
        assert!(!empty.has_triggers());

        let with_patterns = TriggerConfig {
            patterns: vec!["http://".to_string()],
            ..Default::default()
        };
        assert!(with_patterns.has_triggers());

        let with_keywords = TriggerConfig {
            keywords: vec!["test".to_string()],
            ..Default::default()
        };
        assert!(with_keywords.has_triggers());
    }

    #[test]
    fn test_skill_status_display() {
        assert_eq!(SkillStatus::Enabled.to_string(), "enabled");
        assert_eq!(SkillStatus::Disabled.to_string(), "disabled");
    }

    #[test]
    fn test_skill_status_serde() {
        let yaml = "\"enabled\"";
        let status: SkillStatus = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(status, SkillStatus::Enabled);

        let yaml = "\"disabled\"";
        let status: SkillStatus = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(status, SkillStatus::Disabled);
    }

    #[test]
    fn test_skill_accessors() {
        let skill = Skill {
            metadata: SkillMetadata {
                name: "test-skill".to_string(),
                description: "A test".to_string(),
                version: "1.0.0".to_string(),
                triggers: TriggerConfig::default(),
                priority: 75,
                always_load: true,
                tags: vec!["test".to_string(), "demo".to_string()],
                status: SkillStatus::Enabled,
                author: String::new(),
                max_body_size: 4096,
                created_at: None,
                updated_at: None,
            },
            body: "# Test Skill\nSome content.".to_string(),
            source_path: Some(PathBuf::from("/skills/test-skill/SKILL.md")),
        };

        assert_eq!(skill.name(), "test-skill");
        assert_eq!(skill.description(), "A test");
        assert_eq!(skill.priority(), 75);
        assert!(skill.always_load());
        assert!(skill.is_enabled());
        assert_eq!(skill.prompt_content(), "# Test Skill\nSome content.");
        assert_eq!(skill.tags(), &["test", "demo"]);
        assert_eq!(skill.dir_name(), Some("test-skill"));
    }
}
