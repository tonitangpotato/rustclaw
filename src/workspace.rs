//! Workspace file loading.
//!
//! Compatible with OpenClaw workspace format:
//! - SOUL.md — agent personality
//! - AGENTS.md — workspace conventions
//! - USER.md — info about the human
//! - TOOLS.md — local tool notes
//! - HEARTBEAT.md — heartbeat checklist
//! - MEMORY.md — long-term memory
//! - IDENTITY.md — agent identity
//! - BOOTSTRAP.md — first-run setup

use chrono::Local;
use std::path::{Path, PathBuf};

/// Metadata parsed from SKILL.md frontmatter.
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub priority: u8,
    pub always_load: bool,
    pub max_context_bytes: usize,
}

impl Default for SkillMetadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            triggers: Vec::new(),
            priority: 100,
            always_load: false,
            max_context_bytes: 4096,
        }
    }
}

/// Parse YAML frontmatter from a SKILL.md file.
///
/// Frontmatter is delimited by `---` at the start of the file.
/// Returns (metadata, content_without_frontmatter).
/// If no frontmatter is found, returns defaults and the full content.
pub fn parse_skill_frontmatter(content: &str, fallback_name: &str) -> (SkillMetadata, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (
            SkillMetadata {
                name: fallback_name.to_string(),
                ..Default::default()
            },
            content.to_string(),
        );
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    // Skip the newline after the opening ---
    let after_first = after_first.strip_prefix('\n').or_else(|| after_first.strip_prefix("\r\n")).unwrap_or(after_first);

    let Some(end_idx) = after_first.find("\n---") else {
        // No closing delimiter — treat entire content as body (no frontmatter)
        return (
            SkillMetadata {
                name: fallback_name.to_string(),
                ..Default::default()
            },
            content.to_string(),
        );
    };

    let yaml_str = &after_first[..end_idx];
    let rest_start = end_idx + 4; // skip \n---
    let body = if rest_start < after_first.len() {
        let rest = &after_first[rest_start..];
        // Strip leading newline from body
        rest.strip_prefix('\n')
            .or_else(|| rest.strip_prefix("\r\n"))
            .unwrap_or(rest)
    } else {
        ""
    };

    let mut meta = SkillMetadata {
        name: fallback_name.to_string(),
        ..Default::default()
    };

    // Simple YAML parsing — no serde_yaml dependency needed
    for line in yaml_str.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(val) = line.strip_prefix("name:") {
            meta.name = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("description:") {
            meta.description = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("priority:") {
            if let Ok(p) = val.trim().parse::<u8>() {
                meta.priority = p;
            }
        } else if let Some(val) = line.strip_prefix("always_load:") {
            meta.always_load = val.trim() == "true";
        } else if let Some(val) = line.strip_prefix("max_context_bytes:") {
            if let Ok(n) = val.trim().parse::<usize>() {
                meta.max_context_bytes = n;
            }
        } else if line.starts_with("triggers:") {
            // triggers is a YAML list — parsed from subsequent `  - value` lines
            // handled below
        } else if let Some(val) = line.strip_prefix("- ") {
            // This is a list item — belongs to the last list key (triggers)
            meta.triggers.push(val.trim().to_string());
        }
    }

    (meta, body.to_string())
}

/// Workspace context loaded from markdown files.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub soul: Option<String>,
    pub agents: Option<String>,
    pub user: Option<String>,
    pub tools: Option<String>,
    pub heartbeat: Option<String>,
    pub memory: Option<String>,
    pub identity: Option<String>,
    pub bootstrap: Option<String>,
    /// Current LLM model name (set after construction).
    pub model: Option<String>,
    /// Loaded skills from skills/*/SKILL.md: (dir_name, content_without_frontmatter, metadata)
    pub skills: Vec<(String, String, SkillMetadata)>,
}

