# IDEAS.md - Idea Repository

> All ideas captured by RustClaw's Idea Intake pipeline.
> Format: newest first. Each idea has a unique ID for cross-referencing.

---

<!-- New ideas are prepended below this line -->

## IDEA-20260403-01: 自动化 Skill 优化系统
- **Date**: 2026-04-03
- **Source**: Text (potato)
- **Category**: tech/product
- **Tags**: #skills #automation #optimization #self-improvement #meta-learning #rustclaw
- **Effort**: Medium

### Summary
让 RustClaw 的 skill 系统能自动优化自身。现有 skills 是手写的 SKILL.md，`src/skills.rs` 有 auto-generation 能力但未充分利用。核心思路：agent 在使用 skill 时收集效果数据（成功率、token 消耗、用户满意度），自动识别哪些 skill 效果差 → 改写/调参 → A/B test → 保留更好的版本。

### Key Points
- **效果追踪** — 每次 skill 触发后，记录：是否成功完成、token 消耗、用户反馈（explicit: 好评/差评，implicit: 是否被要求重做）
- **自动识别弱 skill** — 成功率低、token 消耗高、频繁被重做的 skill 标记为需优化
- **自动改写** — LLM 分析失败 case，生成改进版 SKILL.md（更清晰的指令、更好的步骤拆分、补充遗漏场景）
- **版本管理** — skill 有版本历史，可以 rollback 到之前版本
- **Trigger 优化** — 分析 false positive（触发了但不该触发）和 false negative（该触发但没触发），自动调整 trigger patterns/keywords
- **新 skill 生成** — 识别重复出现的工作模式（如"每次都要先搜 engram 再查文件"），自动提炼为新 skill
- **与 `src/skills.rs` 现有能力整合** — SkillGenerator 已有 complexity 评估和 auto-gen 逻辑，但缺乏闭环优化

### Potential Value
- **降低维护成本** — skill 不再需要手动调优，agent 自己学会什么 work 什么不 work
- **持续改进** — 随着使用数据积累，skill 质量单调递增
- **可复制** — 优化后的 skill 可以 export 给其他 RustClaw 实例
- **Meta-learning** — 这本身就是一个 "学会学习" 的系统，对 agent 生态有示范意义

### Connections
- 现有 `src/skills.rs` SkillGenerator — 已有 auto-gen 框架，缺闭环
- Engram behavioral stats (`engram_behavior_stats`) — 可作为效果数据源
- Engram soul suggestions (`engram_soul_suggestions`) — 类似理念，但针对 SOUL.md 而非 skills
- SKM (Skill Engine) — trigger matching 层，优化 trigger 需要与 skm 协作

### Status: 💡 New
---

## IDEA-20260402-03: Engineer Union 平台 — Layoff Tracker + 业务替代
- **Date**: 2026-03-31 (初次讨论) → 2026-04-02 (正式录入)
- **Source**: Voice message (3/31), 起因是 Block 等公司大规模裁员工程师
- **Category**: product/business/community
- **Tags**: #engineer #union #layoff #tracker #prediction #open-source #disruption #community
- **Effort**: High

### Summary
一个面向软件工程师/AI工程师的"工会式"平台。核心逻辑：那些声称"AI 可以替代工程师"的公司，反过来说明他们的业务也不需要大公司才能做——小团队 + AI + 开源完全可以替代。平台帮工程师组队，用更小的成本重现这些公司的核心服务。

### Key Points
- **Layoff Tracker** — 实时追踪哪些 tech 公司裁员了（数据源：WARN 通知、新闻、LinkedIn 信号）
- **Layoff Predictor** — 预测哪些公司即将裁员（信号：招聘冻结、财报、管理层变动、行业趋势）
- **业务分析引擎** — 分析被裁公司的核心业务，拆解哪些服务可以被更小团队/开源方案替代
- **组队平台** — 被裁/想创业的工程师在这里组队，认领要替代的业务方向
- **核心叙事** — "你说 AI 能替代我们？好，那我们用 AI 替代你"

