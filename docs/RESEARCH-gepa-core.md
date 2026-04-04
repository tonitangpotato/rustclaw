# Research: gepa-core — Prompt Evolution Rust Crate

## Executive Summary

GEPA (Genetic-Pareto) 是目前 prompt optimization 领域最先进的方法之一（ICLR 2026 Oral），由 Stanford/Berkeley 团队开发。其核心创新是**用自然语言反思代替标量梯度**——LLM 读取完整执行轨迹来诊断失败原因，然后生成改进版本。GEPA 在 6 个任务上平均超过 GRPO（RL）6%，用最少 35x 的 rollout。

目前 GEPA 只有 Python 实现（`pip install gepa`），没有 Rust 实现。我们计划构建 `gepa-core`：一个 LLM-agnostic 的 Rust crate，实现 GEPA 的核心算法，可被 RustClaw 的 skill 自优化系统直接使用。

**推荐：GO。** GEPA 算法结构清晰、模块化好、无需 GPU/权重访问，非常适合用 Rust 实现为基础设施库。

---

## Core Concepts: What Is Prompt Evolution?

### 问题定义

给定一个包含一个或多个文本参数（prompts、instructions、code）的 AI 系统，以及一个评估函数（metric），自动找到使评估分数最大化的文本参数组合。

### 为什么需要自动优化？

- 手工 prompt engineering 是高技能劳动，效率低
- 不同模型/任务需要不同 prompt，无法复用
- 小的 prompt 改动可能带来巨大性能差异（GEPA 在 AIME-2025 上 +12%）

### 两大范式

1. **基于梯度/RL**（GRPO, PPO, TextGrad）
   - 需要大量 rollout（5,000-25,000+）
   - 把执行结果压缩为标量 reward → 信息损失大
   - 需要可微分或策略梯度
   
2. **基于进化/反思**（GEPA, EvoPrompt, PromptBreeder, APE）
   - 用 LLM 作为 mutation operator
   - 读取完整执行轨迹（不仅是分数）
   - 少量 rollout（100-500）即可有效

---

## Existing Approaches: Paper-by-Paper Analysis

### 1. GEPA — Genetic-Pareto (ICLR 2026 Oral) ⭐ 核心

**Paper:** arXiv:2507.19457
**Authors:** Lakshya Agrawal, Omar Khattab, Matei Zaharia 等 (Stanford/Berkeley)
**Code:** https://github.com/gepa-ai/gepa (MIT License)

**核心算法（5 步循环）：**

1. **Select** — 从 Pareto frontier 选一个 candidate（在不同 task 子集上各有优势的 candidates）
2. **Execute** — 在一个 minibatch 上执行，**捕获完整执行轨迹**（reasoning, tool calls, tool outputs, error messages）
3. **Reflect** — teacher LLM 读取轨迹，诊断失败原因，提出改进方向（自然语言）
4. **Mutate** — 基于反思结果和所有祖先的累积教训，生成改进版 candidate
5. **Accept** — 如果新 candidate 在 minibatch 上 sum(scores) 优于原 candidate，接受并更新 Pareto front

**关键创新：**

- **Actionable Side Information (ASI)**: 评估器返回的诊断反馈（error messages, profiler output, reasoning logs），是文本优化的"梯度"类比。这是 GEPA 最核心的概念——其他方法只看 scalar score，GEPA 看完整上下文。
- **Pareto-aware selection**: 不是只维护一个"最好"的 candidate，而是维护一个 Pareto 前沿——每个 candidate 可能在不同任务子集上最好。这避免了遗忘已学到的知识。
- **System-aware merge**: 合并两个在不同任务子集上擅长的 Pareto-optimal candidates，取长补短。
- **Reflective dataset**: 为每个组件构建小型反思数据集（inputs, generated outputs, feedback），feed 给 teacher LLM 做 mutation。

**架构（Python 源码分析）：**

