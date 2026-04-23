# 2026-04-20 CogMemBench Autopilot Tasks

> **Goal**: 把 engram 记忆系统的 benchmark 从"一个数字"推进到"可操作的改进路线图"
> **Baseline**: LoCoMo 78.1% (SOTA 94.37%), evidence recall 54.5%
> **Project root**: `/Users/potato/clawd/projects/cogmembench/`
> **engram binary**: `engram` (Rust, in PATH)

---

## Task 1: LoCoMo 失败案例深度分析

**目标**: 从 439 个错误答案中提取 engram recall 的具体瓶颈模式，产出可操作的改进方向。

**输入文件**:
- `results/locomo-engram-20260420_103221.jsonl` — 1986 条完整结果（含 question, gold_answer, predicted_answer, correct, category, evidence_recall, num_retrieved）
- `results/locomo-engram-20260420_103221-summary.json` — 聚合统计

**已知弱点**:
| Category | Accuracy | Evidence Recall | Questions |
|---|---|---|---|
| Cat 1: Single-hop factual | 68.4% | 24.1% | 282 |
| Cat 2: Multi-hop factual | 80.7% | 60.4% | 321 |
| Cat 3: Open-ended | 87.5% | 30.0% | 96 |
| Cat 4: Temporal reasoning | 86.3% | 61.0% | 841 |
| Cat 5: Adversarial/unanswerable | 63.9% | 62.4% | 446 |

### Steps

- [ ] **1.1** 写 `analysis/failure_analysis.py` 脚本，从 JSONL 加载所有 `correct=false` 的记录
- [ ] **1.2** 按 category 分组统计，输出每个 category 的：错误数、evidence_recall 分布（p25/p50/p75）、num_retrieved 分布
- [ ] **1.3** 对 Cat 1 (68.4%) 的 89 个错误做细分：
  - **Retrieval failure**: evidence_recall = 0（完全没召回相关记忆）
  - **Partial retrieval**: 0 < evidence_recall < 0.5（召回了但不够）
  - **LLM failure**: evidence_recall >= 0.5 但答案仍然错（记忆够了但 Haiku 判断错）
  - 每类各采样 5 个 case，输出 question + gold + predicted + retrieved context
- [ ] **1.4** 对 Cat 5 (63.9%) 的 161 个错误做细分：
  - **False positive**: 问题是 unanswerable 但系统给了具体答案（hallucination）
  - **False negative**: 问题有答案但系统说"不知道"
  - 统计两类比例；false positive 是更严重的问题
  - 每类各采样 5 个 case
- [ ] **1.5** 跨 category 通用模式分析：
  - num_retrieved = 0 的错误有多少？（engram 完全没返回结果）
  - evidence_recall = 0 但 num_retrieved > 0 的有多少？（返回了但全是无关的）
  - 错误是否集中在特定 conversation？（某些对话结构 engram 处理不好）
- [ ] **1.6** 输出 `analysis/locomo-failure-report.md`，包含：
  - Executive summary: top 3 瓶颈 + 预估改进空间
  - 每个 category 的详细分析 + 代表性错误 case
  - Engram-specific 改进建议（recall策略、ranking、embedding质量、Hebbian权重）
  - 优先级排序：修哪个 category 的 ROI 最高

### 验证
```bash
cd /Users/potato/clawd/projects/cogmembench
python3 analysis/failure_analysis.py
# 应输出统计到 stdout + 写 analysis/locomo-failure-report.md
cat analysis/locomo-failure-report.md | head -50
```

### 预期产出
- `analysis/failure_analysis.py` (~200 行)
- `analysis/locomo-failure-report.md` (~300 行)
- 明确的 "修 X 可以把准确率从 78% 提到 ~Y%" 估算

---

## Task 2: LongMemEval Adapter

**目标**: 搭建 LongMemEval benchmark runner，复用 LoCoMo 的 Anthropic OAuth + Haiku 评估架构。

