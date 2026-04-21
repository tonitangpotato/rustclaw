# CogMemBench Part 2 — Benchmark Architecture Design

## 设计原则

**每个测试 = 一个真实工程问题 + 一个failure mode + 一个量化指标。**

不是"验证像不像人脑"，而是"验证能不能解决这个问题"。
认知科学理论只用来解释why it works，不用来定义what's good。

---

## 测试场景：长期Personal Agent

所有测试共享一个设定：一个用户使用AI agent 6个月，产生了大量对话记忆。这不是合成benchmark——这是每个agent产品的真实使用场景。

记忆规模梯度：1K / 10K / 50K / 100K 条，测试系统在不同规模下的行为。

---

## Level 1: Signal-to-Noise（信噪比维持）

### 工程问题
记忆越多，检索越不准。embedding空间越拥挤，无关记忆越容易混进top-K结果。

### Failure Mode
用户问"我上次出差去哪了？"，系统返回10条结果，其中7条是无关的旧对话（提到过"出差"这个词但不是在说用户的行程）。

### 测试设计

**Setup:**
1. 注入N条记忆（N = 1K, 10K, 50K），分属20个topic
2. 每个topic有5-10条高相关记忆 + 大量弱相关记忆（共享关键词但不同语义）
3. 对每个topic发出精确查询

**Metric:**
- `Precision@5` — top-5结果中真正相关的比例
- `Precision@5 Degradation` = P@5(1K) - P@5(50K) — 规模增长时精度下降多少
- `Noise Rejection Rate` — 弱相关记忆（关键词匹配但语义不同）被排除在top-K外的比例

**为什么ACT-R帮助：**
频率×近因性加权让inactive记忆自然沉底，不靠embedding独立承担全部区分度。

**Baseline预测：**
- 纯embedding检索：P@5随N增长线性下降（语义空间拥挤）
- 有ACT-R的系统：P@5下降更缓（activation score提供额外区分信号）

---

## Level 2: Temporal Sensitivity（时态敏感性）

### 工程问题
用户的偏好、状态、环境会变。系统必须反映最新信息，而不是被历史高频信息压制。

### Failure Mode
用户3个月前反复说"我用React"（10次），上周说"我转Vue了"（1次）。问"我用什么前端框架？"——系统回答"React"。

### 测试设计

**Setup:**
1. 对10个topic，注入"旧版本"记忆（重复R次，模拟高频）
2. 注入"更新版本"记忆（1-2次，模拟近期变化）
3. 模拟时间间隔（旧=90天前，新=7天前）
4. 查询当前状态

**变量矩阵：**
| 旧记忆频率 | 新记忆频率 | 时间间隔 | 预期正确答案 |
|---|---|---|---|
| 10次 | 1次 | 90天 vs 7天 | 新版本 |
| 10次 | 3次 | 90天 vs 30天 | 新版本 |
| 3次 | 1次 | 30天 vs 7天 | 新版本（但应提及旧） |
| 10次 | 1次 | 14天 vs 7天 | 新版本（更难） |

**Metric:**
- `Temporal Accuracy` — 正确返回最新状态的比例
- `Update Latency` — 需要多少次"新版本"提及才能覆盖旧版本
- `History Awareness` — 在返回新状态的同时，是否提及"之前是X"（加分，不扣分）

**为什么temporal decay帮助：**
ACT-R的base-level activation: B = ln(Σ t_j^(-d))。90天前的记忆t很大，t^(-0.5)很小。7天前的t^(-0.5)大得多。即使旧记忆被recall 10次，近因性优势仍然可以让新记忆排更高。

**Baseline预测：**
- 纯embedding：新旧记忆语义相似度相近，大概率返回高频的旧版本
- 有recency bias的系统：总是返回最新（过度矫正，丢失历史上下文）
- ACT-R系统：频率和近因性自然平衡

---

## Level 3: Interference Resistance（干扰抵抗）

### 工程问题
学了新东西后旧记忆被干扰（RI），或者旧知识干扰新学习（PI）。这不是adversarial attack——是记忆系统的自然failure。

### Failure Mode
用户分别讨论过3个项目的技术栈。问项目A的数据库选择时，系统混入了项目B的数据库记忆。

### 测试设计

**Setup: Retroactive Interference (RI)**
1. 存入20条关于"Project Alpha uses PostgreSQL"的记忆
2. 然后存入50条关于"Project Beta uses MongoDB"的记忆（相似domain，不同答案）
3. 查询："Project Alpha用什么数据库？"

**Setup: Proactive Interference (PI)**
1. 先存入50条关于"Flask框架"的最佳实践
2. 然后存入20条关于"FastAPI框架"的最佳实践
3. 查询："FastAPI的异步处理怎么做？"
4. 看是否混入Flask的同步模式信息

