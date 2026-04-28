# STRATEGY.md — Niche Data → AI Analysis → Sell

> potato的核心打法。不是vision,是operational playbook。
> 起源:2026-04-27 与 RustClaw 对话(经过两轮校准)。

---

## 一句话

**找信息差大的niche → 收公开数据 → AI分析结构化 → 卖给最痛的人/工具/agent。**

不是独家洞察。是个**做法**。差异化在执行,不在认知。

---

## 节奏(以周/月为单位,不是年)

AI产品周期:Cursor 12个月到$100M、Lovable 3个月到$10M、MCP server可能几周被淘汰。

→ 任何"2年规划"都是幻觉。规划粒度:
- **本周做什么**:具体到tool/PR/post
- **本月验证什么**:一个具体假设
- **3个月后能不能赚到第一笔钱**:有就继续没有就砍

→ 任何超过3个月没产生信号的方向 = 砍。

---

## Pipeline(都做这个,不同niche复用)

```
公开数据源 → 收集 → AI分析 → 结构化 → 卖
```

每个新垂直只新增:数据源连接器 + 垂直的分析逻辑 + delivery surface(API/dashboard/MCP)。

底层复用:xinfluencer scraper、engramai memory、causal-agent推理、GID图。

---

## 候选垂直(不是要全做,是选1-2个集中打)

| 垂直 | 谁付钱 | 付费意愿 | 合规难度 | 数据采集难度 | 已有积累 |
|---|---|---|---|---|---|
| **Trading signals** | 散户/quant/自用 | 极高 | 中(disclaimer) | 中 | autoalpha+HIRO已经在做 |
| **Dating intel** | 约会的人 | 极高 | **极高**(GDPR/反stalkerware) | 中 | 0 |
| **GTM forensics** | indie hackers | 中-高 | 低 | 中 | 0(但有竞品GrowthHunt) |
| **GidHub** | coding agent/dev工具 | 中 | 低 | 已有 | 高(gid-core published) |
| **公司情报** | 求职者/投资人 | 中 | 中 | 低 | 0 |
| **招聘背调** | HR/小公司 | 高 | **极高**(FCRA) | 中 | 0 |

---

## 真实选择题

按 **3个月内能不能拿到第一个付费用户** 排序:

1. **autoalpha trading signals** — 已经有数据有模型,差打包+卖。最快现金流候选。
2. **GTM forensics** — 合规最简单,有竞品但市场大,潜在自用价值高。
3. **GidHub** — 战略最重,但市场还没起,需要等开发者workflow变化。
4. **Dating/招聘背调** — 付费意愿最高,但合规门槛先要跨过去,启动成本高。

---

## Decision Filter (5个问题,过不了就砍)

1. 这个niche的信息差**大不大**?(小 → 没溢价 → 砍)
2. 数据**能合法拿吗**?(灰色 → 想清路径再开)
3. 能复用已有的collection/memory/reasoning层吗?(不能 → 砍或推迟)
4. **谁会先付钱**?具体一个画像。(说不出 → 还没想清楚)
5. 3个月内能跑出**第一笔收入** OR **第一个付费用户**吗?(不能 → 拆小或砍)

---

## 不做什么

- ❌ raw数据转售(法律灰色 + 没差异化)
- ❌ 套壳chat产品
- ❌ potato自己根本用不上的niche(没有迭代闭环)
- ❌ 没有AI分析层的纯爬虫(没溢价)
- ❌ 用"年"做规划单位的方案

---

## 当前堆栈(基础设施,可复用)

| Layer | Tool | Status |
|---|---|---|
| Collection | xinfluencer (Twitter scraper) | 部分实现 |
| Memory | engramai | v0.2.2 published, v0.3 in progress |
| Code graph | GID + GidHub | gid-core v0.2.1 published |
| Causal reasoning | causal-agent | 早期 |
| Agent runtime | RustClaw | v0.1.0, 140 tests |
| Distribution | API网关 / MCP server | **缺口** |
| Billing | 无 | **缺口** |

---

## 下一步(本周可做)

待 potato 选一个垂直,然后:
1. 起requirements.md(走draft-requirements skill)
2. 决定独立repo or 现有项目子模块
3. 第一个数据样本手工跑通pipeline

---

*Updated: 2026-04-27 — 砍掉了所有"年级"叙事和"agent-first"光环话术。*
