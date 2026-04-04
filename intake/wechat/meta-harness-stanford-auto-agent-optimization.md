# Meta-Harness：让 AI 自动设计 Agent Harness（斯坦福论文解读）

- **URL**: https://mp.weixin.qq.com/s/4W2XwuGKki-CUBMmEVwa5w
- **Platform**: wechat
- **Author**: 未署名（公众号文章）
- **Date**: 2026-04-03 (estimated)
- **Fetched**: 2026-04-03
- **Category**: tech
- **Domain**: 🔧 tech + 🧠 research
- **Tags**: meta-harness, harness-engineering, agent-optimization, Stanford, DSPy, Claude-Code, auto-tuning, TerminalBench
- **URL Hash**: mp-weixin-4W2XwuGKki-CUBMmEVwa5w
- **Extraction Method**: Python urllib + HTML parsing

## Summary

斯坦福论文 Meta-Harness，由 Yoonho Lee（Chelsea Finn 博士生）和 Omar Khattab（DSPy 作者）提出。核心思路：把 Harness 优化本身也变成一个 Harness——让 AI Agent 自动设计、评估、迭代 Agent 的编排系统（Harness），替代人类工程师的手工调参。在 TerminalBench-2 上超过人工精调方案，文本分类超 ACE 7.7 个百分点。

## Key Points

- **核心论点**: 模型不是关键，Harness（编排/工具/prompt 系统）才是决定 Agent 表现的关键变量。同一模型换 Harness 性能可翻倍
- **Meta-Harness 三步循环**: 翻档案（读历史 Harness 源码+trace+分数）→ 跑评估（60 个候选）→ 存档（写回文件系统），循环 20 轮
- **关键设计选择**: 给 proposer 完整文件系统访问权限，而非压缩摘要。消融实验：完整文件系统 50.0% vs 分数+摘要 34.9%
- **为什么完整历史重要**: Harness 优化的因果链条特别长（某个 prompt 决策可能 30 步后才爆炸），压缩信息会丢掉做正确决策的关键线索
- **像人一样调试**: TerminalBench-2 搜索轨迹展示了 proposer 识别混杂变量、隔离变量、切换策略、跨实验迁移经验的完整调试过程
- **搜索效率**: 前 4 次评估就追平 OpenEvolve/TTT-Discover 的 40 次最终成绩。原因：最小化外部结构，不预设搜索框架
- **自动发现的策略**: 文本分类的 "草稿-验证" 两阶段策略、数学推理的四路由检索策略——全是 Agent 自己 "长" 出来的
- **两层天花板**: Big Model（模型能力上限）+ Big Harness（实际达到的高度），缺一不可
- **Harness Engineering 三篇章**: 发现问题 → 总结方法论 → 让 AI 自己来
- **泛化性**: 文本空间过拟合比权重空间难得多，且 Harness 代码可读可审查

## Results

| 任务 | Meta-Harness | 对比基线 | 差距 |
|------|-------------|----------|------|
| 文本分类 | +7.7% vs ACE | context 用量只有 1/4 | |
| IMO 数学推理 | +4.7% avg | 5 个未见模型 | |
| TerminalBench-2 (Opus 4.6) | 76.4% | 超 Terminus-KIRA (74.7%)，排名第二 | |
| TerminalBench-2 (Haiku 4.5) | 排名第一 | 超所有已公开 Haiku 方案 | |

## Potential Value

**与 RustClaw/GID 的直接关系**:

1. **gid-harness 就是 Harness**: 我们的 gid-harness（AI 自主开发执行引擎）本质上就是这里说的 Harness。Meta-Harness 的思路可以用来自动优化 gid-harness 的 phase 设计、prompt 模板、tool 选择策略

2. **完整历史 vs 压缩摘要**: 论文验证了 "给 Agent 完整文件系统访问" 比 "压缩成摘要" 好 15+ 个百分点。这和我们的设计选择一致——execution-log.jsonl append-only 事件流 + 文件系统作为 backend

3. **Ritual 自动优化**: 当前 ritual 的 phase 设计是人工的。Meta-Harness 的方法可以让 ritual pipeline 自动进化——每次 ritual 执行产生 trace，用这些 trace 自动改进 ritual 本身

