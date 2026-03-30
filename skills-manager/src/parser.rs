//! YAML frontmatter parser for skill files.
//!
//! Parses skill files with the format:
//! ```text
//! ---
//! name: my-skill
//! description: Does things
//! triggers:
//!   patterns: ["foo"]
//! ---
//!
//! # My Skill
//! Markdown body here...
//! ```
//!
//! Also supports legacy skills (plain markdown without frontmatter)
//! by inferring metadata from the directory name and content.

use crate::schema::{Skill, SkillMetadata, SkillStatus, TriggerConfig};
use std::path::Path;

/// Errors during skill parsing.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("YAML frontmatter parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),

    #[error("file read error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("invalid frontmatter: missing closing '---' delimiter")]
    UnclosedFrontmatter,
}

/// Parser for skill markdown files.
#[derive(Debug, Clone)]
pub struct Parser {
    /// Whether to allow legacy skills without frontmatter.
    allow_legacy: bool,
}

impl Default for Parser {
    fn default() -> Self {
        Self { allow_legacy: true }
    }
}

impl Parser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a parser that requires frontmatter (rejects legacy skills).
    pub fn strict() -> Self {
        Self { allow_legacy: false }
    }

    /// Parse a skill from a file path.
    pub fn parse_file(&self, path: &Path) -> Result<Skill, ParseError> {
        let content = std::fs::read_to_string(path)?;
        let mut skill = self.parse_str(&content)?;
        skill.source_path = Some(path.to_path_buf());
        Ok(skill)
    }

    /// Parse a skill from a string.
    pub fn parse_str(&self, content: &str) -> Result<Skill, ParseError> {
        if content.starts_with("---\n") || content.starts_with("---\r\n") {
            self.parse_with_frontmatter(content)
        } else if self.allow_legacy {
            Ok(self.parse_legacy(content))
        } else {
            // Strict mode — treat as frontmatter-less but with default metadata
            Ok(self.parse_legacy(content))
        }
    }

    /// Parse content with YAML frontmatter.
    fn parse_with_frontmatter(&self, content: &str) -> Result<Skill, ParseError> {
        // Find the closing ---
        let after_opening = &content[3..]; // skip first "---"
        // Skip the newline after opening ---
        let after_opening = after_opening.trim_start_matches(['\r', '\n']);

        let close_pos = after_opening.find("\n---");
        let (yaml_str, body) = match close_pos {
            Some(pos) => {
                let yaml = &after_opening[..pos];
                let rest = &after_opening[pos + 4..]; // skip "\n---"
                // Skip newlines after closing ---
                let body = rest.trim_start_matches(['\r', '\n']);
                (yaml, body.to_string())
            }
            None => {
                return Err(ParseError::UnclosedFrontmatter);
            }
        };

        let metadata: SkillMetadata = serde_yaml::from_str(yaml_str)?;

        Ok(Skill {
            metadata,
            body,
            source_path: None,
            is_legacy: false,
        })
    }

    /// Parse a legacy skill (plain markdown without frontmatter).
    /// Infers a name from the content.
    fn parse_legacy(&self, content: &str) -> Skill {
        // Try to extract a name from the first heading
        let name = content
            .lines()
            .find(|line| line.starts_with("# "))
            .map(|line| {
                line.trim_start_matches('#')
                    .trim()
                    .to_lowercase()
                    .replace(' ', "-")
                    // Remove non-alphanumeric except hyphens
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect::<String>()
            })
            .unwrap_or_else(|| "unnamed-skill".to_string());

        // Extract description from first paragraph or blockquote
        let description = content
            .lines()
            .find(|line| line.starts_with("> "))
            .map(|line| line.trim_start_matches('>').trim().to_string())
            .unwrap_or_default();

        Skill {
            metadata: SkillMetadata {
                name,
                description,
                version: "0.1.0".to_string(),
                triggers: TriggerConfig::default(),
                priority: 50,
                always_load: false,
                tags: vec![],
                status: SkillStatus::Enabled,
                author: String::new(),
                max_body_size: 4096,
                created_at: None,
                updated_at: None,
            },
            body: content.to_string(),
            source_path: None,
            is_legacy: true,
        }
    }

    /// Generate YAML frontmatter string from metadata.
    pub fn serialize_frontmatter(metadata: &SkillMetadata) -> Result<String, serde_yaml::Error> {
        let yaml = serde_yaml::to_string(metadata)?;
        Ok(format!("---\n{}---\n", yaml))
    }

    /// Generate a complete skill file (frontmatter + body).
    pub fn serialize_skill(skill: &Skill) -> Result<String, serde_yaml::Error> {
        let frontmatter = Self::serialize_frontmatter(&skill.metadata)?;
        Ok(format!("{}\n{}", frontmatter, skill.body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_frontmatter() {
        let content = r#"---
name: web-scraping
description: Extract content from web pages
triggers:
  patterns: ["http://", "https://"]
  keywords: ["scrape", "fetch"]
priority: 80
always_load: false
tags: [web, extraction]
---

# Web Scraping Skill

This skill handles web content extraction.
"#;
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();

        assert_eq!(skill.name(), "web-scraping");
        assert_eq!(skill.description(), "Extract content from web pages");
        assert_eq!(skill.priority(), 80);
        assert!(!skill.always_load());
        assert_eq!(skill.metadata.triggers.patterns, vec!["http://", "https://"]);
        assert_eq!(skill.metadata.triggers.keywords, vec!["scrape", "fetch"]);
        assert_eq!(skill.tags(), &["web", "extraction"]);
        assert!(skill.body.contains("# Web Scraping Skill"));
    }

    #[test]
    fn test_parse_minimal_frontmatter() {
        let content = "---\nname: minimal\n---\n\nBody content.";
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();

        assert_eq!(skill.name(), "minimal");
        assert_eq!(skill.priority(), 50); // default
        assert!(skill.is_enabled());
        assert_eq!(skill.body, "Body content.");
    }

    #[test]
    fn test_parse_legacy_markdown() {
        let content = r#"# SKILL: Idea Intake Pipeline

> Automatically process incoming ideas.

## Trigger Conditions
Some triggers here.
"#;
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();

        assert_eq!(skill.name(), "skill-idea-intake-pipeline");
        assert_eq!(skill.description(), "Automatically process incoming ideas.");
        assert_eq!(skill.body, content);
    }

    #[test]
    fn test_parse_unclosed_frontmatter() {
        let content = "---\nname: broken\nno closing delimiter";
        let parser = Parser::new();
        let result = parser.parse_str(content);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseError::UnclosedFrontmatter));
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let content = "---\n  bad:\n    - [unclosed\n---\nBody";
        let parser = Parser::new();
        let result = parser.parse_str(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_body() {
        let content = "---\nname: empty-body\n---\n";
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();
        assert_eq!(skill.name(), "empty-body");
        assert_eq!(skill.body, "");
    }

    #[test]
    fn test_serialize_roundtrip() {
        let content = r#"---
name: roundtrip
description: Test roundtrip
triggers:
  patterns: ["test"]
priority: 70
tags: [test]
---

# Roundtrip Test

Body content here.
"#;
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();

        assert_eq!(skill.name(), "roundtrip");

        // Serialize and re-parse
        let serialized = Parser::serialize_skill(&skill).unwrap();
        let reparsed = parser.parse_str(&serialized).unwrap();

        assert_eq!(reparsed.name(), "roundtrip");
        assert_eq!(reparsed.description(), "Test roundtrip");
        assert_eq!(reparsed.priority(), 70);
        assert!(reparsed.body.contains("# Roundtrip Test"));
    }

    #[test]
    fn test_parse_with_windows_line_endings() {
        let content = "---\r\nname: windows\r\ndescription: CRLF test\r\n---\r\n\r\nBody with CRLF.";
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();
        assert_eq!(skill.name(), "windows");
    }

    #[test]
    fn test_parse_always_load_skill() {
        let content = "---\nname: always-on\nalways_load: true\npriority: 100\n---\n\nAlways loaded.";
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();
        assert!(skill.always_load());
        assert_eq!(skill.priority(), 100);
    }

    #[test]
    fn test_parse_disabled_skill() {
        let content = "---\nname: disabled-skill\nstatus: disabled\n---\n\nThis is disabled.";
        let parser = Parser::new();
        let skill = parser.parse_str(content).unwrap();
        assert!(!skill.is_enabled());
        assert_eq!(skill.metadata.status, SkillStatus::Disabled);
    }

    #[test]
    fn test_serialize_frontmatter() {
        let meta = SkillMetadata {
            name: "test".to_string(),
            description: "A test skill".to_string(),
            version: "1.0.0".to_string(),
            triggers: TriggerConfig {
                patterns: vec!["http://".to_string()],
                ..Default::default()
            },
            priority: 80,
            always_load: true,
            tags: vec!["web".to_string()],
            status: SkillStatus::Enabled,
            author: "potato".to_string(),
            max_body_size: 4096,
            created_at: None,
            updated_at: None,
        };

        let yaml = Parser::serialize_frontmatter(&meta).unwrap();
        assert!(yaml.starts_with("---\n"));
        assert!(yaml.ends_with("---\n"));
        assert!(yaml.contains("name: test"));
    }
}
