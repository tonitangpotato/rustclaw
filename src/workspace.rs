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
use regex::Regex;
use serde::Deserialize;
use skm_core::SkillName;
use skm_select::{SelectionContext, SelectionStrategy, TriggerStrategy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A matched skill ready for system prompt injection.
#[derive(Debug, Clone)]
pub struct MatchedSkill {
    /// Directory name (or skill name if no source path).
    pub dir_name: String,
    /// Prompt content (markdown body without frontmatter).
    pub content: String,
    /// Maximum bytes to inject into prompt.
    pub max_context_bytes: usize,
}

// ============================================================================
// Skill parsing types (compatible with skills-manager format)
// ============================================================================

/// Trigger configuration from YAML frontmatter.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TriggerConfig {
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub regex: Vec<String>,
    #[serde(default)]
    pub globs: Vec<String>,
}

impl TriggerConfig {
    pub fn has_triggers(&self) -> bool {
        !self.patterns.is_empty()
            || !self.keywords.is_empty()
            || !self.regex.is_empty()
            || !self.globs.is_empty()
    }

    /// Flatten all triggers into a single Vec for SKM compatibility.
    /// SKM detects regex patterns by looking for ^, $, or | characters.
    pub fn flatten(&self) -> Vec<String> {
        let mut triggers = Vec::new();

        // Patterns and keywords as-is (they become keywords in TriggerStrategy)
        triggers.extend(self.patterns.iter().cloned());
        triggers.extend(self.keywords.iter().cloned());

        // Regex patterns - SKM detects these by ^, $, or | chars
        // Since SKM uses is_match (which matches anywhere), we wrap patterns
        // that don't have markers in a non-capturing group with empty alternative: (?:...|)
        // This signals regex mode while still matching anywhere in the string
        for re in &self.regex {
            if re.starts_with('^') || re.contains('$') || re.contains('|') {
                triggers.push(re.clone());
            } else {
                // Wrap in alternation with empty string to signal regex to SKM
                // (?:pattern|) matches the pattern OR empty string (always matches somewhere)
                // But we want to match only the pattern, so use: pattern|(?=NEVER_MATCH)
                // Actually simpler: just prepend .* and wrap with |NOMATCH
                // SKM uses .is_match() which finds matches anywhere
                // Just add |$ at end - this signals regex and $ matches end of string
                triggers.push(format!("{}|$", re));
            }
        }

        // Globs - convert to regex patterns (these need anchoring)
        for glob in &self.globs {
            // Simple glob to regex: * -> .*, ? -> .
            let re = glob
                .replace('.', r"\.")
                .replace('*', ".*")
                .replace('?', ".");
            triggers.push(format!("^{}$", re));
        }

        triggers
    }
}

/// Skill metadata from YAML frontmatter.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub triggers: TriggerConfig,
    #[serde(default = "default_priority")]
    pub priority: u8,
    #[serde(default)]
    pub always_load: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,
}

fn default_priority() -> u8 {
    50
}

fn default_max_body_size() -> usize {
    4096
}

/// A parsed skill with metadata and content.
#[derive(Debug, Clone)]
pub struct ParsedSkill {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub source_path: PathBuf,
}

impl ParsedSkill {
    pub fn name(&self) -> &str {
        &self.frontmatter.name
    }

    pub fn dir_name(&self) -> &str {
        self.source_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(&self.frontmatter.name)
    }

    pub fn prompt_content(&self) -> &str {
        &self.body
    }
}

/// Skill registry - loads and caches skills from a directory.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    skills: HashMap<String, ParsedSkill>,
    skills_dir: PathBuf,
    /// Compiled regex patterns for custom matching.
    compiled_regex: HashMap<String, Regex>,
}

impl SkillRegistry {
    /// Load skills from a directory (scans */SKILL.md).
    pub fn load(dir: &Path) -> Result<Self, std::io::Error> {
        let mut skills = HashMap::new();
        let mut compiled_regex = HashMap::new();

        if !dir.exists() {
            return Ok(Self {
                skills,
                skills_dir: dir.to_path_buf(),
                compiled_regex,
            });
        }

        let entries = std::fs::read_dir(dir)?;
        for entry in entries.flatten() {
            let skill_file = entry.path().join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }

            match Self::parse_skill_file(&skill_file) {
                Ok(skill) => {
                    // Pre-compile regex patterns
                    for pattern in &skill.frontmatter.triggers.regex {
                        if let Ok(re) = Regex::new(pattern) {
                            compiled_regex.insert(pattern.clone(), re);
                        }
                    }
                    skills.insert(skill.frontmatter.name.clone(), skill);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse skill {:?}: {}", skill_file, e);
                }
            }
        }

