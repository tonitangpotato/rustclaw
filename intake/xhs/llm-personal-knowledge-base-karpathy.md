# 用LLM打造个人知识库

- **URL**: https://www.xiaohongshu.com/discovery/item/69cfea63000000001a022f06
- **Platform**: 小红书
- **Author**: Unknown (手动 intake)
- **Date**: 2026-04-03
- **Fetched**: 2026-04-03T21:26:00-04:00
- **Category**: tech/product
- **Tags**: LLM, knowledge-base, personal-knowledge-management, Karpathy, Obsidian, content-pipeline, information-hoarding
- **Extraction Method**: manual (user pasted text)

## Summary

作者分享了自己用 LLM 构建个人知识库的完整流程，发现和 Karpathy 昨天发推说的方案惊人相似。核心是：信息抓取（浏览器插件）→ 存储到 Obsidian → LLM 提取有效信息 → 自动生成项目库 → 知识沉淀。并延伸出"内容飞轮"看板（Backlog → WIP → Schedule → Posted），全流程 LLM 辅助。

## Key Points

- 自己写浏览器插件一键抓取图文视频（比 Obsidian Web Clipper 更灵活）
- 存到 Obsidian 的 Inspiration 目录，LLM 用 grep 提取信息
- 写了 Obsidian 插件提供 Inspiration + Projects 视图和内容看板
- 内容飞轮看板：Backlog → WIP → Schedule → Posted，LLM 辅助推进
- Karpathy 用 Obsidian Web Clipper，作者觉得不够顺手自己造轮子
- 项目代号 Nuthouse（坚果屋）— 信息囤积像松鼠采集坚果
- 核心洞察：LLM 之前是"机械团结"（碎片散落），LLM 之后是"有机团结"（碎片可以被串联、模块化重组）
- 引用卡夫卡《万里长城建造》的分段式工作比喻
- Karpathy 认为这个方向值得做新产品

## Potential Value

**与 potato 的系统高度相关：**
- RustClaw 的 Engram + Social Intake + GID 就是同一套系统的 Rust-native 版本
- 差异：potato 的系统是 agent-native（不依赖 Obsidian），支持多平台自动抓取，有认知记忆（ACT-R/Hebbian）
- 这个方向被 Karpathy 背书 = 市场验证，值得考虑产品化
- "内容飞轮"看板的思路可以借鉴到 RustClaw 的内容生成流程
- "Nuthouse"和 potato 的信息囤积习惯完全一致

## Connections Found

- **RustClaw Social Intake** — 本质上是同一套 pipeline，但 potato 的版本更底层（Rust + Agent，不依赖 Obsidian GUI）
- **Engram (engramai)** — 比 Obsidian + grep 更强大：ACT-R 激活模型、Hebbian 关联学习、情感加权
- **GID (gid-core)** — 项目知识图谱，比 Obsidian 的文件夹结构更结构化
- **Marketing Automation Pipeline (IDEA-20260402-02)** — "内容飞轮"看板 = 发布流水线的一部分
- **Skill-JIT (REF-20260403-01)** — 都在探索 LLM 辅助的知识工作流

---

## Raw Content

昨天Karpathy发推说用LLM构建个人知识库，今天刷到有人转发，点开一看这不就是我从年初就在做的事吗！而且流程惊人相似，英雄所见略同啊哈哈

01 我的做法🛠️
- 网上各种信息源（推特、YouTube、小红书、领英…）
- 自己写了个浏览器插件，一键抓取图文视频，视频后台下载，图文转成Markdown
- 存到Obsidian的「Inspiration」目录
- LLM提取有效信息 → 自动生成项目库Project → 形成知识沉淀

02 Karpathy的流程对比📌
信息抓取他用的是Obsidian Web Clipper，我觉得不够顺手，就自己写了插件以便直接下载图文和视频。用Obsidian做IDE是非常顺手的，所有内容均本地存贮，LLM可以直接使用grep进行信息提取，但是信息呈现稍弱，所以我也写了个Obsidian插件，用来提供Inspiration + Projects视图以及内容看板流程。这个内容看板也就是「内容飞轮」看板(类似Trello): Backlog → WIP → Schedule → Posted，全流程LLM辅助推进。

03 更长远的意义🌊
我觉得这种LLM知识库会越来越普及，尤其是需要消费+生产信息的人。 我的项目代号Nuthouse坚果屋，因为对于想我这样有信息囤积癖的人，囤积信息就像松鼠采集坚果一样哈哈。 之前我到过一个更形象的比喻，在LLM出现之前，通过web clipper进行的信息囤积最多算是粗浅的"机械团结"，囤积的内容像是充满废墟的海滩，我们只是是在单纯地捡拾贝壳，允许这些信息素材呈现完全的"去中心化"状态。但是LLM出现之后我们可以触摸到"机械团结的反面，有机团结。就是一根可以把我在海滩上四处剪到的贝壳串起来的线"。在LLM的帮助下我们可以更有效的进行模块化重组： 这些碎片随时可以被放弃，也可以随时被重新组装。就好比卡夫卡的万里长城建造，是一种分段式的工作方式，"在另一处的新的营造过程中，我又会去上一处的废墟中去拆一些能用的材料作为模块在添加进新的碎片中"。

04 写在最后💬 Karpathy说这方向值得做一个新产品，我深表赞同