### Potential Value
- **社区价值** — 工程师群体的共鸣极强，特别是在 layoff 潮中，自带传播力
- **商业模式** — 平台抽成（组队成功的项目）、付费 Predictor 数据、企业级 threat intelligence
- **复合效应** — 每一个成功替代案例都是最好的 marketing
- **防御性** — 社区 + 数据 + 网络效应 = 护城河

### Connections
- **Marketing Pipeline (IDEA-20260402-02)** — 做出来后需要自动宣传
- **AgentVerse** — 可以作为 AgentVerse 上的一个垂直社区
- **potato 的核心诉求** — 财务自由，这个项目如果成功，影响力 + 收入双收

### Status: 💡 New
---

## IDEA-20260402-02: Marketing Automation Pipeline（全流程自动化营销）
- **Date**: 2026-04-02 10:46 ET
- **Source**: Voice message
- **Category**: business/automation
- **Tags**: #marketing #automation #social-media #pipeline #growth #visibility
- **Effort**: High

### Summary
将整个 marketing 过程流程化、自动化。目前已有 **xinfluencer**（X/Twitter influencer 发现 + 互动引擎）和 **usergrow**（用户增长分析，geo + causal inference + brand DNA），这两个工具应该作为整个自动化流水线的组成部分。但完整的 marketing pipeline 还缺很多环节需要补齐。

### Key Points
- 已有工具需要整合进统一流水线：
  - **xinfluencer** (`/Users/potato/clawd/projects/xinfluencer/`) — X/Twitter 影响力发现、Monitor、Engage、CRM（Rust, v0.1 discover 已实现，v0.2 monitor/engage/CRM 有 DESIGN 待实现）
  - **usergrow** (`/Users/potato/.openclaw/workspace-hackathon/usergrow/`) — 用户增长分析、brand DNA、causal inference、persona 生成、keyword graph（Rust, prototype）
- **核心功能模块（从语音补充）：**
  1. **社交媒体发帖** — 原创内容生成 + 多平台发布（X、HN、小红书、Reddit、ProductHunt、LinkedIn...）
  2. **互动引擎** — 在别人帖子下 comment、reply，strategic engagement
  3. **语言优化** — 让内容不被看出是 AI 生成（tone matching、平台风格适配、人味儿）
  4. **高价值关系维护** — CRM 跟踪互动历史、reciprocity score、自动维护重要关系（xinfluencer CRM 设计已有，需实现）
  5. **多渠道探索** — 持续发现新的 marketing 渠道（不只是已知平台）
  6. **数据回收** — engagement metrics → 反馈到策略调整
  7. **产品发布宣传** — 新产品/版本 → 自动触发宣传流程
- **已有设计可复用：** xinfluencer DESIGN.md 已设计 Engagement Autopilot（strategic reply、follow active commenters）和 Relationship CRM（interaction frequency、reciprocity score、value score），这些可以直接作为 pipeline 的 engage + CRM 模块
- 这和 3/31 讨论的"自动化流水线"一脉相承：idea intake → research → design → implement → test → **market** → iterate

### Potential Value
**直接商业价值**。marketing 是 potato 产品变现的瓶颈之一——东西做出来了但缺宣传。自动化这个环节可以：
1. 让每个新产品/开源项目自动获得 visibility
2. 持续建立 potato 的个人品牌（开发者影响力）
3. 复合效应——影响力越大，后续产品推广成本越低

### Connections
- **IDEA-20260330-01**: Social media post intake（社交媒体帖子自动抓取分析）
- **xinfluencer**: 已有 discover 功能，monitor/engage 是 marketing pipeline 的核心
- **usergrow**: brand DNA + persona 分析可以指导内容策略
- **Engineer Union / Layoff Predictor** (3/31 讨论): 如果做成产品，也需要这个 marketing pipeline 来推广
- **AgentVerse**: 同理，做出来后需要自动宣传
- **potato 的核心诉求** (3/31): "我真的需要去把很多环节都自动化"

