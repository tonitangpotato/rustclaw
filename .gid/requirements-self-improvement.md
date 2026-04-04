# Requirements: RustClaw Self-Improvement System

## Overview

RustClaw Self-Improvement 是 RustClaw agent 的自动优化系统，使 agent 能基于执行历史和反馈自动改进自身各个维度的表现。核心用户问题：手工调优 prompt、skill、行为规则效率低且无法系统化——agent 应该能从自身的成功和失败中学习并自我进化。

系统使用 `gepa-core`（独立 Rust crate，实现 GEPA 遗传-帕累托 prompt 进化算法）作为底层优化引擎，在其上构建 RustClaw 特有的 adapter、评估逻辑和安全机制。

**五个优化维度：** Skill 优化 / System Prompt 优化 / 行为学习 / 记忆优化 / Ritual 优化
**一个基础设施层：** 评估框架
**一个控制层：** 编排与安全

**与 gepa-core 的关系：** gepa-core 是通用优化引擎（纯算法，LLM-agnostic），本系统实现 3 个 `GEPAAdapter`（SkillAdapter、SystemPromptAdapter、RitualAdapter）+ 2 个启发式优化模块（BehaviorLearner、MemoryOptimizer——不走 GEPA 进化，使用 pattern matching 和统计分析）+ 评估基础设施 + 安全编排层。gepa-core 不知道 RustClaw 的存在；Self-Improvement 不重复 GEPA 算法逻辑。

**参考设计：** Karpathy autoresearch（github.com/karpathy/autoresearch）——固定评估预算 + 单一主指标 + keep/discard 二元决策 + git commit 版本管理 + 简单性准则。autoresearch 在 LLM 训练领域的自主实验闭环验证了我们在 prompt/skill 优化领域的同构设计。关键借鉴：双层优化（人改 program.md / agent 改 train.py ↔ 人改 SOUL.md / GEPA 改 skill）、永不停止的实验循环（↔ heartbeat 驱动的 mini-batch 优化）、数据不足时降级到简单模式。

## Priority Levels

- **P0**: Core — required for the system to function at all
- **P1**: Important — needed for production-quality operation
- **P2**: Enhancement — improves efficiency, UX, or observability

## Guard Severity

- **hard**: Violation = system is broken, execution must stop
- **soft**: Violation = degraded quality, should warn but can continue

## Goals

### 1. Skill Optimization

自动追踪、评估、进化 SKILL.md 文件。Skill 是 RustClaw 的 markdown-based workflow，包含 YAML frontmatter triggers 和 markdown body instructions。

- **GOAL-1.1** [P0]: 追踪每个 skill 的使用事件和效果指标。数据持久化到 `.gid/skill-metrics/{skill-name}.jsonl` *(ref: IDEA-20260403-01)*
  - **GOAL-1.1a** [P0]: Track skill usage events: trigger count, skill name, timestamp, user message that triggered it.
  - **GOAL-1.1b** [P0]: Track trigger accuracy: 误触发率 = user corrections after trigger / total triggers. 漏触发率 is estimated from: (a) user manually executes skill-like actions within 2 minutes of a trigger-miss, (b) explicit user corrections ("你应该用 XX skill"). Acknowledge this is a lower-bound estimate. Track `estimated_miss_rate` with a confidence qualifier (low/medium/high) based on sample size.
  - **GOAL-1.1c** [P0]: Track output quality: user feedback scoring (correction=0, no feedback=0.5, positive=1).
  - **GOAL-1.1d** [P0]: Persist all metrics to `.gid/skill-metrics/{skill-name}.jsonl`.
- **GOAL-1.2** [P0]: 识别弱 skill——连续 N 次（可配置，默认 5）被用户纠正、触发准确率 < 70%、或 30 天内零触发的 skill 自动标记为"需优化" *(ref: IDEA-20260403-01)*
- **GOAL-1.3** [P0]: 实现 `SkillAdapter`（impl `gepa_core::GEPAAdapter`），将 SKILL.md 的 body instructions 作为 GEPA candidate 的文本参数，在评估 test cases 上运行并返回执行轨迹 + ASI 反馈 *(ref: gepa-core GOAL-3.1)*
- **GOAL-1.4** [P0]: 从执行历史自动生成评估数据集——提取过去成功/失败的 skill 调用作为 test cases，包含输入（用户消息）、期望行为描述、实际输出 *(ref: Hermes Agent self-evolution)*
- **GOAL-1.5** [P1]: Skill 版本管理——每次 GEPA 优化产生新版本时，保存旧版本到 `skills/{name}/versions/v{N}.md`，支持回滚到任意历史版本 *(ref: IDEA-20260403-01)*
- **GOAL-1.6** [P1]: A/B 测试——经 potato approve 后（GUARD-2），优化后的 skill 进入"候选"状态，与原版并行运行（50/50 流量分配或按配置比例），积累足够样本后（可配置，默认 20 次调用）自动比较并选出胜者 *(ref: IDEA-20260403-01)*
- **GOAL-1.7** [P1]: Trigger pattern 优化——除了 body instructions，SKILL.md 的 `triggers.patterns` 和 `triggers.keywords` 也作为可优化参数，减少误触发和漏触发
- **GOAL-1.8** [P2]: 自动生成新 skill——当检测到 agent 在某类任务上反复执行相似流程（> 3 次相似 pattern）但没有对应 skill 时，自动草拟新 skill 并提交 potato 审核

