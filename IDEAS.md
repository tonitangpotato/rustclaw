# IDEAS.md - Idea Repository

> All ideas captured by RustClaw's Idea Intake pipeline.
> Format: newest first. Each idea has a unique ID for cross-referencing.

---

<!-- New ideas are prepended below this line -->

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

### Status: 💡 New

---

