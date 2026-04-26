#!/usr/bin/env python3
"""
One-shot migration: prepend v5.0.0 YAML frontmatter to every ISS-NNN/issue.md.
Idempotent — skips files that already have frontmatter.
"""
from pathlib import Path

ROOT = Path("/Users/potato/rustclaw/.gid/issues")

# Authoritative metadata for all known issues.
# Schema: id -> dict(title, status, priority, created, closed?, severity?, component?, related?, depends_on?, supersedes?, superseded_by?)
ISSUES = {
    # ---------- archive directories (need stub issue.md) ----------
    "ISS-002": dict(
        title="LSP Client for gid-core code_graph",
        status="superseded",
        priority="P3",
        created="2026-03-01",
        closed="2026-04-25",
        component="gid-core/code_graph",
        note="See STATUS.md for full superseded rationale. Phase 1 design docs preserved in this directory.",
    ),
    "ISS-004": dict(
        title="Code Graph LSP Refinement Pipeline Refactoring",
        status="closed",
        priority="P1",
        created="2024-12-01",
        closed="2024-12-15",
        component="gid-core/code_graph",
        note="See REFACTORING_COMPLETE.md for full record.",
    ),
    "ISS-006": dict(
        title="Incremental Updates for gid extract",
        status="closed",
        priority="P1",
        created="2026-04-06",
        closed="2026-04-15",
        component="gid-core/code_graph",
        note="See ISS-006-IMPLEMENTATION-SUMMARY.md and DESIGN-REVIEW-SUMMARY.md for full record.",
    ),
    # ---------- has issue.md, just needs frontmatter ----------
    "ISS-007": dict(
        title="Engram Recall Quality Fixes",
        status="blocked",
        priority="P2",
        created="2026-04-19",
        component="src/memory.rs",
        depends_on=["engram:ISS-032"],
        note="2 of 3 bugs fixed in rustclaw; bug 3 migrated to engram ISS-032.",
    ),
    "ISS-008": dict(
        title="Telegram Disconnect / Reconnect Loop",
        status="open",
        priority="P2",
        created="2026-04-19",
        component="src/channels/telegram.rs",
    ),
    "ISS-009": dict(
        title="Persona System — Multi-Instance Identity + Engram Namespace",
        status="open",
        priority="P2",
        severity="medium",
        created="2026-04-14",
        component="src/config.rs, src/workspace.rs, src/memory.rs",
    ),
    "ISS-010": dict(
        title="Sub-agent Delegation Hard Rules",
        status="closed",
        priority="P1",
        created="2026-04-15",
        closed="2026-04-17",
        component="AGENTS.md, src/orchestrator.rs",
    ),
    "ISS-011": dict(
        title="Streaming Telegram Output Edge Cases",
        status="closed",
        priority="P1",
        created="2026-04-15",
        closed="2026-04-17",
        component="src/channels/telegram.rs",
    ),
    "ISS-012": dict(
        title="Confidence Weighting in Code Graph Extraction",
        status="closed",
        priority="P2",
        created="2026-04-15",
        closed="2026-04-17",
        component="gid-core/code_graph",
    ),
    "ISS-013": dict(
        title="gid_extract tool missing incremental extract + LSP refinement",
        status="closed",
        priority="P1",
        created="2026-04-17",
        closed="2026-04-18",
        component="src/tools.rs (GidExtractTool)",
        related=["ISS-012"],
    ),
    "ISS-014": dict(
        title="Heartbeat Scope & Daily Log Routing",
        status="closed",
        priority="P1",
        created="2026-04-20",
        closed="2026-04-21",
        component="src/heartbeat.rs",
    ),
    "ISS-016": dict(
        title="Engram Auto-Recall Hook Integration",
        status="closed",
        priority="P1",
        created="2026-04-20",
        closed="2026-04-21",
        component="src/memory.rs, src/hooks.rs",
    ),
    "ISS-019": dict(
        title="/ritual cancel does not persist cancellation to state file",
        status="closed",
        priority="P2",
        created="2026-04-22",
        closed="2026-04-25",
        component="src/ritual.rs, .gid/runtime/rituals/",
    ),
    "ISS-020": dict(
        title="Project path discovery friction in cross-project tools",
        status="open",
        priority="P2",
        created="2026-04-23",
        component="src/tools.rs (gid_* tools)",
    ),
    "ISS-021": dict(
        title="Message context side-channel — Envelope refactor",
        status="in_progress",
        priority="P1",
        created="2026-04-22",
        component="src/context.rs, src/memory.rs, engramai",
        note="Phase 1 done (P_before=0.767 baseline). Phases 2-5 ahead.",
    ),
    "ISS-022": dict(
        title="Migrate start_ritual tool to WorkUnit-based identification",
        status="closed",
        priority="P2",
        created="2026-04-23",
        closed="2026-04-25",
        component="src/tools.rs (start_ritual)",
        related=["gid-rs:ISS-029"],
    ),
    "ISS-023": dict(
        title="clawd/projects path debt — hardcoded paths across configs",
        status="open",
        priority="P3",
        created="2026-04-23",
        component="rustclaw.yaml, MEMORY.md, multiple configs",
    ),
    "ISS-024": dict(
        title="gid_* tools need graph_path override parameter",
        status="closed",
        priority="P2",
        created="2026-04-23",
        closed="2026-04-25",
        component="src/tools.rs (all gid_* tools)",
    ),
    "ISS-025": dict(
        title="Ritual implement phase burns tokens with no output",
        status="open",
        priority="P1",
        created="2026-04-24",
        component="src/ritual.rs (implement phase)",
        related=["ISS-027", "ISS-029"],
    ),
    "ISS-026": dict(
        title="start_ritual tool misreports progress (duplicate of ISS-048)",
        status="closed",
        priority="P2",
        created="2026-04-24",
        closed="2026-04-26",
        component="src/tools.rs (start_ritual)",
        superseded_by="ISS-048",
        note="Merged into ISS-048 — same root cause, ISS-048 has the canonical fix.",
    ),
    "ISS-027": dict(
        title="Ritual observer needs context injection",
        status="open",
        priority="P2",
        created="2026-04-26",
        component="src/ritual.rs",
        related=["ISS-025"],
    ),
    "ISS-028": dict(
        title="Duplicate rituals can launch for same work unit",
        status="open",
        priority="P2",
        created="2026-04-26",
        component="src/ritual.rs, .gid/runtime/rituals/",
        related=["ISS-030"],
    ),
    "ISS-029": dict(
        title="Ritual state liveness signal — detect stuck/dead rituals",
        status="open",
        priority="P2",
        created="2026-04-26",
        component="src/ritual.rs",
        related=["ISS-025"],
    ),
    "ISS-030": dict(
        title="Multi-daemon shared workspace ritual race condition",
        status="open",
        priority="P3",
        created="2026-04-26",
        component="src/ritual.rs, src/workspace.rs",
        related=["ISS-028"],
    ),
    "ISS-048": dict(
        title="start_ritual tool misreports 'initializing' state as failure",
        status="open",
        priority="P2",
        created="2026-04-26",
        component="src/tools.rs (start_ritual)",
        supersedes=["ISS-026"],
    ),
    "ISS-049": dict(
        title="Skills directory has no hot-reload",
        status="open",
        priority="P3",
        created="2026-04-26",
        component="src/skills.rs",
    ),
}

