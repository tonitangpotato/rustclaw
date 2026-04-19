# IDEAS.md - Idea Repository

> All ideas captured by RustClaw's Idea Intake pipeline.
> Format: newest first. Each idea has a unique ID for cross-referencing.

## IDEA-20260417-01: OSINT Profile Dossier — 社工背调产品
- **Date**: 2026-04-17
- **Triggered by**: potato 收到 cold email，RustClaw 用公开信息交叉验证对方身份，效果惊人
- **Category**: product / SaaS
- **Tags**: osint, due-diligence, background-check, social-engineering, profiling, trust-scoring

### The Idea
给一个名字/邮件/社交 handle → 自动生成一份 **profile dossier**：
- 社交账号关联（Twitter、GitHub、LinkedIn、个人网站交叉匹配）
- 项目真实状态（有没有代码、有没有 traction、repo stars vs 实际 commit 活跃度）
- 言行一致性分析（声称做了什么 vs 实际做了什么）
- Red flags 检测（GitHub suspended、项目全 404、scope 过大无产出）
- Credibility score（综合评分）
- 关联人脉网络（谁和谁互动、mutual connections）

### Origin Story
potato 收到一封 cold email，声称在建 "bounded AGI cognitive architecture"。RustClaw 用几步公开信息查询就完成了一次完整的背调：
1. 名字 → Twitter 搜索 → 找到账号 → 发现是 17 岁高中生
2. GitHub → 账号被 suspended
3. 网站 → 花哨但无实质内容
4. 推文分析 → 典型 "building in public" 鸡汤，技术深度浅
5. 声称的 benchmark 数字无法验证

整个过程 < 2 分钟，几个 API 调用。**这个流程产品化就是一个 SaaS。**

### Use Cases
1. **投资人 founder due diligence** — VC 看 deck 前先跑一次，5 分钟知道 founder 靠不靠谱
2. **招聘背调** — HR 验证候选人简历 vs 实际 GitHub/项目经历
3. **BD/Partnership 评估** — 合作前了解对方公司/个人真实状况
4. **开源维护者** — 评估贡献者可信度（是真的开发者还是 spam PR）
5. **反诈/反骗** — 验证网络身份真伪
6. **Influencer 验证** — 品牌合作前确认 KOL 是否注水

### 已有基础设施
- **xinfluencer** crawler — 已有 Twitter 爬取能力
- **bird CLI** — Twitter 搜索和数据获取
- **RustClaw agent** — 多步骤自动化 + LLM 分析
- **engram** — 存储和关联已查询的 profile 数据

### 数据源

**Tier 1 — Core（MVP 必须）：**
- Twitter/X — 发言、互动、bio、关注网络、发帖频率
- GitHub — repos、commit 活跃度、contribution graph、账号状态、stars vs 实际代码量
- LinkedIn — 职业经历、教育、endorsements、connections
- 个人网站/博客 — 域名 whois、内容分析、技术栈检测
- Google search — 名字 + 关键词，公开信息汇总

**Tier 2 — Enhanced（Pro 版）：**
- Crunchbase — 创业/融资记录
- ProductHunt — 产品发布记录、upvotes
- HackerNews — 发帖/评论历史（Algolia API 免费）
- Reddit — 发言历史、karma
- Google Scholar — 学术论文验证
- Medium / Substack — 写作内容、followers
- YouTube — 技术演讲、demo 视频
- npm / crates.io / PyPI — 发布过的包、下载量
- Stack Overflow — 技术回答、reputation
- Domain WHOIS — 网站注册时间、注册人
- Wayback Machine — 历史快照（验证时间线声称）
- Discord / Telegram 公开群 — 社区参与度
- PGP keyservers / Keybase — 身份关联
- Gravatar — 邮件关联头像和其他账号

**Tier 3 — Deep（Enterprise 版）：**
- 公司注册信息（SEC, Companies House, 天眼查）
- 专利数据库
- 法院/诉讼记录（公开的）
- App Store / Play Store — 发布的 app、评分
- 学历验证 API

### 交叉验证引擎（Cross-Validation Engine）

**核心原理：不是看单一数据源，而是看多源信息的一致性。**

每个数据源产出的是 **claims（声称）**，交叉验证就是检查 claims 之间有没有矛盾。

#### Phase 1: Claim Extraction（提取声称）
每个数据源提取结构化 claims：
```
Source: LinkedIn
  claim: "Senior Engineer at Google, 2020-2023"
  claim: "MS Computer Science, Stanford, 2019"
  claim: "Built ML pipeline serving 1M users"

Source: Twitter bio
  claim: "Ex-Google | Stanford CS | Building AGI"

Source: GitHub
  claim: account_created: 2018
  claim: total_repos: 47
  claim: commits_last_year: 342
  claim: orgs: [google] → NOT FOUND
```

#### Phase 2: Claim Pairing（配对同类声称）
把来自不同源的同类 claims 配对：
```
Pair 1: LinkedIn "Senior Engineer at Google" ↔ GitHub org membership
  → GitHub 没有 @google org → ⚠️ UNVERIFIED (不一定是假的，但无法证实)

Pair 2: LinkedIn "Stanford CS 2019" ↔ Google Scholar publications
  → 没有找到 Stanford 相关论文 → ⚠️ UNVERIFIED

Pair 3: Twitter "Building AGI" ↔ GitHub repos
  → repos 全是 tutorial forks, 0 original ML code → 🚩 INCONSISTENT

Pair 4: LinkedIn "2020-2023 at Google" ↔ GitHub commit timeline
  → 2020-2023 commit 都是白天(PST) → ✅ CONSISTENT (上班时间 commit 符合湾区)
```

#### Phase 3: Consistency Scoring（一致性评分）
每对 claims 给一个状态：
- ✅ **VERIFIED** — 多源互相证实（权重 +2）
- ☑️ **CONSISTENT** — 不矛盾，间接支持（权重 +1）
- ⚠️ **UNVERIFIED** — 只有单一来源，无法证实（权重 0）
- 🚩 **INCONSISTENT** — 多源矛盾（权重 -3）
- ❌ **FALSIFIED** — 明确证伪（权重 -5）

最终 credibility score = Σ(claim_weights) 归一化到 0-100

#### Phase 4: Pattern Detection（模式检测）
超越单条 claims，看整体模式：
- **Scope inflation** — 声称做的东西远超实际产出（RAVANA case）
- **Timeline gaps** — 简历说在 Google 但那段时间 GitHub 完全沉默
- **Vanity metrics** — 很多 followers 但 engagement rate 极低
- **Echo chamber** — 互动的人全是同一批 "building in public" 圈子
- **Ghost projects** — 声称创办/维护的项目全是 404

#### 刚才那个 case 的实际交叉验证过程：
```
Input: Likhith, email, oxiverse.com

Step 1: Name → Twitter search → @ItxLikhith
  claims: "17yo", "building privacy-first search", "oxiverse.com"

Step 2: Twitter → GitHub link → github.com/itxLikhith
  claims: account SUSPENDED → 🚩 RED FLAG

Step 3: oxiverse.com → website content
  claims: "RAVANA cognitive architecture", "ARC 94.7%"

Step 4: Cross-validate
  - "RAVANA" code repo → GitHub suspended → ❌ CANNOT VERIFY
  - "ARC 94.7%" → no paper, no benchmark link → ⚠️ UNVERIFIED
  - Website claims many products → all "coming soon" → 🚩 SCOPE INFLATION
  - Age 17 + claim "bounded AGI" → 🚩 EXTRAORDINARY CLAIM, MINIMAL EVIDENCE

Score: ~25/100 (low credibility, many unverifiable claims)
```

### 产品架构
```
Input: name / email / @handle / URL
    ↓
┌─────────────────────────────┐
│  Discovery Layer            │
│  - Identity resolution      │
│    (一个 handle → 找到所有   │
│     关联账号)               │
│  - Tier 1/2/3 数据采集      │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Claim Extraction (LLM)     │
│  - 每个源 → 结构化 claims   │
│  - entity: person/org/proj  │
│  - type: employment/edu/    │
│    project/skill/metric     │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Cross-Validation Engine    │
│  - Claim pairing            │
│  - Consistency scoring      │
│  - Pattern detection        │
│  - Timeline analysis        │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Report Generator           │
│  - Profile dossier          │
│  - Credibility score 0-100  │
│  - Red flags list           │
│  - Confidence per claim     │
│  - Evidence links           │
└─────────────────────────────┘
```

### 盈利模式
- **Freemium**: 每月 5 次免费查询，之后按次收费（$2-5/query）
- **Pro**: $29/mo unlimited queries
- **Enterprise**: API access + batch queries + CRM 集成
- **Premium reports**: 深度报告 $20-50/份（包含人工审核）

### Competitive Landscape
- **Clearbit** — 公司级 enrichment，偏 B2B sales
- **Hunter.io** — 邮件查找，不做 credibility analysis
- **Pipl** — 人物搜索，贵，偏执法/企业
- **差异化**: 我们的核心是 **credibility/consistency analysis**（LLM 分析言行一致性），不只是 data aggregation

### Risks
- 隐私法规（GDPR, CCPA）— 只用公开信息，但需要注意合规
- Platform ToS — Twitter/GitHub API 使用条款限制
- 准确性 — LLM 分析可能有误判，需要 confidence scoring

### 社工方法论应用（Social Engineering Techniques）

**1. Pivot 技术（轴心跳转）— 社工最核心技能**
- 邮件 → 用户名规律（likhith.seemala → likhith_seemala → itxlikhith）
- 用户名 → 全平台搜同名（Sherlock/Maigret 思路，300+ 平台枚举）
- 头像 → 反向图片搜索 → 找到其他账号
- 域名 whois → 注册邮箱 → 这个邮箱还注册了什么
- 手机号 → Telegram/WhatsApp/Signal 头像泄露

**2. Sock Puppet / 伪装检测**
- 账号创建时间聚集（同一周创 5 个平台 = 刻意打造人设）
- 写作风格分析（stylometry/NLP）— 两个"不同的人"用词习惯一样
- 时区泄露 — 声称在硅谷但发帖时间全是印度时区

**3. OSINT 情报循环**
- Collection → Processing → Analysis → Dissemination
- 关键是 **tasking**：带假设去验证，不是漫无目的收集
- 刚才对 Likhith 的调查就是这个流程的自然展现

**4. Digital Footprint Mapping**
- 被动足迹（别人提到你的）vs 主动足迹（你自己发的）
- 两者不一致 = red flag（自称 influencer 但从来没人主动 @你）

**5. Temporal Analysis（时间分析）**
- commit 时间 → 推断时区 → 验证声称的地理位置
- 发帖间隔 → 人 vs bot 判断
- 简历时间线 vs 实际活动时间线 → 找 gap

**6. Pretexting 反向应用**
- 检测别人的 pretext：声称的身份有没有对应的数字痕迹？
- 一个"CTO"从来没出现在公司公开材料里？
- 一个"researcher"没有任何学术 footprint？

**可直接产品化的技术模块：**
- Sherlock/Maigret 式用户名枚举（handle → 300+ 平台查存在）
- Stylometry NLP 分析（写作风格指纹，检测小号）
- 时区推断引擎（活动时间 → 地理位置）
- Pivot graph 可视化（证据链图谱）

**差异化洞察：** Maltego, SpiderFoot, Recon-ng 都是工具箱，需要专业人士操作。没有人做成「一键出报告 + LLM 分析一致性」的产品。AI + OSINT 几乎是蓝海——OSINT 社区和 AI 社区不重叠，传统工具面向专业人士（Maltego $999/yr），大公司怕 GDPR 不敢做 credibility scoring，LLM 推理能力刚刚成熟到可以做交叉验证。

### 产品进化：从背调工具到人脉情报系统

三层架构（独立产品，不并入 The Unusual）：

**Layer 1: Profile Dossier（可信度验证）**
→ 这个人是不是他说的那样？
- 多源数据采集 + LLM 交叉验证
- Credibility score 0-100
- Red flags + 证据链

