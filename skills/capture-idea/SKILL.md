---
name: capture-idea
description: Capture and structure incoming ideas, links, and media into discussion documents
version: "1.0.0"
author: potato
triggers:
  patterns:
    - "idea:"
    - "想法:"
    - "intake"
    - "记录一下"
    - "voice message"
    - "语音"
  regex:
    - "https?://"
tags:
  - productivity
  - knowledge-management
priority: 50
always_load: false
max_body_size: 4096
---
# SKILL: Idea Intake Pipeline

> Automatically process incoming ideas, links, and media into structured knowledge.

## Trigger Conditions

This skill activates when potato sends:
- A URL/link (auto-detect `http://` or `https://`)
- A voice/audio message describing an idea
- An explicit "idea:" or "想法:" prefix
- Or says "intake this", "记录一下", etc.

## Pipeline Steps

### Step 1: Content Extraction

**If URL:**
```
→ web_fetch(url) to get page content
→ If video URL (youtube, bilibili, twitter video):
  → exec: yt-dlp --extract-audio --audio-format wav -o /tmp/idea-audio.wav "{url}"
  → stt(/tmp/idea-audio.wav) to transcribe
  → exec: rm /tmp/idea-audio.wav
→ If paywalled/blocked: note it, use whatever we got
```

**If voice message:**
```
→ stt(audio_path) to transcribe
→ Use transcript as raw content
```

**If text:**
```
→ Use raw text directly
```

### Step 2: Analysis & Summary

Generate a structured analysis:
```
- **Title**: One-line descriptive title
- **Source**: URL or "voice message" or "text"
- **Domain**: Primary domain (🔧tech / 💰trading / 📦product / 📈marketing / 🧠research / 💡business / 🎯career / 🏠life) [+ secondary]
- **Summary**: 2-3 sentence summary of the core idea
- **Key Points**: Bullet list of actionable insights
- **Category**: tech/business/product/research/lifestyle/other
- **Potential Value**: How this could generate revenue or strategic advantage
- **Effort Estimate**: Low/Medium/High to implement or explore
- **Tags**: Relevant keywords for future search
```

### Step 3: Find Connections

Search for related existing ideas and projects:
```
→ engram_recall("key concepts from the idea") to find related memories
→ Check IDEAS.md for similar past ideas
→ Check GID graph for related project tasks
→ Note any connections found
```

### Step 3.5: Back-Reference (反向更新)

If connections were found in Step 3, go back and update the **existing** entries:

**If related to an existing IDEA:**
```
→ Find the existing IDEA entry in IDEAS.md
→ Under its "### Connections" section, append a line:
  "- Related: {new-entry-ID} ({new title}) — {brief reason for connection}"
→ If no Connections section exists, create one
```

**If related to an existing project:**
```
→ Find the project's main doc (DESIGN.md, README.md, or requirements.md)
→ If the doc has a "References" or "Related" section, append a reference line
→ If no such section, add one at the bottom:
  "## References\n- {new-entry-ID}: {title} — {brief relevance}"
```

**Rules:**
- Only add back-references when the connection is meaningful (not every vague keyword match)
- Keep back-reference lines brief — one line each, just enough to find the new entry
- Do NOT modify the existing entry's content/analysis, only append to Connections/References
- Do NOT create Hebbian links artificially — let natural recall handle that

### Step 4: Store

1. **IDEAS.md** — Prepend structured entry:
```markdown
## IDEA-{YYYYMMDD}-{NN}: {Title}
- **Date**: {date}
- **Source**: {url or description}
- **Category**: {category}
- **Tags**: {tags}
- **Effort**: {Low/Medium/High}

### Summary
{summary}

### Key Points
{bullet points}

### Potential Value
{value assessment}

### Action Items
- [ ] {Concrete action} — {why} [{P0/P1/P2}]

### Connections
{related ideas/projects found}

### Status: 💡 New
---
```

2. **Engram** — Store as factual memory:
```
→ engram_store(type=factual, importance=0.6, content="Idea: {title} - {summary} - Tags: {tags}")
```

3. **Daily Log** — Append brief entry to memory/YYYY-MM-DD.md:
```
## Idea Captured: {title}
- Source: {url}
- See IDEAS.md IDEA-{id}
```

4. **GID** (if actionable) — Create a task node if the idea is concrete enough:
```
→ gid_add_task(id="idea-{slug}", title="{title}", tags=["idea", "{category}"])
```

### Step 5: Report Back + Proactive Association

Reply to potato with a concise summary, and **actively surface connections**:
```
📥 **Idea Captured: {Title}**
{1-2 sentence summary}
💰 Value: {potential value assessment}
📝 Saved to IDEAS.md as IDEA-{id}

🎯 **Action Items:**
- [ ] {Concrete action} — {why} [{P0/P1/P2}]
- [ ] {Concrete action} — {why} [{P0/P1/P2}]

🔗 **这个让我想到：**
- {Related idea/project} — {WHY it's connected, not just "related"}
- {Another connection} — {concrete link: shared tech, same problem space, builds on each other, etc.}
```

**Association rules:**
- Don't just list keyword matches — explain the actual relationship
- Be specific: "这个的 Progressive Disclosure 可以解决我们 SKM 的 context 占用问题" > "Related to SKM"
- If you found something genuinely interesting, say so — spark a conversation, don't just log
- If no meaningful connections found, don't force it — just say "暂时没发现跟已有项目的明显关联"
- This is a conversation starter, not a report. Potato may want to discuss the connections.

**If back-references were added in Step 3.5, explicitly tell potato:**
```
🔗 **关联更新：**
- 更新了 {IDEA-XXXXXXXX-NN} 的 Connections（{brief reason}）
- 更新了 {project name} 的 References（{brief reason}）
```
This ensures potato is reminded of related past ideas/projects they may have forgotten about. The value of finding connections is zero if we don't surface them.

## ID Format

`IDEA-{YYYYMMDD}-{NN}` where NN is sequential for that day (01, 02, 03...).

## Notes

- Don't over-analyze. Speed > perfection. Capture first, refine later.
- If the idea is clearly just a link share (no idea component), still summarize but mark as "reference" not "idea"
- If potato adds commentary with the link, incorporate that as the "human insight" angle
- Use 中英混用 naturally in summaries if the source is Chinese
- For video URLs, try yt-dlp first; if it fails, fall back to web_fetch for the page description
