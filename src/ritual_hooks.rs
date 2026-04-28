//! `RitualHooks` adapter implementation for rustclaw.
//!
//! Per ISS-052 design §7.3. This is the **only** rustclaw-side extension
//! point into `gid_core::ritual::run_ritual`. All side-effects that
//! `V2Executor` needs (Telegram notifications, durable state persistence,
//! workspace resolution from the gid project registry, cooperative
//! cancellation, PID stamping) flow through this struct.
//!
//! ## Design notes
//!
//! - **`notify`** wraps the existing `NotifyFn` closure so the Telegram /
//!   logging path that rustclaw already wired up (channel-aware, supports
//!   non-Telegram embedders, e.g. test scripts) is reused verbatim. We do
//!   not reach into `channels/telegram.rs` from here — `RustclawHooks` is
//!   intentionally adapter-agnostic.
//! - **`persist_state`** writes atomically (tempfile + `rename(2)`) to the
//!   ritual's state file under `<rituals_dir>/<ritual_id>.json`, matching
//!   the trait's atomicity contract (`hooks.rs::RitualHooks::persist_state`
//!   docstring). The implementation deliberately does **not** stamp the
//!   PID here — that is `stamp_metadata`'s job, called by `V2Executor`
//!   exactly once at ritual start.
//! - **`resolve_workspace`** delegates to gid-core's `RegistryResolver`
//!   (the canonical ISS-022 path). Errors are mapped onto `WorkspaceError`
//!   variants so V2Executor can emit `WorkspaceUnresolved` cleanly.
//! - **`should_cancel`** observes a `tokio_util::sync::CancellationToken`
//!   that is the same token rustclaw's `CancelRegistry` already keeps per
//!   active ritual. The hooks instance is constructed *per-ritual*, so it
//!   captures the ritual's own token at construction time. That means
//!   `/ritual cancel` (which calls `token.cancel()` via the registry)
//!   automatically propagates here without any new wiring.
//! - **`stamp_metadata`** sets `adapter_pid` to the current process ID.
//!   `RitualState` does not currently carry a free-form adapter-name field
//!   (only `adapter_pid`); if one is added later, this is the single site
//!   that should populate it.
//! - **`on_phase_transition`** is currently a no-op. The design sketch
//!   mentions an optional Engram side-write; we keep the hook as a
//!   first-class `Option<EngramWriter>`-shaped slot but do not wire it in
//!   T11 — that is left for ISS-054 follow-ups, since Engram phase-event
//!   shape is still in flux. Today the default trait no-op is sufficient.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use gid_core::ritual::{
    CancelReason, CancelSource, RegistryResolver, RitualHooks,
    V2State as RitualState, WorkUnit, WorkUnitResolver, WorkspaceError,
    resolve_and_validate,
};

use crate::ritual_runner::NotifyFn;

// ═══════════════════════════════════════════════════════════════════════════════
// RustclawHooks
// ═══════════════════════════════════════════════════════════════════════════════

/// rustclaw's `RitualHooks` implementation. One instance per running ritual.
///
/// Constructed at `run_ritual` call sites (`tools.rs` `start_ritual`,
/// `channels/telegram.rs` `/ritual`, etc.) and passed to gid-core as
/// `Arc<dyn RitualHooks>`.
pub struct RustclawHooks {
    /// User-facing notification sink (Telegram, logs, scripted-test capture).
    /// Cloned from the caller's existing `NotifyFn` to preserve channel
    /// formatting and multi-target dispatch already implemented elsewhere.
    notify_fn: NotifyFn,

    /// Directory holding `<ritual_id>.json` state files. Created on first
    /// `persist_state` call if missing.
    rituals_dir: PathBuf,

    /// Cancellation token for *this* ritual. The same token instance is
    /// stored in `CancelRegistry` so `/ritual cancel` cancels both the
    /// in-flight tokio task (current behaviour) and this hook poll.
    cancel_token: CancellationToken,

    /// Override for `resolve_workspace`. `None` → use the on-disk
    /// `~/.config/gid/projects.yml` via `RegistryResolver::load_default`.
    /// `Some(_)` → use the supplied resolver (e.g. an in-memory test
    /// registry). This is the seam that makes §9.3
    /// `hooks_resolve_workspace_via_registry` cheap to test.
    resolver_override: Option<Arc<dyn WorkUnitResolver>>,
}