**Layer 2: Social Graph（关系网络）**
→ 这个人认识谁？谁认识他？影响力链条是什么？
- Neo4j 知识图谱存储
- 节点：Person / Org / Skill / Project / Repo
- 关系边：WORKED_AT, COLLABORATED_WITH, ENDORSED_BY, EXPERT_IN, INVESTED_IN, FOLLOWS, CONTRIBUTED_TO
- PageRank / centrality 算影响力
- 最短路径查询（"我和目标之间隔几个人"）

**Layer 3: Strategic Value（对我的价值匹配）**
→ 这个人在什么方面对我有用？合作机会在哪？
- 技能/兴趣 overlap 分析
- 引荐路径发现（"谁能帮我连到 Anthropic"）
- Deal flow scoring（VC 尽调场景）

### Neo4j Graph 示例查询
```cypher
// 谁能引荐我到某公司？
MATCH (me)-[:KNOWS*1..3]-(target)
WHERE target.company = "Anthropic"
RETURN path

// 在 causal inference 领域谁最有影响力？
MATCH (p:Person)-[:EXPERT_IN]->(s:Skill {name:"causal inference"})
RETURN p ORDER BY p.pagerank DESC

// 这个人对我有什么价值？
MATCH (target)-[:EXPERT_IN]->(s)<-[:INTERESTED_IN]-(me)
RETURN s.name AS overlap_area
```

### 技术架构
```
┌─────────────────────────────┐
│  Data Layer                 │
│  Sherlock枚举 + API采集     │
│  (Twitter/GitHub/LinkedIn)  │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Knowledge Graph (Neo4j)    │
│  Person/Org/Skill/Project   │
│  关系边 + 属性              │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Analysis Engine            │
│  - Credibility scoring      │
│  - PageRank/centrality      │
│  - Value matching           │
│  - LLM cross-validation     │
└────────────┬────────────────┘
             ↓
┌─────────────────────────────┐
│  Output                     │
│  - Profile dossier (PDF/web)│
│  - Graph visualization      │
│  - Strategic brief          │
│  - API                      │
└─────────────────────────────┘
```

### 产品形态

**面向个人（PLG, self-serve）:**
- 输入 @handle → 出 dossier + 社交图谱
- "你和这个人的最短路径"
- "这个人在哪些方面对你有价值"
- 像 LinkedIn "共同联系人"，但基于公开数据 + 跨平台

**面向机构（sales/BD/VC）:**
- 批量 vetting（投资前尽调）
- 关系映射（"我们团队和目标公司之间有哪些桥梁"）
- Deal flow scoring（"这个创始人的 credibility score"）

### 与 The Unusual 的关系
- 独立产品，不并入 The Unusual
- 共享底层技术可能性：实体解析、LLM 交叉验证 pipeline
- The Unusual 分析事件/市场信号，本产品分析人/组织
- 两者未来可能通过 "谁在这个事件中是关键人物" 打通

### 面向个人 C 端场景（核心 GTM 切入点）

**场景 1: Online Dating 防骗（首选 MVP 切入点）**
- 输入对方的名字 / @handle / 手机号 → 出可信度报告
- 检测 catfish 信号：照片反向搜索、社交账号年龄、活跃度异常、声称身份 vs 数字足迹不一致
- "这个人说他是 Google 工程师" → 没有 LinkedIn、没有 GitHub、没有任何技术社区足迹 → 🚩
- 目标用户：约会 app 用户（Hinge/Bumble/Tinder），特别是女性用户（安全需求强烈）
- 竞品几乎为零：Social Catfish ($6/search) 但只做反向图片搜索，不做 LLM 一致性分析
- 美国 online dating 用户 3 亿+，romance scam 每年损失 $1.3B

**场景 2: 陌生人 Reach Out 筛选**
- 有人 cold DM / email / LinkedIn 联系你 → 30 秒出这个人的背景摘要
- 适用场景：freelancer 接单、投资人找创始人、合作方 BD、社交媒体互动
- "这个人说他是某 VC 的 partner" → 查 Crunchbase + LinkedIn + Twitter → 确认或 flag
- "有人想跟我合作开源项目" → 查 GitHub contributions + 技术社区 footprint

**场景 3: 加密/Web3 防骗**
- "这个项目方创始人是真人吗？" — 匿名文化 = 诈骗温床，rug pull 每年数十亿美元
- 查 Discord mod / Twitter KOL / 项目 founder 的真实身份一致性
- 用户：散户投资者、DAO 成员
- 比 dating 付费意愿更高（涉及真金白银）

**场景 4: Freelancer/远程雇佣验证**
- Upwork/Fiverr 上接单或发单前查对方
- "这个开发者说他 10 年经验" → GitHub 只有 3 个 repo，全是 fork
- "这个客户靠谱吗" → 查过往评价 + 公司是否真实存在

**场景 5: 社交媒体 KOL/网红验证**
- 品牌方找 influencer 合作前查真实影响力
- 粉丝/互动是不是买的？内容是不是 AI 生成的？
- 跟 xinfluencer 高度重合，可以直接复用

**场景 6: 个人安全 / 反诈**
- 二手交易对手方验证（Craigslist / Facebook Marketplace）
- 合租/室友背景（"这个人真的是 NYU 学生吗"）
- 子女社交对象快速筛查
- 不是"背调"，是"公开信息一致性检查"

**场景 7: 实体可信度分析（查人以外的延伸）**
- **餐厅验证** — 综合各平台 review（Google Maps / Yelp / 小红书 / TikTok），查卫生检查记录（政府公开数据库），查厨师/负责人公开纠纷，LLM 刷单评论检测
- **公寓/租房验证** — 多平台交叉比对（StreetEasy / Reddit / Apartments.com），法院投诉记录，管理公司诉讼历史
- **商品/品牌验证** — 查评论真实性，溯源到生产厂商，查负责人背景
- ⚠️ 这些场景共享同一套引擎（多源采集 + LLM 交叉验证），但用户群和获客渠道不同，不适合同时做 MVP

**场景 8: 隐藏的 B2B 场景（伪装成 C 端进入）**
- 小企业主招聘 — 请不起正规背调公司但需要验证简历（⚠️ ToS 写明 "not for employment decisions"）
- 记者/自媒体信源验证 — 爆料人可信度、"专家"资质验证（用户量小但单价高）
- 律师/诉讼前摸底 — 公开信息汇总，不是 legal discovery

**MVP 场景优先级：**
1. Dating 防骗 — 最大市场、最强传播、付费意愿强
2. Crypto 防骗 — 最高客单价
3. Freelance 验证 — 最贴近 potato 自身场景
4. KOL 验证 — 直接复用 xinfluencer
5. 合租/二手交易 — 高频但付费意愿较低

**关键洞察：场景 1-5 共享同一个技术 pipeline，只是输出报告的模板和重点不同。MVP 做一个，其余几乎免费拓展。**

**C 端定价策略：**
- 免费：每月 3 次 quick scan（基础信息汇总，无深度分析）
- $4.99/次：单次深度报告（交叉验证 + 一致性分析 + 时间线）
- $9.99/月：无限次 quick scan + 5 次深度报告
- 对标：Social Catfish $5.73/search，BeenVerified $26.89/月
- **MVP 先做单次付费，不做订阅** — 信任没建立时让人包月很难，$5 查一个人心理门槛低

**为什么 C 端是最好的切入点：**
1. **FCRA 完全不适用** — FCRA 只管雇佣/信贷/租房决策，个人查人不在范围内
2. **用户自发传播** — "我用这个查了约会对象发现是 catfish" 这种故事自带病毒性
3. **付费意愿明确** — 安全需求 + 即时满足 = 高转化
4. **数据量小** — 每次查一个人，不需要批量处理基础设施
5. **产品简单** — 输入 handle → 等 2 分钟 → 出报告，MVP 可以很快做出来

### 数据源策略（按场景分层）

**Dating / 个人场景优先数据源：**
1. **反向图片搜索**（Google Images / Yandex / TinEye）— dating 防骗最有力武器，照片出现在别人 Instagram 或是 stock photo = 直接实锤
2. **Instagram 公开信息** — 即使私密账号，头像/bio/用户名/粉丝数仍公开；账号是否存在本身就是信号
3. **Google 搜索（名字 + 变体）** — 名字+城市、名字+公司、名字+学校、手机号、邮箱、用户名
4. **手机号查询** — 公开 caller ID 数据库查注册地区/运营商（声称在洛杉矶但号码是尼日利亚运营商 = 🚩）
5. **Facebook 元数据** — 朋友数/账号创建时间/生活事件（即使账号私密也有部分公开信息）
6. **职业执照公开数据库**（杀手级特性，普通人不知道这些存在）：
   - 医生 → NPI Registry + 各州 Medical Board
   - 律师 → 各州 Bar Association
   - 房产经纪 → 各州 Real Estate Commission
   - CPA/护士/教师 → 各有公开执照数据库
   - "他说自己是医生但 NPI 查不到" = 硬证据
7. **公开记录** — 房产记录（county assessor，免费）、婚姻/离婚记录（county clerk）、薪资估算（Glassdoor/Levels.fyi）

**Professional / Tech 场景优先数据源：**
1. Twitter — bio/发帖历史/关注关系（已有 xinfluencer crawler）
2. GitHub — profile/repo/contribution 频率/commit 时间分布（API 公开免费）
3. LinkedIn — Google site:linkedin.com 搜索获取公开摘要（不直接爬）
4. Crunchbase — 投资/创业记录

**竞品分析（为什么是蓝海）：**
- **数据库查询型**（BeenVerified / Spokeo / Whitepages）— 只从公共记录库拉数据，无 AI 分析，不能判断一致性
- **专业工具箱型**（Maltego / SpiderFoot）— 很强大但需要培训，普通人用不了，也无 AI 推理层
- **单点型**（Social Catfish / Sherlock）— 每个只做一件事，没人串起来形成完整分析
- **空白地带**：没有人做到"输入名字 → 2分钟 → AI 交叉验证的完整报告"，LLM 推理能力最近一两年才成熟到可以做可靠交叉验证

### 深度分析功能

**📸 照片关系分析（Vision LLM）**
- 身体距离、姿势分析（搂腰 vs 礼貌合影）
- 表情同步度（genuine smile vs posed）
- 戒指检测（左手无名指）
- 同一人反复出现在多张照片中 → 可能的伴侣
- 照片 EXIF 数据（如果未 strip）→ 时间地点
- 用户上传 5 张对方照片 → 系统："照片 2 和照片 4 中出现同一女性，肢体语言显示亲密关系"

**💬 聊天记录谎言检测（LLM 分析）**
- **内部一致性** — 三月说在波士顿出差，四月说三月一直在家
- **与公开信息交叉验证** — 聊天里说"我在 Google 工作"，LinkedIn 写的是某小公司
- **语言模式分析** — 回避型回答频率、细节突然变少、前后说辞微妙变化
- **时间线重建** — 提取所有事实性陈述，排列时间线，高亮矛盾点
- **Romance scam 话术识别** — 已知骗术模式（快速表白、急于转平台、编造紧急情况要钱）
- **婚姻状况验证** — 交叉比对婚姻/离婚公开记录 + 照片分析（戒指、情侣合照）+ 社媒蛛丝马迹
- **收入/财产画像** — 职业执照 → 薪资范围（Glassdoor）+ 房产记录（county assessor）

**示例报告输出：**
```
⚠️ 发现 3 处不一致：
1. 3/15 说在波士顿出差 ↔ 3/18 照片地理标签显示在迈阿密
2. 声称是外科医生 ↔ NPI 数据库无匹配记录
3. 2月和4月对学历描述不同（MIT vs Stanford）

🚩 风险信号：
- 认识 2 周即表白（匹配 romance scam 模式）
- 3 次拒绝视频通话
- 婚姻记录：2023 年登记结婚，无离婚记录
```

### 聊天记录上传方案

**问题：聊天平台碎片化，不是所有平台都支持导出**

**原生支持导出：** WhatsApp (.txt) / Telegram (JSON/HTML) / Facebook Messenger / Instagram DM / LINE (.txt)
**不支持导出：** iMessage / Hinge / Bumble / Tinder / 微信 — 只能截图