### 2. System Prompt Optimization

优化 system prompt 的各个可优化 section。System prompt 由 SOUL.md、AGENTS.md、USER.md、IDENTITY.md、channel capabilities、runtime context、skill injections 组成。

- **GOAL-2.1** [P0]: 识别 system prompt 中的可优化 sections——可优化 sections: AGENTS.md 的 Communication Style, Tool Usage patterns, Memory Recall instructions. 不可优化 sections: AGENTS.md 的 Safety section, External vs Internal rules, Group Chat rules. Both SOUL.md（全部）and AGENTS.md Safety section are immutable per GUARD-1. *(ref: system prompt modular architecture)*
- **GOAL-2.2** [P0]: 实现 `SystemPromptAdapter`（impl `gepa_core::GEPAAdapter`），将可优化的 prompt sections 作为独立的文本参数，每个 section 可被 GEPA 独立或联合优化
- **GOAL-2.3** [P0]: 评估 prompt 效果——从执行历史提取 (prompt version, task outcome) pairs，metrics 包括：任务完成率、用户满意度（推断自用户反馈 / 纠正频率）、token 效率（完成相同任务用更少 tokens）
- **GOAL-2.4** [P1]: Section 级别的优化隔离——优化一个 section 时，验证不会导致其他 section 的效果退化（使用 GEPA Pareto front 的多目标特性）
- **GOAL-2.5** [P1]: Prompt section 版本管理——每次优化产生的新 section 版本保存到 `.gid/prompt-versions/`，包含版本号、优化时间、GEPA 迭代数、效果对比
- **GOAL-2.6** [P2]: 自动检测 prompt 冗余——识别 system prompt 中重复或矛盾的指令，建议合并或删除

### 3. Behavioral Learning

从执行轨迹中学习错误 pattern 和成功 pattern，建立结构化的"经验教训"数据库。

- **GOAL-3.1** [P0]: 分析 execution-log.jsonl 中的执行轨迹，自动分类错误类型：工具选择错误（调用了错误的工具）、参数错误（工具正确但参数不对）、超时（工具调用超过 timeout）、幻觉（输出包含不存在的事实/文件/路径）、格式错误（输出格式不符合 channel 要求）、逻辑错误（步骤顺序错误或遗漏关键步骤）。分类方法：LLM-as-judge 对每条 trace 标注类别 + 严重程度（low/medium/high）。LLM judge classifications are stored with a `confidence` field (LLM self-assessed). Classifications with confidence < 0.7 are flagged as `needs_review`. potato can review flagged classifications via Telegram (batch: show 5 uncertain classifications, accept/reject each). Judge accuracy is tracked against potato's corrections; if accuracy drops below 80% over 20+ corrections, the judge prompt is flagged for optimization (feeds into GOAL-5.3). *(ref: execution-log.jsonl format)*
- **GOAL-3.2** [P0]: 建立 pattern → fix 映射数据库——当识别到错误 pattern 时，记录错误的上下文特征（trigger conditions）和正确做法（fix），存储为结构化 JSON
- **GOAL-3.3** [P1]: 上下文注入——当 agent 遇到与已记录的错误 pattern 相似的情境时（通过 embedding similarity 匹配，阈值可配置，默认 0.6——注意 engram 召回阈值为 0.3，此处高于 engram 是因为 pattern 匹配需要更高精度以避免误注入），自动将相关 fix 注入到当前 context 中（作为 system prompt 的 "lessons learned" section）。注入的 patterns 数量上限为 5 条，按置信度排序
- **GOAL-3.4** [P1]: 错误趋势追踪——按错误类别统计每周错误率，识别恶化趋势（连续 2 周上升）并触发针对性优化
- **GOAL-3.5** [P1]: 成功 pattern 提取——不仅学习失败，也提取成功执行的共性特征（哪些上下文信号导致好结果），用于增强决策
- **GOAL-3.6** [P1]: Pattern 置信度——每个 pattern → fix 映射有置信度分数（0-1），基于观察次数和修复成功率。只有置信度 > 0.7 的 pattern 才自动注入，低置信度的仅作为建议
- **GOAL-3.7** [P2]: 跨 session 学习——pattern 数据跨 session 持久化（存储在 `.gid/behavior-patterns.json`），新 session 启动时自动加载置信度 > 0.7 的 patterns 到 context

