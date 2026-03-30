//! Skill registry — load, cache, and query skills from a directory.
//!
//! The registry scans a skills directory where each skill lives in its own
//! subdirectory with a `SKILL.md` file (or a configured filename).
//!
//! ```text
//! skills/
//! ├── idea-intake/
//! │   └── SKILL.md
//! ├── web-scraping/
//! │   └── SKILL.md
//! └── code-review/
//!     └── SKILL.md
//! ```

use crate::parser::Parser;
use crate::schema::{Skill, SkillMetadata, SkillStatus, TriggerConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Errors from the skill registry.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("skills directory does not exist: {0}")]
    DirNotFound(PathBuf),

    #[error("failed to read skills directory: {0}")]
    ReadDir(#[from] std::io::Error),

    #[error("failed to parse skill '{name}': {source}")]
    ParseError {
        name: String,
        #[source]
        source: crate::parser::ParseError,
    },
}

/// Skill registry — manages loaded skills.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    /// All loaded skills, keyed by name.
    skills: HashMap<String, Skill>,
    /// Skills directory path.
    skills_dir: PathBuf,
    /// The filename to look for in each skill directory.
    skill_filename: String,
    /// How long the last load took.
    load_duration_ms: f64,
}

impl SkillRegistry {
    /// Load skills from a directory.
    ///
    /// Scans `dir/*/SKILL.md` and parses each skill file.
    /// Invalid skills are logged and skipped (non-fatal).
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, RegistryError> {
        Self::load_with_filename(dir, "SKILL.md")
    }