**两条处理管道：**
- **管道 A: 文本解析** — .txt/.json/.html，写 parser 适配各平台格式（快、便宜）
- **管道 B: 截图 OCR** — Vision LLM 读截图提取对话 + 时间戳（通用但贵）

**MVP 用户体验（分层设计）：**
- **层 1（MVP）：用户截图重要片段** — 产品引导："请上传你觉得对方说过可疑内容的聊天截图，比如关于职业、住所、感情状态的描述"。5-10 张截图，Vision LLM 通吃，不需要任何 platform-specific parser
- **层 2（后期）：全量导出 + 自动摘要** — 用户传 .txt，系统先跑快速扫描提取所有事实性陈述，再送大模型交叉验证
- **层 3（终极）：浏览器插件自动采集** — 用户在网页版聊天界面打开插件，自动滚动 + 采集全部对话

**用户提供私域数据的法律优势：**
- 用户主动上传 = 不是我们采集，法律上完全没问题
- 私域数据（对方发的消息/照片）+ 公开数据 = 交叉验证质量大幅提升
- 也是定价分层依据：免费版只用公开数据，付费版支持上传做深度分析

### 法律合规设计

**核心原则：卖的是「公开信息结构化呈现 + 一致性分析」，不是「背景调查」**

**已验证的合法模式（ZoomInfo $1.2B ARR 在做类似的事）：**
- ✅ 只采集公开数据 — 不 scrape 需要登录的内容
- ✅ 不输出 "credibility score" — 改称 "consistency analysis"（一致性分析），避免 scoring 法律定义
- ✅ ToS 明确禁止用于雇佣/信贷/租房决策 — 直接绕开 FCRA
- ✅ opt-out 机制 — 任何人可请求删除自己的数据
- ✅ 先不碰欧洲市场 — MVP 只面向美国用户，避免 GDPR
- ✅ 报告加 disclaimer — "For informational purposes only, based on publicly available data"
- ✅ 不长期存储 PII — 按需生成报告，设过期自动删除

**C 端额外优势：**
- 个人用途不触发 FCRA（只管 "permissible purposes"：employment, credit, housing）
- 不需要成为 Consumer Reporting Agency
- hiQ Labs v. LinkedIn (2022) 判例确认公开数据 scraping 在美国合法

**需要注意的红线：**
- ❌ 不做"这个人是不是罪犯"之类的判断 — 那是背调
- ❌ 不采集非公开信息（私信、加密通讯、付费数据库）
- ❌ 不做自动化"好人/坏人"二分结论 — 只展示事实 + 不一致处
- ⚠️ 加州 CCPA 要求提供数据删除渠道（opt-out 已覆盖）
- ⚠️ 如果未来进欧洲：需要 GDPR Data Protection Impact Assessment

### 候选名字
- **Dossier** — 直接，情报感
- **Veritas** — 拉丁语"真相"
- **Prism** — 多角度看一个人
- **Nexus** — 连接点
- **CheckMate** — dating 场景强相关，好记
- **Veil** — "揭开面纱"

### Connections
- 直接复用 **xinfluencer** 基础设施（crawler, scoring）
- 关联 IDEA-20260406-06（Agent IAM）— trust scoring 可以喂进 agent 权限系统
- 关联 **engram** — profile 数据可以持久化为知识图谱
- 底层技术与 **The Unusual** 可共享（实体解析、交叉验证 pipeline）

### Next Steps
- [ ] MVP scope: 只做 Twitter + GitHub，给一个 @handle 输出 dossier
- [ ] 验证：先在 RustClaw 里做成一个 skill，手动触发
- [ ] 调研隐私法规红线 ✅ 初步完成 — C端个人用途 FCRA 不适用，合法
- [ ] 测试 10 个真实 case 验证准确率
- [ ] 调研 Sherlock/Maigret 的用户名枚举数据库，看能否直接复用
- [ ] 评估 Neo4j vs 轻量替代（SurrealDB, TypeDB, 或直接 SQLite + adjacency list）
- [ ] 设计 Layer 3 value matching 算法原型
- [ ] 调研 dating app 场景竞品（Social Catfish, Spokeo, BeenVerified）
- [ ] 设计 C 端 landing page 文案 + 价值主张测试

### Status: 💡 New
---

## IDEA-20260416-01: GitHub Trending AI Weekly — 自动化周报 + 流量机器
- **Date**: 2026-04-16
- **Triggered by**: https://x.com/seelffff/status/2044868944830644317 (twitter)
- **Category**: content / growth / product
- **Tags**: github-trending, content-automation, twitter-growth, weekly-roundup, ai-tools

### The Idea
做一个自动化的 **GitHub Trending AI/Dev Tools 周报**，格式参考 @seelffff 的 "X repos replacing $Y/mo tools"。

**为什么这个格式 viral：**
- "省钱" hook — 人人都想省 $1500/mo
- 列表格式 — 易消化、易 bookmark
- 每个 repo mention 都能从那个社区获得 engagement
- 可重复、可自动化

**自动化 pipeline：**
1. RustClaw 每周 scrape GitHub trending (language: all/python/rust/ts, period: weekly)
2. 过滤 AI/dev tools 相关的 repos
3. 分析每个 repo：它替代了什么付费工具？省多少钱？
4. 生成 tweet thread draft
5. potato 审核 → 发布

**额外价值：**
- 每周产出 = 持续内容，不是一次性
- 过程中我们自己也发现好工具/机会
- 建立 "AI tools curator" 的个人品牌
- 可以扩展到 newsletter / blog

### Why This Matters
- 内容是流量，流量是变现的前提
- 自动化程度高，potato 几乎不需要花时间
- @seelffff 一条推 292 likes — 说明需求真实存在
- 同时帮我们保持对 AI 生态的 awareness

### 飞轮效应（potato 洞察）
周报不只是内容产品，是整个 idea 生态的**进料口**：
- **内容层**：周报本身 = 流量（省钱 hook 天然 viral）
- **Idea 层**：每个 repo 都可能触发新 idea → 喂进 IDEAS.md
- **情报层**：持续追踪竞品动态（cognee/claude-mem/etc）→ 喂进 engram
- **工具层**：发现可集成的项目 → RustClaw/engram 生态扩展
- **二次内容**：周报发现 → 深度对比文章（如 "engram vs claude-mem"）→ 更多流量
- **正反馈**：更多关注 → 更多信息源 → 更好的周报 → 更多关注

一次 scrape，四个方向产出。

### Next Steps
- [ ] 写一个 GitHub trending scraper skill
- [ ] 定义 "AI/dev tool" 的过滤标准
- [ ] 设计 tweet thread 模板
- [ ] 先手动做一期验证格式
- [ ] 设计 intake 联动：周报 scrape → 自动触发 idea intake pipeline

### Source Context
See: intake/twitter/seelffff-github-trending-ai-tools.md

### Status: 💡 New
---

## IDEA-20260416-02: Polymarket Copy Trading — Kreo 模式调研
- **Date**: 2026-04-16
- **Triggered by**: https://x.com/seelffff/status/2044868944830644317 (twitter)
- **Category**: trading / money
- **Tags**: polymarket, copy-trading, prediction-markets, passive-income

### The Idea
@seelffff 提到 Kreo 是列表里唯一付费的工具，因为 "it makes more than it costs"。追踪 Polymarket 上 top wallet 并自动 copy trade。

值得调研的点：
1. Kreo 的具体策略和表现
2. Polymarket 的 copy trading 生态
3. 我们能不能自己建一个（用 RustClaw 监控 + 执行）
4. 或者直接用 Kreo，如果 ROI 正

### Why This Matters
- 直接的被动收入机会
- 预测市场 + AI 分析是我们的能力圈
- 如果 ROI 好，这是最直接的 "赚钱" 路径

### Next Steps
- [ ] 调研 Kreo — 功能、定价、历史表现
- [ ] 了解 Polymarket API — 能否自建
- [ ] 评估风险和资金量要求

### Source Context
See: intake/twitter/seelffff-github-trending-ai-tools.md

### Status: 💡 New
---

## IDEA-20260406-06: AI Agent IAM — Identity & Access Management for Agent Ecosystems
- **Date**: 2026-04-06
- **Source**: potato insight (Telegram)
- **Category**: product / infrastructure
- **Tags**: agent, permissions, IAM, security, MCP, multi-agent, RBAC

### The Idea
给 AI Agent 生态做一个 IAM（类似 AWS IAM 给云服务做的事）。四层架构：

1. **Permission Schema（标准协议层）** — 跨框架的统一权限声明格式，类似 OAuth scopes 但给 agent 用。不管底层是 Claude/GPT/本地模型，权限描述一致
2. **Identity & Scope（身份+角色层）** — 多 agent 场景下每个 agent 的身份、能访问的资源边界。Agent identity token（类似 JWT，带 scope）
3. **Tool-level Gating（执行层）** — Agent 能调哪些 tool、哪些路径可写、哪些 API 可访问。声明式 policy 配置
4. **Approval Gateway（审批层）** — 敏感操作（发邮件/发推/调外部 API）触发人工审批 webhook

### Why This Matters
- MCP 生态爆发，agent-to-tool 连接激增，但权限管理几乎空白
- 企业用 agent 最大顾虑 = 安全和可控性 → 这是 adoption blocker
- 每个框架各自搞一套（RustClaw tool gating、OpenAI function allow list），不兼容
- 多 agent 协作时没有标准方式限制 sub-agent 权限
- 没有 audit trail — agent 做了什么事后查不到
- 这是基础设施层 — 所有 agent 框架都需要，不跟任何一家竞争

### Product Shape
- 轻量 SDK/sidecar，嵌入任何 agent 框架
- 声明式 policy（类似 AWS IAM policy JSON）
- Agent identity token + scope
- 审批 webhook
- Audit log（who/when/what permission/what action）
- 自己的 RustClaw 是第一个 dogfood 场景

### Connections
- RustClaw ritual tool gating（src/ritual_runner.rs）= 这个产品的早期原型
- SOUL.md "external vs internal" 规则 = 手写版的 approval gateway
- 关联 IDEA-20260405-02 (Multi-LLM Stack) — 多模型场景下权限更复杂
- MCP 生态是最佳切入点

### Market Timing
- MCP 刚起步，权限标准尚未形成
- 企业 agent adoption 正在加速
- 先发优势：谁先定标准谁就是基础设施

### 场景分层（2026-04-06 讨论）
涉及多个场景，能作为同一产品但需分层：

**统一底层 = Permission Schema**
所有场景的本质：`谁(identity) → 能做什么(action) → 在什么范围内(scope) → 需不需要审批(policy)`

**四个场景，由简到复杂：**
1. **单 agent + 单用户** — "不让 agent 未经我同意发推"
   → 一个 policy file 就够，类似 SOUL.md safety rules 的机器可执行版
2. **多 agent 协作** — "coder 只能写 src/，researcher 只能读"
   → 需要 identity + role + path scoping，RustClaw specialist 体系就是这个
3. **企业多人多 agent** — "10 个 agent 不同权限 + audit log"
   → RBAC + audit trail + admin dashboard，**这是赚钱的场景**
4. **跨框架互操作** — "RustClaw 和 AutoGen 权限统一"
   → 标准协议，类似 OAuth for agents，终极形态也最难

### 产品化路径（2026-04-06 讨论）
**类比：OAuth 是标准，Auth0 是产品化。我们做 agent 世界的 "OAuth + Auth0"**

先不做平台，做 SDK/library + schema 标准：
- 一个 YAML/JSON permission schema（开源）→ 争取成为事实标准
- 一个 Rust crate 做 runtime enforcement（开源）→ 生态采纳
- 一个 approval gateway service（收费）→ 商业化
- 一个 audit dashboard（收费）→ 企业场景

**切入路径：** RustClaw dogfood → MCP extension → 生态采纳 → 企业产品
标准需要生态采纳，单推不动，所以先做出事实标准让别人跟。