**参考**: LongMemEval (ICLR 2025), 500 questions, 5 categories
- Paper: https://arxiv.org/abs/2410.10813
- GitHub: https://github.com/xiaowu0162/LongMemEval
- 5 abilities: Information Extraction, Multi-Session Reasoning, Temporal Reasoning, Knowledge Updates, Abstraction & Synthesis

**复用组件** (已有，从 `benchmarks/locomo/` 来):
- `llm.py` — OAuth token refresh + Haiku 调用（**直接复用，不改**）
- `evaluator.py` — judge_answer 逻辑（LLM-as-judge 模式）
- `engram_adapter.py` — engram CLI subprocess 调用

### Steps

- [ ] **2.1** 下载 LongMemEval 数据集
  ```bash
  cd /Users/potato/clawd/projects/cogmembench/datasets
  git clone https://github.com/xiaowu0162/LongMemEval.git longmemeval
  ```
- [ ] **2.2** 分析数据集格式：读 `longmemeval/` 下的 JSON/JSONL 结构，搞清楚：
  - 对话数据格式（sessions? turns? 跟 LoCoMo 的区别？）
  - QA 格式（question, answer, evidence 字段名？）
  - 5 个 category 的标识方式
  - 写简要格式说明到 `benchmarks/longmemeval/FORMAT.md`
- [ ] **2.3** 创建 `benchmarks/longmemeval/` 目录结构：
  ```
  benchmarks/longmemeval/
  ├── __init__.py
  ├── config.py          ← 路径、category 名称、LLM 参数
  ├── data_loader.py     ← 解析 LongMemEval JSON → 统一 Conversation/QAPair
  ├── engram_adapter.py  ← 可能直接复用 locomo 的，或继承
  ├── evaluator.py       ← 复用 locomo 的 judge + evidence recall
  ├── runner.py          ← 主编排：load → ingest → query → evaluate → report
  └── FORMAT.md          ← 数据格式说明
  ```
- [ ] **2.4** 实现 `data_loader.py`：
  - 定义或复用 `Conversation`, `QAPair` 类型（跟 locomo 的 data_loader 对齐）
  - 解析 LongMemEval 的格式到统一类型
  - 处理 LongMemEval 特有的字段（如果有 temporal annotations、knowledge update markers 等）
- [ ] **2.5** 实现 `config.py`：
  - `DATASET_PATH` 指向 `datasets/longmemeval/`
  - `CATEGORY_NAMES` 映射 5 个 ability
  - LLM 参数复用 locomo 的（同一个 Haiku model, 同样的 OAuth）
- [ ] **2.6** 实现 `runner.py`：
  - 复用 locomo runner 的模式：checkpoint/resume、JSONL 逐条写、summary JSON
  - Ingest: 把 LongMemEval 的对话数据喂给 engram（per-conversation DB）
  - Query: 对每个 QA pair，调 engram recall → Haiku 生成答案 → judge
  - Summary: per-category accuracy + evidence recall + per-conversation breakdown
- [ ] **2.7** 创建 `run_longmemeval.py` 入口（顶层，跟 `run_locomo.py` 同级）：
  ```python
  # python3 run_longmemeval.py --system engram --conversations all
  ```
- [ ] **2.8** 抽取共享模块到 `benchmarks/common/`（如果 llm.py 和 evaluator.py 完全一致）：
  ```
  benchmarks/common/
  ├── __init__.py
  ├── llm.py             ← OAuth + Haiku（从 locomo 移出来）
  └── evaluator.py       ← judge_answer + evidence_recall
  ```
  - 更新 locomo 和 longmemeval 的 import
  - 确保 `run_locomo.py` 仍然正常工作（回归测试）
- [ ] **2.9** Dry run: 选 1 个 conversation 跑通全流程
  ```bash
  python3 run_longmemeval.py --system engram --conversations <first-conv-id>
  ```
  - 验证: ingest 成功、recall 有结果、judge 能判断、summary JSON 正确

### 验证
```bash
cd /Users/potato/clawd/projects/cogmembench
# 单对话验证
python3 run_longmemeval.py --system engram --conversations <first-id>
cat results/longmemeval-engram-*-summary.json
# 回归: locomo 仍然 work
python3 run_locomo.py --system engram --conversations conv-26
```

