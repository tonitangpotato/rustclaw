//! Ritual Runner — bridges gid-core's v2 pure function state machine with RustClaw's infrastructure.
//!
//! Drives the transition loop: transition(state, event) → (new_state, actions),
//! executing each action via the appropriate executor (LLM, shell, filesystem, Telegram).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::Result;
use gid_core::ritual::{
    V2Phase as RitualPhase,
    V2State as RitualState, V2Event as RitualEvent,
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

/// Shared registry for sending events to paused rituals waiting in-loop.
/// Key: ritual ID, Value: oneshot Sender for the resume event.
pub type EventRegistry = Arc<std::sync::Mutex<std::collections::HashMap<String, tokio::sync::mpsc::Sender<RitualEvent>>>>;

/// Default timeout (seconds) before auto-approving a paused ritual.
const AUTO_APPROVE_TIMEOUT_SECS: u64 = 180; // 3 minutes — auto-apply all findings if no user response

/// Create a new empty cancel registry.
pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Create a new empty event registry.
pub fn new_event_registry() -> EventRegistry {
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
    event_registry: EventRegistry,
    /// Optional AgentRunner for running skill phases as isolated sub-agents.
    /// When set, each phase gets its own session with auto-compact, persist, etc.
    agent_runner: Option<Arc<crate::agent::AgentRunner>>,
}

impl RitualRunner {
    pub fn new(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
    ) -> Self {
        Self::with_registries(project_root, llm_client, notify, new_cancel_registry(), new_event_registry())
    }

    pub fn with_cancel_registry(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
        cancel_registry: CancelRegistry,
    ) -> Self {
        Self::with_registries(project_root, llm_client, notify, cancel_registry, new_event_registry())
    }

    pub fn with_registries(
        project_root: PathBuf,
        llm_client: Arc<tokio::sync::RwLock<Box<dyn crate::llm::LlmClient>>>,
        notify: NotifyFn,
        cancel_registry: CancelRegistry,
        event_registry: EventRegistry,
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
            event_registry,
            agent_runner: None,
        }
    }

    /// Set the AgentRunner for sub-agent-based phase execution.
    pub fn with_agent_runner(mut self, runner: Arc<crate::agent::AgentRunner>) -> Self {
        self.agent_runner = Some(runner);
        self
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
    /// Prioritizes rituals in Waiting* phases (user interaction needed) over
    /// rituals in other active phases (running in background).
    fn find_latest_active(&self) -> Result<Option<RitualState>> {
        let rituals = self.list_rituals()?;
        // First: look for a ritual waiting for user input (most likely what user is responding to)
        let waiting = rituals.iter().find(|r| r.phase.is_paused());
        if let Some(r) = waiting {
            return Ok(Some(r.clone()));
        }
        // Second: any non-terminal, non-idle ritual
        Ok(rituals.into_iter().find(|r| {
            !r.phase.is_terminal() && r.phase != gid_core::ritual::V2Phase::Idle
        }))
    }

    /// Find the ritual currently waiting for approval, if any.
    pub fn find_waiting_approval(&self) -> Result<Option<RitualState>> {
        let rituals = self.list_rituals()?;
        Ok(rituals.into_iter().find(|r| {
            r.phase == gid_core::ritual::V2Phase::WaitingApproval
        }))
    }

    /// Get the target project root for a ritual.
    /// Returns state.target_root if set, otherwise self.project_root.
    fn target_root_for(&self, state: &RitualState) -> PathBuf {
        state.target_root.as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.project_root.clone())
    }

    /// Save state to disk (uses ritual ID for path).
    /// Also stamps `adapter_pid` to the current process so the main agent's
    /// `RitualRegistry` can detect whether it is the executor of a SingleLlm ritual.
    pub fn save_state(&self, state: &RitualState) -> Result<()> {
        std::fs::create_dir_all(&self.rituals_dir)?;
        let path = self.state_path_for(&state.id);
        // Stamp the current process PID without mutating the caller's state.
        let mut stamped = state.clone();
        stamped.adapter_pid = Some(std::process::id());
        let data = serde_json::to_string_pretty(&stamped)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Start a new ritual with a task description.
    /// Extracts target project root from the task text if present (e.g., "Project location: /path/to/project").
    /// Otherwise uses the runner's workspace root.
    ///
    /// **Deprecated for new callers** — prefer [`start_with_work_unit`] which derives
    /// the project root from a structured `WorkUnit` via the gid-core project registry
    /// (see rustclaw ISS-022 / gid-rs ISS-029). This path uses text-grep inheritance
    /// and is kept only for the `/ritual` Telegram command where the user has already
    /// selected a project interactively.
    pub async fn start(&self, task: String) -> Result<RitualState> {
        let target_root = extract_target_project_dir(&task)
            .unwrap_or_else(|| self.project_root.clone());
        let state = RitualState::new()
            .with_target_root(target_root.to_string_lossy().to_string());
        tracing::info!(
            ritual_id = %state.id,
            target_root = %target_root.display(),
            task = %truncate(&task, 80),
            "Starting new ritual"
        );
        self.save_state(&state)?;
        self.advance(&state.id, RitualEvent::Start { task }).await
    }

    /// Start a new ritual from a structured [`WorkUnit`] (rustclaw ISS-022).
    ///
    /// The project root is resolved via gid-core's project registry and validated
    /// before the ritual starts — failure modes are explicit and loud, no silent
    /// fallback to a default workspace (which was the ISS-027 root cause).
    ///
    /// The `WorkUnit` is persisted on `RitualState.work_unit` so downstream phases,
    /// resume paths, and state-file round-trips know which issue/feature/task is
    /// the target of work.
    pub async fn start_with_work_unit(
        &self,
        unit: gid_core::ritual::work_unit::WorkUnit,
        task: String,
    ) -> Result<RitualState> {
        use gid_core::ritual::work_unit::{resolve_and_validate, RegistryResolver};

        let resolver = RegistryResolver::load_default()
            .map_err(|e| anyhow::anyhow!(
                "failed to load project registry (~/.config/gid/projects.yml): {e}"
            ))?;
        let resolved_root = resolve_and_validate(&resolver, &unit).map_err(|e| {
            anyhow::anyhow!("failed to resolve work unit '{}': {e}", unit.label())
        })?;

        let state = RitualState::new().with_work_unit(unit.clone(), resolved_root.clone());
        tracing::info!(
            ritual_id = %state.id,
            work_unit = %unit.label(),
            target_root = %resolved_root.display(),
            task = %truncate(&task, 80),
            "Starting new ritual (from work_unit)"
        );
        self.save_state(&state)?;
        self.advance(&state.id, RitualEvent::Start { task }).await
    }

    /// Cancel a running ritual by ID (triggers cancellation token).
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

    /// Send a user event to a specific ritual by ID.
    pub async fn send_event_to(&self, ritual_id: &str, event: RitualEvent) -> Result<RitualState> {
        // For UserCancel, also trigger cancellation of any running action
        if matches!(&event, RitualEvent::UserCancel) {
            self.cancel_running(ritual_id);
        }
        self.advance(ritual_id, event).await
    }

    /// Send a user event to the most relevant active ritual.
    pub async fn send_event(&self, event: RitualEvent) -> Result<RitualState> {
        let state = self.load_state()?;
        self.send_event_to(&state.id, event).await
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Core: advance — the single entry point for all ritual state transitions
    // ═══════════════════════════════════════════════════════════════════════

    /// Advance a ritual by one step: transition(state, event) → execute actions.
    ///
    /// This is the entire ritual execution model:
    /// 1. Load state from disk
    /// 2. Run the pure transition function
    /// 3. Save new state to disk immediately
    /// 4. Execute fire-and-forget actions (notify, cleanup)
    /// 5. If terminal/paused: return (user calls advance again when ready)
    /// 6. If event-producing action: spawn it in background, which calls advance again on completion
    ///
    /// No loop. No channels. No timeout. State lives on disk.
    /// Process can restart at any time — just call advance again.
    pub async fn advance(&self, ritual_id: &str, event: RitualEvent) -> Result<RitualState> {
        let state = self.load_state_by_id(ritual_id)?;

        tracing::info!(
            ritual_id = %ritual_id,
            from_phase = %state.phase.display_name(),
            event = ?format!("{:?}", &event).chars().take(80).collect::<String>(),
            "Advancing ritual"
        );

        // Guard: don't advance a terminal ritual (except via UserRetry from Escalated)
        if state.phase.is_terminal() && !matches!(&event, RitualEvent::UserRetry | RitualEvent::UserCancel) {
            tracing::warn!(ritual_id = %ritual_id, phase = %state.phase.display_name(), "Cannot advance terminal ritual");
            return Ok(state);
        }

        // Pure transition
        let (new_state, actions) = transition(&state, event);

        // Save immediately — even if action execution crashes, state is correct on disk
        self.save_state(&new_state)?;

        // Send notifications
        for action in &actions {
            if let RitualAction::Notify { message } = action {
                if new_state.phase == RitualPhase::WaitingApproval {
                    self.fire_and_forget_notify(self.enrich_review_notification(message, &new_state));
                } else {
                    self.fire_and_forget_notify(message.clone());
                }
            }
        }

        // Terminal: execute cleanup actions, send summary, done
        if new_state.phase.is_terminal() {
            self.execute_fire_and_forget_with_state(&actions, &new_state).await;
            self.send_terminal_notification(&new_state).await;
            return Ok(new_state);
        }

        // Paused (WaitingApproval, WaitingClarification): save and return.
        // For WaitingApproval: spawn auto-approve timer (applies "all" after timeout).
        if new_state.phase.is_paused() {
            self.execute_fire_and_forget_with_state(&actions, &new_state).await;

            // Auto-approve timer for WaitingApproval
            if new_state.phase == RitualPhase::WaitingApproval {
                self.spawn_auto_approve_timer(ritual_id, &new_state);
            }

            tracing::info!(
                ritual_id = %ritual_id,
                phase = %new_state.phase.display_name(),
                "Ritual paused — waiting for user input"
            );
            return Ok(new_state);
        }

        // Active phase: spawn the event-producing action in background.
        // When it completes, it calls advance() again with the result event.
        self.execute_fire_and_forget_with_state(&actions, &new_state).await;
        self.spawn_event_producing_action(ritual_id, &actions, &new_state);

        Ok(new_state)
    }

    /// Spawn the event-producing action (RunSkill, RunTriage, etc.) in background.
    /// On completion, recursively calls advance() with the result event.
    fn spawn_event_producing_action(
        &self,
        ritual_id: &str,
        actions: &[RitualAction],
        state: &RitualState,
    ) {
        // Find the event-producing action
        let ep_action = match actions.iter().find(|a| a.is_event_producing()) {
            Some(a) => a.clone(),
            None => {
                tracing::error!(ritual_id = %ritual_id, "No event-producing action in non-paused transition");
                return;
            }
        };

        let ritual_id = ritual_id.to_string();
        let state = state.clone();

        // Clone everything the spawned task needs (self is not Send, so we clone fields).
        // Use the ritual's target_root so all operations run in the correct project directory.
        let project_root = self.target_root_for(&state);
        let rituals_dir = self.rituals_dir.clone();
        let legacy_state_path = self.legacy_state_path.clone();
        let llm_client = self.llm_client.clone();
        let notify = self.notify.clone();
        let cancel_registry = self.cancel_registry.clone();
        let event_registry = self.event_registry.clone();
        let agent_runner = self.agent_runner.clone();

        // Register cancellation token for this action
        let cancel_token = tokio_util::sync::CancellationToken::new();
        {
            let mut reg = self.cancel_registry.lock().unwrap();
            reg.insert(ritual_id.clone(), cancel_token.clone());
        }

        tokio::spawn(async move {
            // Rebuild a RitualRunner inside the spawned task
            let runner = RitualRunner {
                project_root,
                rituals_dir,
                legacy_state_path,
                llm_client,
                notify,
                cancel_registry: cancel_registry.clone(),
                event_registry,
                agent_runner,
            };

            // Execute the action with cancellation support
            let (result_event, tokens_used) = tokio::select! {
                _ = cancel_token.cancelled() => {
                    tracing::info!(ritual_id = %ritual_id, "Action cancelled");
                    (RitualEvent::UserCancel, 0)
                }
                result = runner.execute_event_producing_single(&ep_action, &state, &cancel_token) => {
                    match result {
                        Ok(pair) => pair,
                        Err(e) => {
                            tracing::error!(ritual_id = %ritual_id, "Action execution error: {}", e);
                            (RitualEvent::SkillFailed {
                                phase: state.phase.display_name().to_string(),
                                error: format!("Executor error: {}", e),
                            }, 0)
                        }
                    }
                }
            };

            // Record tokens
            if tokens_used > 0 {
                if let Ok(mut current_state) = runner.load_state_by_id(&ritual_id) {
                    let phase_name = current_state.phase.display_name().to_lowercase();
                    current_state = current_state.add_phase_tokens(&phase_name, tokens_used);
                    let _ = runner.save_state(&current_state);
                }
            }

            // Cleanup cancellation token
            {
                let mut reg = cancel_registry.lock().unwrap();
                reg.remove(&ritual_id);
            }

            // Recurse: advance with the result event
            if let Err(e) = runner.advance(&ritual_id, result_event).await {
                tracing::error!(ritual_id = %ritual_id, "Advance after action failed: {}", e);
            }
        });
    }

    /// Spawn a background timer that auto-approves with "all" after AUTO_APPROVE_TIMEOUT_SECS.
    /// If the user responds before timeout, the state will have moved past WaitingApproval
    /// and the timer's advance() call will be a no-op (catch-all escalation is guarded).
    fn spawn_auto_approve_timer(&self, ritual_id: &str, _state: &RitualState) {
        let ritual_id = ritual_id.to_string();
        let project_root = self.project_root.clone();
        let rituals_dir = self.rituals_dir.clone();
        let legacy_state_path = self.legacy_state_path.clone();
        let llm_client = self.llm_client.clone();
        let notify = self.notify.clone();
        let cancel_registry = self.cancel_registry.clone();
        let event_registry = self.event_registry.clone();
        let agent_runner = self.agent_runner.clone();
        let timeout = std::time::Duration::from_secs(AUTO_APPROVE_TIMEOUT_SECS);

        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;

            let runner = RitualRunner {
                project_root,
                rituals_dir,
                legacy_state_path,
                llm_client,
                notify: notify.clone(),
                cancel_registry,
                event_registry,
                agent_runner,
            };

            // Re-check state from disk — user may have responded already
            match runner.load_state_by_id(&ritual_id) {
                Ok(current_state) => {
                    if current_state.phase == RitualPhase::WaitingApproval {
                        tracing::info!(
                            ritual_id = %ritual_id,
                            review_round = current_state.review_round,
                            "Auto-approve timer fired — applying all findings"
                        );
                        // Send notification
                        notify("⏰ Auto-applying all review findings (no response within 3 min)...".to_string()).await;
                        // Advance with apply all
                        if let Err(e) = runner.advance(
                            &ritual_id,
                            RitualEvent::UserApproval { approved: "all".into() },
                        ).await {
                            tracing::error!(ritual_id = %ritual_id, "Auto-approve advance failed: {}", e);
                        }
                    } else {
                        tracing::debug!(
                            ritual_id = %ritual_id,
                            phase = %current_state.phase.display_name(),
                            "Auto-approve timer fired but ritual already moved past WaitingApproval"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(ritual_id = %ritual_id, "Auto-approve timer: couldn't load state: {}", e);
                }
            }
        });
    }
    async fn execute_event_producing_single(
        &self,
        action: &RitualAction,
        state: &RitualState,
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        match action {
            RitualAction::DetectProject => {
                self.detect_project().await.map(|e| (e, 0))
            }
            RitualAction::RunSkill { name, context } => {
                self.run_skill(name, context, cancel_token).await
            }
            RitualAction::RunShell { command } => {
                self.run_shell(command).await.map(|e| (e, 0))
            }
            RitualAction::RunTriage { task } => {
                self.run_triage(task, state).await
            }
            RitualAction::RunPlanning => {
                self.run_planning().await
            }
            RitualAction::RunHarness { tasks } => {
                self.run_harness(tasks, cancel_token).await
            }
            _ => Err(anyhow::anyhow!("Not an event-producing action: {:?}", action)),
        }
    }

    /// Execute fire-and-forget actions (Notify, SaveState, UpdateGraph, Cleanup).
    /// Notify is truly fire-and-forget: spawned as a task, failures only logged.
    async fn execute_fire_and_forget_with_state(&self, actions: &[RitualAction], state: &RitualState) {
        for action in actions {
            match action {
                RitualAction::Notify { .. } => {
                    // Notifications are sent BEFORE event-producing actions in run_loop.
                    // Skip here to avoid duplicate notifications.
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
                RitualAction::ApplyReview { approved } => {
                    // Fire-and-forget: use typed REVIEWER sub-agent to apply findings
                    let target_root = self.target_root_for(state);
                    if let Some(ref runner) = self.agent_runner {
                        let task = format!(
                            "Apply the approved review findings to the documents in {}.\n\
                             Approved findings: {}\n\
                             Read the review file from .gid/reviews/, read the full target document, \
                             and apply ONLY the approved changes using Edit tool.",
                            target_root.display(), approved
                        );
                        let options = crate::agent::SubAgentOptions {
                            workspace: Some(target_root.clone()),
                            ..Default::default()
                        };
                        let sub_result = runner.run_subagent(&crate::agent::AgentType::REVIEWER, &task, options).await;
                        if sub_result.outcome.is_success() {
                            tracing::info!("ApplyReview completed: {}", truncate(&sub_result.output, 200));
                        } else {
                            tracing::error!("ApplyReview failed: {}", sub_result.outcome.display());
                        }
                    } else {
                        tracing::warn!("ApplyReview skipped — no AgentRunner available");
                    }
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

    /// Enrich the WaitingApproval notification with actual review findings from .gid/reviews/.
    fn enrich_review_notification(&self, original_msg: &str, state: &RitualState) -> String {
        // Find the most recent review file, skipping SUMMARY.md which is written last
        // and contains no FINDING- entries.
        let reviews_dir = self.target_root_for(state).join(".gid/reviews");
        let latest_review = match std::fs::read_dir(&reviews_dir) {
            Ok(entries) => {
                let mut files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        let name = e.file_name().to_string_lossy().to_lowercase();
                        name.ends_with(".md") && !name.starts_with("summary")
                    })
                    .collect();
                files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));
                files.first().map(|e| e.path())
            }
            Err(_) => None,
        };

        let review_path = match latest_review {
            Some(p) => p,
            None => return original_msg.to_string(),
        };

        let content = match std::fs::read_to_string(&review_path) {
            Ok(c) => c,
            Err(_) => return original_msg.to_string(),
        };

        // Parse findings: lines starting with "### FINDING-"
        // Capture the first non-blank, non-bold-label body line as the issue summary.
        let mut findings = Vec::new();
        let mut current_finding: Option<(String, String)> = None; // (header, issue)

        for line in content.lines() {
            if line.starts_with("### FINDING-") {
                if let Some(f) = current_finding.take() {
                    findings.push(f);
                }
                current_finding = Some((line.trim_start_matches("### ").to_string(), String::new()));
            } else if let Some((_, ref mut issue)) = current_finding {
                if issue.is_empty() {
                    // Skip blank lines and bold-label lines (e.g. **Affected:**, **Suggested fix:**)
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with("**") && !trimmed.starts_with("---") && !trimmed.starts_with("```") {
                        *issue = trimmed.chars().take(120).collect::<String>();
                        if trimmed.len() > 120 {
                            issue.push_str("...");
                        }
                    }
                }
            }
        }
        if let Some(f) = current_finding.take() {
            findings.push(f);
        }

        if findings.is_empty() {
            return original_msg.to_string();
        }

        // Format enriched notification
        let file_name = review_path.file_name().unwrap_or_default().to_string_lossy();
        let mut msg = format!("📋 Review complete ({}):\n\n", file_name);
        for (header, issue) in &findings {
            msg.push_str(&format!("  {}\n", header));
            if !issue.is_empty() {
                msg.push_str(&format!("    → {}\n", issue));
            }
        }
        msg.push_str(&format!("\n{} finding(s). Reply: 'apply all' / 'apply 1,3' / 'skip'", findings.len()));
        msg
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
    async fn execute_event_producing(
        &self,
        actions: &[RitualAction],
        state: &RitualState,
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        for action in actions {
            match action {
                RitualAction::DetectProject => {
                    return self.detect_project().await.map(|e| (e, 0));
                }
                RitualAction::RunSkill { name, context } => {
                    return self.run_skill(name, context, cancel_token).await;
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
                    return self.run_harness(tasks, cancel_token).await;
                }
                _ => {} // Fire-and-forget handled above
            }
        }
        Err(anyhow::anyhow!("No event-producing action found in actions"))
    }

    /// Scan filesystem to detect project state.
    /// Uses self.project_root — caller should ensure this points to the right project.
    /// In the advance model, detect_project is called from execute_event_producing_single
    /// which runs inside a spawned RitualRunner with the correct project_root.
    async fn detect_project(&self) -> Result<RitualEvent> {
        let root = &self.project_root;

        let has_requirements = root.join("REQUIREMENTS.md").exists()
            || root.join(".gid/requirements.md").exists()
            || root.join(".gid").is_dir() && std::fs::read_dir(root.join(".gid"))
                .map(|entries| entries
                    .filter_map(|e| e.ok())
                    .any(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.starts_with("requirements-") && name.ends_with(".md")
                    }))
                .unwrap_or(false)
            // Multi-doc: feature-level requirements under .gid/features/
            || root.join(".gid/features").is_dir() && std::fs::read_dir(root.join(".gid/features"))
                .map(|entries| entries
                    .filter_map(|e| e.ok())
                    .any(|e| {
                        e.path().join("requirements.md").exists()
                    }))
                .unwrap_or(false);

        let has_design = root.join("DESIGN.md").exists()
            || root.join(".gid/DESIGN.md").exists()
            || root.join(".gid/design.md").exists()
            // Multi-doc: feature-level design docs
            || root.join(".gid/features").is_dir() && std::fs::read_dir(root.join(".gid/features"))
                .map(|entries| entries
                    .filter_map(|e| e.ok())
                    .any(|e| {
                        e.path().join("design.md").exists()
                    }))
                .unwrap_or(false);

        let has_graph = root.join(".gid/graph.yml").exists()
            || root.join("graph.yml").exists();

        let has_cargo = root.join("Cargo.toml").exists();
        let has_package_json = root.join("package.json").exists();
        let has_pyproject = root.join("pyproject.toml").exists();

        let has_source = has_source_in_project(root);

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

        // Count source files — uses workspace members from Cargo.toml/package.json
        let source_file_count = count_source_files_in_project(root).await;

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
            has_requirements,
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

        // Build project context for triage prompt (single source of truth in gid-core)
        let project_ctx = if let Some(ps) = ritual_state.project.as_ref() {
            format!(
                "Project: lang={}, has_req={}, has_design={}, has_graph={}, source_files={}, has_tests={}",
                ps.language.as_deref().unwrap_or("unknown"),
                ps.has_requirements, ps.has_design, ps.has_graph,
                ps.source_file_count, ps.has_tests
            )
        } else {
            "Project: unknown state".into()
        };

        let prompt = gid_core::ritual::build_triage_prompt(task, &project_ctx);

        // Use haiku for triage (cheap, fast)
        let model = "claude-haiku-4-5-20251001";
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
                    Ok(mut result) => {
                        // Deterministic override: if design already exists, skip design phase
                        // regardless of what Haiku says. LLM triage is advisory, not authoritative
                        // for facts we can verify deterministically.
                        if let Some(ps) = &ritual_state.project {
                            if ps.has_design && !result.skip_design {
                                tracing::info!("Override: skip_design=true (design already exists)");
                                result.skip_design = true;
                            }
                        }

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
                        tracing::warn!("Failed to parse triage JSON: {}. Response: {}. Defaulting to full flow.", e, &response_text[..response_text.floor_char_boundary(200)]);
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

    /// Run a skill phase. Uses typed sub-agent via run_subagent when AgentRunner is available.
    /// Falls back to direct RitualLlmAdapter when AgentRunner is None (testing, standalone).
    async fn run_skill(
        &self,
        name: &str,
        context: &str,
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        // V2: all phases use typed sub-agents when AgentRunner is available
        if let Some(ref runner) = self.agent_runner {
            let agent_type = match name {
                "implement" | "execute-tasks" => &crate::agent::AgentType::CODER,
                "review-design" | "review-requirements" | "review-tasks" => &crate::agent::AgentType::REVIEWER,
                "draft-design" | "update-design" => &crate::agent::AgentType::PLANNER,
                _ => &crate::agent::AgentType::CODER,
            };

            let phase_context = match name {
                "implement" | "execute-tasks" => build_implement_context(&self.project_root),
                n if n.starts_with("review-") => build_review_context(name, &self.project_root),
                _ => vec![],
            };

            // Map phase name → skill name for SkillRegistry injection
            let skill_name = match name {
                "implement" | "execute-tasks" => Some("implement"),
                n if n.starts_with("review-") => Some(n),
                "draft-design" | "update-design" => Some(name),
                _ => None,
            };

            // implement/execute-tasks use the agent's current model; others use sonnet
            let model = match name {
                "implement" | "execute-tasks" => {
                    let client = self.llm_client.read().await;
                    Some(client.model_name().to_string())
                }
                _ => Some("claude-sonnet-4-5-20250929".to_string()),
            };

            let options = crate::agent::SubAgentOptions {
                workspace: Some(self.project_root.clone()),
                context: phase_context,
                skill: skill_name.map(String::from),
                model,
                ..Default::default()
            };

            const MAX_RATE_LIMIT_RETRIES: u32 = 3;
            let mut rate_limit_attempts = 0u32;

            let _sub_result = loop {
                let attempt_result = tokio::select! {
                    _ = cancel_token.cancelled() => {
                        return Ok((RitualEvent::SkillFailed {
                            phase: name.to_string(),
                            error: "Cancelled".to_string(),
                        }, 0));
                    }
                    r = runner.run_subagent(agent_type, context, options.clone()) => r,
                };

                use crate::agent::SubAgentOutcome;
                match &attempt_result.outcome {
                    // Success → return immediately
                    SubAgentOutcome::Completed => {
                        tracing::info!(
                            "Ritual phase '{}' completed via sub-agent ({} tokens, {} files)",
                            name, attempt_result.tokens, attempt_result.files_modified.len()
                        );
                        return Ok((RitualEvent::SkillCompleted {
                            phase: name.to_string(),
                            artifacts: attempt_result.files_modified,
                        }, attempt_result.tokens));
                    }

                    // Cancelled → propagate, don't fallback
                    SubAgentOutcome::Cancelled => {
                        return Ok((RitualEvent::SkillFailed {
                            phase: name.to_string(),
                            error: "Cancelled by user".to_string(),
                        }, attempt_result.tokens));
                    }

                    // Auth failed → bail out, fallback would hit the same auth wall
                    SubAgentOutcome::AuthFailed(msg) => {
                        tracing::error!("Ritual phase '{}' auth failed: {}", name, msg);
                        return Ok((RitualEvent::SkillFailed {
                            phase: name.to_string(),
                            error: format!("Authentication failed: {}", msg),
                        }, attempt_result.tokens));
                    }

                    // Rate limited → retry with exponential backoff (max 3 attempts)
                    SubAgentOutcome::RateLimited(msg) => {
                        rate_limit_attempts += 1;
                        if rate_limit_attempts > MAX_RATE_LIMIT_RETRIES {
                            tracing::warn!(
                                "Ritual phase '{}' rate limited {} times, falling back to direct execution",
                                name, rate_limit_attempts
                            );
                            (self.notify)(format!(
                                "⚠️ Rate limited {}x for '{}', falling back to direct execution...",
                                rate_limit_attempts, name
                            )).await;
                            break attempt_result;
                        }
                        let backoff_secs = 2u64.pow(rate_limit_attempts); // 2s, 4s, 8s
                        tracing::warn!(
                            "Ritual phase '{}' rate limited ({}), retry {}/{} in {}s",
                            name, msg, rate_limit_attempts, MAX_RATE_LIMIT_RETRIES, backoff_secs
                        );
                        (self.notify)(format!(
                            "⏳ Rate limited for '{}', retrying in {}s ({}/{})...",
                            name, backoff_secs, rate_limit_attempts, MAX_RATE_LIMIT_RETRIES
                        )).await;
                        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                        continue;
                    }

                    // MaxIterations, ContextTooLarge, Timeout, Error → fall through to direct LLM
                    outcome => {
                        tracing::warn!(
                            "Ritual phase '{}' sub-agent failed ({}), falling back to direct LLM execution",
                            name, outcome.display()
                        );
                        (self.notify)(format!(
                            "⚠️ Sub-agent failed for '{}' ({}), falling back to direct execution...",
                            name, outcome.display()
                        )).await;
                        break attempt_result;
                    }
                }
            };
        }

        // Fallback: direct execution via RitualLlmAdapter (no session management)
        use crate::ritual_adapter::RitualLlmAdapter;
        use gid_core::ritual::llm::{LlmClient as GidLlmClient, ToolDefinition};
        use gid_core::ritual::scope::default_scope_for_phase;

        let adapter = RitualLlmAdapter::new(self.llm_client.clone());
        let gid_client: Arc<dyn GidLlmClient> = adapter.into_arc();

        // Load skill-specific prompt (file-based, with built-in fallback)
        let base_prompt = self.load_skill_prompt(name);
        let skill_prompt = if context.is_empty() {
            base_prompt
        } else {
            format!("## USER TASK\n{}\n\n## INSTRUCTIONS\n{}", context, base_prompt)
        };

        // All available tool definitions
        let all_tools = vec![
            ("Read", ToolDefinition {
                name: "Read".into(),
                description: "Read a file from disk".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path relative to project root" }
                    },
                    "required": ["path"]
                }),
            }),
            ("Write", ToolDefinition {
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
            }),
            ("Edit", ToolDefinition {
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
            }),
            ("Bash", ToolDefinition {
                name: "Bash".into(),
                description: "Run a bash command".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Bash command to execute" }
                    },
                    "required": ["command"]
                }),
            }),
        ];

        // Filter tools by ToolScope for this phase (§5 of DESIGN-ritual-v2)
        let scope = default_scope_for_phase(name);
        let tools: Vec<ToolDefinition> = all_tools.into_iter()
            .filter(|(tool_name, _)| scope.allowed_tools.contains(&tool_name.to_string()))
            .map(|(_, def)| def)
            .collect();

        tracing::debug!(
            skill = name,
            tools = ?tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
            "ToolScope filtered tools for phase"
        );

        // implement phase benefits from stronger model; others use sonnet
        let model = match name {
            "implement" => "opus",
            _ => "sonnet",
        };

        let result = gid_client.run_skill(
            &skill_prompt,
            tools.clone(),
            model,
            &self.project_root,
            25,
        ).await;

        match result {
            Ok(skill_result) => {
                let mut total_tokens = skill_result.tokens_used;
                tracing::info!(
                    "Skill '{}' completed: {} tool calls, {} tokens",
                    name, skill_result.tool_calls_made, total_tokens
                );

                // Self-review loop: up to N rounds of auto-review after key phases.
                // Each round reads back output and checks for issues.
                // Stops when LLM responds with REVIEW_PASS or max rounds reached.
                let review_phases = ["implement", "execute-tasks", "draft-design", "update-design", "draft-requirements"];
                if review_phases.contains(&name) {
                    let max_reviews = 4;
                    for round in 1..=max_reviews {
                        if cancel_token.is_cancelled() {
                            tracing::info!(skill = name, "Self-review cancelled at round {}", round);
                            break;
                        }
                        let checklist = match name {
                            "draft-design" | "update-design" => "\
                             - Does the design actually solve the stated problem?\n\
                             - Are there missing components or interactions?\n\
                             - Are edge cases and error scenarios addressed?\n\
                             - Is the architecture over-engineered or under-engineered?\n\
                             - Are interfaces clear and well-defined?\n\
                             - Does it conflict with existing architecture?",
                            "draft-requirements" => "\
                             - Are requirements specific and testable (not vague)?\n\
                             - Are there missing requirements or unstated assumptions?\n\
                             - Are acceptance criteria measurable?\n\
                             - Do requirements conflict with each other?\n\
                             - Are non-functional requirements covered (perf, security)?",
                            _ => "\
                             - Logic errors and incorrect assumptions\n\
                             - Missing edge cases and error handling\n\
                             - Type mismatches and off-by-one errors\n\
                             - Unused imports or variables\n\
                             - Inconsistencies with the rest of the codebase",
                        };
                        let review_prompt = format!(
                            "## SELF-REVIEW ROUND {}/{}\n\n\
                             Read back ALL files you created or modified in the previous step. \
                             Carefully check for:\n{}\n\n\
                             If you find issues, fix them using the available tools.\n\
                             If everything looks correct after thorough review, respond with exactly: REVIEW_PASS",
                            round, max_reviews, checklist
                        );

                        tracing::info!("Implement self-review round {}/{}", round, max_reviews);
                        let review_result = gid_client.run_skill(
                            &review_prompt,
                            tools.clone(),
                            model,
                            &self.project_root,
                            25,
                        ).await;

                        match review_result {
                            Ok(review) => {
                                total_tokens += review.tokens_used;
                                let output = review.output.to_lowercase();
                                if output.contains("review_pass") {
                                    tracing::info!(
                                        "Self-review passed at round {}/{} ({} tokens used)",
                                        round, max_reviews, review.tokens_used
                                    );
                                    break;
                                }
                                tracing::info!(
                                    "Self-review round {} found issues — {} tool calls, {} tokens",
                                    round, review.tool_calls_made, review.tokens_used
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Self-review round {} failed: {} — continuing", round, e);
                                break;
                            }
                        }
                    }
                }

                Ok((RitualEvent::SkillCompleted {
                    phase: name.to_string(),
                    artifacts: skill_result.artifacts_created.iter().map(|p| p.display().to_string()).collect(),
                }, total_tokens))
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

    /// Resume a ritual from a specific phase, creating a new ritual with prerequisites check.
    /// Detects project state first, then validates that prerequisites for the target phase exist.
    pub async fn resume_from_phase(&self, task: String, phase: RitualPhase, target_root: Option<PathBuf>) -> Result<RitualState> {
        let root = target_root.unwrap_or_else(|| self.project_root.clone());

        // Check prerequisites for the target phase
        let missing = Self::check_prerequisites(&phase, &root);
        if !missing.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot resume from {} — missing prerequisites:\n• {}",
                phase.display_name(),
                missing.join("\n• ")
            ));
        }

        // Detect project state (needed for state machine decisions)
        let detect_event = self.detect_project().await?;
        let project_state = match &detect_event {
            RitualEvent::ProjectDetected(ps) => ps.clone(),
            _ => return Err(anyhow::anyhow!("Unexpected detect result")),
        };

        // Build the state at the PRECEDING phase, then send the event that transitions INTO the target.
        // This ensures the transition function matches correctly.
        let (preceding_phase, entry_event) = Self::build_phase_entry(&phase, &task, &project_state);

        let mut state = RitualState::new()
            .with_task(task.clone())
            .with_target_root(root.to_string_lossy().to_string())
            .with_project(project_state)
            .with_phase(preceding_phase);

        // Special state setup for phases that depend on review context
        if phase == RitualPhase::Planning {
            // Planning is entered after design review round 2 completes
            state = state.with_review_target("design").with_review_round(2);
        } else if phase == RitualPhase::Implementing {
            // When resuming to implement, we want to skip graph review
            // Set triage_size to medium so (graph_was_updated && !is_large) → skip review
            state.triage_size = Some("medium".into());
        }

        // Save the state
        self.save_state(&state)?;

        tracing::info!(
            ritual_id = %state.id,
            target_phase = %phase.display_name(),
            preceding_phase = %state.phase.display_name(),
            target_root = %root.display(),
            "Resuming ritual from phase"
        );

        // Advance with the entry event — this will transition into the target phase
        self.advance(&state.id, entry_event).await
    }

    /// Build the preceding phase and entry event for entering a target phase.
    /// Returns (preceding_phase, event) such that transition(preceding_phase, event) → target_phase.
    fn build_phase_entry(target: &RitualPhase, task: &str, project: &ProjectState) -> (RitualPhase, RitualEvent) {
        match target {
            RitualPhase::Initializing => (
                RitualPhase::Idle,
                RitualEvent::Start { task: task.to_string() },
            ),
            RitualPhase::Triaging => (
                RitualPhase::Initializing,
                RitualEvent::ProjectDetected(project.clone()),
            ),
            RitualPhase::WritingRequirements => (
                RitualPhase::Triaging,
                RitualEvent::TriageCompleted(gid_core::ritual::TriageResult {
                    clarity: "clear".into(),
                    clarify_questions: vec![],
                    size: "large".into(),
                    skip_design: false,
                    skip_graph: false,
                }),
            ),
            RitualPhase::Designing => (
                RitualPhase::Triaging,
                RitualEvent::TriageCompleted(gid_core::ritual::TriageResult {
                    clarity: "clear".into(),
                    clarify_questions: vec![],
                    size: "large".into(),
                    skip_design: false,
                    skip_graph: false,
                }),
            ),
            RitualPhase::Reviewing => (
                RitualPhase::Designing,
                RitualEvent::SkillCompleted {
                    phase: "draft-design".into(),
                    artifacts: vec![],
                },
            ),
            RitualPhase::Planning => (
                // Simulate round-2 approval completing → Planning
                // Need review_round >= 2 and review_target = "design" for the transition to go to Planning
                RitualPhase::WaitingApproval,
                RitualEvent::UserApproval { approved: "all".into() },
            ),
            RitualPhase::Graphing => (
                RitualPhase::Planning,
                RitualEvent::PlanDecided(ImplementStrategy::SingleLlm),
            ),
            RitualPhase::Implementing => (
                RitualPhase::Graphing,
                RitualEvent::SkillCompleted {
                    phase: "generate-graph".into(),
                    artifacts: vec![],
                },
            ),
            RitualPhase::Verifying => (
                RitualPhase::Implementing,
                RitualEvent::SkillCompleted {
                    phase: "implement".into(),
                    artifacts: vec![],
                },
            ),
            _ => (
                RitualPhase::Idle,
                RitualEvent::Start { task: task.to_string() },
            ),
        }
    }

    /// Check prerequisites for resuming from a given phase.
    /// Returns a list of missing prerequisites (empty = all good).
    fn check_prerequisites(phase: &RitualPhase, root: &Path) -> Vec<String> {
        let mut missing = Vec::new();

        let has_requirements = root.join("REQUIREMENTS.md").exists()
            || root.join(".gid/requirements.md").exists()
            || root.join(".gid/features").is_dir() && std::fs::read_dir(root.join(".gid/features"))
                .map(|entries| entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.path().join("requirements.md").exists()))
                .unwrap_or(false);

        let has_design = root.join("DESIGN.md").exists()
            || root.join(".gid/DESIGN.md").exists()
            || root.join(".gid/design.md").exists()
            || root.join(".gid/features").is_dir() && std::fs::read_dir(root.join(".gid/features"))
                .map(|entries| entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.path().join("design.md").exists()))
                .unwrap_or(false);

        let has_graph = root.join(".gid/graph.yml").exists();

        let has_reviews = root.join(".gid/reviews").is_dir()
            && std::fs::read_dir(root.join(".gid/reviews"))
                .map(|entries| entries.filter_map(|e| e.ok()).count() > 0)
                .unwrap_or(false);

        match phase {
            // No prerequisites for early phases
            RitualPhase::Idle | RitualPhase::Initializing | RitualPhase::Triaging
            | RitualPhase::WritingRequirements => {},

            RitualPhase::Designing => {
                // Requirements should exist (or we're doing design-first)
                // Soft check — don't block, just warn
            }
            RitualPhase::Reviewing => {
                if !has_design && !has_requirements {
                    missing.push("No design or requirements document found".into());
                }
            }
            RitualPhase::WaitingApproval => {
                if !has_reviews {
                    missing.push("No review files found in .gid/reviews/".into());
                }
            }
            RitualPhase::Planning => {
                if !has_design {
                    missing.push("No design document found (DESIGN.md or .gid/design.md)".into());
                }
            }
            RitualPhase::Graphing => {
                if !has_design {
                    missing.push("No design document found".into());
                }
            }
            RitualPhase::Implementing => {
                if !has_graph {
                    // Not a hard block — single-file impl doesn't need graph
                    // But warn
                }
                if !has_design {
                    missing.push("No design document found".into());
                }
            }
            RitualPhase::Verifying => {
                // Source code should exist — uses Cargo.toml/package.json workspace members
                if !has_source_in_project(root) {
                    missing.push("No source directory found (checked src/, lib/, and workspace members from Cargo.toml/package.json)".into());
                }
            }
            _ => {} // Terminal states
        }

        missing
    }

    /// Mark the current phase as done and advance to the next phase.
    /// Used when the user manually completed a phase outside the ritual.
    pub async fn mark_phase_done(&self, ritual_id: &str) -> Result<RitualState> {
        let state = self.load_state_by_id(ritual_id)?;

        if state.phase.is_terminal() || state.phase == RitualPhase::Idle {
            return Err(anyhow::anyhow!("No active phase to mark as done (current: {})", state.phase.display_name()));
        }

        // Build a SkillCompleted event to advance the state machine naturally
        let event = match &state.phase {
            RitualPhase::Designing => RitualEvent::SkillCompleted {
                phase: "draft-design".into(),
                artifacts: vec![],
            },
            RitualPhase::WritingRequirements => RitualEvent::SkillCompleted {
                phase: "draft-requirements".into(),
                artifacts: vec![],
            },
            RitualPhase::Reviewing => RitualEvent::SkillCompleted {
                phase: "review-design".into(),
                artifacts: vec![],
            },
            RitualPhase::WaitingApproval => RitualEvent::UserApproval {
                approved: "all".into(),
            },
            RitualPhase::Planning => RitualEvent::PlanDecided(
                state.strategy.clone().unwrap_or(ImplementStrategy::SingleLlm)
            ),
            RitualPhase::Graphing => RitualEvent::SkillCompleted {
                phase: "generate-graph".into(),
                artifacts: vec![],
            },
            RitualPhase::Implementing => RitualEvent::SkillCompleted {
                phase: "implement".into(),
                artifacts: vec![],
            },
            RitualPhase::Verifying => RitualEvent::ShellCompleted {
                stdout: "Manually verified".into(),
                exit_code: 0,
            },
            phase => {
                return Err(anyhow::anyhow!("Cannot mark {} as done — use skip instead", phase.display_name()));
            }
        };

        // Notify
        (self.notify)(format!(
            "✅ Phase '{}' marked as manually completed. Advancing...",
            state.phase.display_name()
        )).await;

        self.advance(ritual_id, event).await
    }

        /// Load skill prompt from file or built-in fallback.
    /// Priority: SkillRegistry → .gid/skills/{name}.md → built-in fallback
    fn load_skill_prompt(&self, skill_name: &str) -> String {
        // Use SkillRegistry if available (via AgentRunner)
        if let Some(ref runner) = self.agent_runner {
            if let Some(skill) = runner.workspace().skill_registry.get(skill_name) {
                return skill.prompt_content().to_string();
            }
        }

        // Project-local skill (fallback for projects with custom skills)
        let gid_skill = self.project_root.join(".gid").join("skills").join(format!("{}.md", skill_name));
        if gid_skill.exists() {
            if let Ok(content) = std::fs::read_to_string(&gid_skill) {
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

    /// Run multiple implementation tasks sequentially.
    /// Each task gets its own LLM session via run_skill("implement", task).
    /// Sequential execution avoids rate limit contention, file conflicts, and token waste.
    /// Results are collected: all succeed → SkillCompleted, any fail → SkillFailed.
    async fn run_harness(
        &self,
        tasks: &[String],
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        tracing::info!(task_count = tasks.len(), "Running harness ({} sequential tasks)", tasks.len());

        if tasks.is_empty() {
            return Ok((RitualEvent::SkillCompleted {
                phase: "implement".into(),
                artifacts: vec![],
            }, 0));
        }

        // For single task, just run directly
        if tasks.len() == 1 {
            return self.run_skill("implement", &tasks[0], cancel_token).await;
        }

        // Run tasks sequentially — avoids rate limit contention, file conflicts,
        // and duplicate system prompt overhead from parallel sessions.
        let mut total_tokens = 0u64;
        let mut all_artifacts = Vec::new();
        let mut failures = Vec::new();

        for (i, task) in tasks.iter().enumerate() {
            if cancel_token.is_cancelled() {
                tracing::info!("Harness cancelled before task {}/{}", i + 1, tasks.len());
                return Ok((RitualEvent::UserCancel, total_tokens));
            }

            let task_ctx = format!(
                "Task {}/{}: {}\n\nIMPORTANT: Only implement THIS specific task. \
                 Other tasks will be handled after this one completes.",
                i + 1, tasks.len(), task
            );
            tracing::info!(task_idx = i, "Starting harness task {}/{}", i + 1, tasks.len());

            match self.run_skill("implement", &task_ctx, cancel_token).await {
                Ok((event, tokens)) => {
                    tracing::info!(task_idx = i, tokens = tokens, "Harness task {}/{} completed", i + 1, tasks.len());
                    total_tokens += tokens;
                    if let RitualEvent::SkillCompleted { artifacts, .. } = event {
                        all_artifacts.extend(artifacts);
                    }
                }
                Err(e) => {
                    tracing::warn!(task_idx = i, error = %e, "Harness task {}/{} failed", i + 1, tasks.len());
                    failures.push(format!("Task {}: {}", i + 1, e));
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
        // Re-read verify_command from .gid/config.yml at execution time,
        // so users can update the config without restarting the ritual.
        let command = {
            let config = gid_core::ritual::load_gating_config(&self.project_root);
            if let Some(ref fresh_cmd) = config.verify_command {
                tracing::info!("Using verify_command from .gid/config.yml: {}", fresh_cmd);
                fresh_cmd.clone()
            } else {
                command.to_string()
            }
        };
        let work_dir = &self.project_root;
        tracing::info!("Running shell command in {}: {}", work_dir.display(), command);

        // Run verification steps sequentially with labeled output.
        // Each step is separated so the LLM knows exactly which stage failed.
        let steps = parse_verify_steps(&command);
        let mut all_stdout = String::new();
        let mut all_stderr = String::new();
        let mut final_exit_code = 0i32;

        for (i, step) in steps.iter().enumerate() {
            let label = &step.label;
            tracing::info!("Verify step {}/{}: [{}] {}", i + 1, steps.len(), label, step.command);

            let output = tokio::time::timeout(
                std::time::Duration::from_secs(300),
                tokio::process::Command::new("bash")
                    .arg("-lc")
                    .arg(&step.command)
                    .current_dir(&work_dir)
                    .output()
            ).await
                .map_err(|_| anyhow::anyhow!("Verify step '{}' timed out after 5 minutes", label))?
                ?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            all_stdout.push_str(&format!("=== {} (exit {}) ===\n{}\n", label, exit_code, stdout));
            if !stderr.is_empty() {
                all_stderr.push_str(&format!("=== {} STDERR ===\n{}\n", label, stderr));
            }

            if !output.status.success() {
                // Stop at first failure — report which step failed
                final_exit_code = exit_code;
                all_stderr.insert_str(0, &format!("FAILED at step: {}\n\n", label));
                tracing::warn!("Verify failed at step '{}' (exit {})", label, exit_code);
                break;
            }
            tracing::info!("Verify step '{}' passed", label);
        }

        if final_exit_code == 0 {
            Ok(RitualEvent::ShellCompleted {
                stdout: truncate(&all_stdout, 2000),
                exit_code: 0,
            })
        } else {
            Ok(RitualEvent::ShellFailed {
                stderr: truncate(&format!("{}\n{}", all_stderr, all_stdout), 2000),
                exit_code: final_exit_code,
            })
        }
    }

    /// Run planning phase — LLM decides SingleLlm vs MultiAgent strategy.
    async fn run_planning(&self) -> Result<(RitualEvent, u64)> {
        use crate::ritual_adapter::RitualLlmAdapter;
        use gid_core::ritual::llm::LlmClient as GidLlmClient;

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
            25,
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

/// Extract a target project directory from the task context.
/// Looks for patterns like "Project location: /path/to/project" in the task description.
/// Returns None if no explicit project path is found (falls back to self.project_root).
/// A single step in the verify pipeline.
struct VerifyStep {
    label: String,
    command: String,
}

/// Parse a verify command into labeled steps.
/// Splits on `&&` and auto-labels each step based on the command content.
///
/// Example: "cargo check 2>&1 && cargo test --lib 2>&1 && cargo test --test '*' 2>&1"
/// → [("check", "cargo check 2>&1"), ("unit test", "cargo test --lib 2>&1"), ("integration test", "cargo test --test '*' 2>&1")]
fn parse_verify_steps(command: &str) -> Vec<VerifyStep> {
    let parts: Vec<&str> = command.split("&&").map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    if parts.len() <= 1 {
        // Single command — run as-is with auto-detected label
        return vec![VerifyStep {
            label: auto_label(command),
            command: command.to_string(),
        }];
    }

    parts.iter().map(|cmd| {
        VerifyStep {
            label: auto_label(cmd),
            command: cmd.to_string(),
        }
    }).collect()
}

/// Auto-detect a human-readable label for a shell command.
fn auto_label(cmd: &str) -> String {
    let cmd_lower = cmd.to_lowercase();
    if cmd_lower.contains("check") || cmd_lower.contains("build") {
        "compile".to_string()
    } else if cmd_lower.contains("--test") || cmd_lower.contains("-test") {
        "integration test".to_string()
    } else if cmd_lower.contains("--lib") {
        "unit test".to_string()
    } else if cmd_lower.contains("test") {
        "test".to_string()
    } else if cmd_lower.contains("lint") || cmd_lower.contains("clippy") {
        "lint".to_string()
    } else {
        "verify".to_string()
    }
}

/// Build context blocks for implement/execute-tasks phases.
/// Injects DESIGN.md, graph task nodes, and review findings.
fn build_implement_context(project_root: &Path) -> Vec<crate::agent::ContextBlock> {
    let mut blocks = Vec::new();

    // Load DESIGN.md (feature-level or top-level)
    for path in &[
        project_root.join(".gid/DESIGN.md"),
        project_root.join("DESIGN.md"),
    ] {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                let truncated = if content.len() > 4096 {
                    format!("{}...\n(truncated)", &content[..content.floor_char_boundary(4096)])
                } else {
                    content
                };
                blocks.push(crate::agent::ContextBlock {
                    label: format!("DESIGN: {} (ALREADY LOADED)", path.file_name().unwrap_or_default().to_string_lossy()),
                    content: truncated,
                });
            }
        }
    }

    // Feature-level design docs
    let features_dir = project_root.join(".gid/features");
    if features_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&features_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let design_path = entry.path().join("DESIGN.md");
                if design_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&design_path) {
                        let truncated = if content.len() > 4096 {
                            format!("{}...\n(truncated)", &content[..content.floor_char_boundary(4096)])
                        } else {
                            content
                        };
                        let feature_name = entry.file_name().to_string_lossy().to_string();
                        blocks.push(crate::agent::ContextBlock {
                            label: format!("DESIGN: {} (ALREADY LOADED)", feature_name),
                            content: truncated,
                        });
                    }
                }
            }
        }
    }

    // Graph task nodes
    let graph_path = project_root.join(".gid/graph.yml");
    if graph_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&graph_path) {
            let task_lines: Vec<&str> = content.lines()
                .filter(|l| l.contains("task-") || l.contains("title:") || l.contains("status:") || l.contains("  - id:"))
                .take(60)
                .collect();
            if !task_lines.is_empty() {
                blocks.push(crate::agent::ContextBlock {
                    label: "Task Graph (.gid/graph.yml)".to_string(),
                    content: format!("```yaml\n{}\n```", task_lines.join("\n")),
                });
            }
        }
    }

    blocks
}