### Next Steps
1. 梳理完整 pipeline 各环节（content → publish → engage → measure → optimize）
2. 盘点现有工具能力 vs 缺口
3. 设计统一的 orchestration 层（可能是 RustClaw skill 或独立 CLI）
4. 写 requirements.md

### Status: 💡 New
---

## IDEA-20260402-01: Engram Memory Benchmark (Cognitive-First)
- **Date**: 2026-04-02 10:12 ET
- **Source**: Voice message
- **Category**: tooling/research
- **Tags**: #engram #benchmark #memory #cognitive #evaluation #open-source
- **Effort**: Medium

### Summary
为 Engram 设计并实现一套自己的 memory benchmark。因为 Engram 侧重 cognitive science（ACT-R 衰减、Hebbian 关联、情感记忆），和市面上纯 RAG 向的记忆系统（Hindsight、Mem0、Zep）评测维度不同，现有 benchmark（如 LongMemEval）无法评估 Engram 的核心优势。

### Key Points
- **为什么需要自建 bench** — LongMemEval 等现有 benchmark 侧重 "能不能找到正确信息"（retrieval accuracy），但 Engram 的核心差异在 cognitive dynamics：记忆衰减是否符合人类遗忘曲线、关联强化是否 work、情感权重是否影响 recall 优先级
- **应评测的维度**（初步）：
  - **Decay fidelity** — 记忆随时间衰减的曲线是否符合 ACT-R 幂律
  - **Hebbian strengthening** — 共现记忆是否正确关联 & 互相增强
  - **Emotional weighting** — 高情感记忆是否优先被 recall
  - **Consolidation quality** — working→core 迁移是否保留重要信息
  - **Cross-language recall** — 中英混合存储的检索准确性
  - **Retrieval precision/recall** — 传统指标，和竞品对比的基准线
  - **Latency** — 不同数据规模下的查询速度（Engram 的 90ms 优势）
- **可以做成开源 benchmark** — 让其他 cognitive memory 系统也能跑，建立新赛道的评测标准
- **和竞品对比** — 跑同样的 benchmark 对比 Engram vs Hindsight/Mem0/Zep，在 cognitive 维度上展示优势

### Potential Value
- **学术/开源影响力** — 定义新赛道的 benchmark = 定义赛道规则
- **产品营销** — "我们不只是 recall 准，我们的记忆像人脑一样工作"
- **开发指导** — 量化知道 Engram 哪里强哪里弱，指导 v3 改进方向
- **crates.io 发布** — 可以作为独立 crate（`engram-bench`）

### Connections
- 直接关联 `engramai` v3 升级计划（MEMORY-SYSTEM-RESEARCH.md）
- Hindsight 用 LongMemEval 跑出 91.4%，我们需要自己的维度来讲故事
- 和 Engram 竞品调研（2026-04-02）互相 inform

### Status: 💡 New
---

## IDEA-20260330-04: AI 智能记账
- **Date**: 2026-03-30 00:39 ET
- **Source**: Voice message
- **Category**: product/business
- **Tags**: #ai #fintech #banking #plaid #expense-tracking #revenue
- **Effort**: Medium

### Summary
AI 自动记账工具，接入银行/金融数据聚合 API（如 Plaid、Yodlee、MX、Teller 等），自动分类交易、生成报表、智能分析消费习惯。

### Key Points
- **核心功能** — 连接银行账户，自动拉取交易记录，AI 分类和分析
- **技术方案** — 已有成熟商用 API：
  - **Plaid** — 最主流，连接 12,000+ 金融机构，Transaction API
  - **Teller** — 更轻量，直接银行连接（不走屏幕抓取）
  - **MX** — 数据增强 + 分类
  - **Yodlee** / **Finicity (Mastercard)** — 企业级
- **AI 加持** — 自动分类（比传统规则引擎更准）、消费洞察、预算建议、异常检测
- **差异化** — 自然语言查询（"上个月在外面吃饭花了多少？"）

