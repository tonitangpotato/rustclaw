# Requirements: Marketing Ritual

## Overview

一套端到端的内容营销自动化管道，覆盖从内容发现到效果回收的完整闭环。核心用户问题：potato 有大量 idea 和技术积累，但缺少系统化的内容生产和分发能力，无法将知识转化为影响力和收入。

管道定义：**Intake → 内容生产 → 分发 → 互动管理 → 数据回收 → 自我优化**

### 本质：Ritual Orchestration，不是独立项目

这套管道不是从零构建。它是一个 **Content Ritual**，将已有组件编排成完整闭环。新增的只是：Style Profile 构建、内容生成 prompts、数据反馈环路、以及连接一切的 Content Ritual 本身。

已有组件（直接复用）：
- ✅ **xinfluencer**（Rust，6,462 行）— crawler（抓取推文历史）、engage（互动）、discover（发现高价值内容/账号）、autopilot（自动化流程）、monitor（监控效果）
- ✅ **social-intake**（skills/social-intake）— 7 平台 content extractor，已实测可用
- ✅ **engram**（engramai）— 认知记忆 + 风格数据存储 + 知识关联
- ✅ **RustClaw skills** — orchestration layer，Telegram 通知 + 交互
- ✅ **gid-harness**（Rust crate）— ritual phase 状态机引擎，已完整实现

新增组件（本项目的核心交付）：
- 🆕 **Style Profile** — 从真实文本中蒸馏 potato 的写作风格
- 🆕 **Content Generation Prompts** — 风格化内容生成的 prompt 体系
- 🆕 **Data Feedback Loop** — 效果数据 → 风格/策略调优
- 🆕 **Content Ritual** — 连接上述所有组件的状态机编排

设计原则：
- **Content Ritual 同构** — 和代码 ritual（design → graph → implement → verify）同构，复用 ritual 基础设施（phase 状态机、Telegram 通知、GID task、specialist 分工）
- **自举模式** — 用这套管道来 marketing 管道本身（先验证自己能产出有价值的内容，再把工具开放给别人）
- **去 AI 感** — 所有自动生产的内容必须看不出是 AI 写的（potato 明确要求）
- **人在环中** — 最终发布需要 potato 过目确认，AI 负责 80% 的工作量

## Priority Levels

- **P0**: Core — 没有这个管道不能运转
- **P1**: Important — 生产级运营需要
- **P2**: Enhancement — 提升效率和规模

## Guard Severity

- **hard**: 违反 = 品牌/法律风险，必须停止
- **soft**: 违反 = 质量下降，警告但可继续

## Goals

### 1. 内容生产（Content Production）

potato 提供一个 topic/素材/intake 条目，系统自动生成适合目标平台的内容草稿。

- **GOAL-1.1** [P0]: 给定一个 topic 或 intake 条目引用（如 `intake/twitter/xxx.md`），能生成一篇内容草稿，包含：标题/hook、正文、hashtags/tags *(ref: Content Ritual 讨论 04/03)*
- **GOAL-1.2** [P0]: 草稿必须匹配 potato 的个人写作风格（Style Profile），而非通用 LLM 输出。Style Profile 从 potato 的历史内容（推文、代码注释、聊天记录）中提取 *(ref: "去 AI 感" 要求 04/03)*
- **GOAL-1.3** [P0]: 支持至少 3 种内容类型：short-form（推文/帖子，≤ 280 字符或平台允许的最大长度）、mid-form（thread/小红书图文，500-2000 字）、long-form（博客/公众号，2000+ 字） *(ref: 多平台需求)*
- **GOAL-1.4** [P1]: 支持从已有 intake 内容衍生——不是复制粘贴外部内容，而是基于外部内容生成 potato 自己的见解/评论/扩展 *(ref: "Build in public" + @zarazhangrui 策略)*
- **GOAL-1.5** [P1]: 支持内容系列化——基于一个大 topic 自动拆分成多篇关联内容（thread 系列、系列帖子等） *(ref: 持续输出策略)*
- **GOAL-1.6** [P2]: 根据历史发布效果数据，推荐最佳发布时间和内容类型 *(ref: 数据驱动优化)*