/// Build context blocks for review phases.
/// Lists documents to review from .gid/features/.
fn build_review_context(phase: &str, project_root: &Path) -> Vec<crate::agent::ContextBlock> {
    let doc_suffix = match phase {
        "review-requirements" => "requirements",
        "review-design" => "design",
        _ => return vec![],
    };

    let mut doc_paths = Vec::new();
    let features_dir = project_root.join(".gid/features");
    if features_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&features_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.path().is_dir() {
                    if let Ok(files) = std::fs::read_dir(entry.path()) {
                        for file in files.filter_map(|f| f.ok()) {
                            let fname = file.file_name().to_string_lossy().to_string();
                            if fname.contains(doc_suffix) && fname.ends_with(".md") {
                                let rel = file.path().strip_prefix(project_root)
                                    .unwrap_or(&file.path()).display().to_string();
                                doc_paths.push(rel);
                            }
                        }
                    }
                }
            }
        }
    }
    // Also check top-level .gid/
    if let Ok(entries) = std::fs::read_dir(project_root.join(".gid")) {
        for entry in entries.filter_map(|e| e.ok()) {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.contains(doc_suffix) && fname.ends_with(".md") && entry.path().is_file() {
                doc_paths.push(format!(".gid/{}", fname));
            }
        }
    }
    doc_paths.sort();

    if doc_paths.is_empty() {
        return vec![];
    }

    // Pre-load file contents with budget
    let full_paths: Vec<PathBuf> = doc_paths.iter()
        .map(|rel| project_root.join(rel))
        .collect();
    let mut blocks = preload_files_with_budget(&full_paths, project_root, 120_000); // ~30K tokens

    // Prepend instructions
    let doc_list = doc_paths.iter().enumerate()
        .map(|(i, d)| format!("{}. {}", i + 1, d))
        .collect::<Vec<_>>()
        .join("\n");
    blocks.insert(0, crate::agent::ContextBlock {
        label: "Review Instructions".to_string(),
        content: format!(
            "Review each document below. They are ALREADY LOADED in this message — do NOT call read_file on them.\n\
             For each, write findings to `.gid/reviews/<name>-{}-review.md`.\n\n\
             Documents:\n{}",
            doc_suffix, doc_list
        ),
    });

    blocks
}

