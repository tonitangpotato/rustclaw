# Engram Benchmark Strategy

## 两步走战略

### Step 1: 跑别人的 benchmark — 证明基本功

### Step 2: 设计自己的 benchmark — 重新定义赛道

---

## Step 1: 现有 Benchmarks 调研

### 1. LongMemEval (ICLR 2025) ⭐ 最重要
- **Repo**: https://github.com/xiaowu0162/LongMemEval (691 stars)
- **论文**: ICLR 2025，最权威的 agent memory benchmark
- **数据**: 500 questions, HuggingFace 上有数据集
- **测试 5 项能力**:
  1. Information Extraction（从对话历史中提取事实）
  2. Multi-Session Reasoning（跨 session 推理）
  3. Knowledge Updates（知识更新/覆盖）
  4. Temporal Reasoning（时间推理，"上周三说了什么"）
  5. Abstention（没有答案时拒绝回答）
- **规模**: LongMemEval-S (~115k tokens, ~40 sessions), LongMemEval-M (~500 sessions)
- **评估方式**: GPT-4o 做 judge, jsonl 格式提交
- **指标**: Recall@K (session/turn level) + QA accuracy
- **如何接入**: 
  - 下载数据 → 喂 chat history 给 engram `add()` → 对每个 question 用 `recall()` → 输出 jsonl → 跑 evaluate_qa.py
  - 需要写一个 adapter script
- **agentmemory 声称**: 95.2% R@5 on LongMemEval-S

### 2. LoCoMo (ACL 2024)
- **Repo**: https://github.com/snap-research/locomo (784 stars)
- **论文**: ACL 2024
- **数据**: 10 very long-term conversations, 多轮对话 + 多模态
- **3 个任务**:
  1. Question Answering（基于对话历史回答问题）
  2. Event Summarization（提取关键事件）
  3. Multimodal Dialog Generation
- **特点**: 每个对话带时间戳、两个 speaker persona
- **评估**: RAG 检索 + GPT-3.5 judge
- **agentmemory 在 comparison 中引用了 LoCoMo**: Hippo 跑了 89.0% 在 LoCoMo 上

### 3. MemEvoBench (arxiv 2026-04-17, 两天前!)
- **论文**: arxiv:2604.15774
- **核心**: 记忆安全 benchmark — adversarial memory injection, noisy tool outputs, biased feedback
- **测试**: 记忆污染导致行为漂移 (memory misevolution)
- **数据**: QA-style (7 domains, 36 risk types) + workflow-style (20 environments)
- **为什么重要**: engram 的 interoceptive layer 理论上能检测 behavioral drift，这是独特优势
- **状态**: 代码未公开，但论文已出。抢先跑有话题性
- **优先级**: P1 (等代码公开)

### 完整优先级
| Benchmark | 来源 | 优先级 |
|---|---|---|
| LongMemEval | ICLR 2025 | P0 必跑 |
| LoCoMo | ACL 2024 | P1 |
| MemEvoBench | arxiv 2026-04 | P1 (等代码) |

---

## Step 1 执行计划

### Priority: 先跑 LongMemEval

**为什么先跑 LongMemEval:**
1. ICLR 2025, 最权威
2. agentmemory 声称 95.2% R@5，我们需要对标数据
3. 评估脚本现成的
4. 500 个标准化 questions，结果可复现

**需要做的工作:**

```
benchmark/
├── longmemeval/
│   ├── adapter.rs          # engram → LongMemEval 接口
│   ├── run.sh              # 完整流程脚本
│   ├── results/            # 结果输出
│   └── README.md           # 复现指南
├── locomo/
│   └── (phase 2)
└── cognibench/             # 我们自己的 (Step 2)
    └── ...
```

**Adapter 逻辑:**
1. 加载 LongMemEval JSON 数据
2. 对每个 question 的 `haystack_sessions`:
   - 按时间顺序，把每个 session 的 user/assistant turns 喂给 engram `add()`
   - 可以用 session timestamp 做 recency bias
3. 对每个 `question`:
   - 用 engram `recall(question, K)` 检索
   - 返回 top-K sessions/turns
4. 输出 `{question_id, hypothesis}` jsonl
5. 用 LongMemEval 的 `evaluate_qa.py` 评分

**关键优势 engram 应该能发挥的:**
- ACT-R activation decay → 自然的 recency/frequency 加权
- Hebbian 关联 → 跨 session 的概念链接
- BM25 + Vector hybrid → 不同检索策略互补
- Ebbinghaus consolidation → knowledge update 场景应该强

**潜在弱点:**
- 没有做过 turn-level 精细检索优化
- embedding 质量取决于用什么模型（目前是可配的）
- 没有 time-aware query expansion

### 目标分数
- **R@5 > 85%** on LongMemEval-S → 够用，可以说"competitive"
- **R@5 > 90%** → 很强，可以跟 agentmemory 正面对标
- **R@5 > 95%** → 碾压，不太现实但如果 ACT-R + Hebbian 真的 work...

