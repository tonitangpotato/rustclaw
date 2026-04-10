---
name: project-init
description: Initialize a new project with standard .gid/ directory structure
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "init project"
    - "new project"
    - "create project"
    - "initialize project"
    - "新建项目"
    - "初始化项目"
    - "开个新项目"
  keywords:
    - "project init"
    - "gid init"
    - "project setup"
    - "scaffold project"
tags:
  - project-management
  - setup
priority: 60
always_load: false
max_body_size: 4096
---
# SKILL: Project Initialization

> Set up a new project with the standard `.gid/` directory structure for GID-driven development.

## Philosophy

Every project managed by GID follows a consistent directory structure. `.gid/` is the project's brain — it holds the dependency graph, feature documentation, and issue tracking. Consistency across projects means skills, tools, and workflows all work the same way everywhere.

## When to Use

- Starting a brand new project
- Onboarding an existing codebase into GID workflow
- When potato says "init project", "新建项目", etc.

## Standard `.gid/` Structure

```
{project_root}/
├── .gid/
│   ├── graph.yml              ← Dependency graph (created by `gid init`)
│   ├── config.yml             ← GID config (optional, for ritual gating etc.)
│   ├── requirements.md        ← Master requirements (GUARDs + feature index)
│   ├── design.md              ← Master design (architecture overview)
│   ├── features/              ← Feature-level documentation
│   │   ├── {feature-name}/
│   │   │   ├── requirements.md
│   │   │   └── design.md
│   │   └── ...
│   ├── issues/                ← Issue tracking
│   │   ├── ISSUES.md          ← Issue index (all issues listed here)
│   │   ├── ISS-001/           ← Per-issue workspace (requirements, design, etc.)
│   │   └── ...
│   ├── reviews/               ← Review findings (auto-generated)
│   │   └── {doc-name}-review.md
│   └── rituals/               ← Ritual state files (auto-generated)
│       └── {id}.json
├── src/                       ← Source code
├── tests/                     ← Tests
└── ...
```

## Pipeline

### Step 1: Determine Project Location

Ask potato (if not obvious from context):
- **Project name**: What's the project called?
- **Location**: Where should it live?
  - New project under RustClaw workspace: `/Users/potato/rustclaw/projects/{name}/`
  - New project under OpenClaw workspace: `/Users/potato/clawd/projects/{name}/`
  - Custom location: wherever potato specifies
  - Existing codebase: use its current location

**⚠️ Never create a project root inside another project's `.gid/`.**

### Step 2: Create Project Root (if new)

```bash
mkdir -p {project_root}
cd {project_root}
```

For Rust projects:
```bash
cargo init {project_root}
```

For other languages, just create the directory.

### Step 3: Initialize GID Graph

```bash
cd {project_root}
gid init
```

This creates `.gid/graph.yml` with project metadata. **`gid init` only creates the graph file** — the rest of the structure is our responsibility.

### Step 4: Create Standard Directories

```bash
cd {project_root}
mkdir -p .gid/features
mkdir -p .gid/issues
mkdir -p .gid/reviews
mkdir -p .gid/rituals
```

### Step 5: Create ISSUES.md Template

```bash
cat > .gid/issues/ISSUES.md << 'EOF'
# Issues: {Project Name}

> 项目使用过程中发现的 bug、改进点和待办事项。
> 格式: ISS-{NNN} [{type}] [{priority}] [{status}]

---

*(No issues yet)*
EOF
```

### Step 6: Git Setup (if needed)

If the project doesn't have `.git/`:
```bash
cd {project_root}
git init
```

Add `.gid/rituals/` and `.gid/reviews/` to `.gitignore` (auto-generated, not worth tracking):
```bash
echo ".gid/rituals/" >> .gitignore
echo ".gid/reviews/" >> .gitignore
```

Keep everything else in `.gid/` tracked:
- `graph.yml` — project structure
- `features/` — requirements & design docs
- `issues/` — issue tracking
- `config.yml` — GID config

### Step 7: Record in Memory

1. **Daily log** — note the new project
2. **Engram** — store for future recall:
   ```
   engram_store(type=factual, importance=0.5,
     content="New project initialized: {name} at {path}. Language: {lang}. Purpose: {brief}")
   ```

### Step 8: Report

```
✅ Project initialized: {name}

📂 Location: {project_root}/
📊 Graph: .gid/graph.yml
📋 Issues: .gid/issues/ISSUES.md
📁 Features: .gid/features/ (empty, ready for requirements & design)

Next steps:
1. Write requirements → .gid/requirements.md (or .gid/features/{feat}/requirements.md)
2. Write design → .gid/design.md (or .gid/features/{feat}/design.md)
3. Generate graph → gid_design
4. Start building → gid_tasks
```

## Feature Documentation Convention

When adding features to an initialized project:

```
.gid/features/{feature-name}/
├── requirements.md    ← WHAT this feature does (GOALs)
└── design.md          ← HOW this feature is built (components, data flow)
```

**Naming rules:**
- Feature directory names: lowercase, hyphen-separated (e.g., `core-engine`, `data-loading`)
- One feature = one directory
- Each feature should have ≤15 GOALs in requirements and ≤8 components in design
- If a feature exceeds these limits, split into sub-features

**Master documents** (project-level) go directly in `.gid/`:
- `.gid/requirements.md` — master requirements (GUARDs, feature index, cross-cutting concerns)
- `.gid/design.md` — master design (architecture overview, cross-cutting patterns)

## Existing Project Onboarding

For codebases that already exist but don't have `.gid/`:

1. Run `gid init` to create graph
2. Create the directory structure (Step 4-5 above)
3. Run `gid_extract` on `src/` to build code-level graph nodes
4. Optionally: write retroactive requirements/design docs based on existing code

## Rules

- **`gid init` is just the graph.** Always follow up with directory creation.
- **Don't skip ISSUES.md.** Even if there are no issues yet, create the template.
- **features/ starts empty.** Don't create placeholder feature dirs — add them when actual features are being designed.
- **One project, one `.gid/`.** Never nest `.gid/` directories.
- **Track `.gid/` in git** (except rituals/ and reviews/).
