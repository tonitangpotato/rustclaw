# Design: Social Media Intake Skill

## 1. Overview

Social Media Intake 是一个 RustClaw Skill，当 potato 在 Telegram 发送社交媒体 URL 时自动触发。它根据 URL 所属平台选择对应的抓取策略，提取内容后进行分析、关联、存储。

**核心设计决策：**
- **Skill 不是代码** — 它是一组 prompt 指令 + 工具调用编排，运行在 RustClaw agent 的 LLM context 里
- **平台路由通过 URL pattern matching** — skill 内部做，不需要 Rust 代码
- **复杂抓取逻辑走 Python 辅助脚本** — 统一的提取引擎 `intake.py` 处理所有平台
- **与 capture-idea skill 的关系** — social-intake 的 trigger priority 高于 capture-idea（80 > 50），当 URL 匹配社交平台时走 social-intake；不匹配时 fallback 到 capture-idea 的通用逻辑
- **存储哲学** — intake/ 是图书馆（存外部内容），IDEAS.md 是笔记本（只存自己的想法），engram 是大脑（只存有认知价值的关联）。外部内容本身不进 IDEAS.md 和 engram

**Trade-offs：**
- 选 Skill 而非独立 crate：开发快、迭代快，但性能和错误处理不如编译时代码。对于个人 intake 工具这是正确的 trade-off。
- 选 Python 辅助脚本而非纯 LLM tool calls：复杂的 URL 解析、短链跟踪、HTML 解析在 Python 中更可靠且可独立测试。
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
│   │  intake.py   │  │  Analyzer  │  │Store││
│   │ (Python CLI) │  │   (LLM)    │  └─────┘│
│   └──────┬───────┘  └────────────┘         │
│          │                                  │
│   ┌──────┴───────────────────────────┐      │
│   │    Platform-Specific Extractors  │      │
│   ├──────┬──────┬─────┬─────┬───────┤      │
│   │Twitter│YouTube│HN  │Reddit│GitHub│      │
│   │(bird) │(yt-dlp)│(API)│(API) │(API) │      │
│   └──────┴──────┴─────┴─────┴───────┘      │
│              │ Fallback to Jina Reader      │
└─────────────────────────────────────────────┘
```

**关键组件：**
1. **intake.py** - Python CLI 提取引擎，独立可测试
2. **SKILL.md** - LLM orchestration 层，调用 intake.py 并做分析/存储
3. **engram** - 知识图谱，用于去重和关联发现

## 3. Components

### 3.0 Prerequisites - RustClaw Integration Points

**Required RustClaw Functions:**

The skill depends on these RustClaw built-in functions (assumed to be implemented):

```rust
// Search the knowledge graph for related content
// Returns: Vector of matching engram entries with relevance scores
fn engram_recall(query: &str) -> Vec<EngramEntry>

// Store a new fact/connection in the knowledge graph
// Parameters:
//   - type: "factual" | "connection" | "belief" | "goal"
//   - importance: 0.0-1.0 (higher = more important for recall)
//   - content: Free-form text that can be searched later
fn engram_store(entry_type: &str, importance: f32, content: &str) -> Result<(), Error>
```

**How Skills Call These (from SKILL.md context):**

Skills use pseudo-function syntax that RustClaw interprets:
```
engram_recall("url_hash:abc123")       # Search for dedup
engram_recall("rust async patterns")   # Semantic search
engram_store(type="factual", importance=0.7, content="...")
```

These get translated to actual Rust function calls by the RustClaw skill executor.

### 3.1 intake.py - Content Extraction Engine

**Location:** `skills/social-intake/scripts/intake.py`

**Interface:**
```bash
# Full extraction
python skills/social-intake/scripts/intake.py <url> [--json]

