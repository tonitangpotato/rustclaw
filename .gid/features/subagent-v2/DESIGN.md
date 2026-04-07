# Sub-Agent V2 Design

## Problems

1. **No agent types.** Every sub-agent gets the same system prompt, same tools, same behavior. An "explore the codebase" agent and an "implement this feature" agent are identical — the LLM guesses what to do from the task text alone.

2. **Task embedded in system prompt.** `SubagentSection` puts the task into the system prompt (`"You were created to handle: {task}"`). Every sub-agent has a unique system prompt → zero prompt cache hits across sub-agents. With Anthropic's prompt caching, a shared system prompt prefix saves ~90% of input tokens on cache hit.

3. **No transcript persistence.** Sub-agent sessions live in memory. Process restart = all context lost. Can't debug what a sub-agent did. Can't audit file changes.

4. **One tool set for all.** `ToolRegistry::for_subagent` gives every sub-agent exec + read + write + edit + search + list_dir. An explorer doesn't need write/edit. A reviewer doesn't need exec. Overly broad tools → sub-agents do things they shouldn't.

5. **Two spawn paths.** SpawnTool and ritual runner both construct AgentConfig + call spawn_agent_with_options + process_with_subagent, but with different prompt building and post-processing logic.

## Design

### Agent Types

Define agent types as structs with explicit tool lists. Each type specifies behavior that's constant across all invocations:

```rust
pub struct AgentType {
    /// Identifier: "explorer", "coder", "reviewer", "planner"
    pub name: &'static str,
    /// System prompt — constant per type, enables prompt cache sharing.
    /// Does NOT contain the task, workspace, or time. Those go elsewhere.
    pub system_prompt: &'static str,
    /// Explicit tool names this agent type can use.
    pub tools: &'static [&'static str],
    /// Default model.
    pub default_model: &'static str,
    /// Default max iterations.
    pub default_max_iterations: u32,
}
```

Built-in types:

| Type | Model | Tools | Max Turns | Use Case |
|------|-------|-------|-----------|----------|
| `explorer` | sonnet | read_file, list_dir, search_files, exec | 20 | Codebase exploration, search, analysis. Exec for `git log`, `find`, `wc`. |
| `coder` | opus | read_file, write_file, edit_file, list_dir, search_files, exec | 40 | Implement features, fix bugs |
| `reviewer` | sonnet | read_file, write_file, edit_file, list_dir, search_files | 20 | Review docs, write findings. No exec. |
| `planner` | sonnet | read_file, list_dir, search_files | 15 | Design, planning. No write, no exec. |

Unknown roles from SpawnTool → `coder` (full tools, safest default).

### Prompt Structure

System prompt contains ONLY the agent type's constant rules. Everything dynamic goes in the user message.

No two-block split. No `build_system_value` changes. No `cache_control` changes.

**Before (broken for caching):**
```
System: "You are a subagent... You were created to handle: {task}" ← unique per invocation, 0% cache hit
User: "Execute the skill described in your system prompt."
```

**After:**
```
System: "You are a coder agent. Rules: ..."                           ← constant per type, cached by API
User: "Time: 2026-04-06\nWorkspace: /path\n\n## Task\n{task}\n\n..."  ← all dynamic content here
```

Anthropic's prompt caching caches the system prompt + tool definitions as a prefix. Same bytes = cache hit. Since agent type rules never change, all sub-agents of the same type share the cache (~90% input token savings on cache hit).

The API's existing caching handles it automatically because the system prompt is now truly constant per type. Tool definitions are also constant per type (same tool list) → even more cache sharing.

### Transcript Persistence

Every sub-agent run persists its transcript to disk as an append-only JSONL file:

```
~/.rustclaw/transcripts/
  {agent_id}.jsonl
```

Each line: `{"role": "assistant", "content": [...], "ts": "2026-04-06T15:30:00Z"}`

**Purpose: debug and audit only.** Not for resume. If a sub-agent crashes mid-phase, the ritual advance model re-executes the entire phase (already works). Trying to resume mid-sub-agent from a transcript introduces stale tool results, compacted-vs-original message mismatches, and partial file writes. Not worth the complexity.

**Implementation:**

```rust
struct TranscriptWriter {
    file: std::io::BufWriter<std::fs::File>,
}

impl TranscriptWriter {
    fn open(agent_id: &str) -> Result<Self>;
    fn append(&mut self, role: &str, content: &str) -> Result<()>;
}
```

