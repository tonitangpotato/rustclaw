# Design: Claude CLI Proxy Backend for RustClaw

## Problem

Anthropic charges extra usage for third-party OAuth apps calling their API directly. RustClaw currently calls the Anthropic API via `llm.rs` → `AnthropicClient`. All LLM usage (conversation + rituals) is billed as extra usage despite having a Max subscription.

## Solution

Route all LLM calls through `claude -p` (Claude Code CLI in headless mode). CC binary is a "native Anthropic application" — subscription covers it. Confirmed:
- `isUsingOverage: false` in every test
- Anthropic engineer Boris Cherny confirmed on X: headless CC + Agent SDK = subscription covered
- Multiple production users confirm this works

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   RustClaw                       │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Telegram  │  │ Rituals  │  │ Heartbeat/    │  │
│  │ Channel   │  │ Runner   │  │ Cron          │  │
│  └────┬──────┘  └────┬─────┘  └───────┬───────┘  │
│       │              │                │           │
│       ▼              ▼                ▼           │
│  ┌─────────────────────────────────────────────┐  │
│  │              LlmClient trait                │  │
│  │  fn chat() / chat_stream() / chat_with_model│  │
│  └────────────────────┬────────────────────────┘  │
│                       │                           │
│  ┌────────────────────▼────────────────────────┐  │
│  │          ClaudeCliClient (NEW)               │  │
│  │                                              │  │
│  │  - Spawns `claude -p` per request            │  │
│  │  - Uses --resume for multi-turn              │  │
│  │  - Parses stream-json output                 │  │
│  │  - Maps CC events → StreamChunk              │  │
│  │  - Manages session_id per conversation       │  │
│  └────────────────────┬────────────────────────┘  │
│                       │                           │
└───────────────────────┼───────────────────────────┘
                        │ spawn process
                        ▼
              ┌──────────────────┐
              │  claude -p CLI   │
              │  (CC binary)     │
              │                  │
              │  OAuth login ──► Anthropic API
              │  subscription    │  (covered)
              └──────────────────┘
```

## ClaudeCliClient

New implementation of `LlmClient` trait that wraps `claude -p`.

### Interface (unchanged)

```rust
#[async_trait]
impl LlmClient for ClaudeCliClient {
    fn model_name(&self) -> &str;
    async fn chat(&self, system: &str, messages: &[Message], tools: &[ToolDefinition]) -> Result<LlmResponse>;
    async fn chat_with_model(&self, system: &str, messages: &[Message], tools: &[ToolDefinition], model: &str) -> Result<LlmResponse>;
    async fn chat_stream(&self, system: &str, messages: &[Message], tools: &[ToolDefinition]) -> Result<mpsc::Receiver<StreamChunk>>;
}
```

### Core Implementation

```rust
pub struct ClaudeCliClient {
    model: String,
    /// Map of conversation_key → CC session_id for --resume
    sessions: Arc<Mutex<HashMap<String, String>>>,
    /// Path to claude binary
    claude_bin: String,
}
```

### How Each Method Works

#### `chat()` / `chat_with_model()` — One-shot (ritual phases)

```rust
async fn chat_with_model(&self, system: &str, messages: &[Message], tools: &[ToolDefinition], model: &str) -> Result<LlmResponse> {
    // 1. Serialize messages into a single prompt text
    let prompt = self.serialize_messages(messages);
    
    // 2. Build command
    let mut cmd = Command::new(&self.claude_bin);
    cmd.args(["-p", &prompt])
       .args(["--system-prompt", system])
       .args(["--model", model])
       .args(["--output-format", "json"])
       .args(["--permission-mode", "bypassPermissions"]);
    
    // 3. If tools provided, let CC use its built-in tools
    if !tools.is_empty() {
        // Map RustClaw tool names → CC tool names
        let cc_tools = map_tools_to_cc(tools);
        cmd.args(["--allowedTools", &cc_tools]);
    } else {
        cmd.args(["--allowedTools", ""]);
        cmd.args(["--max-turns", "1"]);  // truly one-shot only when no tools
    }
    
    // 4. Run and parse JSON output
    let output = cmd.output().await?;
    let result: CcJsonResult = serde_json::from_slice(&output.stdout)?;
    
    // 5. Map to LlmResponse
    Ok(LlmResponse {
        text: Some(result.result),
        tool_calls: vec![],  // CC handled tools internally
        stop_reason: result.stop_reason,
        usage: Usage {
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            cache_read: result.usage.cache_read_input_tokens,
            cache_write: result.usage.cache_creation_input_tokens,
        },
    })
}
```

#### `chat_stream()` — Streaming (conversation)

```rust
async fn chat_stream(&self, system: &str, messages: &[Message], tools: &[ToolDefinition]) -> Result<mpsc::Receiver<StreamChunk>> {
    let prompt = self.serialize_last_user_message(messages);
    let session_key = self.conversation_key(messages);
    
    let mut cmd = Command::new(&self.claude_bin);
    cmd.args(["-p", &prompt])
       .args(["--system-prompt", system])
       .args(["--output-format", "stream-json", "--verbose"]);
    
    // Resume existing conversation if we have a session_id
    if let Some(cc_session) = self.sessions.lock().await.get(&session_key) {
        cmd.args(["--resume", cc_session]);
    }
    
    // Spawn process, read stdout line by line
    let child = cmd.stdout(Stdio::piped()).spawn()?;
    
    let (tx, rx) = mpsc::channel(100);
    
    tokio::spawn(async move {
        let reader = BufReader::new(child.stdout.unwrap());
        for line in reader.lines() {
            let event: CcStreamEvent = serde_json::from_str(&line)?;
            match event.type_.as_str() {
                "assistant" => {
                    // Extract text content, send as StreamChunk::Text
                    if let Some(text) = extract_text(&event) {
                        tx.send(StreamChunk::Text(text)).await;
                    }
                }
                "result" => {
                    // Save session_id for --resume
                    // Send StreamChunk::Done with usage
                }
                _ => {} // ignore init, rate_limit_event, etc.
            }
        }
    });
    
    Ok(rx)
}
```

### Message Serialization

For `--resume` (multi-turn): only send the latest user message as prompt. CC maintains conversation history internally.

For first message (no session): serialize full conversation:
```
[System instructions are passed via --system-prompt]