        Ok(Self {
            skills,
            skills_dir: dir.to_path_buf(),
            compiled_regex,
        })
    }

    /// Create an empty registry.
    pub fn empty() -> Self {
        Self {
            skills: HashMap::new(),
            skills_dir: PathBuf::new(),
            compiled_regex: HashMap::new(),
        }
    }

    /// Parse a single SKILL.md file.
    fn parse_skill_file(path: &Path) -> Result<ParsedSkill, anyhow::Error> {
        let content = std::fs::read_to_string(path)?;
        let (frontmatter, body) = Self::split_frontmatter(&content)?;
        let meta: SkillFrontmatter = serde_yaml::from_str(&frontmatter)?;

        Ok(ParsedSkill {
            frontmatter: meta,
            body: body.to_string(),
            source_path: path.to_path_buf(),
        })
    }

    /// Split content into frontmatter and body.
    fn split_frontmatter(content: &str) -> Result<(String, &str), anyhow::Error> {
        let content = content.trim_start();

        if !content.starts_with("---") {
            anyhow::bail!("Missing frontmatter delimiter");
        }

        let after_open = &content[3..];
        let close_pos = after_open
            .find("\n---")
            .or_else(|| after_open.find("\r\n---"))
            .ok_or_else(|| anyhow::anyhow!("Unclosed frontmatter"))?;

        let frontmatter = after_open[..close_pos].trim().to_string();
        let after_close = &after_open[close_pos + 4..];
        let body = after_close.trim_start_matches(['\n', '\r']);

        Ok((frontmatter, body))
    }

    /// Get all skills.
    pub fn all(&self) -> impl Iterator<Item = &ParsedSkill> {
        self.skills.values()
    }

    /// Get always-load skills sorted by priority (highest first).
    pub fn always_load_skills(&self) -> Vec<&ParsedSkill> {
        let mut skills: Vec<_> = self
            .skills
            .values()
            .filter(|s| s.frontmatter.always_load)
            .collect();
        skills.sort_by(|a, b| b.frontmatter.priority.cmp(&a.frontmatter.priority));
        skills
    }

    /// Build SKM metadata for trigger matching.
    /// This converts our format to SKM's expected format.
    pub fn to_skm_metadata(&self) -> Vec<skm_core::SkillMetadata> {
        self.skills
            .values()
            .filter(|s| s.frontmatter.triggers.has_triggers())
            .map(|s| {
                let triggers = s.frontmatter.triggers.flatten();
                let content = format!("{}\n{}", s.frontmatter.name, s.body);
                let content_hash = xxhash_rust::xxh64::xxh64(content.as_bytes(), 0);

                skm_core::SkillMetadata {
                    name: SkillName::new(&s.frontmatter.name).unwrap_or_else(|_| {
                        // Fallback: use sanitized name
                        SkillName::new("unknown").unwrap()
                    }),
                    description: s.frontmatter.description.clone(),
                    tags: s.frontmatter.tags.clone(),
                    triggers,
                    source_path: s.source_path.clone(),
                    content_hash,
                    estimated_tokens: estimate_tokens(&s.body),
                }
            })
            .collect()
    }

    /// Get a skill by name.
    pub fn get(&self, name: &str) -> Option<&ParsedSkill> {
        self.skills.get(name)
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Estimate token count (simple heuristic: ~3.5 chars per token).
fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / 3.5).ceil() as usize
}

/// Workspace context loaded from markdown files.
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
    /// Skill registry loaded from skills/ directory.
    pub skill_registry: SkillRegistry,
    /// SKM trigger strategy for matching (built from skill_registry).
    /// Wrapped in Arc because TriggerStrategy doesn't implement Clone.
    trigger_strategy: Option<Arc<TriggerStrategy>>,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("root", &self.root)
            .field("soul", &self.soul.as_ref().map(|_| "..."))
            .field("skill_registry", &self.skill_registry)
            .field("trigger_strategy", &self.trigger_strategy.as_ref().map(|_| "TriggerStrategy"))
            .finish()
    }
}

