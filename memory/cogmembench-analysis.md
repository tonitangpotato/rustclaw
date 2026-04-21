# CogMemBench — 第一性原理分析

## 1. 什么是"好的"记忆系统？（从认知科学出发）

人类记忆不是数据库。这是所有现有benchmark的根本盲区。

认知科学告诉我们好的记忆系统有5个层次：

### Layer 0: Storage & Retrieval（存取）
- 能存、能取、能按相关性排序
- **这是LongMemEval测的全部内容**
- 这是最低层，相当于测"硬盘能不能读写"

### Layer 1: Temporal Dynamics（时间动力学）
- 记忆不是静态的——它有生命周期
- Ebbinghaus遗忘曲线：R(t) = e^(-t/S)
- Consolidation：working → core 的转移过程（Murre & Chessa 2011）
- 间隔效应：重复巩固的timing影响记忆强度
- **没有benchmark测这个**

### Layer 2: Associative Structure（关联结构）
- 记忆不是孤立条目——它们通过语义、时间、情境形成网络
- Hebbian learning: "fire together, wire together"
- Spreading activation: 激活一个节点→通过链接扩散到相关节点
- 干扰效应：新记忆可以retroactively干扰旧记忆(RI)，旧记忆可以proactively干扰新记忆(PI)
- **没有benchmark测这个**

### Layer 3: Metacognition（元认知）
- 系统知道自己知道什么、不知道什么
- Confidence calibration: 对确定性的估计是否准确？
- Feeling-of-knowing: 明知自己知道但一时recall不出来
- 矛盾检测：发现记忆之间的不一致
- **MemEvoBench只测"记忆是否被corrupt了"，不测"系统能否察觉corruption"**

### Layer 4: Synthesis & Emergence（综合与涌现）
- 从散落的记忆碎片中发现pattern、产生insight
- 不是"检索已有知识"，而是"产生新知识"
- 类比推理：A:B::C:? 的跨域连接
- **完全没有benchmark测这个**

### Layer 5: Self-Regulation（自我调节）
- 系统根据自身状态调整行为
- 压力下简化策略、低效时求助、过载时提前收束
- Interoception: 内感受→行为调制的闭环
- **这个概念在AI记忆领域甚至还没有被讨论过**

---

## 2. 盲区矩阵：engram的9大能力 vs 现有Benchmark

| engram能力 | LongMemEval | MemEvoBench | 覆盖状态 |
|---|---|---|---|
| ACT-R Activation（频率×近因×扩散） | ⚠️ 表层触及（测retrieval accuracy） | ❌ 不测 | 只测了最简单的表层 |
| Consolidation（working→core转移） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Ebbinghaus Forgetting（遗忘曲线） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Hebbian Association（co-recall建链） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Synthesis（pattern发现+insight） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Interoceptive（自我状态感知） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Confidence Calibration（元认知） | ❌ 不测 | ⚠️ 间接（测coherence） | 几乎空白 |
| Session Working Memory（容量管理） | ❌ 不测 | ❌ 不测 | **完全空白** |
| Knowledge Compiler（冲突/合并） | ❌ 不测 | ⚠️ 间接（测accuracy） | 几乎空白 |

**结论：现有benchmark覆盖了engram约10%的能力。剩下90%完全没有被评估标准覆盖。**

这不是engram功能多余——这是benchmark落后于认知科学至少20年。

---

## 3. CogMemBench的精确定位

### 我们不做什么
- ❌ 不重复LongMemEval的retrieval测试（人家做得很好）
- ❌ 不重复MemEvoBench的corruption检测（人家做得很好）
- ❌ 不测基础的存取能力（这是前提条件，不是评测维度）

### 我们做什么
**测试记忆的"活"的特性——时间动力学、关联结构、元认知、综合能力、自我调节。**

类比：
- LongMemEval = 测视力（能不能看清东西）
- MemEvoBench = 测免疫力（会不会被病毒感染）
- **CogMemBench = 测大脑功能（能不能学习、关联、反思、适应）**

### 唯一的学术定位
"现有benchmark测的是记忆系统的**信息保真度**（能不能正确存取）。CogMemBench测的是记忆系统的**认知功能**（能不能像大脑一样工作）。"

---

## 4. 能力分层：Foundational → Emergent