    /// Load skills with a custom filename.
    pub fn load_with_filename(
        dir: impl AsRef<Path>,
        filename: &str,
    ) -> Result<Self, RegistryError> {
        let dir = dir.as_ref();
        let start = Instant::now();

        if !dir.exists() {
            return Err(RegistryError::DirNotFound(dir.to_path_buf()));
        }

        let parser = Parser::new();
        let mut skills = HashMap::new();

        let mut entries: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let skill_file = entry.path().join(filename);
            if !skill_file.exists() {
                continue;
            }

            match parser.parse_file(&skill_file) {
                Ok(mut skill) => {
                    // If the parsed name is a legacy auto-generated one, use dir name instead
                    let dir_name = entry.file_name().to_string_lossy().to_string();
                    if skill.metadata.name.starts_with("skill-")
                        || skill.metadata.name == "unnamed-skill"
                    {
                        skill.metadata.name = dir_name.clone();
                    }
                    skills.insert(skill.metadata.name.clone(), skill);
                }
                Err(e) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    tracing::warn!("Failed to parse skill '{}': {}", name, e);
                    // Non-fatal — skip this skill
                }
            }
        }

        let elapsed = start.elapsed();

        Ok(Self {
            skills,
            skills_dir: dir.to_path_buf(),
            skill_filename: filename.to_string(),
            load_duration_ms: elapsed.as_secs_f64() * 1000.0,
        })
    }

    /// Create an empty registry (no skills directory).
    pub fn empty() -> Self {
        Self {
            skills: HashMap::new(),
            skills_dir: PathBuf::new(),
            skill_filename: "SKILL.md".to_string(),
            load_duration_ms: 0.0,
        }
    }

    /// Reload all skills from disk.
    pub fn reload(&mut self) -> Result<(), RegistryError> {
        let reloaded = Self::load_with_filename(&self.skills_dir, &self.skill_filename)?;
        self.skills = reloaded.skills;
        self.load_duration_ms = reloaded.load_duration_ms;
        Ok(())
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// Get all skills (unordered).
    pub fn all(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    /// Get all enabled skills.
    pub fn enabled(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values().filter(|s| s.is_enabled())
    }

    /// Get skills that should always be loaded into context.
    pub fn always_load_skills(&self) -> Vec<&Skill> {
        let mut skills: Vec<_> = self
            .skills
            .values()
            .filter(|s| s.is_enabled() && s.always_load())
            .collect();
        skills.sort_by(|a, b| b.priority().cmp(&a.priority()));
        skills
    }

    /// Get all skills sorted by priority (highest first).
    pub fn by_priority(&self) -> Vec<&Skill> {
        let mut skills: Vec<_> = self.skills.values().collect();
        skills.sort_by(|a, b| b.priority().cmp(&a.priority()));
        skills
    }

    /// Get skills by tag.
    pub fn by_tag(&self, tag: &str) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|s| s.tags().iter().any(|t| t == tag))
            .collect()
    }

    /// Number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// How long the last load took (milliseconds).
    pub fn load_duration_ms(&self) -> f64 {
        self.load_duration_ms
    }

    /// The skills directory path.
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Enable a skill by name. Returns true if found.
    pub fn enable(&mut self, name: &str) -> bool {
        if let Some(skill) = self.skills.get_mut(name) {
            skill.metadata.status = SkillStatus::Enabled;
            true
        } else {
            false
        }
    }

    /// Disable a skill by name. Returns true if found.
    pub fn disable(&mut self, name: &str) -> bool {
        if let Some(skill) = self.skills.get_mut(name) {
            skill.metadata.status = SkillStatus::Disabled;
            true
        } else {
            false
        }
    }

    /// Persist enable/disable state to disk by rewriting the SKILL.md file.
    pub fn persist_status(&self, name: &str) -> anyhow::Result<()> {
        let skill = self
            .skills
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("skill '{}' not found", name))?;

        let path = skill
            .source_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("skill '{}' has no source path", name))?;

        let content = Parser::serialize_skill(skill)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Create a new skill from a template.
    ///
    /// Creates the skill directory and SKILL.md file.
    /// Returns the path to the created SKILL.md.
    pub fn create_skill(
        &mut self,
        name: &str,
        description: &str,
        tags: Vec<String>,
    ) -> anyhow::Result<PathBuf> {
        // Validate name
        if name.is_empty() {
            anyhow::bail!("skill name cannot be empty");
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!(
                "skill name must be alphanumeric with hyphens/underscores: '{}'",
                name
            );
        }

        // Check if already exists
        if self.skills.contains_key(name) {
            anyhow::bail!("skill '{}' already exists", name);
        }

        let skill_dir = self.skills_dir.join(name);
        let skill_file = skill_dir.join(&self.skill_filename);

        // Create directory
        std::fs::create_dir_all(&skill_dir)?;

        // Build the skill
        let skill = Skill {
            metadata: SkillMetadata {
                name: name.to_string(),
                description: description.to_string(),
                version: "0.1.0".to_string(),
                triggers: TriggerConfig::default(),
                priority: 50,
                always_load: false,
                tags,
                status: SkillStatus::Enabled,
                author: String::new(),
                max_body_size: 4096,
                created_at: Some(chrono::Utc::now()),
                updated_at: None,
            },
            body: format!(
                "# {}\n\n> {}\n\n## When to Use\n\nDescribe trigger conditions here.\n\n## Steps\n\n1. Step one\n2. Step two\n3. Step three\n\n## Notes\n\n- Additional notes here\n",
                name, description
            ),
            source_path: Some(skill_file.clone()),
        };

        // Write to disk
        let content = Parser::serialize_skill(&skill)?;
        std::fs::write(&skill_file, content)?;

        // Add to registry
        self.skills.insert(name.to_string(), skill);

        Ok(skill_file)
    }

    /// Initialize a skills directory (create it if needed).
    pub fn init_skills_dir(dir: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
        let dir = dir.as_ref();
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
        }

        // Create a README if it doesn't exist
        let readme = dir.join("README.md");
        if !readme.exists() {
            std::fs::write(
                &readme,
                r#"# Skills

AI agent skills managed by `skillz`.

Each subdirectory contains a `SKILL.md` file with YAML frontmatter
and markdown instructions.

## Creating a new skill

```bash
skillz new my-skill
```

## Listing skills

```bash
skillz list
```

## Testing triggers

```bash
skillz test my-skill "check https://example.com"
```
"#,
            )?;
        }

        Ok(dir.to_path_buf())
    }

    /// Get formatted skill content for injection into a system prompt.
    /// Respects `max_body_size` and only returns enabled skills.
    pub fn prompt_skills(&self) -> Vec<(&str, String)> {
        let mut result = Vec::new();
        let mut skills: Vec<_> = self.enabled().collect();
        skills.sort_by(|a, b| b.priority().cmp(&a.priority()));

        for skill in skills {
            let content = if skill.metadata.max_body_size > 0
                && skill.body.len() > skill.metadata.max_body_size
            {
                let truncated = truncate_bytes(&skill.body, skill.metadata.max_body_size);
                format!("{}\n...(truncated)...", truncated)
            } else {
                skill.body.clone()
            };
            result.push((skill.name(), content));
        }

        result
    }

    /// Get stats about the registry.
    pub fn stats(&self) -> RegistryStats {
        let total = self.skills.len();
        let enabled = self.skills.values().filter(|s| s.is_enabled()).count();
        let disabled = total - enabled;
        let always_load = self
            .skills
            .values()
            .filter(|s| s.always_load() && s.is_enabled())
            .count();
        let with_triggers = self
            .skills
            .values()
            .filter(|s| s.metadata.triggers.has_triggers())
            .count();
        let total_body_bytes: usize = self.skills.values().map(|s| s.body.len()).sum();

        // Collect all tags
        let mut all_tags: Vec<String> = self
            .skills
            .values()
            .flat_map(|s| s.tags().iter().cloned())
            .collect();
        all_tags.sort();
        all_tags.dedup();

        RegistryStats {
            total,
            enabled,
            disabled,
            always_load,
            with_triggers,
            total_body_bytes,
            tags: all_tags,
            load_duration_ms: self.load_duration_ms,
        }
    }
}