### 2. Style Profile（写作风格画像）

所有内容生产的核心输入。没有 Style Profile，就只能产出通用 AI 内容。

**数据来源：**
- **推文历史** — 通过 xinfluencer crawler 抓取 potato 的 Twitter 历史推文
- **Telegram 聊天记录** — 通过 engram 存储的自然对话风格
- **代码注释 / commit messages** — 从 git 仓库中提取技术写作风格

**构建方法：**
- **LLM 风格蒸馏** — 从真实文本中提取风格特征（词汇偏好、句式、语气、幽默方式、中英混用习惯）
- **负面约束** — 明确封禁 LLM 典型 pattern（"delve into"、"it's worth noting"、过度使用 em dash、列表滥用等）
- **Few-shot 示例** — 精选 potato 的代表性文本作为 few-shot examples，锚定风格

- **GOAL-2.1** [P0]: 从 potato 的多源历史内容（推文、Telegram 聊天、代码注释/commit messages）中提取写作风格特征：常用词汇、句式偏好、语气、幽默方式、中英混用习惯、平台差异 *(ref: "去 AI 感" 04/03)*
- **GOAL-2.2** [P0]: Style Profile 作为结构化文档存储（如 `profiles/potato-style.md`），包含：正面特征（potato 的风格 + few-shot 示例）+ 负面特征（LLM 典型 pattern 黑名单） *(ref: 04/03 方案)*
- **GOAL-2.3** [P1]: 支持平台差异化风格——Twitter 上的 potato 和公众号上的 potato 语气不同 *(ref: 多平台策略)*
- **GOAL-2.4** [P1]: Style Profile 随时间演进——新的内容发布后自动更新，捕捉风格变化 *(ref: 持续优化)*
- **GOAL-2.5** [P2]: 提供 LLM 深度"反 AI 检测"——生成内容后用独立 LLM 从语义层面判断是否像 AI 写的（句式多样性、论证方式、不完美性），如果像则自动 rewrite。与 GUARD-5 的规则快检互补：GUARD-5 做禁用词/pattern 黑名单快筛，GOAL-2.5 做语义级深度检测 *(ref: "去 AI 感" 04/03)*
- **GOAL-2.6** [P0]: 支持冷启动——当 Twitter 历史推文不足（< 50 条）时，以 Telegram 聊天风格为主数据源，并支持 potato 手动提供 10-20 条代表性文本作为种子样本 *(ref: 新号 bootstrap 问题)*

### 3. 内容分发（Distribution）

将审核通过的内容发布到目标平台。

- **GOAL-3.1** [P0]: 支持 Twitter/X 发布（推文、thread、带图推文）——通过 xinfluencer 的 publish 模块 *(ref: xinfluencer publish 模块，见 Dependencies)*
- **GOAL-3.2** [P2]: 阶段二新增至少 2 个平台的分发——Hacker News、Reddit（不含阶段一已有的 Twitter/X） *(ref: potato 的核心受众平台，阶段二实施)*
- **GOAL-3.3** [P1]: 同一内容自动适配不同平台格式——Twitter 版简短有力，Reddit 版详细有深度，HN 版技术聚焦 *(ref: 多平台策略)*
- **GOAL-3.4** [P1]: 支持定时发布——potato 审核通过后设置发布时间，系统自动发 *(ref: UX 优化)*
- **GOAL-3.5** [P2]: 小红书发布支持（图文生成 + 发布） *(ref: 中文受众覆盖)*
- **GOAL-3.6** [P2]: LinkedIn / 个人博客发布支持 *(ref: 长尾覆盖)*

### 4. 互动管理（Engagement）

发布后的互动不能丢，要闭环。

