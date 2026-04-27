---
name: write-doc
description: Incremental write pattern for long documents (design, requirements, specs, postmortems, RFCs, ADRs, any structured markdown)
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "write design"
    - "写设计"
    - "write doc"
    - "写文档"
    - "write spec"
    - "写spec"
    - "write requirements"
    - "写需求"
    - "RFC"
    - "ADR"
    - "postmortem"
    - "forensic"
    - "design.md"
    - "DESIGN.md"
    - "requirements.md"
    - "spec.md"
    - "review.md"
  keywords:
    - "design doc"
    - "design document"
    - "spec doc"
    - "requirements doc"
    - "long doc"
    - "long document"
    - "incremental"
tags:
  - documentation
  - writing
  - process
priority: 90
always_load: false
---

# SKILL: Incremental Document Writing

> Whenever you are asked to write a structured document — design, requirements, spec, postmortem, RFC, ADR, review, long markdown — write it incrementally. Skeleton first, then fill sections one or two at a time. This applies to **both main agent and any sub-agent** doing the writing.

## When this skill applies

Activate this pattern for any of:

- Design documents (`design.md`, `DESIGN.md`, `<feature>-design.md`)
- Requirements documents (`requirements*.md`)
- Specifications (`spec.md`, `*-spec.md`)
- RFCs / ADRs (`RFC-*.md`, `ADR-*.md`)
- Postmortems, forensics, incident reviews
- Long-form review documents (≥3 sections)
- Any markdown file with multiple `##` sections **expected to exceed 200 lines**
- Any document where you cannot confidently predict it will stay under 200 lines

**Default assumption:** if you're unsure how long it will be, treat it as long. The cost of using the incremental pattern on a short doc is one extra tool call. The cost of NOT using it on a long doc is truncation, context exhaustion, or a half-baked file.

## The pattern

### Step 1 — Tell the user (one short line)

Before the first `write_file`, say something like:
- "I'll write the skeleton then fill sections incrementally."
- "Going incremental — skeleton first, then §1, §2, …"

This sets expectations: the user knows multiple tool calls are coming and won't expect one big reveal.

### Step 2 — Write the skeleton

One `write_file` call. The file should contain:

- Frontmatter / metadata header (issue ID, status, date, related links — whatever the doc convention is)
- Document title and a 2–6 line **lede** that names the problem, the proposed solution in one sentence, and the key bundled issues if any
- All `##` section headings the doc will have
- Each section body is `(TBD)` or a one-line stub describing what it will contain

Total skeleton size target: **30–80 lines.** If the skeleton itself exceeds 100 lines, you're putting too much content in it — pull the prose down into individual section fills.

### Step 3 — Fill sections, one or two at a time

For each `edit_file` call:

- Find the `(TBD)` stub (or pair of adjacent stubs) you're replacing
- Replace with the real content — typically 50–200 lines per call
- Two adjacent sections per call is fine if they're tightly related (e.g. "Goals" + "Non-Goals")
- Three or more sections per call is too many — split

### Step 4 — Verify at the end

After all sections are filled, run a quick check:

```bash
wc -l <file>           # sanity-check final length
grep "(TBD)" <file>    # should be empty
grep -c "^## " <file>  # confirm section count matches what you intended
```

If TBDs remain, fill them. If section count is off, reconcile.

## Hard rules

- **Never write 500+ lines in a single tool call.** If your draft exceeds that, split.
- **Never skip the skeleton.** Even if you "know" the structure, write the skeleton first. It's a 10-second cost that prevents 10-minute disasters.
- **Never batch all sections into one giant edit.** One or two sections per `edit_file` call. Three is the absolute limit and only for very short sections.
- **Don't apologize or re-explain between section fills** — just call the tools. The user can see progress from your tool activity.

## Why

- Large single-write calls are the #1 cause of output truncation in long-context sessions.
- If your context exhausts mid-write, the skeleton-first pattern lets you (or a recovery sub-agent) resume from the same file. Single-shot writes leave a half-baked file with no structure to resume from.
- Forces you to think about structure before prose. Better docs.
- Surfaces structural problems early — "wait, §7 doesn't make sense before §5" when fixing it costs a 30-second skeleton edit, not a 600-line rewrite.

## Sub-agent rule

If you delegate document writing to a sub-agent (`spawn_specialist`), this skill is **automatically loaded** for the sub-agent (via trigger keywords). But also include in your task prompt:

> "Write the document incrementally — skeleton first, then fill sections one or two at a time per edit_file call. See skills/write-doc/SKILL.md."

This is belt-and-suspenders — both the skill trigger and the explicit instruction. Sub-agents are more likely to skip the skeleton step than the main agent.

## Anti-pattern examples

❌ **Bad:** `write_file` with the entire 900-line design.md in one go. Half the time it truncates; the other half it works but you can't fix anything without rewriting the whole file.

❌ **Bad:** Skipping skeleton because "I know the structure." You don't, until you write it down. The skeleton is the structure, externalized.

❌ **Bad:** Writing skeleton with sections that already contain 200 lines of "stub" content. That's not a skeleton; that's the doc with extra steps.

✅ **Good:** `write_file` with 50-line skeleton (title, lede, 12 `##` headings each followed by `(TBD)`). Then 6 `edit_file` calls each filling 2 sections. Total: 7 tool calls, no truncation, easy to resume if context strains.

## Reference

This skill exists because: 2026-04-26 session, RustClaw wrote a ~900-line design.md (gid-rs ISS-052/design.md) and only switched to incremental mode after the user pointed out context was straining. The incremental pattern was already in AGENTS.md but not consistently applied. Fixing both: AGENTS.md upgraded ("MANDATORY"), this skill created (always-triggers on doc-writing keywords).
