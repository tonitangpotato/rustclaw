# RustClaw 自我提升系统综合分析

> 综合 Meta-Harness 论文、Skill-JIT、IDEA-20260403-01（Skill 自动优化）、IDEA-20260403-03（Harness 自我优化）的启示，提出统一框架。

## 问题定义

RustClaw 有三层可以自我优化的东西：

1. **Skill 层** — SKILL.md 文件，定义 agent 的工作流（social-intake、capture-idea、draft-design 等）
2. **Harness 层** — ritual pipeline 的 phase 编排（design → graph → implement → verify）
3. **Memory 层** — engram 的 recall 质量、SOUL.md 的 drive alignment

这三层目前都是**人工调优**的。Meta-Harness 论文证明了：自动优化可以超过人工（TerminalBench-2 76.4% 超人工 74.7%）。

---

## 从各来源学到的关键洞察

### 来自 Meta-Harness 论文
1. **完整历史 >> 压缩摘要** — 50% vs 34.9%。因果链条长时，压缩会丢失关键线索。我们的 execution-log.jsonl append-only 设计已验证正确。
2. **最小化外部结构** — Meta-Harness 不预设搜索框架（不用遗传算法、不用 tree search），而是让 proposer agent 自己决定怎么搜索。前 4 次评估就追平别人 40 次的成绩。
3. **"加法优于修改"** — 第 7 轮转折：不改现有逻辑，只加信息（环境快照）就获得巨大提升。这和 Skill 系统的"叠加能力而非改核心代码"理念一致。
4. **两层天花板** — Big Model + Big Harness 缺一不可。模型能力是上限，Harness 决定你能达到多接近上限。

### 来自 Skill-JIT
1. **Progressive Disclosure 3 层** — frontmatter（always in context, ~100 words）→ body（triggered 时加载）→ references/（按需读取）。解决 context window 浪费问题。
2. **5 种 Pattern 分类** — Tool Wrapper / Generator / Reviewer / Inversion / Pipeline。给 skill 生成提供结构化框架。
3. **Generalization Litmus Test** — "Would someone with a DIFFERENT task using the same tool find this skill useful?" 防止过拟合。
4. **What/How/Verify 三元组** — 每个 step 必须有这三个部分，不允许模糊指令。
5. **缺失：没有优化闭环** — Skill-JIT 只 create/fix，不追踪效果，不自动改进。这是我们的差异化。

### 来自 Karpathy 知识库理念
1. **自增强循环** — 使用产生的结果重新存入系统 → 知识库随使用自我强化。同样适用于 skill/harness 优化：优化后的 skill 产生更好的数据 → 更好的优化。
2. **LLM 作为编译器** — 自动化索引、分类、维护。skill 的 trigger matching 就是一种"编译"——把人类意图编译成具体工作流。

---

## 统一框架：Self-Improvement Engine

三层优化（Skill / Harness / Memory）共享同一个底层循环：

```
Observe → Analyze → Propose → Test → Commit
  ↑                                      |
  └──────────────────────────────────────┘
```

### 1. Observe（数据采集）

**Skill 层数据：**
- 触发频率（哪些 skill 常用/从不用）
- 执行成功率（skill 指导下完成任务的比例）
- Token 消耗（某些 skill 太长导致浪费）
- 用户满意度信号（被要求重做 = 负信号，"不错" = 正信号）
- False positive/negative（触发了不该触发 / 该触发没触发）

**Harness 层数据：**
- Phase 耗时（哪个 phase 是瓶颈）
- Phase 成功/失败率（design phase 总是需要返工？）
- Token 消耗分布（verify phase 占了 80% token？）
- 重试次数和原因（哪类 task 总失败？）
- 端到端完成率（ritual 走完的比例 vs 被 cancel 的比例）

**Memory 层数据：**
- Recall precision（召回的记忆有多少真的有用）
- Miss rate（该想起来但没想起来的）
- Drive alignment 命中率（cross-language 问题是否已解决）

**数据源（已有）：**
- `execution-log.jsonl` — harness 完整执行历史 ✅
- `engram_behavior_stats` — 工具使用成功/失败率 ✅
- `engram_trends` — 情感趋势 ✅
- 缺失：skill 级别的使用追踪（需要在 SKM 加 hook）

### 2. Analyze（模式识别）

不需要复杂的 ML，LLM 本身就是最好的 pattern recognizer：

