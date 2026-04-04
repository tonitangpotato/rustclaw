# Requirements: Social Media Intake

## Overview

potato 日常刷社交媒体时看到有价值的内容，希望直接转发给 RustClaw，由 RustClaw 自动抓取内容、分析总结、关联已有知识库，并存储为结构化归档。外部内容存入 intake/ 目录（图书馆），只有当产生了新想法或有价值关联时，才将那个想法写入 IDEAS.md（笔记本）和 engram（大脑）。

核心用户问题：收藏夹里攒了大量内容但从不回看，需要一个"转发即归档+分析"的零摩擦流程。

未来扩展方向：从被动 intake（potato 转发）到主动 intake（RustClaw 自动刷各平台、发现有价值内容）。

## Priority Levels

- **P0**: Core — 没有这个功能就不能用
- **P1**: Important — 生产级使用需要
- **P2**: Enhancement — 提升效率和体验

## Guard Severity

- **hard**: 违反 = 系统坏了，必须停止
- **soft**: 违反 = 质量下降，警告但可继续

## Goals

### 1. 内容抓取（Content Extraction）

- **GOAL-1.1** [P0]: 给定一个 URL，能提取出该页面的标题、正文、作者、发布时间、平台名称 *(ref: potato voice 04/03)*
- **GOAL-1.2** [P0]: 支持以下平台的 URL 识别和内容提取：Twitter/X、YouTube、Hacker News、Reddit、小红书（xhslink.com 短链 + xiaohongshu.com）、微信公众号（mp.weixin.qq.com）、GitHub（repos, issues, discussions, README） *(ref: potato voice 04/03, priority upgrade)*
- **GOAL-1.3** [P0]: 对于视频/音频内容（YouTube、Twitter 视频、小红书视频），能提取音频并转录为文字 *(ref: potato voice 04/03)*
- **GOAL-1.4** [P0]: 对于图片内容（截图、小红书图文），能通过 vision model 提取文字和描述内容。小红书的核心内容是图片，没有这个能力等于不支持小红书 *(ref: potato voice 04/03)*
- **GOAL-1.5** [P2]: 支持 Telegram 转发消息的元数据识别（转发来源频道、原始作者） *(ref: IDEA-20260330-01)*

### 2. 内容分析（Analysis）

- **GOAL-2.1** [P0]: 对提取的内容生成结构化摘要：标题、2-3句总结、关键要点列表、分类标签 *(ref: potato voice 04/03)*
- **GOAL-2.2** [P0]: 自动关联已有知识库——搜索 engram 和 IDEAS.md 中的相关 idea 和项目，判断外部内容是否对已有想法有增益 *(ref: potato 04/03 "和自己已有的想法有什么关联，对它有什么增益")*
- **GOAL-2.3** [P1]: 评估内容的潜在价值：与 potato 当前项目/兴趣的相关度 *(ref: capture-idea skill)*
- **GOAL-2.4** [P2]: 识别内容中的可执行信息（工具推荐、技术方案、商业机会等），标记为 actionable *(ref: capture-idea skill)*

### 3. 存储与归档（Storage）

- **GOAL-3.1** [P0]: 保存原始内容的纯文本备份 + 结构化摘要到 `intake/YYYY-MM-DD/{platform}-{slug}.md`（防止源链接失效，社交媒体删帖/限流是常态） *(ref: 数据安全)*
- **GOAL-3.2** [P0]: 在当日 daily log（memory/YYYY-MM-DD.md）记录 intake 事件 *(ref: double-write rule)*
- **GOAL-3.3** [P0]: 维护 `intake/index.md` 作为所有 intake 内容的结构化索引（日期、平台、标题、标签、文件路径） *(ref: 可检索性)*
- **GOAL-3.4** [P0]: 只有当外部内容触发了新想法或与已有 idea 产生有价值的关联时，才将**那个想法/关联**写入 IDEAS.md 和 engram。外部内容本身不进 IDEAS.md 和 engram *(ref: potato 04/03 "外部内容和自己的 idea 性质不同")*

### 4. 用户交互（UX）