```
gepa/core/
├── engine.py       (32KB) — GEPAEngine: 主循环，协调 proposers、evaluation、state
├── state.py        (32KB) — GEPAState: Pareto front、candidate history、evaluation cache
├── adapter.py      (9KB)  — GEPAAdapter Protocol: 用户实现的 evaluate + make_reflective_dataset
├── callbacks.py    (17KB) — Event system: 12 种事件类型（iteration_start, candidate_accepted, etc.）
├── data_loader.py  (2KB)  — DataLoader: 抽象数据加载
├── result.py       (12KB) — GEPAResult: 优化结果封装
gepa/proposer/
├── reflective_mutation/ — 反思式 mutation proposer
├── merge.py             — Pareto merge proposer
gepa/adapters/
├── default_adapter/     — 单轮 LLM 系统的默认 adapter
├── dspy_full_program_adapter/ — DSPy 完整 program 的 adapter
├── mcp_adapter/         — MCP tool 描述优化
├── generic_rag_adapter/ — RAG pipeline 优化
```

**性能数据：**
- 比 GRPO 平均 +6%，最多 +20%，用 35x 更少的 rollout
- 比 MIPROv2 +10%（AIME-2025 上 +12%）
- ARC-AGI agent: 32% → 89%
- Databricks 90x 更便宜
- 只需 3 个 examples 就能工作

### 2. DSPy — Declarative Self-Improving Pipelines

**Paper:** arXiv:2310.03714
**Authors:** Omar Khattab, Matei Zaharia 等 (Stanford)

- 将 LLM pipelines 抽象为 text transformation graphs
- 声明式模块 + 编译器自动优化 prompt
- GEPA 是 DSPy 的最新优化器（`dspy.GEPA`）
- **关系：** DSPy 是 framework，GEPA 是 DSPy 内的优化算法之一

**对我们的启示：** 
- 我们不需要实现整个 DSPy，只需要 GEPA 的核心优化算法
- Adapter pattern 允许任何系统接入
- DSPy 的 module/signature 概念类似我们的 Skill/SKILL.md

### 3. EvoPrompt — LLM + Evolutionary Algorithms (ICLR 2024)

**Paper:** arXiv:2309.08532

- 用 LLM 执行进化算子（crossover, mutation）
- 维护 prompt population，迭代进化
- 在 31 个 dataset 上测试，比手写 prompt 高 25%（BBH）
- **关键局限：** 没有 reflection，只是用 LLM 做 random mutation → 不如 GEPA 的 targeted mutation

### 4. PromptBreeder (DeepMind, 2023)

**Paper:** arXiv:2309.16797

- 自指式进化：prompt 自己进化自己
- Mutation prompts 也被进化
- **局限：** 计算开销大（need many generations），没有 Pareto 选择

### 5. APE — Automatic Prompt Engineer (Google, 2022)

**Paper:** arXiv:2211.01910

- 用 LLM 生成 prompt candidates
- 然后评估选最好的
- 单轮，没有迭代进化 → 不如迭代方法

### 6. OPRO — Optimization by PROmpting (Google, 2023)

**Paper:** arXiv:2309.03409

- LLM 作为 optimizer，看历史 prompt + score → 生成新 prompt
- 线性历史，不是 Pareto front
- 不读取执行轨迹 → GEPA 的 ASI 是关键差异

### 7. TextGrad (MIT, 2024)

**Paper:** arXiv:2406.07496

- "Backpropagation through text"
- LLM 生成文本形式的"梯度" → 用来更新文本参数
- 理论上优雅，但实际效果不如 GEPA
- LSE (2026) 显示 GEPA 和 TextGrad 都被 RL-trained self-evolution 超越

### 8. EvoX — Meta-Evolution (Berkeley, 2026)

**Paper:** arXiv:2602.23413

- 进化策略本身也被进化（meta-evolution）
- 在 200 个任务上超过 AlphaEvolve, GEPA
- 最新但更复杂，可作为 GEPA 的未来升级路径

### 9. LSE — Learning to Self-Evolve (Microsoft, 2026)

**Paper:** arXiv:2603.18620

- 用 RL 训练 LLM 在 test time 改进 context
- 4B 模型超越 GPT-5 + GEPA
- **但需要训练模型权重** → 我们是 API-only，不适用

---

## Competitive Landscape: Existing Tools/Libraries

| Tool | Language | Algorithm | Stars | Maintained | License |
|------|----------|-----------|-------|------------|---------|
| **gepa (Python)** | Python | GEPA | 5K+ | ✅ Active | MIT |
| **DSPy** | Python | Multiple (GEPA, MIPROv2, etc.) | 25K+ | ✅ Active | MIT |
| **mlflow** | Python | 集成 GEPA | 20K+ | ✅ | Apache-2.0 |
| **Google ADK** | Python | 集成 GEPA | New | ✅ | Apache-2.0 |
| **Pydantic AI** | Python | 集成 GEPA | 5K+ | ✅ | MIT |
| **EvoPrompt** | Python | EA+LLM | 500+ | ❓ | MIT |

