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

    /// Sweep zombie ritual state files left behind by dead processes.
    ///
    /// A "zombie" is a ritual file with a non-terminal `phase` whose
    /// `adapter_pid` no longer maps to a live process — i.e. the runner that
    /// owned it crashed, was killed, or shut down without calling
    /// `UserCancel`. ISS-019 / ISS-025 created several of these (background
    /// task races, panics inside skill phases).
    ///
    /// Files with `adapter_pid = None` are also swept when stale (no
    /// `updated_at` activity in the last `STALE_HOURS` hours), since they
    /// pre-date PID stamping and cannot be probed.
    ///
    /// The sweep marks each zombie as `Cancelled` (status + phase) via the
    /// canonical `transition(state, UserCancel)` path so the resulting file
    /// is indistinguishable from a user-driven cancel. The dead PID and the
    /// reason ("orphaned: pid {n} dead" / "orphaned: stale {h}h, no pid") are
    /// recorded in the transition log for forensics.
    ///
    /// Returns the list of (ritual_id, reason) pairs that were swept.
    /// Idempotent: re-running on already-terminal files is a no-op.
    pub fn sweep_orphans(&self) -> Result<Vec<(String, String)>> {
        sweep_orphans_in(&self.rituals_dir)
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
    pub fn save_state(&self, _state: &RitualState) -> Result<()> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "save_state",
            "ISS-052: legacy RitualRunner::save_state reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::save_state is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
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
                            reason: None,
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
                RitualAction::SaveState { .. } => {
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
    async fn run_triage(&self, _task: &str, _ritual_state: &RitualState) -> Result<(RitualEvent, u64)> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "run_triage",
            "ISS-052: legacy RitualRunner::run_triage reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::run_triage is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
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
        _name: &str,
        _context: &str,
        _cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "run_skill",
            "ISS-052: legacy RitualRunner::run_skill reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::run_skill is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
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
        _tasks: &[String],
        _cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<(RitualEvent, u64)> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "run_harness",
            "ISS-052: legacy RitualRunner::run_harness reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::run_harness is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
    }

    async fn run_shell(&self, _command: &str) -> Result<RitualEvent> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "run_shell",
            "ISS-052: legacy RitualRunner::run_shell reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::run_shell is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
    }

    /// Run planning phase — LLM decides SingleLlm vs MultiAgent strategy.
    async fn run_planning(&self) -> Result<(RitualEvent, u64)> {
        // ISS-052 T13a: dispatcher migrated to gid_core::ritual::run_ritual via T12.
        // Body stubbed to a graceful Err to catch any legacy call site that escaped
        // T13b's migration sweep. Production must not reach this branch — if it does,
        // it means a /ritual subcommand handler in telegram.rs still calls the old
        // path. Fix the caller, do not remove this guard.
        tracing::error!(
            target: "ritual_runner",
            fn_name = "run_planning",
            "ISS-052: legacy RitualRunner::run_planning reached after T12/T13a migration —              this should be unreachable. The caller must be migrated to run_ritual              (T13b) or to a thin event-recording shim. Returning Err."
        );
        Err(anyhow::anyhow!(
            "ISS-052: RitualRunner::run_planning is a stub after T13a — caller must migrate              to gid_core::ritual::run_ritual (see T13b)"
        ))
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
/// Check whether a process with the given PID is alive on a POSIX host.
///
/// Uses `kill(pid, 0)` — the standard liveness probe. Returns true if the
/// signal could be delivered (process exists and we have permission), false
/// if `ESRCH` (no such process) or any other error.
///
/// PID reuse is a known caveat: a long-dead ritual whose PID has been
/// reassigned to an unrelated process will read as alive. We accept that
/// trade-off — the orphan sweep is best-effort cleanup, not a hard guarantee.
/// In practice the window is bounded by sweep frequency (every daemon start)
/// and PID space size (32k on macOS, 4M on modern Linux).
fn is_pid_alive(pid: u32) -> bool {
    // Safety: `kill(pid, 0)` is a pure liveness probe — no signal is actually
    // delivered. The libc call is signal-safe and reentrant; passing any pid
    // is well-defined (returns -1 with errno=ESRCH for unknown pids).
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    // EPERM means the process exists but we lack permission to signal it —
    // still alive for our purposes.
    let errno = std::io::Error::last_os_error().raw_os_error();
    matches!(errno, Some(libc::EPERM))
}

/// Stale threshold for files without an `adapter_pid` (legacy or
/// pre-ISS-019 rituals). 24h matches the longest realistic ritual.
const ORPHAN_STALE_HOURS: i64 = 24;

/// Implementation of the orphan sweep, decoupled from `RitualRunner` so it
/// can be unit-tested without constructing a full runner (which requires an
/// LLM client and notify fn that the sweep itself never uses).
///
/// Walks every `*.json` file in `rituals_dir`, classifies each as either
/// a zombie (non-terminal phase whose `adapter_pid` is dead, or non-terminal
/// with no pid and `updated_at` older than `ORPHAN_STALE_HOURS`) or live,
/// and rewrites zombies as `Cancelled` via `transition(state, UserCancel)`.
///
/// Returns `(ritual_id, reason)` for each file that was swept.
fn sweep_orphans_in(rituals_dir: &Path) -> Result<Vec<(String, String)>> {
    let mut swept = Vec::new();

    if !rituals_dir.exists() {
        return Ok(swept);
    }

    for entry in std::fs::read_dir(rituals_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }

        // Read & parse defensively — corrupt/in-progress writes shouldn't
        // abort the entire sweep.
        let data = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?path, "sweep: read failed: {}", e);
                continue;
            }
        };
        let state: RitualState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?path, "sweep: parse failed: {}", e);
                continue;
            }
        };

        // Already-terminal phases are never zombies.
        if state.phase.is_terminal() {
            continue;
        }
        // Idle states are not in flight by definition.
        if state.phase == RitualPhase::Idle {
            continue;
        }

        let reason = match state.adapter_pid {
            Some(pid) if !is_pid_alive(pid) => {
                Some(format!("orphaned: pid {} dead", pid))
            }
            Some(_) => None, // pid is alive — not a zombie
            None => {
                // No PID stamp. Sweep only if the file has gone cold —
                // otherwise we'd race against a runner that is mid-write
                // on a state file from before PID stamping landed.
                let age = chrono::Utc::now()
                    .signed_duration_since(state.updated_at);
                if age.num_hours() >= ORPHAN_STALE_HOURS {
                    Some(format!(
                        "orphaned: stale {}h, no pid",
                        age.num_hours()
                    ))
                } else {
                    None
                }
            }
        };

        let Some(reason) = reason else { continue };

        // Drive through the canonical state machine so the resulting file
        // matches a user-driven cancel exactly (status, transitions,
        // notes — everything). Side-effect actions like `Notify` are
        // discarded: there is no user to inform about a sweep of a
        // long-dead ritual.
        let (mut cancelled, _actions) =
            transition(&state, RitualEvent::UserCancel);

        // Overwrite the synthesized event string in the latest transition
        // record with the sweep reason for forensics. The state machine
        // appended exactly one new TransitionRecord during UserCancel.
        if let Some(last) = cancelled.transitions.last_mut() {
            last.event = reason.clone();
        }

        // Persist directly without stamping our PID — we want to preserve
        // the *original* dead `adapter_pid` for forensics. The sweep writes
        // a frozen, terminal record; no further updates are expected.
        let out = serde_json::to_string_pretty(&cancelled)?;
        std::fs::write(&path, out)?;

        tracing::warn!(
            ritual_id = %cancelled.id,
            reason = %reason,
            "swept orphan ritual"
        );
        swept.push((cancelled.id, reason));
    }

    Ok(swept)
}

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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests — orphan sweep (ISS-019 part 3)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod orphan_sweep_tests {
    use super::*;
    use gid_core::ritual::RitualV2Status;
    use std::path::Path;
    use tempfile::TempDir;

    /// Write a state file directly to `dir/<id>.json`, bypassing
    /// `RitualRunner::save_state` so the test fully controls every field
    /// (including `adapter_pid`, which `save_state` would otherwise stamp).
    fn write_state(dir: &Path, state: &RitualState) {
        let path = dir.join(format!("{}.json", state.id));
        let data = serde_json::to_string_pretty(state).unwrap();
        std::fs::write(path, data).unwrap();
    }

    fn read_state(dir: &Path, id: &str) -> RitualState {
        let path = dir.join(format!("{}.json", id));
        let data = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&data).unwrap()
    }

    /// Build a fresh state in a non-terminal phase, with the given pid and
    /// updated_at. Uses `Implementing` as a representative non-terminal phase
    /// (matches the real-world zombie observed in `.gid/rituals/r-e4e1f7.json`).
    fn make_active_state(
        id: &str,
        pid: Option<u32>,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> RitualState {
        let mut s = RitualState::new();
        s.id = id.to_string();
        s.task = "test ritual".into();
        s = s.with_phase(RitualPhase::Implementing);
        s.adapter_pid = pid;
        s.updated_at = updated_at;
        s
    }

    /// PID that is virtually guaranteed to be dead: pid 1 is init/launchd
    /// (alive), but pid 999_999 won't exist on macOS (32k pid limit). On
    /// Linux this could theoretically map to a real process — we mitigate
    /// in `assert!` by sanity-checking via `is_pid_alive` first.
    const DEAD_PID: u32 = 999_999;

    #[test]
    fn dead_pid_is_detected_dead() {
        // Sanity: if this ever passes, the rest of the suite is meaningless.
        assert!(!is_pid_alive(DEAD_PID),
            "DEAD_PID is alive on this host — choose a different pid");
    }

    #[test]
    fn live_pid_is_detected_alive() {
        // Our own pid is, by definition, alive.
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn empty_directory_sweeps_nothing() {
        let tmp = TempDir::new().unwrap();
        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty());
    }

    #[test]
    fn nonexistent_directory_sweeps_nothing() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let swept = sweep_orphans_in(&missing).unwrap();
        assert!(swept.is_empty());
    }

    #[test]
    fn dead_pid_with_active_phase_is_swept() {
        let tmp = TempDir::new().unwrap();
        let state = make_active_state("r-zombie", Some(DEAD_PID), chrono::Utc::now());
        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].0, "r-zombie");
        assert!(swept[0].1.contains(&format!("pid {}", DEAD_PID)),
            "reason should mention dead pid: {}", swept[0].1);

        // Disk state: phase=Cancelled, status=Cancelled, original pid preserved.
        let after = read_state(tmp.path(), "r-zombie");
        assert_eq!(after.phase, RitualPhase::Cancelled);
        assert_eq!(after.status, RitualV2Status::Cancelled);
        assert_eq!(after.adapter_pid, Some(DEAD_PID),
            "sweep must preserve the original dead pid for forensics");
        // Last transition records the sweep reason, not the canonical event string.
        let last = after.transitions.last().expect("should have a transition");
        assert!(last.event.contains("orphaned"), "sweep reason in event: {}", last.event);
        assert_eq!(last.to, RitualPhase::Cancelled);
    }

    #[test]
    fn live_pid_with_active_phase_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let live_pid = std::process::id();
        let state = make_active_state("r-live", Some(live_pid), chrono::Utc::now());
        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty(), "live ritual must not be swept");

        let after = read_state(tmp.path(), "r-live");
        assert_eq!(after.phase, RitualPhase::Implementing,
            "untouched: phase preserved");
        assert_eq!(after.status, RitualV2Status::Active);
    }

    #[test]
    fn terminal_phase_is_left_alone_even_with_dead_pid() {
        let tmp = TempDir::new().unwrap();
        let mut state = RitualState::new();
        state.id = "r-done".into();
        state = state.with_phase(RitualPhase::Done);
        state.adapter_pid = Some(DEAD_PID); // dead, but doesn't matter

        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty(),
            "Done ritual is already terminal — sweep must skip");

        let after = read_state(tmp.path(), "r-done");
        assert_eq!(after.phase, RitualPhase::Done);
        assert_eq!(after.status, RitualV2Status::Done,
            "Done status must be preserved unchanged");
    }

    #[test]
    fn already_cancelled_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let mut state = RitualState::new();
        state.id = "r-cancelled".into();
        state = state.with_phase(RitualPhase::Implementing);
        state = state.with_phase(RitualPhase::Cancelled);
        state.adapter_pid = Some(DEAD_PID);

        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty(),
            "Cancelled is terminal — re-sweeping must be a no-op");
        let after = read_state(tmp.path(), "r-cancelled");
        assert_eq!(after.status, RitualV2Status::Cancelled);
    }

    #[test]
    fn idle_phase_is_left_alone() {
        // Idle is the initial state from RitualState::new() — not in flight.
        let tmp = TempDir::new().unwrap();
        let mut state = RitualState::new();
        state.id = "r-idle".into();
        // No with_phase call → still Idle.
        state.adapter_pid = Some(DEAD_PID);
        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty(),
            "Idle phase is not in flight — sweep must skip");
    }

    #[test]
    fn no_pid_recent_update_is_left_alone() {
        // Legacy file: no pid stamp, but updated recently. Could be a live
        // pre-PID-stamping runner mid-write; sweep must not race against it.
        let tmp = TempDir::new().unwrap();
        let state = make_active_state("r-legacy-fresh", None, chrono::Utc::now());
        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty(),
            "no-pid + recent updated_at must NOT be swept (race window)");
    }

    #[test]
    fn no_pid_stale_update_is_swept() {
        // Legacy zombie: no pid stamp, last touched >24h ago. Definitely dead.
        let tmp = TempDir::new().unwrap();
        let stale_ts = chrono::Utc::now() - chrono::Duration::hours(48);
        let state = make_active_state("r-legacy-stale", None, stale_ts);
        write_state(tmp.path(), &state);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].0, "r-legacy-stale");
        assert!(swept[0].1.contains("stale"),
            "reason should mention staleness: {}", swept[0].1);
        assert!(swept[0].1.contains("48h") || swept[0].1.contains("47h"),
            "reason should include hours: {}", swept[0].1);

        let after = read_state(tmp.path(), "r-legacy-stale");
        assert_eq!(after.status, RitualV2Status::Cancelled);
        assert_eq!(after.adapter_pid, None,
            "no-pid sweep must preserve None pid");
    }

    #[test]
    fn sweep_is_idempotent() {
        // Running sweep twice on the same input produces the same output;
        // the second pass is a no-op.
        let tmp = TempDir::new().unwrap();
        let state = make_active_state("r-zombie", Some(DEAD_PID), chrono::Utc::now());
        write_state(tmp.path(), &state);

        let first = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(first.len(), 1);

        let second = sweep_orphans_in(tmp.path()).unwrap();
        assert!(second.is_empty(),
            "second sweep must find nothing — first already cancelled it");

        // State on disk is still Cancelled, not double-cancelled.
        let after = read_state(tmp.path(), "r-zombie");
        assert_eq!(after.phase, RitualPhase::Cancelled);
    }

    #[test]
    fn mixed_population_only_zombies_swept() {
        // Realistic scenario: a directory containing live, zombie, terminal,
        // and idle rituals. Only the zombie is touched.
        let tmp = TempDir::new().unwrap();

        let zombie = make_active_state("r-zombie", Some(DEAD_PID), chrono::Utc::now());
        let live = make_active_state("r-live", Some(std::process::id()), chrono::Utc::now());

        let mut done = RitualState::new();
        done.id = "r-done".into();
        done = done.with_phase(RitualPhase::Done);

        let mut idle = RitualState::new();
        idle.id = "r-idle".into();

        write_state(tmp.path(), &zombie);
        write_state(tmp.path(), &live);
        write_state(tmp.path(), &done);
        write_state(tmp.path(), &idle);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].0, "r-zombie");

        // Verify the others are untouched.
        assert_eq!(read_state(tmp.path(), "r-live").phase, RitualPhase::Implementing);
        assert_eq!(read_state(tmp.path(), "r-done").phase, RitualPhase::Done);
        assert_eq!(read_state(tmp.path(), "r-idle").phase, RitualPhase::Idle);
    }

    #[test]
    fn corrupt_file_does_not_abort_sweep() {
        // A garbage / partial-write file in the rituals dir must not stop
        // the sweep from processing valid neighbours. This was a real
        // failure mode in early v2 development.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("garbage.json"), "{ not valid json").unwrap();
        let zombie = make_active_state("r-zombie", Some(DEAD_PID), chrono::Utc::now());
        write_state(tmp.path(), &zombie);

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(swept.len(), 1, "corrupt sibling must not block sweep");
        assert_eq!(swept[0].0, "r-zombie");
    }

    #[test]
    fn non_json_files_are_ignored() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "not a ritual").unwrap();
        std::fs::write(tmp.path().join("notes.md"), "ignore me").unwrap();

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert!(swept.is_empty());
    }

    #[test]
    fn legacy_state_without_status_field_round_trips_correctly() {
        // Files written before the `status` field existed deserialize as
        // status=Active. After being swept, they must end up status=Cancelled
        // with the field present in the output JSON.
        let tmp = TempDir::new().unwrap();
        // Synthesize a legacy file: serialize a normal state, then strip the
        // status field by re-parsing as Value, removing the key, re-writing.
        let state = make_active_state("r-legacy", Some(DEAD_PID), chrono::Utc::now());
        let data = serde_json::to_string_pretty(&state).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&data).unwrap();
        v.as_object_mut().unwrap().remove("status");
        std::fs::write(tmp.path().join("r-legacy.json"),
            serde_json::to_string_pretty(&v).unwrap()).unwrap();

        // Confirm the field really is absent on disk.
        let raw = std::fs::read_to_string(tmp.path().join("r-legacy.json")).unwrap();
        assert!(!raw.contains("\"status\""), "status field must be missing pre-sweep");

        let swept = sweep_orphans_in(tmp.path()).unwrap();
        assert_eq!(swept.len(), 1);

        // Post-sweep: status field exists and is Cancelled.
        let after = read_state(tmp.path(), "r-legacy");
        assert_eq!(after.status, RitualV2Status::Cancelled);
        let raw_after = std::fs::read_to_string(tmp.path().join("r-legacy.json")).unwrap();
        assert!(raw_after.contains("\"status\""),
            "sweep must write the status field for downstream consumers");
    }
}
