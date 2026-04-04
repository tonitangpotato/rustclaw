# Requirements: Marketing Automation Pipeline

## Overview

一套端到端的内容营销自动化管道，覆盖从内容发现到效果回收的完整闭环。核心用户问题：potato 有大量 idea 和技术积累，但缺少系统化的内容生产和分发能力，无法将知识转化为影响力和收入。

管道定义：**Intake → 内容生产 → 分发 → 互动管理 → 数据回收 → 自我优化**

设计原则：
- **Content Ritual 同构** — 和代码 ritual（design → graph → implement → verify）同构，复用 ritual 基础设施（phase 状态机、Telegram 通知、GID task、specialist 分工）
- **自举模式** — 用这套管道来 marketing 管道本身（先验证自己能产出有价值的内容，再把工具开放给别人）
- **去 AI 感** — 所有自动生产的内容必须看不出是 AI 写的（potato 明确要求）
- **人在环中** — 最终发布需要 potato 过目确认，AI 负责 80% 的工作量

已有组件：
- ✅ Social Intake（skills/social-intake，7 平台 extractor，已实测可用）
- ✅ xinfluencer（Rust，6,462 行，discover 模块已实现）
- ✅ usergrow（brand DNA + persona，prototype）
- ✅ Engram 认知记忆 + GID 任务图
- ✅ Ritual 基础设施（phase 状态机、通知）

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
- **GOAL-1.3** [P0]: 支持至少 3 种内容类型：short-form（推文/帖子，< 280 字）、mid-form（thread/小红书图文，500-2000 字）、long-form（博客/公众号，2000+ 字） *(ref: 多平台需求)*
- **GOAL-1.4** [P1]: 支持从已有 intake 内容衍生——不是复制粘贴外部内容，而是基于外部内容生成 potato 自己的见解/评论/扩展 *(ref: "Build in public" + @zarazhangrui 策略)*
- **GOAL-1.5** [P1]: 支持内容系列化——基于一个大 topic 自动拆分成多篇关联内容（thread 系列、系列帖子等） *(ref: 持续输出策略)*
- **GOAL-1.6** [P2]: 根据历史发布效果数据，推荐最佳发布时间和内容类型 *(ref: 数据驱动优化)*

### 2. Style Profile（写作风格画像）

所有内容生产的核心输入。没有 Style Profile，就只能产出通用 AI 内容。

- **GOAL-2.1** [P0]: 从 potato 的历史内容中提取写作风格特征：常用词汇、句式偏好、语气、幽默方式、中英混用习惯、平台差异 *(ref: "去 AI 感" 04/03)*
- **GOAL-2.2** [P0]: Style Profile 作为结构化文档存储（如 `profiles/potato-style.md`），包含：正面特征（potato 的风格）+ 负面特征（LLM 典型 pattern 要避免的） *(ref: 04/03 方案)*
- **GOAL-2.3** [P1]: 支持平台差异化风格——Twitter 上的 potato 和公众号上的 potato 语气不同 *(ref: 多平台策略)*
- **GOAL-2.4** [P1]: Style Profile 随时间演进——新的内容发布后自动更新，捕捉风格变化 *(ref: 持续优化)*
- **GOAL-2.5** [P2]: 提供"反 AI 检测"自检——生成内容后用另一个 LLM 判断是否像 AI 写的，如果像则自动 rewrite *(ref: "去 AI 感" 04/03)*

### 3. 内容分发（Distribution）

将审核通过的内容发布到目标平台。