/// Extract markdown structure: all headings + first non-empty line after each heading.
fn extract_markdown_skeleton(content: &str) -> String {
    let mut result = Vec::new();
    let mut want_first_line = false;

    for line in content.lines() {
        if line.starts_with('#') {
            result.push(line.to_string());
            want_first_line = true;
        } else if want_first_line && !line.trim().is_empty() {
            result.push(format!("  → {}", line.trim()));
            want_first_line = false;
        }
    }
    result.join("\n")
}

/// Pre-load files with a total character budget.
/// Each file gets an equal share. If a file exceeds its share:
/// - Skeleton (headings + first sentences) is always included
/// - Full content up to budget, with truncation note
pub fn preload_files_with_budget(
    files: &[PathBuf],
    project_root: &Path,
    total_budget_chars: usize,
) -> Vec<crate::agent::ContextBlock> {
    if files.is_empty() {
        return vec![];
    }

    let per_file_budget = total_budget_chars / files.len();

    files.iter().filter_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        let rel = path.strip_prefix(project_root).unwrap_or(path).display().to_string();

        let block_content = if content.len() <= per_file_budget {
            content
        } else {
            // Skeleton always included
            let skeleton = extract_markdown_skeleton(&content);
            let skeleton_header = format!("### Outline\n{}\n\n### Content\n", skeleton);
            let remaining = per_file_budget.saturating_sub(skeleton_header.len());
            let truncated = &content[..content.floor_char_boundary(remaining)];
            format!(
                "{}{}\n\n(truncated at {} chars — read full file only if you need content beyond this point)",
                skeleton_header, truncated, remaining
            )
        };

        Some(crate::agent::ContextBlock {
            label: format!("Document: {} (ALREADY LOADED — do NOT read again)", rel),
            content: block_content,
        })
    }).collect()
}

