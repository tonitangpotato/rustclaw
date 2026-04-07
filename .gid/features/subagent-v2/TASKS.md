# Sub-Agent V2 — Implementation Tasks

## Task 0: Structured error handling (DONE)

Added alongside V2. See `.gid/features/subagent-error-handling/DESIGN.md`.

- `SubAgentOutcome` enum: Completed, AuthFailed, RateLimited, ContextTooLarge, MaxIterations, Timeout, Cancelled, Error
- `LoopResult` (pub(crate)): output + turns + files_modified + exit_reason
- `LoopExit` enum: Completed, MaxIterations, Timeout{elapsed_secs}, Cancelled
- `classify_error()`: type-safe downcast for reqwest timeout, exact format matching for Anthropic API errors
- `run_subagent` returns `SubAgentResult` (never Err) with structured outcome
- `process_with_subagent` tracks turns, files_modified, sets exit_reason at each break point
- 401 bail-fast: `tried_profiles.len() >= profile_count` (simple counter, no lock)
- OAuth sharing: sub-agents use `clone_boxed()` from parent's LLM client (shared Arc<RwLock<TokenState>>)
- Dead code removed: `run_skill_as_subagent` (285 lines)

---

## Task 1: Cleanup + add data structures (DONE)

Revert partially written `SubAgentRequest`/`SubAgentResult` from earlier exploration in `agent.rs`. Then add the new types:

- `AgentType` struct + 4 constants (`EXPLORER`, `CODER`, `REVIEWER`, `PLANNER`) with name, tools list, default_model, default_max_iterations
- `SubAgentOptions`, `ContextBlock`, `SubAgentResult` structs
- System prompts for each type: **empty string placeholders** (filled in Task 2)

**Files:** `agent.rs`
**Verify:** `cargo check`

---

## Task 2: Write agent type system prompts (DONE)

Write the 4 system prompts. Port ALL rules from existing `SubagentSection` into each type (stay focused, be efficient, plan first, read selectively, be ephemeral, recover from truncated output, no user conversations, no SOUL.md reading). Then add type-specific rules:

- **Explorer:** Read-only exploration. Use exec for git/find/wc. Summarize findings.
- **Coder:** Write code, run tests, follow existing patterns. Be precise with edits.
- **Reviewer:** Read documents, write review findings to files. No shell commands.
- **Planner:** Read-only analysis. Produce design docs and plans. No file modifications.

**Verify:** Diff each prompt against `SubagentSection` in `prompt/sections.rs` — confirm zero dropped rules. Each prompt should be a superset of the generic rules.

---

## Task 3: Add TranscriptWriter (DONE)

Implement `TranscriptWriter` with `open(agent_id)` and `append(role, content)`. Creates `~/.rustclaw/transcripts/{agent_id}.jsonl`. Append-only JSONL: role + text + tool call names (not full tool result bodies).

**Files:** `agent.rs`
**Verify:** Unit test: open, append 3 entries, read file back, verify valid JSONL.

---

## Task 4: Add ToolRegistry::for_agent_type (DONE)

Add `for_agent_type(tools: &[&str], workspace: &str) -> Self`. Registers only the named tools from the available set.

**Files:** `tools.rs`
**Verify:** Unit test: `for_agent_type(&["read_file", "list_dir"], "/tmp")` has exactly 2 tools. `for_agent_type(&["read_file", "exec"], "/tmp")` has exactly 2. Unknown tool name is silently skipped.

---

## Task 5: Implement run_subagent + update process_with_subagent (DONE)

This is the core task. Three changes done together (they share compilation dependency):

**5a.** Add `system_prompt: Option<String>` field to `SubAgent`. In `process_with_subagent`, use it if set, else fall back to `build_subagent_system_prompt`.

**5b.** Add `transcript: Option<&mut TranscriptWriter>` parameter to `process_with_subagent`. After each turn, if Some, append summary. **Update all existing internal call sites to pass `None`** so compilation doesn't break.

**5c.** Implement `run_subagent()` (~50 lines):
1. Generate agent_id
2. Create LLM client (options.model or agent_type.default_model)
3. Create ToolRegistry via `for_agent_type`
4. Build SubAgent with system_prompt from agent_type
5. Build user message: time + workspace + task + context blocks
6. Open TranscriptWriter
7. Call `process_with_subagent(subagent, user_message, Some(transcript))`
8. Return SubAgentResult

**Files:** `agent.rs`
**Verify:** `cargo check`. Existing behavior unchanged (all old call sites pass None for transcript, None for system_prompt).