- **GOAL-3.1** [P0]: 支持 Twitter/X 发布（推文、thread、带图推文）——通过 xinfluencer 的 publish 模块 *(ref: xinfluencer 已有 engage 模块基础)*
- **GOAL-3.2** [P1]: 支持至少 3 个平台的分发：Twitter/X（P0）、Hacker News（P1）、Reddit（P1） *(ref: potato 的核心受众平台)*
- **GOAL-3.3** [P1]: 同一内容自动适配不同平台格式——Twitter 版简短有力，Reddit 版详细有深度，HN 版技术聚焦 *(ref: 多平台策略)*
- **GOAL-3.4** [P1]: 支持定时发布——potato 审核通过后设置发布时间，系统自动发 *(ref: UX 优化)*
- **GOAL-3.5** [P2]: 小红书发布支持（图文生成 + 发布） *(ref: 中文受众覆盖)*
- **GOAL-3.6** [P2]: LinkedIn / 个人博客发布支持 *(ref: 长尾覆盖)*

### 4. 互动管理（Engagement）

发布后的互动不能丢，要闭环。

- **GOAL-4.1** [P1]: 监控已发布内容的评论/回复，通过 Telegram 通知 potato *(ref: xinfluencer monitor 模块)*
- **GOAL-4.2** [P1]: 对评论/回复生成回复建议（非自动发送），potato 一键确认或修改后发送 *(ref: "Treat X like a party" 策略)*
- **GOAL-4.3** [P2]: 识别高价值互动者（同行业、高粉丝、反复互动的用户），标记为 CRM 联系人 *(ref: xinfluencer CRM 模块)*
- **GOAL-4.4** [P2]: 主动发现与 potato 兴趣相关的他人帖子，推荐 potato 去互动（而非只发自己的） *(ref: @zarazhangrui "Reply authentically" 策略)*

### 5. 数据回收与效果分析（Analytics）

没有反馈就没有优化。

- **GOAL-5.1** [P1]: 对每条发布内容追踪效果数据：impressions、likes、replies、retweets、link clicks（各平台可用的指标） *(ref: 数据驱动优化)*
- **GOAL-5.2** [P1]: 生成周报/月报摘要——哪些内容表现好、哪些差、为什么、下一步建议 *(ref: 闭环优化)*
- **GOAL-5.3** [P2]: 将效果数据与内容特征关联分析——什么 topic/格式/时间点效果最好，沉淀为可复用的 pattern *(ref: 自我优化闭环)*
- **GOAL-5.4** [P2]: 效果数据反馈到 Style Profile——根据受众反应微调写作策略 *(ref: 自我优化)*

### 6. Pipeline 编排与 Ritual（Orchestration）

管道本身需要编排，不是一堆散装 skill。

- **GOAL-6.1** [P0]: 实现 Content Ritual 状态机：intake → draft → review → schedule → publish → analyze，每个 phase 有明确的输入/输出和转换条件 *(ref: Content Ritual 同构讨论 04/03)*
- **GOAL-6.2** [P0]: Telegram 通知 + inline button 控制——potato 在手机上就能 approve/reject/edit 内容草稿 *(ref: 人在环中原则)*
- **GOAL-6.3** [P1]: 支持 batch mode——一次启动多条内容的生产流程，并行处理 *(ref: 规模化运营)*
- **GOAL-6.4** [P1]: 每个 phase 产生结构化 trace（耗时、token 消耗、成功/失败、修改量），为自我优化提供数据 *(ref: gid-harness execution-log 模式, Meta-Harness 论文 "完整历史 > 压缩摘要")*
- **GOAL-6.5** [P2]: Pipeline 配置化——不同内容类型可以跳过某些 phase（如纯转发不需要 draft） *(ref: 灵活性)*

### 7. 自我优化（Self-Improvement）

这套管道跑得越多应该越好，不是静态的。

- **GOAL-7.1** [P1]: 基于 execution trace 自动识别管道中的薄弱环节（哪个 phase 耗时最长、哪个 phase 被 potato reject 最多） *(ref: IDEA-20260403-01 Skill 优化系统)*
- **GOAL-7.2** [P1]: 对表现差的 skill/prompt 生成改进建议（非自动修改），potato 审批后应用 *(ref: GEPA 方法论, Hermes Agent self-evolution)*
- **GOAL-7.3** [P2]: 集成 GEPA 或等价的 prompt evolution 机制——自动生成 skill 变体 → 评估 → 择优部署 *(ref: Hermes Agent DSPy+GEPA, ICLR 2026 Oral)*
- **GOAL-7.4** [P2]: LLM-as-judge 评分体系——对每篇内容草稿打分（风格匹配度、可读性、信息密度、去 AI 感程度），作为自动 eval 的 proxy *(ref: Hermes Agent 评分方法)*

