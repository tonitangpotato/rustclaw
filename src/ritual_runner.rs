//! Ritual helpers — pure I/O and parsing utilities for the ritual subsystem.
//!
//! ISS-052 (T13b commit 6): the legacy `RitualRunner` struct was removed.
//! All ritual orchestration now flows through `gid_core::ritual::run_ritual`
//! and `gid_core::ritual::resume_ritual`, which call back into RustClaw via
//! the `RitualHooks` trait (see `crate::ritual_hooks::RustclawHooks`).
//!
//! What lives here now:
//! - State-file readers (`load_state_by_id`, `find_latest_active`,
//!   `list_rituals`)
//! - Cancel/event registries (`CancelRegistry`, `EventRegistry`,
//!   `cancel_running`, `cancel_all_running`)
//! - Orphan sweeper (`sweep_orphans`)
//! - Path helpers (`has_target_project_dir`, `extract_target_project_dir`)
//! - Context-block builders (`preload_files_with_budget`)
//!
//! Everything that used to be a `RitualRunner::method(&self, …)` is now
//! either (a) a free function above, (b) an `Action` interpreted by
//! `run_ritual`, or (c) a hook the runner calls into via `RitualHooks`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::Result;
use gid_core::ritual::{
    V2Phase as RitualPhase,
    V2State as RitualState, V2Event as RitualEvent,
    ImplementStrategy, transition,
};

/// Conflict surfaced when a caller tries to start a ritual that would
/// collide with an existing one. Today only one variant — `AlreadyActive` —
/// is defined (ISS-028 Task 1a). Future variants may cover cross-instance
/// or cross-host collisions; keeping this an enum (not a single error
/// struct) leaves room for them without breaking call sites.
///
/// Pattern follows `gid_core::ritual::WorkspaceError`: a typed error enum
/// adjacent to the subsystem it serves, so callers can pattern-match
/// instead of string-sniffing an `anyhow::Error`.
///
/// ISS-028 GOAL-1: returned by the ritual launcher when a non-terminal
/// ritual already exists for the same `work_unit.label()`. The caller
/// (e.g. `StartRitualTool`) is responsible for surfacing this to the
/// agent in a clear form (GOAL-3).
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RitualConflict {
    /// A non-terminal ritual already exists for the requested work unit.
    ///
    /// `phase ∉ {Done, Cancelled, Escalated}` per ISS-028 GOAL-2 (uses the
    /// canonical terminal-phase definition from ISS-019; do not introduce
    /// a parallel one).
    ///
    /// Fields:
    /// - `ritual_id`: the existing ritual's id (e.g. `r-429789`)
    /// - `phase`: its current non-terminal phase (display string, so
    ///   loggers/UIs don't need to depend on the gid-core enum).
    /// - `work_unit_label`: canonical `WorkUnit::label()` of the
    ///   collision key — kept for log/diagnostic use even though the
    ///   caller already knows which unit they passed in.
    /// - `started_at`: ISO-8601 timestamp the existing ritual was
    ///   created. RFC3339 string to keep this enum free of chrono
    ///   types in its public surface (callers vary in chrono feature
    ///   flags).
    #[error(
        "ritual already active for {work_unit_label}: \
         ritual_id={ritual_id}, phase={phase}, started={started_at}"
    )]
    AlreadyActive {
        ritual_id: String,
        phase: String,
        work_unit_label: String,
        started_at: String,
    },
}

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

/// Flip the cancel token for a running ritual. Returns `true` if a token
/// was found and flipped (i.e. the ritual was registered as running).
///
/// ISS-052 T13b commit 4: lifted out of `RitualRunner::cancel_running` so
/// `/ritual cancel <id>` can fire the token without holding a runner.
pub fn cancel_running(registry: &CancelRegistry, ritual_id: &str) -> bool {
    let reg = registry.lock().unwrap();
    if let Some(token) = reg.get(ritual_id) {
        token.cancel();
        true
    } else {
        false
    }
}

