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
/// Shared registry of active ritual cancellation tokens.
/// Key: ritual ID, Value: CancellationToken.
pub type CancelRegistry = Arc<std::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>>;

/// Create a new empty cancel registry.
pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub struct RitualRunner {
    project_root: PathBuf,
    rituals_dir: PathBuf,
    /// Legacy single state path (for backward compat loading)
    legacy_state_path: PathBuf,
    llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
    notify: NotifyFn,
    cancel_registry: CancelRegistry,
}

impl RitualRunner {
    pub fn new(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
    ) -> Self {
        Self::with_cancel_registry(project_root, llm_client, notify, new_cancel_registry())
    }

    pub fn with_cancel_registry(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
        cancel_registry: CancelRegistry,
    ) -> Self {
        let rituals_dir = project_root.join(".gid/rituals");
        let legacy_state_path = project_root.join(".gid/ritual-state.json");
        Self {
            project_root,
            rituals_dir,
            legacy_state_path,
            llm_client,
            notify,
            cancel_registry,
        }
    }

    /// Get state path for a specific ritual ID.
    fn state_path_for(&self, ritual_id: &str) -> PathBuf {
        self.rituals_dir.join(format!("{}.json", ritual_id))
    }

    /// Load state for a specific ritual by ID.
    pub fn load_state_by_id(&self, ritual_id: &str) -> Result<RitualState> {
        let path = self.state_path_for(ritual_id);
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            let state: RitualState = serde_json::from_str(&data)?;
            Ok(state)
        } else {
            Err(anyhow::anyhow!("Ritual {} not found", ritual_id))
        }
    }

    /// Load the most recent active ritual, or create new Idle state.
    /// Also migrates legacy single-file state if found.
    pub fn load_state(&self) -> Result<RitualState> {
        // Try to find active ritual in rituals dir
        if let Some(state) = self.find_latest_active()? {
            return Ok(state);
        }
        // Legacy fallback
        if self.legacy_state_path.exists() {
            let data = std::fs::read_to_string(&self.legacy_state_path)?;
            let state: RitualState = serde_json::from_str(&data)?;
            return Ok(state);
        }
        Ok(RitualState::new())
    }

    /// List all ritual states (active and terminal).
    pub fn list_rituals(&self) -> Result<Vec<RitualState>> {
        let mut rituals = Vec::new();

        // Check rituals dir
        if self.rituals_dir.exists() {
            for entry in std::fs::read_dir(&self.rituals_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "json") {
                    if let Ok(data) = std::fs::read_to_string(&path) {
                        if let Ok(state) = serde_json::from_str::<RitualState>(&data) {
                            rituals.push(state);
                        }
                    }
                }
            }
        }

        // Legacy fallback
        if rituals.is_empty() && self.legacy_state_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&self.legacy_state_path) {
                if let Ok(state) = serde_json::from_str::<RitualState>(&data) {
                    rituals.push(state);
                }
            }
        }

        // Sort by updated_at descending
        rituals.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(rituals)
    }

    /// Find the latest active (non-terminal, non-idle) ritual.
    fn find_latest_active(&self) -> Result<Option<RitualState>> {
        let rituals = self.list_rituals()?;
        Ok(rituals.into_iter().find(|r| {
            !r.phase.is_terminal() && r.phase != gid_core::ritual::V2Phase::Idle
        }))
    }

    /// Save state to disk (uses ritual ID for path).
    pub fn save_state(&self, state: &RitualState) -> Result<()> {
        std::fs::create_dir_all(&self.rituals_dir)?;
        let path = self.state_path_for(&state.id);
        let data = serde_json::to_string_pretty(state)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Start a new ritual with a task description.
    /// Multiple rituals can now run in parallel.
    pub async fn start(&self, task: String) -> Result<RitualState> {
        let state = RitualState::new();
        let ritual_id = state.id.clone();
        tracing::info!(ritual_id = %ritual_id, task = %task, "Starting new ritual");

        // Register cancellation token
        let cancel_token = tokio_util::sync::CancellationToken::new();
        {
            let mut reg = self.cancel_registry.lock().unwrap();
            reg.insert(ritual_id.clone(), cancel_token.clone());
        }

        let result = self.run_loop(state, RitualEvent::Start { task }, cancel_token).await;

        // Cleanup token from registry
        {
            let mut reg = self.cancel_registry.lock().unwrap();
            reg.remove(&ritual_id);
        }

        result
    }

    /// Cancel a running ritual by ID (triggers cancellation token).
    /// Returns true if a running ritual was found and cancelled.
    pub fn cancel_running(&self, ritual_id: &str) -> bool {
        let reg = self.cancel_registry.lock().unwrap();
        if let Some(token) = reg.get(ritual_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel all running rituals.
    pub fn cancel_all_running(&self) -> usize {
        let reg = self.cancel_registry.lock().unwrap();
        let count = reg.len();
        for token in reg.values() {
            token.cancel();
        }
        count
    }

    /// Send a user event (Cancel, Retry, SkipPhase) to the current ritual.
    pub async fn send_event(&self, event: RitualEvent) -> Result<RitualState> {
        let state = self.load_state()?;
        let ritual_id = state.id.clone();

        // For UserCancel, also trigger cancellation token of running ritual
        if matches!(&event, RitualEvent::UserCancel) {
            self.cancel_running(&ritual_id);
        }

        // Re-register token for the continued loop
        let cancel_token = tokio_util::sync::CancellationToken::new();
        {
            let mut reg = self.cancel_registry.lock().unwrap();
            reg.insert(ritual_id.clone(), cancel_token.clone());
        }

        let result = self.run_loop(state, event, cancel_token).await;

        {
            let mut reg = self.cancel_registry.lock().unwrap();
            reg.remove(&ritual_id);
        }

        result
    }

    /// The core loop: transition → execute actions → get event → repeat until terminal.
    async fn run_loop(
        &self,
        mut state: RitualState,
        mut event: RitualEvent,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<RitualState> {
        loop {
            // Check cancellation before each iteration
            if cancel_token.is_cancelled() {
                tracing::info!(ritual_id = %state.id, "Ritual cancelled via token");
                event = RitualEvent::UserCancel;
            }

            let (new_state, actions) = transition(&state, event);
            state = new_state;

            if state.phase.is_terminal() || state.phase.is_paused() {
                // Terminal or paused: save state, send notification
                self.execute_fire_and_forget_with_state(&actions, &state).await;
                if state.phase.is_terminal() {
                    self.send_terminal_notification(&state).await;
                }
                break;
            }

            // Execute the single event-producing action with cancellation
            let (evt, tokens_used) = tokio::select! {
                _ = cancel_token.cancelled() => {
                    tracing::info!(ritual_id = %state.id, "Ritual interrupted during action execution");
                    (RitualEvent::UserCancel, 0)
                }
                result = self.execute_event_producing(&actions, &state) => {
                    match result {
                        Ok(pair) => pair,
                        Err(e) => (RitualEvent::SkillFailed {
                            phase: state.phase.display_name().to_string(),
                            error: format!("Executor error: {}", e),
                        }, 0),
                    }
                }
            };
            // Record tokens BEFORE SaveState fires
            if tokens_used > 0 {
                let phase_name = state.phase.display_name().to_lowercase();
                state = state.add_phase_tokens(&phase_name, tokens_used);
            }
            // Now fire-and-forget (SaveState will include token counts)
            self.execute_fire_and_forget_with_state(&actions, &state).await;
            event = evt;
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
                let token_summary = format_phase_tokens(&state.phase_tokens);
                format!(
                    "✅ **Ritual complete!**\n\n\
                     📋 Task: {}\n\
                     ⏱ Duration: {}\n\
                     🔄 Transitions: {}\n\
                     {}\
                     {}",
                    truncate(&state.task, 200),
                    duration_str,
                    phases_traversed,
                    if state.verify_retries > 0 {
                        format!("🔧 Verify retries: {}\n", state.verify_retries)
                    } else {
                        String::new()
                    },
                    token_summary,
                )
            }
            gid_core::ritual::V2Phase::Escalated => {
                let error = state.error_context.as_deref().unwrap_or("unknown error");
                let failed_phase = state.failed_phase.as_ref()
                    .map(|p| p.display_name())
                    .unwrap_or("unknown");
                let token_summary = format_phase_tokens(&state.phase_tokens);
                format!(
                    "🚨 **Ritual escalated — needs human intervention**\n\n\
                     📋 Task: {}\n\
                     💥 Failed in: {} phase\n\
                     ❌ Error: {}\n\
                     ⏱ Duration: {}\n\
                     🔄 Transitions: {}\n\
                     {}\n\n\
                     **Next steps:**\n\
                     • `/ritual retry` — retry the failed phase\n\
                     • `/ritual skip` — skip to the next phase\n\
                     • `/ritual cancel` — abort the ritual",
                    truncate(&state.task, 200),
                    failed_phase,
                    truncate(error, 300),
                    duration_str,
                    phases_traversed,
                    token_summary,
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
    /// Returns (event, tokens_used) — tokens_used is 0 for non-LLM actions.
    async fn execute_event_producing(&self, actions: &[RitualAction], state: &RitualState) -> Result<(RitualEvent, u64)> {
        for action in actions {
            match action {
                RitualAction::DetectProject => {
                    return self.detect_project().await.map(|e| (e, 0));
                }
                RitualAction::RunSkill { name, context } => {
                    return self.run_skill(name, context).await;
                }
                RitualAction::RunShell { command } => {
                    return self.run_shell(command).await.map(|e| (e, 0));
                }
                RitualAction::RunTriage { task } => {
                    return self.run_triage(task, state).await;
                }
                RitualAction::RunPlanning => {
                    return self.run_planning().await;
                }
                RitualAction::RunHarness { tasks } => {
                    return self.run_harness(tasks).await;
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

    /// Run triage — lightweight haiku LLM call to assess task clarity/size.
    async fn run_triage(&self, task: &str, ritual_state: &RitualState) -> Result<(RitualEvent, u64)> {
        use gid_core::ritual::TriageResult;

        // Build project context for triage prompt
        let project_ctx = if let Some(ps) = ritual_state.project.as_ref() {
            format!(
                "Project: lang={}, has_design={}, has_graph={}, source_files={}, has_tests={}",
                ps.language.as_deref().unwrap_or("unknown"),
                ps.has_design, ps.has_graph,
                ps.source_file_count, ps.has_tests
            )
        } else {
            "Project: unknown state".into()
        };

        let prompt = format!(
            r#"You are a triage agent. Assess this development task quickly.

{project_ctx}

Task: "{task}"

Respond with ONLY a JSON object (no markdown, no explanation):
{{
  "clarity": "clear" or "ambiguous",
  "clarify_questions": ["question1", ...] (only if ambiguous, otherwise empty array),
  "size": "small", "medium", or "large",
  "skip_design": true/false,
  "skip_graph": true/false
}}

Guidelines:
- "small": bug fix, add a simple command, change a config value, rename something
- "medium": add a feature that touches 2-3 files, refactor a module
- "large": new subsystem, architectural change, multi-file feature
- skip_design=true if the task is small enough that a DESIGN.md update adds no value
- skip_graph=true if the task doesn't add new architectural nodes/edges
- "ambiguous" if the task description is vague, could mean multiple things, or lacks critical info
- Short ≠ simple. "fix the bug" is ambiguous. "fix the auth retry loop in llm.rs" is clear and small."#
        );

        // Use haiku for triage (cheap, fast)
        let model = "haiku";
        tracing::info!(task = task, model = model, "Running triage");

        let llm = self.llm_client.read().await;
        let messages = vec![crate::llm::Message::text("user", &prompt)];
        match llm.chat_with_model("You are a triage agent.", &messages, &[], model).await {
            Ok(response) => {
                let response_text = response.text.clone().unwrap_or_default();
                let tokens_used = (response.usage.input_tokens + response.usage.output_tokens) as u64;
                
                // Try to parse JSON from response
                let json_str = Self::extract_json_str(&response_text);
                match serde_json::from_str::<TriageResult>(json_str) {
                    Ok(result) => {
                        tracing::info!(
                            clarity = %result.clarity,
                            size = %result.size,
                            skip_design = result.skip_design,
                            skip_graph = result.skip_graph,
                            "Triage complete"
                        );
                        Ok((RitualEvent::TriageCompleted(result), tokens_used))
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse triage JSON: {}. Response: {}. Defaulting to full flow.", e, &response_text[..response_text.len().min(200)]);
                        Ok((RitualEvent::TriageCompleted(TriageResult {
                            clarity: "clear".into(),
                            clarify_questions: vec![],
                            size: "large".into(),
                            skip_design: false,
                            skip_graph: false,
                        }), tokens_used))
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Triage LLM call failed: {}. Defaulting to full flow.", e);
                Ok((RitualEvent::TriageCompleted(TriageResult {
                    clarity: "clear".into(),
                    clarify_questions: vec![],
                    size: "large".into(),
                    skip_design: false,
                    skip_graph: false,
                }), 0))
            }
        }
    }

    /// Extract JSON from LLM output (handles markdown code fences).
    fn extract_json_str(output: &str) -> &str {
        if let Some(start) = output.find("```json") {
            let json_start = start + 7;
            if let Some(end) = output[json_start..].find("```") {
                return output[json_start..json_start + end].trim();
            }
        }
        if let Some(start) = output.find("```") {
            let json_start = start + 3;
            if let Some(end) = output[json_start..].find("```") {
                return output[json_start..json_start + end].trim();
            }
        }
        if let Some(start) = output.find('{') {
            if let Some(end) = output.rfind('}') {
                return &output[start..=end];
            }
        }
        output.trim()
    }

    /// Run a skill phase using the RitualLlmAdapter.
    async fn run_skill(&self, name: &str, context: &str) -> Result<(RitualEvent, u64)> {
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

        // implement phase benefits from stronger model; others use sonnet
        let model = match name {
            "implement" => "opus",
            _ => "sonnet",
        };

        let result = gid_client.run_skill(
            &skill_prompt,
            tools,
            model,
            &self.project_root,
        ).await;

        match result {
            Ok(skill_result) => {
                let tokens = skill_result.tokens_used;
                tracing::info!(
                    "Skill '{}' completed: {} tool calls, {} tokens",
                    name, skill_result.tool_calls_made, tokens
                );
                Ok((RitualEvent::SkillCompleted {
                    phase: name.to_string(),
                    artifacts: skill_result.artifacts_created.iter().map(|p| p.display().to_string()).collect(),
                }, tokens))
            }
            Err(e) => {
                tracing::error!("Skill '{}' failed: {}", name, e);
                Ok((RitualEvent::SkillFailed {
                    phase: name.to_string(),
                    error: format!("{}", e),
                }, 0))
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

    /// Run multiple implementation tasks in parallel (multi-agent harness).
    /// Each task gets its own LLM session via run_skill("implement", task).
    /// Results are collected: all succeed → SkillCompleted, any fail → SkillFailed.
    async fn run_harness(&self, tasks: &[String]) -> Result<(RitualEvent, u64)> {
        tracing::info!(task_count = tasks.len(), "Running harness ({} parallel tasks)", tasks.len());

        if tasks.is_empty() {
            return Ok((RitualEvent::SkillCompleted {
                phase: "implement".into(),
                artifacts: vec![],
            }, 0));
        }

        // For single task, just run directly
        if tasks.len() == 1 {
            return self.run_skill("implement", &tasks[0]).await;
        }

        // Run tasks concurrently using tokio::spawn
        // Each gets its own RitualLlmAdapter (separate LLM session)
        let mut handles = Vec::new();
        for (i, task) in tasks.iter().enumerate() {
            let task_ctx = format!(
                "Task {}/{}: {}\n\nIMPORTANT: Only implement THIS specific task. \
                 Other tasks are being handled in parallel by other agents.",
                i + 1, tasks.len(), task
            );
            let llm = self.llm_client.clone();
            let project_root = self.project_root.clone();

            handles.push(tokio::spawn(async move {
                let adapter = crate::ritual_adapter::RitualLlmAdapter::new(llm);
                use gid_core::ritual::llm::{LlmClient as GidLlmClient, ToolDefinition};

                // Build tools (Read, Write, Edit, Bash)
                let tools = vec![
                    ToolDefinition::new("Read", "Read a file", serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    })),
                    ToolDefinition::new("Write", "Write content to a file", serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
                        "required": ["path", "content"]
                    })),
                    ToolDefinition::new("Edit", "Replace exact text in a file", serde_json::json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}, "old_text": {"type": "string"}, "new_text": {"type": "string"}},
                        "required": ["path", "old_text", "new_text"]
                    })),
                    ToolDefinition::new("Bash", "Run a shell command", serde_json::json!({
                        "type": "object",
                        "properties": {"command": {"type": "string"}},
                        "required": ["command"]
                    })),
                ];

                match adapter.run_skill(&task_ctx, tools, "opus", &project_root).await {
                    Ok(result) => Ok((i, result)),
                    Err(e) => Err((i, e.to_string())),
                }
            }));
        }

        // Collect results
        let mut total_tokens = 0u64;
        let mut all_artifacts = Vec::new();
        let mut failures = Vec::new();

        for handle in handles {
            match handle.await {
                Ok(Ok((i, skill_result))) => {
                    tracing::info!(task_idx = i, "Harness task {} completed", i + 1);
                    total_tokens += skill_result.tokens_used;
                    all_artifacts.extend(
                        skill_result.artifacts_created.into_iter()
                            .map(|p| p.to_string_lossy().to_string())
                    );
                }
                Ok(Err((i, error))) => {
                    tracing::warn!(task_idx = i, error = %error, "Harness task {} failed", i + 1);
                    failures.push(format!("Task {}: {}", i + 1, error));
                }
                Err(join_err) => {
                    tracing::error!("Harness task panicked: {}", join_err);
                    failures.push(format!("Task panicked: {}", join_err));
                }
            }
        }

        if failures.is_empty() {
            tracing::info!(
                total_tokens = total_tokens,
                artifacts = all_artifacts.len(),
                "All {} harness tasks completed successfully", tasks.len()
            );
            Ok((RitualEvent::SkillCompleted {
                phase: "implement".into(),
                artifacts: all_artifacts,
            }, total_tokens))
        } else {
            let error_msg = format!(
                "{}/{} tasks failed:\n{}",
                failures.len(), tasks.len(),
                failures.join("\n")
            );
            tracing::warn!("{}", error_msg);
            Ok((RitualEvent::SkillFailed {
                phase: "implement".into(),
                error: error_msg,
            }, total_tokens))
        }
    }

    async fn run_shell(&self, command: &str) -> Result<RitualEvent> {
        tracing::info!("Running shell command: {}", command);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(300),
            tokio::process::Command::new("bash")
                .arg("-lc")
                .arg(command)
                .current_dir(&self.project_root)
                .output()
        ).await
            .map_err(|_| anyhow::anyhow!("Shell command timed out after 5 minutes: {}", command))?
            ?;

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
    async fn run_planning(&self) -> Result<(RitualEvent, u64)> {
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
                let tokens = skill_result.tokens_used;
                // Try to parse the LLM output as a strategy decision
                let strategy = parse_strategy(&skill_result.output);
                tracing::info!("Planning decided strategy: {:?}", strategy);
                Ok((RitualEvent::PlanDecided(strategy), tokens))
            }
            Err(e) => {
                tracing::warn!("Planning failed, defaulting to SingleLlm: {}", e);
                Ok((RitualEvent::PlanDecided(ImplementStrategy::SingleLlm), 0))
            }
        }
    }

    /// Update graph node matching the task description — mark as done.
    /// Uses fuzzy title/description matching, best-effort (errors logged, never crashes ritual).
    async fn update_graph(&self, description: &str) {
        use gid_core::graph::{Graph, NodeStatus};

        let graph_path = self.project_root.join(".gid/graph.yml");
        if !graph_path.exists() {
            tracing::debug!("No graph.yml found, skipping UpdateGraph");
            return;
        }

        let content = match std::fs::read_to_string(&graph_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read graph.yml: {}", e);
                return;
            }
        };
        let mut graph: Graph = match serde_yaml::from_str(&content) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("Failed to parse graph.yml: {}", e);
                return;
            }
        };

        // Fuzzy match: find node whose title/description contains the task text (or vice versa)
        let desc_lower = description.to_lowercase();
        let matched_id = graph.nodes.iter()
            .filter(|n| matches!(n.status, NodeStatus::Todo | NodeStatus::InProgress))
            .find(|n| {
                let title_lower = n.title.to_lowercase();
                let node_desc = n.description.as_deref().unwrap_or("").to_lowercase();
                desc_lower.contains(&title_lower)
                    || title_lower.contains(&desc_lower)
                    || (!node_desc.is_empty() && (
                        desc_lower.contains(&node_desc)
                        || node_desc.contains(&desc_lower)
                    ))
            })
            .map(|n| n.id.clone());

        if let Some(id) = matched_id {
            if graph.mark_task_done(&id) {
                match serde_yaml::to_string(&graph) {
                    Ok(yaml) => {
                        if let Err(e) = std::fs::write(&graph_path, &yaml) {
                            tracing::warn!("Failed to write graph.yml: {}", e);
                        } else {
                            tracing::info!(node_id = %id, "Marked graph node as done");
                        }
                    }
                    Err(e) => tracing::warn!("Failed to serialize graph: {}", e),
                }
            }
        } else {
            tracing::info!(description = %truncate(description, 100), "No matching graph node found");
        }
    }

    /// Clean up ritual artifacts.
    async fn cleanup(&self) {
        // Note: we keep the ritual state file as a record (terminal state).
        // Legacy state file cleanup
        if self.legacy_state_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&self.legacy_state_path).await {
                tracing::warn!("Failed to remove legacy ritual-state.json: {}", e);
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

/// Format per-phase token usage into a readable string.
fn format_phase_tokens(phase_tokens: &std::collections::HashMap<String, u64>) -> String {
    if phase_tokens.is_empty() {
        return String::new();
    }
    // Sort phases in a logical order
    let phase_order = ["initializing", "design", "planning", "graph", "implement", "verify"];
    let mut entries: Vec<(&str, u64)> = phase_tokens
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    entries.sort_by_key(|(k, _)| {
        phase_order.iter().position(|p| p == k).unwrap_or(99)
    });

    let lines: Vec<String> = entries
        .iter()
        .map(|(phase, tokens)| format!("  {} → {}", phase, format_tokens(*tokens)))
        .collect();
    let total: u64 = phase_tokens.values().sum();
    format!("🪙 Tokens: {} total\n{}", format_tokens(total), lines.join("\n"))
}

/// Format token count with K/M suffix for readability.
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
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
