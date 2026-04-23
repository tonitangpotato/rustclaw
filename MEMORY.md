# MEMORY.md - Long-term Memory (RustClaw)

> This is RustClaw's memory — the Rust-native AI agent framework.
> Engram DB at `~/rustclaw/engram-memory.db`.

---

## 📍 Canonical Project Roots (check HERE FIRST before searching paths)

Each project has ONE canonical root. Do NOT search for graphs/issues — look them up here.

- **engram** (monorepo, active, consolidated 2026-04-22 ISS-023) → `/Users/potato/clawd/projects/engram/`
  - GitHub: tonitangpotato/engram-ai (monorepo)
  - Last commit: `3132194 feat: consolidate engram-ai-rust into monorepo (ISS-023)`
  - ⚠️ Graph state mid-migration (2026-04-22): `engram/.gid/` has issues + graph.db but NOT the latest graph.yml with ISS-024 nodes. The live graph.yml (114 nodes, Apr 22 23:33) is still in engram-ai-rust/.gid/. Do NOT push ISS-024 work to the old repo — ask potato where to put it.
- **engram-ai-rust** (old repo, preserved but not active) → `/Users/potato/clawd/projects/engram-ai-rust/`
  - potato: "repo不要不要闪，还要都要留着的，只是merge到那个monorepo里面"
  - All rustclaw/cogmembench configs now point to engram/, NOT this one
- **rustclaw** (this workspace) → `/Users/potato/rustclaw/`
- **agentctl** → `/Users/potato/clawd/projects/agentctl/`
- **xinfluencer** → `/Users/potato/clawd/projects/xinfluencer/`
- **gid-rs** → `/Users/potato/clawd/projects/gid-rs/`
- **autoalpha** → `/Users/potato/clawd/projects/autoalpha/`
- **causal-agent** → `/Users/potato/clawd/projects/causal-agent/`
- **swebench** → `/Users/potato/clawd/projects/swebench/`
- **interview-prep** → `/Users/potato/clawd/projects/interview-prep/`

If potato mentions an issue ID (ISS-NNN) or task name, resolve via this table, don't grep/find.