4. **TerminalBench-2 参考**: 如果要评估 RustClaw 的 coding agent 能力，TerminalBench-2 是直接可用的 benchmark

5. **"加法优于修改" 经验**: 第 7 轮转折点——不改现有逻辑，只加信息（环境快照）。这和 Skill 系统的设计理念一致：不改核心代码，通过 Skill 叠加能力

## Connections Found

- gid-harness (4/2 架构决策) — 我们的 Harness 设计可以从 Meta-Harness 的自动优化思路中受益
- execution-log.jsonl — 完整历史胜过压缩摘要的论点，验证了我们 append-only 事件流的设计
- Ritual V2 — ritual 的 phase 设计可以被 Meta-Harness 式的自动搜索优化
- IDEA-20260403-01 (自动化 Skill 优化) — Skill 也是 Harness 的一部分，可以自动优化
- engram/cognitive memory — "让 Agent 自己做信息检索和因果推理" 和 engram 的 ACT-R 激活机制理念相通

## Action Items

- [x] ~~execution-log.jsonl 保留完整历史~~ — **已实现**：gid-harness telemetry.rs 已是 append-only JSONL，每个事件立即 flush，无压缩无摘要（GUARD-8）。论文验证了这个设计决策正确（完整历史 50% >> 压缩摘要 34.9%）✅
- [ ] 给 gid-harness 加 Meta-Harness 式自动优化循环 — harness 已有完整 trace，下一步是让 proposer agent 读取 execution-log.jsonl 历史，自动提出 harness/ritual 改进建议 [P1]
- [ ] 给 ritual pipeline 加 phase 级 trace（每次 ritual 的 phase 耗时、成功/失败、retry 次数、token 消耗）— 当前 ritual 缺少结构化 trace，需补齐以支持自动优化 [P1]
- [ ] 评估 TerminalBench-2 作为 RustClaw coding agent 能力的 benchmark — 直接可用，有公开排行榜 [P2]
- [ ] 将 "加法优于修改" 原则文档化到 AGENTS.md — Skill 叠加能力 > 修改核心代码，论文第 7 轮转折验证了这个策略 [P1]

## References

- 论文: https://yoonholee.com/meta-harness/paper.pdf
- 项目主页: https://yoonholee.com/meta-harness/
- 推文: https://x.com/yoonholeee/status/2038640635482456118
- 前篇: 《模型不是关键，Harness 才是》(同公众号)

---

## Raw Content

斯坦福今天放出一篇论文，核心思路在于：让 AI 自动设计 Harness，替代人类工程师的手工调参。

在上一篇 Harness Engineering 的文章《模型不是关键，Harness 才是》中，我们提到：同一个模型，换一套 Harness，性能能翻倍。OpenAI、Anthropic、Stripe 各有各的编排哲学，但共识是：Harness 才是决定 Agent 表现的关键变量。

那……既然 Harness 这么重要，为什么还得靠人类工程师一轮一轮地手动迭代呢？

斯坦福的 Yoonho Lee（切尔西·芬恩的博士生）和 Omar Khattab（DSPy 的作者）给出了一个回答：**把 Harness 优化本身也变成一个 Harness。**

论文叫 Meta-Harness，名字起得差点让我以为是 Meta 的新模型……

### 01 先说结果

在文本分类任务上，Meta-Harness 比当前最好的人工设计方案 ACE 高了 7.7 个百分点，同时 context 用量只有 ACE 的四分之一。

在 IMO 级别的数学推理上，一个被自动发现的检索策略，在五个从未见过的模型上平均提升了 4.7 个百分点。

而在 TerminalBench-2 这个 Agent 编程基准上，Meta-Harness 自动发现的 Harness 拿到了 76.4% 的通过率，超过了人工精心调教的 Terminus-KIRA（74.7%），在所有 Opus 4.6 Agent 中排名第二。用 Haiku 4.5 跑的话，更是直接排名第一，超过所有已公开的 Haiku 方案。

### 02 怎么做的

Meta-Harness 的核心机制，其实还，挺简洁的。

想象一个程序员在调试代码。他不会只看最终报错信息就动手改，而是会翻看之前的几次提交记录，对比哪些改动引入了 bug，哪些改动其实是有效的但被别的变更搞砸了。然后基于这些判断，提出下一轮修改。

Meta-Harness 做的就是这件事，只不过调试的对象从代码变成了 Harness 本身。