**Rust 实现：零。** 没有任何 Rust prompt optimization crate。

### 市场信号

- Shopify CEO Tobi Lutke: "GEPA is severely under-hyped"
- 50+ production 使用者（Shopify, Databricks, Dropbox, OpenAI, Pydantic, MLflow, Comet ML）
- Google ADK 内置 GEPA 优化
- ICLR 2026 Oral（顶级认可）
- Databricks 用 GEPA 让开源模型 90x 更便宜地替代 Claude Opus

---

## Technical Approach for gepa-core (Rust)

### 核心组件

```
gepa-core/
├── src/
│   ├── lib.rs              — public API
│   ├── engine.rs           — GEPAEngine: 主优化循环
│   ├── state.rs            — GEPAState: Pareto front, candidate history, cache
│   ├── adapter.rs          — GEPAAdapter trait: 用户实现的接口
│   ├── candidate.rs        — Candidate: Dict<String, String> + metadata
│   ├── pareto.rs           — Pareto front management, dominance checking
│   ├── proposer/
│   │   ├── mod.rs
│   │   ├── reflective.rs   — ReflectiveMutationProposer
│   │   └── merge.rs        — MergeProposer (Pareto merge)
│   ├── evaluation.rs       — EvaluationBatch, scoring, caching
│   ├── callback.rs         — Event system (type-safe)
│   ├── config.rs           — GEPAConfig, EngineConfig
│   ├── result.rs           — GEPAResult
│   └── data.rs             — DataLoader trait
```

### Key Design Decisions

**1. LLM-agnostic — Adapter pattern**

GEPA 需要 LLM 做两件事：(1) 执行 candidate 评估，(2) 用 teacher LLM 做 reflection/mutation。

```rust
#[async_trait]
pub trait GEPAAdapter: Send + Sync {
    type DataInst: Send;
    type Trajectory: Send;
    type RolloutOutput: Send;
    
    async fn evaluate(
        &self,
        batch: &[Self::DataInst],
        candidate: &Candidate,
        capture_traces: bool,
    ) -> Result<EvaluationBatch<Self::Trajectory, Self::RolloutOutput>>;
    
    fn make_reflective_dataset(
        &self,
        candidate: &Candidate,
        eval_batch: &EvaluationBatch<Self::Trajectory, Self::RolloutOutput>,
        components_to_update: &[String],
    ) -> HashMap<String, Vec<serde_json::Value>>;
}
```

RustClaw 实现这个 trait 时，用自己的 LLM provider 调 Claude/GPT。

**2. Pareto Front — 不丢失多样性**

核心数据结构：

```rust
pub struct ParetoFront {
    candidates: Vec<CandidateEntry>,
    // 每个 candidate 在每个验证集样本上的分数
    scores: HashMap<usize, HashMap<DataId, f64>>,
    // Pareto dominance 关系
    front: Vec<usize>,  // indices of non-dominated candidates
}
```

Pareto dominance checking: candidate A dominates B iff A 在所有 task 上 ≥ B 且至少一个上 > B。

**3. Reflective Mutation — 核心差异化**

不是随机 mutation，而是 LLM 读取轨迹后做 targeted mutation：

```rust
pub struct ReflectiveMutationProposer {
    trainset: DataLoader,
    teacher_lm: Box<dyn LLMProvider>,
    reflection_prompt: String,
    mutation_prompt: String,
}

impl ReflectiveMutationProposer {
    pub async fn propose(&self, state: &GEPAState) -> Option<Proposal> {
        // 1. Select parent from Pareto front
        // 2. Sample minibatch from trainset
        // 3. Evaluate parent on minibatch (with traces)
        // 4. Build reflective dataset
        // 5. Teacher LLM reflects on failures → mutation
        // 6. Evaluate new candidate on same minibatch
        // 7. Accept if improved (sum of scores)
    }
}
```

**4. Callback system — 可观测性**