- **GOAL-4.1** [P0]: potato 在 Telegram 发送一条包含 URL 的消息，RustClaw 自动触发 intake 流程（无需额外命令） *(ref: potato voice 04/03)*
- **GOAL-4.2** [P0]: 处理完成后，回复一条简洁的摘要消息，包含：标题、摘要、关联发现；如果产生了新想法则一并说明 *(ref: capture-idea pattern)*
- **GOAL-4.3** [P1]: 支持批量 intake——potato 连续发多条 URL，每条独立处理 *(ref: 实际使用场景)*
- **GOAL-4.4** [P1]: 抓取失败时，明确告知原因（反爬被挡、需要登录、格式不支持等），并建议替代方案 *(ref: honesty rules)*

### 5. 主动 Intake（Future Phase）

- **GOAL-5.1** [P2]: 能按配置定期抓取指定 Twitter 账号/HN front page/Reddit subreddit 的新内容 *(ref: potato voice 04/03 "自动帮我去刷帖子")*
- **GOAL-5.2** [P2]: 对主动抓取的内容进行过滤，只保留与 potato 兴趣相关的（基于历史 intake 的标签分布） *(ref: potato voice 04/03)*

## Guards

- **GUARD-1** [hard]: 永远不代替 potato 在任何社交平台上发布、点赞、评论或进行任何写操作 *(ref: SOUL.md "ask first for external actions")*
- **GUARD-2** [hard]: 不存储任何平台的登录凭证在明文文件中 *(ref: security)*
- **GUARD-3** [soft]: 单次 intake 处理时间不超过 60 秒（从用户发送到收到回复） *(ref: UX)*
- **GUARD-4** [soft]: 抓取请求遵守 robots.txt 和平台 rate limit，不暴力爬取 *(ref: 合规)*
- **GUARD-5** [hard]: 抓取失败时不伪造内容，必须明确标记哪些部分是实际提取的、哪些是推断的 *(ref: SOUL.md honesty rules)*
- **GUARD-6** [soft]: 同一 URL 不重复处理——检查 engram 和 IDEAS.md 中是否已存在相同 URL 的记录，如果存在则告知用户并跳过 *(ref: 去重)*

## Out of Scope

- 不做通用爬虫/搜索引擎
- 不处理需要付费订阅才能看的内容（Substack 付费文章等）
- 不做社交媒体账号管理/发帖（那是 xinfluencer 的职责）
- Phase 1 不做主动 intake（GOAL-5.x），先做好被动转发流程

## Dependencies & Tooling Research

### 各平台抓取方案（Research 04/03）

#### Twitter/X
- **推荐工具**: `jawond/bird` (65 ⭐, TypeScript) — "Bird is a CLI for Twitter, so your agents can tweet"
  - 支持读取推文、回复、发推。专门为 AI agent 设计。
  - 用 cookie 认证，不需要 API key。
  - `npx bird read <tweet_url>` 即可读取推文内容。
  - 还有一个 fork: `Oceanswave/bird` (16 ⭐) 基于 v0.8.0 的修复版。
- **备选**: `nirholas/XActions` (173 ⭐) — 完整的 X 自动化工具包，含 scraper + MCP server + CLI + browser scripts。功能太重，但 scraper 部分可参考。
- **备选**: `LXGIC-Studios/xfetch` (4 ⭐, TypeScript) — 轻量 X CLI scraper，cookie + go。
- **视频**: yt-dlp 提取 Twitter 视频音频 → whisper 转文字
- **难度**: 中。bird CLI 是最轻量可用的方案。

#### YouTube
- **推荐工具**: yt-dlp（事实标准，1000+ 平台支持）
  - `yt-dlp --dump-json <url>` 获取完整 metadata（标题、描述、作者、时长等）
  - `yt-dlp --extract-audio --audio-format wav <url>` 提取音频 → whisper 转文字
  - `yt-dlp --write-auto-sub --sub-lang en,zh --skip-download <url>` 直接下载字幕（如果有）
- **难度**: 低。yt-dlp 对 YouTube 支持最完善。

#### Hacker News
- **推荐方案**: HN 官方 API（Firebase）— 零反爬，完全公开
  - `https://hacker-news.firebaseio.com/v0/item/{id}.json` 获取帖子
  - 帖子页 URL 格式: `news.ycombinator.com/item?id=XXX`
  - 也可直接 web_fetch 抓 HTML
- **参考**: `alirezaalavi87/hackernews-scraper-cli` (Rust 实现，1 ⭐) — 可参考 HN HTML 解析逻辑
- **难度**: 极低。

#### Reddit
- **推荐方案**: Reddit JSON API — 任何 Reddit URL 后加 `.json` 即可获取 JSON
  - 例: `https://old.reddit.com/r/rust/comments/xxx.json`
  - 无需认证，公开帖子直接可读
  - 用 `old.reddit.com` 比 `www.reddit.com` 更容易解析
