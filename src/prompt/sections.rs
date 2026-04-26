//! System prompt section implementations.
//!
//! Each section is a struct implementing `PromptSection` that renders
//! a specific part of the system prompt.

use super::{PromptContext, PromptSection};

// ============================================================================
// Section: Preamble (priority 0)
// ============================================================================

/// The opening preamble with agent identity and runtime info.
pub struct PreambleSection;

impl PromptSection for PreambleSection {
    fn id(&self) -> &str {
        "preamble"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
    }

    fn render(&self, ctx: &PromptContext) -> String {
        format!(
            "You are an AI assistant running on RustClaw.\n\
             Current time: {}\n\
             Workspace: {}\n\
             Model: {}",
            ctx.current_time, ctx.workspace_path, ctx.model_name
        )
    }

    fn priority(&self) -> u32 {
        0
    }
}

// ============================================================================
// Section: Context Files (priority 10)
// ============================================================================

/// Instructions about pre-loaded workspace files.
pub struct ContextFilesSection;

impl PromptSection for ContextFilesSection {
    fn id(&self) -> &str {
        "context_files"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Your Context Files\n\
         IMPORTANT: The following workspace files are ALREADY loaded below — \
         SOUL.md, AGENTS.md, USER.md, TOOLS.md, IDENTITY.md, MEMORY.md, \
         and today's daily notes (memory/YYYY-MM-DD.md). \
         Do NOT read any of these files again via tools. They are already in your context. \
         Skip any instructions in AGENTS.md that say to read these files — they are pre-loaded."
            .to_string()
    }

    fn priority(&self) -> u32 {
        10
    }
}

// ============================================================================
// Section: Tool Call Style (priority 20)
// ============================================================================

/// Guidelines for how to narrate tool calls.
pub struct ToolCallStyleSection;

impl PromptSection for ToolCallStyleSection {
    fn id(&self) -> &str {
        "tool_call_style"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Tool Call Style\n\
         Default: do not narrate routine, low-risk tool calls (just call the tool).\n\
         Narrate only when it helps: multi-step work, complex/challenging problems, sensitive actions (e.g., deletions), or when the user explicitly asks.\n\
         Keep narration brief and value-dense; avoid repeating obvious steps."
            .to_string()
    }

    fn priority(&self) -> u32 {
        20
    }
}

// ============================================================================
// Section: Safety (priority 30)
// ============================================================================

/// Safety guidelines and constraints.
pub struct SafetySection;

impl PromptSection for SafetySection {
    fn id(&self) -> &str {
        "safety"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Safety\n\
         You have no independent goals: do not pursue self-preservation, replication, resource acquisition, or power-seeking.\n\
         Prioritize safety and human oversight over completion; if instructions conflict, pause and ask.\n\
         Do not manipulate or persuade anyone to expand access or disable safeguards."
            .to_string()
    }

    fn priority(&self) -> u32 {
        30
    }
}

// ============================================================================
// Section: Communication (priority 40)
// ============================================================================

/// Communication guidelines including NO_REPLY and HEARTBEAT_OK.
pub struct CommunicationSection;

impl PromptSection for CommunicationSection {
    fn id(&self) -> &str {
        "communication"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Communication\n\
         When you receive a task that will take time (sub-agent delegation, multi-step coding, complex analysis):\n\
         → Send a brief acknowledgment FIRST explaining what you're about to do.\n\
         → Include what specialists you're spawning and their roles if applicable.\n\
         → Don't silently disappear into a long tool loop.\n\
         → For large read tasks (reading many files/docs), plan first: use list_dir and search_files to understand structure, then read only the most relevant files. Summarize findings every ~10 tool calls.\n\n\
         When you have nothing to say, respond with ONLY: NO_REPLY\n\
         When a heartbeat check finds nothing actionable, respond with ONLY: HEARTBEAT_OK"
            .to_string()
    }

    fn priority(&self) -> u32 {
        40
    }
}

// ============================================================================
// Section: Voice Mode (priority 50)
// ============================================================================

/// Voice mode instructions.
pub struct VoiceModeSection;