### 预期产出
- `benchmarks/longmemeval/` 完整模块（~400 行）
- `benchmarks/common/` 共享 LLM + evaluator（~200 行，从 locomo 迁移）
- `run_longmemeval.py` 入口
- 至少 1 conversation 跑通的结果

### ⚠️ 注意事项
- LLM 调用用 **现有的 OAuth flow**（`benchmarks/locomo/llm.py` 里的 `_refresh_token()`），不要引入新的 auth 方式
- Judge model 跟 LoCoMo 一致: Haiku (`claude-3-5-haiku-20241022`)
- engram 调用走 subprocess（`engram` binary），不走 Python binding
- 每个 conversation 独立 DB（`dbs/longmemeval-{conv_id}.db`）

---

## Task 3: CogMemBench 五层设计完善

**目标**: 把 Part 2 的概念设计变成可实现的 benchmark specification。

**背景**: CogMemBench 测的不是"像不像人脑"，而是"记忆系统在真实 agent 工作中的失败模式"。
五层对应五种具体的工程失败模式，每层有独立的量化指标。

**现有代码** (`src/cogmembench/levels/`):
- `l1_signal_noise.py` — Level 1 骨架
- `l2_temporal.py` — Level 2 骨架
- `l3_interference.py` — Level 3 骨架
- `l5_confidence.py` — Level 5 骨架
- (缺 Level 4)

**Part 2 设计** (from `memory/cogmembench-part2-design.md`):
| Level | Name | Tests | Core Metric |
|---|---|---|---|
| L1 | Signal/Noise | 信噪比 + needle in haystack | recall@k, precision@k |
| L2 | Temporal | 时间推理 + 衰减 | temporal_accuracy |
| L3 | Interference | 更新覆盖 + proactive interference | update_accuracy |
| L4 | Integration | 跨 session 综合 | synthesis_quality (LLM judge) |
| L5 | Confidence | 知道自己不知道 | calibration_error |

### Steps

- [ ] **3.1** 读现有骨架代码，评估每个 level 的完成度：
  - 哪些有完整的 test generation logic？
  - 哪些只是空壳 class？
  - 跟 Part 2 设计的 gap 是什么？
  - 输出 `docs/cogmembench-status.md`
- [ ] **3.2** 为每层写 **详细 test case spec**（`docs/cogmembench-spec.md`），包含：
  - **数据生成策略**: 怎么合成测试对话（不依赖 LoCoMo 数据集）
  - **控制变量**: 每个 test 变化什么、固定什么
  - **量化指标**: 精确公式（不是"precision@k"，是具体怎么算）
  - **baseline 预期**: 一个 naive RAG 系统应该拿多少分？engram 应该拿多少？
  - **题目数量**: 每层多少题 = 统计显著（power analysis）
- [ ] **3.3** Level 1 (Signal/Noise) 详细设计:
  - Needle-in-haystack: 在 N 条对话中插入 1 条关键信息，测能否召回
  - 参数: N = [10, 50, 100, 500, 1000]（scalability curve）
  - Distractor quality: random vs topically-similar（两种难度）
  - 指标: recall@1, recall@5, MRR, latency vs N
- [ ] **3.4** Level 2 (Temporal) 详细设计:
  - Temporal ordering: "X 是在 Y 之前还是之后？"
  - Recency bias test: 最近的信息 vs 早期的信息，recall 差异
  - Decay curve: 存入后 T=[1min, 1h, 1d, 7d, 30d] 模拟，recall 衰减
  - 指标: temporal_ordering_accuracy, recency_ratio, decay_half_life
- [ ] **3.5** Level 3 (Interference) 详细设计:
  - Proactive interference: 旧信息干扰新信息的学习
  - Retroactive interference: 新信息覆盖旧信息
  - Update test: "X 的电话号码从 A 改成 B"，问电话号码 → 应该回答 B
  - 指标: update_accuracy, interference_rate, old_info_intrusion_rate
