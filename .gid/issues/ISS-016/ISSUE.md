# ISS-016: Main Agent Is Blind to Active Rituals

**Created**: 2026-04-20
**Priority**: High
**Status**: Analysis Complete → Ready for Fix

---

## Problem

The main agent has **zero awareness** of ritual execution state. When a ritual is running (either because a sub-agent / background `ritual_adapter` is executing it, or because the main agent itself is the LLM driving a `SingleLlm`-strategy ritual), the system prompt contains no indication of this fact.

### Symptoms observed (2026-04-20, ISS-015 ritual r-e07fa7)

1. User asked "ritual 进度如何？" — main agent had no idea a ritual was active until it manually shelled into the state file. It could not answer the question without tool calls.
2. During a `SingleLlm` ritual, the main agent's LLM loop **is** the implementer, but it receives no signal that it's supposed to be enacting an implement skill. It behaves as a generic assistant, wasting turns re-discovering context.
3. When the background `ritual_adapter` hits turn-limit and relaunches (4x in ~6 min, 250k tokens), the main agent cannot detect this pathology because it has no view into the ritual state stream.
4. Risk of starting a **second** ritual on top of an active one — nothing in the prompt warns against it.

### Why this is a root problem (not a patch)

This is not about "add better logging". It's an architectural gap: **ritual state is a first-class part of the agent's situational awareness and must be in the system prompt, always**. The same way we inject SOUL.md, USER.md, recent memories, and active skills — we need to inject "are you currently inside a ritual, and if so what phase".

Without this:
- The agent's decisions are made with incomplete world-state
- `SingleLlm` strategy is fundamentally broken (agent doesn't know it's the implementer)
- Concurrent rituals can silently conflict
- User cannot get accurate status without forcing tool calls

---

## Scope

### In scope

