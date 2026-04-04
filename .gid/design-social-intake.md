# Design: Social Media Intake Skill

## 1. Overview

Social Media Intake 是一个 RustClaw Skill，当 potato 在 Telegram 发送社交媒体 URL 时自动触发。它根据 URL 所属平台选择对应的抓取策略，提取内容后进行分析、关联、存储。

**核心设计决策：**
- **Skill 不是代码** — 它是一组 prompt 指令 + 工具调用编排，运行在 RustClaw agent 的 LLM context 里
- **平台路由通过 URL pattern matching** — skill 内部做，不需要 Rust 代码
- **复杂抓取逻辑走辅助脚本** — 小红书等需要 headless browser 的平台，用 Python 脚本（通过 `exec` 调用）
- **与 capture-idea skill 的关系** — social-intake 的 trigger priority 高于 capture-idea（80 > 50），当 URL 匹配社交平台时走 social-intake；不匹配时 fallback 到 capture-idea 的通用逻辑
- **存储哲学** — intake/ 是图书馆（存外部内容），IDEAS.md 是笔记本（只存自己的想法），engram 是大脑（只存有认知价值的关联）。外部内容本身不进 IDEAS.md 和 engram

**Trade-offs：**
- 选 Skill 而非独立 crate：开发快、迭代快，但性能和错误处理不如编译时代码。对于个人 intake 工具这是正确的 trade-off。
- 选 Python 辅助脚本而非纯 LLM tool calls：小红书/微信的 HTML 解析在 prompt 里做太脆弱，Python 脚本更可靠且可独立测试。
- 选 Jina Reader 作为通用 fallback 而非自己写 readability：节省开发时间，免费 API 够用。

**Satisfies:** GOAL-1.1, GOAL-1.2, GOAL-4.1

## 2. Architecture

```
┌─────────────────────────────────────────────┐
│                  RustClaw Agent              │
│                                             │
│  ┌─────────────┐    ┌────────────────────┐  │
│  │ capture-idea │    │  social-intake     │  │
│  │ (priority 50)│    │  (priority 80)     │  │
│  └─────────────┘    └────────┬───────────┘  │
│                              │              │
│           ┌──────────────────┼──────────┐   │
│           ▼                  ▼          ▼   │
│   ┌──────────────┐  ┌────────────┐  ┌─────┐│
│   │Platform Router│  │  Analyzer  │  │Store││
│   └──────┬───────┘  └────────────┘  └─────┘│
│          │                                  │
│   ┌──────┴───────────────────────────┐      │
│   │        Extraction Layer          │      │
│   ├──────┬──────┬─────┬─────┬───────┤      │
│   │web_  │yt-dlp│bird │xhs  │jina   │      │
│   │fetch │      │ CLI │.py  │reader │      │
│   └──────┴──────┴─────┴─────┴───────┘      │
└─────────────────────────────────────────────┘
```

SKILL.md 里包含完整的路由逻辑和每个平台的抓取指令。Agent 读取 SKILL.md 后按照指令调用对应工具。

## 3. Components

### 3.1 URL Router (Skill 内 prompt 逻辑)

**Responsibility:** 识别 URL 所属平台，分发到对应抓取策略。

**路由表:**
```
twitter.com, x.com, t.co           → Twitter 策略
youtube.com, youtu.be               → YouTube 策略
news.ycombinator.com                → HN 策略
reddit.com, old.reddit.com          → Reddit 策略
xhslink.com, xiaohongshu.com        → 小红书策略
mp.weixin.qq.com                    → 微信公众号策略
github.com, raw.githubusercontent.com → GitHub 策略
*（其他）                             → Jina Reader fallback
```

**短链处理:** xhslink.com 和 t.co 需要 follow redirect。用 `exec: curl -Ls -o /dev/null -w '%{url_effective}' "{url}"` 获取真实 URL。

**Satisfies:** GOAL-1.2

### 3.2 Platform Extractors

每个平台有自己的抓取策略，全部在 SKILL.md 里定义为 prompt 指令。

#### 3.2.1 Twitter/X Extractor