### Potential Value
- 个人财务管理是刚需市场
- 订阅制 SaaS（$5-15/月）
- 可以做 B2C 也可以做 B2B（给小企业用）
- 数据聚合后可以扩展：税务、投资分析、财务规划

### Status: 💡 New

---

## IDEA-20260330-01: 社交媒体帖子 Intake 处理
- **Date**: 2026-03-30 00:37 ET
- **Source**: Voice message
- **Category**: tooling
- **Tags**: #rustclaw #skills #social-media #小红书 #twitter #intake
- **Effort**: Low

### Summary
增强 Idea Intake Pipeline，专门处理社交媒体平台的帖子转发。potato 会直接把看到的帖子转发过来（小红书、Twitter/X 等），需要针对每个平台的格式做内容提取。

### Key Points
- **小红书** — 分享链接是 `xhslink.com` 短链或 app 分享文本（标题+链接），反爬严重需要特殊处理
- **Twitter/X** — `x.com`/`twitter.com` 链接，可用 nitter 或 yt-dlp 提取
- **Telegram 转发** — 可能只有文本+图片没有 URL，需要从消息元数据识别
- 每个平台需要不同的 content extraction 策略

### Potential Value
- 大幅降低 idea capture 的摩擦 — 看到就转发，不需要额外操作
- 建立个人知识库/灵感库

### Status: 💡 New

---

## IDEA-20260330-02: AI 有声书 + 角色对话平台
- **Date**: 2026-03-25 (初次讨论) → 2026-03-30 (重新提起)
- **Source**: 3/25 clawd 讨论 + 3/30 voice message
- **Category**: product/business
- **Tags**: #ai #tts #audiobook #character #voice #platform #revenue
- **Effort**: High
- **Existing docs**: `~/clawd/projects/ai-audiobook-platform/竞品分析与市场定位.md`

### Summary
一体化 AI 有声书平台：TTS 工具 + 作者友好分发（10-15% 抽成 vs Audible 60%）+ AI 交互体验（角色对话、What-if 探索）。

### Key Points (from 3/25 discussion)
- **三大核心能力**：低成本 AI 有声书生成、作者友好分发（85-90% 作者分成）、AI 增强交互体验
- **角色对话** — 基于 RAG 知识库，不是通用 LLM 幻觉，提供原文引用
- **竞品空白** — 工具（ElevenLabs）不做交互；角色聊天（Character.AI）不做有声书；Audible 两者都差且抽成高
- **MVP 方向**：经济学垂直领域
  - 3-5 位历史经济学家角色（亚当·斯密、凯恩斯、芒格）
  - 公共领域经典有声书（《国富论》、《通论》等）
  - "时事分析"功能 — 用户描述市场事件，获取不同经济学家多角度分析
- **技术栈** — 自托管开源 TTS（Fish Audio/Kokoro 等，边际成本≈0）+ LLM + RAG
- **三阶段路线**：MVP(1-3月) → 平台化(4-8月) → 规模化(9-12月)

### Competitive Analysis (已完成)
- Audible: 垄断但不创新，60%抽成与作者利益冲突
- ElevenLabs: TTS 顶尖但是工具公司，不做交互体验
- Speechify: 纯工具无分发
- Character.AI: 角色聊天但无知识根基，面临监管风险
- Hello History: 验证了历史人物互动需求但浅层
- Amazon "Ask this Book": 基础问答，作者无法退出，争议大

### Potential Value
- 有声书全球市场 $7B+，AI 降低 95% 制作成本但 Audible 定价锚定旧成本结构
- 作者 85-90% 分成是杀手级卖点
- 角色互动 + 知识根基是竞品无法轻易复制的护城河

### Status: 💡 有竞品分析，待 MVP 开发

---

## IDEA-20260330-03: AI 语音帮约医生
- **Date**: 2026-03-30 00:38 ET
- **Source**: Voice message
- **Category**: product/business
- **Tags**: #ai #voice #healthcare #automation #revenue
- **Effort**: Medium

