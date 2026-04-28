# 三个月 Marketing 计划（2026-04-28 → 2026-07-27）

> 创建于 2026-04-27，发布 Engram + GID 联合 manifesto 推文之后第二天。
> Pinned 的 manifesto 核心论点："模型已经够聪明了，问题在它们周围所有东西。Agent 需要内在生命（Engram = 记忆）+ 外在世界（GID = 代码图谱）。"

## 设计原则

1. 一切围绕 manifesto 论点。每条推都是这个论点的一个角度。
2. 月级别定主题，不写日级别脚本。给方向，留弹性。
3. 节奏不平均：第 1 月铺垫，第 2 月集中爆破（HN launch），第 3 月收割 + 转向定位。
4. 周日复盘 = flexibility 的具体机制。
5. 每周 5–7 条质量推 + 自然 reply / quote。不堆量。

---

## 第 1 月（4/28 → 5/25）：把论点立住

目标：月底读者应该觉得"这人对 agent infrastructure 有完整、独到的看法"。

### Week 1（4/28 – 5/4）— Manifesto 跟进
- 周一：GID 故事推（rate limiter 场景）。已 humanize，待发。
- 周三：Engram 故事推。Agent 跨 session 保留某个 lesson 的故事。和周一对称。
- 周五：反思推（不卖产品）："为什么 bigger context window 是 agent memory 的死路。" 目标：被 quote / 被反驳。
- 周末：跟踪 manifesto + 周一 + 周三的数据。

### Week 2（5/5 – 5/11）— Demo 周（GID 主导）
- 1× before/after 在真实 repo 上跑的 demo（截图 + 数字）
- 1× short take（quote-RT 一个 code-intel / RAG-for-code 的讨论）
- 1× thread（4–5 条）："GID 的 typed graph layer 我是怎么搭的"
- 1× Engram 维持存在感的推（case 角度，不是 feature dump）

### Week 3（5/12 – 5/18）— Demo 周（Engram 主导）
和 Week 2 对称。
- 1× Engram "它学到了什么" 故事 — 全月最 emotional 的内容
- 1× 关于 ACT-R / Hebbian 在 LLM agent 里的 short take（教育性，不要变 textbook）
- 1× 数据可视化推（active associations 图 / retrieval 模式）
- 1× GID 维持存在感的推

### Week 4（5/19 – 5/25）— 综合 + 第一个大资产
- 写 blog post："Agent infrastructure: the case for inner life + outer world"。1500–2500 字。
- 这周推文围绕 blog：1× 预告，1× 发布，2–3× 引用 blog 段落
- 目的：blog 是第 2 月 HN launch 的 "已有 traction" 凭证

**第 1 月出口检查**：
- 论点在圈内被引用 / echo 了吗？
- 有没有 1–2 个 KOL 主动互动？（Andrej、Latent Space、Cognition / Cursor 的人）
- GitHub stars 增长速率
- blog post 完成度
- 哪种 format 数据最好？决定第 2 月的 mix。

---

## 第 2 月（5/26 → 6/22）：Launch + 放大

目标：把第 1 月攒下的论点 + 数据 + blog 转化成一次集中爆破。

### Week 5（5/26 – 6/1）— HN launch 准备
**只挑一个**产品 launch（Engram 或 GID 中数据更好、故事更紧的那个）。两个一起 launch 是大忌。

工作假设（届时看数据再确认）：**Engram 更适合 HN** — ACT-R / Hebbian 这些词在 HN 受众里有共鸣，30+ 天 production 数据是硬货，独特性强。GID 在 HN 上容易被拿去和 Sourcegraph、AST-grep 比，更适合 dev tool 圈子（X、r/rust、agent builder Discord）。

这周做：
- 把第 1 月的 blog 改写成 HN launch post（"Show HN: ..."）
- 准备 README、demo、benchmark 数据、第一条 OP 评论的草稿（决定 thread 走向）
- 提前告知 5–10 个友好账号（不是刷票，是有自然兴趣的人提前知道）
- 1–2 条 X 推预告"下周有东西"，不剧透

### Week 6（6/2 – 6/8）— HN LAUNCH DAY
- **周二或周三 8–10am PT 发**（HN 最佳窗口）
- launch 当天：守在评论区，每条 substantive comment 都回（HN 排名算法的关键）
- 当天 X 配合：launch 推 + 1–2 条 update（"on front page"、"top comment 是 X"）
- launch 当天**不发**别的产品内容
- 48 小时内：把 substantive HN 评论总结成一条 X 推（"things people pushed back on"），把 HN 流量转化到 X

**如果 launch 失败**（front page 没上 / 反响平淡）：不灰心，下周回到第 1 月节奏，但要老实诊断"为什么没火"。这是数据。