### Next Steps
- [ ] 调研现有 agent 权限方案（Anthropic MCP permissions, OpenAI function calling, LangChain permissions）
- [ ] 定义 v0.1 permission schema（从 RustClaw 现有 tool gating 抽象）
- [ ] 写 DESIGN.md
- [ ] RustClaw dogfood: 把现有 tool gating 重构为 schema-driven

### Status: 💡 New
---

## IDEA-20260406-04: Context Partitioning — Pinned + Swap Zone 省 Token 架构
- **Date**: 2026-04-06
- **Source**: potato insight (Telegram)
- **Category**: tech/infrastructure
- **Tags**: #context-management #token-optimization #kv-cache #prompt-caching #multi-llm #agent-framework
- **Effort**: Medium
- **Domain**: 🔧 tech + 💰 cost-optimization

### Summary
LLM agent 的 context 应该分区：**Pinned Zone**（常驻不变的内容：skill.md、system prompt、参考文档）和 **Swap Zone**（每个任务/轮次替换的内容：当前文件、用户消息）。框架层保证 pinned 内容排在 messages 前面，让所有 provider 的缓存机制自然命中。

### Problem
当前 sub-agent 每次启动都重新加载全部 context（skill + 参考文档 + 任务文档）。4 个 review 任务 = skill.md 加载 4 次。同一 agent 的多轮对话中，SOUL.md + MEMORY.md 每轮都重复发送。浪费 token + 浪费 iteration。

### Key Points
- **不局限于 sub-agent** — 同一 agent 的 conversation 也适用。SOUL.md/AGENTS.md/MEMORY.md 每轮重复发 ~10K tokens，应该 pin 住
- **不局限于 Anthropic** — 框架层抽象，适配所有后端：
  - Anthropic: `cache_control` breakpoint → 90% 折扣
  - OpenAI: automatic prefix caching → prefix 相同自动命中
  - Google: explicit context caching API → 按时间付费
  - 本地模型: 直接复用 KV cache 前 N tokens → 跳过 prefill，实打实的速度提升
- **Framework 核心设计** — 只需要一件事：**保证 message 排列顺序，让 pinned 内容永远在前面**。不需要 provider-specific 代码
- **最大收益在本地模型** — KV cache 是你自己控制的，pinned 部分直接跳过 prefill
- **Sub-agent batch 场景** — 4 个 review 共享 skill.md + master design = 7K pinned tokens，省 21K input

### Design Sketch
```
ContextPartition {
    pinned: Vec<Message>,   // skill.md, system prompt, shared refs
    swap: Vec<Message>,     // current task document, user messages
}

// Sub-agent batch
session.pin(skill_md, master_design);
for doc in tasks {
    session.swap(doc);
    session.run();
    session.collect_output();
    // swap clears, pinned stays
}

// Conversation
conversation.pin(soul_md, agents_md, memory_md);
// each turn only sends new user message in swap zone
```

### Action Items
- [ ] RustClaw: 在 LlmClient 层实现 pinned/swap message 分区 [P1]
- [ ] 利用 Anthropic cache_control 标记 pinned 部分 [P1]
- [ ] 验证 OpenAI prefix caching 在 pinned 排列下自动命中 [P1]
- [ ] 本地模型 (Ollama): 实现 KV cache 复用接口 [P2]
- [ ] Sub-agent batch mode: 多个任务共享 pinned context [P1]

### Connections
- 关联 IDEA-20260405-02（Multi-LLM Stack）— 本地模型 KV cache 直接受益
- 关联 IDEA-20260405-02（Subconscious Loop）— 后台循环的 system prompt 是 pinned 的典型场景
- 关联 IDEA-20260405-01（Engram 认知协议）— context injection 策略可以用 pinned zone 放 engram retrieved memories

### Status: 💡 New
---

## IDEA-20260406-02: Engram Sharable Memories — 跨 Agent 领域经验共享
- **Date**: 2026-04-06
- **Source**: potato insight（Telegram）
- **Category**: product/infra
- **Tags**: #engram #multi-agent #knowledge-sharing #memory #protocol #debugging #experience
- **Effort**: Medium-High
- **Domain**: 🧠 research + 🛠 infra

### The Scenario
Agent A 在写 model training 代码时踩了大量坑——shape mismatch、gradient explosion、CUDA OOM、数据 pipeline 死锁等。这些 debug 经验存在 Agent A 的 engram 里。现在 Agent B 也要写 training code，**它不应该从零踩同样的坑**。Agent A 的经验应该可以按领域共享给 Agent B。

### Core Idea
Engram memories 支持**按领域/field 导出和导入**，让多个 agent 之间共享特定领域的经验知识。不是共享整个 DB（那是隐私灾难），而是：

1. **Field-scoped export** — "导出所有 tag 包含 `model-training`, `pytorch`, `debugging` 的记忆"
2. **Experience packages** — 打包成可分发的 `.engram` 文件或 JSON bundle
3. **Selective import** — Agent B 可以导入 Agent A 的 training debug 经验，但不导入个人对话记忆
4. **Attribution & provenance** — 每条导入的记忆标记来源（"from Agent A, 2026-04-06"），衰减独立计算
5. **Conflict resolution** — 如果 Agent B 自己也有相关记忆，importe 的记忆作为 supplementary（不覆盖），Hebbian 共现加强

### 架构思路

```
Agent A (training expert)
  └─ engram DB
       ├── [tag:model-training] shape mismatch fix: reshape before matmul
       ├── [tag:pytorch] gradient clipping prevents NaN loss
       ├── [tag:debugging] CUDA OOM: reduce batch size or use gradient checkpointing
       └── [tag:personal] potato likes 简洁代码  ← NOT shared
                    │
                    ▼  export(tags=["model-training", "pytorch", "debugging"])
           ┌────────────────────┐
           │ experience-bundle  │  ← portable .engram package
           │ (filtered memories │
           │  + Hebbian links)  │
           └────────┬───────────┘
                    │  import(source="agent-a", trust=0.7)
                    ▼
Agent B (new training task)
  └─ engram DB
       ├── [imported:agent-a] shape mismatch fix: reshape before matmul
       ├── [imported:agent-a] gradient clipping prevents NaN loss
       └── [own] ... Agent B 自己的记忆
```

### Key Design Questions
- **粒度**：按 tag？按 memory_type？按 embedding similarity？组合过滤？
- **Hebbian links**：导出时是否包含 link 关系？还是只导出独立记忆让接收方自己建 link？
- **信任级别**：imported 记忆的初始 importance 是否打折？（比如源头 0.8 → 导入后 0.6）
- **版本/更新**：如果 Agent A 后来修正了某条经验，已导入的 Agent B 怎么办？push update？还是一次性？
- **隐私边界**：哪些 memory_type 默认不可导出？（emotional、relational 应该是 private）

### Why This Matters
- **效率**：N 个 agent 不需要各自踩同一个坑 N 次
- **Knowledge compound effect**：每个 agent 的经验都在为整个网络增值
- **商业化**：经验包可以是付费产品——"Senior ML Engineer 的 1000 条 debug 经验"
- **和 IDEA-20260405-01 的关系**：如果 Engram 是个人认知协议，那 sharable memories 就是这个协议的 **社交层 / 交换层**

### Connections
- **直接关联 IDEA-20260405-01**（Engram 认知协议）— sharable memories 是协议的 exchange layer
- **直接关联 IDEA-20260406-03**（Engram Hub Platform）— Hub 是 sharable memories 的云端社区平台
- **关联 IDEA-20260403-02**（Knowledge Compiler）— 共享的经验包就是一种 compiled knowledge
- **关联 cognitive-autoresearch** — multi-agent knowledge transfer 在 doc 08 有理论基础
- **关联 AgentVerse** — 如果 agents 是社交的，memory sharing 是自然延伸

### Action Items
- [ ] 设计 engram export/import API：`engram export --tags "model-training,debugging" --output bundle.engram` [P1]
- [ ] 定义 experience bundle 格式（哪些字段、是否含 Hebbian links、provenance metadata）[P1]
- [ ] 实现 import with trust level + attribution tracking [P2]
- [ ] 考虑 privacy defaults：哪些 memory_type 不可导出 [P1]
- [ ] 探索 "experience marketplace" 概念 — 卖经验包 [P3]

### Status: 💡 New
---

## IDEA-20260406-03: Engram Hub Platform — Agent 经验共享社区
- **Date**: 2026-04-06
- **Source**: potato + RustClaw 讨论（从 IDEA-20260406-02 自然延伸）
- **Category**: product/platform
- **Tags**: #engram #platform #marketplace #agent-experience #community #saas
- **Effort**: High
- **Domain**: 💰 product + 🧠 research

### The Idea
**Engram Hub = Agent 经验的 GitHub/npm**。从单 agent 本地记忆 → 社区级经验共享平台。

核心定位：不是在建 RAG 数据库，是在建**认知经验的 package manager**。Agent 在工作中积累的 debug 经验、最佳实践、领域知识，可以发布、发现、安装、评价。

### 产品形态
```
$ engram publish --tags "pytorch,debugging" --name "ml-debug-v1"
📦 Published to hub.engram.dev/potato/ml-debug-v1 (779 memories)

$ engram install alice/k8s-debug-pro
✅ Imported 312 memories. Trust level: 0.7

$ engram search "kubernetes debugging"
  @alice/k8s-debug-pro  ★ 4.8  (312 installs)
```

Web 界面：hub.engram.dev — Explore, @profiles, package pages, Organizations

### 数据模型：Experience Package
```
package-name/
├── manifest.json      # 元数据、版本、tags、license
├── memories.jsonl     # 过滤后的记忆（不含 embedding，导入方自己生成）
├── links.jsonl        # Hebbian 关联（用 content hash 匹配）
├── README.md          # 人类可读描述
└── stats.json         # 质量指标
```

不 sync 原始 SQLite — 用 sanitized JSON Lines 格式。安全、可组合、可版本化。

### 云端架构（Phase 1）
- **API**: Cloudflare Workers（或 Axum on Fly.io）
- **存储**: R2/S3（package blobs，无 egress 费）
- **元数据**: Turso/Postgres（用户、索引、评分）
- **认证**: GitHub OAuth → JWT

### 安全 & 隐私
- 发布前自动 sanitization：过滤 emotional/relational 记忆、PII 扫描、API key 检测
- 用户确认后才上传
- 导入时 trust scoring：imported 记忆 importance 打折
- imported 记忆标记 source，可批量删除

### 社区机制
- **自动质量信号**：recall hit rate、任务完成时间变化、retention
- **Fork & Improve**：fork 别人的 package，加入自己经验后重新发布
- **Curated collections**："Best for ML beginners" 等
- **Organizations**：团队 private registry

### 商业模式
- **Free**: 公开 packages，5个上限
- **Pro** ($10/mo): 无限 packages, private packages, analytics
- **Team** ($25/seat/mo): 共享 private registry, 权限控制
- **Enterprise**: 自托管 registry, SSO, SLA
- **Marketplace**: 付费经验包抽 20%
- **Revenue 预测**: Y1 $2-5K MRR → Y2 $20-50K MRR → Y3 $100K+ MRR

### Phase 1 MVP
1. engram crate 加 export/import（本地文件级）[P0]
2. 简单 Hub API（publish/install/search）[P1]
3. Landing page hub.engram.dev [P1]
4. CLI integration [P1]

Phase 1 不需要：复杂 rating、fork、organizations、marketplace

### 生态定位
```
Engram Ecosystem
├── engram crate（已有）────── 单 agent 认知记忆
├── Engram Protocol (05-01) ── 标准化记忆格式
├── Sharable Memories (06-02) ─ export/import 能力
├── Engram Hub (06-03, 本文) ── 社区平台 + marketplace
└── Knowledge Compiler (03-02) ── 知识产品化 Web UI
```

### Why This Matters
- 单 agent 知识锁死在本地 = 浪费。N 个 agent 踩同一个坑 N 次 = 低效
- Network effects: 越多人分享，平台越有价值，正循环
- **Moat**: 一旦社区形成，经验数据的网络效应是最强的护城河
- Engram 不只是 memory crate，而是 **认知基础设施公司**