fn extract_target_project_dir(context: &str) -> Option<PathBuf> {
    // Pattern 1: Known prefix patterns (backward compat)
    // "Project location: /path/...", "project_root: /path/...", etc.
    for line in context.lines() {
        let trimmed = line.trim().trim_start_matches('*').trim();
        for prefix in &[
            "Project location:",
            "project location:",
            "Project root:",
            "project_root:",
            "Working directory:",
            "working directory:",
            "Workspace:",
            "workspace:",
            "Target project:",
            "target project:",
            "Project dir:",
            "project dir:",
            "Project directory:",
            "project directory:",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let path_str = rest.trim().trim_end_matches('/');
                let path = PathBuf::from(path_str);
                if path.is_absolute() && path.exists() && path.is_dir() {
                    return Some(path);
                }
            }
        }
    }

    // Pattern 2: Absolute paths in parentheses, e.g. "(/Users/potato/clawd/projects/gid-rs/)"
    for cap in find_parenthesized_paths(context) {
        let path = PathBuf::from(&cap);
        if path.is_absolute() && path.exists() && path.is_dir() {
            return Some(path);
        }
    }

    // Pattern 3: Standalone absolute paths — /Users/..., /home/..., /opt/..., /tmp/..., /var/...
    // Find any absolute path token in the text that exists as a directory.
    for candidate in find_standalone_absolute_paths(context) {
        let path = PathBuf::from(&candidate);
        if path.exists() && path.is_dir() {
            return Some(path);
        }
    }

    None
}