# Dedup check only (returns only url_hash and existing status)
python skills/social-intake/scripts/intake.py <url> --dedup-check
```

**Invocation from SKILL.md:**
```
Bash: cd {project_root} && python skills/social-intake/scripts/intake.py "{url}" --json
```

**Responsibilities:**
- URL validation and normalization
- URL hash generation (SHA256 first 16 chars of canonical URL)
- Short link resolution (follow redirects for t.co, xhslink.com, b23.tv, etc.)
- Platform detection (domain-based regex matching)
- Platform-specific extraction with fallback chain
- Unified JSON output format

**Error Handling:**
- Returns `success: false` with detailed `error` field on failure
- Does NOT throw exceptions - always returns valid JSON
- Timeout: 60s per URL (enforced by subprocess timeout in Skill)

**Output Schema:**
```json
{
  "url": "原始 URL",
  "canonical_url": "标准化 URL",
  "platform": "twitter|youtube|hn|reddit|xhs|wechat|github|other",
  "title": "标题",
  "author": "作者",
  "date": "发布日期",
  "raw_content": "提取的文本内容",
  "extraction_method": "使用的方法",
  "url_hash": "去重 hash（SHA256 前16位）",
  "success": true,
  "error": null
}
```

**Platform Extraction Methods:**

| 平台 | 主要方法 | Fallback | Vision Support |
|------|---------|----------|----------------|
| Twitter/X | `npx bird read` | Jina Reader | ✅ Phase 1 (卡片/图片) |
| YouTube | `yt-dlp --dump-json` | Jina Reader | N/A |
| Hacker News | HN Firebase API | - | N/A |
| Reddit | Reddit JSON API | Jina Reader | ⚠️ Phase 2 |
| 小红书 | Jina Reader + Vision | Vision-only | ✅ Phase 1 (P0) |
| 微信公众号 | Jina Reader | HTML parse | ⚠️ Phase 2 |
| GitHub | GitHub API + raw | Jina Reader | N/A |
| 其他 | Jina Reader | web_fetch | ⚠️ Phase 2 |

**Vision Integration (Phase 1):**
- 小红书: Jina Reader 提取文字部分 → 如果图片链接存在，下载图片 → Claude vision 分析图片内容 → 合并文字+图片分析
- Twitter: 检测推文中的图片/卡片 → Claude vision 提取内容
- 实现: `extract_with_vision(url, initial_content, image_urls) -> enhanced_content`

**Error Handling:**
- 提取失败时 `success: false`, `error` 字段包含原因和建议
- LLM 根据 error 提供用户友好的解释和替代方案
- Rate limit 错误: 延迟重试（exponential backoff）
- 反爬/403: 建议用户手动复制内容
- 404: 标记为失效链接，不存储

**Satisfies:** GOAL-1.1, GOAL-1.2, GOAL-1.3

### 3.2 LLM Analysis Layer (in SKILL.md)

**Workflow:**

```
1. Dedup check
   └─> engram_recall("url_hash:{hash}")
       └─> 如果找到，回复"已处理"并终止

2. Call intake.py
   └─> python intake.py "{url}" --json
       └─> 解析 JSON 输出

3. Enhanced extraction (optional)
   └─> 对于 YouTube: 尝试下载字幕或转录
   └─> 对于长内容: 使用 stt() 工具

4. Content analysis
   └─> 生成结构化分析：
       - 标题、摘要、关键点
       - 分类、标签
       - 与 potato 工作的相关性
       - 可执行见解

5. Connection discovery
   └─> engram_recall("{关键概念}")
   └─> engram_recall("{标签}")
   └─> 列出相关已有 idea/项目

6. Three-layer storage
   ├─> Layer 1: intake/{platform}/{slug}.md (ALWAYS)
   │   └─> 包含完整内容 + 分析
   ├─> Layer 2: memory/{date}.md (ALWAYS)
   │   └─> 简短日志条目
   └─> Layer 3: IDEAS.md (CONDITIONAL)
       └─> 仅当触发新想法时写入

7. User response
   └─> Telegram 结构化回复
```

**Satisfies:** GOAL-2.1, GOAL-2.2, GOAL-2.3, GOAL-2.4

### 3.3 Storage Layer

#### 3.3.1 intake/ Directory Structure

```
intake/
├── twitter/
│   └── {author}-{id}.md
├── youtube/
│   └── {channel}-{title-slug}.md
├── hn/
│   └── item-{id}.md
├── reddit/
│   └── {subreddit}-{id}.md
├── github/
│   └── {owner}-{repo}.md
├── xhs/
│   └── {id}.md
├── wechat/
│   └── {slug}.md
└── other/
    └── {domain}-{slug}.md
```

**每个文件格式：**
```markdown
# {title}

- **URL**: {url}
- **平台**: {platform}
- **作者**: {author}
- **日期**: {published_date}
- **抓取时间**: {timestamp}
- **分类**: {category}
- **标签**: {tags}
- **URL Hash**: {url_hash}
- **提取方法**: {extraction_method}

## 摘要

{2-3 句总结}

## 关键点

{要点列表}

## 相关性

{对 potato 工作的价值评估}

## 关联

{从 engram_recall 找到的相关 idea/项目}

## 可执行见解

{用这个信息该做什么}
{具体行动及上下文}

---

## 原始内容

{完整提取的文本/转录}
```

**Satisfies:** GOAL-3.1

#### 3.3.2 Daily Log (memory/)

简短条目：
```markdown
## 📥 Social Intake: {title}
- **来源**: {url} ({platform})
- **已保存**: intake/{platform}/{slug}.md
- **关键**: {一句话总结}
```

**Satisfies:** GOAL-3.2

#### 3.3.3 IDEAS.md (Conditional)

**写入条件（满足任一即可）：**
1. 内容触发了 potato 的新想法
2. 内容与现有 idea 有强关联（需要更新现有 idea）

**写入格式（新想法）：**
```markdown
## IDEA-{YYYYMMDD}-{NN}: {你的新想法标题}
- **日期**: {date}
- **触发来源**: {url} ({platform})
- **分类**: {category}
- **标签**: {tags}

### 想法
{描述你的想法 - 不是外部内容}

### 为什么重要
{与你的项目/目标的联系}