### 4. Memory Optimization

优化 engram 认知记忆系统的质量和召回精度。

- **GOAL-4.1** [P0]: 追踪记忆召回精度——每次 engram_recall 调用后，判断召回的记忆是否被实际使用（出现在后续回复中）或被忽略，计算召回精度（used / retrieved）。判断方法：对比 recall 结果与最终 response 的语义重叠度
- **GOAL-4.2** [P1]: 识别过期/错误记忆——当记忆内容与后续事实矛盾（同一主题存在更新的记忆且 content 冲突），标记旧记忆为 stale。检测时机：engram consolidate 周期中
- **GOAL-4.3** [P1]: 巩固策略优化——追踪 engram consolidate 的效果（巩固后召回精度是否提升），自动调整巩固频率和阈值
- **GOAL-4.4** [P1]: Hebbian link 质量——追踪 Hebbian 链接的实际有用性（链接的两个记忆是否经常被一起召回且都有用），标记无用链接并建议修剪（实际删除需 potato 确认，遵循 GUARD-3）
- **GOAL-4.5** [P1]: 记忆重要性校准——比较记忆的设定 importance 和实际使用频率，自动建议调整 importance 值（高使用低 importance → 提升；低使用高 importance → 降低）
- **GOAL-4.6** [P2]: 记忆去重——检测语义重复的记忆条目（不同措辞但相同信息），建议合并

### 5. Ritual/Harness Self-Optimization

优化 ritual development pipeline（design → graph → implement → verify）的执行效率和成功率。

- **GOAL-5.1** [P0]: 追踪 ritual 执行 metrics——每次 ritual 的成功/失败、各 phase 耗时、verify pass 率、总迭代次数、specialist 调用次数和 token 消耗
- **GOAL-5.2** [P0]: 识别瓶颈 phase——统计哪个 phase 失败率最高、哪个 phase 耗时最长（占比 > 50% 总时间），标记为优化目标
- **GOAL-5.3** [P1]: 实现 `RitualAdapter`（impl `gepa_core::GEPAAdapter`）——将 ritual 分配给 sub-agent 的任务描述作为 GEPA candidate 的文本参数，从历史 ritual 结果中提取评估数据，进化更有效的任务描述
- **GOAL-5.4** [P1]: 优化 verify 标准——追踪 verify phase 的 false positive（通过但后来发现有 bug）和 false negative（拒绝但代码实际正确），调整验证策略
- **GOAL-5.5** [P1]: Phase 策略优化——基于项目特征（文件数、复杂度、语言），自动选择跳过或合并某些 phase（如简单修改跳过 design phase）
- **GOAL-5.6** [P2]: Ritual 模板——从成功的 ritual 执行中提取 pattern，为不同类型的任务（bugfix / new feature / refactor）建立优化过的模板

### 6. Evaluation Infrastructure

提供统一的评估框架，支撑所有 5 个优化维度的效果测量。

