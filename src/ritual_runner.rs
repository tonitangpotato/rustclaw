//! Ritual Runner — bridges gid-core's v2 pure function state machine with RustClaw's infrastructure.
//!
//! Drives the transition loop: transition(state, event) → (new_state, actions),
//! executing each action via the appropriate executor (LLM, shell, filesystem, Telegram).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::Result;
use gid_core::ritual::{
    V2State as RitualState, V2Phase as RitualPhase, V2Event as RitualEvent,
    V2Action as RitualAction, V2ProjectState as ProjectState,
    ImplementStrategy, transition, truncate,
};

/// Callback for sending notifications (Telegram messages).
pub type NotifyFn = Arc<dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync>;

/// Executes the ritual state machine loop, bridging gid-core's pure transitions
/// with RustClaw's IO capabilities (LLM, shell, filesystem, notifications).
pub struct RitualRunner {
    project_root: PathBuf,
    state_path: PathBuf,
    llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
    notify: NotifyFn,
}

impl RitualRunner {
    pub fn new(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
    ) -> Self {
        let state_path = project_root.join(".gid/ritual-state.json");
        Self {
            project_root,
            state_path,
            llm_client,
            notify,
        }
    }

    /// Load persisted state or create new Idle state.
    pub fn load_state(&self) -> Result<RitualState> {
        if self.state_path.exists() {
            let data = std::fs::read_to_string(&self.state_path)?;
            let state: RitualState = serde_json::from_str(&data)?;
            Ok(state)
        } else {
            Ok(RitualState::new())
        }
    }