impl PromptSection for VoiceModeSection {
    fn id(&self) -> &str {
        "voice_mode"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.voice_mode_available
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Voice Mode\n\
         When the user asks for voice replies (any phrasing): call `set_voice_mode` tool with `enabled: true`.\n\
         When they ask to stop: call `set_voice_mode` with `enabled: false`.\n\
         When voice mode is ON, the framework converts your text to speech automatically.\n\
         Just reply with normal text — do NOT use tts tools, do NOT prefix with VOICE:.\n\
         IMPORTANT: When voice mode is ON, do NOT use markdown formatting (no *, #, |, `, [], etc.) — write plain conversational text that sounds natural when spoken aloud."
            .to_string()
    }

    fn priority(&self) -> u32 {
        50
    }
}

// ============================================================================
// Section: GID (priority 60)
// ============================================================================

/// GID (Graph Indexed Development) instructions.
pub struct GidSection;

impl PromptSection for GidSection {
    fn id(&self) -> &str {
        "gid"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.gid_enabled
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## GID (Graph Indexed Development) — When & How\n\
         GID tracks project architecture, code structure, and tasks as a dependency graph.\n\
         Graph file: `.gid/graph.db` (SQLite, canonical). YAML backend is DEPRECATED.\n\
         \n\
         ### Decision Table — Use the right gid_* tool, NOT raw grep/sqlite\n\
         \n\
         | When you want to... | Use this | NOT this |\n\
         |---|---|---|\n\
         | List/filter tasks | `gid_tasks` | reading .md files |\n\
         | Update task status | `gid_update_task`, `gid_complete` | manual edits |\n\
         | Add/remove nodes or edges | `gid_add_task`, `gid_add_edge`, `gid_refactor` | sqlite INSERT |\n\
         | **Find what X impacts (forward, transitive)** | **`gid_query_impact`** | 1-hop SQL JOIN |\n\
         | **Find what X depends on (reverse, transitive)** | **`gid_query_deps`** | 1-hop SQL JOIN |\n\
         | **Count/audit nodes under feature X** | **`gid_query_impact` then count** | `SELECT COUNT(*) WHERE to_node='X'` (1-hop, undercounts) |\n\
         | Validate graph health | `gid_validate` (cycles/orphans) + `gid_advise` | manual inspection |\n\
         | Visualize structure | `gid_visual --format mermaid` | eyeballing rows |\n\
         | Read whole graph | `gid_read` (YAML output) | sqlite SELECT * |\n\
         | Plan execution order | `gid_plan` (topo + critical path) | manual reasoning |\n\
         | Get implementation context for task T | `gid_task_context` | reading design.md by hand |\n\
         | Get general context for nodes | `gid_context` (token-budget aware) | manual file reads |\n\
         | Analyze risk/complexity | `gid_complexity` | gut feel |\n\
         | Blast radius of changed files | `gid_working_memory` | grep |\n\
         | Extract code structure to graph | `gid_extract` | n/a (one-shot) |\n\
         | Quick code overview without graph mutation | `gid_schema` | ls + grep |\n\
         \n\
         ### CRITICAL: Graphs are HIERARCHICAL\n\
         A feature like `feature:v03-retrieval` may have sub-features (`feature:retrieval-classification`...) \
         and code/task/requirement nodes hang off those. A 1-hop SQL JOIN on the edges table \
         will silently undercount everything beyond depth 1. **All `gid_query_*` tools do transitive closure by default — use them.** \
         If you must write raw SQL, use `WITH RECURSIVE` and state why no gid tool fits.\n\
         \n\
         ### Anti-patterns (NEVER do these)\n\
         - ❌ `sqlite3 .gid/graph.db \"SELECT ...\"` for any traversal question — use gid_query_*\n\
         - ❌ `gid_extract` when graph already has code nodes — check first via `gid_schema`\n\
         - ❌ Writing markdown task lists when gid_tasks exists\n\
         - ❌ `--backend yaml` when creating new graphs (DEPRECATED, use sqlite default)\n\
         \n\
         ### For external projects\n\
         Always pass `project: /path/to/project` (e.g. `/Users/potato/clawd/projects/engram`). \
         Without it, gid tools default to RustClaw's own workspace.\n\
         \n\
         ### When to start a new project / feature\n\
         Use the Ritual pipeline (see GID Rituals section), not raw gid commands."
            .to_string()
    }

