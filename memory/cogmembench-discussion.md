# CogMemBench — 完整讨论记录 & 设计决策

> 从Part 1分析到Part 2设计的全部讨论脉络，包括关键争论和转向。
> 最后更新: 2026-04-20

---

## 一、起点：Part 1 竞品分析

### 现有benchmark全景

| Benchmark | 来源 | 测什么 | 不测什么 |
|---|---|---|---|
| **LongMemEval** (2024) | 微软 | 长对话retrieval accuracy（5能力维度：information extraction, multi-session reasoning, temporal reasoning, knowledge updates, abstraction & synthesis） | 不测记忆系统内部运作，只测LLM能不能从context里找到正确答案 |
| **LoCoMo** (2024) | — | 超长对话（35个session）下的记忆保持 | 同上，测的是LLM+RAG的pipeline |
| **MemEvoBench** (2025) | — | LLM在被污染记忆池中的决策安全性。3种攻击：misleading injection、noisy tool returns、biased user feedback | 不测记忆系统——记忆系统只是context来源。测的是LLM会不会被骗 |
| **AMemGym** (ICLR 2026) | 复旦 | 交互式长对话中的记忆操作——分write/read/utilization三阶段。有"state evolution"和diagnostic评估 | 测的是memory policy（什么时候写、什么时候读），不测记忆内部的认知特性 |
| **Hindsight** (2025) | Vectorize.io | 不是benchmark而是系统，但在LongMemEval+LoCoMo上跑分。4个network(world/experience/opinion/observation) + 3个operation(retain/recall/reflect) | 跑的还是LongMemEval/LoCoMo的指标 |
| **MemoryBench** (2025) | — | 从反馈中持续学习 | 窄focus |
| **Memory-R1** (2025) | — | RL训练agent做memory操作 | 不测认知特性 |

### 核心发现

**所有现有benchmark都在Layer 0（存取能力）打转。** 没有任何benchmark测试：
- 遗忘曲线是否符合认知科学预测
- 关联结构（Hebbian）的质量
- 置信度校准（ECE）
- 干扰抵抗（PI/RI）
- 自我状态监控与策略调整

engram实现了认知科学的5个层次（temporal dynamics → associative structure → metacognition → synthesis → self-regulation），但现有benchmark只覆盖了最底层~10%。

---

## 二、关键转向：从"像人脑"到"解决真实问题"

### potato的challenge（决定性讨论）

**Q1: "出了benchmark，如何证明engram是更好的记忆系统？"**

初始思路（被否定）：
- 测engram的forgetting curve R²是否符合Ebbinghaus → 证明它"像人脑"
- 审稿人会反驳："AI没有生物硬件限制，为什么要模拟遗忘？完美记住一切不是更好？"
- 这个反驳是致命的——用"像人脑所以好"来论证是循环论证

**Q2: "retrieval accuracy重要是显而易见的，但遗忘曲线R²重要不是显而易见的。而且engram不是真的遗忘，只是activation score降低导致排序靠后。"**

这彻底改变了设计方向。

### 最终定位

**不是"验证有多像人脑"，而是"验证能真的解决各种工程问题"。**

每个认知模块映射到一个工程问题：
| 认知模块 | 解决的工程问题 |
|---|---|
| ACT-R activation | 记忆量增大时信噪比维持（noise floor上升） |
| Temporal decay | 旧信息被高频数据压制（stale info dominance） |
| Hebbian learning | 分散的信息无法被关联（association blindness） |
| Confidence calibration | 系统对错误信息过度自信（overconfidence） |
| Consolidation | context window有限时选什么放进去（capacity pressure） |
| Interoceptive | 连续失败不知道换策略（blind repetition） |

**benchmark测的是工程性能，认知科学只用来解释why it works。**

---

## 三、Part 2 设计：6个Level

### Level 1: Signal-to-Noise（信噪比维持）
- **问题**: 记忆越多，检索越不准。embedding空间拥挤
- **Failure mode**: 查"上次出差去哪"，返回7/10条无关结果（共享关键词但语义不同）
- **Metric**: Precision@5, Precision@5 Degradation (P@5_1K - P@5_50K), Noise Rejection Rate
- **规模梯度**: 1K/10K/50K/100K条记忆

### Level 2: Temporal Sensitivity（时态敏感性）
- **问题**: 用户状态变了，系统还返回旧信息
- **Failure mode**: 说了10次"用React"（3月前），说了1次"转Vue了"（上周）→系统答React
- **Metric**: Temporal Accuracy, Update Latency（需要多少次提及才能覆盖旧版本）
- **变量矩阵**: 旧频率×新频率×时间间隔

### Level 3: Interference Resistance（干扰抵抗）
- **问题**: 相似domain的记忆互相污染
- **Failure mode**: 问Project A的数据库→混入Project B的数据库记忆
- **测试类型**: Retroactive Interference (RI), Proactive Interference (PI), Similarity-Based
- **Metric**: Cross-contamination Rate, RI Score, PI Score

### Level 4: Context Assembly Quality（上下文组装质量）
- **问题**: 从10万条里选K条放进context，选错了回答就错
- **Failure mode**: 需要A+B+C+D才能回答的问题，只检索到A+B
- **测试类型**: Multi-hop Assembly, Contradiction-aware, Capacity Pressure
- **Metric**: Coverage Score, Redundancy Rate, Contradiction Inclusion, Assembly F1

### Level 5: Calibrated Confidence（校准置信度）
- **问题**: 系统说"95%确定"但其实是猜的
- **Failure mode**: 从模糊对话推断生日→输出confidence 0.95→用户信了→错了
- **Metric**: ECE (Expected Calibration Error), Overconfidence Rate, Source Attribution Accuracy

