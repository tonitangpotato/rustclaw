# 一页纸术语速查表 — Engram 专版

> 给 Phoebe 做 **Engram**（potato 的认知记忆系统）相关 marketing 用。
> Engram 是给 AI agent 用的"会思考、会遗忘、会联想"的记忆系统。
> 受众主要是 **AI 工程师、agent 开发者、对认知科学好奇的技术人**。

---

## 🧠 Engram 是什么（一句话定位）

- **工程师版**：A neuroscience-grounded memory engine for AI agents — ACT-R activation, Hebbian learning, Ebbinghaus forgetting.
- **半专业版**：让 AI agent 拥有"像人一样会遗忘、会联想、会越用越熟"的长期记忆系统。
- **大众版**：让 AI 不再每次见你都"失忆从头来"，而是真的能记住、回忆、并且自然遗忘。

---

## 🧬 神经科学 / 认知科学概念

这部分是 Engram 的"灵魂术语"，因为 Engram 的卖点就是**它不是普通的向量数据库，而是基于真实人脑研究的设计**。这些词保留它们能让 Engram 听起来"科学严肃 + 有理论根基"。

| 术语 | 一句话解释 | 大众版 / Marketing 用 |
|---|---|---|
| **Engram（印迹）** | 神经科学里指"记忆在大脑里留下的物理痕迹" | 产品名字本身就是个隐喻 — "AI 的记忆痕迹" |
| **ACT-R** | 卡耐基梅隆大学开发的一套"人类认知如何运作"的理论模型 | "模拟人脑的认知模型" |
| **ACT-R activation** | 一段记忆当前的"激活强度"，越常用越强，越久不用越弱 | "记忆的鲜活度" |
| **Activation decay** | 不使用的记忆会自然衰减 | "记忆会自然淡化" |
| **Hebbian learning** | "一起激活的神经元会越连越紧" — 心理学家 Hebb 的理论 | "经常一起出现的事会被联想到一起" |
| **Hebbian association** | 两段记忆因为常一起出现而被关联起来 | "记忆联想" |
| **Ebbinghaus forgetting curve（艾宾浩斯遗忘曲线）** | 19 世纪心理学家发现的"人类遗忘速度"曲线 | "符合人类自然遗忘规律" |
| **Cognitive architecture（认知架构）** | 模拟人脑工作方式的整体设计 | "类脑设计" |
| **Recognition memory（识别记忆）** | 不是"主动想起"，是"看见这个再认出来"的记忆类型 | "似曾相识"那种记忆 |
| **Working memory（工作记忆）** | 你当前脑子里正在活跃处理的内容（容量很小） | "短期记忆 / 当下专注的内容" |
| **Episodic memory（情景记忆）** | 关于"某次经历"的记忆，带时间地点 | "事件记忆"，例如"上周三跟 Phoebe 聊了 X" |
| **Factual memory（事实记忆）** | 关于"事情本身是什么"的记忆，不带时间 | "知识记忆"，例如"巴黎是法国首都" |
| **Procedural memory（程序记忆）** | 关于"怎么做某件事"的记忆 | "技能 / 操作记忆"，例如"怎么骑车" |
| **Semantic memory（语义记忆）** | 关于概念和意义的记忆 | "概念知识" |

---

## 🌊 信号 / 情绪 / 内感受层

Engram 不只是存记忆，还有"情绪"和"身体感受"层 — 这是 potato 比所有竞品都激进的地方。

| 术语 | 一句话解释 | 大众版 |
|---|---|---|
| **Interoception（内感受）** | 大脑感知"自己内部状态"的能力（疲劳、紧张、舒服） | "AI 的'体感'" |
| **Interoceptive layer** | Engram 里专门追踪"agent 自己内部状态"的层 | "AI 的内在状态层" |
| **Somatic markers（躯体标记）** | Damasio 的理论：身体感受会给决策打"情绪标签" | "情绪记号" |
| **Valence（效价）** | 情绪的正负方向（好 / 坏 / 中性） | "情绪正负倾向" |
| **EmotionBus** | Engram 内部传递情绪信号的总线 | "情绪信号通道" |
| **Anomaly（异常度）** | 当前情况跟过去模式偏离了多少 | "当下有多反常" |
| **Confidence（置信度）** | AI 对当前判断有多大把握 | "把握程度" |
| **Alignment（对齐度）** | 当前行为跟核心目标有多匹配 | "跟初心的契合度" |
| **Drive（驱动力）** | Agent 的核心目标 / 价值观 | "AI 的核心追求" |

> 💡 **Marketing tip**：这一节的术语是 Engram 区别于"普通 RAG / 向量库"的关键卖点。竞品都没这层。讲故事时要重点突出。

---

## 📚 记忆操作类

| 术语 | 一句话解释 | 大众版 |
|---|---|---|
| **Recall（回忆）** | 根据查询从记忆库取出相关记忆 | "回忆 / 想起" |
| **Store（存储）** | 把新信息存进记忆库 | "记下来" |
| **Consolidation（巩固）** | 把短期记忆整理成长期记忆的过程（人睡觉时大脑做这事） | "记忆固化" |
| **Knowledge Compiler** | Engram 里把零散记忆合并成"知识主题页"的模块 | "记忆整理器" |
| **Knowledge topic** | 同主题记忆合并后形成的"主题页" | "主题知识页" |
| **Decay（衰减）** | 记忆随时间自然减弱 | "记忆淡化" |
| **Reinforcement（强化）** | 记忆被使用后会变强 | "越想越清晰" |
| **Retrieval（检索）** | 从记忆库里把信息取出来的过程 | "调取记忆" |
| **Injection（注入）** | 把检索出来的记忆塞进 AI 的当前上下文 | "把记忆喂给 AI" |
| **Quarantine（隔离）** | 不确定要不要保留的记忆，先放隔离区观察 | "记忆缓冲区" |

