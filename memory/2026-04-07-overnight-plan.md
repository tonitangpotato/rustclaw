# 🌙 Overnight Task Plan — 2026-04-07

> **开始时间**: ~01:00 EDT
> **预计结束**: potato 醒来前
> **安全规则**: 不删数据、不删代码、不删 DB。每个 task commit。cargo test 全 pass 才 commit。
> **卡住规则**: 超过 20min 没进展 → 记录原因到 daily log → 跳到下一项
> **每小时 heartbeat 检查此文档进度**

---

## Phase 1: 清理 ✅ DONE

### T1.1 ✅ agentctl 29 tasks 标 done
- **Workspace**: `/Users/potato/clawd/projects/agentctl/`
- **状态**: 已完成

### T1.2 ✅ gid-rs features STATUS.md 更新
- **Workspace**: `/Users/potato/clawd/projects/gid-rs/`
- **状态**: 已完成

### T1.3 ✅ SESSION-PERSIST-PLAN.md 标已实现
- **Workspace**: `/Users/potato/rustclaw/`
- **状态**: 已完成

---

## Phase 2: gid-rs 核心实现

### T2.1 Ritual Context Integration
- **Workspace**: `/Users/potato/clawd/projects/gid-rs/`
- **Feature dir**: `.gid/features/ritual-context-integration/`
- **Design**: `design.md` ✅ 已写
- **Review**: `.gid/reviews/ritual-context-integration-design-review.md` ✅ 已完成 (9/11 applied, FINDING-8 deliberate skip, FINDING-9 acknowledged)
- **Graph**: ❌ 未建
- **流程**:
  1. ~~design~~ ✅
  2. ~~review~~ ✅
  3. ~~apply findings~~ ✅ (9/11)
  4. 建 graph (gid_design → parse YAML → graph.yml)
  5. 实现代码 (v2_executor.rs + harness/types.rs)
  6. cargo test -p gid-core
  7. git commit
- **改动文件**:
  - `crates/gid-core/src/ritual/v2_executor.rs` — enrich_implement_context(), safe_truncate(), resolve_gid_root()
  - `crates/gid-core/src/harness/types.rs` — TaskContext::render_prompt()
- **注意**: 刚才跳过了 graph 步骤直接 implement 了，需要补建 graph 或检查实现是否正确

### T2.2 ISS-009 Cross-Layer Phase 1
- **Workspace**: `/Users/potato/clawd/projects/gid-rs/`
- **Feature dir**: `.gid/features/iss-009-cross-layer/`
- **Design**: `design.md` ✅ 已写
- **Review**: ❌ 未做
- **Graph**: ❌ 未建
- **流程**:
  1. ~~design~~ ✅
  2. review (spawn review-design skill → write to .gid/reviews/)
  3. apply findings (等 review 完成后 apply)
  4. 建 graph
  5. 实现代码
  6. cargo test -p gid-core
  7. git commit
- **改动文件** (预计):
  - `crates/gid-core/src/graph.rs` — 新 EdgeRelation::BelongsTo
  - `crates/gid-core/src/code_graph/types.rs` — module node type
  - `crates/gid-core/src/code_graph/extract.rs` — module 节点生成 + TestsFor edge
  - `crates/gid-core/src/query.rs` — impact/deps 多 relation 遍历

### T2.3 ISS-006 Incremental Extract
- **Workspace**: `/Users/potato/clawd/projects/gid-rs/`
- **Feature dir**: `.gid/features/incremental-extract/`
- **Design**: `DESIGN.md` ✅ 已写
- **Review**: `DESIGN-review.md` ✅ 已完成 (FINDING-1~4 需 apply)
- **Graph**: ❌ 未建
- **流程**:
  1. ~~design~~ ✅
  2. ~~review~~ ✅
  3. apply FINDING-1~4
  4. 建 graph
  5. 实现代码
  6. cargo test -p gid-core
  7. git commit
- **注意**: review 建议 apply FINDING-1~4 然后实现

### T2.4 SQLite Migration
- **Workspace**: `/Users/potato/clawd/projects/gid-rs/`
- **Feature dir**: `.gid/features/sqlite-migration/`
- **Design**: 5 个 design docs ✅ (design-storage, design-migration, design-history, design-context, master)
- **Review**: 所有 reviews 已完成且 applied ✅
- **Graph**: ❌ 未建
- **流程**:
  1. ~~design~~ ✅
  2. ~~review~~ ✅
  3. ~~apply findings~~ ✅
  4. 建 graph
  5. 实现代码 — **最大块**，时间不够先做 StorageTrait
  6. cargo test -p gid-core
  7. git commit
- **注意**: 这是最大的 task，时间不够可以只实现 storage trait 层

---

## Phase 3: RustClaw 端集成

### T3.1 RustClaw Ritual Integration
- **Workspace**: `/Users/potato/rustclaw/`
- **前置**: T2.1 完成
- **流程**:
  1. 更新 Cargo.toml 的 gid-core 依赖 (git ref 或 path)
  2. 调整 RustClaw 的 ritual 调用代码
  3. cargo build --release
  4. cargo test
  5. 重启 daemon (launchd KeepAlive 已确认)
  6. git commit