1. **Detect active rituals across all known project workspaces** (not just the agent's own workspace — the rituals the user runs are typically in `/Users/potato/clawd/projects/*/.gid/rituals/` or `/Users/potato/rustclaw/.gid/rituals/`).
2. **Inject a `## Active Ritual Status` section** into the system prompt when ≥1 active ritual is found.
3. **The section must tell the LLM**:
   - Ritual ID, phase, task, project path, strategy (SingleLlm / MultiAgent), retry counts, last update time
   - Whether the main agent itself is the LLM driving the ritual (SingleLlm + this process) — **critical** for SingleLlm correctness
   - Rules: don't start a new ritual, cancel before taking over, etc.
4. **Detection must be cheap** — this runs on every prompt build. No sync I/O that blocks for >10ms. Cache with TTL.
5. **Pathology detection**: if a ritual has been in the same phase for >N minutes with increasing `turn_count` but no code diff, flag it in the prompt so the agent can proactively tell the user.

### Out of scope (this ISS)

- Rewriting `ritual_adapter` turn-limit behavior (→ separate issue, see ISS-017 candidate below)
- Changing ritual state file format
- TUI / dashboard surface changes (they already read the state files directly)

---

## Design

### Component 1: `RitualRegistry` (new module, `src/ritual_registry.rs`)

Responsibilities:
- Discover active ritual state files across all known project roots
- Parse them into a lightweight `ActiveRitual` struct
- Cache results with a short TTL (default 5s) to avoid filesystem thrash

```rust
pub struct ActiveRitual {
    pub id: String,
    pub project_root: PathBuf,
    pub state_path: PathBuf,
    pub phase: String,           // "Planning" | "Implementing" | "Verifying" | ...
    pub strategy: String,        // "SingleLlm" | "MultiAgent"
    pub task_summary: String,    // First 200 chars of task
    pub verify_retries: u32,
    pub turn_count: Option<u32>, // If adapter logs this
    pub updated_at: DateTime<Utc>,
    pub suspected_stuck: bool,   // True if no update in >3 min
}

pub struct RitualRegistry {
    known_roots: Vec<PathBuf>,   // Configured in rustclaw.yaml
    cache: RwLock<Option<(Vec<ActiveRitual>, Instant)>>,
    ttl: Duration,
}

impl RitualRegistry {
    pub fn new(known_roots: Vec<PathBuf>) -> Self { ... }
    pub async fn active_rituals(&self) -> Vec<ActiveRitual>;
    pub fn invalidate(&self);   // Call after starting/cancelling a ritual
}
```

**Discovery algorithm**:
1. For each `root` in `known_roots`:
2.   Scan `root/.gid/rituals/*.json`
3.   For each file: parse, check `phase` is not in terminal set {"Done", "Cancelled", "Failed"}
4.   If `updated_at` is stale (>10 min) → treat as dead, skip
5.   Compute `suspected_stuck`: no update in >3 min AND phase unchanged
6. Return flattened list sorted by `updated_at` desc

### Component 2: Config — `rustclaw.yaml`

Add:
```yaml
ritual:
  known_project_roots:
    - /Users/potato/rustclaw
    - /Users/potato/clawd/projects/gid-rs
    - /Users/potato/clawd/projects/agentctl
    - /Users/potato/clawd/projects/xinfluencer
  registry_ttl_secs: 5
  stuck_threshold_secs: 180
```

**Why explicit list, not auto-discover?** Root fix: we could glob `/Users/potato/clawd/projects/*/.gid/`, but that couples the agent to a filesystem convention. Explicit config is honest about what the agent tracks and lets the user opt in/out. Registry can grow to support auto-discovery later if useful; for now keep it simple and explicit.

### Component 3: Prompt integration (`src/workspace.rs` + `src/prompt/sections.rs`)

Add a new section in `build_system_prompt_full` (and its `PromptBuilder` equivalent), positioned **after skills notice** and **before workspace files**:

```
## Active Ritual Status

⚠️ **1 ritual currently running. You are NOT a generic assistant right now — you are embedded in a ritual pipeline.**

### r-e07fa7 (Implementing, verify_retries=1)
- **Project**: /Users/potato/clawd/projects/gid-rs
- **Task**: Fix ISS-015: PRAGMA foreign_keys inside transaction is a no-op...
- **Strategy**: SingleLlm
- **You are the executor**: YES (main agent LLM is driving this ritual)
- **Last update**: 2m ago
- **State file**: /Users/potato/clawd/projects/gid-rs/.gid/rituals/r-e07fa7.json

### Rules when ≥1 ritual is active:
- Do NOT call `gid_ritual_init` or start a parallel ritual
- If user asks "what's happening" → summarize phase + retries from above, don't re-describe the task
- If user wants you to take over manually → call `gid_ritual_cancel` first, then proceed
- If `suspected_stuck: true` → proactively inform the user with specifics

### If You Are the Executor (SingleLlm)
- Follow the phase's skill instructions (implement / verify / etc.)
- The phase's tool scope is already applied to your session
- Do not ask "what should I do" — the ritual state tells you
```

When no rituals active: the section is **omitted entirely** (no noise in normal operation).

### Component 4: `Agent` wiring

- `Agent::new()` builds a `RitualRegistry` from config and stores it in an `Arc`.
- `build_system_prompt_full` takes an `&Arc<RitualRegistry>` or pulls active rituals synchronously via `tokio::runtime::Handle::current().block_on()` (cache-hit is near-instant, cache-miss is a few file reads — acceptable on prompt build).
- After the agent starts or cancels a ritual via tools, `registry.invalidate()` is called so next prompt sees fresh state.

### Component 5: "You are the executor" detection

For SingleLlm strategy, we need to tell the main agent "you are driving this". Heuristic:
- If `ActiveRitual.strategy == "SingleLlm"` AND the main agent's process PID matches the ritual's `adapter_pid` (if the adapter writes it to state), set `you_are_executor = true`.
- Otherwise `false` (another agent / background adapter is driving).

If PID cannot be confirmed, default to `false` + add a note: `"Unknown executor — check state file"`.

**TODO in fix**: ensure `ritual_adapter` writes its own PID into the state file. Tiny change.

---

## Acceptance Criteria

- AC-1: When a ritual is active in any configured `known_project_root`, the main agent's system prompt contains `## Active Ritual Status` section with ritual ID, phase, task summary, project path, strategy, and last-update age.
- AC-2: When no rituals are active, the section is completely omitted (no empty header).
- AC-3: Prompt-build overhead from ritual detection is <20ms p99 (measured via existing prompt-build timing).
- AC-4: `suspected_stuck` flag correctly fires for rituals with no state update in >3 min.
- AC-5: Starting a new ritual updates the registry within 1 prompt-build cycle (via `invalidate()`).
- AC-6: Unit tests cover: (a) no rituals → no section, (b) 1 active ritual → section present with correct fields, (c) stuck ritual → warning present, (d) terminal-state ritual → excluded from list.
- AC-7: Main agent can answer "what ritual is running and what phase?" without any tool calls when a ritual is active.

---

## Non-Goals

- Fixing `ritual_adapter` infinite relaunch loops (separate issue — this ISS only makes the main agent *aware* of them, not *fix* them).
- Adding a UI / dashboard view for rituals (already exists via state files).
- Auto-discovery of project roots via filesystem glob (explicit config for now).

---

## Risks

1. **Stale state files from crashed adapters** could show as active forever. Mitigation: `updated_at > 10min` = treat as dead.
2. **Cross-workspace permission**: reading `/Users/potato/clawd/projects/*/.gid/rituals/*.json` from rustclaw process — should be fine (same user), but worth testing.
3. **Prompt token bloat** when many rituals are active. Mitigation: cap at 5 rituals listed, summarize the rest ("+N more").

---

## Related

- ISS-015 — the ritual that exposed this problem
- Possible follow-up: **ISS-017 (candidate): `ritual_adapter` turn-limit pathology** — when adapter hits 20-turn limit, it relaunches the skill from scratch losing context. Separate root fix: stream state / lift turn limit / checkpoint progress.

---

## Implementation Notes (2026-04-20)

### Initial landing

- `src/ritual_registry.rs` (658 lines) — `RitualRegistry` with 5s TTL cache, scanning configured `.gid/rituals/*.json` across roots, suspected-stuck detection (>3m no `updated_at` progression), and `render_prompt_section()` for system-prompt injection.
- `src/config.rs` — new `RitualConfigSection` with `known_project_roots`, `registry_ttl_secs`, `stuck_threshold_secs`, `dead_threshold_secs`.
- `src/workspace.rs` — `Workspace` carries `Option<Arc<RitualRegistry>>`; the "Active Ritual Status" section is emitted between section 8 and section 9 of `build_system_prompt_full()`.
- `src/agent.rs` — `Agent::new()` builds the registry from config, stores it on `workspace` *and* late-binds it into `ToolRegistry.ritual_registry` (shared `Arc<Mutex<...>>` slot).
- `rustclaw.yaml` — `ritual:` block lists 3 project roots: rustclaw itself, gid-rs, and (placeholder) clawd projects.
- 269 tests pass, including 8 new `ritual_registry::tests` unit tests covering AC-1 through AC-6.

### Technical debt resolution (2026-04-20, same day)

The initial landing left two gaps called out in TODOs; both are now closed.

1. **`adapter_pid` written to ritual state** (AC-7 prerequisite).
   - `gid-core::ritual::state_machine::RitualState` gained `adapter_pid: Option<u32>` (serde `skip_serializing_if` = None so old state files round-trip cleanly).
   - New builder `with_current_adapter_pid()` stamps `std::process::id()`.
   - `RitualRunner::save_state()` in `src/ritual_runner.rs` now clones the state, stamps `adapter_pid` to the current process PID, then serializes — so *every* write to a ritual state file carries the owning process PID without the caller having to remember.
   - Effect: `RitualRegistry::parse_state_file()` can finally set `you_are_executor = true` when `strategy == SingleLlm` and the PID matches — the "you are the executor" banner becomes actually correct rather than a placeholder.

2. **Cache invalidation on ritual start**.
   - `ToolRegistry` gained a shared slot `ritual_registry: Arc<Mutex<Option<Arc<RitualRegistry>>>>`, mirroring the `ritual_notify` slot pattern.
   - `StartRitualTool` now holds that slot and invalidates the registry cache *before* and *after* `runner.start(task)`. This means a newly-started ritual is visible on the very next prompt build instead of waiting up to 5s for the TTL to expire.
   - `Agent::new()` injects the same `Arc<RitualRegistry>` it stashes in `Workspace` into the `ToolRegistry`'s slot, so both readers share a single cache.

### What's still out-of-scope

- `/ritual cancel` and in-loop ritual actions do not call `invalidate()` directly. The 5s TTL catches them. Not worth wiring through the state machine until/unless it becomes a visible problem.
- Suspected-stuck detection is purely time-based (`updated_at` unchanged for >3m). Turn-count-based pathology detection (ISS-017 territory) requires the adapter to write turn metrics into the state file — separate work.