- **参考**: `proxidize/reddit-scraper` (17 ⭐, Python) — dual-mode（simple requests + async proxy rotation）
- **难度**: 低。

#### 小红书 ⚠️ 最难
- **参考项目**: 
  - `xiaofuqing13/redbooks` (Python + DrissionPage) — 完整的小红书爬虫，支持关键词搜索、博主主页、图片视频下载、评论采集。用 DrissionPage（Chromium 自动化）绕过反爬。需要 Cookie 登录。
  - `RedNote/Xiaohongshu-API` — 小红书逆向 API，提供 xsec_token 生成、shield 算法等。商业级但可参考思路。
- **方案**: 
  1. 优先尝试 web_fetch 直接抓取（可能被 block）
  2. Fallback: 用 DrissionPage/Playwright headless browser
  3. 短链 xhslink.com 需要先 follow redirect 拿到真实 URL
  4. 图片内容用 Claude vision 解析
- **难度**: 高。反爬强，短链需要 redirect，内容多为图片。

#### 微信公众号
- **参考项目**:
  - `likemaoke/wechat-article-to-md` — Claude Code Skill，Python 脚本抓取 mp.weixin.qq.com 文章转 Markdown。提取标题、作者、正文、图片。结构清晰可直接借鉴。
  - `vag-Zhao/WeMediaSpider` (11 ⭐) — 带 GUI 的微信公众号爬虫
- **方案**: mp.weixin.qq.com 链接可以直接 web_fetch，文章页面不需要登录。参考 wechat-article-to-md 的 Python 脚本做 HTML→text 转换。
- **难度**: 低-中。公众号文章页面相对友好，但格式解析需要一些 HTML 处理。

#### GitHub
- **推荐方案**: 直接 web_fetch + raw.githubusercontent.com
  - README: `https://raw.githubusercontent.com/{owner}/{repo}/{branch}/README.md`
  - Issue/Discussion: 直接 web_fetch GitHub 页面，RustClaw 内置的 readability 提取器足够
  - GitHub API: `https://api.github.com/repos/{owner}/{repo}` 获取 metadata
- **难度**: 极低。GitHub 完全公开。

### 通用 Fallback
- **Jina Reader API**: `https://r.jina.ai/{url}` — 免费 API，把任何 URL 转成 LLM-friendly 的 markdown 文本。可以作为所有平台的通用 fallback。
- **web_fetch** (RustClaw 内置): 通用网页抓取 + readability 提取

### 多平台工具
- **`sokomishalov/skraper`** (330 ⭐, Kotlin/Java) — 支持 19 个平台（Facebook, Instagram, Twitter, YouTube, TikTok, Telegram, Reddit, Pinterest 等），不需要认证。有 CLI 和 Telegram bot。但是 JVM 依赖太重，不适合我们直接用。可参考它的抓取策略。

### 工具清单（需安装/已有）
| 工具 | 状态 | 用途 |
|------|------|------|
| yt-dlp | ❌ 需安装 | 视频/音频提取（YouTube, Twitter 视频等） |
| bird CLI | ❌ 需安装 (`npx jawond/bird`) | Twitter/X 推文读取 |
| whisper-cli | ✅ 已有 | 音频转文字 |
| web_fetch | ✅ RustClaw 内置 | 通用网页抓取 |
| Claude vision | ✅ 内置 | 图片 OCR 和内容理解 |
| engram | ✅ 已有 | 记忆存储和检索 |
| Jina Reader | ✅ 免费 API | 通用 URL → markdown fallback |
| DrissionPage | ❌ 需安装 (pip) | 小红书反爬绕过（如需要） |
## Implementation Form

这是一个 **RustClaw Skill**（`skills/social-intake/SKILL.md`），不是独立项目。原因：
1. 核心逻辑是 prompt 指导 + 工具调用编排，不需要写 Rust 代码
2. 复用现有 capture-idea skill 的存储模式
3. 通过 trigger patterns（URL regex）自动激活

对于小红书等需要复杂抓取逻辑的平台，可能需要一个辅助 Python 脚本（参考 wechat-article-to-md 的模式），放在 `skills/social-intake/scripts/` 下。

---

**14 GOALs** (10 P0 / 1 P1 / 3 P2) + **6 GUARDs** (3 hard / 3 soft)