After each turn in the agentic loop: `transcript.append("assistant", &response_text)` and `transcript.append("tool_results", &tool_summary)`. Lightweight — no serialization of full tool result blobs, just summaries.

**Size control:** Transcript captures message roles + text content + tool call names, NOT full tool result bodies (which can be 50K+). Estimated ~5KB per turn, ~200KB for a 40-turn coder agent. Acceptable.

### Unified Execution

One method. Both SpawnTool and ritual runner use it.

```rust
impl AgentRunner {
    /// Run a sub-agent to completion. Always synchronous (awaits result).
    /// Callers wrap in tokio::spawn for fire-and-forget.
    pub async fn run_subagent(
        &self,
        agent_type: &AgentType,
        task: &str,
        options: SubAgentOptions,
    ) -> Result<SubAgentResult>;
}

pub struct SubAgentOptions {
    /// Override agent_type's default model.
    pub model: Option<String>,
    /// Override agent_type's default max_iterations.
    pub max_iterations: Option<u32>,
    /// Working directory (default: runner's workspace).
    pub workspace: Option<PathBuf>,
    /// Extra context appended to the user message as labeled sections.
    pub context: Vec<ContextBlock>,
}

impl Default for SubAgentOptions {
    fn default() -> Self {
        Self { model: None, max_iterations: None, workspace: None, context: vec![] }
    }
}

pub struct ContextBlock {
    pub label: String,
    pub content: String,
}

pub struct SubAgentResult {
    pub agent_id: String,
    pub output: String,
    pub tokens: u64,
    pub turns: u32,
    pub transcript_path: PathBuf,
}
```

### Internal Flow

```
run_subagent(agent_type, task, options)
  1. Generate agent_id
  2. Create LLM client (model from options or agent_type.default_model)
  3. Create ToolRegistry from agent_type.tools (explicit list, not one-size-fits-all)
  4. System prompt = agent_type.system_prompt (constant string, cached by API)
  5. Build user message: time + workspace + task + options.context blocks
  6. Open TranscriptWriter
  7. Call process_with_subagent(subagent, user_message, transcript):
     - Existing agentic loop: auto-compact, streaming, reactive compact, cancellation
     - After each turn: if transcript is Some, append summary
  8. Return SubAgentResult
```

`run_subagent` is a thin wrapper (~50 lines) that:
- Builds the right SubAgent from AgentType + options
- Builds the user message from task + context
- Opens TranscriptWriter, passes it into process_with_subagent
- Extracts token count from session

`process_with_subagent` gets two minor additions:
- `transcript: Option<&mut TranscriptWriter>` parameter — if set, appends a summary after each turn (~5 lines in the loop)
- Uses `subagent.system_prompt` if set, else falls back to `build_subagent_system_prompt` (~1 line)

The 300-line agentic loop logic (streaming, compact, cancellation, tool execution) is untouched.

### Fallback: No AgentRunner

`run_subagent` requires `AgentRunner`. When `AgentRunner` is None (testing, standalone gid-core, or if sub-agent creation fails), the ritual runner falls back to direct LLM execution via `RitualLlmAdapter` — the existing code path, unchanged:

```rust
// ritual_runner.rs run_skill():
if let Some(ref runner) = self.agent_runner {
    // V2: typed sub-agent via run_subagent
    let agent_type = phase_to_agent_type(name);
    let result = runner.run_subagent(agent_type, &task, options).await;
    // ... convert to RitualEvent
} else {
    // Fallback: direct LLM execution (existing RitualLlmAdapter code, unchanged)
    let adapter = RitualLlmAdapter::new(self.llm_client.clone());
    // ...
}
```

This preserves backward compatibility and testability.

### Callers

**SpawnTool:**
```rust
// Map user's "role" to AgentType, fallback to CODER
let agent_type = match role.as_deref() {
    Some("explorer" | "researcher") => &AgentType::EXPLORER,
    Some("reviewer") => &AgentType::REVIEWER,
    Some("planner" | "architect") => &AgentType::PLANNER,
    _ => &AgentType::CODER,
};

let result = runner.run_subagent(
    agent_type,
    &task,
    SubAgentOptions { workspace, model, max_iterations, ..Default::default() },
).await?;

// SpawnTool handles its own: truncation, notifications, fire-and-forget, event broadcast
```