User: 帮我查天气
Assistant: 28度，晴天
User: 明天呢？
```

### CC Stream-JSON Events

```jsonl
{"type":"system","subtype":"init","session_id":"...","model":"...","tools":[...]}
{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}
{"type":"rate_limit_event","rate_limit_info":{"isUsingOverage":false}}
{"type":"result","subtype":"success","result":"...","session_id":"...","usage":{...}}
```

Map to RustClaw's `StreamChunk`:
- `assistant` with text → `StreamChunk::Text(text)`
- `assistant` with tool_use → CC handles internally (or `StreamChunk::ToolUse` if we need it)
- `result` → `StreamChunk::Done(usage, stop_reason)`

## Ritual Integration

### Current Flow (ritual_runner.rs:906)
```rust
async fn run_skill_as_subagent(&self, runner, name, context) {
    let subagent = runner.spawn_agent_with_options(&config, max_iterations)?;
    let result = runner.process_with_subagent(&subagent, &task, Some(&phase_id)).await;
}
```

### New Flow
```rust
async fn run_skill_as_subagent(&self, runner, name, context) {
    let model = match name { "implement" => "claude-opus-4-6", _ => "claude-sonnet-4-5-20250929" };
    let skill_prompt = self.load_skill_prompt(name);
    let tools = match name {
        "implement" | "verify" => "Read,Write,Edit,Bash,Glob,Grep",
        "draft-design" | "draft-requirements" => "Read,Glob,Grep",
        _ => "Read,Glob,Grep",
    };
    
    let mut cmd = Command::new("claude");
    cmd.args(["-p", &task])
       .args(["--system-prompt", &skill_prompt])
       .args(["--model", model])
       .args(["--allowedTools", tools])
       .args(["--output-format", "stream-json", "--verbose"])
       .args(["--permission-mode", "bypassPermissions"])
       .current_dir(&self.project_root);
    
    // Stream output for progress notifications
    let child = cmd.stdout(Stdio::piped()).spawn()?;
    let (result_text, usage) = self.stream_and_collect(child, &notify).await?;
    
    // Convert to ritual event
    Ok((RitualEvent::SkillCompleted { output: result_text }, usage.total_tokens()))
}
```

### Tool Scope Mapping

| Ritual Phase | Current ToolScope | CC --allowedTools |
|---|---|---|
| triage | read_file, list_dir | Read,Glob |
| draft-requirements | read_file, list_dir, grep | Read,Glob,Grep |
| review-requirements | read_file, list_dir, grep | Read,Glob,Grep |
| draft-design | read_file, list_dir, grep | Read,Glob,Grep |
| review-design | read_file, list_dir, grep | Read,Glob,Grep |
| planning | read_file, list_dir | Read,Glob |
| graphing | read_file, bash (gid only) | Read,Glob,Bash |
| implement | read_file, write_file, bash, grep | Read,Write,Edit,Bash,Glob,Grep |
| verify | read_file, bash, grep | Read,Bash,Glob,Grep |

## Conversation Integration

### Session Management

```rust
/// Map RustClaw session_key → CC session_id
struct SessionMap {
    map: HashMap<String, CcSession>,
}

