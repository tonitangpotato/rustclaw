//! Template-based system prompt builder.
//!
//! This module provides a modular, conditional system for building system prompts.
//! Each section implements the `PromptSection` trait and can be conditionally
//! included based on configuration and context.

mod sections;

pub use sections::*;

use tracing::{debug, info};

use crate::config::Config;
use crate::workspace::MatchedSkill;

/// A section of the system prompt, conditionally included.
pub trait PromptSection: Send + Sync {
    /// Unique identifier for this section.
    fn id(&self) -> &str;

    /// Whether this section should be included given current context.
    fn should_include(&self, ctx: &PromptContext) -> bool;

    /// Render the section content.
    fn render(&self, ctx: &PromptContext) -> String;

    /// Priority for ordering (lower = earlier in prompt).
    fn priority(&self) -> u32 {
        100
    }
}

/// Context available to sections during rendering.
pub struct PromptContext<'a> {
    pub current_time: String,
    pub workspace_path: String,
    pub model_name: String,
    pub is_heartbeat: bool,
    pub is_subagent: bool,
    pub subagent_task: Option<&'a str>,
    pub user_message: Option<&'a str>,
    pub config: &'a PromptConfig,
    // Workspace file contents
    pub soul: Option<&'a str>,
    pub agents: Option<&'a str>,
    pub user: Option<&'a str>,
    pub tools: Option<&'a str>,
    pub identity: Option<&'a str>,
    pub memory: Option<&'a str>,
    pub heartbeat: Option<&'a str>,
    pub daily_notes: Option<String>,
    pub matched_skills: Vec<MatchedSkill>,
}

/// Configuration for which sections to include.
#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub gid_enabled: bool,
    pub ritual_enabled: bool,
    pub harness_enabled: bool,
    pub voice_mode_available: bool,
    pub memory_enabled: bool,
    pub skills_enabled: bool,
    pub orchestrator_enabled: bool,
}

impl PromptConfig {
    /// Create a PromptConfig from the main Config.
    pub fn from_config(config: &Config) -> Self {
        let gid_enabled = config.gid.enabled;
        Self {
            gid_enabled,
            // Ritual and harness are auto-enabled when GID is enabled (for now)
            ritual_enabled: gid_enabled,
            harness_enabled: gid_enabled,
            // Voice mode is always available (it's a runtime toggle)
            voice_mode_available: true,
            // Memory (engram) recall instructions are shown if memory config exists
            memory_enabled: config.memory.engram_db.is_some(),
            // Skills are always enabled (empty registry just means no skills)
            skills_enabled: true,
            orchestrator_enabled: config.orchestrator.enabled,
        }
    }

    /// Create a default config (all features enabled).
    pub fn default_all_enabled() -> Self {
        Self {
            gid_enabled: true,
            ritual_enabled: true,
            harness_enabled: true,
            voice_mode_available: true,
            memory_enabled: true,
            skills_enabled: true,
            orchestrator_enabled: false,
        }
    }
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            gid_enabled: false,
            ritual_enabled: false,
            harness_enabled: false,
            voice_mode_available: true,
            memory_enabled: false,
            skills_enabled: true,
            orchestrator_enabled: false,
        }
    }
}

/// The builder that assembles the system prompt from sections.
pub struct PromptBuilder {
    sections: Vec<Box<dyn PromptSection>>,
}

impl PromptBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    /// Create a builder with default sections for full agent prompt.
    pub fn with_defaults() -> Self {
        let mut builder = Self::new();

        // Add all default sections
        builder.add_section(Box::new(PreambleSection));
        builder.add_section(Box::new(ContextFilesSection));
        builder.add_section(Box::new(ToolCallStyleSection));
        builder.add_section(Box::new(SafetySection));
        builder.add_section(Box::new(CommunicationSection));
        builder.add_section(Box::new(VoiceModeSection));
        builder.add_section(Box::new(GidSection));
        builder.add_section(Box::new(GidRitualSection));
        builder.add_section(Box::new(GidHarnessSection));
        builder.add_section(Box::new(MemoryRecallSection));
        builder.add_section(Box::new(SkillsSection));
        builder.add_section(Box::new(WorkspaceFilesSection));
        builder.add_section(Box::new(HeartbeatSection));
        builder.add_section(Box::new(MemoryFileSection));
        builder.add_section(Box::new(DailyNotesSection));
        builder.add_section(Box::new(MatchedSkillsSection));

        builder
    }

    /// Create a builder for subagent prompts (minimal sections).
    pub fn for_subagent() -> Self {
        let mut builder = Self::new();

        // Subagents only get the subagent-specific section
        builder.add_section(Box::new(SubagentSection));

        builder
    }

    /// Add a custom section.
    pub fn add_section(&mut self, section: Box<dyn PromptSection>) {
        self.sections.push(section);
    }

    /// Build the final prompt.
    pub fn build(&self, ctx: &PromptContext) -> String {
        let mut sections: Vec<_> = self
            .sections
            .iter()
            .filter(|s| s.should_include(ctx))
            .collect();

        sections.sort_by_key(|s| s.priority());

        let included_ids: Vec<&str> = sections.iter().map(|s| s.id()).collect();
        let result = sections
            .iter()
            .map(|s| s.render(ctx))
            .collect::<Vec<_>>()
            .join("\n\n");

        // Estimate tokens (~4 chars per token for English/mixed content)
        let estimated_tokens = result.len() / 4;
        info!(
            sections = included_ids.len(),
            estimated_tokens,
            "System prompt built"
        );
        debug!(
            included = ?included_ids,
            chars = result.len(),
            "System prompt sections"
        );

        result
    }
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}