整个流程分三步循环：

**第一步，翻档案。** 一个 Coding Agent（论文用的是 Claude Code + Opus 4.6）去读文件系统里存储的所有历史记录，包括之前每一版 Harness 的源代码、评估分数、执行 trace。

**第二步，跑评估。** 把新提出的 Harness 拿去跑实际任务，收集成绩和 trace。

**第三步，存档。** 把这一轮的所有产物，代码、分数、推理过程、执行日志，全部写回文件系统，供下一轮查阅。

就这样，不断循环。论文里的典型配置是跑 20 轮迭代，每轮评估约 60 个候选 Harness。

和已有的文本优化方法（OPRO、TextGrad、AlphaEvolve、GEPA 等）相比，Meta-Harness 最关键的设计选择在于：**给 proposer 完整的文件系统访问权限，取代压缩后的摘要。**

### 03 完整历史

现有的文本优化方法，基本都在做某种形式的信息压缩。有的只保留分数，有的只看最近一次的结果，有的让模型先生成一个摘要再做决策。

这些压缩在小规模任务上问题不大。但 Harness 优化有个特殊之处：**因果链条特别长。**

一个 Harness 的某个设计决策，比如 prompt 里加了一句清理指令，可能在 30 步之后才导致 Agent 陷入死循环。如果你只看最终分数，或者只看压缩后的摘要，这种长程因果关系就被丢掉了。

论文做了一组消融实验：
- 只看分数：中位数 34.6，最高 41.3
- 分数 + 摘要：中位数 34.9，最高 38.7
- 完整文件系统：**中位数 50.0，最高 56.7**

甚至摘要版的最高准确率（38.7）还不如完整版的中位数（50.0）。**压缩信息不只是损失了一些边角细节，而是丢掉了做出正确决策所需的关键线索。**

单次评估就能产生约一千万 token 的原始输出，远超任何模型的 context window。所以不能把所有东西塞进一个 prompt，必须让 Agent 自己决定去读什么。

实际运行中，proposer 每轮迭代平均读取 82 个文件（范围 69-99），其中 41% 是之前的 Harness 源码，40% 是执行 trace，剩下的是分数和其他文件。

### 04 像人一样调试

TerminalBench-2 搜索轨迹分析：

第 1-2 轮迭代，proposer 同时修复结构性 bug + 改写 prompt 模板。两个候选都从 64.4% 基线大幅回退。

第 3 轮，识别混杂变量：结构性修复和有害的 prompt 变更混在了一起。拆开后单独验证。

第 4-6 轮继续失败，学到经验：碰 prompt 和完成逻辑的改动风险太高。

**第 7 轮转折**: 换策略，纯粹"加法"操作——在第一次 LLM 调用前加环境快照命令。不动别的，只加信息。成为最佳候选。

第 8 轮组合已验证的修复。第 10 轮跨实验迁移经验。

**整个过程 = 尝试、失败、识别混杂因素、隔离变量、切换策略、组合已验证的修复、跨实验迁移。**

### 05 搜索速度

Meta-Harness 前 4 次评估追平 OpenEvolve/TTT-Discover 的 40 次最终成绩。原因：最小化外部结构，不预设搜索框架，所有决策交给 proposer 自己判断。

### 06 Harness 长什么样

自动发现的 Harness 是 100-1000 行 Python 程序：
- 文本分类："草稿-验证" 两阶段策略（初始预测 → 检索支持者+挑战者 → self-challenging）
- 数学推理：四路由检索策略（组合/几何/数论/默认，各用不同检索参数）

全是 Agent 自己"长"出来的，无人类预设。

### 07 两个天花板

- Big Model（模型能力上限）+ Big Harness（实际达到的高度），缺一不可
- Meta-Harness 把 Harness 天花板尽量往模型天花板靠近
- 泛化性：文本空间过拟合比权重空间难，且代码可读可审查

### 08 Harness Engineering 三篇章

1. 发现问题（Mitchell Hashimoto, OpenAI Codex）
2. 总结方法论（Anthropic GAN 式对抗, Stripe Minions Blueprint）
3. 让 AI 自己来（Meta-Harness）

**"Harness 要 Build to Delete"——Meta-Harness 的可能性：直接让 AI 替你重新设计更好的。**