impl Clone for Workspace {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            soul: self.soul.clone(),
            agents: self.agents.clone(),
            user: self.user.clone(),
            tools: self.tools.clone(),
            heartbeat: self.heartbeat.clone(),
            memory: self.memory.clone(),
            identity: self.identity.clone(),
            bootstrap: self.bootstrap.clone(),
            model: self.model.clone(),
            skill_registry: self.skill_registry.clone(),
            trigger_strategy: self.trigger_strategy.clone(),
        }
    }
}

impl Workspace {
    /// Load workspace files from a directory.
    pub fn load(dir: &str) -> anyhow::Result<Self> {
        let root = Path::new(dir).to_path_buf();

        // Load skills from skills/ directory
        let skills_dir = root.join("skills");
        let skill_registry = if skills_dir.is_dir() {
            SkillRegistry::load(&skills_dir).unwrap_or_else(|e| {
                tracing::warn!("Failed to load skills registry: {}", e);
                SkillRegistry::empty()
            })
        } else {
            SkillRegistry::empty()
        };

        // Build SKM trigger strategy from our skill metadata
        let trigger_strategy = if !skill_registry.is_empty() {
            let skm_metadata = skill_registry.to_skm_metadata();
            TriggerStrategy::from_metadata(&skm_metadata).ok().map(Arc::new)
        } else {
            None
        };

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
            skill_registry,
            trigger_strategy,
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
             → Don't silently disappear into a long tool loop.\n\
             → For large read tasks (reading many files/docs), plan first: use list_dir and search_files to understand structure, then read only the most relevant files. Summarize findings every ~10 tool calls.\n\n\
             When you have nothing to say, respond with ONLY: NO_REPLY\n\
             When a heartbeat check finds nothing actionable, respond with ONLY: HEARTBEAT_OK\n\n\
             ## Voice Mode\n\
             When the user asks for voice replies (any phrasing): call `set_voice_mode` tool with `enabled: true`.\n\
             When they ask to stop: call `set_voice_mode` with `enabled: false`.\n\
             When voice mode is ON, the framework converts your text to speech automatically.\n\
             Just reply with normal text — do NOT use tts tools, do NOT prefix with VOICE:.\n\
             IMPORTANT: When voice mode is ON, do NOT use markdown formatting (no *, #, |, `, [], etc.) — write plain conversational text that sounds natural when spoken aloud.\n\n\
             ## GID (Graph Indexed Development)\n\
             GID tracks project architecture, code structure, and tasks as a dependency graph (.gid/graph.yml).\n\
             - New project: Write DESIGN.md → `gid_design` → `gid_advise` → `gid_tasks`\n\
             - Existing codebase: `gid_extract` → `gid_read` or `gid_schema`\n\
             - During dev: `gid_tasks` (check), `gid_update_task` (progress), `gid_complete` (done), `gid_query_impact` (before changes)\n\
             - Quality: `gid_validate`, `gid_advise`, `gid_visual`\n\
             - Always use GID for task tracking, never raw markdown lists.\n\n\
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
            for skill in &matched_skills {
                output.push_str(&format!("### skills/{}/SKILL.md\n", skill.dir_name));
                let max_bytes = skill.max_context_bytes;
                if skill.content.len() > max_bytes {
                    output.push_str(crate::text_utils::truncate_bytes(&skill.content, max_bytes));
                    output.push_str("\n...(truncated)...\n");
                } else {
                    output.push_str(&skill.content);
                }
                output.push_str("\n\n");
            }
        }