```
给 proposer agent：
- 最近 N 次 ritual execution 的完整 log
- 最近 N 次 skill 触发的效果记录
- engram 的 behavioral stats
- 当前所有 SKILL.md 的 frontmatter

问：
1. 哪个 skill/phase 表现最差？为什么？
2. 有没有重复出现的失败模式？
3. 有没有新的工作模式应该被提炼成 skill？
4. token 消耗有没有优化空间？
```

**关键设计决策（来自 Meta-Harness）：**
- 给 proposer **完整历史**，不要压缩成摘要
- **不预设搜索框架** — 让 proposer 自己决定要改什么
- 但要给 proposer **可改的东西**的清单（所有 SKILL.md、ritual config、engram 配置）

### 3. Propose（生成改进方案）

Proposer 输出：
```yaml
proposals:
  - target: skills/social-intake/SKILL.md
    type: modify  # modify | create | split | merge | delete
    change: "Step 3 的 engram recall 查询太宽泛，改为 domain-specific 查询"
    expected_impact: "减少 30% 无关结果，提升 intake 质量"
    risk: low

  - target: ritual.phases.verify
    type: modify
    change: "verify phase 增加 type-check 前置步骤，减少全量测试的无效重试"
    expected_impact: "verify phase 耗时减少 40%"
    risk: medium
```

### 4. Test（验证改进）

**Skill 层验证：**
- A/B test：同一触发条件，随机选新旧版本
- Shadow mode：新版本运行但不生效，和旧版本比较输出
- 回归检测：改了的 skill 不能让之前成功的 case 失败

**Harness 层验证：**
- 在低风险 task 上先跑新配置
- 比较 token 消耗、完成率、耗时
- 如果指标恶化 → 自动 rollback

**这里 Skill-JIT 的 "Generalization Litmus Test" 很有用：**
- 改进后的 skill 不能过拟合到最近的失败 case
- 应该在一组代表性 case 上都表现更好

### 5. Commit（应用改进）

- 通过 version control — skill 有版本历史，可 rollback
- Progressive rollout — 先小范围，再全量
- 通知 potato — "我优化了 social-intake skill 的 Step 3，预期效果：..."
- 记录到 engram — 为什么改、改了什么、效果如何

---

## Progressive Disclosure + SKM 整合

Skill-JIT 的 3 层加载模型可以直接改进 SKM：

**现状：**
```
SKM trigger match → 注入全量 SKILL.md（500+ 行）
```

**改进：**
```
Layer 0: 所有 skill 的 frontmatter 永远在 context（每个 ~50 tokens，10 个 skill = 500 tokens）
Layer 1: 触发的 skill 注入 body（~500 tokens）
Layer 2: agent 显式请求时加载 references/（按需，可能几千 tokens）
```

**实现路径：**
1. SKILL.md 格式不变（YAML frontmatter + markdown body 天然分层）
2. 加 `references/` 子目录约定（如 `skills/social-intake/references/platform-examples.md`）
3. SKM 加 token budget 感知 — 剩余 context 空间 < 阈值时只加载 frontmatter
4. Agent 有 `load_skill_detail(skill_name)` 工具 — 按需加载 Layer 2

---

## 和 xinfluencer 的关系

自我优化系统和 xinfluencer 不冲突，反而互补：
- xinfluencer 用 ritual 开发 → 产生 execution-log 数据
- 这些数据 feed 自我优化系统的 Analyze 阶段
- 优化后的 ritual/skill 加速下一个 xinfluencer feature 的开发
- **正反馈循环**

---

## 实施优先级

| 步骤 | 描述 | Effort | 依赖 |
|------|------|--------|------|
| 0 | SKM Progressive Disclosure（3 层加载）| 小 | 无 |
| 1 | Skill 使用追踪 hook（在 SKM 加数据采集）| 小 | 无 |
| 2 | Ritual phase trace（结构化 trace 补齐）| 小 | 无 |
| 3 | Proposer agent prototype（读历史、出建议）| 中 | 1+2 |
| 4 | Shadow mode A/B test（skill 层）| 中 | 3 |
| 5 | Auto-commit + rollback（完整闭环）| 大 | 4 |

步骤 0-2 可以立即开始，不影响 xinfluencer 开发。
步骤 3 在积累足够数据后启动（~1-2 周后）。