**Ritual runner:**
```rust
let (agent_type, context) = match phase {
    "implement" | "execute-tasks" => (&AgentType::CODER, build_implement_context(&self.project_root)),
    "review-design" | "review-requirements" | "review-tasks" => (&AgentType::REVIEWER, build_review_context(phase, &self.project_root)),
    "draft-design" | "update-design" => (&AgentType::PLANNER, vec![]),
    _ => (&AgentType::CODER, vec![]),
};

let result = runner.run_subagent(
    agent_type,
    &task_context,
    SubAgentOptions {
        workspace: Some(self.project_root.clone()),
        context,
        ..Default::default()
    },
).await?;

// Ritual runner converts to RitualEvent::SkillCompleted/SkillFailed
```

### What Changes

| File | Change |
|------|--------|
| `agent.rs` | Add `AgentType` (4 constants), `SubAgentOptions`, `SubAgentResult`, `run_subagent()` (thin wrapper ~50 lines), `TranscriptWriter` (~30 lines) |
| `agent.rs` | `SubAgent` gets `system_prompt: Option<String>` field. `process_with_subagent` gets `transcript: Option<&mut TranscriptWriter>` param, uses `subagent.system_prompt` if set. Two small additions (~6 lines total), not a refactor. |
| `tools.rs` | SpawnTool calls `run_subagent` instead of inline spawn logic. Role→AgentType mapping. |
| `tools.rs` | `ToolRegistry::for_agent_type(agent_type, workspace)` replaces `for_subagent(workspace)`. Takes `&[&str]` tool list. |
| `ritual_runner.rs` | `run_skill_as_subagent` becomes ~15 lines: pick AgentType + build context + call `run_subagent` |
| `ritual_runner.rs` | `ApplyReview` handler migrates from `spawn_agent_with_options` + `process_with_subagent` to `run_subagent(&AgentType::REVIEWER, ...)` |
| `ritual_runner.rs` | Extract `build_implement_context()` and `build_review_context()` as standalone functions |
| `llm.rs` | No changes needed |

### What Doesn't Change

- `process_with_subagent` — the 300-line agentic loop logic stays as-is (two minor additions: `subagent.system_prompt` check + optional transcript append, ~6 lines total)
- Streaming, auto-compact, cancellation, max-tokens recovery — untouched
- LLM client creation, SessionManager, AnthropicClient, `build_system_value`, `llm.rs`
- Fire-and-forget wrapping (SpawnTool's `tokio::spawn` + notifications stays in SpawnTool)
- Ritual advance model
- Main agent's agentic loop

### Risks

| Risk | Impact | Mitigation |
|------|--------|-----------|
| New system prompts change sub-agent behavior | Medium | Port existing SubagentSection rules into each agent type prompt. Test before deploying. |
| Tool list too restrictive for some tasks | Low | Each type has explicit list; easy to add tools. `CODER` has full access as fallback. |
| Transcript I/O adds latency per turn | Negligible | JSONL append is ~0.1ms. Summaries only, not full tool results. |
| System prompt constant means no per-invocation customization | Low | All dynamic content goes in user message. Agent type rules are stable by design. |
| SpawnTool role mapping is lossy | Low | Unknown roles → CODER (full tools). User can still override model/iterations. |

### Migration

All three callers of `spawn_agent_with_options` + `process_with_subagent` migrate atomically in one PR:
1. **SpawnTool** (`tools.rs`) — wait mode and fire-and-forget mode
2. **run_skill_as_subagent** (`ritual_runner.rs`) — ritual phase execution
3. **ApplyReview** (`ritual_runner.rs`) — fire-and-forget review application

After migration, `spawn_agent_with_options` and `ToolRegistry::for_subagent` are unused by RustClaw code but remain as public API for backward compatibility. No removal needed.

### Pre-Implementation Cleanup

Revert the partially written `SubAgentRequest` and `SubAgentResult` structs in `agent.rs` (added during earlier design exploration, before this design was finalized). The actual implementation uses `AgentType` + `SubAgentOptions` + `SubAgentResult` as defined above.

### Not In Scope

- Agent definition files (`.rustclaw/agents/*.yml`) — built-in types are enough for now
- Multi-agent coordination (swarm/team mode)
- Permission bubbling (surface sub-agent permission requests to user)
- Fork mode (share parent's exact message history)
- Transcript-based resume (crash recovery re-executes the phase via advance model)