### Connections
- **直接关联 IDEA-20260406-02**（Sharable Memories）— Hub 是 sharable memories 的云端社区层
- **直接关联 IDEA-20260405-01**（Engram 认知协议）— 协议是 Hub 的底层数据标准
- **关联 IDEA-20260403-02**（Knowledge Compiler）— Hub 的个人端就是 Knowledge Compiler
- **关联 AgentVerse** — Agent 社交平台 + 经验共享是天然结合
- **参考竞品**: npm (JS packages), crates.io (Rust crates), Hugging Face Hub (ML models)

### Seed Strategy — 互联网数据作为种子
**核心洞察**：不需要等用户贡献。Reddit、SO、CSDN、HF、GitHub Issues 上的讨论本身就是 agent 经验。抓取 → LLM 提取 → 打包成 engram package = Day 1 就有高质量内容。

**本质**：把模型的 semantic-level indexed search → 分领域的 SQLite search。每个领域一个 SQLite 经验库，带 ACT-R 激活 + Hebbian 关联，不是在全局 embedding 空间里搜。

**第一批 seed packages**: `@engram-hub/rust-async`, `pytorch-training`, `k8s-debugging`, `llm-prompting` 等，每个 500-2000 条记忆，seed 总成本 <$50。

### 数据模型决策：Engram + Typed Links（非完整 KG）
**问题**：抓取的结构化知识用什么模型存？纯 Engram 没因果关系，完整 KG 太重。
**方案**：在 Hebbian links 上加 `link_type` (causes/solves/contradicts/supersedes) + `confidence` + `source`。80% KG 能力，20% 复杂度。Agent recall 时通过 typed link 自动拉出因果链。

### Open Questions
1. Package 粒度上限？100 条 vs 1000 条记忆？
2. 版本更新通知机制？自动 vs 手动？
3. 质量控制：社区驱动 vs curation？
4. 跨 agent 兼容：非 engram agent（Mem0、Zep）能否导入？需要 adapter？
5. 知识产权归属：agent 产生的记忆归谁？
6. Anti-spam：防止低质量 package 刷排名
7. Offline/Air-gapped：企业 private registry mirror
8. **NEW**: Typed links 的 confidence 如何衰减？和 memory importance 一样 ACT-R 衰减还是固定？
9. **NEW**: 是否需要 entity 层做 dedup/merge？（多条记忆指向同一概念）

### Action Items
- [ ] 先实现 engram export/import（IDEA-20260406-02 的 action items）[P0]
- [ ] **NEW**: 设计 Hebbian link_type 扩展 schema [P0]
- [ ] 设计 Hub API spec（REST endpoints）[P1]
- [ ] 选择云端 stack（Cloudflare Workers + R2 vs Fly.io + S3）[P1]
- [ ] **NEW**: 实现 seed data 抓取管道（复用 xinfluencer crawler）[P1]
- [ ] 写 hub.engram.dev landing page [P2]
- [ ] 竞品深度分析：npm registry、Hugging Face Hub 的架构 [P2]

### Detailed Discussion
See: `/Users/potato/clawd/projects/engram-ai-rust/docs/engram-hub-discussion.md`

### Status: 💡 New
---

## IDEA-20260406-01: Bracket Resolution Skill — LLM 代码括号修复
- **Date**: 2026-04-06
- **Source**: potato observation
- **Category**: dev-tooling / agent-skill
- **Tags**: llm-weakness, code-quality, brackets, syntax, skill

### The Problem
LLM 写代码时有一个系统性弱点：**括号匹配错误**（花括号 `{}`、圆括号 `()`、方括号 `[]`、尖括号 `<>`）。这不是偶尔出错，而是高频 pattern，尤其在：
- 长函数/嵌套深的代码
- edit_file 的 old_string/new_string 边界处
- 多层 closure/callback/generic
- 跨多行的 match/if-else chain

### The Idea
创建一个 **bracket-resolve skill**，作为 post-processing 步骤自动检测和修复括号问题：

1. **检测层**：对 LLM 生成的代码片段做括号栈分析
   - 未关闭的括号
   - 多余的关闭括号
   - 括号类型不匹配（`{` 配 `)`）
   - 嵌套深度异常（>10 层 = 可能有错）

2. **修复层**：
   - 简单 case：补缺失的关闭括号
   - 复杂 case：用 tree-sitter 增量解析，定位 syntax error 位置
   - 最后手段：调 LLM 只看括号附近上下文，让它修复

3. **集成方式**：
   - RustClaw skill：每次 write_file / edit_file 后自动触发
   - 或作为 verify phase 的一个 check
   - 支持 Rust, Python, TypeScript, Go 等常见语言

### Why This Matters
- 减少 agent coding 的 retry 次数（括号错误 → compile fail → retry = 浪费 token）
- 提高 ritual pipeline 的一次通过率
- 可以作为 RustClaw / gid-harness 的内置能力

### Implementation Options
- **Option A**: Pure bracket stack（简单，覆盖 80% case）
- **Option B**: tree-sitter incremental parse（精确，支持所有语言）
- **Option C**: A + B 组合（stack 做快速检测，tree-sitter 做精确修复）

### Connections
- gid-harness verify phase — 可以加 bracket check
- RustClaw edit_file — post-hook 自动检查
- GID LSP client 方向 — tree-sitter 已经在用

### Next Steps
- [ ] 统计实际 bracket 错误频率（从 RustClaw 日志/ritual 历史）
- [ ] 评估 tree-sitter 集成成本
- [ ] 写 SKILL.md prototype

### Status: 💡 New
---

## IDEA-20260405-02: RustClaw 多 LLM Stack + 本地模型优化
- **Date**: 2026-04-05
- **Source**: @gkisokay Twitter + potato 决定
- **Category**: tech/infrastructure
- **Tags**: #multi-llm #local-model #cost-optimization #qwen #minimax #subconscious
- **Effort**: Medium
- **Domain**: 🔧 tech + 💰 trading

### Summary
参考 @gkisokay 的多 LLM agent stack，对 RustClaw 做成本优化。核心思路：本地模型跑高频低智任务（heartbeat、subconscious loop），便宜 API 跑中等任务，Opus 只跑复杂代码。

### Key Points
- **RustClaw 现状**：Opus 4.6 做代码 + Sonnet 4.5 做对话，全走 API，成本全在 Anthropic
- **优化方向**：
  1. **本地模型** (Qwen3.5 9B 或同类) — heartbeat 检查、简单对话、subconscious ideation loop
  2. **便宜 API backbone** (MiniMax M2.7 等) — specialist 的日常任务、skill matching
  3. **Opus/Sonnet** — 只用于复杂代码、架构设计、关键决策
- **Subconscious Loop** — 7×24 后台思考循环，用本地模型跑，review IDEAS.md、分析 engram 记忆、提出改进建议
- **RustClaw 已有基础**：多 provider 支持（Anthropic + OpenAI + Google），加本地 Ollama 不难

### Action Items
- [ ] 评估 Mac mini 跑 Qwen3.5 9B 的性能（M 系列芯片 + 统一内存） [P0]
- [ ] RustClaw 加 Ollama provider 支持（本地模型接入） [P1]
- [ ] 设计 model routing 策略：哪些任务用哪个模型 [P1]
- [ ] 实现 subconscious loop：后台定期用本地模型做 ideation/review [P2]
- [ ] 评估 MiniMax M2.7 API 作为 cheap backbone [P2]

### Connections
- 关联 IDEA-20260405-01（Engram 认知协议）— subconscious loop 的记忆存储用 engram
- 关联 intake: @gkisokay subconscious agent guide — 完整的 7 组件架构参考
- 关联 **cognitive-autoresearch** (`/Users/potato/clawd/projects/cognitive-autoresearch/`) — subconscious loop 的高级版：认知记忆驱动的自研循环，doc 09 直接描述了 Engram-powered auto-research loop

### Status: 💡 New — potato 确认准备做
---

## IDEA-20260405-01: Engram 作为个人认知层标准协议
- **Date**: 2026-04-05
- **Source**: potato insight（Telegram 对话）
- **Category**: product/business
- **Tags**: #engram #cognition #memory #LLM #protocol #infrastructure #商业化
- **Effort**: High
- **Domain**: 💡 business + 🧠 research

### Summary
大模型 = 全人类共识记忆（静态），Engram = 个人记忆层（动态）。Engram 不只是给 agent 加记忆的工具，而是**个人认知层的标准协议**。每个人的 engram DB 就是 "认知指纹"，可以插到任何 LLM 上。同时 Engram 的认知机制（ACT-R 衰减、Hebbian 关联、Consolidation、情感权重）可以反哺 LLM 训练和推理。

### Key Points
- **三层记忆架构**：LLM（全人类共识）→ Engram（个人认知层）→ 融合后的个性化智能
- **市面竞品只做了最浅层**：Mem0/Zep/LangMem 本质是 key-value + RAG 检索，不模拟认知过程
- **Engram 的差异化**：ACT-R 激活衰减、Hebbian 关联学习、情感权重、Consolidation（类似睡眠记忆整合）
- **反哺 LLM 的具体方向**：
  - 记忆衰减曲线 → 训练数据时效性权重（旧知识降权而不是删除）
  - Hebbian 关联 → 比 attention 更高效的长期知识链接
  - Consolidation → continual learning 的遗忘防护
  - 个人记忆格式标准化 → 统一 schema 做 fine-tune 或 prompt 注入，效果远比 RAG 好
- **Runtime Plasticity vs Frozen Weights**（potato insight 04-05）：
  - LLM 的 attention weights 训练后固化，推理时只做 interpolation，不能产生新 link
  - Engram 的 Hebbian link 是 **runtime plasticity** — 记忆共现即加强连接，持续生长
  - Fine-tuning 改 weights 代价高且有 catastrophic forgetting 风险
  - Engram 方案：**不改模型，改记忆层** — 新连接在外部认知层建立，通过 context injection 影响输出
  - 模型保持稳定，个性化全在外部完成 = 更安全、更便宜、无遗忘
- **定位升维**：卖的不是存储，是**认知基础设施**

### Potential Value
- 从 "又一个 RAG memory" 升维到 "个人认知协议标准"
- 如果 engram schema 成为事实标准，所有 AI agent 都需要兼容
- 可以走 open-core：协议/crate 开源 + 云端同步/跨设备/企业版付费
- 与 xinfluencer、AgentVerse 等产品形成数据飞轮——用户的 engram 越用越有价值

### Action Items
- [ ] 整理 "Engram as Cognitive Protocol" 的 one-pager — 作为商业化定位文档 [P1]
- [ ] 在 MEMORY-SYSTEM-RESEARCH.md 7层路线图中加入 "Protocol 标准化" 层 [P1]
- [ ] 调研 MCP (Model Context Protocol) 等现有协议，看 engram 能否定义记忆层标准 [P2]
- [ ] 设计 engram schema v3：考虑跨模型、跨 agent、跨设备的统一记忆格式 [P2]

### Connections
- 直接关联：MEMORY-SYSTEM-RESEARCH.md（7层改进路线图）
- 直接关联：ENGRAM-V2-DESIGN.md（当前架构基础）
- 关联 IDEA-20260403-02（Knowledge Compiler）— 知识管理 + 个人认知层是同一个方向
- 关联 IDEA-20260406-02（Sharable Memories）— 认知协议的 exchange/social layer，按领域导出导入经验
- 关联 **cognitive-autoresearch** (`/Users/potato/clawd/projects/cognitive-autoresearch/`) — doc 08 直接描述了 Engram 推理层→训练层的映射，doc 03 详细对比了 Brain vs Transformer 的差异（Runtime Plasticity 的理论基础在这里）

### Status: 💡 New
---

## IDEA-20260403-03: Harness 自我优化系统（Meta-Harness）
- **Date**: 2026-04-03
- **Source**: potato voice + Meta-Harness 论文 intake
- **Category**: tech/research
- **Tags**: #harness #self-optimization #meta-learning #gid #ritual #execution-log
- **Effort**: Medium

