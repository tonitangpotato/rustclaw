# 一页纸术语速查表 — Phoebe Marketing 用

> 给 Phoebe 做 GID / RustClaw / Engram 相关 marketing 用的术语速查表。
> 重点：**对工程师受众保留术语，对大众受众翻译成"人话版"**。

---

## 🧱 代码结构类

| 术语 | 一句话解释 | 大众版（marketing 用） |
|---|---|---|
| **Module（模块）** | 一段代码组成的功能单位 | "一个功能模块"，类比公司的一个部门 |
| **auth module** | 负责"判断你是谁"的代码（登录、密码、验证） | "登录认证那部分" |
| **session module** | 记住"你已经登录了"的代码 | "登录状态管理那部分" |
| **utils.rs** | 一堆全项目都会用的小工具函数 | "公共工具函数" |
| **Directory / folder structure** | 文件夹的组织方式 | "项目的文件夹结构" |
| **Tightly-coupled** | 两段代码绑得很死，改一个另一个就要改 | "高度耦合"或直接说"紧密关联" |
| **Loosely-coupled** | 两段代码各管各的，互不影响 | "解耦"或"独立" |
| **Refactor（重构）** | 不改功能，只整理代码结构 | "代码整理"或"代码翻新" |

---

## 🌐 图与依赖类

| 术语 | 一句话解释 | 大众版 |
|---|---|---|
| **Dependency graph（依赖图）** | "谁用了谁"的关系网 | "代码之间的关联地图" |
| **箭头方向 A → B** | A 依赖 B（A 离不开 B） | "A 用到了 B" |
| **Downstream call sites** | 用到这段代码的所有其他地方 | "下游用到的地方" |
| **Blast radius（爆炸半径）** | 一个改动会波及多大范围 | "影响范围" |
| **Impact slice（影响切片）** | 一次改动会影响到的所有东西的清单 | "影响清单" |
| **Edge（边）** | 图里两个节点之间的那条线 | "一条关联" |
| **Node（节点）** | 图里的一个点（一个函数 / 一个文件） | "一个代码单元" |

> **箭头方向 cheat**：A → B 意思是 "A 需要 B"。顺着箭头走 = "我需要什么"，逆着箭头走 = "我影响了谁"。

---

## 🤖 AI / Agent 类

| 术语 | 一句话解释 | 大众版 |
|---|---|---|
| **Agent** | 能自己执行多步任务的 AI | "AI 助手"或"AI 智能体" |
| **Coding agent** | 专门写代码的 AI agent | "AI 编程助手" |
| **LLM** | Large Language Model，大语言模型 | "AI 大模型" |
| **Token** | LLM 处理文本的最小单位（≈0.75 个英文单词） | "AI 处理文字的计量单位" |
| **40k tokens** | 大约 3 万英文单词，相当于一本书的一章 | "一大段文字" |
| **Context（上下文）** | 你一次性给 AI 看的所有材料 | "AI 当前能看到的信息" |
| **Context window** | AI 一次最多能看的内容长度 | "AI 的'视野'大小" |
| **Local model / Local LLM** | 跑在自己电脑上的 AI 模型，不联网 | "本地 AI 模型" |

---

## 🔬 算法 / 技术类

| 术语 | 一句话解释 | 大众版 |
|---|---|---|
| **AST** | 代码被解析成的树状结构 | "代码的结构化表达" |
| **AST parsing** | 把代码读成结构化形式 | "解析代码结构" |
| **tree-sitter** | 一个开源的 AST parsing 工具 | "业界标准的代码解析工具" |
| **Community detection** | 一类算法，能在网络里自动找出"抱团的群体" | "自动分组算法" |
| **Infomap** | 一个具体的 community detection 算法（来自信息论） | "一种聪明的自动分组算法" |
| **Louvain / spectral clustering** | 另外两种 community detection 算法 | （提一下名字就好，不用展开） |
| **Random walker（随机游走）** | 想象一个小人在图上乱走，用来分析图的结构 | "想象有个小机器人在图里游荡" |
| **MDL（最小描述长度）** | 信息论概念："用多少字能把这件事说清楚" | （对大众别提，太硬） |

---

## 💻 工程语言 / 工具类

| 术语 | 一句话解释 | 卖点关联 |
|---|---|---|
| **Rust** | 一种编程语言，特点是**快**和**安全** | potato 用 Rust 写 = "性能好、稳定" |
| **TypeScript / TS** | 前端最流行的语言之一 | "支持 TS" = 前端工程师用户群覆盖 |
| **Python** | AI / 数据科学最流行的语言 | "支持 Python" = AI 工程师用户群覆盖 |
| **Crate** | Rust 里"一个软件包"的意思 | 类比 Python 的 package |
| **cargo install** | Rust 的安装命令 | 一行命令装上工具 |
| **CLI** | Command Line Interface，命令行工具 | "在终端里直接用" |

---

## 📣 受众分层（Marketing 关键）

**🛠️ 工程师受众**（HN, r/LocalLLaMA, r/programming, 工程师 Twitter）
→ **保留术语**。术语是信任感的来源。砍掉术语反而会显得 "不专业 / 像营销号"。

**👥 大众受众**（普通 Twitter, LinkedIn, 投资人, 创始人圈）
→ **全部翻译成大白话**。一个英文术语都不留，或者首次出现时括号解释一次。

**🎯 半专业受众**（PM, 技术 leader, 早期客户）
→ **混合**。核心术语保留（agent, LLM, context），细节术语翻译（Infomap, MDL, AST）。

---

## 🎯 GID 一句话定位（不同受众版本）

- **工程师版**：A graph-based substrate that lets coding agents reason about codebases by structure instead of text.
- **半专业版**：让 AI 编程助手"看懂"代码之间的关联，而不是只能搜文字。
- **大众版**：让 AI 写代码不再"只见树木不见森林"。

---

## ✏️ 常见误解 cheat

- ❌ "依赖图就是文件夹结构" → ✅ 文件夹是人为组织，依赖图是真实关联，两者经常不一样
- ❌ "AI 看代码越多越好" → ✅ AI 看太多无关代码反而效果差（context 被挤占）
- ❌ "Refactor 就是改代码" → ✅ Refactor 是**不改功能**只改结构，区别于 "fix bug" 或 "add feature"
- ❌ "箭头 A → B 意思是 A 给了 B 东西" → ✅ 是 **A 需要 B**（A 依赖 B）

---

*最后更新：2026-04-26*
*有新术语随时让 RustClaw 加进来。*