```bash
# 读取推文文字内容
npx bird read "{url}"

# 如果包含视频
yt-dlp --extract-audio --audio-format wav -o /tmp/intake-audio.wav "{url}"
stt("/tmp/intake-audio.wav")
rm /tmp/intake-audio.wav
```

**Fallback 链:** bird CLI → web_fetch → Jina Reader
**Satisfies:** GOAL-1.1, GOAL-1.3

#### 3.2.2 YouTube Extractor

```bash
# 获取 metadata (title, description, author, duration, upload_date)
yt-dlp --dump-json "{url}" | jq '{title, description, uploader, duration, upload_date}'

# 优先下载字幕（如果有）
yt-dlp --write-auto-sub --sub-lang en,zh --skip-download -o /tmp/intake-sub "{url}"
# 如果字幕存在，读取字幕文件
cat /tmp/intake-sub.*.vtt

# 如果没有字幕，提取音频转文字
yt-dlp --extract-audio --audio-format wav -o /tmp/intake-audio.wav "{url}"
stt("/tmp/intake-audio.wav")
rm /tmp/intake-audio.wav
```

**注意:** 长视频（>30min）的音频文件很大。如果 duration > 1800s，只提取前 30 分钟或仅用 metadata + description。
**Satisfies:** GOAL-1.1, GOAL-1.3

#### 3.2.3 Hacker News Extractor

```bash
# 从 URL 提取 item ID
# URL 格式: news.ycombinator.com/item?id=12345

# HN API 获取帖子数据
web_fetch("https://hacker-news.firebaseio.com/v0/item/{id}.json")
# 返回: {title, url, text, by, time, score, descendants}

# 如果帖子有外链 (url 字段)，也抓取外链内容
web_fetch("{item.url}")
```

**Satisfies:** GOAL-1.1

#### 3.2.4 Reddit Extractor

```bash
# Reddit JSON API — URL 后加 .json
web_fetch("https://old.reddit.com/r/{sub}/comments/{id}.json")
# 返回: [post_data, comments_data]
# post_data[0].data.children[0].data: {title, selftext, author, score, url, created_utc}

# 如果帖子包含外链
web_fetch("{post.url}")
```

**Satisfies:** GOAL-1.1

#### 3.2.5 小红书 Extractor

这是最复杂的平台，需要辅助 Python 脚本。

```bash
# Step 1: 短链解析 (xhslink.com → xiaohongshu.com)
curl -Ls -o /dev/null -w '%{url_effective}' "{url}"

# Step 2: 尝试 web_fetch 直接抓取
web_fetch("{real_url}")
# 如果失败（返回空或登录页），使用 Jina Reader fallback
web_fetch("https://r.jina.ai/{real_url}")

# Step 3: 图片内容识别
# 小红书帖子图片 URL 通常在 HTML meta 标签或 JSON-LD 中
# 提取图片 URL 后：
#   a) 下载图片: curl -o /tmp/intake-img.jpg "{img_url}"
#   b) ⚠️ 已知限制: RustClaw 当前没有 vision API 工具（LLM tool 不支持图片输入）
#   c) Phase 1 workaround: 提取 alt text / meta description / 帖子文字部分
#   d) Phase 2: 给 RustClaw 加 vision tool 或用辅助脚本调 Claude vision API

# Step 4: 视频内容（如果有）
yt-dlp --extract-audio --audio-format wav -o /tmp/intake-audio.wav "{real_url}"
stt("/tmp/intake-audio.wav")
```

**Phase 1 策略:** web_fetch + Jina Reader。接受可能失败（反爬导致内容为空），此时明确告知用户并建议截图发送。
**Phase 2 增强:** 引入 DrissionPage 脚本 `skills/social-intake/scripts/xhs_fetch.py`，接口: `python xhs_fetch.py "{url}"` → stdout 输出 JSON `{title, text, image_urls[], video_url?}`。

**Satisfies:** GOAL-1.1, GOAL-1.3, GOAL-1.4

#### 3.2.6 微信公众号 Extractor

