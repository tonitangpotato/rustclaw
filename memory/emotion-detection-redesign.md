# Emotion Detection Redesign — LLM-Native, No Fallback

**Date**: 2026-04-19
**Status**: Draft
**Scope**: engramai (extractor, accumulator) + RustClaw (engram_hooks, memory)

---

## 1. 问题：关键词 Fallback 掩盖了系统性故障

### 1.1 当前数据流

```
用户消息
  ├─→ Haiku LLM extraction → ExtractedFact{content, memory_type, importance, confidence}
  │     (有 LLM 在跑，但不输出 emotion/valence)
  │
  └─→ detect_emotion() → 30个关键词匹配 → f64 (0.7 / -0.5 / 0.0)
        ↓
      detect_domain() → ~20个关键词匹配 → "coding" / "trading" / "general" 等
        ↓
      EmotionalAccumulator.record_emotion(domain, valence)
        ↓
      InteroceptiveHub → system prompt 注入
```

### 1.2 三个根本问题

**问题 A：重复处理，浪费了 LLM 的理解能力**

Haiku 已经在 `extract()` 里完整读了对话内容，它完全理解语境、语气、情绪。但输出 schema 里没有 valence 字段，所以这些理解被丢弃了。然后我们又用 30 个关键词重新"猜"情绪——这等于用拼音输入法翻译一篇英语文章，Haiku 已经读懂了我们却不让它说。

**问题 B：Fallback 掩盖故障**

这是 potato 指出的核心问题。当前系统的哲学是"有总比没有好"——Haiku 不给 valence？没关系，用关键词匹配。关键词匹配不准？没关系，至少有个数字。这种 fallback 链的恶果：

- **你永远不知道 Haiku extraction 是否出了问题**。如果 prompt 改坏了、API 超时了、JSON 解析失败了——关键词匹配照样给你一个看起来合理的数字，一切正常。
- **你永远不知道情绪数据的质量**。0.7 是 LLM 深度理解的结果，还是因为消息里碰巧出现了 "nice" 这个词？你分不清。
- **debug 变成噩梦**。interoceptive state 显示 coding domain valence = 0.3——这是真实趋势还是关键词噪音？无法判断。

这个模式（"静默 fallback 掩盖真实问题"）很可能已经在系统其他地方也存在了。

**问题 C：EmotionalAccumulator 接收单个 f64，丢失了所有上下文**

```rust
pub fn record_emotion(&self, domain: &str, valence: f64)
```

一条消息可能包含复杂情绪："代码终于跑通了但是性能太差了"——这既有成就感 (+0.6) 又有挫败感 (-0.4)。当前系统要么取平均 (0.1)，要么只取一个，两种都不对。

更深层的问题：`domain` 也是单个字符串。同一条消息可能同时涉及 coding 和 trading，但系统只能归到一个 domain。detect_domain 用关键词匹配选第一个命中的——如果消息是 "trading bot 的代码有 bug"，它会匹配到 "coding"（因为 "bug" 先出现在关键词列表里），而不是 "trading"。

---

## 2. 设计目标

1. **Haiku 作为唯一情绪来源**——不保留关键词匹配 fallback
2. **Haiku 不输出 = 报错**——让问题浮出水面
3. **支持多情绪、多 domain**——一条消息可以产生多条情绪记录
4. **零额外 API 成本**——只改 prompt 和 schema，不加新调用
5. **向后兼容**——`EmotionalAccumulator` 的存储层（SQLite 表）不改

---

## 3. 方案

### 3.1 ExtractedFact 加字段（engramai）

```rust
// src/extractor.rs
pub struct ExtractedFact {
    pub content: String,
    pub memory_type: String,
    pub importance: f64,
    #[serde(default = "default_confidence")]
    pub confidence: String,
    
    // ── NEW ──
    /// Emotional valence of this fact: -1.0 (very negative) to 1.0 (very positive).
    /// 0.0 = neutral/informational. Required from LLM extraction.
    pub valence: f64,
    
    /// Domain this fact belongs to: "coding", "trading", "research", "communication", "general"
    #[serde(default = "default_domain")]
    pub domain: String,
}

fn default_domain() -> String {
    "general".to_string()
}
```

**为什么 `valence` 是 `f64` 不是 `Option<f64>`：**

Option 意味着"可以没有"，那就又回到了 fallback 思维。我们要的语义是：每个 extracted fact 都必须有情绪标注。Haiku 不给 = serde 解析失败 = 错误冒泡 = 我们知道出了问题。

`serde` 对缺失的 `f64` 字段默认会报 `missing field` 错误，这正是我们想要的行为。

`domain` 用 `#[serde(default)]` 是因为 domain 确实可以是 "general"——这是一个合理的默认值，不是在掩盖问题。