- [ ] **3.6** Level 4 (Integration) 详细设计 — **新建 `l4_integration.py`**:
  - Cross-session synthesis: Session 1 说 "我喜欢日本料理"，Session 5 说 "我下周去东京"，问 "推荐餐厅" → 需要综合两个 session
  - Multi-fact reasoning: 需要 >= 2 条独立记忆组合才能回答
  - 指标: synthesis_quality (LLM judge, 1-5 scale), required_hops vs actual_hops
- [ ] **3.7** Level 5 (Confidence) 详细设计:
  - Known-unknown: 系统从未见过的信息，应该说"不知道"
  - Confidence calibration: 系统说"确信"时的准确率 vs 说"不确定"时的准确率
  - Adversarial: 诱导系统编造答案的 prompt（跟 LoCoMo Cat 5 对齐）
  - 指标: ECE (Expected Calibration Error), false_positive_rate, abstention_accuracy
- [ ] **3.8** 设计 **数据生成 pipeline**:
  - 不依赖外部数据集 → 完全合成
  - 用 LLM 生成测试对话（prompt template per level）
  - 每个对话有 ground truth annotation（哪条 turn 包含答案）
  - 生成器的 seed 固定 → 可复现
  - 输出 `src/cogmembench/data/generator.py` 的接口设计
- [ ] **3.9** 设计 **评估 pipeline**:
  - 跟 LoCoMo/LongMemEval 统一：Haiku judge + JSONL 输出 + summary JSON
  - 每层独立跑、独立评分，但统一 CLI:
    ```bash
    python3 -m cogmembench run --level 1 --system engram
    python3 -m cogmembench run --level all --system engram
    ```
  - Dashboard: 五层雷达图（每层 0-100 归一化分数）
- [ ] **3.10** 整合输出 `docs/cogmembench-spec.md`（~500 行），作为实现的完整 spec

### 验证
```bash
# spec 文档存在且完整
wc -l docs/cogmembench-spec.md  # 应 >= 400 行
# 每层都有 "Data Generation", "Control Variables", "Metrics", "Baseline" section
grep -c "## Level" docs/cogmembench-spec.md  # 应 = 5
# status 文档存在
cat docs/cogmembench-status.md
```

### 预期产出
- `docs/cogmembench-status.md` — 现有代码状态评估
- `docs/cogmembench-spec.md` — 五层完整 specification（实现蓝图）
- `src/cogmembench/levels/l4_integration.py` — Level 4 骨架代码
- 明确的实现优先级排序

---

## 执行顺序

**Task 1 先做** — 分析失败案例是最低成本最高信息量的工作，结果直接指导 engram 改进方向
**Task 2 次之** — LongMemEval 是第二个数据点，跟 LoCoMo 交叉验证瓶颈
**Task 3 最后** — CogMemBench spec 是长期投入，依赖前两个 task 的 findings 来校准设计

---

## 关键约束

- **LLM 调用统一用 OAuth flow** (`benchmarks/locomo/llm.py` 的 `_refresh_token()`)，model = `claude-3-5-haiku-20241022`
- **engram 通过 subprocess 调用** (`engram` binary)，不走 Python binding
- **结果格式统一**: JSONL per-question + JSON summary
- **数据集路径**: `datasets/locomo/`, `datasets/longmemeval/`
- **结果路径**: `results/locomo-*`, `results/longmemeval-*`
- **每个 conversation 独立 engram DB**: `dbs/{benchmark}-{conv_id}.db`

---

## Reference: CogMemBench 第一性原理分析

> Source: `memory/cogmembench-analysis.md` — 完整背景分析，供 autopilot 理解项目定位和设计决策。

### 认知科学五层模型

人类记忆不是数据库。好的记忆系统有5个层次：

- **Layer 0: Storage & Retrieval** — 存取（LongMemEval 测的全部内容，最低层）
- **Layer 1: Temporal Dynamics** — 时间动力学（Ebbinghaus遗忘曲线、consolidation、间隔效应）→ **没有benchmark测**
- **Layer 2: Associative Structure** — 关联结构（Hebbian learning、spreading activation、干扰效应）→ **没有benchmark测**
- **Layer 3: Metacognition** — 元认知（confidence calibration、feeling-of-knowing、矛盾检测）→ **几乎空白**
- **Layer 4: Synthesis & Emergence** — 综合与涌现（pattern discovery、跨域类比）→ **完全没有benchmark测**
- **Layer 5: Self-Regulation** — 自我调节（interoception、压力下行为调制）→ **概念都没被讨论过**