def fmt_list(items):
    return "[" + ", ".join(f'"{x}"' for x in items) + "]"

def make_frontmatter(iid, m):
    lines = ["---", f'id: "{iid}"', f'title: "{m["title"]}"',
             f'status: {m["status"]}', f'priority: {m["priority"]}',
             f'created: {m["created"]}']
    if "closed" in m:
        lines.append(f'closed: {m["closed"]}')
    if "severity" in m:
        lines.append(f'severity: {m["severity"]}')
    if "component" in m:
        lines.append(f'component: "{m["component"]}"')
    if "depends_on" in m:
        lines.append(f'depends_on: {fmt_list(m["depends_on"])}')
    if "related" in m:
        lines.append(f'related: {fmt_list(m["related"])}')
    if "supersedes" in m:
        lines.append(f'supersedes: {fmt_list(m["supersedes"])}')
    if "superseded_by" in m:
        lines.append(f'superseded_by: "{m["superseded_by"]}"')
    if "note" in m:
        # YAML single-line note (escape quotes)
        n = m["note"].replace('"', '\\"')
        lines.append(f'note: "{n}"')
    lines.append("---")
    lines.append("")
    return "\n".join(lines)

# ---- Process each issue ----
for iid in sorted(ISSUES.keys()):
    d = ROOT / iid
    if not d.is_dir():
        print(f"SKIP {iid}: directory missing")
        continue
    issue_md = d / "issue.md"
    meta = ISSUES[iid]
    fm = make_frontmatter(iid, meta)

    if issue_md.exists():
        body = issue_md.read_text()
        if body.lstrip().startswith("---"):
            print(f"SKIP {iid}: already has frontmatter")
            continue
        new = fm + body
        issue_md.write_text(new)
        print(f"FRONTMATTER {iid}")
    else:
        # Stub for archive-only directories
        contents = [f"# {iid}: {meta['title']}", ""]
        if "note" in meta:
            contents.append(meta["note"])
            contents.append("")
        contents.append("See sibling files in this directory for full record.")
        contents.append("")
        existing = sorted([p.name for p in d.iterdir() if p.is_file()])
        if existing:
            contents.append("## Archived files in this directory")
            for f in existing:
                contents.append(f"- `{f}`")
        body = "\n".join(contents) + "\n"
        issue_md.write_text(fm + body)
        print(f"STUB {iid}")

print("\nDone.")