### 3.2 更新 EXTRACTION_PROMPT（engramai）

在现有 prompt 的 JSON schema 说明中加入：

```
Respond with ONLY a JSON array (no markdown, no explanation):
[{"content": "...", "memory_type": "...", "importance": 0.X, "confidence": "confident|likely|uncertain", "valence": 0.X, "domain": "..."}]

Field rules:
- valence (REQUIRED): emotional valence of this specific fact, from -1.0 (very negative) to 1.0 (very positive). 
  0.0 = purely neutral/informational. 
  Consider the speaker's emotional state, not just keywords.
  Examples: frustration with a bug = -0.5, excitement about a working feature = 0.7, 
  neutral status report = 0.0, mixed feelings = use the dominant emotion.
- domain (REQUIRED): which domain this fact belongs to. 
  One of: "coding", "trading", "research", "communication", "general".
  Choose the most specific applicable domain.
```

**为什么不让 LLM 输出多个 domain 或多个 valence per fact：**

它已经输出多个 facts 了。一条消息 "代码跑通了但是性能太差" 自然会被 Haiku 拆成两个 facts：
- `{content: "代码终于跑通了", valence: 0.6, domain: "coding"}`
- `{content: "但性能太差，需要优化", valence: -0.4, domain: "coding"}`

每个 fact 就是一个原子情绪单元——不需要在 fact 内部再拆。这就是为什么多情绪/多domain 的支持是免费的。

### 3.3 MemoryManager 缓存 extraction 结果（engramai）

```rust
// src/memory.rs (engramai crate)
// 在 MemoryManager struct 中加：
last_extraction_emotions: Mutex<Option<Vec<(f64, String)>>>,  // Vec<(valence, domain)>
```

`store()` 方法在 extraction 成功后，从 `Vec<ExtractedFact>` 中收集所有 `(valence, domain)` 对，缓存起来。

```rust
// 在 store() 内部，extraction 成功后：
let emotions: Vec<(f64, String)> = facts.iter()
    .map(|f| (f.valence, f.domain.clone()))
    .collect();
*self.last_extraction_emotions.lock().unwrap() = Some(emotions);
```

暴露 getter：

```rust
/// Take the emotion data from the last extraction.
/// Returns None if no extraction has happened since last take.
/// This is a one-shot: calling it clears the cache.
pub fn take_last_emotions(&self) -> Option<Vec<(f64, String)>> {
    self.last_extraction_emotions.lock().unwrap().take()
}
```

**为什么是 `Vec<(f64, String)>` 不是单个 `(f64, String)`：**

一条消息可能产生多个 facts，每个有不同的 valence 和 domain。全部传给 accumulator，让它逐条记录。

### 3.4 RustClaw engram_hooks.rs 改动

```rust
// 当前代码（删除）：
let emotion = MemoryManager::detect_emotion(user_msg);
let domain = MemoryManager::detect_domain(&store_content);
if let Err(e) = self.memory.process_interaction(&store_content, emotion, domain) { ... }

// 新代码：
match self.memory.take_last_emotions() {
    Some(emotions) => {
        for (valence, domain) in &emotions {
            if let Err(e) = self.memory.process_interaction(&store_content, *valence, domain) {
                // process_interaction 失败不应该静默——log at warn level
                tracing::warn!("Emotion recording failed for domain '{}': {}", domain, e);
            }
        }
        if !emotions.is_empty() {
            tracing::debug!(
                "Recorded {} emotion signals from LLM extraction",
                emotions.len()
            );
        }
    }
    None => {
        // Extraction 没有产生结果——这可能是因为消息太短被跳过了（store 的 skip 逻辑），
        // 或者 extraction 真的失败了。
        // store() 已经在内部 log 了 extraction 错误，这里只需要 debug log。
        tracing::debug!("No emotion data from extraction (message may have been skipped or extraction failed)");
    }
}
```

### 3.5 删除关键词匹配方法（RustClaw memory.rs）

```rust
// 删除：
pub fn detect_emotion(user_msg: &str) -> f64 { ... }
pub fn detect_domain(content: &str) -> &'static str { ... }
```

这两个方法完全删除，不保留。如果有其他代码调用它们，编译器会告诉我们——这比 grep 更可靠。

### 3.6 错误处理策略：不 fallback

| 场景 | 当前行为 | 新行为 |
|------|---------|--------|
| Haiku extraction 成功，有 valence | N/A（当前不输出） | 正常路径：记录到 accumulator |
| Haiku extraction 成功，没给 valence | N/A | serde 解析失败 → `store()` 返回 Err → RustClaw log warn |
| Haiku API 超时/错误 | 静默 fallback 到关键词 | `store()` 返回 Err → RustClaw log warn → `take_last_emotions()` 返回 None |
| 消息太短被 skip | 关键词照样匹配 | skip → 不调 extraction → `take_last_emotions()` 返回 None → 不记录（正确行为：短消息没有情绪信号） |