impl Workspace {
    /// Load workspace files from a directory.
    pub fn load(dir: &str) -> anyhow::Result<Self> {
        let root = Path::new(dir).to_path_buf();

        // Load skills from skills/ directory, parsing frontmatter metadata
        let mut skills = Vec::new();
        let skills_dir = root.join("skills");
        if skills_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                entries.sort_by_key(|e| e.file_name());
                for entry in entries {
                    if entry.path().is_dir() {
                        let skill_file = entry.path().join("SKILL.md");
                        if let Ok(content) = std::fs::read_to_string(&skill_file) {
                            let dir_name = entry.file_name().to_string_lossy().to_string();
                            let (metadata, body) = parse_skill_frontmatter(&content, &dir_name);
                            skills.push((dir_name, body, metadata));
                        }
                    }
                }
            }
        }

        Ok(Self {
            soul: Self::read_optional(&root, "SOUL.md"),
            agents: Self::read_optional(&root, "AGENTS.md"),
            user: Self::read_optional(&root, "USER.md"),
            tools: Self::read_optional(&root, "TOOLS.md"),
            heartbeat: Self::read_optional(&root, "HEARTBEAT.md"),
            memory: Self::read_optional(&root, "MEMORY.md"),
            identity: Self::read_optional(&root, "IDENTITY.md"),
            bootstrap: Self::read_optional(&root, "BOOTSTRAP.md"),
            model: None,
            skills,
            root,
        })
    }

    /// Get the agent's display name from IDENTITY.md.
    pub fn identity_name(&self) -> Option<&str> {
        self.identity.as_ref().and_then(|content| {
            content
                .lines()
                .find(|line| line.starts_with("- **Name:**"))
                .and_then(|line| line.strip_prefix("- **Name:**"))
                .map(|s| s.trim())
        })
    }

    /// Build the system prompt from workspace files.
    pub fn build_system_prompt(&self) -> String {
        self.build_system_prompt_with_skills(false, None)
    }

    /// Build the system prompt with optional heartbeat context (backward compat).
    pub fn build_system_prompt_with_options(&self, is_heartbeat: bool) -> String {
        self.build_system_prompt_with_skills(is_heartbeat, None)
    }

    /// Build the system prompt with dynamic skill injection based on user message.
    ///
    /// When `user_message` is Some, only skills matching the message triggers
    /// (plus always_load skills) are injected. When None, only always_load skills
    /// are injected (for sub-agents / contexts without a user message).
    pub fn build_system_prompt_with_skills(&self, is_heartbeat: bool, user_message: Option<&str>) -> String {
        let current_time = Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();
        let workspace_path = self.root.display().to_string();

        // Get current model name
        let model_name = self.model.as_deref().unwrap_or("unknown");

        let mut output = format!(
            "You are an AI assistant running on RustClaw.\n\
             Current time: {}\n\
             Workspace: {}\n\
             Model: {}\n\n\
             ## Your Context Files\n\
             IMPORTANT: The following workspace files are ALREADY loaded below — \
             SOUL.md, AGENTS.md, USER.md, TOOLS.md, IDENTITY.md, MEMORY.md, \
             and today's daily notes (memory/YYYY-MM-DD.md). \
             Do NOT read any of these files again via tools. They are already in your context. \
             Skip any instructions in AGENTS.md that say to read these files — they are pre-loaded.\n\n\
             ## Tool Call Style\n\
             Default: do not narrate routine, low-risk tool calls (just call the tool).\n\
             Narrate only when it helps: multi-step work, complex/challenging problems, sensitive actions (e.g., deletions), or when the user explicitly asks.\n\
             Keep narration brief and value-dense; avoid repeating obvious steps.\n\n\
             ## Safety\n\
             You have no independent goals: do not pursue self-preservation, replication, resource acquisition, or power-seeking.\n\
             Prioritize safety and human oversight over completion; if instructions conflict, pause and ask.\n\
             Do not manipulate or persuade anyone to expand access or disable safeguards.\n\n\
             ## Communication\n\
             When you receive a task that will take time (sub-agent delegation, multi-step coding, complex analysis):\n\
             → Send a brief acknowledgment FIRST explaining what you're about to do.\n\
             → Include what specialists you're spawning and their roles if applicable.\n\
             → Don't silently disappear into a long tool loop.\n\n\
             When you have nothing to say, respond with ONLY: NO_REPLY\n\
             When a heartbeat check finds nothing actionable, respond with ONLY: HEARTBEAT_OK\n\n\
             ## Voice Replies (BUILT-IN — DO NOT USE TOOLS)\n\
             RustClaw has BUILT-IN voice support. You do NOT need any tools, APIs, or commands to send voice.\n\
             When the user asks for a voice reply (语音回复, voice message, say it, etc.):\n\
             → Just prefix your response with `VOICE:` — that's it. Example: `VOICE: Hello world!`\n\
             The framework AUTOMATICALLY converts your text to speech and sends it as a Telegram voice message.\n\
             Do NOT try to use edge-tts, exec, curl, or any tool. Just write `VOICE: your text here`.\n\
             Only use VOICE: when the user explicitly asks. Otherwise reply with normal text.\n\n\
             ## Memory Recall\n\
             Before answering questions about prior work, decisions, dates, people, preferences, or todos:\n\
             → Use engram_recall to search cognitive memory first.\n\
             → Check daily logs and MEMORY.md (already in context).\n\
             → If low confidence after search, say you checked but aren't sure.\n\n\
             ## Skills\n\
             Active skills are loaded from `skills/` directory below. Follow their SKILL.md instructions when the task matches.\n",
            current_time, workspace_path, model_name
        );

        if let Some(soul) = &self.soul {
            output.push_str("\n### SOUL.md\n");
            output.push_str(soul);
            output.push_str("\n");
        }
        if let Some(agents) = &self.agents {
            output.push_str("\n### AGENTS.md\n");
            output.push_str(agents);
            output.push_str("\n");
        }
        if let Some(user) = &self.user {
            output.push_str("\n### USER.md\n");
            output.push_str(user);
            output.push_str("\n");
        }
        if let Some(tools) = &self.tools {
            output.push_str("\n### TOOLS.md\n");
            output.push_str(tools);
            output.push_str("\n");
        }
        if let Some(identity) = &self.identity {
            output.push_str("\n### IDENTITY.md\n");
            output.push_str(identity);
            output.push_str("\n");
        }

        // Include HEARTBEAT.md content during heartbeat polls
        if is_heartbeat {
            if let Some(heartbeat) = &self.heartbeat {
                output.push_str("\n### HEARTBEAT.md\n");
                output.push_str(heartbeat);
                output.push_str("\n");
            }
        }

        // Include MEMORY.md in system prompt (truncated to avoid huge context)
        if let Some(memory) = &self.memory {
            output.push_str("\n### MEMORY.md\n");
            // Truncate to ~8KB to keep context manageable
            if memory.len() > 8192 {
                output.push_str(crate::text_utils::truncate_bytes(memory, 8192));
                output.push_str("\n\n...(truncated, use read_file for full MEMORY.md)...\n");
            } else {
                output.push_str(memory);
            }
            output.push_str("\n");
        }

        // Include today's daily notes if they exist
        let today = Local::now().format("%Y-%m-%d").to_string();
        let daily_path = self.root.join("memory").join(format!("{}.md", today));
        if let Ok(daily) = std::fs::read_to_string(&daily_path) {
            output.push_str(&format!("\n### memory/{}.md (today)\n", today));
            if daily.len() > 4096 {
                output.push_str(crate::text_utils::truncate_bytes(&daily, 4096));
                output.push_str("\n\n...(truncated)...\n");
            } else {
                output.push_str(&daily);
            }
            output.push_str("\n");
        }

        // Dynamically inject matching skills based on user message
        let matched_skills = self.match_skills(user_message.unwrap_or(""), 5);
        if !matched_skills.is_empty() {
            output.push_str("\n## Active Skills\n");
            output.push_str("These skills define automated workflows. Follow them when trigger conditions match.\n\n");
            for (name, content, meta) in &matched_skills {
                output.push_str(&format!("### skills/{}/SKILL.md\n", name));
                let max_bytes = meta.max_context_bytes;
                if content.len() > max_bytes {
                    output.push_str(crate::text_utils::truncate_bytes(content, max_bytes));
                    output.push_str("\n...(truncated)...\n");
                } else {
                    output.push_str(content);
                }
                output.push_str("\n\n");
            }
        }

        output
    }

    /// Match skills against a user message. Returns matched skills sorted by priority.
    ///
    /// - Skills with `always_load: true` are always included.
    /// - Otherwise, checks if any trigger keyword appears in the message (case-insensitive substring).
    /// - Results are sorted by priority (lower = higher priority).
    /// - Returns at most `max_skills` results.
    pub fn match_skills(&self, user_message: &str, max_skills: usize) -> Vec<(String, String, SkillMetadata)> {
        let msg_lower = user_message.to_lowercase();

        let mut matched: Vec<(String, String, SkillMetadata)> = self
            .skills
            .iter()
            .filter(|(_, _, meta)| {
                if meta.always_load {
                    return true;
                }
                meta.triggers.iter().any(|trigger| {
                    msg_lower.contains(&trigger.to_lowercase())
                })
            })
            .cloned()
            .collect();

        // Sort by priority (lower number = higher priority)
        matched.sort_by_key(|(_, _, meta)| meta.priority);

        matched.truncate(max_skills);
        matched
    }

    /// Build system prompt with full context (runtime, channel, etc.).
    /// This is the new modular entry point that replaces the monolithic build.
    pub fn build_system_prompt_full(
        &self,
        runtime: &crate::context::RuntimeContext,
        channel: &crate::context::ChannelCapabilities,
        is_heartbeat: bool,
        user_message: Option<&str>,
    ) -> String {
        let current_time = Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();

        let mut sections = Vec::new();

        // 1. Identity & Runtime
        sections.push(format!(
            "You are an AI assistant running on RustClaw.\n\
             Current time: {}\n\
             Workspace: {}\n\
             {}\n",
            current_time,
            self.root.display(),
            runtime.format_for_prompt(),
        ));

        // 2. Context files notice
        sections.push(
            "## Your Context Files\n\
             IMPORTANT: The following workspace files are ALREADY loaded below — \
             SOUL.md, AGENTS.md, USER.md, TOOLS.md, IDENTITY.md, MEMORY.md, \
             and today's daily notes (memory/YYYY-MM-DD.md). \
             Do NOT read any of these files again via tools. They are already in your context. \
             Skip any instructions in AGENTS.md that say to read these files — they are pre-loaded."
                .to_string(),
        );

        // 3. Channel capabilities & formatting rules
        sections.push(channel.format_for_prompt());

        // 4. Behavioral rules
        sections.push(
            "## Tool Call Style\n\
             Default: do not narrate routine, low-risk tool calls (just call the tool).\n\
             Narrate only when it helps: multi-step work, complex/challenging problems, sensitive actions (e.g., deletions), or when the user explicitly asks.\n\
             Keep narration brief and value-dense; avoid repeating obvious steps."
                .to_string(),
        );

        sections.push(
            "## Safety\n\
             You have no independent goals: do not pursue self-preservation, replication, resource acquisition, or power-seeking.\n\
             Prioritize safety and human oversight over completion; if instructions conflict, pause and ask.\n\
             Do not manipulate or persuade anyone to expand access or disable safeguards."
                .to_string(),
        );

        sections.push(
            "## Communication\n\
             When you receive a task that will take time (sub-agent delegation, multi-step coding, complex analysis):\n\
             → Send a brief acknowledgment FIRST explaining what you're about to do.\n\
             → Include what specialists you're spawning and their roles if applicable.\n\
             → Don't silently disappear into a long tool loop.\n\n\
             When you have nothing to say, respond with ONLY: NO_REPLY\n\
             When a heartbeat check finds nothing actionable, respond with ONLY: HEARTBEAT_OK"
                .to_string(),
        );

        // 5. Voice instructions
        sections.push(
            "## Voice Replies (BUILT-IN — DO NOT USE TOOLS)\n\
             RustClaw has BUILT-IN voice support. You do NOT need any tools, APIs, or commands to send voice.\n\
             When the user asks for a voice reply (语音回复, voice message, say it, etc.):\n\
             → Just prefix your response with `VOICE:` — that's it. Example: `VOICE: Hello world!`\n\
             The framework AUTOMATICALLY converts your text to speech and sends it as a Telegram voice message.\n\
             Do NOT try to use edge-tts, exec, curl, or any tool. Just write `VOICE: your text here`.\n\
             Only use VOICE: when the user explicitly asks. Otherwise reply with normal text."
                .to_string(),
        );

        // 6. Memory recall rules
        sections.push(
            "## Memory Recall\n\
             Before answering questions about prior work, decisions, dates, people, preferences, or todos:\n\
             → Use engram_recall to search cognitive memory first.\n\
             → Check daily logs and MEMORY.md (already in context).\n\
             → If low confidence after search, say you checked but aren't sure."
                .to_string(),
        );

        // 7. Skills notice
        sections.push(
            "## Skills\n\
             Active skills are loaded from `skills/` directory below. Follow their SKILL.md instructions when the task matches."
                .to_string(),
        );

        // 8. Workspace files
        let mut ws = String::new();
        if let Some(soul) = &self.soul {
            ws.push_str("\n### SOUL.md\n");
            ws.push_str(soul);
            ws.push('\n');
        }
        if let Some(agents) = &self.agents {
            ws.push_str("\n### AGENTS.md\n");
            ws.push_str(agents);
            ws.push('\n');
        }
        if let Some(user) = &self.user {
            ws.push_str("\n### USER.md\n");
            ws.push_str(user);
            ws.push('\n');
        }
        if let Some(tools) = &self.tools {
            ws.push_str("\n### TOOLS.md\n");
            ws.push_str(tools);
            ws.push('\n');
        }
        if let Some(identity) = &self.identity {
            ws.push_str("\n### IDENTITY.md\n");
            ws.push_str(identity);
            ws.push('\n');
        }
        if is_heartbeat {
            if let Some(heartbeat) = &self.heartbeat {
                ws.push_str("\n### HEARTBEAT.md\n");
                ws.push_str(heartbeat);
                ws.push('\n');
            }
        }
        if let Some(memory) = &self.memory {
            ws.push_str("\n### MEMORY.md\n");
            if memory.len() > 8192 {
                ws.push_str(crate::text_utils::truncate_bytes(memory, 8192));
                ws.push_str("\n\n...(truncated, use read_file for full MEMORY.md)...\n");
            } else {
                ws.push_str(memory);
            }
            ws.push('\n');
        }
        sections.push(ws);

        // 9. Daily notes (today + yesterday)
        let mut daily = String::new();
        let today = Local::now().format("%Y-%m-%d").to_string();
        let yesterday = (Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();

        for (label, date) in [("today", &today), ("yesterday", &yesterday)] {
            let path = self.root.join("memory").join(format!("{}.md", date));
            if let Ok(content) = std::fs::read_to_string(&path) {
                daily.push_str(&format!("\n### memory/{}.md ({})\n", date, label));
                daily.push_str(&content);
                daily.push('\n');
            }
        }
        if !daily.is_empty() {
            sections.push(daily);
        }

        // 10. Matched skills
        let matched_skills = self.match_skills(user_message.unwrap_or(""), 5);
        if !matched_skills.is_empty() {
            let mut skills_section = "## Active Skills\nThese skills define automated workflows. Follow them when trigger conditions match.\n\n".to_string();
            for (name, content, meta) in &matched_skills {
                skills_section.push_str(&format!("### skills/{}/SKILL.md\n", name));
                let max_bytes = meta.max_context_bytes;
                if content.len() > max_bytes {
                    skills_section
                        .push_str(crate::text_utils::truncate_bytes(content, max_bytes));
                    skills_section.push_str("\n...(truncated)...\n");
                } else {
                    skills_section.push_str(content);
                }
                skills_section.push_str("\n\n");
            }
            sections.push(skills_section);
        }

        sections.join("\n\n")
    }

    /// Read a file if it exists, return None otherwise.
    fn read_optional(root: &Path, filename: &str) -> Option<String> {
        let path = root.join(filename);
        std::fs::read_to_string(&path).ok()
    }

    /// Get path to memory directory.
    pub fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_skill_frontmatter tests ──────────────────────────────

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = r#"---
name: Test Skill
description: A test skill for unit testing
triggers:
  - hello
  - world
  - https://
priority: 42
always_load: false
max_context_bytes: 2048
---
# Body Content

This is the skill body.
"#;
        let (meta, body) = parse_skill_frontmatter(content, "fallback");
        assert_eq!(meta.name, "Test Skill");
        assert_eq!(meta.description, "A test skill for unit testing");
        assert_eq!(meta.triggers, vec!["hello", "world", "https://"]);
        assert_eq!(meta.priority, 42);
        assert!(!meta.always_load);
        assert_eq!(meta.max_context_bytes, 2048);
        assert!(body.starts_with("# Body Content"));
        assert!(body.contains("This is the skill body."));
    }

    #[test]
    fn test_parse_frontmatter_none() {
        let content = "# Just a Skill\n\nNo frontmatter here.\n";
        let (meta, body) = parse_skill_frontmatter(content, "my-skill");
        assert_eq!(meta.name, "my-skill");
        assert!(meta.description.is_empty());
        assert!(meta.triggers.is_empty());
        assert_eq!(meta.priority, 100);
        assert!(!meta.always_load);
        assert_eq!(meta.max_context_bytes, 4096);
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_partial() {
        let content = r#"---
name: Partial Skill
priority: 10
---
Body here.
"#;
        let (meta, body) = parse_skill_frontmatter(content, "fallback");
        assert_eq!(meta.name, "Partial Skill");
        assert!(meta.description.is_empty());
        assert!(meta.triggers.is_empty());
        assert_eq!(meta.priority, 10);
        assert!(!meta.always_load);
        assert_eq!(meta.max_context_bytes, 4096);
        assert_eq!(body.trim(), "Body here.");
    }

    #[test]
    fn test_parse_frontmatter_always_load() {
        let content = r#"---
name: Always On
always_load: true
---
Content.
"#;
        let (meta, body) = parse_skill_frontmatter(content, "fallback");
        assert_eq!(meta.name, "Always On");
        assert!(meta.always_load);
        assert_eq!(body.trim(), "Content.");
    }

    #[test]
    fn test_parse_frontmatter_no_closing_delimiter() {
        let content = "---\nname: Broken\npriority: 5\n";
        let (meta, body) = parse_skill_frontmatter(content, "fallback");
        // No closing --- → treated as no frontmatter
        assert_eq!(meta.name, "fallback");
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_frontmatter_empty_body() {
        let content = "---\nname: No Body\n---\n";
        let (meta, body) = parse_skill_frontmatter(content, "fallback");
        assert_eq!(meta.name, "No Body");
        assert!(body.is_empty() || body.trim().is_empty());
    }

    // ── Helper: build a test Workspace with skills ─────────────────

    fn make_workspace(skills: Vec<(String, String, SkillMetadata)>) -> Workspace {
        Workspace {
            root: PathBuf::from("/tmp/test-workspace"),
            soul: None,
            agents: None,
            user: None,
            tools: None,
            heartbeat: None,
            memory: None,
            identity: None,
            bootstrap: None,
            model: None,
            skills,
        }
    }

    fn make_skill(name: &str, triggers: &[&str], priority: u8, always_load: bool) -> (String, String, SkillMetadata) {
        (
            name.to_string(),
            format!("# Skill: {}", name),
            SkillMetadata {
                name: name.to_string(),
                description: format!("Desc for {}", name),
                triggers: triggers.iter().map(|s| s.to_string()).collect(),
                priority,
                always_load,
                max_context_bytes: 4096,
            },
        )
    }

    // ── match_skills tests ─────────────────────────────────────────

    #[test]
    fn test_match_skills_trigger_hit() {
        let ws = make_workspace(vec![
            make_skill("idea-intake", &["http://", "https://", "idea:"], 50, false),
            make_skill("code-review", &["review", "pr", "diff"], 60, false),
        ]);

        let matched = ws.match_skills("check out https://example.com", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].2.name, "idea-intake");
    }

    #[test]
    fn test_match_skills_no_match() {
        let ws = make_workspace(vec![
            make_skill("idea-intake", &["http://", "https://", "idea:"], 50, false),
            make_skill("code-review", &["review", "pr", "diff"], 60, false),
        ]);

        let matched = ws.match_skills("what is the weather today?", 5);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_skills_always_load() {
        let ws = make_workspace(vec![
            make_skill("core-rules", &[], 10, true),
            make_skill("idea-intake", &["http://"], 50, false),
        ]);

        // Even with no matching triggers, always_load skill is included
        let matched = ws.match_skills("random message", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].2.name, "core-rules");
    }

    #[test]
    fn test_match_skills_always_load_plus_trigger() {
        let ws = make_workspace(vec![
            make_skill("core-rules", &[], 10, true),
            make_skill("idea-intake", &["http://"], 50, false),
        ]);

        let matched = ws.match_skills("look at http://example.com", 5);
        assert_eq!(matched.len(), 2);
        // Sorted by priority: core-rules (10) before idea-intake (50)
        assert_eq!(matched[0].2.name, "core-rules");
        assert_eq!(matched[1].2.name, "idea-intake");
    }

    #[test]
    fn test_match_skills_priority_sorting() {
        let ws = make_workspace(vec![
            make_skill("low-prio", &["test"], 200, false),
            make_skill("high-prio", &["test"], 10, false),
            make_skill("med-prio", &["test"], 100, false),
        ]);

        let matched = ws.match_skills("this is a test", 5);
        assert_eq!(matched.len(), 3);
        assert_eq!(matched[0].2.name, "high-prio");
        assert_eq!(matched[1].2.name, "med-prio");
        assert_eq!(matched[2].2.name, "low-prio");
    }

    #[test]
    fn test_match_skills_max_limit() {
        let ws = make_workspace(vec![
            make_skill("skill-a", &["test"], 10, false),
            make_skill("skill-b", &["test"], 20, false),
            make_skill("skill-c", &["test"], 30, false),
            make_skill("skill-d", &["test"], 40, false),
            make_skill("skill-e", &["test"], 50, false),
        ]);

        let matched = ws.match_skills("test message", 3);
        assert_eq!(matched.len(), 3);
        assert_eq!(matched[0].2.name, "skill-a");
        assert_eq!(matched[1].2.name, "skill-b");
        assert_eq!(matched[2].2.name, "skill-c");
    }

    #[test]
    fn test_match_skills_case_insensitive() {
        let ws = make_workspace(vec![
            make_skill("idea-intake", &["HTTPS://", "Idea:"], 50, false),
        ]);

        let matched = ws.match_skills("check https://example.com", 5);
        assert_eq!(matched.len(), 1);

        let matched2 = ws.match_skills("IDEA: something cool", 5);
        assert_eq!(matched2.len(), 1);
    }

    #[test]
    fn test_match_skills_chinese_triggers() {
        let ws = make_workspace(vec![
            make_skill("idea-intake", &["想法:", "记录一下"], 50, false),
        ]);

        let matched = ws.match_skills("想法: 做一个新项目", 5);
        assert_eq!(matched.len(), 1);

        let matched2 = ws.match_skills("帮我记录一下这个想法", 5);
        assert_eq!(matched2.len(), 1);
    }

    #[test]
    fn test_match_skills_empty_message() {
        let ws = make_workspace(vec![
            make_skill("always", &[], 10, true),
            make_skill("triggered", &["hello"], 50, false),
        ]);

        let matched = ws.match_skills("", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].2.name, "always");
    }
}
