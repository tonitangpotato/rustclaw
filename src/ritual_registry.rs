//! Ritual Registry — gives the main agent situational awareness of active rituals.
//!
//! ISS-016: The main agent has no view into ritual state. When a ritual is running
//! (either a background adapter or the main agent itself as `SingleLlm` executor),
//! the agent needs to know: which ritual, what phase, who is the executor, and
//! whether it's suspected stuck.
//!
//! This module:
//! 1. Scans configured `known_project_roots` for `.gid/rituals/*.json` state files.
//! 2. Parses each into a lightweight `ActiveRitual` struct.
//! 3. Caches results with a TTL (default 5s) so prompt-build overhead is near-zero.
//! 4. Renders a `## Active Ritual Status` Markdown section for injection into the
//!    system prompt.
//!
//! # Performance
//!
//! The registry is designed for prompt-build hot path: one cache-hit read is a
//! single `RwLock::read` + comparison (<1µs). Cache miss scans state files
//! synchronously (a handful of small JSON reads — typically <10ms even for
//! multiple project roots).

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

// ─── Public types ──────────────────────────────────────────────────────────

/// Snapshot of a single active ritual, suitable for display in the system prompt.
#[derive(Debug, Clone)]
pub struct ActiveRitual {
    /// Ritual ID, e.g. "r-f8366c".
    pub id: String,
    /// Filesystem root of the project the ritual targets.
    pub project_root: PathBuf,
    /// Path to the state JSON file backing this ritual.
    pub state_path: PathBuf,
    /// Current phase (e.g. "Implementing", "Reviewing"). Free-form string so the
    /// registry does not need to stay lock-stepped with the `RitualPhase` enum.
    pub phase: String,
    /// Strategy ("SingleLlm" or "MultiAgent"), or None if state pre-dates triage.
    pub strategy: Option<String>,
    /// First 200 chars of the ritual's task description (human summary).
    pub task_summary: String,
    /// verify_retries counter — useful signal for "stuck verifying" pathology.
    pub verify_retries: u32,
    /// Last time the state file was updated (per its internal `updated_at` field).
    pub updated_at: DateTime<Utc>,
    /// True when the ritual appears stuck: no `updated_at` progression for
    /// longer than the configured `stuck_threshold`.
    pub suspected_stuck: bool,
    /// Whether the main agent in *this* process is the SingleLlm executor.
    /// Set to `true` only when strategy == SingleLlm AND the adapter PID matches
    /// the current process. For MultiAgent or unknown, `false`.
    pub you_are_executor: bool,
    /// PID of the ritual adapter process, if the state file recorded it.
    pub adapter_pid: Option<u32>,
    /// ISS-029b: structured health classification computed at scan time
    /// from `phase_entered_at` + `last_heartbeat`. Captured here as a
    /// human-readable string (rather than the gid-core enum directly) so
    /// rendering is decoupled from the enum's evolution. Format examples:
    /// "Healthy (45s in phase)", "LongRunning (820s, expected ≤300s)",
    /// "Wedged (no heartbeat for 142s)", "Terminal (Done)".
    /// `None` for state files written before ISS-029a (no
    /// `phase_entered_at`/`last_heartbeat` fields).
    pub health: Option<String>,
}

impl ActiveRitual {
    /// Human-readable "time since last update", e.g. "12s ago", "4m ago", "2h ago".
    pub fn age_display(&self) -> String {
        let now = Utc::now();
        let age = now.signed_duration_since(self.updated_at);
        format_duration(age)
    }
}

/// Registry configuration — derived from `rustclaw.yaml` `ritual:` block.
#[derive(Debug, Clone)]
pub struct RitualRegistryConfig {
    /// Absolute paths to project roots to scan for rituals. Each root is
    /// expected to contain a `.gid/rituals/` directory.
    pub known_roots: Vec<PathBuf>,
    /// Cache time-to-live. Default: 5 seconds.
    pub ttl: Duration,
    /// How long a ritual may sit at the same `updated_at` before we flag
    /// `suspected_stuck`. Default: 180 seconds.
    pub stuck_threshold: Duration,
    /// Rituals whose `updated_at` is older than `dead_threshold` are skipped
    /// entirely (treated as crashed processes). Default: 600 seconds.
    pub dead_threshold: Duration,
}