- **GOAL-4.1** [P1]: 监控已发布内容的评论/回复，通过 Telegram 通知 potato *(ref: xinfluencer monitor 模块)*
- **GOAL-4.2** [P1]: 对评论/回复生成回复建议（非自动发送），potato 一键确认或修改后发送 *(ref: "Treat X like a party" 策略)*
- **GOAL-4.3** [P2]: 识别高价值互动者（同行业、高粉丝、反复互动的用户），标记为 CRM 联系人 *(ref: xinfluencer CRM 模块)*
- **GOAL-4.4** [P1]: 主动发现与 potato 兴趣相关的他人帖子，推荐 potato 去互动（而非只发自己的）——通过 xinfluencer discover 模块 *(ref: @zarazhangrui "Reply authentically" 策略)*

### 5. 数据回收与效果分析（Analytics）

没有反馈就没有优化。基于自身历史基线衡量表现，不追求绝对数字。

**Twitter 核心指标：**
- **Impressions** — 曝光量
- **Engagement Rate** — (likes + replies + retweets + clicks) / impressions
- **Reply Depth** — 回复链深度，>3 层 = 引发了真正的讨论
- **Follower Conversion** — 内容带来的新关注数
- **Link CTR** — 链接点击率

**"好表现" 定义：** 超过自身 30 天滑动平均值的可配置倍数（默认 1.5x）。不与他人比较，只与自己的历史比较。

**Pattern 沉淀：** 持续追踪什么 topic / 发布时间 / 内容格式表现最好，沉淀为可复用的 pattern。

- **GOAL-5.1** [P1]: 对每条发布内容追踪效果数据：impressions、engagement rate、reply depth、follower conversion、link CTR *(ref: 数据驱动优化)*
- **GOAL-5.2** [P1]: 生成周报/月报摘要——哪些内容表现好（超过 30 天均值的可配置倍数，默认 1.5x）、哪些差、为什么、下一步建议 *(ref: 闭环优化)*
- **GOAL-5.3** [P1]: 维护 30 天滑动平均基线，所有效果评估基于自身历史对比而非绝对值 *(ref: 以自身基线为标准)*
- **GOAL-5.4** [P1]: 将效果数据与内容特征关联分析——什么 topic/格式/时间点效果最好，沉淀为可复用的 pattern（pattern sedimentation） *(ref: 自我优化闭环)*
- **GOAL-5.5** [P2]: 效果数据反馈到 Style Profile——根据受众反应微调写作策略 *(ref: 自我优化)*

### 6. Proactive Intake（主动内容发现）

系统不只等 potato 喂素材，要主动出击发现高价值内容。组合 xinfluencer discover + social-intake 实现。

- **GOAL-6.1** [P0]: 周期性扫描 potato 关注的账号、话题、关键词，发现高价值内容（通过 xinfluencer discover 模块） *(ref: proactive intake 需求 04/03)*
- **GOAL-6.2** [P0]: 发现的高价值内容自动进入 social-intake 提取管道，结构化存储到 engram *(ref: 自动化 intake 闭环)*
- **GOAL-6.3** [P0]: 发现有价值内容后，通过 Telegram 向 potato 发送通知摘要（内容要点 + 为什么值得关注 + 建议的行动：回复/引用/写衍生内容）。当 potato 选择"写衍生内容"时，自动触发 Content Production（GOAL-1.4），传入 intake 文件路径（如 `intake/twitter/xxx.md`）作为衍生素材 *(ref: 人在环中 + 主动推送 + 跨模块触发)*
- **GOAL-6.4** [P1]: 可配置的扫描策略——关注列表、话题关键词、扫描频率、高价值判定阈值 *(ref: 灵活性)*

### 7. Pipeline 编排与 Ritual（Orchestration）

管道本身需要编排，不是一堆散装 skill。

