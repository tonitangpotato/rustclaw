# Idea Pool — Design

## 核心概念

Idea Pool不是一个tool，是一套**pipeline**，串联现有系统：

```
输入 (语音/文字/URL)
  ↓
┌──────────────┐
│ Capture      │ → engram store (namespace: "ideas", type: idea)
│ + Auto-tag   │ → Haiku extracts: category, tags, related_projects
└──────┬───────┘
       ↓
┌──────────────┐
│ Auto-Research│ → web_search + web_fetch + summarize
│              │ → findings追加到idea的engram record
└──────┬───────┘
       ↓
┌──────────────┐
│ Auto-Link    │ → engram recall找相关ideas (Hebbian自动形成)
│              │ → GID query找相关projects
│              │ → 输出关联图
└──────┬───────┘
       ↓
┌──────────────┐
│ Notify       │ → 发Telegram: "新想法已capture，关联了X个ideas和Y个projects"
└──────────────┘
```

## Idea 数据结构

```yaml
# 存在engram里，metadata字段携带结构化数据
content: "用causal inference做stock研报自动分析"
type: idea
importance: 0.7
metadata:
  status: raw          # raw → evaluating → doing → done → dropped
  category: fintech    # auto-tagged by Haiku
  tags: [causal, stocks, research, automation]
  related_projects: [autoalpha, the-unusual]
  source: text         # text | voice | url
  source_url: null     # 如果从URL capture
  research_summary: "..."  # auto-research结果
  created_at: 2026-03-29
  evaluated_at: null
  started_at: null
```

## 5个入口

### 1. 直接文字/语音
```
potato: "我有个想法，用engram的Hebbian learning做推荐系统"
→ Capture → Tag → Research → Link → Notify
```

### 2. URL (社媒帖子/文章/视频)
```
potato: "这个帖子跟我的autoalpha想法有关 https://x.com/..."
→ Fetch URL → Summarize (文字/视频/音频) → Extract insights
→ 找到相关ideas → 存为reference → Link到ideas → Notify
```

### 3. 命令式
```
/idea add "用LLM做代码review自动化"
/idea list [status] [category]
/idea status <id> evaluating
/idea review              # 列出所有raw ideas，建议下一步
/idea research <id>       # 对已有idea补充research
/idea link <id> <project> # 手动关联到project
```

### 4. Heartbeat自动review
```
每周一次扫描：
- raw ideas > 7天 → 提醒evaluate
- evaluating ideas > 14天 → 提醒decide (do or drop)
- 新Hebbian links formed → 通知关联发现
```

### 5. 被动capture
```
potato在对话中提到"以后可以..."或"有个idea..."
→ engram的LLM extractor自动识别为idea
→ 走capture pipeline
```

## 和现有系统的集成

| 系统 | 角色 |
|------|------|
| **Engram** | idea存储、recall、Hebbian关联、namespace隔离 |
| **GID** | 当idea → doing时，创建project node + task graph |
| **web_fetch** | URL capture时获取内容 |
| **summarize** | 视频/音频/长文的总结 |
| **web_search** | auto-research阶段搜索相关信息 |
| **Cron/Heartbeat** | 定期review提醒 |
| **Telegram** | 通知 + 交互入口 |

## 实现方案

不是加一个tool，是加一个 **IdeaPipeline module**：

```rust
// src/idea_pool.rs

pub struct IdeaPool {
    memory: Arc<MemoryManager>,  // engram
    // tools for research
}

impl IdeaPool {
    /// Capture a new idea from text
    pub async fn capture(&self, content: &str, source: IdeaSource) -> Idea { ... }
    
    /// Capture from URL (fetch → summarize → extract → link)
    pub async fn capture_url(&self, url: &str, context: Option<&str>) -> Idea { ... }
    
    /// Auto-research an idea (web search + summarize)
    pub async fn research(&self, idea_id: &str) -> ResearchResult { ... }
    
    /// Find related ideas and projects
    pub async fn find_links(&self, idea_id: &str) -> Vec<IdeaLink> { ... }
    
    /// Update idea status
    pub async fn update_status(&self, idea_id: &str, status: IdeaStatus) { ... }
    
    /// Review: list ideas needing attention
    pub async fn review(&self) -> ReviewReport { ... }
    
    /// Promote idea to project (creates GID graph)
    pub async fn promote_to_project(&self, idea_id: &str, project_dir: &str) { ... }
}
```

## 状态流转

```
raw ──→ evaluating ──→ doing ──→ done
  │         │            │
  └─dropped─┘────dropped─┘

raw: 刚capture，未review
evaluating: 在research/思考阶段
doing: 已创建project，在执行
done: 完成
dropped: 放弃（保留记录，可能以后revive）
```

## 开放问题

1. **Idea的ID**: 用engram的memory_id，还是单独的sequential ID（方便命令行引用）？
2. **Research深度**: auto-research默认做多深？1次web search + 3个结果的摘要？还是更深？
3. **被动capture**: LLM extractor怎么区分idea vs普通episodic memory？加个extraction rule？
4. **GID集成粒度**: idea → doing时，自动创建完整的GID项目结构？还是只创建一个node？