---

## Task 6: Migrate all three callers (DONE)

All three migrate together in one pass. After this task, no code calls `spawn_agent_with_options` + `process_with_subagent` directly.

**6a. SpawnTool** (`tools.rs`):
- Add role→AgentType mapping (explorer/researcher → EXPLORER, reviewer → REVIEWER, planner/architect → PLANNER, default → CODER)
- Replace inline spawn logic with `run_subagent` call
- Keep SpawnTool's own: truncation (8K), fire-and-forget wrapping, notifications, progress pings, event broadcast

**6b. run_skill** (`ritual_runner.rs`):
- Replace `run_skill_as_subagent` with phase→AgentType mapping + `run_subagent` call
- Remove the `needs_subagent` gate — all phases use typed sub-agents when AgentRunner is available
- Extract `build_implement_context()` and `build_review_context()` as standalone functions
- Keep fallback to `RitualLlmAdapter` when `agent_runner` is None

**6c. ApplyReview** (`ritual_runner.rs`):
- Replace `spawn_agent_with_options` + `process_with_subagent` with `run_subagent(&AgentType::REVIEWER, ...)`

**Files:** `tools.rs`, `ritual_runner.rs`
**Verify:** `cargo check` — zero errors, zero warnings. `grep -r "spawn_agent_with_options\|process_with_subagent" src/ | grep -v "fn \|pub \|///\|//\|test"` returns no direct usage outside definitions.

---

## Task 7: Tests (DONE)

**Automated:**
- `for_agent_type` with restricted tool list → correct tool count
- `for_agent_type` with CODER tools → has exec, write, edit
- `for_agent_type` with PLANNER tools → no exec, no write, no edit
- `TranscriptWriter` open + append + verify JSONL
- `SubAgentRequest` build_prompt: task in user message, not in system prompt
- Each `AgentType` constant has non-empty system_prompt, non-empty tools, valid model string

**Manual (post-deploy):**
- Start a ritual → verify sub-agents use typed prompts (check logs for system prompt)
- Spawn sub-agent via Telegram → verify role mapping works
- Check `~/.rustclaw/transcripts/` for transcript files after sub-agent completes
- Check logs: second sub-agent of same type should show `cache_read` > 0 (prompt cache hit)

---

## Task 8: Context pre-loading with budget (DONE)

Sub-agents waste iterations reading large files. Pre-load document content into context blocks so the sub-agent works directly without calling read_file.

**Problem:** Naive pre-loading can explode context (5 files × 10KB = 50KB ≈ 15K tokens), causing early auto-compact that throws away the pre-loaded content.

**Solution:** Budget-aware pre-loading. Total pre-load budget = 30K tokens (~120K chars). Divided equally among files. Files exceeding their share are truncated with a note.

**8a.** Add `preload_files_with_budget(files, budget_chars)` helper in `ritual_runner.rs`:
- Reads each file, budget = `total_budget / file_count` chars per file
- If file fits in budget: include full content
- If file exceeds budget: include markdown skeleton (all `#` headings + first sentence of each section) + full content up to budget, with note "(truncated — sections N-M omitted, see outline above)"
- `extract_markdown_skeleton(content)` helper: scan lines, collect `#` headings + first non-empty line after each heading
- Label: `"Document: {path} (ALREADY LOADED — do NOT read again)"`

**8b.** Update `build_review_context()`: use `preload_files_with_budget` to include file content, not just file paths.

**8c.** Update `build_implement_context()`: DESIGN.md content already included with truncation (done). Add "ALREADY LOADED" to labels.

**8d.** Update `AgentType::REVIEWER` system prompt: add rule "If document content is provided in your task, review it directly. Do NOT re-read files already in your context."

**8e.** SpawnTool context pre-loading: when main agent spawns a REVIEWER sub-agent and the task mentions file paths, extract those paths and pre-load their content via the same budget helper. This covers the non-ritual spawn path.

**Files:** `ritual_runner.rs`, `agent.rs`, `tools.rs`
**Verify:** Start a review ritual phase with 5 docs. Check logs — sub-agent should NOT call read_file on pre-loaded documents. Review should complete within iteration limit. Also test: spawn a reviewer sub-agent from main agent with file paths in task — content should be pre-loaded.

---

## Task 9: Build + deploy

1. `cargo check` — zero errors, zero warnings
2. `cargo test --features full -p gid-core` — all tests pass
3. `cargo build --release`
4. Restart RustClaw
5. Run manual tests from Task 7

**Verify:** All pass.