impl Default for RitualRegistryConfig {
    fn default() -> Self {
        Self {
            known_roots: Vec::new(),
            ttl: Duration::from_secs(5),
            stuck_threshold: Duration::from_secs(180),
            dead_threshold: Duration::from_secs(600),
        }
    }
}

/// Cross-workspace registry of active rituals, with TTL cache.
pub struct RitualRegistry {
    config: RitualRegistryConfig,
    cache: RwLock<Option<CachedSnapshot>>,
}

struct CachedSnapshot {
    rituals: Vec<ActiveRitual>,
    captured_at: Instant,
}

impl std::fmt::Debug for RitualRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RitualRegistry")
            .field("known_roots", &self.config.known_roots)
            .field("ttl", &self.config.ttl)
            .field("stuck_threshold", &self.config.stuck_threshold)
            .finish()
    }
}

// ─── Implementation ────────────────────────────────────────────────────────

impl RitualRegistry {
    pub fn new(config: RitualRegistryConfig) -> Self {
        Self {
            config,
            cache: RwLock::new(None),
        }
    }

    /// Fetch active rituals, using the TTL cache if fresh.
    pub fn active_rituals(&self) -> Vec<ActiveRitual> {
        // Fast path: read cache if fresh.
        if let Ok(guard) = self.cache.read() {
            if let Some(snap) = guard.as_ref() {
                if snap.captured_at.elapsed() < self.config.ttl {
                    return snap.rituals.clone();
                }
            }
        }
        // Slow path: rescan and update cache.
        let rituals = self.scan();
        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(CachedSnapshot {
                rituals: rituals.clone(),
                captured_at: Instant::now(),
            });
        }
        rituals
    }

    /// Invalidate the cache — call after starting/cancelling a ritual so the
    /// next prompt build sees fresh state.
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.cache.write() {
            *guard = None;
        }
    }

    /// Render the `## Active Ritual Status` markdown block for system prompt
    /// injection. Returns `None` when no rituals are active (caller should
    /// skip emitting a section entirely).
    pub fn render_prompt_section(&self) -> Option<String> {
        let rituals = self.active_rituals();
        if rituals.is_empty() {
            return None;
        }
        Some(render_section(&rituals))
    }

    // ─── Internals ──────────────────────────────────────────────────────────

    fn scan(&self) -> Vec<ActiveRitual> {
        let now = Utc::now();
        let my_pid = std::process::id();
        let mut out = Vec::new();
        for root in &self.config.known_roots {
            let rituals_dir = root.join(".gid").join("rituals");
            if !rituals_dir.is_dir() {
                continue;
            }
            let entries = match std::fs::read_dir(&rituals_dir) {
                Ok(it) => it,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                match parse_state_file(&path, root, now, &self.config, my_pid) {
                    Ok(Some(active)) => out.push(active),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::debug!(
                            "ritual_registry: failed to parse {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }
        // Sort: suspected_stuck first, then most-recently-updated first.
        out.sort_by(|a, b| {
            b.suspected_stuck
                .cmp(&a.suspected_stuck)
                .then(b.updated_at.cmp(&a.updated_at))
        });
        out
    }
}

// ─── Parsing ───────────────────────────────────────────────────────────────

/// Minimal deserialization view of the ritual state file. We only pull fields
/// we actually render, to avoid tight coupling with gid-core's evolving schema.
#[derive(Debug, Deserialize)]
struct StateFile {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    phase: Option<serde_json::Value>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    strategy: Option<serde_json::Value>,
    #[serde(default)]
    verify_retries: Option<u32>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    /// Optional: adapter PID if the adapter has been taught to record it.
    #[serde(default)]
    adapter_pid: Option<u32>,
    /// ISS-029a: phase entry timestamp, used by `health()` to detect
    /// long-running phases. Optional for backwards-compat with state
    /// files written before ISS-029a.
    #[serde(default)]
    phase_entered_at: Option<DateTime<Utc>>,
    /// ISS-029a: last event-loop tick timestamp, used by `health()` to
    /// detect wedged rituals. Optional for backwards-compat.
    #[serde(default)]
    last_heartbeat: Option<DateTime<Utc>>,
}

fn parse_state_file(
    path: &Path,
    project_root: &Path,
    now: DateTime<Utc>,
    cfg: &RitualRegistryConfig,
    my_pid: u32,
) -> Result<Option<ActiveRitual>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let state: StateFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    let id = match state.id {
        Some(s) if !s.is_empty() => s,
        _ => path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("r-unknown")
            .to_string(),
    };

    let phase = phase_to_string(&state.phase).unwrap_or_else(|| "Unknown".to_string());
    if is_terminal_phase(&phase) {
        return Ok(None);
    }

    let updated_at = state.updated_at.unwrap_or(now);
    let age = now.signed_duration_since(updated_at);
    let dead_threshold = ChronoDuration::from_std(cfg.dead_threshold)
        .unwrap_or_else(|_| ChronoDuration::seconds(600));
    if age > dead_threshold {
        return Ok(None);
    }

    let stuck_threshold = ChronoDuration::from_std(cfg.stuck_threshold)
        .unwrap_or_else(|_| ChronoDuration::seconds(180));
    let suspected_stuck = age > stuck_threshold;

    let strategy = strategy_to_string(&state.strategy);
    let task_summary = state
        .task
        .as_deref()
        .map(truncate_summary)
        .unwrap_or_default();

    let you_are_executor = matches!(strategy.as_deref(), Some("SingleLlm"))
        && state.adapter_pid.map(|p| p == my_pid).unwrap_or(false);

    // ISS-029b: classify health when the state file is new enough to
    // carry `phase_entered_at` + `last_heartbeat`. Both fields are
    // required — partial information would silently misclassify wedged
    // rituals. For older state files (pre-ISS-029a) we leave `health`
    // as `None` and the renderer falls back to the legacy
    // `suspected_stuck` heuristic.
    let health = match (state.phase_entered_at, state.last_heartbeat) {
        (Some(entered), Some(beat)) => {
            Some(classify_health(&phase, entered, beat, now))
        }
        _ => None,
    };

    Ok(Some(ActiveRitual {
        id,
        project_root: project_root.to_path_buf(),
        state_path: path.to_path_buf(),
        phase,
        strategy,
        task_summary,
        verify_retries: state.verify_retries.unwrap_or(0),
        updated_at,
        suspected_stuck,
        you_are_executor,
        adapter_pid: state.adapter_pid,
        health,
    }))
}

/// ISS-029b: minimal local replica of `gid_core::ritual::RitualHealth`'s
/// classification logic. Kept here (rather than importing
/// `RitualState::health()`) because the registry only deserializes a
/// narrow subset of state fields — pulling in the full `RitualState`
/// schema just to call `health()` would couple us to every gid-core
/// schema change.
///
/// Thresholds match `gid_core::ritual::state_machine`:
/// - `WEDGED_HEARTBEAT_THRESHOLD_SECS` = 60s
/// - terminal phases short-circuit regardless of heartbeat freshness
/// - long-running threshold is a single fixed soft budget (300s) here;
///   gid-core has per-phase budgets but the registry doesn't carry the
///   phase-typed enum, so we use a conservative single value. This
///   under-flags `LongRunning` (vs gid-core), never over-flags — which
///   is the safe direction for a status-display heuristic.
fn classify_health(
    phase: &str,
    phase_entered_at: DateTime<Utc>,
    last_heartbeat: DateTime<Utc>,
    now: DateTime<Utc>,
) -> String {
    if is_terminal_phase(phase) {
        return format!("Terminal ({})", phase);
    }
    let beat_age = (now - last_heartbeat).num_seconds().max(0);
    let phase_age = (now - phase_entered_at).num_seconds().max(0);
    const WEDGED_THRESHOLD_SECS: i64 = 60;
    const LONG_RUNNING_THRESHOLD_SECS: i64 = 300;

    if beat_age > WEDGED_THRESHOLD_SECS {
        return format!("Wedged (no heartbeat for {}s)", beat_age);
    }
    if phase_age > LONG_RUNNING_THRESHOLD_SECS {
        return format!(
            "LongRunning ({}s in phase, expected ≤{}s)",
            phase_age, LONG_RUNNING_THRESHOLD_SECS
        );
    }
    format!("Healthy ({}s in phase)", phase_age)
}

/// Convert the phase JSON value (which may be a bare string like `"Implementing"`
/// or an object `{"Reviewing": {...}}`) into a display string.
fn phase_to_string(val: &Option<serde_json::Value>) -> Option<String> {
    match val.as_ref()? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => map.keys().next().cloned(),
        _ => None,
    }
}

fn strategy_to_string(val: &Option<serde_json::Value>) -> Option<String> {
    match val.as_ref()? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => map.keys().next().cloned(),
        _ => None,
    }
}