---

## Step 2: 自己的 Benchmark — CogMemBench (暂名)

### 核心洞察

**现有 benchmarks 的盲区：**

LongMemEval 和 LoCoMo 测的都是 **retrieval accuracy** — "你能不能找到正确的记忆？" 这是必要条件但不是充分条件。

**它们完全没测的东西:**
1. **Associative recall** — "A 让你想到了什么?" (不是精确检索，是联想)
2. **Behavioral adaptation** — 连续失败后系统行为是否改变？
3. **Memory consolidation quality** — sleep cycle 后重要记忆是否更强？
4. **Interference resistance** — 大量相似记忆是否导致混淆？
5. **Temporal ordering** — 不是 "什么时候说的" 而是 "哪个先说的"
6. **Forgetting correctness** — 不重要的记忆是否正确衰减？
7. **Emotional/stress memory** — 高情绪时刻的记忆是否更强？（flashbulb memory）
8. **Cross-session concept evolution** — 同一个概念在不同 session 里的定义变化

### CogMemBench 设计

```
CogMemBench: Benchmark for Cognitive Memory Systems
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Level 1: Retrieval (baseline, comparable to LongMemEval)
├── 1.1 Exact recall — 能找到精确事实
├── 1.2 Temporal recall — 按时间检索
└── 1.3 Multi-hop reasoning — 跨 session 推理

Level 2: Cognitive (engram's differentiator)
├── 2.1 Associative recall — 给出 concept A，能联想到 related concept B？
├── 2.2 Consolidation — sleep cycle 后，重要记忆 activation 是否 > 初始？
├── 2.3 Interference — 100 个相似记忆中，能否区分微妙差异？
├── 2.4 Forgetting curve — 低重要性记忆是否按 Ebbinghaus 曲线衰减？
└── 2.5 Knowledge update — 矛盾信息出现后，旧记忆是否被正确替代？

Level 3: Behavioral (only engram can do this)
├── 3.1 Stress detection — 连续 N 次失败后，interoceptive signal 是否激活？
├── 3.2 Strategy adaptation — stress 状态下行为是否改变？
├── 3.3 Flow detection — 持续成功时 flow signal 是否上升？
└── 3.4 Recovery — stress 消除后系统是否回归 baseline？
```

### 为什么这个设计是杀手锏

**Level 1** — 所有 memory 系统都能参加，证明我们不弱于人。

**Level 2** — 只有实现了认知模型的系统能真正表现好。agentmemory 用 Ebbinghaus 但没有 ACT-R/Hebbian，在 2.1 和 2.2 上会弱。

**Level 3** — 只有 engram 能参加。这直接展示了 cognitive architecture vs memory layer 的差距。其他系统在这一层根本 **没有可测的东西**。

### 评估方式

- Level 1: 跟 LongMemEval 兼容的 R@K + accuracy 指标
- Level 2: 自定义但可量化（consolidation ratio, interference F1, decay curve fit R²）
- Level 3: 二元检测（signal triggered? Y/N）+ 行为变化度量

### 发布策略

1. 先跑 LongMemEval，拿到 competitive 数字
2. 设计 CogMemBench，开源 benchmark + 数据集
3. 跑 engram + agentmemory + mem0 + baseline 的对比
4. 发 blog post + HN 帖子: "Why Memory Benchmarks Are Missing the Point"
5. PR 到 agentmemory 的 COMPARISON.md 里附上 CogMemBench 结果

---

## 时间估算

| Task | Effort | Priority |
|------|--------|----------|
| LongMemEval adapter (Rust/Python) | 2-3 天 | P0 |
| 跑 LongMemEval-S | 1 天 (含 API 费用) | P0 |
| 分析结果 + 优化 | 1-2 天 | P0 |
| CogMemBench Level 1 设计 + 数据集 | 2 天 | P1 |
| CogMemBench Level 2 设计 + 数据集 | 3 天 | P1 |
| CogMemBench Level 3 设计 + 数据集 | 2 天 | P1 |
| 跑 comparison (engram + others) | 2 天 | P1 |
| Blog post + 发布 | 1 天 | P1 |

**总计: ~2 周**

P0 (LongMemEval): 本周可以开始
P1 (CogMemBench): 下周

---

## 关键决策需要 potato 确认

1. **benchmark/ 放在哪个 repo？** engram repo 里？还是单独的 cognibench repo？
   - 建议：Level 1-2 放 engram repo，Level 3 放 engram repo（反正只有我们能跑）
   - CogMemBench 本身作为独立 repo 开源（包含评估代码 + 数据集）

2. **LongMemEval 用 LLM judge 需要 OpenAI API key** — 评估脚本用 gpt-4o 做 judge，需要 API 费用

3. **engram 的 embedding 模型** — 跑 benchmark 时用什么 embedding？all-MiniLM-L6-v2 (本地快) 还是 OpenAI ada-002 (质量高)？

4. **名字** — ✅ **CogMemBench** (Cognitive Memory Benchmark) — potato confirmed