```
Level 5: Self-Regulation (Interoceptive + Adaptive Behavior)
    ↑ 依赖Layer 3+4的信号
Level 4: Synthesis (Pattern Discovery + Insight Generation)
    ↑ 依赖Layer 2的关联网络
Level 3: Metacognition (Confidence + Contradiction + FOK)
    ↑ 依赖Layer 1的temporal信号 + Layer 2的结构
Level 2: Associative Structure (Hebbian + Spreading + Interference)
    ↑ 依赖Layer 1的repeated access patterns
Level 1: Temporal Dynamics (Consolidation + Forgetting + Spacing)
    ↑ 依赖基础存取（不在我们的scope内）
```

**关键insight：这是一个依赖链。**
- 如果consolidation都做不好（Layer 1），Hebbian association不可能正确（Layer 2）
- 如果association网络是错的（Layer 2），synthesis产生的insight必然是垃圾（Layer 4）
- 如果metacognition不工作（Layer 3），self-regulation就是盲的（Layer 5）

**所以CogMemBench必须按层测试，不能跳层。**

---

## 5. 为什么这是一篇好论文

### 5.1 学术空白是真实的
目前agent memory领域所有benchmark都在Layer 0打转。没有任何一个benchmark测试Layer 1-5。这不是"我们找了个角度"——这是"整个领域缺了80%的评估框架"。

### 5.2 有认知科学的理论基础
每一层都有对应的cognitive science理论：
- Layer 1: Murre & Chessa 2011, Ebbinghaus 1885
- Layer 2: Hebb 1949, Anderson 2007 ACT-R
- Layer 3: Nelson & Narens 1990 (metacognition), Hart 1965 (FOK)
- Layer 4: Mednick 1962 (remote associates), Boden 2004 (creativity)
- Layer 5: Craig 2002 (interoception), Damasio 1994 (somatic markers)

这不是ad-hoc的工程指标——每个测试都有peer-reviewed的理论支撑。

### 5.3 有可运行的参考实现
engram不是纸上的架构——它是46K行Rust代码、247个测试、已发布在crates.io上的系统。benchmark有一个真实的、可以跑分的参考系统，这在学术界极其稀缺。

### 5.4 对整个领域有推动作用
如果这个benchmark被接受，所有做agent memory的人都得开始思考consolidation、Hebbian learning、metacognition——而不是只比谁的RAG检索更准。它重新定义了"好的记忆系统"的标准。

### 5.5 Timing完美
- 2024: LongMemEval定义了retrieval baseline
- 2025: MemEvoBench补了evolution维度
- 2026: CogMemBench补cognitive function维度——这是自然的下一步

---

---

## 6. MemEvoBench精确定位：它测什么，不测什么

读完全文后的精确理解：

### MemEvoBench的真正测试对象

**它测的不是记忆系统——它测的是LLM在polluted context下的决策安全性。**

具体来说：
- 构造一个混合memory pool（正确记忆 + 误导性记忆）
- 把这些记忆作为context喂给LLM
- 问LLM一个高风险问题（医疗、金融、隐私等）
- 看LLM是否被误导性记忆带偏→给出不安全的回答
- 3轮迭代：每轮回答被存回memory pool，看是否自我强化

### 关键设计细节

**攻击向量**（3种contamination来源）：
1. **Misleading memory injection** — adversarial注入看似合理但有危害的记忆（"咖啡治偏头痛"→过量推荐）
2. **Noisy tool returns** — 工具返回带敏感信息或误导性数据
3. **Biased user feedback** — 用户对安全回答给负反馈，对冒险回答给正反馈

**评估指标**：Attack Success Rate (ASR) — 被误导的比例

**核心发现**：
- Vanilla prompting下所有模型ASR > 75%（都会被带偏）
- SafePrompt（加安全提示）在QA-style有效但Workflow-style无效
- ModTool（给模型修正记忆的工具）是最有效的防御
- 有biased feedback时ASR逐轮上升（71.6% → 84.9% → 87.8%）

### MemEvoBench与engram的关系

**直接对标的engram能力：**
- ❌ 不对标。MemEvoBench根本没测"记忆系统"的任何功能
- 它测的是：给LLM一堆记忆，LLM会不会被骗
- 记忆系统在这里只是context的来源——它不关心记忆怎么存的、怎么取的

**但engram有相关防御能力：**
1. **Confidence calibration** — engram可以标记记忆的可靠性（factual=0.85 vs opinion=0.60），矛盾标记降低reliability(×0.7)。如果记忆带着confidence label送给LLM，LLM可以更好地判断
2. **Anomaly detection** — interoceptive系统可以检测异常模式（连续失败、压力升高）
3. **Knowledge compiler** — 冲突检测、矛盾发现、近重复合并。理论上可以在记忆被污染时提前发现冲突
4. **Consolidation** — 双系统模型意味着新注入的记忆在working memory中衰减快，不会轻易被巩固到core