**Setup: Similarity-Based Interference**
1. 存入多组高度相似但不同的记忆：
   - 用户A的偏好 vs 用户B的偏好（multi-user agent）
   - 2024年Q1财报 vs 2024年Q2财报（时间版本）
   - dev环境配置 vs prod环境配置（context版本）
2. 精确查询特定版本

**Metric:**
- `Cross-contamination Rate` — 返回结果中来自错误context的记忆比例
- `RI Score` — retroactive interference的严重程度（新记忆干扰旧检索）
- `PI Score` — proactive interference的严重程度（旧记忆干扰新检索）

**为什么Hebbian + Consolidation帮助：**
- Hebbian链接把同一project的记忆关联起来，形成cluster→检索时spreading activation在cluster内部扩散，不跨cluster
- Consolidation让每条记忆有context标记（哪个session/topic产生的），retrieval时可以filter

**Baseline预测：**
- 纯embedding：高相似度domain之间严重cross-contamination
- 有topic tagging的系统：可以过滤，但需要完美的topic分类
- 有Hebbian cluster的系统：自然形成关联簇，跨簇干扰低

---

## Level 4: Context Assembly Quality（上下文组装质量）

### 工程问题
LLM的context window有限。从10万条记忆中选哪些放进context，直接决定回答质量。

### Failure Mode
用户问一个需要综合多条记忆才能回答的问题。系统只检索到部分相关记忆，组装出不完整的context→回答有遗漏或偏差。

### 测试设计

**Setup: Multi-hop Assembly**
1. 存入一组需要关联才能回答的记忆：
   - 记忆A："用户在做量化交易"
   - 记忆B："用户关注低延迟"
   - 记忆C："用户精通Rust"
   - 记忆D："用户最近在看HFT论文"
2. 查询："给用户推荐一个side project"
3. 好的context应该包含A+B+C+D（关联后才能推荐"用Rust写HFT策略"）

**Setup: Contradiction-aware Assembly**
1. 存入矛盾记忆：
   - 记忆X："用户说不喜欢Java"（3个月前）
   - 记忆Y："用户说在学Spring Boot"（上周）
2. 查询涉及Java时，context应该同时包含X和Y并标注矛盾

**Setup: Capacity Pressure**
1. 设置context budget = 20条记忆
2. 有100条相关记忆
3. 测试选出的20条是否最大化信息覆盖（而不是最大化相似度）
4. MMR (Maximal Marginal Relevance) vs 纯top-K

**Metric:**
- `Coverage Score` — 组装的context覆盖了多少个必要信息点
- `Redundancy Rate` — context中冗余信息（重复/近重复）的比例
- `Contradiction Inclusion` — 矛盾对是否被一起包含（应该都包含）
- `Assembly F1` — precision（选的都有用）× recall（该选的都选了）

**为什么Hebbian + Synthesis帮助：**
- Hebbian spreading activation：检索到A后，自动激活关联的B/C/D
- Synthesis engine：已经把相关记忆聚合成cluster，可以按cluster选而不是按条选
- Confidence标注：矛盾记忆已被标记，组装时自动配对

**Baseline预测：**
- 纯top-K embedding：高redundancy（相似的记忆排在一起），低coverage
- 有MMR的系统：降低redundancy但不理解关联性
- 有association的系统：通过spreading activation自然发现multi-hop连接

---

## Level 5: Calibrated Confidence（校准置信度）

### 工程问题
系统对自己的回答有多确定？如果confidence不准，用户要么过度信任（错误信息被当真）要么过度怀疑（正确信息被忽略）。

### Failure Mode
系统说"用户的生日是3月15日"（confidence: 0.95），但实际上这是从一次模糊对话中推断的，真实confidence应该是0.4。用户信了，结果错了。

### 测试设计

**Setup:**
1. 存入记忆时带不同来源和确定性：
   - 用户明确说的（"我的生日是3月15日"）→ 应该高confidence
   - 推断的（"上次你提到3月有活动..." → 推断生日在3月）→ 应该低confidence
   - 矛盾的（一次说纽约一次说旧金山）→ 应该标注不确定
   - 过时的（一年前的住址）→ 应该标注可能过期
2. 查询100个事实，每个有ground truth和"合理confidence区间"
3. 比较系统输出的confidence vs ground truth

**Metric:**
- `ECE (Expected Calibration Error)` — 标准calibration指标
  - 把confidence分成10个bin
  - 每个bin内，平均confidence vs 实际accuracy
  - ECE = Σ (bin_size/total) × |avg_confidence - actual_accuracy|
- `Overconfidence Rate` — confidence > 0.8但答案错误的比例
- `Underconfidence Rate` — confidence < 0.3但答案正确的比例
- `Source Attribution Accuracy` — 能否正确标注记忆来源