fn is_terminal_phase(phase: &str) -> bool {
    matches!(phase, "Done" | "Cancelled" | "Escalated" | "Failed")
}

fn truncate_summary(s: &str) -> String {
    const MAX: usize = 200;
    // Find the first newline; the first line is typically the clearest summary.
    let first_line = s.lines().next().unwrap_or(s).trim();
    if first_line.chars().count() <= MAX {
        return first_line.to_string();
    }
    let mut out = String::with_capacity(MAX + 1);
    for (i, c) in first_line.chars().enumerate() {
        if i >= MAX {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

fn format_duration(age: ChronoDuration) -> String {
    let secs = age.num_seconds().max(0);
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

// ─── Rendering ─────────────────────────────────────────────────────────────

const MAX_LISTED_RITUALS: usize = 5;

fn render_section(rituals: &[ActiveRitual]) -> String {
    let mut s = String::new();
    s.push_str("## Active Ritual Status\n\n");

    let listed: Vec<&ActiveRitual> = rituals.iter().take(MAX_LISTED_RITUALS).collect();
    let hidden = rituals.len().saturating_sub(listed.len());

    let any_executor = listed.iter().any(|r| r.you_are_executor);
    let any_stuck = listed.iter().any(|r| r.suspected_stuck);

    if any_executor {
        s.push_str(
            "⚠️ **You are currently the executor of a SingleLlm ritual.** \
             You are NOT a generic assistant — you are driving the ritual pipeline. \
             Follow the active phase's skill instructions, respect its tool scope, \
             and do not ask \"what should I do?\" — the ritual phase tells you.\n\n",
        );
    } else {
        s.push_str(&format!(
            "⚠️ **{} ritual{} currently running.** \
             Ritual state is first-class situational awareness: \
             do not start a parallel ritual, and when the user asks about progress, \
             summarize from the data below rather than calling tools.\n\n",
            rituals.len(),
            if rituals.len() == 1 { "" } else { "s" },
        ));
    }

    if any_stuck {
        s.push_str(
            "🚨 **At least one ritual appears stuck** (no state update for a long time). \
             Proactively tell the user which phase is stalled, and suggest \
             `/ritual status` or cancelling.\n\n",
        );
    }

    for r in &listed {
        s.push_str(&render_one(r));
    }

    if hidden > 0 {
        s.push_str(&format!("- …and {} more ritual(s) not shown.\n\n", hidden));
    }

    s.push_str(
        "### Rules when ≥1 ritual is active\n\
         - Do NOT start a new ritual (no `gid_ritual_init`, no nested `/ritual`).\n\
         - If the user wants you to take over manually → cancel the ritual first, then proceed.\n\
         - If a ritual is flagged stuck → report specifics (ID, phase, age) to the user proactively.\n\
         - Answer \"what ritual is running?\" from the data above, without tool calls.\n",
    );

    s
}

fn render_one(r: &ActiveRitual) -> String {
    let mut s = String::new();
    let stuck_marker = if r.suspected_stuck { " 🚨 stuck" } else { "" };
    let executor_marker = if r.you_are_executor {
        " · YOU ARE THE EXECUTOR"
    } else {
        ""
    };
    s.push_str(&format!(
        "### `{}` — {}{}{}\n",
        r.id, r.phase, stuck_marker, executor_marker
    ));
    s.push_str(&format!("- **Project**: `{}`\n", r.project_root.display()));
    if !r.task_summary.is_empty() {
        s.push_str(&format!("- **Task**: {}\n", r.task_summary));
    }
    if let Some(strategy) = &r.strategy {
        s.push_str(&format!("- **Strategy**: {}\n", strategy));
    }
    if r.verify_retries > 0 {
        s.push_str(&format!("- **Verify retries**: {}\n", r.verify_retries));
    }
    // ISS-029b: surface structured health classification when present.
    // Older state files (pre-ISS-029a) lack `phase_entered_at` /
    // `last_heartbeat`, in which case `health` is None and we fall back
    // to the legacy `suspected_stuck` marker on the heading line.
    if let Some(h) = &r.health {
        s.push_str(&format!("- **Health**: {}\n", h));
    }
    s.push_str(&format!("- **Last update**: {}\n", r.age_display()));
    s.push_str(&format!("- **State file**: `{}`\n\n", r.state_path.display()));
    s
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn write_state(dir: &Path, id: &str, body: serde_json::Value) -> PathBuf {
        let rituals = dir.join(".gid").join("rituals");
        fs::create_dir_all(&rituals).unwrap();
        let path = rituals.join(format!("{}.json", id));
        fs::write(&path, serde_json::to_string_pretty(&body).unwrap()).unwrap();
        path
    }

    fn make_registry(root: &Path) -> RitualRegistry {
        RitualRegistry::new(RitualRegistryConfig {
            known_roots: vec![root.to_path_buf()],
            ttl: Duration::from_millis(0), // disable cache for deterministic tests
            stuck_threshold: Duration::from_secs(180),
            dead_threshold: Duration::from_secs(600),
        })
    }

    #[test]
    fn ac2_no_rituals_no_section() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = make_registry(tmp.path());
        assert!(reg.active_rituals().is_empty());
        assert!(reg.render_prompt_section().is_none());
    }

    #[test]
    fn ac1_single_active_ritual_renders_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        write_state(
            tmp.path(),
            "r-aaa111",
            json!({
                "id": "r-aaa111",
                "phase": "Implementing",
                "task": "Fix ISS-015: PRAGMA foreign_keys inside transaction\nmore details",
                "strategy": "SingleLlm",
                "verify_retries": 1,
                "updated_at": now,
            }),
        );
        let reg = make_registry(tmp.path());
        let rituals = reg.active_rituals();
        assert_eq!(rituals.len(), 1, "should find exactly one active ritual");
        let r = &rituals[0];
        assert_eq!(r.id, "r-aaa111");
        assert_eq!(r.phase, "Implementing");
        assert_eq!(r.strategy.as_deref(), Some("SingleLlm"));
        assert_eq!(r.verify_retries, 1);
        assert!(!r.suspected_stuck);
        assert!(r
            .task_summary
            .contains("Fix ISS-015"));

        let section = reg.render_prompt_section().expect("section should render");
        assert!(section.contains("## Active Ritual Status"));
        assert!(section.contains("r-aaa111"));
        assert!(section.contains("Implementing"));
        assert!(section.contains("SingleLlm"));
        assert!(section.contains("Verify retries"));
    }

    #[test]
    fn ac4_stuck_ritual_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        let long_ago = Utc::now() - ChronoDuration::seconds(400);
        write_state(
            tmp.path(),
            "r-stuck1",
            json!({
                "id": "r-stuck1",
                "phase": "Reviewing",
                "task": "do a thing",
                "updated_at": long_ago,
            }),
        );
        let reg = make_registry(tmp.path());
        let rituals = reg.active_rituals();
        assert_eq!(rituals.len(), 1);
        assert!(rituals[0].suspected_stuck, "should flag stuck");
        let section = reg.render_prompt_section().unwrap();
        assert!(section.contains("stuck"));
    }

    #[test]
    fn ac6_terminal_phases_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        for (id, phase) in [
            ("r-done1", "Done"),
            ("r-cancel1", "Cancelled"),
            ("r-esc1", "Escalated"),
        ] {
            write_state(
                tmp.path(),
                id,
                json!({
                    "id": id,
                    "phase": phase,
                    "task": "t",
                    "updated_at": now,
                }),
            );
        }
        // also include one active to make sure the filter isn't too strict
        write_state(
            tmp.path(),
            "r-live1",
            json!({
                "id": "r-live1",
                "phase": "Planning",
                "task": "t",
                "updated_at": now,
            }),
        );
        let reg = make_registry(tmp.path());
        let rituals = reg.active_rituals();
        assert_eq!(rituals.len(), 1, "terminal phases must be excluded");
        assert_eq!(rituals[0].id, "r-live1");
    }

    #[test]
    fn ac6_dead_ritual_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let ancient = Utc::now() - ChronoDuration::hours(2);
        write_state(
            tmp.path(),
            "r-dead1",
            json!({
                "id": "r-dead1",
                "phase": "Implementing",
                "task": "t",
                "updated_at": ancient,
            }),
        );
        let reg = make_registry(tmp.path());
        assert!(reg.active_rituals().is_empty(), "2h-old ritual is dead");
    }

    #[test]
    fn you_are_executor_requires_pid_match_and_singlellm() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let my_pid = std::process::id();

        // SingleLlm + matching pid → you_are_executor
        write_state(
            tmp.path(),
            "r-me",
            json!({
                "id": "r-me",
                "phase": "Implementing",
                "task": "t",
                "strategy": "SingleLlm",
                "updated_at": now,
                "adapter_pid": my_pid,
            }),
        );
        // SingleLlm + different pid → NOT you_are_executor
        write_state(
            tmp.path(),
            "r-other",
            json!({
                "id": "r-other",
                "phase": "Implementing",
                "task": "t",
                "strategy": "SingleLlm",
                "updated_at": now,
                "adapter_pid": my_pid + 12345,
            }),
        );
        // MultiAgent + matching pid → NOT you_are_executor
        write_state(
            tmp.path(),
            "r-multi",
            json!({
                "id": "r-multi",
                "phase": "Implementing",
                "task": "t",
                "strategy": {"MultiAgent": {"tasks": []}},
                "updated_at": now,
                "adapter_pid": my_pid,
            }),
        );
        let reg = make_registry(tmp.path());
        let rituals = reg.active_rituals();
        assert_eq!(rituals.len(), 3);

        let find = |id: &str| rituals.iter().find(|r| r.id == id).unwrap();
        assert!(find("r-me").you_are_executor);
        assert!(!find("r-other").you_are_executor);
        assert!(!find("r-multi").you_are_executor);
    }

    #[test]
    fn cache_hit_avoids_rescan() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        write_state(
            tmp.path(),
            "r-cache",
            json!({
                "id": "r-cache",
                "phase": "Planning",
                "task": "t",
                "updated_at": now,
            }),
        );
        let reg = RitualRegistry::new(RitualRegistryConfig {
            known_roots: vec![tmp.path().to_path_buf()],
            ttl: Duration::from_secs(60), // long TTL
            stuck_threshold: Duration::from_secs(180),
            dead_threshold: Duration::from_secs(600),
        });
        let first = reg.active_rituals();
        assert_eq!(first.len(), 1);

        // Delete the underlying file — cache should still report it.
        std::fs::remove_dir_all(tmp.path().join(".gid")).unwrap();
        let second = reg.active_rituals();
        assert_eq!(second.len(), 1, "cache must survive underlying deletion");

        // Invalidate → rescan → empty.
        reg.invalidate();
        let third = reg.active_rituals();
        assert!(third.is_empty());
    }

    #[test]
    fn phase_object_form_parses() {
        // Some future schema may emit phase as {"Implementing":{...}}.
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        write_state(
            tmp.path(),
            "r-objphase",
            json!({
                "id": "r-objphase",
                "phase": {"Implementing": {"some_field": 1}},
                "task": "t",
                "updated_at": now,
            }),
        );
        let reg = make_registry(tmp.path());
        let rituals = reg.active_rituals();
        assert_eq!(rituals.len(), 1);
        assert_eq!(rituals[0].phase, "Implementing");
    }
}