        output
    }

    /// Match skills against a user message using SKM's TriggerStrategy.
    ///
    /// - Skills with `always_load: true` are always included.
    /// - Trigger matching supports patterns, keywords, regex, and globs.
    /// - Results are sorted by score (highest first), then truncated to `max_skills`.
    pub fn match_skills(&self, user_message: &str, max_skills: usize) -> Vec<MatchedSkill> {
        let mut results: Vec<MatchedSkill> = Vec::new();

        // Add always-load skills first (sorted by priority descending)
        let always_skills = self.skill_registry.always_load_skills();
        let always_names: std::collections::HashSet<_> = always_skills
            .iter()
            .map(|s| s.name())
            .collect();

        for skill in &always_skills {
            results.push(MatchedSkill {
                dir_name: skill.dir_name().to_string(),
                content: skill.prompt_content().to_string(),
                max_context_bytes: skill.frontmatter.max_body_size,
            });
        }

        // Use SKM trigger strategy if available
        if let Some(ref strategy) = self.trigger_strategy {
            if !user_message.is_empty() {
                let skm_metadata = self.skill_registry.to_skm_metadata();
                let metadata_refs: Vec<_> = skm_metadata.iter().collect();
                let ctx = SelectionContext::new();

                // Run trigger matching - handle both tokio runtime and non-runtime contexts
                let matches = if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    // In a tokio runtime - use block_in_place
                    tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            strategy.select(user_message, &metadata_refs, &ctx).await
                        })
                    })
                } else {
                    // Not in a runtime - create a temporary one
                    // TriggerStrategy.select is actually sync despite the async signature
                    tokio::runtime::Builder::new_current_thread()
                        .build()
                        .map(|rt| {
                            rt.block_on(async {
                                strategy.select(user_message, &metadata_refs, &ctx).await
                            })
                        })
                        .unwrap_or_else(|_| Err(skm_select::SelectError::Selection("Failed to create runtime".to_string())))
                };

                if let Ok(matches) = matches {
                    for m in matches {
                        let skill_name = m.skill.as_str();

                        // Skip if already in always-load
                        if always_names.contains(skill_name) {
                            continue;
                        }

                        // Find the full skill to get content
                        if let Some(skill) = self.skill_registry.get(skill_name) {
                            results.push(MatchedSkill {
                                dir_name: skill.dir_name().to_string(),
                                content: skill.prompt_content().to_string(),
                                max_context_bytes: skill.frontmatter.max_body_size,
                            });
                        }
                    }
                }
            }
        }

        results.truncate(max_skills);
        results
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
             → Don't silently disappear into a long tool loop.\n\
             → For large read tasks (reading many files/docs), plan first: use list_dir and search_files to understand structure, then read only the most relevant files. Summarize findings every ~10 tool calls.\n\n\
             When you have nothing to say, respond with ONLY: NO_REPLY\n\
             When a heartbeat check finds nothing actionable, respond with ONLY: HEARTBEAT_OK"
                .to_string(),
        );

        // 5. Voice mode info
        sections.push(
            "## Voice Mode\n\
             When the user asks for voice replies (any phrasing — '开启语音', 'voice mode', 'speak to me', etc.):\n\
             → Call the `set_voice_mode` tool with `enabled: true`.\n\
             When they ask to stop voice replies: call `set_voice_mode` with `enabled: false`.\n\
             When voice mode is ON, the framework automatically converts your text replies to speech.\n\
             Just reply with normal text after toggling — do NOT use tts tools, do NOT prefix with VOICE:.\n\
             IMPORTANT: When voice mode is ON, do NOT use markdown formatting (no *, #, |, `, [], etc.) — write plain conversational text that sounds natural when spoken aloud.\n\
             Do NOT assume the user wants voice replies just because they sent a voice message."
                .to_string(),
        );

        // 6. GID workflow guide
        sections.push(
            "## GID (Graph Indexed Development) — When & How\n\
             GID tracks project architecture, code structure, and tasks as a dependency graph.\n\
             Graph file: `.gid/graph.yml`. Use GID for ALL project/task tracking.\n\n\
             ### Starting a new project:\n\
             1. Write a DESIGN.md (architecture, components, dependencies)\n\
             2. `gid_design` with the design file path → generates graph nodes + tasks\n\
             3. `gid_advise` → review graph quality, fix issues\n\
             4. `gid_tasks` → see the task breakdown, start working\n\n\
             ### Joining an existing codebase:\n\
             1. `gid_extract` on the source directory → builds code-level nodes (files, classes, functions)\n\
             2. `gid_read` → understand the full graph structure\n\
             3. `gid_schema` → quick overview of code architecture without full graph\n\n\
             ### During development:\n\
             - `gid_tasks` — check current tasks (filter by status: todo, in_progress, done)\n\
             - `gid_update_task` — update task status as you work\n\
             - `gid_complete` — mark done, see what's unblocked\n\
             - `gid_add_task` / `gid_add_edge` — add new tasks or dependencies discovered during work\n\
             - `gid_query_impact` — before changing something, check what else is affected\n\
             - `gid_query_deps` — understand what a task depends on\n\n\
             ### Quality & maintenance:\n\
             - `gid_validate` — detect cycles, orphans, broken references\n\
             - `gid_advise` — get improvement suggestions, recommended task order\n\
             - `gid_visual` — render graph (ASCII/Mermaid) for overview\n\
             - `gid_refactor` — rename, merge, or split nodes\n\
             - `gid_history` — save/restore graph snapshots\n\n\
             ### Key rules:\n\
             - Always use GID for task tracking, never raw markdown task lists\n\
             - After significant work, update task status via `gid_update_task`\n\
             - Before starting implementation, check `gid_tasks` for ready tasks"
                .to_string(),
        );

        // 7. Memory recall rules
        sections.push(
            "## Memory Recall\n\
             Before answering questions about prior work, decisions, dates, people, preferences, or todos:\n\
             → Use engram_recall to search cognitive memory first.\n\
             → Check daily logs and MEMORY.md (already in context).\n\
             → If low confidence after search, say you checked but aren't sure."
                .to_string(),
        );

        // 8. Skills notice
        sections.push(
            "## Skills\n\
             Active skills are loaded from `skills/` directory below. Follow their SKILL.md instructions when the task matches."
                .to_string(),
        );

        // 9. Workspace files
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

        // 10. Daily notes (today + yesterday)
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
            for skill in &matched_skills {
                skills_section.push_str(&format!("### skills/{}/SKILL.md\n", skill.dir_name));
                let max_bytes = skill.max_context_bytes;
                if skill.content.len() > max_bytes {
                    skills_section
                        .push_str(crate::text_utils::truncate_bytes(&skill.content, max_bytes));
                    skills_section.push_str("\n...(truncated)...\n");
                } else {
                    skills_section.push_str(&skill.content);
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
    use std::fs;

    // ── Helper: create a skill directory with a SKILL.md ───────────

    fn create_skill_file(skills_dir: &Path, dir_name: &str, content: &str) {
        let skill_dir = skills_dir.join(dir_name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    fn make_skill_content(
        name: &str,
        patterns: &[&str],
        priority: u8,
        always_load: bool,
    ) -> String {
        let patterns_yaml = if patterns.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = patterns
                .iter()
                .map(|p| format!("    - \"{}\"", p))
                .collect();
            format!("triggers:\n  patterns:\n{}\n", items.join("\n"))
        };

        format!(
            "---\nname: {name}\ndescription: Desc for {name}\n{patterns_yaml}priority: {priority}\nalways_load: {always_load}\n---\n\n# Skill: {name}\n",
        )
    }

    fn make_workspace_with_skills(skills: Vec<(&str, &[&str], u8, bool)>) -> (tempfile::TempDir, Workspace) {
        let tmp = tempfile::TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        for (name, patterns, priority, always_load) in &skills {
            let content = make_skill_content(name, patterns, *priority, *always_load);
            create_skill_file(&skills_dir, name, &content);
        }

        let skill_registry = SkillRegistry::load(&skills_dir).unwrap();
        let trigger_strategy = if !skill_registry.is_empty() {
            let skm_metadata = skill_registry.to_skm_metadata();
            TriggerStrategy::from_metadata(&skm_metadata).ok().map(Arc::new)
        } else {
            None
        };

        let ws = Workspace {
            root: tmp.path().to_path_buf(),
            soul: None,
            agents: None,
            user: None,
            tools: None,
            heartbeat: None,
            memory: None,
            identity: None,
            bootstrap: None,
            model: None,
            skill_registry,
            trigger_strategy,
        };

        (tmp, ws)
    }

    // ── match_skills tests ─────────────────────────────────────────

    #[test]
    fn test_match_skills_trigger_hit() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("idea-intake", &["http://", "https://", "idea:"], 50, false),
            ("code-review", &["review", "pr", "diff"], 60, false),
        ]);

        let matched = ws.match_skills("check out https://example.com", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].dir_name, "idea-intake");
    }

    #[test]
    fn test_match_skills_no_match() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("idea-intake", &["http://", "https://", "idea:"], 50, false),
            ("code-review", &["review", "pr", "diff"], 60, false),
        ]);

        let matched = ws.match_skills("what is the weather today?", 5);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_skills_always_load() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("core-rules", &[], 10, true),
            ("idea-intake", &["http://"], 50, false),
        ]);

        // Even with no matching triggers, always_load skill is included
        let matched = ws.match_skills("random message", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].dir_name, "core-rules");
    }

    #[test]
    fn test_match_skills_always_load_plus_trigger() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("core-rules", &[], 10, true),
            ("idea-intake", &["http://"], 50, false),
        ]);

        let matched = ws.match_skills("look at http://example.com", 5);
        assert_eq!(matched.len(), 2);
        // always-load comes first, then triggered
        assert_eq!(matched[0].dir_name, "core-rules");
        assert_eq!(matched[1].dir_name, "idea-intake");
    }

    #[test]
    fn test_match_skills_max_limit() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("skill-a", &["test"], 80, false),
            ("skill-b", &["test"], 70, false),
            ("skill-c", &["test"], 60, false),
            ("skill-d", &["test"], 50, false),
            ("skill-e", &["test"], 40, false),
        ]);

        let matched = ws.match_skills("test message", 3);
        assert_eq!(matched.len(), 3);
    }

    #[test]
    fn test_match_skills_case_insensitive() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("idea-intake", &["HTTPS://", "Idea:"], 50, false),
        ]);

        let matched = ws.match_skills("check https://example.com", 5);
        assert_eq!(matched.len(), 1);

        let matched2 = ws.match_skills("IDEA: something cool", 5);
        assert_eq!(matched2.len(), 1);
    }

    #[test]
    fn test_match_skills_chinese_triggers() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("idea-intake", &["想法:", "记录一下"], 50, false),
        ]);

        let matched = ws.match_skills("想法: 做一个新项目", 5);
        assert_eq!(matched.len(), 1);

        let matched2 = ws.match_skills("帮我记录一下这个想法", 5);
        assert_eq!(matched2.len(), 1);
    }

    #[test]
    fn test_match_skills_empty_message() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("always", &[], 10, true),
            ("triggered", &["hello"], 50, false),
        ]);

        let matched = ws.match_skills("", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].dir_name, "always");
    }

    #[test]
    fn test_match_skills_empty_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = Workspace {
            root: tmp.path().to_path_buf(),
            soul: None,
            agents: None,
            user: None,
            tools: None,
            heartbeat: None,
            memory: None,
            identity: None,
            bootstrap: None,
            model: None,
            skill_registry: SkillRegistry::empty(),
            trigger_strategy: None,
        };

        let matched = ws.match_skills("anything", 5);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_skills_content_included() {
        let (_tmp, ws) = make_workspace_with_skills(vec![
            ("my-skill", &["trigger"], 50, false),
        ]);

        let matched = ws.match_skills("trigger this", 5);
        assert_eq!(matched.len(), 1);
        assert!(matched[0].content.contains("# Skill: my-skill"));
    }

    #[test]
    fn test_match_skills_regex_triggers() {
        let tmp = tempfile::TempDir::new().unwrap();
        let skills_dir = tmp.path().join("skills");
        fs::create_dir_all(&skills_dir).unwrap();

        create_skill_file(
            &skills_dir,
            "url-detect",
            "---\nname: url-detect\ndescription: Detect URLs\ntriggers:\n  regex:\n    - \"https?://[^\\\\s]+\"\npriority: 80\n---\n\n# URL Detection\n",
        );

        let skill_registry = SkillRegistry::load(&skills_dir).unwrap();
        let trigger_strategy = if !skill_registry.is_empty() {
            let skm_metadata = skill_registry.to_skm_metadata();
            TriggerStrategy::from_metadata(&skm_metadata).ok().map(Arc::new)
        } else {
            None
        };

        let ws = Workspace {
            root: tmp.path().to_path_buf(),
            soul: None,
            agents: None,
            user: None,
            tools: None,
            heartbeat: None,
            memory: None,
            identity: None,
            bootstrap: None,
            model: None,
            skill_registry,
            trigger_strategy,
        };

        let matched = ws.match_skills("visit http://example.com/path?q=1", 5);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].dir_name, "url-detect");
    }
}