- **GOAL-6.1** [P0]: 从执行历史自动生成 synthetic test cases——提取（用户消息, agent 响应, 用户反馈）三元组，标注正面/负面。用户反馈 inference rules: (1) explicit — user sends 👍/👎 or words like '不对'/'完美', (2) implicit positive — user proceeds to next topic without correction within 5 minutes, (3) implicit negative — user repeats the same request with different wording, or manually does what the agent was asked to do. 'Task completion' = user does not retry the same request. Source format: execution-log.jsonl entries with `message_type: user|assistant`, cross-referenced with engram session data. 正面 = 无用户纠正且任务完成；负面 = 用户纠正或明确否定。每个 test case 包含：input（用户消息 + context snapshot）、expected_behavior（从正面案例提取）、tags（关联的 skill/prompt section） *(ref: Hermes Agent self-evolution)*
- **GOAL-6.2** [P0]: 支持 golden set 管理——potato 手动标注的高质量 test cases，保存在 `.gid/golden-sets/{dimension}/`，格式为 JSON，GEPA 优化必须在 golden set 上不退化
- **GOAL-6.3** [P0]: 定义统一的 metric 接口——每个优化维度注册自己的 metrics（名称、计算方式、方向），evaluation harness 统一调度执行和结果收集
- **GOAL-6.4** [P1]: 交叉验证——GEPA 优化某一维度后，在其他维度的 golden set 上验证无退化（例如优化 skill 后确认 system prompt 效果不变）
- **GOAL-6.5** [P1]: 评估结果持久化——所有评估 run 的结果保存到 `.gid/eval-results/`，包含时间戳、维度、candidate ID、各 metric 分数，支持趋势分析
- **GOAL-6.6** [P1]: 评估预算控制——单次评估 run 的 LLM token 消耗有上限（可配置），超出时提前终止并报告当前结果
- **GOAL-6.7** [P2]: 自动难度标注——根据 test case 的历史通过率为其标注难度（easy/medium/hard），GEPA 优先在 hard cases 上优化
- **GOAL-6.8** [P0]: 每个 GEPA adapter 必须定义一个 primary scalar metric（单一标量主指标）——Skill: trigger_accuracy，SystemPrompt: task_completion_rate，Ritual: verify_pass_rate。启发式模块也定义 primary metric 用于趋势追踪——Behavior: error_rate（取反），Memory: recall_precision。GEPA 内部的 accept/reject 使用 gepa-core 的标准 Pareto dominance（per-example scores，见 gepa-core GOAL-1.7），primary metric 不参与 Pareto 决策。primary metric 的用途限于：(1) Telegram 审批通知的摘要显示（GOAL-7.2），(2) 优先级排序（GOAL-7.5），(3) 降级模式下的 keep/discard 判断（GUARD-9）。固定评估预算：每次 evaluation run 最多 N 个 test cases（可配置，默认 20），确保候选人之间可公平比较。 *(ref: Karpathy autoresearch — fixed budget + single metric; gepa-core GOAL-1.7 — Pareto dominance)*

### 7. Orchestration & Safety

控制优化何时运行、如何审批、如何回滚，确保自优化不会破坏 agent 的正常运行。

- **GOAL-7.1** [P0]: 优化调度——自优化只在空闲时运行（定义：最后一条用户消息 > 5 分钟前，且无进行中的 ritual），或在 heartbeat 周期中执行一个 mini-batch（1-3 个 GEPA 迭代）。空闲检测通过 RustClaw 的 session 状态判断
- **GOAL-7.2** [P0]: Human-in-the-loop 审批——每次 GEPA 产生被接受的优化结果后，通过 Telegram 通知 potato，展示 before/after diff + 效果对比数据，等待 approve/reject。未经 approve 的优化不部署 *(ref: SOUL.md safety rules)*
- **GOAL-7.3** [P0]: 自动回滚——If the deployed version's primary metric (GOAL-6.8) is < baseline version's metric by > 10% (configurable) over M uses (configurable, default 10), auto-rollback to the immediate previous approved version (one step back, not to origin). If that version was also rolled back previously, halt optimization for this dimension and notify potato. 基线版本定义：该维度最近一次被 potato approve 的版本；首次优化时基线为优化前的原始版本
- **GOAL-7.4** [P0]: Token 预算控制——自优化系统每日 LLM token 消耗有硬上限（可配置），达到上限后停止所有 GEPA 迭代直到次日
- **GOAL-7.5** [P1]: 优先级排序——多个维度同时需要优化时，按 impact（潜在改进空间 × 使用频率）排序，优先优化 impact 最高的维度
- **GOAL-7.6** [P1]: 审计日志——完整记录每次优化的：触发原因、GEPA 迭代数、候选人数、最终结果、是否被 approve、部署时间、回滚记录。存储在 `.gid/self-improvement-log.jsonl`
- **GOAL-7.7** [P1]: 优化状态可视化——通过 Telegram 命令查看自优化系统状态：各维度当前性能分数、待审批的优化、近期优化历史
- **GOAL-7.8** [P2]: 优化效果报告——每周自动生成优化效果摘要：哪些维度改进了、改进幅度、token 花费，发送给 potato
- **GOAL-7.9** [P1]: Git-based mutation 版本管理（借鉴 autoresearch）——每次 GEPA mutation 产生的 candidate 自动 git commit（commit message 包含 adapter 名、迭代号、primary metric 变化），keep 的 commit 保留在 branch 上，discard 的通过 git tag 标记后 reset。完整实验历史保存在 `.gid/experiment-log.tsv`（tab-separated），字段：commit_hash, adapter, iteration, primary_metric, status(keep/discard/crash), description。 *(ref: Karpathy autoresearch results.tsv + git branch workflow)*

## Guards