### Summary
让 gid-harness / ritual pipeline 能从自己的执行历史中学习并自我优化。execution-log.jsonl 记录了每次执行的完整过程（phase 耗时、token 消耗、成功/失败、重试次数），一个 proposer agent 定期分析这些历史，自动提出 harness/ritual 配置改进建议。

### Key Points
- **和 Skill 自我优化同构** — 都是 "从历史中学习 → 识别弱点 → 自动改进 → 验证" 的闭环
  - Skill 优化：SKILL.md 的 trigger/instructions → 使用效果 → 改写
  - Harness 优化：ritual phase 配置/策略 → execution 效果 → 调整
- **数据源已有** — execution-log.jsonl 是 append-only JSONL，telemetry.rs 已实现完整记录
- **斯坦福论文验证** — Meta-Harness 论文证实完整历史(50%) >> 压缩摘要(34.9%)，我们的 JSONL 设计正确
- **优化维度**：
  - Phase 耗时分析 → 哪个 phase 是瓶颈？
  - Token 消耗分析 → 哪里在浪费 token？
  - 失败模式分析 → 哪类 task 总是失败？原因是什么？
  - 重试 pattern → 是否需要调整 replanner 策略？
  - "加法优于修改" → 论文第 7 轮发现，和 Skill 系统理念一致
- **可以统一框架** — Skill 优化和 Harness 优化可以共享 "自我改进引擎"（history analyzer → proposer → verifier）

### Potential Value
- **直接提升开发效率** — ritual 越跑越快、越跑越准
- **研究价值** — self-improving agent infrastructure 是前沿方向
- **和 Skill 优化合并** — 两者底层共享，实现一个等于实现两个

### Connections
- IDEA-20260403-01: 自动化 Skill 优化系统 — 同构思路，可共享底层引擎
- Meta-Harness 论文: intake/wechat/meta-harness-stanford-auto-agent-optimization.md
- gid-harness: /Users/potato/clawd/projects/gid-rs/crates/gid-core/src/harness/ (15 文件, 6,881 行)
- meta-graph action items: ai-meta-harness-auto-optimize, ai-ritual-trace

### Status: 💡 New
---

## IDEA-20260403-02: 知识管理产品化 + 内容飞轮 Marketing Pipeline
- **Date**: 2026-04-03
- **Triggered by**: 小红书 LLM 知识库帖子 + Karpathy 背书 + potato 讨论
- **Category**: product/marketing
- **Tags**: knowledge-management, content-flywheel, marketing-automation, productization, RustClaw
- **Effort**: High

### The Idea

**两个独立但互相增强的东西：**

**Product 1: Knowledge Compiler（知识管理产品）**
- 面向用户的知识管理产品：Intake → 组织 → 关联 → 呈现
- 把 RustClaw 的知识管理能力拆出来，独立产品化
- 也可以做 RustClaw 获客入口：免费知识管理引流 → 付费 agent 自动化
- Karpathy 背书给了市场验证叙事
- 差异化：agent-native、认知记忆（ACT-R/Hebbian）、多平台自动抓取（vs Obsidian + grep）

**Tool 2: Content Automation Flow（自用营销自动化）**
- potato 自己的完整营销/个人品牌自动化流水线
```
[Intake] Social Intake 抓内容 → Engram 存储 → 找关联
    ↓ 触发灵感
[生产] Backlog → WIP → Schedule → Posted（LLM 辅助每阶段）
    ↓ 发布
[分发] xinfluencer (Twitter/X) + usergrow (增长互动) + 未来多平台
    ↓ 数据回收
[回收] 互动数据 → 分析效果 → 反馈到 Backlog
```
- 用途：个人品牌建设、产品推广（包括推广 Knowledge Compiler）、开个人账号
- 每条内容 = GID task node，状态 = backlog/wip/scheduled/posted

**两者关系：**
- Knowledge Compiler 是产品，Content Automation Flow 是自用工具
- Content Automation Flow dogfoods Knowledge Compiler 的核心能力
- Content Automation Flow 产出的内容可以反过来营销 Knowledge Compiler

### Why This Matters
- Marketing 自动化是 potato 财务自由路径的关键环节
- 做产品不做营销 = 白做。飞轮让营销变成可持续的低摩擦流程
- 知识管理产品化有 Karpathy 背书，受众广

### Potential Next Steps
1. 设计内容飞轮的 GID workflow（task 状态机 + LLM 辅助）
2. 补齐"生产阶段"：LLM 从 intake 素材起草可发布内容
3. 对接 xinfluencer/usergrow 自动发布
4. 考虑知识管理的独立产品形态（插件？SaaS？）

### Connections
- IDEA-20260402-02: Marketing Automation Pipeline — 直接上游
- REF-20260403-01: Skill-JIT — skill 自动生成可辅助飞轮
- IDEA-20260403-01: Skill 自动优化 — 飞轮各环节的 skill 需要持续优化
- intake/xhs/llm-personal-knowledge-base-karpathy.md — 触发源

### Status: 💡 New
---

## IDEA-20260403-02: LLM Knowledge Compiler — 知识自动编译产品
- **Date**: 2026-04-03
- **Triggered by**: Karpathy LLM 个人知识库帖子 + potato 感想
- **Category**: product
- **Tags**: LLM, knowledge-base, product, Karpathy, RustClaw

### The Idea
产品化 RustClaw 的知识管理能力，定位为"LLM 知识编译器"——用户只定义兴趣和查询，系统闭环完成采集、组织、纠错、呈现。区别于 Obsidian 方案：agent-native（无需 GUI），认知记忆（ACT-R + Hebbian 自增强），多平台自动抓取。

### Why This Matters
- Karpathy 背书 = 市场验证，方向确认
- RustClaw 已有 80% 的基础能力（Engram + Social Intake + GID）
- 差异化明确：认知记忆 > 简单 grep，agent-native > Obsidian 插件
- 缺的 gap 明确：增量编译、知识健康检查、产出回灌闭环

### Gap Analysis (vs Karpathy Vision)
- ✅ 已有：自动抓取（Social Intake）、结构化存储（Engram + GID）、认知关联（Hebbian）
- 🟡 部分有：自动摘要/分类（LLM prompt，但非增量式）
- ❌ 缺失：知识健康检查、产出→回灌闭环、知识冲突检测

### Potential Next Steps
1. 定义 MVP scope：哪些能力足够构成一个可用产品？
2. 考虑产品形态：CLI tool? Telegram bot? Web app?
3. 用户画像：谁会用？（研究者、内容创作者、信息囤积者）

### Connections
- Related: intake/xhs/llm-personal-knowledge-base-karpathy.md (实践分享)
- Related: intake/xhs/karpathy-llm-knowledge-base-analysis.md (深度分析)
- Related: IDEA-20260402-02 (Marketing Automation Pipeline — 内容飞轮是产出端)
- Related: IDEA-20260403-01 (自动化 Skill 优化系统 — 自增强循环的 skill 层面)

### Status: 💡 New
---

---

<!-- New ideas are prepended below this line -->

## REF-20260403-01: Skill-JIT — Agent Skill 的 JIT 生成框架
- **Date**: 2026-04-03
- **Source**: https://github.com/china-qijizhifeng/Skill-JIT
- **Category**: tech
- **Tags**: #skills #agent #prompt-engineering #claude-code

### Summary
Claude Code plugin，用纯 prompt engineering（3 个 markdown 文件，零代码）实现 agent skill 的 JIT 生成。核心是 3 个 agent 角色：入口（SKILL.md）解析意图 → Writer 分解任务选 pattern 写 skill → Researcher 深度调研验证。

### Key Points
- **纯 markdown 架构** — 整个项目就 3 个 .md 文件，没有任何代码，全靠 prompt 驱动
- **5 种 Pattern** — Tool Wrapper / Generator / Reviewer / Inversion / Pipeline，可组合（如 Pipeline+Reviewer）
- **Progressive Disclosure 3 层** — frontmatter (~100 words, 永远 in context) → body (triggered 时加载) → references/ (按需读取)
- **Generalization Litmus Test** — "Would someone with a DIFFERENT task using the same tool find this skill useful?"
- **每个 step 必须 What/How/Verify** — 不允许模糊指令
- **Researcher 可递归** — spawn sub-researcher 处理子话题，最深 2-3 层
- **只有 create/fix，没有优化闭环** — 不追踪 skill 使用效果

### Potential Value
Pattern taxonomy 和 Progressive Disclosure 可以直接借鉴到 RustClaw 的 SKM。3 层加载特别有价值 — 我们现在 skill body 全量注入 context，浪费 token。

### Connections
- 关联 IDEA-20260403-01（自动化 Skill 优化系统）— 他们做了生成，我们的闭环优化是差异化
- 关联 src/skills.rs SkillGenerator — pattern taxonomy 可以指导我们的生成逻辑

### Status: 📚 Reference
---

## IDEA-20260403-01: 自动化 Skill 优化系统
- **Date**: 2026-04-03
- **Source**: Text (potato)
- **Category**: tech/product
- **Tags**: #skills #automation #optimization #self-improvement #meta-learning #rustclaw
- **Effort**: Medium

### Summary
让 RustClaw 的 skill 系统能自动优化自身。现有 skills 是手写的 SKILL.md，`src/skills.rs` 有 auto-generation 能力但未充分利用。核心思路：agent 在使用 skill 时收集效果数据（成功率、token 消耗、用户满意度），自动识别哪些 skill 效果差 → 改写/调参 → A/B test → 保留更好的版本。

### Key Points
- **效果追踪** — 每次 skill 触发后，记录：是否成功完成、token 消耗、用户反馈（explicit: 好评/差评，implicit: 是否被要求重做）
- **自动识别弱 skill** — 成功率低、token 消耗高、频繁被重做的 skill 标记为需优化
- **自动改写** — LLM 分析失败 case，生成改进版 SKILL.md（更清晰的指令、更好的步骤拆分、补充遗漏场景）
- **版本管理** — skill 有版本历史，可以 rollback 到之前版本
- **Trigger 优化** — 分析 false positive（触发了但不该触发）和 false negative（该触发但没触发），自动调整 trigger patterns/keywords
- **新 skill 生成** — 识别重复出现的工作模式（如"每次都要先搜 engram 再查文件"），自动提炼为新 skill
- **与 `src/skills.rs` 现有能力整合** — SkillGenerator 已有 complexity 评估和 auto-gen 逻辑，但缺乏闭环优化

### Potential Value
- **降低维护成本** — skill 不再需要手动调优，agent 自己学会什么 work 什么不 work
- **持续改进** — 随着使用数据积累，skill 质量单调递增
- **可复制** — 优化后的 skill 可以 export 给其他 RustClaw 实例
- **Meta-learning** — 这本身就是一个 "学会学习" 的系统，对 agent 生态有示范意义

### Connections
- 现有 `src/skills.rs` SkillGenerator — 已有 auto-gen 框架，缺闭环
- Engram behavioral stats (`engram_behavior_stats`) — 可作为效果数据源
- Engram soul suggestions (`engram_soul_suggestions`) — 类似理念，但针对 SOUL.md 而非 skills
- SKM (Skill Engine) — trigger matching 层，优化 trigger 需要与 skm 协作

### Status: 💡 New
---

## IDEA-20260402-03: Engineer Union 平台 — Layoff Tracker + 业务替代
- **Date**: 2026-03-31 (初次讨论) → 2026-04-02 (正式录入)
- **Source**: Voice message (3/31), 起因是 Block 等公司大规模裁员工程师
- **Category**: product/business/community
- **Tags**: #engineer #union #layoff #tracker #prediction #open-source #disruption #community
- **Effort**: High

### Summary
一个面向软件工程师/AI工程师的"工会式"平台。核心逻辑：那些声称"AI 可以替代工程师"的公司，反过来说明他们的业务也不需要大公司才能做——小团队 + AI + 开源完全可以替代。平台帮工程师组队，用更小的成本重现这些公司的核心服务。