/// Flip every token in the registry. Returns the number of tokens fired.
pub fn cancel_all_running(registry: &CancelRegistry) -> usize {
    let reg = registry.lock().unwrap();
    let count = reg.len();
    for token in reg.values() {
        token.cancel();
    }
    count
}

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

// ── ritual state IO (free functions) ─────────────────────────────────────
//
// ISS-052 T13b commit 1: extract state-file readers from RitualRunner so
// `/ritual` subcommand handlers can load state without constructing a full
// runner. RitualRunner itself will be deleted in commit 6.
//
// Writes go through `RustclawHooks::persist_state` (driven by gid-core's
// `run_ritual` / `resume_ritual`) — there is intentionally no public writer
// here. State files are owned by the state machine, not by call sites.

/// Path of a ritual's state file inside `rituals_dir`.
fn state_path_for(rituals_dir: &Path, ritual_id: &str) -> PathBuf {
    rituals_dir.join(format!("{}.json", ritual_id))
}

/// Load state for a specific ritual by ID. Errors if missing or unparsable.
pub fn load_state_by_id(rituals_dir: &Path, ritual_id: &str) -> Result<RitualState> {
    let path = state_path_for(rituals_dir, ritual_id);
    if !path.exists() {
        return Err(anyhow::anyhow!("Ritual {} not found", ritual_id));
    }
    let data = std::fs::read_to_string(&path)?;
    let state: RitualState = serde_json::from_str(&data)?;
    Ok(state)
}