- **GOAL-7.1** [P0]: 实现 Content Ritual 状态机：intake → draft → review → schedule → publish → analyze，每个 phase 有明确的输入/输出和转换条件 *(ref: Content Ritual 同构讨论 04/03)*
- **GOAL-7.2** [P0]: Telegram 通知 + inline button 控制——potato 在手机上就能操作内容草稿。最少支持 4 个 action：✅ Approve（通过）/ ✏️ Edit（修改后重新生成）/ ❌ Reject（丢弃）/ ⏰ Schedule（设定发布时间） *(ref: 人在环中原则)*
- **GOAL-7.3** [P1]: 支持 batch mode——一次启动多条内容的生产流程，并行处理 *(ref: 规模化运营)*
- **GOAL-7.4** [P1]: 每个 phase 产生结构化 trace（耗时、token 消耗、成功/失败、修改量），为自我优化提供数据 *(ref: gid-harness execution-log 模式, Meta-Harness 论文 "完整历史 > 压缩摘要")*
- **GOAL-7.5** [P2]: Pipeline 配置化——不同内容类型可以跳过某些 phase（如纯转发不需要 draft） *(ref: 灵活性)*
- **GOAL-7.6** [P1]: Draft 生成失败时（LLM 错误、超时、Style Profile 不可用），自动重试最多 3 次（指数退避），持续失败则通知 potato 并将任务标记为 blocked，保存已有 context 供后续重试 *(ref: 可靠性 — draft phase 错误处理)*
- **GOAL-7.7** [P1]: 发布失败时自动重试（指数退避，最多 3 次），持续失败则通知 potato 并保存草稿到队列。API 限流时自动延后发布时间 *(ref: 可靠性)*

### 8. 自我优化（Self-Improvement）

这套管道跑得越多应该越好，不是静态的。自我优化能力依赖 **gepa-core**（独立 crate，将发布到 crates.io）作为外部依赖。

- **GOAL-8.1** [P1]: 基于 execution trace 自动识别管道中的薄弱环节（哪个 phase 耗时最长、哪个 phase 被 potato reject 最多）。此阶段用 heuristic 实现（统计分析），不依赖 gepa-core *(ref: Skill 优化系统)*
- **GOAL-8.2** [P1]: 对表现差的 skill/prompt 生成改进建议（非自动修改），potato 审批后应用 *(ref: GEPA 方法论, Hermes Agent self-evolution)*
- **GOAL-8.3** [P2]: 通过 RustClaw Self-Improvement 系统集成 prompt evolution——Content Production 的 skill 和 prompt 作为 self-improvement 的优化目标，自动生成变体 → 评估 → 择优部署。marketing-ritual 提供评估 metrics（GOAL-5.x 数据），self-improvement 执行优化循环 *(ref: self-improvement GOAL-1.3 SkillAdapter, gepa-core, Hermes Agent DSPy+GEPA)*
- **GOAL-8.4** [P2]: LLM-as-judge 评分体系——对每篇内容草稿打分（风格匹配度、可读性、信息密度、去 AI 感程度），作为自动 eval 的 proxy *(ref: Hermes Agent 评分方法)*

## Guards

- **GUARD-1** [hard]: 所有自动生产的内容，在发布前必须经过 potato 确认（至少看一眼）。系统可以草拟、可以排队、但不能自动发布。唯一例外：potato 通过 Telegram 对特定内容类型明确开启 auto-publish 标记（如 quote tweets），该标记需每 30 天重新确认，过期自动回退为需审批 *(ref: SOUL.md "ask first for external actions")*
- **GUARD-2** [hard]: 生成的内容不得包含虚假信息、未经验证的声明、或抄袭他人原创内容。引用外部内容必须标注来源 *(ref: honesty rules)*
- **GUARD-3** [hard]: 社交平台的 API credentials / cookies 加密存储，不出现在日志、engram、或任何明文文件中 *(ref: security)*
- **GUARD-4** [soft]: 每平台发布频率上限由 potato 配置，系统强制执行上限。新号启动期可设较高频率，成熟期可降低。防止刷屏引起反感 *(ref: @zarazhangrui "consistency > volume")*
- **GUARD-5** [soft]: 基于规则的 AI 痕迹快检——维护 LLM 典型 pattern 黑名单（如 "delve into", "it's worth noting", 过度使用 em dash、列表滥用等），生成内容时实时匹配，命中则自动 rewrite 该段落。这是快速廉价的第一道防线，深度语义检测见 GOAL-2.5 *(ref: "去 AI 感" 要求)*
- **GUARD-6** [soft]: 遵守各平台 rate limit 和 ToS，不用 bot 行为引起封号风险 *(ref: 合规)*
- **GUARD-7** [soft]: 每条内容的生成成本（LLM token）记录在 trace 中，单条内容成本不超过 $0.50（soft limit，可调） *(ref: 成本控制)*