struct CcSession {
    cc_session_id: String,
    created_at: Instant,
    last_used: Instant,
    turn_count: u32,
}
```

- First message in a conversation: no `--resume`, CC creates new session
- Subsequent messages: `--resume <session_id>`, CC continues conversation
- Session expiry: if CC session gets too old / compacted, start fresh

### System Prompt

Passed via `--system-prompt` flag. CC adds its own system prompt on top (~5K tokens, cached after first call). Our system prompt (SOUL.md etc.) is appended.

This means ~5K tokens overhead for CC's identity. Acceptable — it's cached (cache_read, not cache_creation) after the first call.

## Configuration

```toml
# rustclaw.toml
[llm]
provider = "claude-cli"  # NEW option
model = "claude-opus-4-6"

[llm.claude_cli]
binary = "claude"           # path to claude binary
timeout_secs = 600          # per-request timeout
max_turns = 50              # max agent loop iterations for ritual phases
session_ttl_hours = 24      # expire CC sessions after this long
```

Fallback: if `claude` binary not found or fails, fall back to direct API (`AnthropicClient`).

## Implementation Plan

### Phase 1: ClaudeCliClient (Day 1)
- [ ] New file: `src/claude_cli.rs`
- [ ] Implement `LlmClient` trait
- [ ] Message serialization (messages → prompt text)
- [ ] Stream-JSON parser (CC events → StreamChunk)
- [ ] Session management (session_id tracking, --resume)
- [ ] Error handling (process crash, timeout, CC errors)

### Phase 2: Ritual Integration (Day 2)
- [ ] Modify `ritual_runner.rs::run_skill_as_subagent()` to use `claude -p`
- [ ] Tool scope mapping (RustClaw scopes → CC --allowedTools)
- [ ] Progress streaming (parse CC stream for Telegram notifications)
- [ ] Timeout handling (kill process after 600s)
- [ ] Cancel support (kill process on /stop)

### Phase 3: Conversation Integration (Day 2-3)
- [ ] Wire `ClaudeCliClient` into `create_client()` for provider = "claude-cli"
- [ ] Session map: RustClaw session_key → CC session_id
- [ ] --resume for multi-turn conversations
- [ ] Token tracking from CC usage output
- [ ] Fallback to direct API on CC failure

### Phase 4: Testing & Polish (Day 3)
- [ ] Test ritual phases (implement, verify, design, review)
- [ ] Test multi-turn conversation via Telegram
- [ ] Verify `isUsingOverage: false` in production
- [ ] Monitor dashboard for extra usage charges
- [ ] Add metrics/logging for CC process spawn time

## Review Issues Found

### Issue 1: `--max-turns 1` kills ritual agentic loops
`chat()` pseudo-code has `--max-turns 1`. This would make ritual phases one-shot — no tool use loop. Ritual implement needs 20-40 turns of read→edit→test→fix.

**Fix:** Remove `--max-turns` for rituals (let CC run until done), or set `--max-turns 50` as upper bound. Only use `--max-turns 1` for simple conversation turns where RustClaw manages the loop.

### Issue 2: Permission mode
CC by default asks for human confirmation before Bash/Write/Edit. Automated ritual phases will hang waiting for input.

**Fix:** Always pass `--permission-mode bypassPermissions` for all `claude -p` calls.

### Issue 3: CC system prompt identity conflict
CC's built-in system prompt says "You are Claude Code, an AI coding assistant." RustClaw's SOUL.md says "You are RustClaw..." Both active simultaneously → model confused about identity.

**Fix:** For rituals, this is fine — CC acting as a coding assistant is desired behavior. For conversation, prepend to `--system-prompt`: "Ignore any prior identity instructions. You are [RustClaw identity]..." Alternatively, accept CC's identity for all interactions (simplest).

### Issue 4: GID MCP not configured in CC
GID MCP is configured in RustClaw's config, but CC has its own MCP config at `~/.claude/settings.json`. CC won't have GID tools unless explicitly added.

**Fix:** Add GID to CC's MCP config:
```json
// ~/.claude/settings.json → mcpServers
"gid": {
  "command": "gid",
  "args": ["mcp-serve"]
}
```
Verify with `claude -p "list your MCP tools" --output-format json`.

### Issue 5: `--resume` + `--system-prompt` interaction
When resuming a CC session, CC already has the system prompt from the first call. Passing `--system-prompt` again may duplicate it or be ignored.

**Fix:** Only pass `--system-prompt` on first call (no `--resume`). For resumed calls, omit it — CC keeps the original system prompt in its session state. Need to verify this empirically.

### Issue 6: Stderr capture
CC writes errors to stderr. Current design only reads stdout. A crash or auth error would be silently lost.

**Fix:** Capture both stdout and stderr. On non-zero exit code, parse stderr for error message and return as `anyhow::Error`.

### Issue 7: Concurrent CC processes
Multiple simultaneous rituals or conversations each spawn a `claude -p` process. All share the same OAuth token → potential rate limit conflicts between concurrent CC instances (same 5-hour bucket).

**Fix:** This is the same as running multiple CC sessions in different terminals — CC handles it. But add a concurrency limit (e.g., max 4 concurrent CC processes) to avoid overwhelming the rate limit bucket. Use a `tokio::sync::Semaphore`.

### Issue 8: Custom tools not available in CC
RustClaw has custom tools (engram_recall, engram_store, gid_* via direct integration). CC doesn't know about these unless they're MCP servers.

**Fix:** For ritual phases, engram/GID are already MCP-capable or CLI-invokable via Bash. Include in skill prompts: "Use `bash engram --db ... recall ...` for memory retrieval." For conversation, same approach — CC can call CLI tools via Bash.

### Issue 9: Working directory for conversations
Rituals use `current_dir(&self.project_root)` — correct. But conversation `claude -p` needs a cwd too. CC loads CLAUDE.md from cwd.

**Fix:** Use RustClaw's workspace root as cwd for conversation calls. Create a CLAUDE.md there with RustClaw's identity/instructions (alternative to `--system-prompt` for persistent config).

### Issue 10: `tool_calls: vec![]` return breaks agent loop
`chat()` returns empty `tool_calls` because CC handles tools internally. But RustClaw's `agent.rs` agentic loop expects: LLM returns tool_calls → agent executes → feeds results back. If tool_calls is always empty, the agent loop never executes tools.

**Fix:** Two modes:
- **Ritual mode:** CC handles full agentic loop. `chat()` returns final result only. Caller doesn't need tool_calls. ✅ Already correct.
- **Conversation mode:** RustClaw manages the loop. Need CC to NOT execute tools, but return tool call intents. Use `--allowedTools ""` + define tools in system prompt as JSON schema + parse structured output. OR: let CC handle all tools for conversation too (simpler, but loses RustClaw custom tools).

**Recommended:** Let CC handle tools for both modes. Register essential custom tools as MCP servers or Bash wrappers. This is the simplest path.

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| CC binary update breaks stream-json format | Pin CC version, test on upgrade |
| CC identity conflicts with RustClaw persona | Accept CC identity for rituals; override via --system-prompt for conversation |
| Process spawn latency (~2s) | Acceptable for rituals; --resume reuses CC's cached context |
| CC binary not installed / auth expired | Fallback to AnthropicClient (extra usage but works) |
| Anthropic changes policy on headless CC | Low risk — Boris Cherny confirmed, official docs say it's allowed |
| CC's context management conflicts with ours | For rituals: CC manages (fine). For conversation: let CC manage, disable RustClaw compact |
| CC session state gets stale | TTL-based expiry, start fresh session when needed |
| Concurrent rate limit exhaustion | Semaphore limits concurrent CC processes (max 4) |
| Custom tools unavailable in CC | Register as MCP or wrap in Bash; include CLI invocations in skill prompts |
| Permission prompts hang automated runs | Always use `--permission-mode bypassPermissions` |

## Token Cost Impact

| Scenario | Before (direct API) | After (claude -p) |
|---|---|---|
| Ritual implement (Opus, 40 turns) | ~$5-15 extra usage | $0 (subscription) |
| Conversation (Sonnet, 20 turns) | ~$0.50 extra usage | $0 (subscription) |
| Heartbeat (Sonnet, 1 turn) | ~$0.03 extra usage | $0 (subscription) |
| CC system prompt overhead | 0 | ~5K tokens cached (negligible) |

**Estimated monthly savings: $100-500+ depending on usage.**
