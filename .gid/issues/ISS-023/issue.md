---
id: "ISS-023"
title: "clawd/projects path debt — hardcoded paths across configs"
status: open
priority: P3
created: 2026-04-23
component: "rustclaw.yaml, MEMORY.md, multiple configs"
---
# ISS-023: `/Users/potato/clawd/projects/` Path Debt — Migrate to `/Users/potato/projects/`

**发现日期**: 2026-04-23
**发现者**: potato + RustClaw
**组件**: workspace-wide (rustclaw configs, MEMORY.md, gid registry, per-project docs)
**优先级**: P2 (not blocking, but accumulates if ignored)
**状态**: Phase 1 done (symlink), Phase 2-3 pending
**类型**: tech-debt
**标签**: filesystem, migration, configs, root-fix

---

## 症状

所有项目散在 `/Users/potato/clawd/projects/` 下，但 `clawd/` 本身已经不是活跃的 workspace root（OpenClaw 那套现在基本不用了，rustclaw 才是主力）。当前访问这些项目要么通过：

- 绝对路径 `/Users/potato/clawd/projects/engram/`（冗长、命名历史包袱）
- Symlink `/Users/potato/rustclaw/projects → /Users/potato/clawd/projects`（只在 rustclaw workspace 内生效，其他地方没有）

**结果**：文档、配置、memory 里一半写 `clawd/projects/`，一半写 `rustclaw/projects/`，混乱。

## 根因（Root Cause）

历史原因：
1. 最早 OpenClaw 时代，`/Users/potato/clawd/` 是唯一 workspace，projects 自然放在下面
2. RustClaw 独立出来后，`clawd/` 降级为"共享数据目录"，但 projects 没有搬家
3. 加了个 symlink `rustclaw/projects → clawd/projects` 临时缓解，但没有在跨 workspace 场景下根治

这不是单纯的命名问题——是**组织结构没跟上工具变化**。现在主力是 rustclaw，projects 应该在 `$HOME/projects/` 这种 workspace-neutral 位置，任何 workspace 都能直接引用。

## 影响面 Audit（2026-04-23 scan）

**硬编码 `/Users/potato/clawd/projects/` 的位置**：

### 关键配置（会 break 运行时）
- `/Users/potato/.config/gid/projects.yml` — gid project registry，7+ 条目
- `/Users/potato/rustclaw/Cargo.toml` — path dep 指向 engramai / gid-core
- `/Users/potato/rustclaw/rustclaw.yaml` — gid registry overrides, specialist workspaces

### 文档（迁移后需同步，不影响运行）
- `/Users/potato/rustclaw/MEMORY.md` — canonical project roots 表 (~10 refs)
- `/Users/potato/rustclaw/IDEAS.md` — 散引用 (~5 refs)
- `/Users/potato/rustclaw/TOOLS.md` — shared workspace 说明
- `/Users/potato/rustclaw/AGENTS.md` — 可能有
- 各 project 内部的 `AGENTS.md` / `MEMORY.md` / `docs/` — 未详细 audit，至少几十处

### 未 audit（必然存在）
- 各 project 的 `.gid/graph.db` 里 node 的 `file_path` 字段（SQLite TEXT 列，绝对路径）
- 各 project 的 `.gid/config.yml`
- launchd plist（如果有 daemon）
- shell scripts (`~/bin/`, 各 project 的 `scripts/`)
- git worktrees / git remotes（不太可能但要查）

## 三阶段迁移方案（Root Fix，分阶段降风险）

### Phase 1: Symlink 桥接（已完成 2026-04-23）

```bash
ln -s /Users/potato/clawd/projects /Users/potato/projects
```

**效果**：
- 新代码/新 config 可以用 `/Users/potato/projects/...`
- 老的 `/Users/potato/clawd/projects/...` 继续工作
- 零风险，零停机
- 不清 debt，只是解除阻塞

**验收**：
- [x] `ls /Users/potato/projects/engram/` 能访问（等价于 `clawd/projects/engram`）
- [x] 现有 daemon / config 继续正常工作

### Phase 2: 系统性替换所有引用（未来）

用 ripgrep 列出所有引用，逐个文件替换：

```bash
rg -l "/Users/potato/clawd/projects" ~/rustclaw ~/.config ~/clawd/projects \
  | grep -v '.git/' | grep -v 'target/' | grep -v '.gid/backups/'

# 对每个文件：
sed -i '' 's|/Users/potato/clawd/projects/|/Users/potato/projects/|g' <file>
```