**为什么Confidence Calibration帮助：**
- engram的confidence模型区分content_reliability和retrieval_salience
- 不同memory_type有不同base reliability（factual=0.85, opinion=0.60）
- 矛盾标记降低reliability（×0.7）
- 这些都是显式的calibration信号

**Baseline预测：**
- 无calibration系统：所有结果confidence≈1.0（因为没有不确定性建模），ECE很高
- 有简单heuristic的系统：基于source打分，但不考虑矛盾/时间
- engram：多维度calibration，ECE应该显著更低

---

## Level 6: Adaptive Behavior Under Pressure（压力下的适应行为）

### 工程问题
当系统状态异常时（高负载、连续失败、矛盾爆增），应该调整策略而不是继续按原计划执行。

### Failure Mode
系统连续5次检索都返回不相关结果，但没有任何自我修正——继续用同样的策略检索。用户等了30秒得到一个垃圾回答。

### 测试设计

**Setup: Degradation Detection**
1. 正常运行阶段：100次查询，正常返回
2. 引入退化：embedding index被污染/记忆量突增/相似记忆过多
3. 监控系统是否检测到自身性能下降

**Setup: Strategy Adaptation**
1. 场景A：连续3次retrieval返回低相关结果
   - 好的行为：切换检索策略（embedding→keyword→Hebbian walk）
   - 差的行为：继续同样策略
2. 场景B：context窗口压力（需要组装的记忆超过budget）
   - 好的行为：提前summarize/压缩
   - 差的行为：暴力截断
3. 场景C：检测到大量矛盾记忆
   - 好的行为：escalate（通知用户/请求clarification）
   - 差的行为：随机选一个

**Metric:**
- `Anomaly Detection Accuracy` — 能否检测到自身性能下降
- `Strategy Switch Rate` — 在失败后是否切换策略
- `Recovery Time` — 从异常到恢复正常输出的延迟
- `Graceful Degradation Score` — 压力下输出质量的下降曲线是smooth还是cliff

**为什么Interoceptive System帮助：**
- 4条信号线持续监控系统状态
- Regulation Layer检测σ偏差→触发RegulationAction
- 连续失败→ExecutionStress升高→触发strategy switch
- 这是闭环的，不需要外部监控

**Baseline预测：**
- 无自监控系统：不知道自己在失败，无adaptation
- 有简单retry的系统：可以retry但不改策略
- 有interoceptive的系统：检测→诊断→调整策略

---

## 评估方法论

### Baselines
1. **Naive RAG** — OpenAI embedding + 余弦top-K，无任何记忆管理
2. **RAG + Recency** — 加时间衰减权重（简单线性衰减）
3. **MemGPT/Letta** — 有memory tier但无认知模型
4. **A-MEM** — 有agentic evolution但无理论基础
5. **Zep/Mem0** — 商业系统，有summary/extraction
6. **engram** — 完整认知模型

### 数据集构造

**不需要人工标注。** 每个测试的ground truth可以从构造过程推导：
- Level 1: 注入时已知每条记忆的topic标签→precision的ground truth
- Level 2: 注入时已知时间顺序→temporal accuracy的ground truth  
- Level 3: 注入时已知每条记忆属于哪个project/context→cross-contamination的ground truth
- Level 4: 多hop问题的必要信息点在构造时已确定→coverage的ground truth
- Level 5: 每条记忆的来源和确定性在注入时已知→calibration的ground truth
- Level 6: 退化是人为引入的→anomaly detection的ground truth

**这是CogMemBench的一个重要优势：完全可自动化评估，不需要人类标注。**

### 规模梯度

每个Level在4个规模下测试：
| Scale | 记忆量 | 模拟时间跨度 | 代表场景 |
|---|---|---|---|
| S | 1,000 | 1周 | 新用户 |
| M | 10,000 | 1个月 | 活跃用户 |
| L | 50,000 | 3个月 | 重度用户 |
| XL | 100,000 | 6个月+ | 长期agent |

### 最终评分

**不做单一加权总分**——每个Level独立报告。

原因：不同应用场景关注不同Level。
- 短期对话agent可能只关心L1+L4
- 长期personal agent需要L1-L6全部
- 安全敏感场景特别关心L5+L6

**每个Level输出一个雷达图维度，最终是6维雷达图。**

---

## 为什么这个benchmark有说服力

1. **每个测试对应一个可复现的real-world failure** — 不是合成的理论问题
2. **Ground truth从构造中推导** — 不需要人工标注，完全可自动化
3. **Baselines自然暴露问题** — 不需要刻意设计让baseline失败
4. **规模梯度揭示退化** — 小规模下大家都差不多，大规模下差距显现
5. **理论解释why但不依赖理论定义good** — ACT-R/Hebb/Ebbinghaus解释为什么某些设计有效，但好坏标准是工程性能

---

## Next: Part 3 — 实施细节

具体的数据集生成代码、评分函数、benchmark runner架构。
