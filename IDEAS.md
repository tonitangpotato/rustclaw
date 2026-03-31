# IDEAS.md - Idea Repository

> All ideas captured by RustClaw's Idea Intake pipeline.
> Format: newest first. Each idea has a unique ID for cross-referencing.

---

<!-- New ideas are prepended below this line -->

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

