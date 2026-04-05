# Requirements: Self-Evolving Harness

## What This Is

RustClaw 做任务时自然积累经验，skills 和 harness 策略从成功/失败中自动进化。像人做多了变熟练，不是建一个优化平台。

**核心循环**: 执行 → 记录结果 → 发现 pattern → 提议改进 → potato approve → 部署 → 继续执行

**不是什么**: 不是遗传算法，不是 A/B 测试框架，不是评估平台。是 agent 从自己的工作中学习。

## GOALs

### Observation（先看，再改）

- **GOAL-1** [P0]: **Execution journaling** — 每次 skill 执行和 ritual phase 完成后，记录结构化结果到 `.gid/execution-journal.jsonl`。字段：timestamp, skill_name, phase (if ritual), task_summary, outcome (success/failure/corrected), duration_secs, tokens_used, user_correction (if any, verbatim text)。这是所有进化的数据基础。

- **GOAL-2** [P0]: **Correction detection** — 当 potato 纠正 agent 行为时（重新表述请求、手动执行 agent 应该做的事、明确说"不对"/"应该用XX"），自动标记上一次执行为 `corrected`，记录纠正内容。检测方法：(1) 显式否定词匹配，(2) 2分钟内同主题重复请求。

- **GOAL-3** [P1]: **Pattern extraction** — 当同一 skill 积累 ≥ 10 条 journal entries 后，用 LLM 分析 failure/correction 记录，提取 "错误 pattern → 正确做法" 对。存储在 `.gid/learned-patterns/{skill-name}.json`。每个 pattern 有 confidence（观察次数 / 总次数）。

### Evolution（自然改进）

- **GOAL-4** [P0]: **Skill rewrite proposal** — 当某 skill 的 failure rate > 30%（最近 20 次），或被连续纠正 3 次，LLM 基于 learned patterns 重写 SKILL.md 的 instructions 部分。重写结果保存为 `.gid/proposals/{skill-name}-v{N}.md`，**不自动部署**。

- **GOAL-5** [P0]: **Ritual strategy learning** — 追踪每个 ritual phase 的耗时和失败率。当某 phase 连续失败 > 2 次（同类任务），记录失败原因并调整下次该 phase 的 task description。调整方式：在 task description 前注入 "上次失败原因：{reason}，请避免：{pattern}"。

- **GOAL-6** [P1]: **Trigger tuning** — 追踪 skill 的误触发（触发后被纠正）和漏触发（potato 手动做了 skill 应该做的事）。当误触发率 > 20% 或有明确漏触发 pattern 时，提议修改 triggers.patterns/keywords。

### Deployment（human-in-the-loop）

- **GOAL-7** [P0]: **Telegram approval** — 每个 proposal（skill rewrite 或 trigger change）通过 Telegram 发给 potato，显示：当前版本 vs 提议版本的 diff + 触发原因 + 相关 failure 数据。potato 回复 approve/reject。

- **GOAL-8** [P0]: **Version history** — 每次 approved 的变更，旧版本保存到 `skills/{name}/versions/v{N}.md`。支持 `/evolve rollback {skill} {version}` 回滚。

- **GOAL-9** [P1]: **Auto-rollback** — 部署新版本后，追踪接下来 10 次执行。如果 failure rate 比旧版本高 > 15%，自动回滚并通知 potato。

### Control

- **GOAL-10** [P0]: **Kill switch** — `/evolve off` 关闭所有自进化，`/evolve on` 开启，`/evolve status` 查看状态（各 skill 的 success rate、pending proposals、最近改进）。状态持久化。

- **GOAL-11** [P1]: **Token budget** — 自进化相关的 LLM 调用（pattern extraction、skill rewrite）每日上限 50K tokens（可配置）。达到后停止直到次日。

- **GOAL-12** [P1]: **Idle-only execution** — Pattern extraction 和 skill rewrite 只在空闲时运行（最后一条用户消息 > 5 分钟，无活跃 ritual）。不影响正常响应速度。

## Guards

- **GUARD-1** [hard]: SOUL.md、AGENTS.md Safety section 永不被修改。Skill evolution 只改 SKILL.md 的 instructions 和 triggers，不改 agent 核心行为。
- **GUARD-2** [hard]: 所有变更经 potato approve 后才部署。唯一例外：auto-rollback 到上一个 approved 版本。
- **GUARD-3** [hard]: 不删除任何数据。Journal、patterns、旧版本永久保留。
- **GUARD-4** [soft]: 进化后的 skill instructions 长度不超过原版 1.5x。同效果更短优先。

## Out of Scope

- 遗传算法 / 多目标优化 — 不需要，LLM rewrite + human judgment 就够
- System prompt 优化 — 暂不动，风险太高
- Memory optimization — engram 有自己的 consolidate 机制
- A/B 测试 — 样本量不够，sequential deploy + rollback 更实际
- Evaluation framework — 不需要独立评估系统，execution journal 本身就是评估数据

## Dependencies

- **execution-journal.jsonl** — 新建，本系统的核心数据源
- **RustClaw ritual_runner** — 需要在 phase 完成时 emit journal entry
- **RustClaw skill executor** — 需要在 skill 执行后 emit journal entry
- **Telegram channel** — approval 通知

## Implementation Notes

可以分两步：
1. **Week 1**: GOAL-1 + GOAL-2（纯 observation，开始积累数据）
2. **Week 2+**: GOAL-4 + GOAL-5 + GOAL-7（有数据后开始进化）

其余 GOALs 按需加。

---

**Summary: 12 GOALs** (6 P0 / 6 P1) **+ 4 GUARDs** (3 hard / 1 soft)