/// Find absolute paths enclosed in parentheses: `(/path/to/dir)` or `(/path/to/dir/)`
fn find_parenthesized_paths(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            if let Some(close) = text[i + 1..].find(')') {
                let inner = text[i + 1..i + 1 + close].trim();
                let inner = inner.trim_end_matches('/');
                if inner.starts_with('/') && !inner.contains(' ') {
                    results.push(inner.to_string());
                }
                i = i + 1 + close + 1;
                continue;
            }
        }
        i += 1;
    }
    results
}

/// Find standalone absolute paths in text that look like directories.
/// Matches paths starting with /Users/, /home/, /opt/, /tmp/, /var/, /srv/, /etc/.
fn find_standalone_absolute_paths(text: &str) -> Vec<String> {
    let prefixes = ["/Users/", "/home/", "/opt/", "/tmp/", "/var/", "/srv/"];
    let mut results = Vec::new();

    for line in text.lines() {
        // Tokenize by whitespace, backticks, quotes, and common delimiters
        for token in line.split(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'' || c == ',' || c == ';') {
            let token = token.trim_start_matches('(').trim_end_matches(')');
            let token = token.trim_end_matches('/');
            let token = token.trim_end_matches('.');
            if token.is_empty() {
                continue;
            }
            if prefixes.iter().any(|p| token.starts_with(p)) {
                // Ensure it looks like a path (no weird chars)
                if token.chars().all(|c| c.is_alphanumeric() || c == '/' || c == '-' || c == '_' || c == '.') {
                    results.push(token.to_string());
                }
            }
        }
    }
    results
}