**关键原则：宁可没有数据，不要错误数据。**

没有情绪数据 = accumulator 不更新 = interoceptive state 保持上一次的值。这比注入噪音好得多。

---

## 4. EmotionalAccumulator 本身的问题

你问的"EmotionalAccumulator 本身就是接收单个 f64 的，这是不是也是个问题"——

**不是问题，但需要澄清为什么。**

### 4.1 record_emotion 签名不需要改

```rust
pub fn record_emotion(&self, domain: &str, valence: f64) -> Result<(), rusqlite::Error>
```

这个签名是对的。它接收一个 domain + 一个 valence，记录一条情绪事件。如果一条消息有 3 个 facts，就调 3 次——每次一个 (domain, valence) 对。这正是 3.4 节里 `for (valence, domain) in &emotions` 做的事。

accumulator 不需要知道"这 3 条来自同一条消息"——它只需要记录事件流。running average 自然会平滑单条消息的多个信号。

### 4.2 真正的 accumulator 问题：running average 的局限

当前 running average 公式：

```rust
new_valence = (old_valence * count + new_value) / (count + 1)
```

这意味着：
- **永远不会遗忘**。第 1 条消息和第 10000 条消息对当前 valence 的影响相同（都是 1/N）。
- **近期情绪变化被稀释**。如果前 100 条都是 +0.5，第 101 条是 -0.8，valence 从 0.5 变成 0.487——几乎感知不到。

但这是一个独立的改进点，跟情绪检测来源（LLM vs 关键词）是正交的。这次改动不碰 accumulator 的平均算法。如果之后要改，可以换成 EMA（exponential moving average）——但那是另一个文档。

### 4.3 domain 硬编码列表

当前 domain 是硬编码的 5 个：coding, trading, research, communication, general。

让 Haiku 输出 domain 后，理论上它可以输出任意 domain（比如 "health", "finance", "family"）。但 accumulator 不在乎——它存的是 TEXT PRIMARY KEY，什么 domain 都接受。

问题在下游：interoceptive hub 和 system prompt 模板可能只认识这 5 个 domain。但这也不是 blocking issue——未知 domain 只是不会出现在 trend 报告里，不会报错。

**prompt 里约束 domain 为固定列表（3.2 节已经做了）是当前最合理的选择。** 以后如果需要扩展 domain，只需要改 prompt + 确认下游能处理。

---

## 5. 改动清单

### engramai crate（3 个文件）

| 文件 | 改动 | 大小 |
|------|------|------|
| `src/extractor.rs` | ExtractedFact 加 `valence: f64` + `domain: String` 字段 | ~5 行 |
| `src/extractor.rs` | EXTRACTION_PROMPT 加 valence/domain 说明 | ~10 行 |
| `src/memory.rs` | MemoryManager 加 `last_extraction_emotions` 缓存 + `take_last_emotions()` | ~20 行 |

### RustClaw（2 个文件）

| 文件 | 改动 | 大小 |
|------|------|------|
| `src/engram_hooks.rs` | 用 `take_last_emotions()` 替换 detect_emotion/detect_domain | ~15 行改 |
| `src/memory.rs` | 删除 `detect_emotion()` + `detect_domain()` | ~30 行删 |

### 不改的

- `EmotionalAccumulator` — 签名和逻辑不变
- `InteroceptiveHub` — 读 accumulator 的方式不变
- `BehaviorFeedback` — 不相关
- `emotional_trends` SQLite 表 — schema 不变

---

## 6. 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Haiku 偶尔不输出 valence | 低 | 该条消息 extraction 整体失败（所有 facts 被丢弃） | prompt 明确 REQUIRED；serde 解析失败会 log warn，我们能发现并修 prompt |
| Haiku valence 精度不如预期 | 中 | 情绪趋势有噪音 | 仍然比关键词匹配好几个数量级；且 accumulator 的 running average 会平滑 |
| 删除 detect_emotion 后编译失败 | 低 | 发现其他意外调用者 | 好事——编译器帮我们找到所有依赖点 |
| engramai 版本升级破坏 RustClaw 兼容 | 低 | RustClaw 需要同步更新 | 同一个人维护两个 crate |

---

## 7. 不做的事

- **不改 accumulator 平均算法**——那是独立优化，不在这次范围
- **不改 interoceptive hub**——它读 accumulator 的方式不变
- **不加新的 API 调用**——只改 prompt
- **不保留 detect_emotion/detect_domain 作为 "backup"**——这正是要消灭的 fallback 思维