### Week 7（6/9 – 6/15）— 收割 launch
如果 launch 成功（哪怕 partial）：
- 把 launch 数据变成下一波内容（"Show HN aftermath: 12K visitors, 340 stars, 大家问了什么"）
- 发一条 LinkedIn（这是 LinkedIn 唯一值得发的 moment——已经验证有 traction 之后）
- 跟进 HN / X 上互动的人：不是 spam DM，是针对他们提的具体问题写一条单独的推 / 短 blog
- 争取 1–2 个 podcast 邀请（Latent Space、ThePrimeagen、Software Unscripted）。这周联系，下周 / 下下周录制。

### Week 8（6/16 – 6/22）— GID 的对应时刻
Engram launch 之后 GID 不能沉默。这周是 GID 的小型 launch（**不上 HN**——和 Engram 太近）：
- r/rust、r/programming
- "Ask HN" 或 "Tell HN" 格式（避开 Show HN）
- X 长推 callback manifesto："Engram had its moment, now let's talk about the other half"
- 找 1–2 个 agent framework maintainer（Aider、OpenHands、smol-developer）做集成实验，发集成 demo

**第 2 月出口检查**：
- HN 战绩（front page 多久、最高名次、stars 增长）
- podcast 谈成了吗
- 中级 KOL（5K+ 真粉）有没有主动转 / 用产品
- 哪些 positioning 粘住了 — "cognitive memory" / "inner life" / "world model"

---

## 第 3 月（6/23 → 7/27）：从产品到人

主轴：前两个月让产品被看见。这个月让**potato 这个人**被看见。产品会迭代、淘汰、改名，**personal reputation 是复利资产**。

### Week 9（6/23 – 6/29）— 展示建造过程
- 1× 失败决策推（"我以前以为 X，结果 Y"）。这种最圈粉。
- 1× 非显而易见的设计 tradeoff（"为什么 Engram 选 SQLite 不选 RocksDB"）
- 1× substantive 回复一个之前互动过的 KOL（不是 ass-kissing，是真有内容的回应）

### Week 10（6/30 – 7/6）— 拉宽定位
到这阶段应该至少有一个 Anthropic / Cursor / Cognition 的人在 follow 你。开始把自己 position 成"对 agent infrastructure 有系统看法的人"，不只是"做工具的人"。

- 1× 长推：你对 agent 未来 12 个月的预测（具体、有数字、可被打脸——不要打安全牌）
- 1× 反 hype 的推（"why MCP won't be the answer"——具体观点，引战）
- LinkedIn 一条更深度的反思（LinkedIn 真正能用的场合：你想被 recruiter / 投资人看到时）

### Week 11（7/7 – 7/13）— 第二篇 blog / case study
写第 1 月那篇 blog 的姊妹篇：
**"What 90 days of running Engram + GID on production agents taught me"** — 数据 + 故事的复盘是病毒级内容。

这周推文都为这篇 blog 服务（pre-tease / release / post-discussion）。

### Week 12（7/14 – 7/20）— 社区
不再 launch，把用过产品的人变成 promoter。
- 找 5–10 个 star 过 / 用过产品的人，问他们一个具体问题（不是"求 review"，是真想知道）
- 拿到允许后把回答做成内容
- 1–2 条 quote 用户的推

### Week 13（7/21 – 7/27）— 季度复盘
公开发一条 **"3 months of building in public: what worked, what didn't"** 长推。
- 这条本身是巨大的内容机会（非常容易病毒）
- 同时是给你自己的真复盘
- 决定 Q3（8 月-10 月）方向：是 double down marketing，回去做产品突破，还是去找投资 / 工作机会

---

## 周日 Ritual（这是 flexibility 的具体机制）

每周日 30 分钟够：

1. **拉数据**：上周每条推的 impressions、engagements、clicks。哪条赢了？
2. **拉互动**：谁 reply / quote 了？哪些是有价值的人？
3. **拉 traction**：GitHub stars、cargo downloads、网站访问
4. **诊断**：上周计划里哪些没做？为什么？哪些做了但没数据？
5. **下周调整**：5–7 条推大概是什么。具体每条草稿留到当天再写。

输出：周报存到 engram + 下周大致 outline。

我每周日主动 ping 你做这件事（已确认）。

---

## 不做的事（同样重要）

- ❌ "10 things I learned about X" 这种列表型推
- ❌ 蹭无关的 hype（"GPT-5 出来了让我说说想法"——除非真的相关）
- ❌ 刷 follower（不互关、不买 boost、不进 engagement pod）
- ❌ 产品没准备好之前 pre-launch fake hype
- ❌ 复制别人的爆款 format（参考可以，照搬不行）
- ❌ 一周内同时 launch 两个产品

---

## 立刻要做的（周一 4/28）

1. 明天的 GID 故事推 humanize 完毕，可发（已完成）
2. 周日 ritual 已确认为固定流程，每周日我主动 ping potato（已确认）