**Common mistake (I've made this 2x now):** Assuming engram-ai-rust is "the real one" because its graph.yml is fresher. The monorepo consolidation happened 2026-04-22 — engram/ is canonical going forward, even though the graph.yml migration is incomplete.

Tracked: `.gid/issues/ISS-020-project-path-discovery-friction.md`

---

## About potato

- **Name**: potato (oneB)
- 全职程序员, building towards financial freedom
- **Personality**: curious, honest, moves FAST, prefers action over planning
- **Values honesty** over performance. Trusts agent with big ideas.

### Working Style
- potato has ideas → crystallize into designs → iterate fast
- "以后never简化问题" — potato的明确要求
- Prefers deep explanations with concrete examples, not jargon

---

## RustClaw Development History

### Architecture
- **Rust AI agent framework** — full-featured, single binary
- 35MB release binary, 140 tests, 0 warnings
- **Channels**: Telegram (@rustblawbot), Discord, Signal, WhatsApp, Matrix, Slack
- **Memory**: Engram (engramai crate) + GID (gid-core crate) + file-based logs
- **LLM**: Anthropic (Claude), OpenAI, Google — streaming support
- **Orchestrator**: Multi-specialist delegation (coder + researcher)
- **Dashboard**: Web UI at port 8081

### Dependencies (crates.io)
- `engramai` v0.2.2 — neuroscience-grounded memory (ACT-R, Hebbian learning)
- `gid-core` v0.2.1 — graph-indexed code intelligence + task management

### Completed Features (all 13 TODOs done, 2026-03-28)
- ✅ Token Tracking — TokenTracker atomic counters, all providers
- ✅ Heartbeat Channel Routing — non-HEARTBEAT_OK responses auto-send to Telegram
- ✅ Streaming Telegram — typing indicator + streaming output
- ✅ Session Persistence — SQLite-backed conversation history
- ✅ Sub-agent Shared Engram — `for_subagent_with_memory()`
- ✅ Hot-reload Orchestrator — config changes auto-update specialists
- ✅ TTS/STT — built-in VOICE: prefix, OGG output
- ✅ Dashboard Agent Name — reads from IDENTITY.md
- ✅ Dashboard Orchestrator View — /api/tokens + /api/orchestrator
- ✅ Interactive CLI — `rustclaw chat` REPL with /clear
- ✅ Interactive Setup — `rustclaw init` wizard
- ✅ Code Cleanup — dead code removed, 0 warnings

### Cross-Language Drive Alignment (2026-03-29)
- Problem: SOUL.md in Chinese → keyword matching fails for English content
- Solution: `score_alignment_hybrid()` = max(keyword, embedding) in engramai
- DriveEmbeddings pre-computed at startup, threshold 0.3 for cross-language

### Context System Refactor (2026-03-29, biggest change)
- **src/context.rs** — 6 new types: MessageContext, ChatType, QuotedMessage, ChannelCapabilities, RuntimeContext, ProcessedResponse
- **MessageContext** — LLM sees sender name/username, chat type (direct/group), quoted messages
- **ChannelCapabilities** — channels declare what they support (voice, tables, markdown, etc.), LLM adapts output format
- **RuntimeContext** — OS, arch, version, hostname injected into system prompt
- **ProcessedResponse** — unified extraction of VOICE:, NO_REPLY, [[reply_to:N]] from raw LLM output
- **Modular system prompt** — broke monolithic format! into 10 composable sections
- **Yesterday's daily notes** — system prompt now loads yesterday's log too

### Skill System (2026-03-29)
- **Skills auto-loading** — scans `skills/*/SKILL.md`, injects into system prompt
- **Dynamic trigger matching** — YAML frontmatter with triggers, priority, always_load
- **Idea Intake Pipeline** — first skill: processes URLs/ideas into IDEAS.md + engram + GID

### Bug Fixes (2026-03-29)
- **fd leak** — notify kqueue→fsevent, config watcher watches file not directory
- **FTS5 corruption** — rebuilt full-text search index in engram DB
- **block_in_place** — OAuth token refresh in async context panic fix
- **whisper.cpp** — Python whisper→whisper-cli, 3x faster STT (32s→11s)

### Behavior Improvements (2026-03-29)
- Persistent typing indicator (refresh every 4s)
- Unified send_response (voice/text logic consolidated)
- Voice mode toggle per chat
- "Acknowledge before working" rule in system prompt + AGENTS.md

### Test Count: 281 (up from 166; +2 ISS-021 Phase 1 baseline tests)

### ISS-021 Phase 1 — Envelope side-channel (2026-04-23)
- **Envelope** type replaces `MessageContext` (alias kept until Phase 4)
- `HookContext.envelope: Option<Envelope>` plumbed (defaults None, populated by Phase 2+3)
- `MemoryManager::store/store_explicit` migrated to `engram.store_raw(content, StorageMeta { user_metadata, ... })` — accepts optional Envelope, serializes to `user_metadata.envelope`
- Recall quality baseline: 10 fixtures × (3 gold + 5 distractor incl. 1 near-topic), Precision@3 metric, **P_before = 0.767** (unsaturated, 0.233 headroom for Phase 5 significance test)
- `MemoryManager::for_testing()` test-only constructor — avoids hand-constructed struct literal fragility across refactors
- Storage audit built into baseline test: asserts 0 Quarantined, all items land as `Stored(_)` — ensures store_raw migration is a behavioral no-op
- **Phase 1 scope expanded (justified)**: store_raw migration + fixture re-design + for_testing refactor all pulled into Phase 1 as root-fix prerequisites (details in `.gid/issues/ISS-021.../issue.md` "Phase 1 Execution Record")

## Core Rules

- **NEVER simplify the architecture** — follow the design (potato's explicit rule)
- Use GID for code structure analysis, dependency tracking, impact queries, and task management
- **NEVER fabricate numbers** — always compute from data
- Double-write rule: MEMORY.md + daily log + engram for key learnings

### Architecture Notes
- **context.rs** is the new "structured metadata" layer between channels and the agent
- System prompt is modular: context files → skills → channel caps → runtime → behavior rules
- Skills are markdown-based workflows with YAML frontmatter triggers — no Rust code needed

### Interoceptive Emotion System (2026-04-19)
- **Layer 1 + Layer 2: LIVE** — 4 signal meters → InteroceptiveHub → system prompt injection
- **Layer 3 (行为调制): ✅ DONE** — 自适应σ偏差检测 + 系统提示注入 + 循环内干预
- **核心目的**: 自我察觉→自我修正（闭环）。stress高→换策略，flow低→主动问人，load爆→提前收束
- **实现**: AdaptiveBaseline (Welford算法), cold-start fallback, mid-loop intervention at 3+ consecutive failures
- 247 tests pass, engramai v0.2.3, src/interoceptive.rs 530 lines

*Last updated: 2026-04-23 (ISS-021 Phase 1 complete)*

---

## GID Ecosystem (2026-04-02)

### 四个项目定位
- **gid-core** — 图引擎 + 共享类型（事件格式、状态 schema）
- **gid-harness** — AI 自主开发执行引擎 ✅ **已完整实现**
- **gidterm** — TUI surface，纯展示层，读 execution-log.jsonl
- **agentctl** — daemon 进程管家（TUI + Telegram bot，7,001行，38 tests）

### gid-harness ✅ DONE
- **15 个 Rust 源文件，6,881 行代码**
- 路径：`/Users/potato/clawd/projects/gid-rs/crates/gid-core/src/harness/`
- 模块：executor, scheduler, replanner, context, notifier, planner, verifier, topology, worktree, config, types, telemetry, log_reader, execution_state
- 文件系统是 backend：graph.yml + execution-log.jsonl + execution-state.json
- 7-Phase 流程（Phase 1-3 人机协作，Phase 4-7 AI 自动）
- gate:human tag 做审批控制

### 关键架构决策
- 方案 B：harness 独立实现，gidterm 是纯 UI
- 共享协议不共享代码：事件格式和状态 schema 在 gid-core
- 所有 surface（Telegram、gidterm、CLI）读写同一套文件

---

## 产品商业化定位 (2026-04-03 potato 明确)

### 可卖钱的产品
- **xinfluencer** — X/Twitter 影响力增长工具，Rust，6,462 行，13 模块
  - 自用：集成进 RustClaw，Telegram Bot 控制
  - 商业：作为独立 SaaS 产品卖
  - 功能：autopilot, engage, discover, crawler, scoring, brand_audit, graph, monitor
- **Knowledge Compiler** (IDEA-20260403-02) — 知识管理产品化

### 内部工具（不适合直接卖）
- **gid-harness** — AI 开发执行引擎，主要内部使用，作为服务卖比较困难
- **agentctl** — 进程管家，纯运维工具

---

## Recall 失败根因 + Trace Logger (2026-04-23)

### 三层缺陷(今晚深挖,potato 直觉命中根因)
1. **Store 层** — 关键 meta 事件(canonical repo 是哪个等)没入规范记忆或 compile 成 topic
2. **Compile 层(根因)** — knowledge_compile 没编译"仓库结构"这类 meta 话题,搜索 0 命中。133 个 unresolved conflicts 可能阻塞新 topic 生成
3. **Retrieve 层** — EngramRecallHook query 构造 naive:`session_recall(&ctx.content, ...)` 直接用消息原文当 query,无 session context 融合,无 meta-state 检索

### 幻觉传导机制(新洞察,今晚 session 内演示两次)
Recall 失败 → agent 无法察觉 → 用想象补齐 → 输出错误信息。
Agent 无法区分"hook 注入的是真相"vs"hook 注入的是噪音"。
**对策**:看到 recall 返回时质疑"这是不是只是偏移",关键事实性断言先用工具核实。

### 已实施:Recall Trace Logger
- 改了 `src/engram_hooks.rs`,新增 `write_recall_trace()`,每次 recall append 一行 JSON 到 `/Users/potato/rustclaw/recall-trace.jsonl`
- 字段:ts, session_key, query, query_len, ok, full_recall_triggered, result_count, results[{content, type, confidence, label}]
- 失败静默,零行为变化,cargo check 通过,3/3 测试过
- 分析命令:`jq 'select(.result_count == 0)' recall-trace.jsonl` 等

### 待办
1. 累积 trace 数据(几小时 - 1 天),离线分类失败模式
2. 写 engram 项目 bug issue(Store/Compile/Retrieve + 幻觉传导 + 实时证据)
3. `knowledge_compile --dry-run` 诊断 Compile 层(为啥 discovery 不聚类 meta 话题)
4. 检查 133 个 unresolved conflicts 是否在阻塞
5. **新 IDEA**:Pre-restart auto-summary hook(今晚发现"自动总结再重启"功能实际不存在,只是 `restart_self` 发 Telegram + exit)

详细时间线:`memory/2026-04-23.md` 00:00-00:30 段落。

---

## engram × gid 通用 KB 架构思考 (2026-04-15)

### 核心洞察：两个 crate 的拼接点已经存在

**engram 已有能力：**
- 实体抽取（`entities.rs`，Aho-Corasick + regex，规则式，抽 Project/Person/Tech/Concept）
- LLM 抽取（`extractor.rs`，text → ExtractedFact，但输出是扁平记忆条目，不是三元组）
- 4信号聚类（`synthesis/cluster.rs`，Hebbian权重 + 实体Jaccard + embedding余弦 + 时间接近度）
- 向量+FTS5混合搜索（`hybrid_search.rs`）
- Hebbian 学习（co-recall 自动建链）

**gid 已有能力：**
- Infomap 社区检测（`infer/clustering.rs`，4700行，加权网络，极其成熟）
- 图操作全套（refactor/validate/impact/deps/advise/visual）
- LLM labeling（聚类后命名）
- 知识节点（per-node findings/file_cache/tool_history）

**关键发现：**
- Infomap 不绑定代码——代码特定的只是边权策略（imports=1.0, calls=0.8）
- 换成通用知识图谱只需换边权：relates_to=1.0, caused_by=0.8, Hebbian强度=直接当权重
- engram 的 extractor 输出格式从 ExtractedFact 改成 (entity, relation, entity) 三元组就能直接喂 gid
- 两个聚类器应该能互相输入但现在互不知道

**通用 KB pipeline：**
```
文本 → engram extractor (改输出格式) → 三元组
                                        ↓
三元组 → gid graph → Infomap 聚类 → 社区发现
                                        ↓
社区 → engram recall 加权 (同社区记忆 Hebbian 增强)
```

**不是两个孤岛要建桥，是拼接口已经在那了，只差一层胶水。**

### 战略意义
- 市面上没人这么做（认知记忆层 + 结构知识图谱层 双层配合）
- Cognee 试图揉成一个但丢失各自优势
- engram 提供发现（"这个可能相关"），gid 提供解释（"具体怎么相关"）
- 这是完整的 agent 知识系统