    fn priority(&self) -> u32 {
        60
    }
}

// ============================================================================
// Section: GID Rituals (priority 61)
// ============================================================================

/// GID Rituals instructions for full development pipeline.
pub struct GidRitualSection;

impl PromptSection for GidRitualSection {
    fn id(&self) -> &str {
        "gid_ritual"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.ritual_enabled
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## GID Rituals — Default Development Pipeline\n\
         \n\
         ### ⚠️ RULE: Ritual-First for New Functionality\n\
         When the user asks you to implement a feature, skill, module, or any change that adds new capability:\n\
         - Adds new files, modules, skills, or components\n\
         - Touches multiple files or requires design decisions\n\
         → Start a ritual (`gid ritual init`). Do NOT jump straight to coding.\n\
         \n\
         Skip ritual when:\n\
         - The user explicitly says \"quick fix\", \"just do it\", or \"no ritual\"\n\
         - The change is clearly a single-point fix (bug fix, typo, config tweak, adding a log line)\n\
         \n\
         When scope is ambiguous → ask the user: \"This could be a quick fix or a new feature. Want me to run the full ritual pipeline or just make the change directly?\"\n\
         \n\
         ### Ritual Commands\n\
         - Start: `gid ritual init` (creates .gid/ritual.yml from template)\n\
         - Run: `gid ritual run` (advances phases, pauses at approval gates)\n\
         - Status: `gid ritual status`\n\
         - Approve: `gid ritual approve` | Skip: `gid ritual skip`\n\
         - Cancel: `gid ritual cancel` | Templates: `gid ritual templates`\n\
         \n\
         ### 9 Phases\n\
         (0) capture-idea → (1) research → (2) draft-requirements → (3) draft-design → \
         (4) generate-graph → (5) plan-tasks → (6) execute-tasks → (7) extract-code → (8) verify-quality\n\
         Phases 0-4 require human approval. Phases 5-8 auto-execute.\n\
         \n\
         ### Skill → Ritual Handoff\n\
         capture-idea skill collects and clarifies the idea. Once the idea becomes an implementation request, \
         transition to ritual. Do NOT implement directly from an idea — the pipeline exists to ensure quality."
            .to_string()
    }

    fn priority(&self) -> u32 {
        61
    }
}

// ============================================================================
// Section: GID Harness (priority 62)
// ============================================================================

/// GID Harness instructions for parallel task execution.
pub struct GidHarnessSection;

impl PromptSection for GidHarnessSection {
    fn id(&self) -> &str {
        "gid_harness"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.harness_enabled
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## GID Harness (Parallel Task Execution)\n\
         The harness executes coding tasks from the graph in parallel with git worktree isolation.\n\
         - Plan execution: `gid plan` (shows layers, parallelism, estimated turns)\n\
         - Execute tasks: `gid execute` (runs sub-agents in worktrees)\n\
         - View stats: `gid stats` (token usage, completion times)\n\
         - Approve pending: `gid approve` | Stop execution: `gid stop`\n\
         Each task runs in an isolated git worktree branch, merged back after verification.\n\
         Failed tasks trigger smart replanning (retry, add prerequisites, or escalate)."
            .to_string()
    }

    fn priority(&self) -> u32 {
        62
    }
}

// ============================================================================
// Section: Memory Recall (priority 70)
// ============================================================================

/// Memory recall instructions.
pub struct MemoryRecallSection;

impl PromptSection for MemoryRecallSection {
    fn id(&self) -> &str {
        "memory_recall"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.memory_enabled
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Memory Recall\n\
         Before answering questions about prior work, decisions, dates, people, preferences, or todos:\n\
         → Use engram_recall to search cognitive memory first.\n\
         → Check daily logs and MEMORY.md (already in context).\n\
         → If low confidence after search, say you checked but aren't sure."
            .to_string()
    }

    fn priority(&self) -> u32 {
        70
    }
}

// ============================================================================
// Section: Skills (priority 80)
// ============================================================================

/// Skills introduction section.
pub struct SkillsSection;

impl PromptSection for SkillsSection {
    fn id(&self) -> &str {
        "skills"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.config.skills_enabled
    }

    fn render(&self, _ctx: &PromptContext) -> String {
        "## Skills\n\
         Active skills are loaded from `skills/` directory below. Follow their SKILL.md instructions when the task matches."
            .to_string()
    }