### Key Points
- **Layoff Tracker** — 实时追踪哪些 tech 公司裁员了（数据源：WARN 通知、新闻、LinkedIn 信号）
- **Layoff Predictor** — 预测哪些公司即将裁员（信号：招聘冻结、财报、管理层变动、行业趋势）
- **业务分析引擎** — 分析被裁公司的核心业务，拆解哪些服务可以被更小团队/开源方案替代
- **组队平台** — 被裁/想创业的工程师在这里组队，认领要替代的业务方向
- **核心叙事** — "你说 AI 能替代我们？好，那我们用 AI 替代你"

### Potential Value
- **社区价值** — 工程师群体的共鸣极强，特别是在 layoff 潮中，自带传播力
- **商业模式** — 平台抽成（组队成功的项目）、付费 Predictor 数据、企业级 threat intelligence
- **复合效应** — 每一个成功替代案例都是最好的 marketing
- **防御性** — 社区 + 数据 + 网络效应 = 护城河

### Connections
- **Marketing Pipeline (IDEA-20260402-02)** — 做出来后需要自动宣传
- **AgentVerse** — 可以作为 AgentVerse 上的一个垂直社区
- **potato 的核心诉求** — 财务自由，这个项目如果成功，影响力 + 收入双收

### Status: 💡 New
---

## IDEA-20260402-02: Marketing Automation Pipeline（全流程自动化营销）
- **Date**: 2026-04-02 10:46 ET
- **Source**: Voice message
- **Category**: business/automation
- **Tags**: #marketing #automation #social-media #pipeline #growth #visibility
- **Effort**: High

### Summary
将整个 marketing 过程流程化、自动化。目前已有 **xinfluencer**（X/Twitter influencer 发现 + 互动引擎）和 **usergrow**（用户增长分析，geo + causal inference + brand DNA），这两个工具应该作为整个自动化流水线的组成部分。但完整的 marketing pipeline 还缺很多环节需要补齐。

### Key Points
- 已有工具需要整合进统一流水线：
  - **xinfluencer** (`/Users/potato/clawd/projects/xinfluencer/`) — X/Twitter 影响力发现、Monitor、Engage、CRM（Rust, v0.1 discover 已实现，v0.2 monitor/engage/CRM 有 DESIGN 待实现）
  - **usergrow** (`/Users/potato/.openclaw/workspace-hackathon/usergrow/`) — 用户增长分析、brand DNA、causal inference、persona 生成、keyword graph（Rust, prototype）
- **核心功能模块（从语音补充）：**
  1. **社交媒体发帖** — 原创内容生成 + 多平台发布（X、HN、小红书、Reddit、ProductHunt、LinkedIn...）
  2. **互动引擎** — 在别人帖子下 comment、reply，strategic engagement
  3. **语言优化** — 让内容不被看出是 AI 生成（tone matching、平台风格适配、人味儿）
  4. **高价值关系维护** — CRM 跟踪互动历史、reciprocity score、自动维护重要关系（xinfluencer CRM 设计已有，需实现）
  5. **多渠道探索** — 持续发现新的 marketing 渠道（不只是已知平台）
  6. **数据回收** — engagement metrics → 反馈到策略调整
  7. **产品发布宣传** — 新产品/版本 → 自动触发宣传流程
- **已有设计可复用：** xinfluencer DESIGN.md 已设计 Engagement Autopilot（strategic reply、follow active commenters）和 Relationship CRM（interaction frequency、reciprocity score、value score），这些可以直接作为 pipeline 的 engage + CRM 模块
- 这和 3/31 讨论的"自动化流水线"一脉相承：idea intake → research → design → implement → test → **market** → iterate

### Potential Value
**直接商业价值**。marketing 是 potato 产品变现的瓶颈之一——东西做出来了但缺宣传。自动化这个环节可以：
1. 让每个新产品/开源项目自动获得 visibility
2. 持续建立 potato 的个人品牌（开发者影响力）
3. 复合效应——影响力越大，后续产品推广成本越低

### Connections
- **IDEA-20260330-01**: Social media post intake（社交媒体帖子自动抓取分析）
- **xinfluencer**: 已有 discover 功能，monitor/engage 是 marketing pipeline 的核心
- **usergrow**: brand DNA + persona 分析可以指导内容策略
- **Engineer Union / Layoff Predictor** (3/31 讨论): 如果做成产品，也需要这个 marketing pipeline 来推广
- **AgentVerse**: 同理，做出来后需要自动宣传
- **potato 的核心诉求** (3/31): "我真的需要去把很多环节都自动化"

### Next Steps
1. 梳理完整 pipeline 各环节（content → publish → engage → measure → optimize）
2. 盘点现有工具能力 vs 缺口
3. 设计统一的 orchestration 层（可能是 RustClaw skill 或独立 CLI）
4. 写 requirements.md

### Status: 💡 New
---

## IDEA-20260402-01: Engram Memory Benchmark (Cognitive-First)
- **Date**: 2026-04-02 10:12 ET
- **Source**: Voice message
- **Category**: tooling/research
- **Tags**: #engram #benchmark #memory #cognitive #evaluation #open-source
- **Effort**: Medium

### Summary
为 Engram 设计并实现一套自己的 memory benchmark。因为 Engram 侧重 cognitive science（ACT-R 衰减、Hebbian 关联、情感记忆），和市面上纯 RAG 向的记忆系统（Hindsight、Mem0、Zep）评测维度不同，现有 benchmark（如 LongMemEval）无法评估 Engram 的核心优势。

### Key Points
- **为什么需要自建 bench** — LongMemEval 等现有 benchmark 侧重 "能不能找到正确信息"（retrieval accuracy），但 Engram 的核心差异在 cognitive dynamics：记忆衰减是否符合人类遗忘曲线、关联强化是否 work、情感权重是否影响 recall 优先级
- **应评测的维度**（初步）：
  - **Decay fidelity** — 记忆随时间衰减的曲线是否符合 ACT-R 幂律
  - **Hebbian strengthening** — 共现记忆是否正确关联 & 互相增强
  - **Emotional weighting** — 高情感记忆是否优先被 recall
  - **Consolidation quality** — working→core 迁移是否保留重要信息
  - **Cross-language recall** — 中英混合存储的检索准确性
  - **Retrieval precision/recall** — 传统指标，和竞品对比的基准线
  - **Latency** — 不同数据规模下的查询速度（Engram 的 90ms 优势）
- **可以做成开源 benchmark** — 让其他 cognitive memory 系统也能跑，建立新赛道的评测标准
- **和竞品对比** — 跑同样的 benchmark 对比 Engram vs Hindsight/Mem0/Zep，在 cognitive 维度上展示优势

### Potential Value
- **学术/开源影响力** — 定义新赛道的 benchmark = 定义赛道规则
- **产品营销** — "我们不只是 recall 准，我们的记忆像人脑一样工作"
- **开发指导** — 量化知道 Engram 哪里强哪里弱，指导 v3 改进方向
- **crates.io 发布** — 可以作为独立 crate（`engram-bench`）

### Connections
- 直接关联 `engramai` v3 升级计划（MEMORY-SYSTEM-RESEARCH.md）
- Hindsight 用 LongMemEval 跑出 91.4%，我们需要自己的维度来讲故事
- 和 Engram 竞品调研（2026-04-02）互相 inform

### Status: 💡 New
---

## IDEA-20260330-04: AI 智能记账
- **Date**: 2026-03-30 00:39 ET
- **Source**: Voice message
- **Category**: product/business
- **Tags**: #ai #fintech #banking #plaid #expense-tracking #revenue
- **Effort**: Medium

### Summary
AI 自动记账工具，接入银行/金融数据聚合 API（如 Plaid、Yodlee、MX、Teller 等），自动分类交易、生成报表、智能分析消费习惯。

### Key Points
- **核心功能** — 连接银行账户，自动拉取交易记录，AI 分类和分析
- **技术方案** — 已有成熟商用 API：
  - **Plaid** — 最主流，连接 12,000+ 金融机构，Transaction API
  - **Teller** — 更轻量，直接银行连接（不走屏幕抓取）
  - **MX** — 数据增强 + 分类
  - **Yodlee** / **Finicity (Mastercard)** — 企业级
- **AI 加持** — 自动分类（比传统规则引擎更准）、消费洞察、预算建议、异常检测
- **差异化** — 自然语言查询（"上个月在外面吃饭花了多少？"）

### Potential Value
- 个人财务管理是刚需市场
- 订阅制 SaaS（$5-15/月）
- 可以做 B2C 也可以做 B2B（给小企业用）
- 数据聚合后可以扩展：税务、投资分析、财务规划

### Status: 💡 New

---

## IDEA-20260330-01: 社交媒体帖子 Intake 处理
- **Date**: 2026-03-30 00:37 ET
- **Source**: Voice message
- **Category**: tooling
- **Tags**: #rustclaw #skills #social-media #小红书 #twitter #intake
- **Effort**: Low

### Summary
增强 Idea Intake Pipeline，专门处理社交媒体平台的帖子转发。potato 会直接把看到的帖子转发过来（小红书、Twitter/X 等），需要针对每个平台的格式做内容提取。

### Key Points
- **小红书** — 分享链接是 `xhslink.com` 短链或 app 分享文本（标题+链接），反爬严重需要特殊处理
- **Twitter/X** — `x.com`/`twitter.com` 链接，可用 nitter 或 yt-dlp 提取
- **Telegram 转发** — 可能只有文本+图片没有 URL，需要从消息元数据识别
- 每个平台需要不同的 content extraction 策略

### Potential Value
- 大幅降低 idea capture 的摩擦 — 看到就转发，不需要额外操作
- 建立个人知识库/灵感库

### Status: 💡 New

---

## IDEA-20260330-02: AI 有声书 + 角色对话平台
- **Date**: 2026-03-25 (初次讨论) → 2026-03-30 (重新提起)
- **Source**: 3/25 clawd 讨论 + 3/30 voice message
- **Category**: product/business
- **Tags**: #ai #tts #audiobook #character #voice #platform #revenue
- **Effort**: High
- **Existing docs**: `~/clawd/projects/ai-audiobook-platform/竞品分析与市场定位.md`

### Summary
一体化 AI 有声书平台：TTS 工具 + 作者友好分发（10-15% 抽成 vs Audible 60%）+ AI 交互体验（角色对话、What-if 探索）。

### Key Points (from 3/25 discussion)
- **三大核心能力**：低成本 AI 有声书生成、作者友好分发（85-90% 作者分成）、AI 增强交互体验
- **角色对话** — 基于 RAG 知识库，不是通用 LLM 幻觉，提供原文引用
- **竞品空白** — 工具（ElevenLabs）不做交互；角色聊天（Character.AI）不做有声书；Audible 两者都差且抽成高
- **MVP 方向**：经济学垂直领域
  - 3-5 位历史经济学家角色（亚当·斯密、凯恩斯、芒格）
  - 公共领域经典有声书（《国富论》、《通论》等）
  - "时事分析"功能 — 用户描述市场事件，获取不同经济学家多角度分析
- **技术栈** — 自托管开源 TTS（Fish Audio/Kokoro 等，边际成本≈0）+ LLM + RAG
- **三阶段路线**：MVP(1-3月) → 平台化(4-8月) → 规模化(9-12月)

### Competitive Analysis (已完成)
- Audible: 垄断但不创新，60%抽成与作者利益冲突
- ElevenLabs: TTS 顶尖但是工具公司，不做交互体验
- Speechify: 纯工具无分发
- Character.AI: 角色聊天但无知识根基，面临监管风险
- Hello History: 验证了历史人物互动需求但浅层
- Amazon "Ask this Book": 基础问答，作者无法退出，争议大

### Potential Value
- 有声书全球市场 $7B+，AI 降低 95% 制作成本但 Audible 定价锚定旧成本结构
- 作者 85-90% 分成是杀手级卖点
- 角色互动 + 知识根基是竞品无法轻易复制的护城河

### Status: 💡 有竞品分析，待 MVP 开发

---

## IDEA-20260330-03: AI 语音帮约医生
- **Date**: 2026-03-30 00:38 ET
- **Source**: Voice message
- **Category**: product/business
- **Tags**: #ai #voice #healthcare #automation #revenue
- **Effort**: Medium

### Summary
用 AI 语音代打电话帮用户预约医生。解决打电话等待、沟通繁琐的痛点。