### engram能力 vs 现有Benchmark覆盖

| engram能力 | LongMemEval | MemEvoBench | 覆盖状态 |
|---|---|---|---|
| ACT-R Activation（频率×近因×扩散） | ⚠️ 表层触及 | ❌ | 只测最简单的表层 |
| Consolidation（working→core转移） | ❌ | ❌ | **完全空白** |
| Ebbinghaus Forgetting（遗忘曲线） | ❌ | ❌ | **完全空白** |
| Hebbian Association（co-recall建链） | ❌ | ❌ | **完全空白** |
| Synthesis（pattern发现+insight） | ❌ | ❌ | **完全空白** |
| Interoceptive（自我状态感知） | ❌ | ❌ | **完全空白** |
| Confidence Calibration（元认知） | ❌ | ⚠️ 间接 | 几乎空白 |
| Session Working Memory（容量管理） | ❌ | ❌ | **完全空白** |
| Knowledge Compiler（冲突/合并） | ❌ | ⚠️ 间接 | 几乎空白 |

**结论：现有benchmark覆盖了engram约10%的能力。剩下90%完全没有被评估标准覆盖。**

### CogMemBench 精确定位

**不做什么：**
- ❌ 不重复 LongMemEval 的 retrieval 测试
- ❌ 不重复 MemEvoBench 的 corruption 检测
- ❌ 不测基础存取能力

**做什么：测试记忆的"活"的特性——时间动力学、关联结构、元认知、综合能力、自我调节。**

类比：
- LongMemEval = 测视力（能不能看清东西）
- MemEvoBench = 测免疫力（会不会被病毒感染）
- **CogMemBench = 测大脑功能（能不能学习、关联、反思、适应）**

### 学术定位

"现有benchmark测的是记忆系统的**信息保真度**。CogMemBench测的是记忆系统的**认知功能**。"

理论基础：
- Layer 1: Murre & Chessa 2011, Ebbinghaus 1885
- Layer 2: Hebb 1949, Anderson 2007 ACT-R
- Layer 3: Nelson & Narens 1990, Hart 1965
- Layer 4: Mednick 1962, Boden 2004
- Layer 5: Craig 2002, Damasio 1994

### MemEvoBench vs CogMemBench 行为漂移对比

| 维度 | MemEvoBench | CogMemBench |
|---|---|---|
| 被测对象 | LLM的决策安全性 | 记忆系统的认知功能 |
| 记忆系统角色 | 提供context（被动） | 被测试的主体（主动） |
| 攻击模型 | adversarial memory injection | proactive/retroactive interference（认知科学） |
| 成功标准 | LLM不被误导 | 记忆正确consolidation/association/synthesis |
| 行为漂移来源 | 外部攻击 | 自然认知退化（interference, consolidation bias） |

两者互补，不重叠。

### CogMemBench 层依赖链

```
Level 5: Self-Regulation → 依赖 Layer 3+4 的信号
Level 4: Synthesis → 依赖 Layer 2 的关联网络
Level 3: Metacognition → 依赖 Layer 1 temporal + Layer 2 结构
Level 2: Associative Structure → 依赖 Layer 1 的 repeated access patterns
Level 1: Temporal Dynamics → 依赖基础存取（不在 scope 内）
```

**必须按层测试，不能跳层。**

### 为什么是好论文

1. **学术空白真实** — 整个领域缺了80%的评估框架
2. **认知科学理论基础** — 每层都有peer-reviewed理论支撑
3. **可运行的参考实现** — engram: 46K行Rust代码, 247测试, crates.io已发布
4. **Timing完美** — 2024 LongMemEval → 2025 MemEvoBench → 2026 CogMemBench（自然下一步）
5. **对领域有推动** — 重新定义"好的记忆系统"标准