    /// Save state to disk.
    pub fn save_state(&self, state: &RitualState) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(state)?;
        std::fs::write(&self.state_path, data)?;
        Ok(())
    }

    /// Start a new ritual with a task description.
    pub async fn start(&self, task: String) -> Result<RitualState> {
        let state = RitualState::new();
        let event = RitualEvent::Start { task };
        self.run_loop(state, event).await
    }

    /// Send a user event (Cancel, Retry, SkipPhase) to the current ritual.
    pub async fn send_event(&self, event: RitualEvent) -> Result<RitualState> {
        let state = self.load_state()?;
        self.run_loop(state, event).await
    }

    /// The core loop: transition → execute actions → get event → repeat until terminal.
    async fn run_loop(&self, mut state: RitualState, mut event: RitualEvent) -> Result<RitualState> {
        loop {
            let (new_state, actions) = transition(&state, event);
            state = new_state;

            // Execute fire-and-forget actions first (Notify, SaveState, UpdateGraph, Cleanup)
            self.execute_fire_and_forget_with_state(&actions, &state).await;

            if state.phase.is_terminal() {
                // Send enriched terminal notification (duration, context, guidance)
                self.send_terminal_notification(&state).await;
                break;
            }

            // Execute the single event-producing action
            event = match self.execute_event_producing(&actions).await {
                Ok(evt) => evt,
                Err(e) => RitualEvent::SkillFailed {
                    phase: state.phase.display_name().to_string(),
                    error: format!("Executor error: {}", e),
                },
            };
        }
        Ok(state)
    }

    /// Execute fire-and-forget actions (Notify, SaveState, UpdateGraph, Cleanup).
    /// Notify is truly fire-and-forget: spawned as a task, failures only logged.
    async fn execute_fire_and_forget_with_state(&self, actions: &[RitualAction], state: &RitualState) {
        for action in actions {
            match action {
                RitualAction::Notify { message } => {
                    self.fire_and_forget_notify(message.clone());
                }
                RitualAction::SaveState => {
                    if let Err(e) = self.save_state(state) {
                        tracing::error!("Failed to save ritual state: {}", e);
                    }
                }
                RitualAction::UpdateGraph { description } => {
                    self.update_graph(description).await;
                }
                RitualAction::Cleanup => {
                    self.cleanup().await;
                }
                _ => {} // Event-producing actions handled separately
            }
        }
    }

    /// Send a notification as fire-and-forget: spawned into a tokio task.
    /// Send failures are logged but never crash the ritual.
    fn fire_and_forget_notify(&self, message: String) {
        let notify = self.notify.clone();
        tokio::spawn(async move {
            notify(message).await;
        });
    }

    /// Send enriched notification when ritual reaches a terminal state.
    /// Adds duration, phase summary, error context, and retry guidance.
    async fn send_terminal_notification(&self, state: &RitualState) {
        let elapsed = chrono::Utc::now()
            .signed_duration_since(state.started_at);
        let duration_str = format_duration(elapsed);

        let phases_traversed = state.transitions.len();

        let msg = match &state.phase {
            gid_core::ritual::V2Phase::Done => {
                format!(
                    "✅ **Ritual complete!**\n\n\
                     📋 Task: {}\n\
                     ⏱ Duration: {}\n\
                     🔄 Transitions: {}\n\
                     {}",
                    truncate(&state.task, 200),
                    duration_str,
                    phases_traversed,
                    if state.verify_retries > 0 {
                        format!("🔧 Verify retries: {}", state.verify_retries)
                    } else {
                        String::new()
                    },
                )
            }
            gid_core::ritual::V2Phase::Escalated => {
                let error = state.error_context.as_deref().unwrap_or("unknown error");
                let failed_phase = state.failed_phase.as_ref()
                    .map(|p| p.display_name())
                    .unwrap_or("unknown");
                format!(
                    "🚨 **Ritual escalated — needs human intervention**\n\n\
                     📋 Task: {}\n\
                     💥 Failed in: {} phase\n\
                     ❌ Error: {}\n\
                     ⏱ Duration: {}\n\
                     🔄 Transitions: {}\n\n\
                     **Next steps:**\n\
                     • `/ritual retry` — retry the failed phase\n\
                     • `/ritual skip` — skip to the next phase\n\
                     • `/ritual cancel` — abort the ritual",
                    truncate(&state.task, 200),
                    failed_phase,
                    truncate(error, 300),
                    duration_str,
                    phases_traversed,
                )
            }
            gid_core::ritual::V2Phase::Cancelled => {
                format!(
                    "🛑 **Ritual cancelled**\n\n\
                     📋 Task: {}\n\
                     ⏱ Duration: {}",
                    truncate(&state.task, 200),
                    duration_str,
                )
            }
            _ => return, // Not terminal, nothing to send
        };

        // Fire-and-forget: don't block on send, don't crash on failure
        self.fire_and_forget_notify(msg);
    }

    /// Find and execute the single event-producing action from the action list.
    async fn execute_event_producing(&self, actions: &[RitualAction]) -> Result<RitualEvent> {
        for action in actions {
            match action {
                RitualAction::DetectProject => {
                    return self.detect_project().await;
                }
                RitualAction::RunSkill { name, context } => {
                    return self.run_skill(name, context).await;
                }
                RitualAction::RunShell { command } => {
                    return self.run_shell(command).await;
                }
                RitualAction::RunPlanning => {
                    return self.run_planning().await;
                }
                RitualAction::RunHarness { tasks } => {
                    tracing::warn!(
                        "RunHarness not yet implemented ({} tasks), falling back to SingleLlm",
                        tasks.len()
                    );
                    let context = format!(
                        "Implement all of the following tasks:\n{}",
                        tasks.iter().enumerate()
                            .map(|(i, t)| format!("{}. {}", i + 1, t))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    return self.run_skill("implement", &context).await;
                }
                _ => {} // Fire-and-forget handled above
            }
        }
        Err(anyhow::anyhow!("No event-producing action found in actions"))
    }

    /// Scan filesystem to detect project state.
    async fn detect_project(&self) -> Result<RitualEvent> {
        let root = &self.project_root;

        let has_design = root.join("DESIGN.md").exists()
            || root.join(".gid/DESIGN.md").exists();

        let has_graph = root.join(".gid/graph.yml").exists()
            || root.join("graph.yml").exists();

        let has_cargo = root.join("Cargo.toml").exists();
        let has_package_json = root.join("package.json").exists();
        let has_pyproject = root.join("pyproject.toml").exists();

        let has_source = root.join("src").exists()
            || root.join("lib").exists()
            || root.join("app").exists();

        let has_tests = root.join("tests").exists()
            || root.join("test").exists()
            || root.join("spec").exists();

        // Detect language
        let language = if has_cargo {
            Some("rust".to_string())
        } else if has_package_json {
            Some("typescript".to_string())
        } else if has_pyproject {
            Some("python".to_string())
        } else {
            None
        };

        // Count source files (basic scan)
        let source_file_count = if root.join("src").exists() {
            count_files_recursive(&root.join("src")).await
        } else {
            0
        };

        // Detect verify command — prefer .gid/config.yml, fallback to language default
        let gating_config = gid_core::ritual::load_gating_config(root);
        let verify_command = gating_config.verify_command.or_else(|| {
            if has_cargo {
                Some("cargo build && cargo test".to_string())
            } else if has_package_json {
                Some("npm test".to_string())
            } else if has_pyproject {
                Some("python -m pytest".to_string())
            } else {
                None
            }
        });

        let project_state = ProjectState {
            has_design,
            has_graph,
            has_source,
            has_tests,
            language,
            source_file_count,
            verify_command,
        };

        tracing::info!(
            "Project detected: design={}, graph={}, source={} ({} files), tests={}, lang={:?}",
            project_state.has_design,
            project_state.has_graph,
            project_state.has_source,
            project_state.source_file_count,
            project_state.has_tests,
            project_state.language,
        );

        Ok(RitualEvent::ProjectDetected(project_state))
    }

    /// Run a skill phase using the RitualLlmAdapter.
    async fn run_skill(&self, name: &str, context: &str) -> Result<RitualEvent> {
        use crate::ritual_adapter::RitualLlmAdapter;
        use gid_core::ritual::llm::{LlmClient as GidLlmClient, ToolDefinition};

        let adapter = RitualLlmAdapter::new(self.llm_client.clone());
        let gid_client: Arc<dyn GidLlmClient> = adapter.into_arc();

        // Load skill-specific prompt (file-based, with built-in fallback)
        let base_prompt = self.load_skill_prompt(name);
        let skill_prompt = if context.is_empty() {
            base_prompt
        } else {
            format!("## USER TASK\n{}\n\n## INSTRUCTIONS\n{}", context, base_prompt)
        };

        // Define standard tools for skill execution
        let tools = vec![
            ToolDefinition {
                name: "Read".into(),
                description: "Read a file from disk".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to project root" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "Write".into(),
                description: "Write entire content to a file (creates or overwrites)".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to project root" },
                        "content": { "type": "string", "description": "Full file content to write" }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDefinition {
                name: "Edit".into(),
                description: "Replace exact text in a file. oldText must match exactly (including whitespace). Use for precise, surgical edits instead of rewriting entire files.".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to project root" },
                        "oldText": { "type": "string", "description": "Exact text to find and replace" },
                        "newText": { "type": "string", "description": "New text to replace with" }
                    },
                    "required": ["path", "oldText", "newText"]
                }),
            },
            ToolDefinition {
                name: "Bash".into(),
                description: "Run a bash command".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Bash command to execute" }
                    },
                    "required": ["command"]
                }),
            },
        ];

        let result = gid_client.run_skill(
            &skill_prompt,
            tools,
            "sonnet",
            &self.project_root,
        ).await;

        match result {
            Ok(skill_result) => {
                tracing::info!(
                    "Skill '{}' completed: {} tool calls, {} tokens",
                    name, skill_result.tool_calls_made, skill_result.tokens_used
                );
                Ok(RitualEvent::SkillCompleted {
                    phase: name.to_string(),
                    artifacts: skill_result.artifacts_created.iter().map(|p| p.display().to_string()).collect(),
                })
            }
            Err(e) => {
                tracing::error!("Skill '{}' failed: {}", name, e);
                Ok(RitualEvent::SkillFailed {
                    phase: name.to_string(),
                    error: format!("{}", e),
                })
            }
        }
    }

        /// Load skill prompt from file or built-in fallback.
    /// Priority: .gid/skills/{name}.md → ~/rustclaw/skills/{name}/SKILL.md → built-in
    fn load_skill_prompt(&self, skill_name: &str) -> String {
        // Project-local skill
        let gid_skill = self.project_root.join(".gid").join("skills").join(format!("{}.md", skill_name));
        if gid_skill.exists() {
            if let Ok(content) = std::fs::read_to_string(&gid_skill) {
                return content;
            }
        }

        // RustClaw skills directory
        let home = std::env::var("HOME").unwrap_or_default();
        let rustclaw_skill = PathBuf::from(&home)
            .join("rustclaw").join("skills").join(skill_name).join("SKILL.md");
        if rustclaw_skill.exists() {
            if let Ok(content) = std::fs::read_to_string(&rustclaw_skill) {
                return content;
            }
        }

        // Built-in fallbacks with specific guidance
        match skill_name {
            "draft-design" => "Read the project structure and the user's task description. \
                Create a DESIGN.md document with: Overview, Goals/Non-goals, Design, \
                Implementation Plan, Testing Strategy, Open Questions. \
                Keep it concise (500-1500 words). Write to DESIGN.md.".into(),
            "update-design" => "Read the existing DESIGN.md and the user's task. \
                Make MINIMAL targeted edits — add a new section or update the relevant section only. \
                Do NOT rewrite the entire document. Use Edit tool to modify specific sections. \
                If the change is trivial (e.g., adding a command), add a brief bullet point.".into(),
            "generate-graph" | "design-to-graph" => "Read DESIGN.md. Generate a GID graph in YAML format \
                and write it to .gid/graph.yml. Include component nodes, file nodes, and task nodes.".into(),
            "update-graph" => "Read the existing .gid/graph.yml and the task description. \
                Add or update ONLY the relevant nodes/edges. Do NOT regenerate the entire graph. \
                Use Edit tool for targeted changes. For small features, just add 1-3 task nodes.".into(),
            "implement" => "Implement the described changes. Read relevant source files, \
                make the necessary code changes. Follow existing patterns. \
                Be precise — edit only the files that need to change.".into(),
            _ => format!("Execute the '{}' skill using the provided tools. \
                Read existing files, make targeted changes, run commands as needed.", skill_name),
        }
    }

    /// Run a shell command (for verify phase).
    async fn run_shell(&self, command: &str) -> Result<RitualEvent> {
        tracing::info!("Running shell command: {}", command);

        let output = tokio::process::Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(&self.project_root)
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        tracing::info!("Shell exit code: {}", exit_code);

        if output.status.success() {
            Ok(RitualEvent::ShellCompleted {
                stdout: truncate(&stdout, 2000),
                exit_code,
            })
        } else {
            Ok(RitualEvent::ShellFailed {
                stderr: truncate(&format!("{}\n{}", stderr, stdout), 2000),
                exit_code,
            })
        }
    }

    /// Run planning phase — LLM decides SingleLlm vs MultiAgent strategy.
    async fn run_planning(&self) -> Result<RitualEvent> {
        use crate::ritual_adapter::RitualLlmAdapter;
        use gid_core::ritual::llm::{LlmClient as GidLlmClient, ToolDefinition};

        let adapter = RitualLlmAdapter::new(self.llm_client.clone());
        let gid_client: Arc<dyn GidLlmClient> = adapter.into_arc();

        // Read DESIGN.md if available for planning context
        let design_content = match tokio::fs::read_to_string(
            self.project_root.join("DESIGN.md")
        ).await {
            Ok(content) => content,
            Err(_) => {
                tokio::fs::read_to_string(self.project_root.join(".gid/DESIGN.md"))
                    .await
                    .unwrap_or_default()
            }
        };

        let planning_prompt = format!(
            "# Implementation Planning\n\n\
             Analyze the following design and decide on an implementation strategy.\n\n\
             ## Design\n{}\n\n\
             ## Instructions\n\
             Based on the design, decide:\n\
             1. **SingleLlm**: One agent implements everything sequentially (for small/medium tasks)\n\
             2. **MultiAgent**: Split into parallel tasks (for large tasks with independent components)\n\n\
             Respond with ONLY a JSON object:\n\
             - For single: `{{\"strategy\": \"single\"}}`\n\
             - For multi: `{{\"strategy\": \"multi\", \"tasks\": [\"task1\", \"task2\", ...]}}`",
            truncate(&design_content, 15000)
        );

        // No tools needed for planning — DESIGN.md content is already in the prompt
        let tools = vec![];

        let result = gid_client.run_skill(
            &planning_prompt,
            tools,
            "sonnet",
            &self.project_root,
        ).await;

        match result {
            Ok(skill_result) => {
                // Try to parse the LLM output as a strategy decision
                let strategy = parse_strategy(&skill_result.output);
                tracing::info!("Planning decided strategy: {:?}", strategy);
                Ok(RitualEvent::PlanDecided(strategy))
            }
            Err(e) => {
                tracing::warn!("Planning failed, defaulting to SingleLlm: {}", e);
                Ok(RitualEvent::PlanDecided(ImplementStrategy::SingleLlm))
            }
        }
    }

    /// Best-effort update of graph nodes.
    async fn update_graph(&self, description: &str) {
        let graph_path = self.project_root.join(".gid/graph.yml");
        if graph_path.exists() {
            tracing::info!("UpdateGraph (best-effort): {}", truncate(description, 100));
            // Best-effort: just log for now. Full graph update would require
            // parsing graph.yml and updating node statuses.
        } else {
            tracing::debug!("No graph.yml found, skipping UpdateGraph");
        }
    }

    /// Clean up ritual artifacts.
    async fn cleanup(&self) {
        // Remove ritual state file
        if self.state_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&self.state_path).await {
                tracing::warn!("Failed to remove ritual-state.json: {}", e);
            }
        }
        // Remove ritual.yml if present
        let ritual_yml = self.project_root.join(".gid/ritual.yml");
        if ritual_yml.exists() {
            if let Err(e) = tokio::fs::remove_file(&ritual_yml).await {
                tracing::warn!("Failed to remove ritual.yml: {}", e);
            }
        }
    }
}

/// Parse the LLM output to determine implementation strategy.
fn parse_strategy(output: &str) -> ImplementStrategy {
    // Try to find JSON in the output
    if let Some(start) = output.find('{') {
        if let Some(end) = output.rfind('}') {
            let json_str = &output[start..=end];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                if val.get("strategy").and_then(|s| s.as_str()) == Some("multi") {
                    if let Some(tasks) = val.get("tasks").and_then(|t| t.as_array()) {
                        let task_list: Vec<String> = tasks
                            .iter()
                            .filter_map(|t| t.as_str().map(|s| s.to_string()))
                            .collect();
                        if !task_list.is_empty() {
                            return ImplementStrategy::MultiAgent { tasks: task_list };
                        }
                    }
                }
            }
        }
    }
    ImplementStrategy::SingleLlm
}

/// Format a chrono::Duration into a human-readable string (e.g., "2m 34s", "1h 5m").
fn format_duration(d: chrono::TimeDelta) -> String {
    let total_secs = d.num_seconds().max(0);
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Recursively count files in a directory.
async fn count_files_recursive(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(mut entries) = tokio::fs::read_dir(&current).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    count += 1;
                }
            }
        }
    }
    count
}