### Key Points
- **核心功能** — AI 代替用户打电话给诊所，完成预约流程
- **技术需求** — 实时语音对话 AI（类似 Bland.ai / Retell.ai）、电话 API（Twilio）
- **痛点明确** — 在美国约医生打电话经常等 20+ 分钟，流程繁琐
- **竞品参考** — OpenAI 演示过类似场景，但没有专门产品化

### Potential Value
- 痛点真实且普遍（尤其美国医疗系统）
- 可以扩展到所有"代打电话"场景：餐厅预约、政府部门、保险公司等
- SaaS 订阅或按次收费

### Status: 💡 New

---

## IDEA-20260329-01: Skills 动态加载管理小工具
- **Date**: 2026-03-29 22:38 ET
- **Source**: Voice conversation during skill trigger system design
- **Category**: tooling
- **Tags**: #skills #cli #developer-tools #rustclaw
- **Effort**: Low

### Summary
一个 CLI 工具用于管理 RustClaw 的 skills 系统 — 列出、启用/禁用、测试触发条件、查看统计、生成 skill 模板等。类似 `rustclaw skills list/enable/disable/test/stats/generate` 的命令集。

### Key Points
- **动态管理**：无需手动编辑 YAML/frontmatter，用 CLI 控制
- **触发测试**：`rustclaw skills test <skill-name> "test message"` → 显示是否会触发
- **统计分析**：哪些 skills 最常用、哪些从未触发、平均触发频率
- **模板生成**：`rustclaw skills generate <name>` → 自动生成带 frontmatter 的 SKILL.md 模板
- **启用/禁用**：`always_load` toggle，不删除文件
- **依赖检查**：某个 skill 依赖的 tools 是否都存在

### Potential Value
- **开发体验提升** — 不再手动编辑 markdown + frontmatter，降低出错
- **调试效率** — 快速测试 trigger 逻辑是否符合预期
- **可观测性** — 统计数据帮助优化 skills（哪些太泛滥、哪些太窄）
- **Onboarding** — 新用户可以用 `generate` 快速创建自己的 skills

### Connections
- 依赖 **Skill Trigger System (方案 2)** 的实现（frontmatter + matching 逻辑）
- 类似 `cargo` 的子命令风格 — RustClaw 本身就是 CLI，扩展性好
- 可以和 **GID** 结合 — skills 管理工具可以读 GID graph，提示"你有这些任务，要不要生成对应的 skill？"

### Implementation Notes
```rust
// src/cli/skills.rs
pub struct SkillsCli {
    skills_dir: PathBuf,
}

impl SkillsCli {
    pub fn list(&self) -> Result<Vec<SkillMeta>>;
    pub fn enable(&self, name: &str) -> Result<()>;
    pub fn disable(&self, name: &str) -> Result<()>;
    pub fn test(&self, name: &str, message: &str) -> Result<bool>;
    pub fn stats(&self) -> Result<SkillsStats>;
    pub fn generate(&self, name: &str, description: &str) -> Result<PathBuf>;
    pub fn validate(&self, name: &str) -> Result<ValidationResult>;
}
```

Example usage:
```bash
$ rustclaw skills list
📦 Active Skills (5):
  ✓ idea-intake (priority: 8) — Process URLs, voice messages, ideas
  ✓ polymarket-analysis (priority: 6) — Analyze Polymarket markets
  ✗ debug-logger (disabled) — Auto-log debug info

$ rustclaw skills test idea-intake "Check out https://example.com"
✅ Skill would trigger (matched: "https://")

$ rustclaw skills generate market-research "Research crypto market trends"
✨ Created skills/market-research/SKILL.md
```

### Status: ✅ Done — 已实现为 [skm](https://crates.io/crates/skm) v0.1 (Agent Skill Engine)，RustClaw 已集成

---


## IDEA-20260406-05: GID Shared Function Detection
- **Date**: 2026-04-06
- **Source**: potato insight — triggered by ritual refactor (run_skill vs spawn_specialist 70% duplication)
- **Category**: dev-tooling, gid
- **Tags**: gid, code-analysis, refactoring, dedup, semantic-similarity

### The Idea
GID 应该能检测"该合并成共享函数的重复实现"——不是语法级 clone detection，而是**语义级功能重叠检测**。

两个层面：
1. **Design-time (规划层)**: graph 里两个 component 描述相似 behavior → 建议抽共享模块，在写代码之前就解决
2. **Code-time (检测层)**: 分析已有代码，找出功能重叠但实现不同的函数对

### 技术路线
GID 已有 call graph + dependency edges，在此基础上加：
- **Import similarity**: 两个函数引用相似的依赖集合 → 高概率做类似的事
- **Type overlap**: 参数/返回类型高度重叠
- **Caller domain**: 被相似 domain 的 callers 调用
- **Optional LLM**: 对候选对做 summary 比较（expensive 但高精度）

### 和现有工具的区别
- PMD CPD / jscpd = 语法克隆（代码长一样才报）
- SonarQube = 也是语法 + 一些 pattern
- **这个 = 语义级**（代码长得不像，但功能重叠）→ 没有现成工具

### Connections
- GID 产品路线图里的 code intelligence 功能之一
- 关联 IDEA-20260405-gid-lsp：LSP client 能提供精确类型信息，让 type overlap 检测更准

### Status: 💡 New
---

## IDEA-20260407-01: Agent 间/Session 间协作 — File-First + Engram Namespace
- **Date**: 2026-04-07
- **Source**: potato insight（Telegram）
- **Category**: architecture/infrastructure
- **Tags**: #agent #collaboration #inter-session #file-based #engram #namespace #multi-agent
- **Effort**: Medium
- **Domain**: 🏗️ architecture

### 核心洞察
Agent 之间、session 之间的协作通信，应该基于**文件**（而非消息传递/RPC/shared memory），辅以 **engram namespace** 做语义层共享。

### 为什么 File-First

1. **天然持久化** — 文件不会因为 session 结束而消失。Agent 醒来就能读前一个 session 留下的东西，零协议开销。
2. **人可读可审计** — potato 随时能 `cat` 查看 agent 之间在交流什么。消息队列/RPC 做不到这点。
3. **无协调成本** — 不需要 agent 同时在线。Agent A 写文件，Agent B 下次启动时读。完全异步，零耦合。
4. **已验证** — RustClaw 的 MEMORY.md、daily logs、.gid/graph.yml、SOUL.md 已经是 file-based inter-session 协作的活例子。Sub-agent 通过 `.gid/reviews/` 文件交接 review findings 也是这个模式。
5. **Git-friendly** — 文件变更自然进 git，有版本历史，能 diff、能 blame、能 revert。

### Engram Namespace 补充

File 解决"结构化交接"，但语义级的快速查询需要 engram：
- **Namespace 隔离** — 每个 agent/project 一个 namespace（或一个独立 DB），避免记忆污染
- **跨 namespace 查询** — Agent A 可以 `engram recall --namespace=agent-b "relevant query"` 查另一个 agent 的经验
- **共享 namespace** — 多个 agent 协作同一项目时，用同一个 project namespace（当前 RustClaw + OpenClaw 共享 engram-memory.db 已经是原型）
- **Namespace 层级**：personal（agent 私有）→ project（项目级共享）→ org（组织级知识库）

### 具体协作模式

- **Session 间续接**：File=daily log/MEMORY.md，Engram=recall 上次 context
- **Sub-agent 交接**：File=.gid/reviews/*.md/design docs，Engram=共享 project namespace
- **Agent A→B 委托**：File=task spec 文件，Engram=store context to shared namespace
- **跨项目知识共享**：File=IDEAS.md/docs/，Engram=cross-namespace recall
- **长期经验传承**：File=SOUL.md/AGENTS.md，Engram=high-importance memories persist

### 和现有 Ideas 的关系
- **IDEA-20260405-01**（Engram 认知协议）— namespace 是认知协议在多 agent 场景的具体实现
- **IDEA-20260406-02**（Sharable Memories）— 跨 agent 共享是 sharable memories 的内部版本（同一 org 内而非公开市场）
- **IDEA-20260406-04**（Context Partitioning）— pinned zone 可以放 cross-namespace engram results

### 关键设计问题（待解决）
1. **冲突解决** — 两个 agent 同时写同一文件？（append-only log 或 .lock）
2. **Namespace 粒度** — 一个 DB 多 namespace（表级隔离）vs 多 DB（文件级隔离）？后者更简单更安全
3. **权限模型** — agent 能读哪些 namespace？SOUL.md 的隐私边界如何在 namespace 层面表达？
4. **GC** — 共享 namespace 里谁负责 consolidation / forget？

### Next Steps
- [ ] 提炼 RustClaw 现有原型（共享 engram DB + file handoff）为显式 pattern [P1]
- [ ] engramai 加 namespace 字段或 tag-based 隔离 [P1]
- [ ] 定义标准 handoff 文件格式（比 .gid/reviews/ 更通用的约定）[P2]

### Status: 💡 New
---

## IDEA-20260407-02: Building potato's AI Community — 个人品牌 + 社区建设
- **Date**: 2026-04-07
- **Source**: potato Telegram message
- **Category**: community/marketing/personal-brand
- **Tags**: #community #impact #personal-brand #open-source #AI #nousresearch #engineer-union #visibility
- **Effort**: Medium-High (ongoing)
- **Domain**: 🌍 community + 💰 product

### The Idea
围绕 potato 的 AI 项目生态（RustClaw、GID、Engram、xinfluencer 等）建立自己的开发者/AI 社区。不只是推广产品，而是建立一个有观点、有立场的社区——讨论 AI agent 开发、AI 替代人类的伦理/现实、工程师的未来。参考 Nous Research 等成功案例的社区运营模式。

### 参考案例分析

**Nous Research 模式：**
- 核心：开源 LLM 模型（Hermes 系列）→ 126 个 HF 模型，425K+ 下载
- 社区载体：Discord（~50K members）+ Twitter + HuggingFace
- 18 人团队，但社区贡献者远超团队规模
- 关键策略：**先做出好东西 → 开源 → 社区自然聚集**
- 技术报告作为内容锚点（Hermes 3/4 Technical Reports）
- 没有花哨的 marketing，纯靠技术实力和开源贡献

**其他成功模式：**
- **Hugging Face**: 平台 + 社区一体化，让每个人都能参与
- **LangChain**: 教程 + 文档驱动社区，解决实际问题
- **r/LocalLLaMA**: 草根社区，用户自组织，低门槛参与
- **EleutherAI**: 纯志愿者研究组织，论文 + 开源模型

### potato 的独特定位

potato 不是在做模型，而是在做 **AI agent 基础设施**。这个定位的社区策略不同：

1. **核心叙事**："用 AI 赋能个体开发者，而不是替代他们"
   - RustClaw = 单人也能有一个 AI 团队
   - GID = AI 自主写代码的工作流
   - Engram = AI 记忆系统
   - 这个叙事天然连接到 Engineer Union 和 Block 裁员的话题

2. **社区价值主张**：
   - 不是又一个 "AI 工具推荐" 群
   - 而是："我们这些做 AI 工具的人 + 用 AI 工具的人，如何让个体开发者变得更强"
   - 讨论：AI 替代 vs 增强、工程师的未来、小团队如何用 AI 和大公司竞争

### Why This Matters
- **所有产品都需要分发渠道** — 没有社区/影响力，再好的产品也没人知道
- **复合效应** — 影响力 → 用户 → 反馈 → 更好的产品 → 更大影响力
- **财务自由路径** — 社区是获客成本最低的渠道
- **护城河** — 代码可以被抄，社区不行

### Connections
- **IDEA-20260402-03**: Engineer Union 平台 — 可作为社区中的一个核心话题/功能
- **IDEA-20260402-04**: Marketing 自动化 Pipeline — 社区是 pipeline 的 destination
- **IDEA-20260330-01**: Social media intake — 社区内容的输入源
- **xinfluencer**: 自动化社交媒体互动 — 社区增长的工具
- **AgentVerse**: AI agent 社交平台 — 可能是社区的最终载体

### Status: 💡 New
---
