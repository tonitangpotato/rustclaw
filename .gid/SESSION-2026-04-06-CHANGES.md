# Code Changes — 2026-04-06

## Files Changed

### `src/agent.rs`
**Sub-Agent V2 + Structured Error Handling**

Added:
- `AgentType` struct + 4 constants (EXPLORER, CODER, REVIEWER, PLANNER) with per-type system prompts, tool lists, default model/iterations
- `SubAgentOptions`, `ContextBlock` — config for `run_subagent`
- `SubAgentResult` — always-returned result with `outcome: SubAgentOutcome`, `files_modified: Vec<String>`
- `SubAgentOutcome` enum — Completed, AuthFailed, RateLimited, ContextTooLarge, MaxIterations, Timeout, Cancelled, Error
- `LoopResult` (pub(crate)) — internal loop result with `exit_reason: LoopExit`
- `LoopExit` enum — Completed, MaxIterations, Timeout{elapsed_secs}, Cancelled
- `classify_error()` — classifies anyhow::Error into SubAgentOutcome using type-safe downcast + exact format matching
- `run_subagent()` — unified entry point, returns SubAgentResult (never Err)
- `TranscriptWriter` — JSONL audit log per sub-agent run
- `system_prompt: Option<String>` + `model_override: Option<String>` on SubAgent
- `clone_boxed()` on LlmClient trait — shares OAuth token manager with sub-agents

Modified:
- `process_with_subagent()` returns `Result<LoopResult>` (was `Result<String>`)
  - Tracks `completed_turns`, `files_modified`, `loop_exit` in the agentic loop
  - Sets exit reason at each break point (timeout, cancel, max iterations, normal)
  - Uses `chat_stream_with_model` for sub-agents (streaming + model override)
  - `transcript: Option<&mut TranscriptWriter>` param for per-turn logging
  - Uses `subagent.system_prompt` if set (constant per type → prompt cache sharing)

### `src/tools.rs`
**SpawnTool migration + ToolRegistry::for_agent_type**

Added:
- `ToolRegistry::for_agent_type(tools, workspace)` — registers only named tools

Modified:
- SpawnTool fire-and-forget: uses `run_subagent` with role→AgentType mapping, reports `outcome.display()` + `files_modified`
- SpawnTool wait mode: same pattern, structured error reporting

### `src/ritual_runner.rs`
**Advance model + structured outcomes**

Added:
- `advance()` — stateless event-driven ritual execution (replaced `run_loop`)
- `spawn_event_producing_action()` — spawns action in background, calls advance on completion
- `target_root: Option<String>` on RitualState — set at start, used everywhere
- `target_root_for()` — reads from state, falls back to project_root
- `build_implement_context()`, `build_review_context()` — context injection helpers
- `build_ritual_event_from_text()` — natural language approval parsing
- `parse_verify_steps()` + `auto_label()` — labeled verify step execution

Removed:
- `run_loop()` — replaced by `advance()`
- `run_skill_as_subagent()` (285 lines) — replaced by `run_subagent`
- Event registry channel-based routing — advance model is stateless, no channels needed

Modified:
- `run_skill()`: all phases use typed sub-agents via `run_subagent`. Maps phase→AgentType. Fallback to RitualLlmAdapter when no AgentRunner.
- `ApplyReview`: uses `run_subagent(&AgentType::REVIEWER, ...)`
- `run_shell()`: re-reads verify_command from `.gid/config.yml` at execution time. Runs steps sequentially with labeled output.
- Approval routing: `/ritual apply` = `/ritual approve`. Natural language matching for "apply all", "approve all", "yes", "ok", "好".

### `src/llm.rs`
**Streaming model override + 401 bail-fast + timeout no-retry**

Added:
- `chat_stream_with_model()` on LlmClient trait + AnthropicClient impl
- `collect_stream()` — collects streaming response into LlmResponse
- `clone_boxed()` on LlmClient trait + all implementations (Anthropic, OpenAI, Google, ClaudeCli)
- `request_timeout_secs` field on LlmConfig (configurable HTTP timeout)

Modified:
- 401 handling: bail after `tried_profiles.len() >= profile_count` (no lock, no profile list re-query)
- Timeout errors: bail immediately, no 5× retry with same payload
- Error logging: includes `is_timeout`, `is_connect`, `is_body` flags

### `src/config.rs`
- `request_timeout_secs: u64` on LlmConfig (default 120)
- `#[derive(Clone)]` on AuthMode

### `src/oauth.rs`
- `#[derive(Clone)]` on OAuthTokenManager (enables Arc-sharing with sub-agents)

### `src/claude_cli.rs`
- `clone_boxed()` implementation for ClaudeCliClient

### `src/orchestrator.rs`
- `process_with_subagent` call updated: `.output` on LoopResult

### `src/channels/telegram.rs`
- `/ritual apply` aliased to `/ritual approve`
- `try_route_to_waiting_ritual`: disk-based fallback (no channel dependency)
- `build_ritual_event_from_text`: explicit approval pattern matching (no hijacking unrelated messages)
- Voice message: removed `[Voice message]` prefix, transcribed text passed as plain text
- Catch-all guard: short/ambiguous text checked against waiting rituals before starting new ones

### `src/prompt/sections.rs`
- No changes (SubagentSection still exists as fallback)

### Context Pre-loading (Task 8, across files)

**`src/ritual_runner.rs`:**
- `extract_markdown_skeleton()` — extracts all `#` headings + first sentence per section from markdown
- `preload_files_with_budget(files, project_root, budget_chars)` — reads files with total character budget (120K chars ≈ 30K tokens), divides equally among files. Over-budget files get: skeleton (always) + truncated content + note to read full file if needed.
- `build_review_context()` rewritten: pre-loads document content via `preload_files_with_budget`, adds "ALREADY LOADED — do NOT read again" labels
- `build_implement_context()` labels updated: "ALREADY LOADED" on DESIGN docs

**`src/agent.rs`:**
- `AgentType::REVIEWER` system prompt: added rule "If document content is provided in your task (labeled ALREADY LOADED), review it directly. Do NOT re-read files that are already in your context."

---

## Sub-Agent V2 Task Completion Status

| Task | Status |
|------|--------|
| 0. Structured error handling | Done |
| 1. Cleanup + data structures | Done |
| 2. System prompts (4 types) | Done |
| 3. TranscriptWriter | Done |
| 4. ToolRegistry::for_agent_type | Done |
| 5. run_subagent + process_with_subagent | Done |
| 6. Migrate 3 callers (atomic) | Done |
| 7. Tests (337 pass, 0 warnings) | Done |
| 8. Context pre-loading with budget | Done |
| 9. Build + deploy | Pending (RustClaw busy) |

## Design Docs Written

- `.gid/features/subagent-v2/DESIGN.md` — Agent types, prompt caching, transcript, unified execution
- `.gid/features/subagent-v2/TASKS.md` — 8 implementation tasks
- `.gid/features/subagent-error-handling/DESIGN.md` — SubAgentOutcome, LoopResult, classify_error, files_modified
- `.gid/features/incremental-extract/DESIGN.md` — ISS-006 incremental extract design (not yet implemented)
- `.gid/SESSION-2026-04-06.md` — Session summary