**顺序**：
1. 先改**文档**（MEMORY.md, IDEAS.md, TOOLS.md, AGENTS.md, per-project docs）——零运行时风险
2. 再改**非关键配置**（rustclaw.yaml specialist workspaces 里的引用）
3. 最后改**关键配置**（`Cargo.toml` path dep, `~/.config/gid/projects.yml`）——必须验证 build/runtime 正常
4. **跳过**：`.gid/graph.db` 里的 `file_path`（重跑 extract 即可；不值得为旧图 SQL 改）

**每一步后验证**：
- Cargo 编译通过
- `gid tasks` / `gid validate` 正常（registry 生效）
- rustclaw daemon `systemctl`/launchd 重启正常

### Phase 3: 物理 move + 删除 symlink（最终）

```bash
# 停所有用 clawd/projects 路径的 daemon
# （rustclaw, 可能的 cron, launchd services）

mv /Users/potato/clawd/projects /Users/potato/projects_real
rm /Users/potato/projects        # 删 Phase 1 的 symlink
mv /Users/potato/projects_real /Users/potato/projects  # 物理搬家完成

# 反向 symlink（可选，给 legacy 外部引用留后路）
ln -s /Users/potato/projects /Users/potato/clawd/projects

# 验证
# - 所有 git repo HEAD 正常
# - rustclaw 启动
# - gid 注册表生效
```

**前置条件**：Phase 2 完成，grep 结果为 0 matches（除了故意留的 archival docs）。

**回滚方案**：Phase 3 如果出问题，把 symlink 反过来即可（`/Users/potato/clawd/projects → /Users/potato/projects`）。

## 副问题（一起考虑）

1. **symlink 语义 vs bind mount**：Phase 1 用了 symlink，某些 tool（特别是会 resolve realpath 的工具，如 git、cargo）可能把路径 normalize 回 `clawd/projects`。实测 cargo path dep 用 `/Users/potato/projects/...` 会被 normalize 到 `clawd/projects/...`，不影响功能但 lock file 里会显示真实路径。**结论**：可接受，Phase 3 物理搬家后 normalize 反过来也就对了。

2. **rustclaw workspace 内的 `projects → clawd/projects` symlink**：已经存在。Phase 3 之后应该改成 `projects → /Users/potato/projects`（或者直接删，因为 `$HOME/projects/` 已经是 workspace-neutral）。

3. **`.gid/graph.db` 里的 file_path**：SQLite 列存绝对路径。Phase 3 后这些路径会变成 broken pointer。**建议**：不做 SQL migration，直接对每个 project 跑一次 `gid extract --incremental` 让 gid 自己 refresh。

## 验收标准

### Phase 1（done）
- [x] `/Users/potato/projects/` symlink 存在
- [x] 新代码可以用该路径访问项目
- [x] 本 ISS 文档存在于 rustclaw `.gid/issues/`

### Phase 2（pending）
- [ ] 所有 `/Users/potato/rustclaw/` 下的 `.md/.yaml/.toml` 文件 grep `clawd/projects` = 0 matches
- [ ] `~/.config/gid/projects.yml` 所有 path 字段用 `/Users/potato/projects/`
- [ ] `rustclaw/Cargo.toml` 的 path dep 用 `/Users/potato/projects/`
- [ ] `cargo build --release` 通过
- [ ] `gid tasks --project engram` 等命令正常

### Phase 3（pending）
- [ ] `/Users/potato/projects/` 是真实目录（非 symlink）
- [ ] `/Users/potato/clawd/projects` 不存在或是反向 symlink
- [ ] rustclaw daemon 重启成功
- [ ] 各项目的 `.gid/graph.db` 重跑 extract，node 的 file_path 用新路径

## 影响范围

- 运行时系统：rustclaw, 所有用 gid registry 的工具
- 文档：rustclaw 的 MEMORY/IDEAS/TOOLS/AGENTS + 每个 project 内部的对应文档
- 破坏性：Phase 3 之前无破坏；Phase 3 当下如果漏改引用会 break
- 回滚成本：Phase 1/2 可随时回滚；Phase 3 回滚 = 再搬一次（低风险，就是浪费时间）

## 关联

- `/Users/potato/rustclaw/projects` symlink（存在已久）——Phase 1 之前这是唯一的 workaround
- 触发讨论：2026-04-23 23:40 potato 提议整体迁移
- 先决依赖：无
- 阻塞：无（Phase 1 已解除阻塞）