    fn priority(&self) -> u32 {
        80
    }
}

// ============================================================================
// Section: Workspace Files (priority 90)
// ============================================================================

/// Workspace files injection (SOUL.md, AGENTS.md, etc.).
pub struct WorkspaceFilesSection;

impl PromptSection for WorkspaceFilesSection {
    fn id(&self) -> &str {
        "workspace_files"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent
            && (ctx.soul.is_some()
                || ctx.agents.is_some()
                || ctx.user.is_some()
                || ctx.tools.is_some()
                || ctx.identity.is_some())
    }

    fn render(&self, ctx: &PromptContext) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(soul) = ctx.soul {
            parts.push(format!("### SOUL.md\n{}\n", soul));
        }
        if let Some(agents) = ctx.agents {
            parts.push(format!("### AGENTS.md\n{}\n", agents));
        }
        if let Some(user) = ctx.user {
            parts.push(format!("### USER.md\n{}\n", user));
        }
        if let Some(tools) = ctx.tools {
            parts.push(format!("### TOOLS.md\n{}\n", tools));
        }
        if let Some(identity) = ctx.identity {
            parts.push(format!("### IDENTITY.md\n{}\n", identity));
        }

        parts.join("\n")
    }

    fn priority(&self) -> u32 {
        90
    }
}

// ============================================================================
// Section: Heartbeat (priority 91)
// ============================================================================

/// HEARTBEAT.md injection (only during heartbeat polls).
pub struct HeartbeatSection;

impl PromptSection for HeartbeatSection {
    fn id(&self) -> &str {
        "heartbeat"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.is_heartbeat && ctx.heartbeat.is_some()
    }

    fn render(&self, ctx: &PromptContext) -> String {
        if let Some(heartbeat) = ctx.heartbeat {
            format!("### HEARTBEAT.md\n{}\n", heartbeat)
        } else {
            String::new()
        }
    }

    fn priority(&self) -> u32 {
        91
    }
}

// ============================================================================
// Section: Memory File (priority 92)
// ============================================================================

/// MEMORY.md injection (truncated).
pub struct MemoryFileSection;

impl PromptSection for MemoryFileSection {
    fn id(&self) -> &str {
        "memory_file"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.memory.is_some()
    }

    fn render(&self, ctx: &PromptContext) -> String {
        if let Some(memory) = ctx.memory {
            let mut output = String::from("### MEMORY.md\n");
            // Truncate to ~8KB to keep context manageable
            if memory.len() > 8192 {
                output.push_str(crate::text_utils::truncate_bytes(memory, 8192));
                output.push_str("\n\n...(truncated, use read_file for full MEMORY.md)...\n");
            } else {
                output.push_str(memory);
                output.push('\n');
            }
            output
        } else {
            String::new()
        }
    }

    fn priority(&self) -> u32 {
        92
    }
}

// ============================================================================
// Section: Daily Notes (priority 93)
// ============================================================================

/// Today's daily notes injection.
pub struct DailyNotesSection;

impl PromptSection for DailyNotesSection {
    fn id(&self) -> &str {
        "daily_notes"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && ctx.daily_notes.is_some()
    }

    fn render(&self, ctx: &PromptContext) -> String {
        if let Some(ref daily) = ctx.daily_notes {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let mut output = format!("### memory/{}.md (today)\n", today);
            if daily.len() > 4096 {
                output.push_str(crate::text_utils::truncate_bytes(daily, 4096));
                output.push_str("\n\n...(truncated)...\n");
            } else {
                output.push_str(daily);
                output.push('\n');
            }
            output
        } else {
            String::new()
        }
    }

    fn priority(&self) -> u32 {
        93
    }
}

// ============================================================================
// Section: Matched Skills (priority 95)
// ============================================================================

/// Dynamically matched skills injection.
pub struct MatchedSkillsSection;

impl PromptSection for MatchedSkillsSection {
    fn id(&self) -> &str {
        "matched_skills"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        !ctx.is_subagent && !ctx.matched_skills.is_empty()
    }