/// Registry statistics.
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total: usize,
    pub enabled: usize,
    pub disabled: usize,
    pub always_load: usize,
    pub with_triggers: usize,
    pub total_body_bytes: usize,
    pub tags: Vec<String>,
    pub load_duration_ms: f64,
}

impl std::fmt::Display for RegistryStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "📊 Skills Registry Stats")?;
        writeln!(f, "  Total:        {}", self.total)?;
        writeln!(f, "  Enabled:      {}", self.enabled)?;
        writeln!(f, "  Disabled:     {}", self.disabled)?;
        writeln!(f, "  Always-load:  {}", self.always_load)?;
        writeln!(f, "  With triggers:{}", self.with_triggers)?;
        writeln!(
            f,
            "  Body size:    {} bytes ({:.1} KB)",
            self.total_body_bytes,
            self.total_body_bytes as f64 / 1024.0
        )?;
        writeln!(f, "  Load time:    {:.2}ms", self.load_duration_ms)?;
        if !self.tags.is_empty() {
            writeln!(f, "  Tags:         {}", self.tags.join(", "))?;
        }
        Ok(())
    }
}

/// Truncate a string to at most `max_bytes` bytes on a char boundary.
fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_skill(dir: &Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_load_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let registry = SkillRegistry::load(tmp.path()).unwrap();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let result = SkillRegistry::load("/nonexistent/path");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RegistryError::DirNotFound(_)));
    }

    #[test]
    fn test_load_skills() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "skill-a",
            "---\nname: skill-a\ndescription: First skill\npriority: 80\n---\n\n# Skill A\nContent A.",
        );
        create_test_skill(
            tmp.path(),
            "skill-b",
            "---\nname: skill-b\ndescription: Second skill\npriority: 60\ntags: [test]\n---\n\n# Skill B\nContent B.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        assert_eq!(registry.len(), 2);
        assert!(registry.get("skill-a").is_some());
        assert!(registry.get("skill-b").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_load_legacy_skills() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "my-legacy",
            "# Legacy Skill\n\n> Does legacy things.\n\nSome content.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        assert_eq!(registry.len(), 1);
        // Should use dir name since auto-generated name starts with "skill-"
        let skill = registry.get("my-legacy").unwrap();
        assert_eq!(skill.name(), "my-legacy");
    }

    #[test]
    fn test_by_priority() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(tmp.path(), "low", "---\nname: low\npriority: 10\n---\nLow");
        create_test_skill(tmp.path(), "high", "---\nname: high\npriority: 90\n---\nHigh");
        create_test_skill(tmp.path(), "mid", "---\nname: mid\npriority: 50\n---\nMid");

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let sorted = registry.by_priority();
        assert_eq!(sorted[0].name(), "high");
        assert_eq!(sorted[1].name(), "mid");
        assert_eq!(sorted[2].name(), "low");
    }

    #[test]
    fn test_always_load() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "always",
            "---\nname: always\nalways_load: true\npriority: 100\n---\nAlways on.",
        );
        create_test_skill(
            tmp.path(),
            "sometimes",
            "---\nname: sometimes\nalways_load: false\n---\nSometimes.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let always = registry.always_load_skills();
        assert_eq!(always.len(), 1);
        assert_eq!(always[0].name(), "always");
    }

    #[test]
    fn test_by_tag() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "web1",
            "---\nname: web1\ntags: [web, api]\n---\nWeb 1",
        );
        create_test_skill(
            tmp.path(),
            "web2",
            "---\nname: web2\ntags: [web, scraping]\n---\nWeb 2",
        );
        create_test_skill(
            tmp.path(),
            "other",
            "---\nname: other\ntags: [cli]\n---\nOther",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let web_skills = registry.by_tag("web");
        assert_eq!(web_skills.len(), 2);

        let cli_skills = registry.by_tag("cli");
        assert_eq!(cli_skills.len(), 1);

        let empty = registry.by_tag("nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_enable_disable() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "toggleable",
            "---\nname: toggleable\n---\nContent.",
        );

        let mut registry = SkillRegistry::load(tmp.path()).unwrap();
        assert!(registry.get("toggleable").unwrap().is_enabled());

        assert!(registry.disable("toggleable"));
        assert!(!registry.get("toggleable").unwrap().is_enabled());
        assert_eq!(registry.enabled().count(), 0);

        assert!(registry.enable("toggleable"));
        assert!(registry.get("toggleable").unwrap().is_enabled());

        assert!(!registry.disable("nonexistent"));
    }

    #[test]
    fn test_create_skill() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let mut registry = SkillRegistry::load(&skills_dir).unwrap();
        assert!(registry.is_empty());

        let path = registry
            .create_skill("new-skill", "A brand new skill", vec!["test".to_string()])
            .unwrap();

        assert!(path.exists());
        assert_eq!(registry.len(), 1);
        assert!(registry.get("new-skill").is_some());

        let skill = registry.get("new-skill").unwrap();
        assert_eq!(skill.description(), "A brand new skill");
        assert_eq!(skill.tags(), &["test"]);

        // Should fail for duplicate
        let result = registry.create_skill("new-skill", "Duplicate", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_skill_validation() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        let mut registry = SkillRegistry::load(&skills_dir).unwrap();

        // Empty name
        assert!(registry.create_skill("", "No name", vec![]).is_err());

        // Invalid chars
        assert!(registry
            .create_skill("bad name!", "Bad", vec![])
            .is_err());
    }

    #[test]
    fn test_init_skills_dir() {
        let tmp = TempDir::new().unwrap();
        let skills_dir = tmp.path().join("new-skills");

        let path = SkillRegistry::init_skills_dir(&skills_dir).unwrap();
        assert!(path.exists());
        assert!(skills_dir.join("README.md").exists());

        // Idempotent
        let path2 = SkillRegistry::init_skills_dir(&skills_dir).unwrap();
        assert_eq!(path, path2);
    }

    #[test]
    fn test_reload() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(tmp.path(), "original", "---\nname: original\n---\nContent.");

        let mut registry = SkillRegistry::load(tmp.path()).unwrap();
        assert_eq!(registry.len(), 1);

        // Add a new skill to disk
        create_test_skill(tmp.path(), "added", "---\nname: added\n---\nNew content.");

        registry.reload().unwrap();
        assert_eq!(registry.len(), 2);
        assert!(registry.get("added").is_some());
    }

    #[test]
    fn test_stats() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "s1",
            "---\nname: s1\ntags: [web]\nalways_load: true\ntriggers:\n  patterns: [\"http\"]\n---\n\nBody 1",
        );
        create_test_skill(
            tmp.path(),
            "s2",
            "---\nname: s2\ntags: [web, cli]\nstatus: disabled\n---\n\nBody 2",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let stats = registry.stats();

        assert_eq!(stats.total, 2);
        assert_eq!(stats.enabled, 1);
        assert_eq!(stats.disabled, 1);
        assert_eq!(stats.always_load, 1);
        assert_eq!(stats.with_triggers, 1);
        assert!(stats.total_body_bytes > 0);
        assert!(stats.tags.contains(&"web".to_string()));
        assert!(stats.tags.contains(&"cli".to_string()));
    }

    #[test]
    fn test_prompt_skills() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "visible",
            "---\nname: visible\npriority: 80\n---\n\nVisible content.",
        );
        create_test_skill(
            tmp.path(),
            "hidden",
            "---\nname: hidden\nstatus: disabled\n---\n\nHidden content.",
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let prompts = registry.prompt_skills();

        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].0, "visible");
        assert!(prompts[0].1.contains("Visible content."));
    }

    #[test]
    fn test_prompt_skills_truncation() {
        let tmp = TempDir::new().unwrap();

        let long_body = "x".repeat(5000);
        create_test_skill(
            tmp.path(),
            "big",
            &format!(
                "---\nname: big\nmax_body_size: 100\n---\n\n{}",
                long_body
            ),
        );

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        let prompts = registry.prompt_skills();

        assert_eq!(prompts.len(), 1);
        // Should be truncated to ~100 bytes + "...(truncated)..."
        assert!(prompts[0].1.len() < 200);
        assert!(prompts[0].1.contains("...(truncated)..."));
    }

    #[test]
    fn test_load_performance() {
        let tmp = TempDir::new().unwrap();

        // Create 10 skills
        for i in 0..10 {
            create_test_skill(
                tmp.path(),
                &format!("skill-{:02}", i),
                &format!(
                    "---\nname: skill-{:02}\ndescription: Skill number {}\ntriggers:\n  keywords: [\"test{}\"]\npriority: {}\ntags: [perf-test]\n---\n\n# Skill {}\n\nThis is skill content number {}. It has some text to make it realistic.\n",
                    i, i, i, i * 10, i, i
                ),
            );
        }

        let registry = SkillRegistry::load(tmp.path()).unwrap();
        assert_eq!(registry.len(), 10);
        // Should be well under 10ms for 10 skills
        assert!(
            registry.load_duration_ms() < 100.0,
            "Load took {:.2}ms, expected <100ms",
            registry.load_duration_ms()
        );
    }

    #[test]
    fn test_truncate_bytes() {
        assert_eq!(truncate_bytes("hello", 10), "hello");
        assert_eq!(truncate_bytes("hello", 3), "hel");
        assert_eq!(truncate_bytes("hello", 0), "");

        // UTF-8 boundary test
        let s = "你好世界"; // Each char is 3 bytes
        assert_eq!(truncate_bytes(s, 6), "你好");
        assert_eq!(truncate_bytes(s, 5), "你"); // Can't split 好, falls back to 3
        assert_eq!(truncate_bytes(s, 3), "你");
    }

    #[test]
    fn test_empty_registry() {
        let registry = SkillRegistry::empty();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.always_load_skills().is_empty());
        assert!(registry.by_priority().is_empty());
    }

    #[test]
    fn test_persist_status() {
        let tmp = TempDir::new().unwrap();

        create_test_skill(
            tmp.path(),
            "persist-test",
            "---\nname: persist-test\ndescription: Test persistence\n---\n\nContent.",
        );

        let mut registry = SkillRegistry::load(tmp.path()).unwrap();
        registry.disable("persist-test");
        registry.persist_status("persist-test").unwrap();

        // Reload and verify
        let registry2 = SkillRegistry::load(tmp.path()).unwrap();
        assert!(!registry2.get("persist-test").unwrap().is_enabled());
    }
}
