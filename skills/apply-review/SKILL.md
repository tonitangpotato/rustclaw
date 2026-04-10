---
name: apply-review
description: Apply approved review findings to a document — reads the review file and makes targeted edits
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "apply review"
    - "apply findings"
    - "apply FINDING"
    - "应用修改"
  regex:
    - "(?i)apply.*(finding|review)"
    - "(?i)FINDING-\\d+"
tags:
  - development
  - quality
priority: 55
always_load: false
max_body_size: 4096
---
# SKILL: Apply Review Findings

> Reads a review file + the original document, applies only the approved changes. Precise, targeted edits.

## Purpose

After a review skill (review-design, review-requirements) writes findings to `.gid/features/{feature}/reviews/` or `.gid/issues/{ISS-NNN}/reviews/`, the human approves specific findings. This skill applies only the approved ones.

## Input

The user message will contain:
- Which findings to apply (e.g., "apply FINDING-1,3,5" or "apply all")
- The review file path (e.g., `.gid/features/{feature}/reviews/design-r1.md`)
- The target document path (or infer from the review file header)

## Process

1. **Read the review file** from `.gid/features/{feature}/reviews/{type}-r{N}.md` (or `.gid/issues/{ISS-NNN}/reviews/`)
2. **Read the target document** completely — you need full context to make correct edits
3. **For each approved finding**, apply the suggested fix:
   - Use `Edit` tool for targeted changes (preferred — preserves surrounding context)
   - Use `Write` only if a section needs complete replacement
   - Preserve document structure, formatting, and numbering
4. **After all changes**, re-read the modified sections to verify consistency
5. **Report** what was changed: finding ID, section affected, brief description of change

## Rules

- **Read the FULL target document before making any changes.** Context matters — a change in §3 might affect §7.
- **Apply ONLY the approved findings.** Do not make additional improvements you notice.
- **Use Edit tool, not Write**, whenever possible. Surgical edits preserve formatting and reduce diff noise.
- **Maintain numbering consistency.** If you add/remove a GOAL, renumber subsequent ones. Update cross-references.
- **Preserve existing content.** If a finding says "make GOAL-12 more specific", rewrite GOAL-12 but don't touch GOAL-11 or GOAL-13.
- **After applying all changes, update the review file**: mark applied findings as `✅ Applied` and note the change made.
- **If a suggested fix is ambiguous or would break consistency**, report it instead of guessing. Say "FINDING-X: suggested fix conflicts with GOAL-Y, skipping — needs human decision."

## Output Format

```markdown
## Applied Changes

### FINDING-1 ✅
- Section: §3 GOAL-12
- Change: Made acceptance criteria measurable ("response time < 200ms p95")
- Edit: Replaced vague "fast response" with specific latency target

### FINDING-3 ✅
- Section: §5 GUARD-3
- Change: Resolved contradiction with GOAL-7 by adding exception clause

### FINDING-5 ⚠️ Skipped
- Reason: Suggested fix conflicts with GUARD-2, needs human decision

### Summary
- Applied: 2/3
- Skipped: 1/3 (needs human decision)
```