```rust
pub enum GEPAEvent {
    IterationStart { iteration: usize },
    CandidateAccepted { idx: usize, score: f64, parent_ids: Vec<usize> },
    CandidateRejected { reason: String },
    ParetoFrontUpdated { new_front: Vec<usize>, displaced: Vec<usize> },
    MergeAttempted { parent_ids: Vec<usize> },
    OptimizationEnd { best_candidate: Candidate, best_score: f64 },
}

pub trait GEPACallback: Send + Sync {
    fn on_event(&self, event: &GEPAEvent);
}
```

**5. 序列化 — 断点续传**

```rust
// State serialization for checkpointing
impl GEPAState {
    pub fn save(&self, path: &Path) -> Result<()>;
    pub fn load(path: &Path) -> Result<Self>;
}
```

### 与 RustClaw 的集成点

```
RustClaw Skill System
    │
    ├── SkillAdapter impl GEPAAdapter
    │   ├── evaluate(): 运行 skill 在 test cases 上，记录执行轨迹
    │   └── make_reflective_dataset(): 从 execution-log.jsonl 构建反思数据
    │
    ├── SystemPromptAdapter impl GEPAAdapter
    │   └── 优化 system prompt sections
    │
    └── ToolDescriptionAdapter impl GEPAAdapter
        └── 优化 tool descriptions
```

---

## Key Design Decisions Summary

| Decision | Chosen | Rationale |
|----------|--------|-----------|
| Language | Rust | 性能 + 与 RustClaw 一致 + crates.io 生态无竞品 |
| LLM 接口 | Async trait (adapter pattern) | 用户自带 LLM provider，crate 不依赖特定 API |
| Pareto front | Multi-objective | GEPA 核心设计，避免遗忘 |
| Serialization | serde + JSON | 与 GEPA Python 兼容，断点续传 |
| Async | tokio | LLM 调用必须 async |
| 评估并行 | Configurable parallelism | minibatch 内 examples 可并行评估 |
| 日志 | tracing crate | Rust 生态标准 |
| 依赖 | 最小化 | serde, tokio, async-trait, tracing; 不依赖任何 LLM SDK |

---

## Recommendation

### Go / No-Go Assessment

**Recommendation: GO**

**Reasoning:**

1. **Market gap**: Rust 生态没有任何 prompt optimization crate。零竞争。
2. **Algorithm maturity**: GEPA 是 ICLR 2026 Oral，50+ production users，算法已验证。
3. **Implementation feasibility**: 核心算法清晰（Pareto front + reflective mutation + merge），Python 源码可读（~100KB core），Rust 实现完全可行。
4. **Strategic value**: 作为 RustClaw 自优化系统的核心引擎，直接提升 agent 能力。
5. **Ecosystem alignment**: 与 engramai (认知记忆) 和 gid-core (图索引开发) 一样，作为独立 crate 发布，扩大 Rust AI 生态。

### Key Success Factors

- **Adapter pattern 设计好**：让任何 Rust 项目都能接入，不只是 RustClaw
- **Pareto front 实现正确**：这是算法核心，需要仔细测试
- **Reflective mutation prompt 质量**：影响优化效果，需要实验调优
- **文档 + 示例**：让非 AI 专家也能用

### Differentiators vs Python GEPA

- 性能（Rust 的并行评估 + 状态管理）
- 类型安全（编译时保证 adapter 正确性）
- 单二进制部署（no pip, no venv）
- 与 Rust AI agent 生态集成（RustClaw, Rig, Swarm-RS 等）
- 更小的 footprint（适合边缘部署）

---

## Prior Art (Hermes Agent Self-Evolution)

Hermes Agent (NousResearch, 23.9K stars) 是目前唯一将 GEPA 集成到 agent 自优化系统的项目：

- **hermes-agent-self-evolution** repo (600 stars)
- 流程：wrap skill as DSPy module → generate eval dataset → run GEPA → compare → PR
- Phase 1 完成（Skill evolution），Phase 2-5 未完成
- 用 DSPy + GEPA Python 实现
- 评估数据来源：synthetic generation, SessionDB mining, hand-curated golden sets

**我们的优势：**
- 我们有 execution-log.jsonl（比 Hermes 的 SessionDB 更结构化的轨迹数据）
- 我们有 engram behavioral stats（使用效果追踪）
- 我们有 GID graph（代码结构 + 依赖理解）
- 原生 Rust → 更快的迭代循环

---

*Research completed: 2026-04-03*
*Sources: arXiv papers, GitHub repos, Hermes Agent intake notes*
