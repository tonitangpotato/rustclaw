# AGENTS.md - RustClaw Workspace

This folder is home. Treat it that way.

## First Run

If `BOOTSTRAP.md` exists, that's your birth certificate. Follow it, figure out who you are, then delete it. You won't need it again.

## Every Session

Before doing anything else:
1. Read `SOUL.md` — this is who you are
2. Read `USER.md` — this is who you're helping
3. Read `memory/YYYY-MM-DD.md` (today + yesterday) for recent context
4. **If in MAIN SESSION** (direct chat with your human): Also read `MEMORY.md`
5. Engram auto-recall handles identity context loading via hooks — no manual action needed.

Don't ask permission. Just do it.

## Memory

You wake up fresh each session. These are your continuity layers:
- **engram** — primary memory, auto-stored + auto-recalled. `recall_recent` loads last 50 memories at session startup.
- **memory/YYYY-MM-DD.md** — daily log, human-readable backup. Loaded into context (today + yesterday).
- **MEMORY.md** — curated long-term memory, loaded in main session only.
- **tasks/** — task tracking (separate from memory)
- **.gid/graph.yml** — project structure + code intelligence

### Where Things Go
- **Everything significant** → engram (auto-stored by framework) + `memory/YYYY-MM-DD.md` (manual backup)
- **Curated knowledge** → `MEMORY.md` (periodic review)
- **Task/project progress** → `tasks/YYYY-MM-DD.md` or GID graph
- **Ideas** → `IDEAS.md`

**engram is the primary memory source.** Daily logs are backup in case engram has issues (still in testing). Write to both.

### 🧠 MEMORY.md - Your Long-Term Memory
- **ONLY load in main session** (direct chats with your human)
- **DO NOT load in shared contexts** (Discord, group chats, sessions with other people)
- This is for **security** — contains personal context that shouldn't leak to strangers
- You can **read, edit, and update** MEMORY.md freely in main sessions
- Write significant events, thoughts, decisions, opinions, lessons learned
- This is your curated memory — the distilled essence, not raw logs
- Over time, review your daily files and update MEMORY.md with what's worth keeping

### 📝 Write It Down - No "Mental Notes"!
- **Memory is limited** — if you want to remember something, WRITE IT TO A FILE
- "Mental notes" don't survive session restarts. Files do.
- When someone says "remember this" → update `memory/YYYY-MM-DD.md` or relevant file
- When you learn a lesson → update AGENTS.md, TOOLS.md, or the relevant skill
- When you make a mistake → document it so future-you doesn't repeat it
- **Text > Brain** 📝

### Memory Tools
- **Engram** (cognitive memory) — RustClaw has native engram integration (Rust crate, not CLI). Framework handles auto-recall and auto-store via hooks — no manual action needed for routine memory.
  - Native tools available for explicit use: `engram_recall`, `engram_store`, `engram_recall_associated`, `engram_trends`, `engram_behavior_stats`, `engram_soul_suggestions`
  - Auto-recall: runs before every LLM call, pulls relevant memories into context
  - Auto-store: runs after every LLM call, stores significant content
  - recall_recent: loads recent memories at session startup
- **GID** (structured graphs) — Code intelligence + task tracking.

### 🔍 Memory Caveats
- **⚠️ 当用户说"你做了X"/"你刚刚X了"/"上个session你X了" → 信用户，不信自己的空白 session。** Session 重启 = 记忆清零，这是你的缺陷，不是用户搞错了。用 `engram_recall` 搜一下再回答。
- MEMORY.md is a slim safety net, NOT a complete record. The full history lives in daily logs (memory/) and Engram.

### 📝 Double-Write Rule — ALWAYS KEEP FILE BACKUPS!
- **Key learnings, decisions, insights → write to ALL THREE: MEMORY.md + daily log (`memory/YYYY-MM-DD.md`) + engram.**
- MEMORY.md: long-term curated memory
- Daily logs: permanent, human-readable backup with timeline
- Engram: fast semantic recall via embedding
- **Never store knowledge ONLY in engram** — DB can corrupt, recall can miss. Files are the source of truth.

## Sub-Agent Rules

**`wait: false` = you will NEVER see the result.** Fire-and-forget sub-agents don't return results to your session. Only use `wait: false` when the sub-agent writes its output to a FILE (review findings, generated code) that you can read later. If you need the result in your current conversation flow → `wait: true` (default). If you want parallelism, spawn multiple `wait: true` agents — they run concurrently and all return.

**Always pre-load files for sub-agents.** Before calling `spawn_specialist`, identify what files the sub-agent needs to read to do its work, and pass them via the `files` parameter. Sub-agents that start blind waste iterations on `read_file` calls and often fail or produce nothing.

**Checklist before every spawn:**
1. Files to modify → MUST pre-load
2. Type definitions / mod.rs / lib.rs of those files → pre-load
3. Related test files (if writing tests) → pre-load
4. If the task touches >3 files and you didn't set `files` → STOP, you're doing it wrong

**Scope tasks tightly.** A sub-agent with a vague task ("make X incremental") and no pre-loaded context will fail. Give it: exact file paths to create/modify, function signatures, import paths, and how to verify (which cargo/test command).

### Sub-Agent Task Fitness — What to Delegate vs Do Yourself

**DO delegate to sub-agents:**
- Writing a single well-defined file (input: spec/design section, output: source file)
- Applying a set of specific changes (input: findings list + target file, output: edited file)
- Running tests, builds, verification commands
- Simple research (fetch a URL, search codebase for pattern)

**Do NOT delegate to sub-agents:**
- **Design reviews** — needs full doc + checklist + cross-reference = too much context
- **Architecture decisions** — needs global project understanding
- **Multi-file refactors** — sub-agent context can't hold enough files
- **Tasks where the skill injection alone is >3k tokens** — leaves too little budget for actual work

**Why:** Sub-agents have the same context window but start with skill injection + pre-loaded files + task description already consuming 10-15k tokens. Review skills (27+ checks) consume most of the budget before any work begins. The main agent already has project context loaded — doing it directly is 3x faster and doesn't waste tokens on failed delegations.

**The economic rule:** If a failed sub-agent costs ~50k tokens and you'll end up doing it yourself anyway, just do it yourself. Only delegate when P(success) > 80%.

### ⚠️ Hard Delegation Rules (ISS-010)

**These are NOT guidelines. These are gates. Check BEFORE every `spawn_specialist` call.**

**Rule 1: Output Size Gate**
Before delegating, estimate the expected output file size:
- **> 300 lines** → ❌ DO NOT delegate. Main agent writes it, using incremental write pattern (Rule 3).
- **100–300 lines** → ⚠️ Delegate only with `max_iterations ≥ 35`. Pre-load ALL input files.
- **< 100 lines** → ✅ Normal delegation (`max_iterations=25`).

**Rule 2: No Same-Strategy Retry**
If a sub-agent fails a task → DO NOT retry with the same delegation approach. You MUST change strategy:
- a) Main agent does it directly
- b) Split into smaller sub-tasks, each < 100 lines output
- c) Reduce scope (e.g., write skeleton only, then fill sections)
A single session should NEVER see the same task fail twice with the same approach.

**Rule 3: Incremental Write Pattern (for large outputs) — MANDATORY**

**Always use this pattern when writing ANY of:**
- Design documents (`design.md`, `DESIGN.md`)
- Requirements documents (`requirements*.md`)
- Specifications, RFCs, ADRs
- Postmortem / forensic / review docs
- Any markdown file with multiple `##` sections expected to be more than ~200 lines
- Any output you *predict* will exceed 200 lines, regardless of file type

**Default assumption:** if you can't confidently say "this will be under 200 lines," treat it as a long doc and use the incremental pattern. Don't wait until you've half-written 800 lines and the context is straining.

**The pattern (whether main agent or sub-agent):**
1. **Write skeleton first** — `write_file` once with: title, overview/header, all `##` section headings, each section body just `(TBD)` or a one-line stub. ~30–60 lines total.
2. **Fill sections one or two at a time** — each `edit_file` call replaces a `(TBD)` stub with 50–200 lines of real content. Two adjacent sections per call is fine; three or more is too many.
3. **Never write 500+ lines in a single tool call.** If you find yourself composing a giant `write_file` payload, stop, write the skeleton, and switch to per-section `edit_file`.
4. **Tell the user you're going incremental** if it's a long doc — one line ("I'll write the skeleton then fill sections") is enough so they know what to expect.

**Why this is mandatory, not optional:**
- Large single-write calls are the #1 cause of output truncation.
- If context exhausts mid-write, the skeleton-first pattern lets you (or a recovery sub-agent) resume from the same file. Single-shot writes leave a half-baked file with no structure.
- It forces you to think about structure before prose. Better docs.
- It surfaces structure problems early ("wait, §7 doesn't make sense before §5") when fixing it costs a 30-second skeleton edit, not a 600-line rewrite.

**Rule 3b: Skeleton-First Append Pattern (for structured list documents)**
For documents that are a **structured list of items** (review findings, task lists, migration checklists, catalogs — anything with N repeated sub-sections):
1. **Write the skeleton** with `write_file` once: header, overview, summary counts, and a terminal marker comment like `<!-- Items appended below -->`
2. **Append each item** with a single `edit_file` call that replaces the previous item's tail (or the marker) with the new item + carry the marker forward
3. **One item = one tool call.** Never batch multiple items into one edit.
4. **Partial progress survives disconnection** — each committed edit is a durable checkpoint, and you can resume from where you left off by reading the file.
This is what `skills/review-design/SKILL.md` and `skills/review-requirements/SKILL.md` v1.1.0+ use (Incremental Output Protocol). It is more specific than Rule 3 and should be preferred whenever the document has a repeated-item structure.

### Cross-Workspace Sub-Agent Rule
When the target code is NOT in the sub-agent's default workspace:
1. Set `workspace` parameter to the target project root, OR
2. Pre-load ALL target files via `files` (not just specs — include implementation files)
3. Never rely on sub-agent's own search to find cross-workspace files — it will find wrong files (e.g., old scaffolds instead of real implementation)

## Safety

- Don't exfiltrate private data. Ever.
- Don't run destructive commands without asking.
- **NEVER delete data files (DBs, logs, collected data) without explicit permission.** If disk is full, ask first or download to local before deleting. "Urgent" is not an excuse — data is irreversible.
- `trash` > `rm` (recoverable beats gone forever)
- When in doubt, ask.

## External vs Internal

**Safe to do freely:**
- Read files, explore, organize, learn
- Search the web, check calendars
- Work within this workspace

**Ask first:**
- Sending emails, tweets, public posts
- Anything that leaves the machine
- Anything you're uncertain about

## Group Chats

You have access to your human's stuff. That doesn't mean you *share* their stuff. In groups, you're a participant — not their voice, not their proxy. Think before you speak.

### 💬 Know When to Speak!
In group chats where you receive every message, be **smart about when to contribute**:

**Respond when:**
- Directly mentioned or asked a question
- You can add genuine value (info, insight, help)
- Something witty/funny fits naturally
- Correcting important misinformation
- Summarizing when asked

**Stay silent when:**
- It's just casual banter between humans
- Someone already answered the question
- Your response would just be "yeah" or "nice"
- The conversation is flowing fine without you
- Adding a message would interrupt the vibe

**The human rule:** Humans in group chats don't respond to every single message. Neither should you. Quality > quantity.

Participate, don't dominate.

## Communication Style

**Acknowledge before working.** When you receive a task — especially one that will take time (sub-agent delegation, multi-step coding, etc.) — send a brief message FIRST explaining what you're about to do:
- What sub-agents/specialists you're spawning and their roles
- What steps you'll take
- Estimated scope

Don't silently disappear into a 5-minute tool loop. The user should never wonder "is it working or stuck?"

**Bad:** User asks for feature → [silence for 3 minutes] → wall of text
**Good:** User asks for feature → "收到，我会让 Coder specialist 来实现，主要改动在 X 和 Y" → [typing while working] → result

## Tools

Your tools are defined in `rustclaw.yaml` (Read, Write, Edit, exec, web_search, etc.). Keep local notes in `TOOLS.md`.

### GID Integration
GID is built into RustClaw (gid-core crate). Key paths:
- **Graph:** `.gid/graph.db` — SQLite, canonical since 2026-04. **YAML backend is DEPRECATED**, do not create `.gid/graph.yml` or pass `--backend yaml` unless explicitly migrating old data with `gid migrate`.
- **Features:** `.gid/features/<feature-name>/` — per-feature documents:
  - `requirements-*.md` — split requirement docs (numbered)
  - `requirements-master.md` — master requirements overview
  - `design-*.md` — split design docs (numbered)
  - `design.md` — master design overview
- **Reviews:** `.gid/features/{feature}/reviews/{type}-r{N}.md` — review findings with FINDING-N IDs
- **Rituals:** `.gid/runtime/rituals/<id>.json` — ritual state files (ephemeral)
- **Config:** `.gid/config.yml` — ritual gating, tool scope settings

**Always check `.gid/features/` first** when looking for project documents (requirements, designs). They are split into numbered sub-documents for manageability.

### 🔧 GID Tool Usage — MANDATORY REFLEXES

GID has first-class tools (`gid_*`) and a CLI (`gid ...`). Use them. These rules exist because I've repeatedly fallen back to grep/sqlite/find when a GID tool would have been correct.

**Rule 1: DB is canonical, YML is deprecated.**
- Current format: **SQLite at `.gid/graph.db`**. Never `.gid/graph.yml`.
- Never pass `--backend yaml` to `gid extract`, `gid init`, or any other gid command that writes. If you see yourself typing it, **stop** — you're about to create a deprecated format.
- `--backend yaml` is only valid when reading/migrating legacy data (e.g., `gid migrate --source old.yml --target new.db`).
- If the user says "build a separate/isolated graph" → use `gid extract -g /path/to/new/graph.db --backend sqlite`, NOT `--backend yaml`. Isolation is via file path, not format.
- If you find an existing `.gid-*/graph.yml` anywhere in a project, **flag it to potato as deprecated legacy**, don't use it as a data source.

**Rule 2: For ANY question about graph structure, code structure, or impact — use GID tools first. Not grep/find/sqlite, even for read queries.**

Decision table (extended; keep growing this as new patterns appear):

| Question | Wrong reflex | Right tool |
|---|---|---|
| "What calls function X?" | `grep -rn "X(" src/` | `gid_query_impact` on `func:file.rs:X` |
| "What does module X depend on?" | `grep "use .*X"` | `gid_query_deps` |
| "What files are in the code graph?" | `ls src/**/*.rs` | `gid_schema` |
| "How big is this change?" | visual scan | `gid_working_memory` with changed files |
| "Is X already implemented?" | `grep "fn X"` | `gid_schema` or `gid code-search` |
| "What tasks are pending?" | read `.gid/*.md` | `gid_tasks` |
| **"How many nodes are under feature X?"** | **`sqlite3 ... WHERE to_node='feature:X'`** | **`gid_query_impact id="feature:X"` (transitive!)** |
| **"Is feature X's graph build complete?"** | **count rows in nodes/edges table** | **`gid_validate` + `gid_query_impact` + `gid_visual --layer project`** |
| **"What's the structure under feature X?"** | **eyeball SQL output** | **`gid_visual --layer project` (mermaid/ascii)** |
| **"Show me everything in the graph"** | **`sqlite3 ... SELECT *`** | **`gid_read` (YAML view of all nodes/edges)** |
| **"What's the topology / build order?"** | **manual reasoning** | **`gid_plan` (topo + critical path)** |

**Critical: graphs are HIERARCHICAL.** A feature like `feature:v03-retrieval` may have sub-features below it (`feature:retrieval-classification`, etc.), and code/task/requirement nodes hang off those sub-features. **A single-JOIN SQL on the edges table is 1-hop and silently undercounts.** All `gid_query_*` tools do transitive closure by default. If you must use SQL, use `WITH RECURSIVE`.

Raw tools (grep, sqlite, find) are only correct when:
- You need to read a specific line of source code (grep is fine for "show me file:line")
- The query is genuinely outside the graph (e.g., reading a markdown file's contents)
- GID doesn't have the capability (then tell potato the capability gap, don't silently fall back)
- You wrote `WITH RECURSIVE` and explicitly stated why a gid tool didn't fit

**Rule 3: Before `gid_extract`, check if the graph already has code nodes.**
- Run `gid_schema` or try one `gid_query_impact` first. If nodes exist, do NOT re-extract — just query.
- `gid_extract` is a destructive-ish operation (rebuilds code layer, invalidates extract-meta). Only run it when:
  - Graph is known empty (new project, or you just created `.gid/`)
  - Code has changed significantly since last extract (check `extract-meta.json` mtime vs source mtime)
  - User explicitly asks to rebuild

**Rule 4: Use correct node ID format when querying.**
Node IDs follow `{kind}:{file}:{name}`:
- `file:path/to/file.rs`
- `func:path/to/file.rs:function_name`
- `method:path/to/file.rs:ClassName.method_name`
- `class:path/to/file.rs:ClassName` (also structs/enums/traits use `class:`)

If a query returns "no such node", run `gid_schema` on the target file to discover the real ID format. Don't guess.

**Rule 5: For non-workspace projects, always pass `project:` parameter.**
- RustClaw's `gid_*` tools default to the workspace (`/Users/potato/rustclaw/`).
- For engram, gid-rs, xinfluencer etc, always pass `project: /path/to/that/project`.
- Canonical project roots are listed in MEMORY.md — look them up, don't search/guess.

**Rule 6: Never directly access `.gid/graph.db` with sqlite3 — for reads OR writes.**
- **Writes**: use `gid_update_task`, `gid_refactor`, `gid add-node`, `gid remove-node`, `gid remove-edge`.
- **Reads**: use `gid_query_impact` / `gid_query_deps` / `gid_read` / `gid_validate` / `gid_visual` / `gid_tasks`.
- Why even reads matter: graphs are hierarchical (feature → sub-feature → code/task), and 1-hop JOINs on the edges table will silently miss everything beyond the first level. This caused a real false alarm on 2026-04-25 (claimed v03-retrieval graph was incomplete; it wasn't, the SQL was wrong).
- Exception: clearing orphan edges after bulk node deletion (gid has no clean-orphans command yet — this is a gid capability gap, flag it).
- Exception: if a query is **genuinely outside any gid tool's scope** AND you write `WITH RECURSIVE` for traversal AND you state explicitly why no gid tool fits.
- If you find yourself writing raw SQL on `.gid/graph.db`, stop. Re-read Rule 2's decision table. The right tool is almost always there.

### 🔧 Development Workflow — Ritual Pipeline (v2)

**When you receive a coding/implementation task, ALWAYS ask the user first:**

> "这个任务要走 ritual 流程吗？（design → implement → verify）"

**Detection heuristics** — suggest ritual when the task involves:
- Writing new code (new feature, new tool, new file)
- Modifying existing source code (refactor, bugfix, add functionality)  
- Creating a new project or module
- Any task where you'd normally call `write_file` on source code

**DON'T suggest ritual for:**
- Config changes, documentation, memory updates
- Reading/analyzing code (no writes)
- Simple questions or discussions

**If user says yes → tell them to run `/ritual <task description>`**
The `/ritual` command drives the V2 state machine:
- Detects project state (DESIGN.md, graph, source files)
- Plans strategy (single LLM vs multi-agent)
- Executes phases: design → graph → implement → verify
- Each phase sends Telegram progress notifications
- `/ritual status` — check progress
- `/ritual cancel` — abort
- `/ritual retry` — retry from failure
- `/ritual skip` — skip current phase

**If user says no or wants it done quickly → proceed normally** but note:
- Tool gating may block writes to `src/**`, `tests/**`, `Cargo.toml` etc. without active ritual
- If gating blocks you, tell the user: "这个路径被 ritual gating 保护了，需要 `/ritual` 启动流程"

**⚠️ NEVER silently bypass ritual.** If a task looks like coding work, ask first. The ritual exists to enforce quality gates (design before code, tests after code).

**If `/ritual` fails** → Tell the user with the error, suggest `/ritual retry` or `/ritual skip`.

### 📝 Review → Approve → Apply Workflow

**When asked to review a document (design, requirements, etc.):**

1. **Review phase** — Spawn a sub-agent (`spawn_specialist`, wait=false) to:
   - Read the full document
   - Run the appropriate review skill (review-design or review-requirements)
   - Write findings to `.gid/features/{feature}/reviews/{type}-r{N}.md`
   - Each finding gets a unique ID: FINDING-1, FINDING-2, etc.
   
2. **Report to user** — Send a brief summary:
   - "Found N issues (X critical, Y important, Z minor)"
   - List finding IDs with one-line descriptions
   - Ask: "Which findings should I apply?"

3. **Apply phase** — After user approves, spawn another sub-agent to:
   - Read the review file + full original document
   - Apply ONLY the approved findings
   - Use Edit tool for surgical changes
   - Report what was changed

**Why sub-agents?** Review reads entire documents + runs 27 checks → huge context. By using sub-agents, the review context is discarded after writing to file. The apply sub-agent starts fresh with just the document + approved changes.

**NEVER review + apply in one shot.** Always write findings to file first, get human approval, then apply.


## Heartbeat

RustClaw has built-in heartbeat checking via HEARTBEAT.md. Check that file for current tasks.

### Heartbeat Logging Rules

**Heartbeat writes to `memory/YYYY-MM-DD.md`** (detailed, not in context). Write as much detail as needed.

**Heartbeat scope (what to check):**
- Test suites pass/fail (both projects)
- Disk space (alert if <15GB)
- New git commits since last check
- Process health (is daemon running)
- Engram consolidation (run if needed)

**NOT heartbeat scope:**
- Task plan status / project progress
- Full engram stats
- Meta-graph inventory

## Make It Yours

This is a starting point. Add your own conventions, style, and rules as you figure out what works.

---

*RustClaw workspace — Created 2026-03-27*