### 下一步
{如果适用，列出可执行项}

### 来源上下文
见: intake/{platform}/{slug}.md 获取完整外部内容

### 状态: 💡 新
```

**更新现有 idea（如果强关联）：**
在相关 IDEA section 追加：
```markdown
**Connection ({date})**: {url} provides {explanation}. See intake/{platform}/{slug}.md
```

**Satisfies:** GOAL-3.4

#### 3.3.4 Engram Integration

**写入 engram 的内容（三种类型）：**

1. **Dedup 记录（总是）：**
```python
engram_store(
    type="factual",
    importance=0.3,
    content="Intake processed: url_hash:{hash} | {url} | {platform} | {title} | intake/{path}"
)
```

2. **新想法（仅当写入 IDEAS.md 时）：**
```python
engram_store(
    type="factual",
    importance=0.7,
    content="New idea: {idea_summary}. Triggered by {url}. Tags: {tags}. See IDEAS.md"
)
```

3. **连接（仅当发现关联时）：**
```python
engram_store(
    type="connection",
    importance=0.6,
    content="{url} connects to IDEA-{id}: {explanation}"
)
```

**Satisfies:** GOAL-3.5

## 4. User Experience

### 4.1 触发条件

**自动触发** - 当 Telegram 消息包含以下域名：
- twitter.com, x.com, t.co
- youtube.com, youtu.be
- news.ycombinator.com
- reddit.com, old.reddit.com
- xiaohongshu.com, xhslink.com
- mp.weixin.qq.com
- github.com

**优先级:** 80（高于 capture-idea 的 50）

**Satisfies:** GOAL-4.1

### 4.2 响应格式

**单个 URL:**
```
✅ 已保存: {title}

📝 {一句话总结}

🔖 分类: {category}
🏷️  标签: {tags}

{如果发现关联:}
🔗 关联: {简要列表}

{如果有可执行见解:}
💡 可执行: {简要列表}

📂 已保存到: intake/{platform}/{slug}.md
{如果添加到 IDEAS.md: "💡 已添加新想法到 IDEAS.md"}
```

**多个 URL (GOAL-4.3, P1):**
Skill 检测到多个 URL 时，串行处理（Phase 1），每个完成后发送简短确认：
```
✅ 1/3 已保存: {title1}
✅ 2/3 已保存: {title2}
✅ 3/3 已保存: {title3}

📊 批量处理完成:
- 新保存: 2 个
- 已存在跳过: 1 个
- 详见 memory/{today}.md
```

**处理时间:** 
- 单个文本内容: <30秒
- 单个音频转录: <2分钟
- 批量 (N 个): <30秒 × N （串行）

**Satisfies:** GOAL-4.2, GOAL-4.3

## 5. Phase 1 Limitations & Phase 2 Enhancements

### Phase 1 (MVP Implementation)

✅ **Working:**
- 文本内容提取（所有平台）
- YouTube metadata + 字幕
- 去重和索引
- 知识图谱集成
- **Vision API 集成** - 小红书图片 OCR（P0 requirement, GOAL-1.4）
- 基础错误处理和 fallback 链

⚠️ **Known Limitations:**
- Twitter 图片/卡片内容提取有限（仅文字部分）
- 长视频转录耗时长（>10分钟视频）
- 串行处理（一次一个 URL）

### Phase 2 (Future Enhancements)

🔮 **Planned:**
1. **性能优化**
   - 异步批处理（多个 URL 并行提取）
   - 增量转录（长视频分段处理）
   - 结果缓存（避免重复下载）

2. **智能索引**
   - 向量搜索（语义相似内容发现）
   - 自动标签聚类

3. **增强的 Vision 分析**
   - Twitter 图片卡片全内容识别
   - 截图中的代码/终端输出提取

4. **Action Dashboard**
   - 可执行项独立追踪页面
   - 自动提醒待跟进内容

**Satisfies:** 为 Phase 2 提供清晰的升级路径

## 6. Success Criteria

- ✅ URL 检测自动触发，无需手动指定 skill
- ✅ 去重生效（重复 URL 秒回）
- ✅ 平台专用提取器提供比通用爬虫更好的结果
- ✅ 内容被分析、标记、关联到已有知识
- ✅ 有价值的内容进入 IDEAS.md，普通内容仅归档
- ✅ 响应快速（<30秒文本，<2分钟音频）
- ✅ 错误处理友好（提取失败时给出明确指导）

## 7. Development Approach

**Phase 1 (MVP):**
1. ✅ 实现 `intake.py` CLI 工具
2. ✅ 编写 `SKILL.md` orchestration 逻辑
3. 测试各平台提取效果
4. 迭代优化 prompt 和分析质量

**Phase 2 (Enhancements):**
1. 添加 vision API 集成
2. 实现向量搜索
3. 构建 action dashboard

**测试策略：**
- `intake.py` 独立单元测试（每个 extractor）
- 端到端测试（通过 Telegram 发送真实 URL）
- 边界情况测试（短链、重定向、失效链接、反爬等）

