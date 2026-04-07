# HEARTBEAT.md

## 🌙 Overnight Task Plan (2026-04-07, active until potato wakes up)
## 🌙 Overnight Task Plan (2026-04-07, active until potato wakes up)

> **📋 详细计划: `memory/2026-04-07-overnight-plan.md`**
> **每小时 heartbeat 检查一次进度。卡住就跳过，记录原因，继续下一项。**
> **完整流程: design → review → apply → graph → implement → test → commit**

### Phase 1: 清理 ✅ DONE
- [x] T1.1 agentctl tasks 标 done
- [x] T1.2 gid-rs STATUS.md
- [x] T1.3 SESSION-PERSIST-PLAN.md

### Phase 2: gid-rs 核心 (`/Users/potato/clawd/projects/gid-rs/`)
- [x] T2.1 Ritual Context Integration — ✅ Done (手动修复，307 tests pass)
- [x] T2.1.5 Ritual Workspace Detection Bug — ✅ Fixed (支持 crates/*/src/ workspace layout)
- [x] T2.1.6 Ritual Workspace Detection Root Fix — ✅ 用 has_source_in_project() 和 count_source_files_in_project() 替换硬编码 crates//packages/
- [x] T2.2 ISS-010 Review 深度分级 — ✅ Done — review skill 按 triage size 分级 (small=8checks/sonnet, medium=20/opus, large=27/opus) + 修 _max_iterations 未传递
- [ ] T2.3 ISS-009 Cross-Layer — design✅ → review → apply → graph → implement → test → commit
- [ ] T2.4 ISS-006 Incremental Extract — design✅ review✅ → apply FINDING-1~4 → graph → implement → test → commit
- [ ] T2.5 SQLite Migration — design✅ review✅ applied✅ → graph → implement (大块，先做 trait) → test → commit

### Phase 3: RustClaw (`/Users/potato/rustclaw/`)
- [ ] T3.1 Ritual Integration — 更新 gid-core dep → 调整代码 → build → test → 重启 daemon

### Phase 4: Engram (`/Users/potato/clawd/projects/engram-ai-rust/`)
- [ ] T4.1 Business Plan (Hub + Share Memory)
- [ ] T4.2 ISS-001 P0 consolidate corruption 修复
- [ ] T4.3 Bracket Resolution Skill (`/Users/potato/rustclaw/skills/`)
- [ ] T4.4 Engram Hub → requirements
- [ ] T4.5 Engram Share Memory → requirements
- [ ] T4.6 Context Partitioning → crate API

### Phase 5: Engram 全面梳理 (`/Users/potato/clawd/projects/engram-ai-rust/`)
- [ ] T5.1 文档汇总 → TODO-MASTER.md
- [ ] T5.2 feature dirs 创建
- [ ] T5.3 ISS-002~007 简单修复

### ⚠️ 执行规则
1. **完整流程** — design → review → apply → graph → implement → test → commit
2. **卡住就跳过** — 20min 没进展 → 记录原因 → 下一项
3. **不删除任何东西** — 只增改，不删
4. **每个 task commit** — cargo test 全 pass 才 commit
5. **不确定的决策写 daily log** — 等 potato 决定
6. **Ritual 问题即修** — 使用 ritual 过程中遇到任何问题（bug、卡住、行为不符预期），立即记录到 ISSUES.md + daily log，然后 root fix（不是 workaround）

---

## Memory Maintenance (每次 heartbeat)
- [ ] `engram --db ~/rustclaw/engram-memory.db consolidate` — strengthen memories
- [ ] `engram --db ~/rustclaw/engram-memory.db stats` — check stats, note any anomalies
- [ ] 检查 overnight task 进度 — 标记已完成项，推进下一项

## Proactive Habits (对话中)
When answering questions about history/preferences:
→ First: `engram --db ~/rustclaw/engram-memory.db recall "..." --limit 5`

When learning something important:
→ Store: `engram --db ~/rustclaw/engram-memory.db add --type factual --importance 0.8 "..."`

## Auto-Triage (每次 heartbeat)
- [ ] Read `.gid/meta-graph.yml` — check for `status: new` action_item nodes
- [ ] For P0 items: read intake source file + target project code → generate epic + task nodes in target project's `.gid/graph.yml` → update meta-graph status to `triaged`
- [ ] For P1 items (if time permits): same as P0 but lower priority
- [ ] P2 items: leave as-is until manually requested
- [ ] Report any newly triaged items to potato via Telegram

## Disk Health (每天一次)
- [ ] Check `df -h /` — if <15GB free, alert potato