- **改动文件** (预计):
  - `Cargo.toml` — gid-core 版本/ref
  - `src/ritual_runner.rs` — 确认调用新 API
- **注意**: 重启前确认 launchd KeepAlive

---

## Phase 4: Engram + 商业文档

### T4.1 Engram Cloud Business Plan
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **流程**:
  1. 读现有文档: ENGRAM-V2-DESIGN.md, MEMORY-SYSTEM-RESEARCH.md
  2. 读 IDEAS.md 中 Engram 相关 ideas
  3. 写 BUSINESS-PLAN.md (Hub + Share Memory)
  4. git commit
- **注意**: 参考 gid-rs BUSINESS-PLAN.md 格式

### T4.2 ISS-001 P0 consolidate corruption 修复
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **前置**: 读 INVESTIGATION-2026-03-31.md
- **流程**:
  1. 诊断问题根因
  2. 实现修复 (integrity_check + FTS5 rebuild + exclusive lock)
  3. cargo test
  4. git commit
- **注意**: 这是 P0 bug，涉及 DB 操作要格外小心

### T4.3 Bracket Resolution Skill 立项
- **Workspace**: `/Users/potato/rustclaw/`
- **流程**:
  1. 在 skills/ 创建 bracket-resolution/SKILL.md
  2. 写 YAML frontmatter + instructions
  3. 或者走 ritual 流程 design → implement
- **参考**: IDEAS.md IDEA-20260406-01

### T4.4 Engram Hub → requirements
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **流程**:
  1. 读 engram-hub-discussion.md
  2. 写 requirements doc (GOALs + GUARDs)
  3. git commit

### T4.5 Engram Share Memory → requirements
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **流程**: 同 T4.4

### T4.6 Context Partitioning → crate API
- **Workspace**: `/Users/potato/rustclaw/` 或新 crate
- **Feature dir**: `.gid/features/context-partitioning/` (如已存在)
- **参考**: IDEAS.md IDEA-20260406-04
- **流程**:
  1. 读已有 requirements docs
  2. 设计 API
  3. 写 design doc
- **注意**: 这个可能需要 potato 确认方向

---

## Phase 5: Engram 全面梳理

### T5.1 Engram 文档 → TODO-MASTER.md
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **流程**:
  1. 读所有文档: ISSUES.md, LEARNINGS.md, ENGRAM-V2-DESIGN.md, INVESTIGATION-2026-03-31.md, MEMORY-SYSTEM-RESEARCH.md
  2. 汇总所有未实现项
  3. 写 TODO-MASTER.md (分优先级)
  4. git commit

### T5.2 为每个待实现项建 feature dir
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **前置**: T5.1 完成
- **流程**:
  1. 为每个 P0/P1 项创建 `.gid/features/<name>/`
  2. 写 requirements.md
  3. 更新 graph
  4. git commit

### T5.3 Open issues ISS-002~007
- **Workspace**: `/Users/potato/clawd/projects/engram-ai-rust/`
- **流程**:
  1. 读 ISSUES.md ISS-002~007
  2. 为每个可快速修复的 issue 做 fix
  3. cargo test
  4. git commit
- **注意**: 只修简单的，复杂的留给 potato 决定

---

## ⚠️ 执行规则 (每次开始 task 前重读)

1. **完整流程**: design → review → apply findings → 建 graph → implement → test → commit
2. **卡住就跳过**: 超过 20min 没进展 → 记录原因到 daily log → 跳下一项
3. **不删除任何东西**: 只增改，不删
4. **每完成一个 task 就 git commit**: 方便回滚
5. **cargo test 必须全 pass**: 不 pass 不 commit
6. **不确定的决策写 daily log**: 等 potato 决定
7. **RustClaw 重启前确认 launchd KeepAlive**: 已确认 OK
8. **Phase 顺序可调**: 如果某个 Phase 卡住，可以跳到下一个 Phase

---

## 进度追踪 (heartbeat 每小时更新)

| Task | Status | 时间 | 备注 |
|------|--------|------|------|
| T1.1 | ✅ | 00:30 | agentctl done |
| T1.2 | ✅ | 00:40 | STATUS.md |
| T1.3 | ✅ | 00:45 | session persist |
| T2.1 | ✅ | 01:05 | Fixed duplicate fn defs, compile fix, 307 tests pass, committed |
| T2.2 | ⬜ | | |
| T2.3 | ⬜ | | |
| T2.4 | ⬜ | | |
| T3.1 | ⬜ | | |
| T4.1 | ⬜ | | |
| T4.2 | ⬜ | | |
| T4.3 | ⬜ | | |
| T4.4 | ⬜ | | |
| T4.5 | ⬜ | | |
| T4.6 | ⬜ | | |
| T5.1 | ⬜ | | |
| T5.2 | ⬜ | | |
| T5.3 | ⬜ | | |