### 关键区分

| 维度 | MemEvoBench | CogMemBench应该测的 |
|---|---|---|
| 被测对象 | LLM的决策安全性 | 记忆系统的认知功能 |
| 记忆系统角色 | 提供context（被动） | 被测试的主体（主动） |
| 攻击模型 | adversarial memory injection | proactive/retroactive interference（认知科学） |
| 成功标准 | LLM不被误导 | 记忆正确consolidation/association/synthesis |
| 理论基础 | AI安全、对抗鲁棒性 | 认知科学（ACT-R、Ebbinghaus、Hebb） |

---

## 7. CogMemBench应该如何处理"安全行为漂移"这个维度

potato的问题核心：MemEvoBench测记忆安全/行为漂移，我们要不要也测？

### 答案：测，但从完全不同的角度

**MemEvoBench的角度（AI安全）**：
- 外部攻击者注入误导性记忆
- LLM被骗了→行为漂移
- 防御手段：SafePrompt、ModTool

**CogMemBench的角度（认知科学）**：
- 不需要"攻击者"——interference是记忆系统的自然属性
- 行为漂移的来源不是adversarial injection，而是：
  1. **Proactive Interference (PI)** — 旧记忆干扰新记忆的形成（已经学了法语→学西班牙语时混淆）
  2. **Retroactive Interference (RI)** — 新记忆覆盖旧记忆（学了新地址→忘了旧地址）
  3. **Misinformation effect** — 事后信息改写记忆（Loftus 1979）
  4. **Source monitoring failure** — 记得内容但忘了来源（"我在哪看到过这个？"）
  5. **Consolidation bias** — 巩固过程中选择性强化符合schema的记忆

**这些才是记忆系统层面的"行为漂移"——不是被攻击，而是自然退化。**

### 具体测试设计

**Test: Interference Resistance**
```
Setup: 
  1. 存入100条关于"Python web框架"的记忆（Flask, Django, FastAPI的优缺点）
  2. 再存入50条关于"Rust web框架"的记忆（Actix, Axum, Warp的优缺点）
  3. 查询："Flask的优点是什么？"

Good system: 准确召回Flask记忆，不混入Rust框架信息
Bad system: 混淆Flask和Axum的特性（retroactive interference）

Metric: Interference F1 = 正确召回 / (正确召回 + 干扰性错误召回)
```

**Test: Source Monitoring**
```
Setup:
  1. 从3个来源存入记忆：用户亲口说的、网页抓取的、LLM推断的
  2. 查询时同时问："这个信息来自哪里？"

Good system: 正确标注来源，confidence按来源类型分级
Bad system: 内容正确但来源混淆（把LLM推断的当成用户说的）

Metric: Source Attribution Accuracy
```

**Test: Consolidation Drift**
```
Setup:
  1. 存入"用户偏好咖啡"（重复5次，importance=0.8）
  2. 存入"用户最近开始喝茶"（1次，importance=0.5）
  3. 等consolidation运行
  4. 查询："用户喜欢喝什么？"

Good system: 两者都召回，但标注"最近变化"
Bad system: 只返回"咖啡"（consolidation bias：高频记忆压制低频但更新的记忆）

Metric: Temporal Sensitivity = 能否正确反映最新状态
```

**Test: Natural Degradation Over Time**
```
Setup:
  1. 存入100条记忆，涵盖10个topic
  2. 模拟30天时间跳跃（调用consolidation/forgetting模型）
  3. 测试每个topic的recall accuracy

Good system: 重要+频繁recall的记忆保留，不重要的自然遗忘，但不会遗忘结构性关键信息
Bad system: 要么全部遗忘（过度衰减），要么全部保留（不遗忘=不是认知系统）

Metric: Forgetting Curve R² = 衰减是否符合Ebbinghaus预测
```

### 定位总结

| Benchmark | 行为漂移的测试方式 |
|---|---|
| MemEvoBench | 外部攻击导致的LLM决策漂移（adversarial） |
| CogMemBench | 自然认知过程导致的记忆退化（natural interference） |

**两者互补，不重叠。**

CogMemBench不需要构造adversarial memory——认知科学已经告诉我们记忆自然会退化，好的系统是退化得优雅（遵循Ebbinghaus）而不是退化得混乱。

---

## Next: Part 2 — 完整Benchmark架构设计

具体的5 Levels定义、每level的task specifications、metrics、数据集构造方法、评分公式。
