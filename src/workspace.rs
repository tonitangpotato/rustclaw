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
}

impl Workspace {
    /// Load workspace files from a directory.
    pub fn load(dir: &str) -> anyhow::Result<Self> {
        let root = Path::new(dir).to_path_buf();

        Ok(Self {
            soul: Self::read_optional(&root, "SOUL.md"),
            agents: Self::read_optional(&root, "AGENTS.md"),
            user: Self::read_optional(&root, "USER.md"),
            tools: Self::read_optional(&root, "TOOLS.md"),
            heartbeat: Self::read_optional(&root, "HEARTBEAT.md"),
            memory: Self::read_optional(&root, "MEMORY.md"),
            identity: Self::read_optional(&root, "IDENTITY.md"),
            bootstrap: Self::read_optional(&root, "BOOTSTRAP.md"),
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
        self.build_system_prompt_with_options(false)
    }

    /// Build the system prompt with optional heartbeat context.
    pub fn build_system_prompt_with_options(&self, is_heartbeat: bool) -> String {
        let current_time = Local::now().format("%Y-%m-%d %H:%M:%S %Z").to_string();
        let workspace_path = self.root.display().to_string();

        let mut output = format!(
            "You are an AI assistant running on RustClaw.\n\
             Current time: {}\n\
             Workspace: {}\n\n\
             ## Your Context Files\n",
            current_time, workspace_path
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

        // MEMORY.md only in main session (security) - handled separately

        output
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