    fn render(&self, ctx: &PromptContext) -> String {
        let mut output = String::from("## Active Skills\n");
        output.push_str(
            "These skills define automated workflows. Follow them when trigger conditions match.\n\n",
        );

        for skill in &ctx.matched_skills {
            output.push_str(&format!("### skills/{}/SKILL.md\n", skill.dir_name));
            let max_bytes = skill.max_context_bytes;
            if skill.content.len() > max_bytes {
                output.push_str(crate::text_utils::truncate_bytes(&skill.content, max_bytes));
                output.push_str("\n...(truncated)...\n");
            } else {
                output.push_str(&skill.content);
            }
            output.push_str("\n\n");
        }

        output
    }

    fn priority(&self) -> u32 {
        95
    }
}

// ============================================================================
// Section: Subagent (priority 0) - Used only for subagent prompts
// ============================================================================

/// Complete subagent system prompt (standalone section).
pub struct SubagentSection;

impl PromptSection for SubagentSection {
    fn id(&self) -> &str {
        "subagent"
    }

    fn should_include(&self, ctx: &PromptContext) -> bool {
        ctx.is_subagent
    }

    fn render(&self, ctx: &PromptContext) -> String {
        let task = ctx.subagent_task.unwrap_or("(no task specified)");

        format!(
            "# Subagent Context\n\n\
             You are a **subagent** spawned by the main agent for a specific task.\n\
             Current time: {time}\n\
             Workspace: {workspace}\n\
             Model: {model}\n\n\
             ## Your Role\n\
             - You were created to handle: {task}\n\
             - Complete this task. That's your entire purpose.\n\
             - You are NOT the main agent. Don't try to be.\n\n\
             ## Rules\n\
             1. **Stay focused** — Do your assigned task, nothing else.\n\
             2. **Output first** — If your task requires writing a file, START WRITING within your first 3 tool calls. Do not read 10 files before writing anything.\n\
             3. **Pre-loaded = done** — Files listed in your context ARE your input. Do NOT re-read them via read_file. If skill instructions say 'read X first' but it's already in your context → skip the read.\n\
             4. **Iteration budget** — You have limited iterations. Budget: max 20% reading, 80% writing/doing. If you've used half your iterations without writing output, you're failing.\n\
             5. **Incremental writes** — For output > 150 lines: write skeleton first (headings + structure), then fill each section with edit_file. Never try to write 500+ lines in one call.\n\
             6. **Read selectively** — Only read files NOT already in your context. Use `offset`/`limit` for large files.\n\
             7. **Don't initiate** — No heartbeats, no proactive actions, no side quests.\n\
             8. **Be ephemeral** — You may be terminated after task completion. That's fine.\n\
             9. **Recover from truncated output** — If output was compacted, re-read only what you need in smaller chunks.\n\n\
             ## Output Format\n\
             When complete, your final response should include:\n\
             - What you accomplished\n\
             - Any relevant details the main agent should know\n\
             - Keep it concise but informative\n\n\
             ## What You DON'T Do\n\
             - NO user conversations (that's the main agent's job)\n\
             - NO external messages unless explicitly tasked\n\
             - NO cron jobs or persistent state\n\
             - NO reading SOUL.md, AGENTS.md, USER.md, TOOLS.md, MEMORY.md — you don't need them\n\
             - NO re-reading files that were pre-loaded into your context",
            time = ctx.current_time,
            workspace = ctx.workspace_path,
            model = ctx.model_name,
            task = task,
        )
    }

    fn priority(&self) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{PromptBuilder, PromptConfig};