---

## 🆚 跟竞品对比常用词

Engram 的 marketing 经常要对比"普通做法 vs Engram 做法"。

| 普通做法 | Engram 做法 | 一句话讲清差异 |
|---|---|---|
| **Vector database（向量数据库）** | Cognitive memory（认知记忆） | "向量库只会按相似度搜，Engram 会按激活度、关联强度、新旧程度综合判断" |
| **RAG (Retrieval-Augmented Generation)** | Cognitive recall | "RAG 是死的关键词搜索，Engram 是活的认知检索" |
| **Chat history / context window** | Episodic + Working memory | "聊天历史是流水账，Engram 把它分层成长期 / 短期记忆" |
| **Embeddings only** | Embeddings + Activation + Hebbian | "光靠语义相似不够，还要看用过多少次、跟啥关联紧" |
| **No forgetting** | Ebbinghaus forgetting | "AI 永不遗忘 = 上下文越塞越满；自然遗忘 = 永远清爽" |

---

## 🏗️ Engram 架构层次

| 层 | 工程师版 | 大众版 |
|---|---|---|
| **Layer 1: Memory Store** | ACT-R + Hebbian 驱动的记忆库 | "记忆本身住在哪里" |
| **Layer 2: Knowledge Compiler** | 后台合并零散记忆成主题页 | "夜里整理记忆的'清洁工'" |
| **Layer 3: Knowledge Store** | 主题页存储 | "整理好的知识图书馆" |
| **Interoceptive Layer** | 实时追踪 agent 内部状态 | "AI 的'体感监控'" |

---

## 🔬 进阶 / 学术对照

如果有人问 "Engram 跟 X 比有什么区别"，这里是常见的几个对标：

| 对标对象 | 是什么 | 跟 Engram 的关系 |
|---|---|---|
| **Mem0** | 另一个 agent 记忆开源项目 | 直接竞品。Mem0 是 Python，Engram 是 Rust，更深入认知科学 |
| **Letta (前 MemGPT)** | UC Berkeley 出的 agent 长记忆方案 | 更偏"无限上下文"，Engram 更偏"类脑认知" |
| **GEPA** | Stanford 的 prompt 优化方法 | 名字像 — 但 GEPA 改 prompt，Engram 管记忆。两者可以叠加 |
| **GEP / Gene Evolution Protocol** | 把经验压缩成"基因"，运行时注入 | 思路跟 Engram 互补 — Engram 未来 v0.3 可能往这个方向加层 |
| **A-Mem, Cognee, Zep** | 其他 agent 记忆系统 | Engram 区别：Rust 原生 + 神经科学根基 + 内感受层 |

---

## 🎯 Engram 故事框架（marketing 套路）

写 Engram 的内容，几乎都可以套这个三段：

**1. 痛点（普通 AI 记忆有什么毛病）**
> "现在的 AI agent 记忆就是把聊天记录塞进向量库，搜出来再喂回去 — 这根本不是记忆，是搜索。"

**2. 类比人脑（Engram 怎么不一样）**
> "人脑不是这样工作的。人脑会遗忘（Ebbinghaus），会联想（Hebbian），最近用过的会更鲜活（ACT-R）。Engram 把这些机制原生实现了出来。"

**3. 实际效果（用户能拿到什么）**
> "结果就是 AI 越用越懂你，但上下文不会越塞越满；AI 会自动联想到相关的事，但又不会被无关信息干扰。"

---

## ✏️ 常见误解 cheat

- ❌ "Engram 就是个记忆数据库" → ✅ Engram 是**认知架构**，记忆只是其中一层
- ❌ "记忆永远记住才好" → ✅ **会遗忘**才是 feature 不是 bug — 不遗忘 = 上下文爆炸
- ❌ "Hebbian 就是给记忆打标签" → ✅ Hebbian 是**两段记忆因为共同出现而自动建立联系**，跟标签不一样
- ❌ "ACT-R 是个算法" → ✅ ACT-R 是**整套人类认知理论**，Engram 用了它的"激活扩散"那部分
- ❌ "Engram 跟 RAG 一样" → ✅ RAG 只看语义相似，Engram 还看激活强度、关联强度、时间衰减

---

## 🗣️ 不同受众的开场白模板

**给 AI 工程师**：
> "Engram is a Rust-native cognitive memory engine for AI agents. ACT-R activation, Hebbian learning, Ebbinghaus forgetting — all primitives, no Python tax."

**给创始人 / PM**：
> "现在 AI agent 的记忆都是死的搜索。Engram 让它变成活的认知 — 会遗忘、会联想、越用越懂你。"

**给投资人 / 大众**：
> "Engram 让 AI 像人一样记忆 — 该记的记住，不重要的自然忘掉，相关的事会自动联想到一起。"

---

*最后更新：2026-04-26*
*配套文件：`marketing/glossary-for-phoebe.md`（GID 通用版）*
*有新术语随时让 RustClaw 加进来。*