/// Check if a task description contains an explicit target project directory.
/// Returns true if `extract_target_project_dir` would find a path.
pub fn has_target_project_dir(context: &str) -> bool {
    extract_target_project_dir(context).is_some()
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

/// Discover workspace member directories by reading the project's manifest file.
///
/// Supports:
/// - Cargo.toml `[workspace] members = ["crates/foo", "crates/bar"]` (with glob patterns)
/// - package.json `"workspaces": ["packages/*"]` (with glob patterns)
///
/// Returns absolute paths of member directories that actually exist on disk.
fn discover_workspace_member_dirs(root: &Path) -> Vec<PathBuf> {
    let mut members = Vec::new();

    // Try Cargo.toml
    let cargo_toml = root.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            if let Ok(parsed) = content.parse::<toml::Table>() {
                if let Some(workspace) = parsed.get("workspace").and_then(|v| v.as_table()) {
                    if let Some(member_list) = workspace.get("members").and_then(|v| v.as_array()) {
                        for m in member_list {
                            if let Some(pattern) = m.as_str() {
                                // Expand glob patterns (e.g., "crates/*")
                                let full_pattern = root.join(pattern);
                                if let Ok(paths) = glob::glob(full_pattern.to_string_lossy().as_ref()) {
                                    for path in paths.flatten() {
                                        if path.is_dir() {
                                            members.push(path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Try package.json (Node.js workspaces)
    let package_json = root.join("package.json");
    if members.is_empty() && package_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(workspaces) = parsed.get("workspaces").and_then(|v| v.as_array()) {
                    for ws in workspaces {
                        if let Some(pattern) = ws.as_str() {
                            let full_pattern = root.join(pattern);
                            if let Ok(paths) = glob::glob(full_pattern.to_string_lossy().as_ref()) {
                                for path in paths.flatten() {
                                    if path.is_dir() {
                                        members.push(path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    members
}

/// Check if a project has source files — either at root level or in workspace members.
fn has_source_in_project(root: &Path) -> bool {
    // Direct source directories
    if root.join("src").exists() || root.join("lib").exists() || root.join("app").exists() {
        return true;
    }
    // Workspace members from manifest
    let members = discover_workspace_member_dirs(root);
    members.iter().any(|m| m.join("src").exists() || m.join("lib").exists())
}

/// Count source files across the project — root src/ or all workspace member src/ dirs.
async fn count_source_files_in_project(root: &Path) -> usize {
    // Direct source directory takes precedence
    if root.join("src").exists() {
        return count_files_recursive(&root.join("src")).await;
    }
    // Otherwise sum across workspace members
    let members = discover_workspace_member_dirs(root);
    let mut total = 0;
    for member in &members {
        let src = member.join("src");
        if src.exists() {
            total += count_files_recursive(&src).await;
        }
    }
    total
}

/// Parse a phase name string into a RitualPhase.
/// Accepts display names, short names, and aliases.
pub fn parse_phase_name(name: &str) -> Option<RitualPhase> {
    match name.to_lowercase().trim() {
        "idle" => Some(RitualPhase::Idle),
        "init" | "initializing" | "initialize" => Some(RitualPhase::Initializing),
        "triage" | "triaging" => Some(RitualPhase::Triaging),
        "requirements" | "req" | "writing-requirements" | "writingrequirements" => Some(RitualPhase::WritingRequirements),
        "design" | "designing" => Some(RitualPhase::Designing),
        "review" | "reviewing" => Some(RitualPhase::Reviewing),
        "plan" | "planning" => Some(RitualPhase::Planning),
        "graph" | "graphing" => Some(RitualPhase::Graphing),
        "implement" | "implementing" | "impl" => Some(RitualPhase::Implementing),
        "verify" | "verifying" | "test" | "testing" => Some(RitualPhase::Verifying),
        "done" => Some(RitualPhase::Done),
        _ => None,
    }
}