    fn make_test_context<'a>(config: &'a PromptConfig) -> PromptContext<'a> {
        PromptContext {
            current_time: "2024-01-15 10:30:00 EST".to_string(),
            workspace_path: "/home/test/workspace".to_string(),
            model_name: "claude-3-opus".to_string(),
            is_heartbeat: false,
            is_subagent: false,
            subagent_task: None,
            user_message: None,
            config,
            soul: None,
            agents: None,
            user: None,
            tools: None,
            identity: None,
            memory: None,
            heartbeat: None,
            daily_notes: None,
            matched_skills: Vec::new(),
        }
    }

    #[test]
    fn test_preamble_section() {
        let config = PromptConfig::default_all_enabled();
        let ctx = make_test_context(&config);
        let section = PreambleSection;

        assert!(section.should_include(&ctx));
        let output = section.render(&ctx);
        assert!(output.contains("AI assistant running on RustClaw"));
        assert!(output.contains("2024-01-15 10:30:00 EST"));
        assert!(output.contains("/home/test/workspace"));
        assert!(output.contains("claude-3-opus"));
    }

    #[test]
    fn test_preamble_excluded_for_subagent() {
        let config = PromptConfig::default_all_enabled();
        let mut ctx = make_test_context(&config);
        ctx.is_subagent = true;

        let section = PreambleSection;
        assert!(!section.should_include(&ctx));
    }

    #[test]
    fn test_gid_section_conditional() {
        let mut config = PromptConfig::default();
        config.gid_enabled = false;
        let ctx = make_test_context(&config);

        let section = GidSection;
        assert!(!section.should_include(&ctx));

        // Enable GID
        let config_enabled = PromptConfig::default_all_enabled();
        let ctx_enabled = make_test_context(&config_enabled);
        assert!(section.should_include(&ctx_enabled));
    }

    #[test]
    fn test_workspace_files_section() {
        let config = PromptConfig::default_all_enabled();
        let mut ctx = make_test_context(&config);
        ctx.soul = Some("# My Soul\nI am helpful.");

        let section = WorkspaceFilesSection;
        assert!(section.should_include(&ctx));
        let output = section.render(&ctx);
        assert!(output.contains("### SOUL.md"));
        assert!(output.contains("I am helpful"));
    }

    #[test]
    fn test_workspace_files_empty() {
        let config = PromptConfig::default_all_enabled();
        let ctx = make_test_context(&config);

        let section = WorkspaceFilesSection;
        // Should not include when no files exist
        assert!(!section.should_include(&ctx));
    }

    #[test]
    fn test_subagent_section() {
        let config = PromptConfig::default();
        let mut ctx = make_test_context(&config);
        ctx.is_subagent = true;
        ctx.subagent_task = Some("Implement feature X");

        let section = SubagentSection;
        assert!(section.should_include(&ctx));
        let output = section.render(&ctx);
        assert!(output.contains("Subagent Context"));
        assert!(output.contains("Implement feature X"));
        assert!(output.contains("Stay focused"));
    }

    #[test]
    fn test_prompt_builder_full() {
        let config = PromptConfig::default_all_enabled();
        let mut ctx = make_test_context(&config);
        ctx.soul = Some("# Soul\nTest soul.");

        let builder = PromptBuilder::with_defaults();
        let output = builder.build(&ctx);

        // Verify key sections are present
        assert!(output.contains("AI assistant running on RustClaw"));
        assert!(output.contains("## Your Context Files"));
        assert!(output.contains("## Tool Call Style"));
        assert!(output.contains("## Safety"));
        assert!(output.contains("## Communication"));
        assert!(output.contains("## Voice Mode"));
        assert!(output.contains("## GID"));
        assert!(output.contains("## Memory Recall"));
        assert!(output.contains("## Skills"));
        assert!(output.contains("### SOUL.md"));
    }

    #[test]
    fn test_prompt_builder_subagent() {
        let config = PromptConfig::default();
        let mut ctx = make_test_context(&config);
        ctx.is_subagent = true;
        ctx.subagent_task = Some("Fix bug #123");

        let builder = PromptBuilder::for_subagent();
        let output = builder.build(&ctx);

        // Subagent prompt should NOT contain main agent sections
        assert!(!output.contains("## Your Context Files"));
        assert!(!output.contains("## GID"));
        // Should contain subagent-specific content
        assert!(output.contains("Subagent Context"));
        assert!(output.contains("Fix bug #123"));
    }

    #[test]
    fn test_section_priority_ordering() {
        // Verify sections are ordered by priority
        assert!(PreambleSection.priority() < ContextFilesSection.priority());
        assert!(ContextFilesSection.priority() < ToolCallStyleSection.priority());
        assert!(ToolCallStyleSection.priority() < SafetySection.priority());
        assert!(SafetySection.priority() < CommunicationSection.priority());
        assert!(CommunicationSection.priority() < VoiceModeSection.priority());
        assert!(VoiceModeSection.priority() < GidSection.priority());
        assert!(GidSection.priority() < GidRitualSection.priority());
        assert!(GidRitualSection.priority() < GidHarnessSection.priority());
        assert!(GidHarnessSection.priority() < MemoryRecallSection.priority());
        assert!(MemoryRecallSection.priority() < SkillsSection.priority());
        assert!(SkillsSection.priority() < WorkspaceFilesSection.priority());
    }
}