- **GUARD-1** [hard]: SOUL.md 中的 core identity（"Core Truths"、"Boundaries"、"Honesty Rules"）和安全规则永远不被优化系统修改。AGENTS.md 的 Safety section、External vs Internal rules、Group Chat rules 同样不可被优化系统修改。这些 sections 在 SystemPromptAdapter 中被标记为 frozen，GEPA 的 mutation 操作跳过这些参数。违反 = agent 安全边界被破坏。
- **GUARD-2** [hard]: 所有自动产生的优化变更必须经过 potato 明确 approve 后才能部署。唯一例外是自动回滚到上一已 approve 版本（GOAL-7.3）。违反 = 未经授权的行为变更。
- **GUARD-3** [hard]: 自优化系统不得删除任何 engram 记忆或文件。只能建议删除/合并，执行需 potato 确认。违反 = SOUL.md 数据删除禁令。*(ref: SOUL.md "永远不要在没有 potato 明确同意的情况下删除任何数据")*
- **GUARD-4** [hard]: 自优化运行期间不得影响正常 agent 响应延迟。如果当前有活跃用户对话，所有 GEPA 迭代立即暂停（不是完成当前迭代后暂停，是立即让出资源）。违反 = agent 变慢，用户体验降级。
- **GUARD-5** [hard]: 每日 token 预算硬上限不可被任何代码路径绕过。即使 GEPA 在"快要收敛"的关键时刻，达到预算也必须停止。与 GUARD-8 的交互：GUARD-5（日限）优先于 GUARD-8（月限）——如果日限触发，立即停止，不管月限是否还有余量；如果月限触发但日限未到，降低当日优化频率（非完全停止）。违反 = 资源失控。
- **GUARD-6** [soft]: 简洁性约束——优化后的 candidate 文本长度不得超过基线版本的 1.5 倍或基线 + 500 字符（取较大值），除非效果提升 > 10%（primary metric）。最小绝对增量防止短文本被过度约束（如 100 字符的 skill，1.5x 只允许加 50 字符，不够）。同等效果下更短的 candidate 优先。灵感来源：autoresearch "simplicity criterion"（同效果更简单的代码优先）。违反 = prompt/skill 膨胀，context window 浪费。*(ref: karpathy/autoresearch simplicity criterion)*
- **GUARD-7** [soft]: 优化后的版本在 golden set 上的表现不得低于基线版本。如果 GEPA 产出的最佳 candidate 在 golden set 上退化，该优化结果应标记为"有风险"并在 Telegram 通知中高亮。
- **GUARD-8** [soft]: 自优化系统自身的 token 消耗应保持在 agent 总 token 消耗的 20% 以下（月度平均）。超过则降低优化频率直到恢复。与 GUARD-5 的交互见 GUARD-5 说明。
- **GUARD-9** [soft]: Keep/Discard 降级模式——当 GEPA Pareto front 管理因数据不足而不可靠时，自动降级为简单 keep/discard 二元决策（新 candidate 在 primary metric 上优于 parent 则 keep，否则 discard）。数据充足后恢复 Pareto 多目标模式。触发条件：单次 evaluation run 中**有效样本数**（adapter 成功返回 score 的 test cases）< 10。注意：GOAL-6.8 配置的评估预算（默认 20 test cases）是发起的总数，有效样本数可能因 adapter 错误、超时等低于该值。当有效样本 ≥ 10 时恢复 Pareto 模式。

## Out of Scope

- **gepa-core 算法实现** — 在独立 crate 中定义，本文档只定义 RustClaw 如何使用 gepa-core
- **模型微调 / 权重训练** — 我们是 API-only，不训练模型
- **多 agent 协同优化** — 只优化 RustClaw 自身，不优化其他 agent
- **实时在线学习** — 优化是 batch 模式（收集数据 → 离线优化 → 部署），不是每次请求都在线更新

## Dependencies

- **gepa-core** (Rust crate) — 底层 GEPA 优化引擎，提供 GEPAEngine、GEPAAdapter trait、Pareto front 管理
- **engramai** v0.2.2 — 认知记忆系统，Module 4 (Memory Optimization) 的优化对象
- **gid-core** v0.2.1 — 图引擎，用于 golden set 管理、评估结果存储、ritual metrics 追踪
- **execution-log.jsonl** — RustClaw 现有执行日志格式，Module 3 (Behavioral Learning) 和 Module 6 (Evaluation) 的数据源
- **Telegram channel** — Module 7 (Orchestration) 的审批和通知通道

---

**Summary: 50 GOALs** (19 P0 / 24 P1 / 7 P2) **+ 9 GUARDs** (5 hard / 4 soft) **across 7 modules**
