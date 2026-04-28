# Autopilot Tasks — 2026-04-28 (overnight) [v2]

> potato 睡觉，agent 自动跑这些。每个子任务独立、单 commit、有明确 DoD。
> Repos：`gid-rs` (`/Users/potato/clawd/projects/gid-rs`) / `rustclaw` (`/Users/potato/rustclaw`) / `engram` (`/Users/potato/clawd/projects/engram`)。

---

## 总原则

1. **不能动**：gid-rs ISS-052（potato 手测中）/ rustclaw ISS-050 / ISS-051（依赖 0.4.0 发布）。**不要 publish 任何 crate**。**不要改 ISS-052 的 design.md**——potato 正在拿它做 acceptance reference。
2. **执行单元 = 子任务（1a / 1b / 2a ...）**：每个子任务独立 commit，DoD ≤ 80 LOC + 1-2 tests，main agent 直接做（不 delegate，避免 sub-agent 失败浪费 token）。
3. **执行顺序**：严格按编号顺序串行。同 module 改动有冲突依赖，**不要并行**。
4. **Ritual 流程**：src/* 改动走 `/ritual <ISS-NNN>`。文档/issue.md 改动直接编辑。
5. **失败处理**：卡住（编译错、范围超预期、改完测试挂修不掉）→ 写 `/Users/potato/rustclaw/BLOCKER-<task-id>.md` 包含错误信息和已尝试方案，**跳过**做下一个，**不要硬撑**。
6. **Commit**：一个子任务一个 commit。message: `<type>: ISS-NNN <one-line>`。例：`feat: ISS-028a add AlreadyActive variant`。
7. **Push**：每个 commit 直推 origin/main。两个 repo 都允许直推。
8. **现实预期**：预计能跑完 4-7 个子任务（约 3-5 小时实际工作）。Engram task 9 + 10 + 12 是文档型快速任务（合计 ~45min），即使主线 stuck 也建议先把这三个做完。Task 11 是 engram 唯一实质代码任务，**估 90min，只在 rustclaw / gid-rs 主线推进顺利时插入**——如果出现任何 BLOCKER，跳过 11，留给 potato。剩下的写到收尾日志，明天再做。

---

## 执行排序（严格按此顺序）

子任务 → 估时 → 风险

| # | 任务 | Repo | 估时 | 风险 |
|---|---|---|---|---|
| 1a | ISS-028 AlreadyActive variant + pre-flight check + 3 单测 | rustclaw | 45m | 低 |
| 1b | ISS-028 orphan reconciler（启动期扫描 + WARN log） | rustclaw | 30m | 低 |
| 2a | ISS-029 RitualState 加字段 + RitualHealth 枚举 + health() + 单测 | rustclaw | 40m | 低 |
| 2b | ISS-029 phase 循环里 30s heartbeat tick + tool 输出 health | rustclaw | 45m | 中 |
| 3a | ISS-056 Layer 1: triage → max_iterations | gid-rs | 15m | 低 |
| 4 | ISS-051 save_state retry + StatePersistFailed event + 单测 | gid-rs | 60m | 低 |
| 3b | ISS-056 Layer 2: STATUS 协议 + Paused state + 解析 + 单测 | gid-rs | 70m | 中 |
| 7 | ISS-050 process protocol doc（纯文档） | gid-rs | 30m | 低 |
| 8 | ISS-053 v3 design review（产出 review 文件，不应用） | gid-rs | 40m | 低 |
| 9 | engram ISS-046 housekeeping（mark closed） | engram | 10m | 低 |
| 10 | engram ISS-047 housekeeping（mark done + 链 ISS-048） | engram | 10m | 低 |
| 11 | engram ISS-048 lift novel edge endpoints（Option B 核心实现 + 2 单测） | engram | 90m | **🟡 中-高** |
| 12 | engram open-iss audit doc | engram | 25m | 低 |
| 6 | **[最后做、风险高]** ISS-049 skills hot-reload | rustclaw | 60m | **🔴 高** |

**已删除**：原 Task 5（ISS-054 改 ISS-052 design.md）——potato 手测期间不动那个文档。

**冲突依赖**：1a→1b（同 RitualRunner）/ 2a→2b（同 RitualState）/ 3a→3b（同 V2Executor::implement_phase）/ 9→10→11（engram 内部，避免 issue.md merge 干扰；11 也依赖 9 + 10 的 housekeeping 先清掉）。**必须串行**。

**Engram 任务定位**：9 / 10 / 12 是低风险文档/审计；**11 是今晚 engram 唯一实质代码任务**，独立 commit，不阻塞 rustclaw / gid-rs 任何任务，可以在 rustclaw / gid-rs 任务跑完后插入；如果时间不够 → **跳过 Task 11，留给 potato 起床后用更长 session 做**。9 / 10 / 12 即使时间紧也建议做完——纯文档、零风险。

---

## Task 1a — rustclaw ISS-028a: AlreadyActive variant + pre-flight check [P0]

**Repo**: `/Users/potato/rustclaw` · **Issue**: `.gid/issues/ISS-028/issue.md`

### Scope（≤ 80 LOC）

只做三件事：
1. `RitualConflict` 加 `AlreadyActive { ritual_id, phase, started_at }` variant。
2. `RitualRunner::start_with_work_unit` 入口加 pre-flight：扫 `runtime/rituals/` 目录的活 ritual，匹配 `work_unit.label()` + 非终态 → 返回 `AlreadyActive`。
3. `StartRitualTool` 把 `AlreadyActive` 转成 agent-friendly 字符串（按 ISS-028 GOAL-3 给的格式）。

### DoD

- [x] `RitualConflict::AlreadyActive` 加好。
- [x] `start_with_work_unit` 在 ritual 创建前查重；复用 ISS-019 已有的 terminal phase 定义，**不要新写一份**。
- [x] `StartRitualTool` 输出可读错误。
- [x] 单测 3 个：(a) 无 active → ok (b) 同 work_unit 二次 start → AlreadyActive (c) 第一个进 terminal 后二次 start → ok。
- [x] `cargo test ritual` 全绿。
- [x] commit + push: `feat: ISS-028a duplicate ritual prevention`

### 实施提示

- Pre-flight check 扫目录就行，N < 10，开销可忽略。
- 不要加新枚举/状态，只复用现有 phase 检查。

### Ritual

`/ritual ISS-028`，phase = implementing。1a 完成后**不要 close ritual**，1b 继续在同一 ritual 里做。

---

## Task 1b — rustclaw ISS-028b: orphan reconciler [P0]

**Repo**: `/Users/potato/rustclaw` · **依赖 1a**

### Scope（≤ 50 LOC）

`RitualRunner::new` 末尾跑一次 orphan reconciler：扫所有 ritual 文件，发现 same `work_unit.label()` 的 terminal 重复 → log WARN，**不删**（GOAL-5）。

### DoD

- [x] Reconciler 函数加好（同步或 spawn 都行，但要在 `new()` 里跑一次）。
- [x] WARN log 包含 ritual_ids 列表 + work_unit label。
- [x] 单测：构造 fixture 目录有 2 个同 work_unit 的 terminal ritual → reconciler 返回 1 个 duplicate group。
- [x] commit + push: `feat: ISS-028b orphan ritual reconciler`
- [x] **完成后 close ISS-028 ritual**（这是 ISS-028 的最后一块）。

---

## Task 2a — rustclaw ISS-029a: RitualState 字段 + RitualHealth + health() [P0]

**Repo**: `/Users/potato/rustclaw` · **Issue**: `.gid/issues/ISS-029/issue.md` · **依赖 1b 完成**（同模块）

### Scope（≤ 70 LOC + serde 兼容）

纯数据/逻辑层改动，不碰执行循环：
1. `RitualState` 加 `phase_entered_at: DateTime<Utc>` + `last_heartbeat: DateTime<Utc>` 字段，`#[serde(default = ...)]` 默认到 `updated_at`，**确保旧 ritual 文件读得回来**。
2. `RitualHealth` 枚举（Healthy / LongRunning / Wedged / Terminal）。
3. `RitualState::health(now: DateTime<Utc>) -> RitualHealth` 方法。Wedged 阈值：`now - last_heartbeat > 60s`（即 2 × 30s tick）。
4. Phase transition 处统一更新 `phase_entered_at`——找单一 transition 入口集中改，不要散落。

### DoD

- [x] 字段加好 + serde 兼容（旧文件 round-trip 测试通过）。
- [x] `RitualHealth` + `health()` 实现。
- [x] 单测：(a) 旧 ritual 文件 deserialize ok (b) phase=Implementing + last_heartbeat=now-90s → Wedged (c) terminal phase 永远返回 Terminal。
- [x] commit + push: `feat: ISS-029a ritual health introspection` (4e63c2a)
- [ ] 不要 close ritual，2b 接着做。

### Ritual

`/ritual ISS-029`，phase = implementing。

---

## Task 2b — rustclaw ISS-029b: heartbeat tick + tool 输出 [P0]

**Repo**: `/Users/potato/rustclaw` · **依赖 2a**

### Scope（≤ 60 LOC）

1. Implementing/Reviewing/Verifying 三个 phase 的执行循环里加 30s tick 更新 `last_heartbeat` 并 persist。**不要起新后台线程**——找现有循环里的自然 tick 点。
2. `gid_ritual_status` tool 输出加 `health` 字段（如果 tool 不存在就在 status 报告里附加）。
3. `.gid/runtime/rituals/README.md` 加期望耗时表（GOAL-5）。

### DoD

- [ ] 三个 phase 循环都有 heartbeat tick（找一个 tick 点即可，没有就在 phase 入口/出口打两次，README 标 wedged 阈值放宽到 5min）。
- [ ] tool 输出 health。
- [ ] README.md 写好。
- [ ] commit + push: `feat: ISS-029b heartbeat + status reporting`
- [ ] close ISS-029 ritual。

### 实施提示

- 如果发现现有循环都是同步阻塞 → 只在 phase 入口/出口打 heartbeat，**README 里把 wedged 阈值文档化为 5 min**，不要硬塞 tokio task。

---

## Task 3a — gid-rs ISS-056a: triage → max_iterations [P1]

**Repo**: `/Users/potato/clawd/projects/gid-rs` · **Issue**: `.gid/issues/ISS-056/issue.md`

### Scope（≤ 20 LOC，trivial）

`V2Executor::implement_phase` 读 `state.triage.size`，传 `max_iterations` 给 `RitualLlmAdapter::run_skill`：
- small=15, medium=30, large=50
- None → 默认 30

### DoD

- [ ] 改动 ≤ 20 LOC。
- [ ] 单测：mock state with `triage.size=Some(Small)` → 验证 run_skill 收到 max_iterations=15。
- [ ] commit + push: `feat: ISS-056a triage-aware turn budget`
- [ ] 不要 close ritual，3b 接着做。

### Ritual

`/ritual ISS-056`。

---

## Task 4 — gid-rs ISS-051: save_state retry [P1]

**Repo**: `/Users/potato/clawd/projects/gid-rs` · **Issue**: `.gid/issues/ISS-051/issue.md`

> **顺序故意插在 3a 和 3b 之间**：3a 完成后 3b 是个独立大改，先把 ISS-051 这个独立小改完成，避免一直占着 ISS-056 ritual。

### Scope（≤ 60 LOC，gid-core 一侧）

1. `save_state` 返回 `Result<()>`（gid-core 内）。
2. 重试 3 次，exp backoff（100ms / 400ms / 1.6s），用 `tokio::time::sleep`，**不要引新依赖**。
3. 第 3 次失败 emit `StatePersistFailed { attempts, last_error }` event，state machine 进 `Failed`（复用 Failed，不要新 variant）。
4. **rustclaw 侧不动**——issue.md 末尾标注 "AC for rustclaw side deferred to post-0.4.0-publish"。

### DoD

- [ ] save_state 返回 Result。
- [ ] 3 次 retry + exp backoff。
- [ ] StatePersistFailed event。
- [ ] 单测：(a) mock writer 前 2 次返回 io::Error，第 3 次成功 → Ok + 0 个 event (b) 4 次都失败 → Failed 状态 + 1 个 event。
- [ ] issue.md 加 deferred 标注。
- [ ] commit + push: `feat: ISS-051 save_state retry + failure event`

### Ritual

`/ritual ISS-051`，独立 ritual。

---

## Task 3b — gid-rs ISS-056b: STATUS 协议 + Paused state [P1]

**Repo**: `/Users/potato/clawd/projects/gid-rs` · **依赖 3a**

### Scope（≤ 100 LOC）

1. `skills/implement/SKILL.md` 末尾追加 STATUS 协议（agent must end with `STATUS: complete` 或 `STATUS: incomplete: <reason>`）。
2. `RitualState` 加 `Paused { reason }` variant（**不要复用 Failed**——语义不同）。
3. `V2Executor::verify_implementation` 解析 last skill output 末尾 100 行内的 `^STATUS:` 行：
   - `complete` → 现有路径（cargo check/test）
   - `incomplete` → emit `ImplementIncomplete` event，进 `Paused`，跳过 cargo verify。
   - 缺失 → 视为 incomplete + reason="missing STATUS self-report"。

### DoD

- [ ] SKILL.md 加协议说明。
- [ ] Paused variant 加好（state machine transitions 也补全）。
- [ ] verify_implementation 解析 STATUS。
- [ ] 单测：mock skill output 末尾是 `STATUS: incomplete: hit turn limit` → final state = Paused（不是 Done，不是 Failed）。
- [ ] commit + push: `feat: ISS-056b implement-skill STATUS self-report`
- [ ] close ISS-056 ritual。

---

## Task 7 — gid-rs ISS-050: process protocol doc [P3]

**Repo**: `/Users/potato/clawd/projects/gid-rs` · **Issue**: `.gid/issues/ISS-050/issue.md`

> 纯文档，描述 `gid drift` 命令将来怎么用。**不实现**——CLI 部分依赖 0.4.0 之后才安全。

### DoD

- [ ] 新文件 `docs/drift-protocol.md`：
  - 问题陈述（4 类 drift: orphan / missing / stale / aligned）
  - `gid drift --dir <path>` 预期行为（exit codes / 输出格式）
  - `gid drift add` / `cancel` / `split` 子命令各自做什么
  - Ritual implement post-condition 调用 drift check 的预期接口
  - engram v0.3 reconciliation 作为 validation case 的设计
- [ ] AGENTS.md 加一段 "drift protocol"，链接到上面文档。
- [ ] ISS-050 issue.md 加一条 "Spec doc done in this commit; CLI implementation deferred to post-0.4.0"。
- [ ] commit + push: `docs: ISS-050 drift protocol spec`

### Ritual

否。直接编辑。

---

## Task 8 — gid-rs ISS-053: v3 design review [P3]

**Repo**: `/Users/potato/clawd/projects/gid-rs` · **Issue**: `.gid/issues/ISS-053/issue.md`

> Design 已经写得详细。今晚只产出 review，**不应用**——potato 起床后决定。

### DoD

- [ ] 检查 `skills/review-design/` skill 是否存在；不存在 → **跳过 Task 8**，不要 fallback 到别的 skill。
- [ ] 存在 → spawn sub-agent（pre-load ISS-053/issue.md），产出 `.gid/issues/ISS-053/reviews/design-r1.md`，每条 FINDING-N 格式。
- [ ] 重点关注：
  - D2 "Layout is data, not code" — scalability test 的 binding fixture 例子
  - §6 Acceptance criteria 是否每条都自动化可测
  - §7 Migration 从当前 graph.db 过渡的步骤
- [ ] 不要修改 ISS-053/issue.md 本身。
- [ ] commit + push: `docs: ISS-053 design review r1`

### Ritual

否。

---

## Task 9 — engram ISS-046: housekeeping (mark closed) [P3]

**Repo**: `/Users/potato/clawd/projects/engram` · **Issue**: `.gid/issues/ISS-046/issue.md`

> 实现已经在 `crates/engram-cli/src/main.rs` 落地（`with_pipeline_pool` + drain hooks，行 1078-1196，见 grep `ISS-046:`）。LoCoMo conv-26 验证 32/32 pipeline runs 成功。**仅 issue.md status 没更新**——这是文档维护任务，不动代码。

### Scope（≤ 30 LOC，纯文档）

1. `.gid/issues/ISS-046/issue.md` 顶部 `**Status**: open` → `**Status**: closed`，加 `**Closed**: 2026-04-28`，加一段 "Resolution" 段引用 commit hash（`git log --oneline -- crates/engram-cli/src/main.rs | grep ISS-046` 第一条）。
2. 不要碰别的字段、不要 reformat 全文。
3. **不要** 跑 `gid_complete`——engram 的 `.gid/graph.db` issue tracker 还没建 ISS-046 节点（参考 ISS-028 backfill）。

### DoD

- [ ] `Status: closed` + `Closed: 2026-04-28` + Resolution 段（≤ 5 行）。
- [ ] 引用真实的 commit hash（用 `git log` 验证，不要编）。
- [ ] commit + push: `docs: ISS-046 mark closed (impl shipped in <hash>)`

### Ritual

否，直接编辑。

---

## Task 10 — engram ISS-047: housekeeping (mark done) [P3]

**Repo**: `/Users/potato/clawd/projects/engram` · **Issue**: `.gid/issues/ISS-047/issue.md`

> Commit `be35217` 验证：32/32 pipeline runs 成功，无 rollback，`applied_deltas` 持久化正确。Status 仍是 in_progress——需要标 done。

### Scope（≤ 30 LOC，纯文档）

1. `Status: in_progress` → `Status: done`，加 `**Closed**: 2026-04-28`。
2. 加一段 "Verification" 段：32/32 LoCoMo conv-26 fresh-ingest pipeline runs 成功，no rollback，引 commit `be35217`。
3. **追加一条 Forward note**：本 issue 修了"rollback 机制"，但暴露了 ISS-048（extractors 架构 mismatch），交叉链接。

### DoD

- [ ] Status / Closed / Verification 都更新。
- [ ] Forward link 到 ISS-048。
- [ ] commit + push: `docs: ISS-047 mark done (verified on conv-26)`

### Ritual

否，直接编辑。

---

## Task 11 — engram ISS-048: lift novel edge endpoints into entity drafts [P0]

**Repo**: `/Users/potato/clawd/projects/engram` · **Issue**: `.gid/issues/ISS-048/issue.md` · **依赖**：Task 9 + Task 10 完成（不强依赖代码，但顺序上先把 housekeeping 清掉，避免 review 时分心）

> **🔴 这是今晚 engram 唯一的实质性代码任务**。Option B（见 issue.md "Plan"）：把 EdgeExtractor 产出的 triple 中、不在 `ctx.entity_drafts` 的 subject/object 提升为 `DraftEntity { kind: Other("unknown"), provenance: EdgeLift }`，让 `resolve_edges` 能解析。零额外 LLM 调用。
>
> **范围控制**：今晚只做 **核心 lift + 1 个集成测试**。`provenance: EdgeLift` 这种新枚举变体如果发现需要改 schema → **跳过、写 BLOCKER**，不要硬上。

### Scope（≤ 150 LOC）

**单 commit、单 ritual。** 拆成三块顺序写：

#### 块 A：adapter 函数（~30 LOC，改 `crates/engramai/src/resolution/adapters.rs`）

加 `pub fn draft_entity_from_triple_endpoint(name: &str, occurred_at, affect, content_hash) -> DraftEntity`：
- canonical_name = 复用 `draft_entity_from_mention` 同款 normalization（NFKC + trim + lowercase）。
- `kind = EntityKind::other("unknown")`（用 sanctioned constructor，不要直接 `EntityKind::Other(...)`）。
- `provenance` 字段：**先看 `DraftEntity` 现有 provenance enum**——如果有 `Pattern` / `Mention` 这种已存在的变体，用最弱信号那个；如果发现要新增 `EdgeLift` 变体且改动 > 20 LOC（serde / DB schema / 现有 match 不全） → **块 A 写 STOP，写 BLOCKER-task11.md，跳整个 task 11**。

#### 块 B：pipeline glue（~50 LOC，改 `crates/engramai/src/resolution/pipeline.rs`）

在 `extract_edges` 之后、`resolve_edges` 之前，插一步 `lift_novel_endpoints(ctx)`：
- 收集 `ctx.entity_drafts` 中所有 normalized canonical_name → set。
- 遍历 `ctx.extracted_triples`：每个 triple 的 subject + object，normalize 后**不在 set 里**就调 adapter 生成 draft 追加到 `ctx.entity_drafts`，并把 normalized 名加进 set 防本批次重复。
- 找不到 triples 字段名就 grep `extracted_triples` 找现有用法。
- **不要**改 `resolve_edges` 的逻辑，那是 ISS-047 fix 的边界。

#### 块 C：测试（~50 LOC）

1. 单测 `pipeline_lifts_novel_edge_subjects_into_drafts`（放 `pipeline.rs` 同文件 `#[cfg(test)] mod tests`）：
   - 构造 mock `extracted_triples = [("Caroline Martinez", "works_at", "Acme")]`。
   - `entity_drafts` 初始为空。
   - 调 `lift_novel_endpoints`。
   - assert: `entity_drafts.len() == 2`，canonical_name 包含 `"caroline martinez"` 和 `"acme"`，kind 都是 `EntityKind::other("unknown")`。
2. 单测 `pipeline_lift_dedup_against_pattern_drafts`：
   - `entity_drafts` 已有 `"caroline martinez"`（pattern 命中）。
   - triple 也是 `("Caroline Martinez", ..., "Acme")`。
   - lift 后 `entity_drafts.len() == 2`（不是 3）：原 caroline + 新 acme。

**不**做的：LoCoMo conv-26 端到端集成测试（基础设施太重，留给 potato 起床后跑）。issue.md 的 "Acceptance criteria" 里 LoCoMo 那条 **block 留着、标 deferred**。

### DoD

- [ ] adapter 函数 + 测试覆盖 normalization。
- [ ] pipeline `lift_novel_endpoints` 钩在 `extract_edges` 和 `resolve_edges` 之间。
- [ ] 2 个单测过。
- [ ] `cargo test -p engramai resolution::pipeline` 全绿。
- [ ] **不要**跑全 workspace `cargo test`——超时风险高、任何无关 fail 都会让你 panic 回滚；只跑 `-p engramai`。
- [ ] issue.md 加一段 "Implementation note (2026-04-28)"：列已实现 / 已 deferred 的部分。
- [ ] commit + push: `feat: ISS-048 lift novel edge endpoints into entity drafts`
- [ ] **不要** close ISS-048——LoCoMo 集成测试是 acceptance gate，没跑过不算完。issue.md status 保持 `in_progress`。

### 实施提示

- **provenance enum 是关键风险点**。先 `grep -rn "DraftEntity" crates/engramai/src/resolution/` 看现有结构，再下笔。
- normalization 必须和 `draft_entity_from_mention` 完全一致——否则 dedup 漏，pattern-命中的 entity 会和 lifted 的同名 entity 双写。**dedup 测试就是为了抓这个 bug**。
- 如果 `DraftEntity` 没有 provenance 字段（v0.3 设计可能没加），就先不加，加 TODO 注释 + 在 issue.md 标后续补。

### Ritual

`/ritual ISS-048`，phase = implementing。**block A 完成或失败前不要进 block B**（避免回滚困难）。

---

## Task 12 — engram open-ISS audit doc [P3]

**Repo**: `/Users/potato/clawd/projects/engram`

> 把 engram 仓库当前 open issues 状态写一个快照，方便 potato 起床后扫一眼决定下一步。**不动代码**。

### Scope（≤ 100 LOC markdown）

新建 `.gid/issues/_audit-2026-04-28.md`：

#### 内容大纲

1. **Open / in_progress 列表**（grep `Status` 字段）：
   - ISS-040（P2，graph store generic refactor）
   - ISS-041（P1，Episode struct 定义）
   - ISS-042（P1，ReextractReport struct）
   - ISS-043（P2，单 tx atomicity in PipelineRecordProcessor）
   - ISS-044（P1，Wire MigrationOrchestrator → PipelineRecordProcessor）
   - ISS-046 / ISS-047（housekeeping，今晚已 close，标已完成）
   - ISS-048（in_progress，今晚 partial）
2. **v0.3 in_progress features**（来自 `.gid-v03-context/graph.db`）：
   - feature:retrieval-classification（in_progress）
   - feature:retrieval-execution（in_progress）
   - feature:v03-retrieval（parent，in_progress）
3. **Critical path 推荐**（基于依赖）：
   - ISS-048 完成 LoCoMo 验证 → ISS-021 / ISS-018 retrieval quality 才能干净测
   - ISS-041 + ISS-042 是 v0.3 ingestion contract 的前置，不做下面 retrieval orchestrator 不能 wiring
   - ISS-043 + ISS-044 是 migration 块，可以独立做（不阻塞 retrieval）
4. **建议下个 session 的优先级**：potato 起床后挑一个方向（retrieval orchestrator 链路 vs migration 块 vs 单 issue 收尾），不要一次都做。

### DoD

- [ ] 文件创建好，三个段落齐全。
- [ ] 列表里所有 issue ID 都用 grep 实际验证存在 + 引 issue.md 的 Title 行（不要凭记忆）。
- [ ] commit + push: `docs: engram open-iss audit 2026-04-28`

### 实施提示

- 用 `grep -m1 "^Status\|^- \*\*Status" .gid/issues/ISS-NNN/issue.md` 批量提取状态——注意 040-044 是 frontmatter 格式，045+ 是 inline 格式。
- 不要去重新阅读每个 issue.md 全文——太费 context。只读 head + status。

### Ritual

否。

---

## Task 6 — rustclaw ISS-049: skills hot-reload [P2 但放最后]

**Repo**: `/Users/potato/rustclaw` · **Issue**: `.gid/issues/ISS-049/issue.md`

> **🔴 风险高**：改的是 RustClaw 自己依赖的 watcher。写错会让 agent 自己挂掉，晚上没人盯着重启不了。
> **执行规则**：只在前面所有任务都成功后才做。如果有任何 BLOCKER 已经出现 → **跳过 Task 6**，留给 potato 起床后做。

### DoD

- [ ] `src/skills.rs` 加 file watcher（用 `notify` crate，不引新依赖）。
- [ ] 200ms debounce。
- [ ] 触发后 rebuild SkillRegistry + TriggerStrategy，原子替换。
- [ ] 不监听 `.tmp` / `*.swp` / `.DS_Store`。
- [ ] **错误恢复保底**：watcher 内任何 panic / error → 保留旧 registry，log WARN，**绝不清空 registry**（这是防自挂的关键）。
- [ ] 单测：tempdir 创建 skill → registry 包含；修改 → 反映新内容；删除 → 移除。
- [ ] log INFO: "skills reloaded: N skills, +X added, -Y removed"。
- [ ] **冒烟测试**：commit 前手动跑 `cargo test skills`，**再**在本地跑一次 `cargo build --release`，**再**手动起 binary 验证 skills 能加载——三条都通过才 push。
- [ ] commit + push: `feat: ISS-049 skills hot-reload`

### Ritual

`/ritual ISS-049`。

### 实施提示

- 抄 `src/config.rs` 的 watcher 模式。
- 用 `Arc<RwLock<SkillRegistry>>` 或 `ArcSwap`——看现状，哪个简单用哪个。
- 出错路径必须保守：catch 一切 → log → 不动 registry。

---

## 收尾（每个 task 结束后做）

每完成一个子任务后立即：
1. `git push`
2. 在 `memory/2026-04-28.md` 追加一行：`- ✅ <task-id>: <commit-hash> <one-line summary>`
3. 失败 → `BLOCKER-<task-id>.md` 写错误信息 + 已尝试方案，daily log 标 ❌。

全部跑完（或时间到 / 出现 blocker）后，在 `memory/2026-04-28.md` 末尾写总结：
- 完成数 / 跳过数 / 失败数
- 每个失败的 BLOCKER 文件路径
- 给 potato 起床的 next-action 建议

### Hard rules（重申）

- ❌ 不要 `cargo publish`
- ❌ 不要改 `gid-rs/.gid/issues/ISS-052/design.md`
- ❌ 不要改 rustclaw ISS-050 / ISS-051 实现（只是文档/标注 OK）
- ❌ 不要在 engram 仓库跑全 workspace `cargo test`——ISS-021 / retrieval orchestrator 链有已知 fail/skip，会污染绿/红判定。Task 11 只跑 `cargo test -p engramai resolution::pipeline`。
- ❌ 不要 close ISS-048（Task 11 完成后保持 in_progress——LoCoMo 验证还没跑）。
- ❌ 不要碰 `.gid-v03-context/graph.db`——那是 v0.3 build graph，今晚不需要改。
- ❌ 失败任务不要硬撑——跳过、记录、继续
- ❌ Task 6 风险高，前面任何 BLOCKER 出现就跳过
- ❌ Task 11（engram）出 BLOCKER 不影响后续任务执行——engram 与其他 repo 独立。