## Out of Scope

- 不做 SEO 优化工具（专业 SEO 不在范围内）
- 不做付费广告投放管理（Google Ads、Twitter Ads 等）
- 不做多用户 SaaS（只为 potato 服务，产品化是后续阶段）
- 不做视频内容生产（短视频/长视频制作不在当前范围，文字+图片优先）
- 不做竞品监控/行业报告（那是 research 工具的职责）

## Dependencies

- **social-intake**（skills/social-intake）— 管道的入口，已实现
- **xinfluencer**（Rust crate）— crawler（推文历史抓取）、discover（高价值内容发现）、engage（互动）、monitor（效果监控）、autopilot（自动化流程）、**publish（待新增）**——用于发布推文/thread，当前不存在，需作为新模块开发
- **engram**（engramai）— 记忆存储 + 风格数据存储 + 知识关联
- **RustClaw skills** — orchestration layer，Telegram 通知 + 交互
- **gid-harness**（Rust crate）— Content Ritual 的状态机引擎，已完整实现
- **gepa-core**（Rust crate，独立项目）— prompt evolution / self-optimization 引擎，将发布到 crates.io，GOAL-8.x 的外部依赖
- **Telegram inline buttons** — potato 审批内容的主要交互方式，需在 RustClaw 中实现

## Implementation Strategy

### 阶段一：Twitter/X 全功能完成

单平台打透。在 Twitter/X 上实现所有功能的完整闭环，不做半成品：

**执行顺序（按依赖关系）：**

1. **Style Profile** — 从推文历史 + Telegram 聊天 + 代码注释中蒸馏 potato 风格 *(前置条件：所有内容生产的基础)*
2. **Content Ritual 完整状态机** — intake → draft → review → schedule → publish → analyze *(依赖 Style Profile 定义 draft 阶段的风格输入)*
3. **内容生产** — 全部 3 种内容类型（short/mid/long-form） *(依赖 Style Profile + Content Ritual)*
4. **Twitter/X 分发** — 推文、thread、带图推文、定时发布 *(依赖内容生产输出)*
5. **Proactive Intake** — 自动发现高价值内容 + Telegram 通知推送 *(可与 3-4 并行)*
6. **互动管理** — 评论监控、回复建议、主动发现互动机会 *(依赖分发模块已有发布内容)*
7. **数据回收** — 效果追踪、30 天基线对比、pattern 沉淀 *(依赖内容已发布产生数据)*
8. **自我优化** — execution trace 分析、薄弱环节识别、改进建议 *(依赖数据回收积累足够数据)*
9. **Telegram 审批** — inline button 控制全流程 *(可与 1-4 并行开发，步骤 2 的 review phase 需要)*

### 阶段二：多平台扩展

Twitter/X 全功能验证后，扩展到其他平台：

- Hacker News — 技术深度内容
- Reddit — 社区讨论型内容
- 小红书 — 中文受众图文内容
- 各平台风格自动适配
- gepa-core 集成，prompt evolution 自动化

---

**42 GOALs** (12 P0 / 20 P1 / 10 P2) + **7 GUARDs** (3 hard / 4 soft)