/// List all ritual states in `rituals_dir`, sorted by `updated_at` desc.
/// Defensive: corrupt or unparsable files are skipped silently.
pub fn list_rituals(rituals_dir: &Path) -> Result<Vec<RitualState>> {
    let mut rituals = Vec::new();

    if rituals_dir.exists() {
        for entry in std::fs::read_dir(rituals_dir)? {
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

    rituals.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(rituals)
}

/// Find the most relevant active ritual.
/// Priority: paused (waiting for user input) > non-terminal active > none.
pub fn find_latest_active(rituals_dir: &Path) -> Result<Option<RitualState>> {
    let rituals = list_rituals(rituals_dir)?;
    if let Some(r) = rituals.iter().find(|r| r.phase.is_paused()) {
        return Ok(Some(r.clone()));
    }
    Ok(rituals.into_iter().find(|r| {
        !r.phase.is_terminal() && r.phase != RitualPhase::Idle
    }))
}

/// Pre-flight duplicate check for `start_ritual` (ISS-028 GOAL-1/2/3).
///
/// Scans `rituals_dir` for a ritual whose `work_unit.label()` matches
/// `label` AND whose phase is non-terminal — i.e. the prospective new
/// ritual would race or duplicate an in-flight one. Returns the most
/// recently-updated match, or `None` if the path is clear.
///
/// "Non-terminal" reuses `RitualPhase::is_terminal()` from ISS-019 —
/// **do not** introduce a parallel terminal-phase definition here.
/// `Idle` is treated as non-terminal too: a freshly-created ritual that
/// has not yet transitioned still owns the work unit.
///
/// Cost is bounded: real `.gid/rituals/` directories hold ≤ ~50 files
/// (terminal sweeps prune them), and `list_rituals` already silently
/// skips unparsable entries.
pub fn find_active_for_work_unit(
    rituals_dir: &Path,
    label: &str,
) -> Result<Option<RitualState>> {
    let rituals = list_rituals(rituals_dir)?;
    Ok(rituals.into_iter().find(|r| {
        if r.phase.is_terminal() {
            return false;
        }
        match r.work_unit.as_ref() {
            Some(w) => w.label() == label,
            None => false,
        }
    }))
}

/// Sweep zombie ritual state files. See `sweep_orphans_in` for full semantics.
pub fn sweep_orphans(rituals_dir: &Path) -> Result<Vec<(String, String)>> {
    sweep_orphans_in(rituals_dir)
}

/// One duplicate-terminal group reported by [`reconcile_terminal_duplicates`].
///
/// `work_unit_label` is the canonical [`WorkUnit::label()`](
/// gid_core::ritual::work_unit::WorkUnit::label) shared by the rituals;
/// `ritual_ids` lists every terminal ritual file that resolved to that
/// label (length ≥ 2 — singletons are not duplicates and never appear).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub work_unit_label: String,
    pub ritual_ids: Vec<String>,
}

/// Scan `rituals_dir` for *terminal* rituals that share a `work_unit.label()`,
/// returning one [`DuplicateGroup`] per colliding label (groups of size ≥ 2).
///
/// This is the read-only half of the orphan reconciler (ISS-028 Task 1b /
/// GOAL-5): historical / completed ritual files accumulate in
/// `.gid/rituals/`, and when two terminal rituals share a work unit it
/// usually indicates a past leak (a ritual that should have been resumed
/// instead spawned a duplicate, or sweep-on-startup raced with a
/// concurrent start). We surface the collision but **never delete** —
/// these files are forensic record for the human to triage.
///
/// Companion to [`find_active_for_work_unit`] (which catches *future*
/// duplicates pre-flight) and [`sweep_orphans`] (which only touches
/// non-terminal zombies).
///
/// Notes:
/// - Rituals with `work_unit = None` are skipped (legacy state files
///   from before the `WorkUnit` field landed have no collision key).
/// - Unparsable / unreadable JSON files are silently skipped — same
///   defensive policy as `sweep_orphans_in`. A lone bad file must not
///   stop the reconciler.
/// - Result ordering: groups are sorted by `work_unit_label` for
///   deterministic logging; `ritual_ids` within each group are sorted
///   for the same reason.
pub fn reconcile_terminal_duplicates(
    rituals_dir: &Path,
) -> Result<Vec<DuplicateGroup>> {
    use std::collections::BTreeMap;

    let mut by_label: BTreeMap<String, Vec<String>> = BTreeMap::new();

    if !rituals_dir.exists() {
        return Ok(Vec::new());
    }

    for entry in std::fs::read_dir(rituals_dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("reconcile: dirent failed: {}", e);
                continue;
            }
        };
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }

        let data = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?path, "reconcile: read failed: {}", e);
                continue;
            }
        };
        let state: RitualState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?path, "reconcile: parse failed: {}", e);
                continue;
            }
        };

        // Only terminal rituals participate in duplicate detection —
        // active/idle collisions are handled pre-flight by
        // `find_active_for_work_unit` (ISS-028 Task 1a).
        if !state.phase.is_terminal() {
            continue;
        }
        let Some(label) = state.work_unit.as_ref().map(|w| w.label())
        else {
            continue;
        };

        by_label.entry(label).or_default().push(state.id);
    }

    let mut groups: Vec<DuplicateGroup> = by_label
        .into_iter()
        .filter(|(_, ids)| ids.len() >= 2)
        .map(|(label, mut ids)| {
            ids.sort();
            DuplicateGroup {
                work_unit_label: label,
                ritual_ids: ids,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.work_unit_label.cmp(&b.work_unit_label));
    Ok(groups)
}

/// Run the orphan reconciler at startup: scan `rituals_dir` for terminal
/// duplicates and WARN-log each collision. Returns the same group list
/// as [`reconcile_terminal_duplicates`] for caller-side metrics/tests.
///
/// ISS-028 Task 1b / GOAL-5: this is intentionally **observe-only** — no
/// state mutation, no file deletion. The point is to make duplicate
/// terminal rituals *visible* in daemon logs so a human notices the
/// pattern, not to silently clean up.
///
/// Logging shape (one record per group):
/// - level: `WARN`
/// - fields: `work_unit_label`, `count`, `ritual_ids`
///
/// Fail-soft: if the scan itself errors (I/O), we log the error and
/// return an empty list rather than aborting daemon startup.
pub fn reconcile_orphans(rituals_dir: &Path) -> Vec<DuplicateGroup> {
    let groups = match reconcile_terminal_duplicates(rituals_dir) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("orphan reconcile failed: {}", e);
            return Vec::new();
        }
    };

    if groups.is_empty() {
        tracing::debug!("orphan reconcile: no terminal duplicates");
        return groups;
    }

    for g in &groups {
        tracing::warn!(
            work_unit_label = %g.work_unit_label,
            count = g.ritual_ids.len(),
            ritual_ids = ?g.ritual_ids,
            "duplicate terminal rituals detected for {} ({} files): {:?}",
            g.work_unit_label,
            g.ritual_ids.len(),
            g.ritual_ids
        );
    }
    groups
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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests — duplicate ritual prevention (ISS-028 Task 1a)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod duplicate_prevention_tests {
    use super::*;
    use gid_core::ritual::work_unit::WorkUnit;
    use std::path::Path;
    use tempfile::TempDir;

    /// Write a state file directly to `dir/<id>.json` (mirrors the helper
    /// used by `orphan_sweep_tests`).
    fn write_state(dir: &Path, state: &RitualState) {
        let path = dir.join(format!("{}.json", state.id));
        let data = serde_json::to_string_pretty(state).unwrap();
        std::fs::write(path, data).unwrap();
    }

    /// Build a non-terminal state stamped with the given `WorkUnit`. Phase
    /// defaults to `Implementing` — a representative non-terminal phase
    /// matching the real-world ritual files we collide with.
    fn make_state_for(id: &str, work_unit: WorkUnit, phase: RitualPhase) -> RitualState {
        let mut s = RitualState::new();
        s.id = id.to_string();
        s.task = format!("test ritual for {}", work_unit.label());
        s.work_unit = Some(work_unit);
        s = s.with_phase(phase);
        s
    }

    fn iss_028() -> WorkUnit {
        WorkUnit::Issue {
            project: "rustclaw".into(),
            id: "ISS-028".into(),
        }
    }

    fn iss_029() -> WorkUnit {
        WorkUnit::Issue {
            project: "rustclaw".into(),
            id: "ISS-029".into(),
        }
    }

    #[test]
    fn no_active_ritual_returns_none() {
        // Empty directory — pre-flight should let the new ritual proceed.
        let tmp = TempDir::new().unwrap();
        let hit = find_active_for_work_unit(tmp.path(), &iss_028().label()).unwrap();
        assert!(hit.is_none(), "empty dir → no collision");

        // Directory with one ritual but for a *different* work unit also
        // returns None: the lookup key is the work unit label, not "any ritual".
        let other = make_state_for("r-other", iss_029(), RitualPhase::Implementing);
        write_state(tmp.path(), &other);
        let hit = find_active_for_work_unit(tmp.path(), &iss_028().label()).unwrap();
        assert!(hit.is_none(), "different work unit → no collision");
    }

    #[test]
    fn duplicate_active_ritual_is_detected() {
        // An in-flight ritual on ISS-028 must block a second start on ISS-028.
        let tmp = TempDir::new().unwrap();
        let active = make_state_for("r-active", iss_028(), RitualPhase::Implementing);
        write_state(tmp.path(), &active);

        let hit = find_active_for_work_unit(tmp.path(), &iss_028().label())
            .unwrap()
            .expect("non-terminal ritual on same work unit must be detected");
        assert_eq!(hit.id, "r-active");
        assert_eq!(hit.phase, RitualPhase::Implementing);
        assert_eq!(
            hit.work_unit.as_ref().map(|w| w.label()),
            Some(iss_028().label())
        );
    }

    #[test]
    fn terminal_ritual_does_not_block_new_start() {
        // The first ritual on ISS-028 has finished (Done) — a fresh start
        // on ISS-028 is allowed. This is the canonical "complete the
        // work, then iterate" workflow and must not trigger AlreadyActive.
        // Verifies we reuse `phase.is_terminal()` (ISS-019), so all of
        // {Done, Cancelled, Escalated} count as cleared.
        let tmp = TempDir::new().unwrap();

        for (id, phase) in [
            ("r-done", RitualPhase::Done),
            ("r-cancelled", RitualPhase::Cancelled),
            ("r-escalated", RitualPhase::Escalated),
        ] {
            let state = make_state_for(id, iss_028(), phase);
            write_state(tmp.path(), &state);
        }

        let hit = find_active_for_work_unit(tmp.path(), &iss_028().label()).unwrap();
        assert!(
            hit.is_none(),
            "all matching rituals are terminal → pre-flight must clear; got: {:?}",
            hit.as_ref().map(|r| (&r.id, r.phase.clone()))
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests — terminal duplicate reconciler (ISS-028 Task 1b)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod reconcile_tests {
    use super::*;
    use gid_core::ritual::work_unit::WorkUnit;
    use tempfile::TempDir;

    fn iss(id: &str) -> WorkUnit {
        WorkUnit::Issue {
            project: "rustclaw".into(),
            id: id.into(),
        }
    }

    fn write_terminal(
        dir: &Path,
        ritual_id: &str,
        wu: WorkUnit,
        phase: RitualPhase,
    ) {
        assert!(phase.is_terminal(), "fixture: phase must be terminal");
        let mut s = RitualState::new();
        s.id = ritual_id.into();
        s.task = format!("test {}", wu.label());
        s.work_unit = Some(wu);
        s = s.with_phase(phase);
        let path = dir.join(format!("{}.json", ritual_id));
        std::fs::write(path, serde_json::to_string_pretty(&s).unwrap()).unwrap();
    }

    /// Two terminal rituals on the same work unit → one duplicate group.
    /// This is the canonical "leak in history" the reconciler exists to
    /// surface (ISS-028 GOAL-5).
    #[test]
    fn two_terminal_rituals_same_work_unit_form_one_group() {
        let tmp = TempDir::new().unwrap();
        write_terminal(tmp.path(), "r-001", iss("ISS-028"), RitualPhase::Done);
        write_terminal(
            tmp.path(),
            "r-002",
            iss("ISS-028"),
            RitualPhase::Cancelled,
        );

        let groups = reconcile_terminal_duplicates(tmp.path()).unwrap();
        assert_eq!(groups.len(), 1, "exactly one duplicate group expected");
        let g = &groups[0];
        assert_eq!(g.work_unit_label, iss("ISS-028").label());
        assert_eq!(g.ritual_ids, vec!["r-001".to_string(), "r-002".to_string()]);
    }

    /// Singletons are not duplicates: distinct work units, even if both
    /// terminal, must not be reported. This is the no-false-positive
    /// case — a healthy `.gid/rituals/` directory full of one-per-issue
    /// completions stays silent.
    #[test]
    fn distinct_work_units_yield_no_groups() {
        let tmp = TempDir::new().unwrap();
        write_terminal(tmp.path(), "r-001", iss("ISS-028"), RitualPhase::Done);
        write_terminal(tmp.path(), "r-002", iss("ISS-029"), RitualPhase::Done);

        let groups = reconcile_terminal_duplicates(tmp.path()).unwrap();
        assert!(groups.is_empty(), "different work units → no group");
    }

    /// A non-terminal ritual sharing a label with a terminal one is *not*
    /// a duplicate-history case — that's the active-collision case that
    /// `find_active_for_work_unit` handles. The reconciler must filter
    /// non-terminal phases out before grouping.
    #[test]
    fn non_terminal_ritual_excluded_from_groups() {
        let tmp = TempDir::new().unwrap();
        write_terminal(tmp.path(), "r-done", iss("ISS-028"), RitualPhase::Done);
        // Hand-write a non-terminal ritual on the same work unit.
        let mut active = RitualState::new();
        active.id = "r-active".into();
        active.task = "active".into();
        active.work_unit = Some(iss("ISS-028"));
        active = active.with_phase(RitualPhase::Implementing);
        std::fs::write(
            tmp.path().join("r-active.json"),
            serde_json::to_string_pretty(&active).unwrap(),
        )
        .unwrap();

        let groups = reconcile_terminal_duplicates(tmp.path()).unwrap();
        assert!(
            groups.is_empty(),
            "non-terminal ritual must not pair with terminal one; got: {:?}",
            groups
        );
    }

    /// Reconciler must survive a missing rituals dir (fresh checkout)
    /// without erroring — daemon startup runs before any ritual exists.
    #[test]
    fn missing_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let groups = reconcile_terminal_duplicates(&missing).unwrap();
        assert!(groups.is_empty());
    }
}