```bash
# mp.weixin.qq.com 文章可以直接 web_fetch
web_fetch("{url}")
# 提取: 标题 (og:title), 作者 (profile_nickname), 正文 (#js_content)

# 如果 web_fetch 返回的内容质量不够好，用 Jina Reader
web_fetch("https://r.jina.ai/{url}")
```

**注意:** 微信文章的链接有时效性（带 token 参数），过期后可能无法访问。所以 GOAL-3.4 的原文备份对微信尤其重要。
**Satisfies:** GOAL-1.1

#### 3.2.7 GitHub Extractor

```bash
# Repo 页面: github.com/{owner}/{repo}
# → 获取 README
web_fetch("https://raw.githubusercontent.com/{owner}/{repo}/HEAD/README.md")
# → 获取 repo metadata
web_fetch("https://api.github.com/repos/{owner}/{repo}")
# 返回: {description, stargazers_count, language, topics, created_at, updated_at}

# Issue 页面: github.com/{owner}/{repo}/issues/{num}
web_fetch("{url}")  # GitHub issues 页面 readability 提取效果好

# Discussion 页面: 同上，直接 web_fetch
```

**Satisfies:** GOAL-1.1

#### 3.2.8 Jina Reader Fallback

```bash
# 任何无法匹配到特定平台的 URL，或特定平台抓取失败时
web_fetch("https://r.jina.ai/{url}")
# 返回: markdown 格式的页面内容
```

**Satisfies:** GOAL-1.1 (fallback)

### 3.3 Content Analyzer

**Responsibility:** 对提取的原始内容进行结构化分析和知识关联。

这部分完全由 LLM 在 prompt 中完成，不需要外部工具。

**输出结构:**
```
title: string          # 一句话标题
platform: string       # 来源平台
author: string         # 原作者
date: string           # 发布日期
summary: string        # 2-3 句摘要
key_points: string[]   # 关键要点列表
category: enum         # tech/business/product/research/lifestyle/other
tags: string[]         # 搜索用标签
potential_value: string # 与 potato 项目/兴趣的相关度评估
actionable: bool       # 是否包含可执行信息
```

**知识关联（GOAL-2.2）:**
```
engram_recall("{title} {key concepts}")  → 找相关记忆
engram_recall("{tags}")                  → 找相关标签
# 在回复中列出找到的关联
```

**Satisfies:** GOAL-2.1, GOAL-2.2, GOAL-2.3, GOAL-2.4

### 3.4 Storage Layer

**Responsibility:** 将抓取和分析结果写入正确的存储位置。核心原则：**intake/ 是图书馆（存外部内容），IDEAS.md 是笔记本（只存自己的想法），engram 是大脑（只存有认知价值的关联）。**

#### 3.4.1 原文备份 + 结构化摘要（intake/ 目录）

每次 intake 生成一个文件，包含原始内容和分析结果：

```bash
mkdir -p intake/{YYYY-MM-DD}
# slug 从标题生成，去掉特殊字符，最长 50 字符
write_file("intake/{YYYY-MM-DD}/{platform}-{slug}.md", """
# {title}
- **URL**: {url}
- **Platform**: {platform}
- **Author**: {author}
- **Date**: {published_date}
- **Fetched**: {timestamp}
- **Category**: {category}
- **Tags**: {tags}

## Summary
{2-3 sentence summary}

## Key Points
{bullet list}

## Potential Value
{relevance to potato's projects}

## Connections
{related ideas/projects found via engram_recall, if any}

---

## Raw Content

{raw extracted content}
""")
```

**Satisfies:** GOAL-3.1

#### 3.4.2 索引文件（intake/index.md）

维护一个结构化索引，方便检索和回顾：

```markdown
# Social Media Intake Index

## 2026-04-03
| Platform | Title | Tags | File |
|----------|-------|------|------|
| twitter | How to build AI agents | ai, agent | [link](2026-04-03/twitter-how-to-build-ai-agents.md) |
| youtube | Rust async explained | rust, async | [link](2026-04-03/youtube-rust-async-explained.md) |
```