## Guards

- **GUARD-1** [hard]: 所有自动生产的内容，在发布前必须经过 potato 确认（至少看一眼）。系统可以草拟、可以排队、但不能自动发布。唯一例外：potato 明确设置了"自动发布"规则的特定内容类型 *(ref: SOUL.md "ask first for external actions")*
- **GUARD-2** [hard]: 生成的内容不得包含虚假信息、未经验证的声明、或抄袭他人原创内容。引用外部内容必须标注来源 *(ref: honesty rules)*
- **GUARD-3** [hard]: 社交平台的 API credentials / cookies 加密存储，不出现在日志、engram、或任何明文文件中 *(ref: security)*
- **GUARD-4** [soft]: 每周发布频率上限 potato 可配置（默认：Twitter ≤ 21 条/周，其他平台 ≤ 5 条/周），防止刷屏引起反感 *(ref: @zarazhangrui "consistency > volume")*
- **GUARD-5** [soft]: 内容生成的 AI 痕迹检测——如果自检发现内容像 AI 写的（典型 pattern: "delve into", "it's worth noting", 过度使用 em dash），自动触发 rewrite *(ref: "去 AI 感" 要求)*
- **GUARD-6** [soft]: 遵守各平台 rate limit 和 ToS，不用 bot 行为引起封号风险 *(ref: 合规)*
- **GUARD-7** [soft]: 每条内容的生成成本（LLM token）记录在 trace 中，单条内容成本不超过 $0.50（soft limit，可调） *(ref: 成本控制)*

## Out of Scope

- 不做 SEO 优化工具（专业 SEO 不在 MVP 范围内）
- 不做付费广告投放管理（Google Ads、Twitter Ads 等）
- 不做多用户 SaaS（MVP 只为 potato 服务，产品化是 Phase 2）
- 不做视频内容生产（短视频/长视频制作不在当前范围，文字+图片优先）
- 不做竞品监控/行业报告（那是 research 工具的职责）

## Dependencies

- **Social Intake**（skills/social-intake）— 管道的入口，已实现
- **xinfluencer**（Rust crate）— Twitter/X 分发 + 互动监控，discover 已实现，需补 engage/publish/monitor
- **usergrow**（Python）— brand DNA + persona 分析，prototype，为 Style Profile 提供理论基础
- **gid-harness**（Rust crate）— Content Ritual 的状态机引擎，已完整实现
- **Engram**（engramai）— 记忆存储 + 知识关联
- **Telegram inline buttons** — potato 审批内容的主要交互方式，需在 RustClaw 中实现

## Implementation Strategy

### Phase 1: 最小闭环（MVP）
- Style Profile v1（手动 + LLM 分析 potato 历史内容）
- Content Ritual 基础状态机（draft → review → publish 三步）
- Twitter/X 单平台发布
- potato Telegram 审批（文字回复，不需要 inline buttons）
- 每条内容的基础 trace

### Phase 2: 多平台 + 数据闭环
- 多平台分发（HN、Reddit）
- 效果数据回收 + 周报
- Telegram inline buttons 审批
- Style Profile 自动演进
- batch mode

### Phase 3: 自我优化
- GEPA/prompt evolution 集成
- LLM-as-judge 自动评分
- 薄弱环节自动识别 + 改进建议
- 完整 trace → 优化闭环

---

**27 GOALs** (8 P0 / 11 P1 / 8 P2) + **7 GUARDs** (3 hard / 4 soft)