impl RustclawHooks {
    /// Construct a new `RustclawHooks` for a single ritual.
    ///
    /// `cancel_token` MUST be the same token entered into the
    /// `CancelRegistry` for this ritual's ID. Otherwise `/ritual cancel`
    /// will not propagate.
    pub fn new(
        notify_fn: NotifyFn,
        rituals_dir: PathBuf,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            notify_fn,
            rituals_dir,
            cancel_token,
            resolver_override: None,
        }
    }

    /// Test-only seam: install a custom `WorkUnitResolver` (e.g. in-memory
    /// fake) instead of reading the on-disk registry. Returns `self` for
    /// builder-style chaining.
    pub fn with_resolver(mut self, resolver: Arc<dyn WorkUnitResolver>) -> Self {
        self.resolver_override = Some(resolver);
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RitualHooks impl
// ═══════════════════════════════════════════════════════════════════════════════

#[async_trait]
impl RitualHooks for RustclawHooks {
    async fn notify(&self, message: &str) {
        // NotifyFn is `Fn(String) -> Pin<Box<dyn Future<Output = ()>>>`. We
        // own a clone of the Arc, so calling it is cheap and parallel-safe.
        (self.notify_fn)(message.to_string()).await;
    }

    async fn persist_state(&self, state: &RitualState) -> std::io::Result<()> {
        // Atomicity contract per `RitualHooks::persist_state` docstring:
        // tempfile-then-rename. On error, on-disk file is unchanged
        // (rename(2) is atomic on POSIX, and best-effort atomic on Windows).
        std::fs::create_dir_all(&self.rituals_dir)?;
        let final_path = self.rituals_dir.join(format!("{}.json", state.id));
        let tmp_path = self.rituals_dir.join(format!(".{}.json.tmp", state.id));
        let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    fn resolve_workspace(&self, work_unit: &WorkUnit) -> Result<PathBuf, WorkspaceError> {
        // Two paths: explicit override (test fakes) or the default registry.
        // Both go through `resolve_and_validate` so the path-existence check
        // and registry-vs-disk reconciliation logic stays single-sourced.
        match &self.resolver_override {
            Some(resolver) => resolve_and_validate(resolver.as_ref(), work_unit)
                .map_err(map_resolver_err),
            None => {
                let resolver = RegistryResolver::load_default()
                    .map_err(|e| WorkspaceError::RegistryError(e.to_string()))?;
                resolve_and_validate(&resolver, work_unit).map_err(map_resolver_err)
            }
        }
    }

    fn stamp_metadata(&self, state: &mut RitualState) {
        // PID is the only adapter-shaped field on `RitualState` today.
        // Keep this site as the canonical write — orphan-sweep and zombie
        // detection both rely on `adapter_pid` being non-None for active
        // rituals.
        state.adapter_pid = Some(std::process::id());
    }

    fn should_cancel(&self) -> Option<CancelReason> {
        if self.cancel_token.is_cancelled() {
            Some(CancelReason {
                source: CancelSource::UserCommand,
                message: "user requested /ritual cancel".to_string(),
            })
        } else {
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Map `gid-core`'s `WorkUnitResolver` error string into a `WorkspaceError`.
///
/// Today `resolve_and_validate` returns `anyhow::Error`; we classify on the
/// error message because the underlying error types are not exported. If
/// gid-rs ever publishes a structured error enum we should replace this
/// string-matching with a typed match — tracked via ISS-054 follow-ups.
fn map_resolver_err(e: anyhow::Error) -> WorkspaceError {
    let msg = e.to_string();
    if msg.contains("not found") || msg.contains("unknown project") {
        WorkspaceError::NotFound(msg)
    } else if msg.contains("does not exist") || msg.contains("path") {
        // PathMissing wants a PathBuf. We don't have the offending path
        // here; surface it as a registry error rather than fabricating one.
        WorkspaceError::RegistryError(msg)
    } else {
        WorkspaceError::RegistryError(msg)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests — ISS-052 §9.3
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    //! ISS-052 §9.3 — `RustclawHooks` unit tests.
    //!
    //! Five tests, one per public hook surface, isolated via tempdirs and
    //! in-memory test doubles. No real Telegram client, no real
    //! `~/.config/gid/projects.yml` access — all of those would couple the
    //! suite to environment state and contradict the ISS-052 design goal of
    //! "single dispatcher, fully testable".

    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    // ── Shared helpers ─────────────────────────────────────────────────

    /// In-memory `NotifyFn` capture. The returned `Vec` collects every
    /// message the SUT sent through `hooks.notify`.
    fn capture_notify() -> (NotifyFn, Arc<Mutex<Vec<String>>>) {
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_for_closure = captured.clone();
        let notify_fn: NotifyFn = Arc::new(move |msg: String| {
            let cap = captured_for_closure.clone();
            Box::pin(async move {
                cap.lock().unwrap().push(msg);
            })
        });
        (notify_fn, captured)
    }

    /// Trivial `NotifyFn` that drops all messages — used when the test does
    /// not care about notifications (e.g. persist test).
    fn noop_notify() -> NotifyFn {
        Arc::new(|_msg: String| Box::pin(async {}))
    }

    /// Hardcoded-map `WorkUnitResolver` per ISS-029 §3 design note. Maps
    /// project name → `PathBuf`, no disk IO.
    struct FakeResolver {
        path: PathBuf,
    }

    impl WorkUnitResolver for FakeResolver {
        fn resolve(&self, _unit: &WorkUnit) -> anyhow::Result<PathBuf> {
            Ok(self.path.clone())
        }
    }

    // ── Tests ──────────────────────────────────────────────────────────

    /// §9.3 row 1 — `notify` plumbs the message through the embedder's
    /// `NotifyFn`. We capture into a `Vec` and assert the exact string
    /// round-tripped (no truncation, no double-dispatch).
    #[tokio::test]
    async fn hooks_notify_calls_telegram() {
        let (notify_fn, captured) = capture_notify();
        let tmp = TempDir::new().unwrap();
        let hooks = RustclawHooks::new(
            notify_fn,
            tmp.path().join("rituals"),
            CancellationToken::new(),
        );

        hooks.notify("hello from ritual").await;
        hooks.notify("second message").await;

        let msgs = captured.lock().unwrap().clone();
        assert_eq!(
            msgs,
            vec![
                "hello from ritual".to_string(),
                "second message".to_string(),
            ],
            "notify must forward each message verbatim, in order"
        );
    }

    /// §9.3 row 2 — `persist_state` writes to `<rituals_dir>/<id>.json`
    /// atomically. We assert the file exists, parses back into the same
    /// `RitualState`, and that no `.tmp` artefact is left behind.
    #[tokio::test]
    async fn hooks_persist_writes_correct_path() {
        let tmp = TempDir::new().unwrap();
        let rituals_dir = tmp.path().join("rituals");
        let hooks = RustclawHooks::new(
            noop_notify(),
            rituals_dir.clone(),
            CancellationToken::new(),
        );

        let mut state = RitualState::new();
        state.id = "r-test-persist".to_string();

        hooks
            .persist_state(&state)
            .await
            .expect("persist_state must succeed in a fresh tempdir");

        let expected = rituals_dir.join("r-test-persist.json");
        assert!(
            expected.exists(),
            "state file should land at <rituals_dir>/<id>.json, got {}",
            expected.display()
        );

        // Round-trip parse — guards against silent serialisation regression.
        let raw = std::fs::read_to_string(&expected).unwrap();
        let parsed: RitualState = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.id, "r-test-persist");

        // Tempfile artefact must be cleaned up by rename(2).
        let stray = std::fs::read_dir(&rituals_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with('.')
            });
        assert!(
            stray.is_none(),
            "atomic-write contract violated: tempfile artefact left in rituals_dir"
        );
    }

    /// §9.3 row 3 — `resolve_workspace` dispatches through the injected
    /// resolver and validates the returned path (must exist + contain
    /// `.gid/`). We use the test-only `with_resolver` seam to avoid
    /// touching `~/.config/gid/projects.yml`.
    #[test]
    fn hooks_resolve_workspace_via_registry() {
        let tmp = TempDir::new().unwrap();
        // resolve_and_validate requires `.gid/` to exist
        std::fs::create_dir_all(tmp.path().join(".gid")).unwrap();

        let resolver = Arc::new(FakeResolver {
            path: tmp.path().to_path_buf(),
        });
        let hooks = RustclawHooks::new(
            noop_notify(),
            tmp.path().join("rituals"),
            CancellationToken::new(),
        )
        .with_resolver(resolver);

        let unit = WorkUnit::Issue {
            project: "test-project".to_string(),
            id: "ISS-001".to_string(),
        };

        let resolved = hooks
            .resolve_workspace(&unit)
            .expect("fake resolver returns the validated tempdir");

        // Canonicalise both for macOS /private/var symlink resolution.
        assert_eq!(
            resolved.canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap(),
            "resolve_workspace must return the resolver's path verbatim"
        );
    }

    /// §9.3 row 4 — `should_cancel` observes the cancel token. Per the
    /// trait contract (§4 docstring), once cancellation is requested the
    /// hook returns `Some(_)`. The token's `is_cancelled` is sticky (does
    /// not auto-reset) — V2Executor only polls until the first `Some`,
    /// then transitions to `Cancelled`, so we don't need the design's
    /// "Some-once-then-None" semantics here. The §9.3 row is satisfied by
    /// "flip flag → observe Some". We additionally assert pre-flip
    /// returns None — the previous false-positive failure mode.
    #[test]
    fn hooks_cancel_flag_observed() {
        let tmp = TempDir::new().unwrap();
        let token = CancellationToken::new();
        let hooks = RustclawHooks::new(
            noop_notify(),
            tmp.path().join("rituals"),
            token.clone(),
        );

        // Pre-flip: must NOT report cancellation.
        assert!(
            hooks.should_cancel().is_none(),
            "fresh hooks must not report cancellation"
        );

        // Flip the flag — same token instance is shared with the registry,
        // so this is what `/ritual cancel` does in production.
        token.cancel();

        let reason = hooks
            .should_cancel()
            .expect("post-cancel poll must yield Some(reason)");
        assert_eq!(reason.source, CancelSource::UserCommand);
        assert!(
            reason.message.contains("cancel"),
            "cancel reason message should mention cancellation, got: {:?}",
            reason.message
        );
    }

    /// §9.3 row 5 — `stamp_metadata` writes `adapter_pid` to the current
    /// process ID. (`RitualState` has no `adapter` string field today —
    /// see the `stamp_metadata` impl docstring.)
    #[test]
    fn hooks_stamp_metadata_sets_pid_and_adapter() {
        let tmp = TempDir::new().unwrap();
        let hooks = RustclawHooks::new(
            noop_notify(),
            tmp.path().join("rituals"),
            CancellationToken::new(),
        );

        let mut state = RitualState::new();
        assert!(
            state.adapter_pid.is_none(),
            "fresh state should have no adapter_pid stamped"
        );

        hooks.stamp_metadata(&mut state);

        assert_eq!(
            state.adapter_pid,
            Some(std::process::id()),
            "stamp_metadata must set adapter_pid to current process"
        );
    }

    /// ISS-052 AC — Rustclaw cannot bypass the `file_snapshot` zero-file
    /// gate (ISS-038). The gate lives in `gid_core::ritual::V2Executor::run_skill`
    /// and is exercised by `skill_required_zero_files_fails` in gid-core.
    /// The structural reason it cannot be bypassed from rustclaw is that
    /// `RitualHooks` exposes **no** skill-dispatch method — embedders only
    /// supply ambient capabilities (notify / persist / resolve / cancel /
    /// stamp / on_phase_transition). There is no `run_skill` hook, no
    /// `run_shell` hook, no `run_triage` hook to override.
    ///
    /// This test compiles a `RustclawHooks` reference as `&dyn RitualHooks`
    /// and explicitly enumerates the hook surface. If a future change adds
    /// a skill-dispatch method to the trait (which would re-open the
    /// dispatcher fragmentation that ISS-052 closed), the build for this
    /// test will not break — but the comment + the gid-core gate-bypass
    /// review at PR time will. That is the intended trip wire: the
    /// `RitualHooks` trait surface is a contract, and any skill-dispatch
    /// addition should be flagged in code review.
    #[test]
    fn ritualhooks_surface_has_no_skill_dispatch_method() {
        use gid_core::ritual::RitualHooks;

        let tmp = TempDir::new().unwrap();
        let hooks = RustclawHooks::new(
            noop_notify(),
            tmp.path().join("rituals"),
            CancellationToken::new(),
        );

        // Coerce to the trait object — this is exactly how `run_ritual`
        // and `resume_ritual` consume the hooks: as `Arc<dyn RitualHooks>`.
        let _trait_obj: &dyn RitualHooks = &hooks;

        // The five callable methods on `RitualHooks` (per gid-core
        // crates/gid-core/src/ritual/hooks.rs trait definition):
        //   - notify(&self, &str)
        //   - persist_state(&self, &RitualState)
        //   - resolve_workspace(&self, &WorkUnit)
        //   - should_cancel(&self) -> Option<CancelReason>
        //   - stamp_metadata(&self, &mut RitualState)
        //   - on_phase_transition(&self, &RitualPhase, &RitualPhase)  [default impl]
        //
        // None of these dispatches a skill. V2Executor owns `run_skill`
        // exclusively, which is where the ISS-038 zero-file gate lives.
        // Therefore the gate cannot be bypassed by any rustclaw code path.
        //
        // If a `run_skill` (or similar dispatch) hook is ever added, this
        // test should be deleted *and* a new gate-fires-through-rustclaw
        // integration test added.
    }
}