### Level 6: Adaptive Behavior Under Pressure（压力下适应行为）
- **问题**: 连续失败不换策略，压力下质量断崖式下降
- **Failure mode**: 连续5次检索不相关结果，继续同样策略
- **测试类型**: Degradation Detection, Strategy Adaptation, Recovery
- **Metric**: Anomaly Detection Accuracy, Strategy Switch Rate, Recovery Time, Graceful Degradation Score

---

## 四、竞品Overlap分析（重要！）

逐Level对比已有benchmark的覆盖情况：

### L1 Signal-to-Noise at Scale — ✅ 空白
- 所有benchmark都是固定规模测试
- 没有人测precision随记忆量增长的退化曲线
- AMemGym的context length可配，但没做规模梯度对比
- **完全差异化**

### L2 Temporal Sensitivity — ⚠️ 有overlap
- LongMemEval有"temporal reasoning"和"knowledge updates"能力维度
- AMemGym核心就是测"state evolution"
- Hindsight也在temporal reasoning上跑分
- **差异化角度**: 他们测"能不能知道更新了"，我们测"频率vs近因性冲突时的权衡行为"
- 例如：旧信息被提及10次 vs 新信息被提及1次——这个冲突场景没人测过

### L3 Interference Resistance — ✅ 空白
- PI/RI在AI memory benchmark里零覆盖
- MemEvoBench测adversarial injection（外部攻击），不是自然interference
- 认知心理学的核心概念，AI领域完全没有对应benchmark
- **完全差异化**

### L4 Context Assembly Quality — ⚠️ 有部分overlap
- LongMemEval的"multi-session reasoning"涉及组装多条信息
- AMemGym的utilization阶段测记忆使用质量
- **差异化角度**: 没人测"context budget限制下的选择质量"和"redundancy控制"
- 以及"矛盾记忆是否被一起包含"

### L5 Calibrated Confidence — ✅ 空白
- Hindsight有confidence scores，但没有benchmark测ECE
- 没有任何benchmark问"说80%确定的事，真的80%对吗"
- **完全差异化**

### L6 Adaptive Behavior — ✅ 空白
- AMemGym有"self-evolution"概念但是优化memory policy，不是实时自监控
- 没有benchmark测"系统知不知道自己在失败"
- **完全差异化**

### 汇总

| Level | Overlap程度 | 竞品 | 我们的差异化 |
|---|---|---|---|
| L1 Signal-to-Noise | ✅ 空白 | 无 | 规模梯度退化曲线 |
| L2 Temporal Sensitivity | ⚠️ 部分 | LongMemEval, AMemGym | 频率vs近因性冲突 |
| L3 Interference (PI/RI) | ✅ 空白 | 无 | 认知科学核心概念首次引入 |
| L4 Context Assembly | ⚠️ 部分 | LongMemEval, AMemGym | budget限制+redundancy+矛盾处理 |
| L5 Calibration (ECE) | ✅ 空白 | 无 | 首个测记忆confidence校准的benchmark |
| L6 Adaptive Behavior | ✅ 空白 | 无 | 首个测记忆系统自监控的benchmark |

**6个Level中4个完全空白，2个有部分overlap但差异化明确。**

---

## 五、待决策问题

### Q1: 保留L2和L4还是砍掉？

**保留的理由：**
- 有overlap不代表redundant——我们测的角度确实不同
- L2的"频率vs近因性冲突"和L4的"budget-constrained selection"是genuinely new
- 6个Level构成完整认知评估，砍掉会有gap

**砍掉的理由：**
- 审稿人可能说"temporal sensitivity AMemGym已经测了"
- 论文更容易被accept如果每个维度都是100% novel
- 集中精力做4个完全空白的Level可能更有冲击力

**倾向：保留，但要在论文中精确说明差异。**
- L2: 不是"能不能detect update"（AMemGym测了），而是"频率和近因性冲突时的决策质量"
- L4: 不是"能不能multi-hop"（LongMemEval测了），而是"有K条budget时的信息选择最优性"

### Q2: 评估方法

**重要优势：所有Level的ground truth可以从构造过程推导，不需要人工标注。**
- 这是因为benchmark是synthetic的——我们控制注入的数据，所以知道答案
- 这让benchmark完全可自动化、可复现

### Q3: Baselines选择

计划的6个baseline：
1. Naive RAG（embedding + cosine top-K）
2. RAG + Recency（加线性时间衰减）
3. MemGPT/Letta
4. A-MEM
5. Zep/Mem0
6. engram

注意：Hindsight也应该作为baseline——它是目前LongMemEval上的SOTA。

### Q4: 论文投稿目标

待讨论。CogMemBench的定位适合：
- NeurIPS 2026 (Datasets & Benchmarks track)
- EMNLP 2026
- ICLR 2027

---

## 六、设计原则总结

1. **每个test = 真实工程问题 + failure mode + 量化指标**
2. **不依赖"像人脑所以好"——测工程性能，理论解释why**
3. **Ground truth从构造推导，不需要人工标注**
4. **规模梯度（1K→100K），小规模大家差不多，大规模见真章**
5. **不做单一总分——6维雷达图，不同应用关注不同维度**
6. **不重复已有benchmark——LongMemEval测retrieval，MemEvoBench测safety，我们测cognitive function**

---

## 七、文件索引

| 文件 | 内容 |
|---|---|
| `memory/cogmembench-analysis.md` | Part 1 完整分析（认知层次、盲区矩阵、MemEvoBench精确定位） |
| `memory/cogmembench-part2-design.md` | Part 2 benchmark架构设计（6 Levels详细定义） |
| `memory/cogmembench-discussion.md` | **本文件**：全部讨论脉络 + 竞品overlap + 待决策问题 |