每次 intake 后 append 一行到对应日期下。

**Satisfies:** GOAL-3.3 (索引)

#### 3.4.3 Daily Log

```markdown
## Social Intake: {title}
- Source: {url} ({platform})
- Saved to: intake/{date}/{platform}-{slug}.md
- Key: {one-line summary}
```

**Satisfies:** GOAL-3.2

#### 3.4.4 有条件写入 IDEAS.md + Engram

**只有当外部内容触发了新想法或有价值关联时才写入。** 判断标准：

1. engram_recall 找到了相关的已有 idea/项目 → 写关联
2. 内容直接启发了新的 feature idea 或行动项 → 写新 idea
3. 纯信息性内容（别人的教程、新闻等）→ **不写** IDEAS.md/engram

```
# 只在发现有价值关联时写入
if connections_found or new_idea_inspired:
    # IDEAS.md — 写的是自己的想法，不是外部内容
    ## IDEA-{YYYYMMDD}-{NN}: {你的想法标题}
    - **Triggered by**: {intake url}
    - {你的想法内容...}
    
    # Engram — 存认知价值，不存原始内容
    engram_store(
      type: "factual",
      importance: 0.6,
      content: "从 {url} 发现 {insight}，可以用在 {project/idea}"
    )
```

**Satisfies:** GOAL-3.4

### 3.5 Dedup Checker

**Responsibility:** 避免重复处理同一 URL。

```
# 在开始处理前检查
engram_recall("{url}")
# 如果找到完全匹配的 URL → 回复 "这个链接之前已经处理过了，见 REF-xxx" 并跳过
# 如果找到相似但不同的 URL（比如同一帖子的不同链接形式）→ 提示但继续处理
```

**URL 规范化:** 去掉 query params 中的 tracking 参数（utm_*, ref, fbclid 等）后再比较。

**Satisfies:** GUARD-6

## 4. Data Models

### 4.1 IntakeResult（概念模型，在 prompt 中使用）

```
IntakeResult:
  url: string              # 原始 URL
  canonical_url: string    # 规范化后的 URL（去 tracking params）
  platform: Platform       # twitter | youtube | hn | reddit | xhs | wechat | github | other
  title: string
  author: string | null
  date: string | null
  raw_content: string      # 提取的原始文本
  summary: string          # LLM 生成的摘要
  key_points: string[]
  category: string
  tags: string[]
  potential_value: string
  actionable: bool
  connections: string[]     # engram 关联结果
  extraction_method: string # 用了哪个工具/fallback
  ref_id: string           # REF-YYYYMMDD-NN
```

### 4.2 Platform（枚举）

```
Platform:
  twitter   — twitter.com, x.com, t.co
  youtube   — youtube.com, youtu.be
  hn        — news.ycombinator.com
  reddit    — reddit.com, old.reddit.com
  xhs       — xhslink.com, xiaohongshu.com
  wechat    — mp.weixin.qq.com
  github    — github.com, raw.githubusercontent.com
  other     — 其他所有 URL（走 Jina fallback）
```

## 5. Data Flow

```
User sends URL in Telegram
        │
        ▼
┌─ Skill Trigger (regex: https?://) ──┐
│  social-intake (priority 80) wins    │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ Dedup Check ────────────────────────┐
│  engram_recall(url)                  │
│  if exists → reply "已处理" → STOP   │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ URL Router ─────────────────────────┐
│  match url against platform patterns │
│  if short link → follow redirect     │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ Platform Extractor ─────────────────┐
│  call platform-specific tools        │
│  if fails → try Jina Reader fallback │
│  if video → yt-dlp + stt            │
│  if image → vision model             │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ Content Analyzer ───────────────────┐
│  LLM generates structured analysis   │
│  engram_recall for connections        │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ Storage ─────────────────────────────┐
│  1. intake/ dir (raw + structured)    │
│  2. intake/index.md (索引)            │
│  3. daily log (brief event)           │
│  4. [conditional] IDEAS.md + engram   │
│     (only if new idea triggered)      │
└──────────────┬───────────────────────┘
               │
               ▼
┌─ Reply to User ──────────────────────┐
│  📎 title + summary + connections    │
│  💡 new idea (if triggered)          │
└──────────────────────────────────────┘
```