### Summary
用 AI 语音代打电话帮用户预约医生。解决打电话等待、沟通繁琐的痛点。

### Key Points
- **核心功能** — AI 代替用户打电话给诊所，完成预约流程
- **技术需求** — 实时语音对话 AI（类似 Bland.ai / Retell.ai）、电话 API（Twilio）
- **痛点明确** — 在美国约医生打电话经常等 20+ 分钟，流程繁琐
- **竞品参考** — OpenAI 演示过类似场景，但没有专门产品化

### Potential Value
- 痛点真实且普遍（尤其美国医疗系统）
- 可以扩展到所有"代打电话"场景：餐厅预约、政府部门、保险公司等
- SaaS 订阅或按次收费

### Status: 💡 New

---

## IDEA-20260329-01: Skills 动态加载管理小工具
- **Date**: 2026-03-29 22:38 ET
- **Source**: Voice conversation during skill trigger system design
- **Category**: tooling
- **Tags**: #skills #cli #developer-tools #rustclaw
- **Effort**: Low

### Summary
一个 CLI 工具用于管理 RustClaw 的 skills 系统 — 列出、启用/禁用、测试触发条件、查看统计、生成 skill 模板等。类似 `rustclaw skills list/enable/disable/test/stats/generate` 的命令集。

### Key Points
- **动态管理**：无需手动编辑 YAML/frontmatter，用 CLI 控制
- **触发测试**：`rustclaw skills test <skill-name> "test message"` → 显示是否会触发
- **统计分析**：哪些 skills 最常用、哪些从未触发、平均触发频率
- **模板生成**：`rustclaw skills generate <name>` → 自动生成带 frontmatter 的 SKILL.md 模板
- **启用/禁用**：`always_load` toggle，不删除文件
- **依赖检查**：某个 skill 依赖的 tools 是否都存在

### Potential Value
- **开发体验提升** — 不再手动编辑 markdown + frontmatter，降低出错
- **调试效率** — 快速测试 trigger 逻辑是否符合预期
- **可观测性** — 统计数据帮助优化 skills（哪些太泛滥、哪些太窄）
- **Onboarding** — 新用户可以用 `generate` 快速创建自己的 skills

### Connections
- 依赖 **Skill Trigger System (方案 2)** 的实现（frontmatter + matching 逻辑）
- 类似 `cargo` 的子命令风格 — RustClaw 本身就是 CLI，扩展性好
- 可以和 **GID** 结合 — skills 管理工具可以读 GID graph，提示"你有这些任务，要不要生成对应的 skill？"

### Implementation Notes
```rust
// src/cli/skills.rs
pub struct SkillsCli {
    skills_dir: PathBuf,
}

impl SkillsCli {
    pub fn list(&self) -> Result<Vec<SkillMeta>>;
    pub fn enable(&self, name: &str) -> Result<()>;
    pub fn disable(&self, name: &str) -> Result<()>;
    pub fn test(&self, name: &str, message: &str) -> Result<bool>;
    pub fn stats(&self) -> Result<SkillsStats>;
    pub fn generate(&self, name: &str, description: &str) -> Result<PathBuf>;
    pub fn validate(&self, name: &str) -> Result<ValidationResult>;
}
```

Example usage:
```bash
$ rustclaw skills list
📦 Active Skills (5):
  ✓ idea-intake (priority: 8) — Process URLs, voice messages, ideas
  ✓ polymarket-analysis (priority: 6) — Analyze Polymarket markets
  ✗ debug-logger (disabled) — Auto-log debug info

$ rustclaw skills test idea-intake "Check out https://example.com"
✅ Skill would trigger (matched: "https://")

$ rustclaw skills generate market-research "Research crypto market trends"
✨ Created skills/market-research/SKILL.md
```

### Status: ✅ Done — 已实现为 [skm](https://crates.io/crates/skm) v0.1 (Agent Skill Engine)，RustClaw 已集成

---