## 6. Error Handling

**Extraction 失败（GOAL-4.4）：**
```
Fallback 链: platform tool → Jina Reader → web_fetch raw → 告知失败

回复格式:
"⚠️ {platform} 抓取失败: {原因}
尝试了: {tool1} → {tool2} → {tool3}
建议: {具体建议，如 '可以截图发给我，我用 vision 读'}"
```

**部分成功：**
如果拿到了标题和部分内容但不完整，仍然处理并标记 `[部分提取]`。不伪造缺失内容（GUARD-5）。

**工具不可用（yt-dlp/bird 未安装）：**
检测到工具缺失时，回复安装命令并跳过该步骤。
```bash
# 检查工具是否可用
which yt-dlp || echo "yt-dlp not installed: brew install yt-dlp"
which npx || echo "npx not available: install Node.js"
```

**超时（GUARD-3）：**
yt-dlp 下载和 whisper 转录可能很慢。对于长视频（>30min），跳过音频转录，仅用 metadata + description。

## 7. Testing & Verification

测试方式：手动发送各平台 URL 到 Telegram，验证输出。

| 平台 | 测试 URL 类型 | 验证点 |
|------|-------------|--------|
| Twitter | 纯文字推文、带图片、带视频 | 文字提取完整、视频转录 |
| YouTube | 短视频、长视频、有字幕/无字幕 | metadata 正确、字幕优先于转录 |
| HN | 帖子、Ask HN、外链帖 | 评论+外链都能抓 |
| Reddit | self post、link post | 正文+评论提取 |
| 小红书 | 短链、长链、图文、视频 | 短链解析、图片 vision、视频转录 |
| 微信 | 公众号文章 | 标题+正文+作者 |
| GitHub | repo、issue、discussion | README + metadata |
| 其他 | 任意网页 | Jina fallback 工作 |

**去重测试:** 同一 URL 发两次，第二次应提示已处理。

**失败测试:** 发一个已下线的页面 URL，验证错误提示清晰。

## 8. Skill Trigger 设计

### 8.1 与 capture-idea 的共存

**问题:** capture-idea 也有 `https?://` regex trigger，priority 50。social-intake priority 80，会先触发。

**方案:** social-intake 匹配社交平台 URL → 走 social-intake。不匹配 → SKM 继续匹配下一个 skill → capture-idea 接管。

social-intake 的 trigger regex 需要精确匹配社交平台域名，而不是所有 URL：

```yaml
triggers:
  regex:
    - "(twitter\\.com|x\\.com|t\\.co)/"
    - "(youtube\\.com|youtu\\.be)/"
    - "news\\.ycombinator\\.com"
    - "(reddit\\.com|old\\.reddit\\.com)/"
    - "(xhslink\\.com|xiaohongshu\\.com)/"
    - "mp\\.weixin\\.qq\\.com"
    - "github\\.com/"
```

非社交平台 URL（比如博客、文档）仍然走 capture-idea。

**Satisfies:** GOAL-4.1

### 8.2 用户附加评论

如果 potato 发 URL 时附带了文字评论，那段评论应该作为 "human insight" 纳入分析，而不是只处理 URL。

## 9. 依赖安装

首次使用前需要安装：

```bash
# yt-dlp (视频/音频提取)
brew install yt-dlp

# bird CLI (Twitter) — 通过 npx 免安装，首次运行会自动下载
npx bird --version

# DrissionPage (小红书 fallback, Phase 2)
# pip install DrissionPage
```

## 10. Non-Goals (明确不做的)

- 不做登录态管理（不存 cookie/token 到文件系统）
- 不做评论区深度抓取（只取帖子本身，不递归抓评论）
- 不做翻译（内容是什么语言就保留什么语言）
- 不做定时批量抓取（Phase 2 GOAL-5.x）
- 不修改 RustClaw Rust 源码（纯 Skill + 辅助脚本）
