---
name: social-intake
description: Intelligent social media content intake - extract, analyze, and archive external content with knowledge graph integration
version: "1.3.0"
author: potato
triggers:
  regex:
    - "(twitter\\.com|x\\.com|t\\.co)/"
    - "(youtube\\.com|youtu\\.be)/"
    - "news\\.ycombinator\\.com"
    - "(reddit\\.com|old\\.reddit\\.com)/"
    - "(xhslink\\.com|xiaohongshu\\.com)/"
    - "mp\\.weixin\\.qq\\.com"
    - "github\\.com/"
tags:
  - social-media
  - knowledge-management
  - content-curation
priority: 80
always_load: false
max_body_size: 8192
---
# SKILL: Social Media Intake

> Automated social media content extraction, analysis, and archival system.
> External content goes to intake/ (library), ideas triggered by it go to IDEAS.md (notebook).

## Philosophy

**Three-Layer Storage Model:**
- **intake/** = Library (external content archive with analysis)
- **IDEAS.md** = Notebook (only your own ideas triggered by external content)
- **engram** = Brain (dedup records + idea connections)

**Core Principle**: Archive everything → Analyze deeply → Only valuable insights go to IDEAS.md

## Trigger Conditions

Auto-activates for URLs from:
- Twitter/X, YouTube, Hacker News, Reddit
- 小红书, 微信公众号, GitHub

Priority: 80 (higher than generic capture-idea)

---

## Processing Pipeline

### Step 1: Deduplication Check

```bash
# Get URL hash for deduplication
cd {project_root}
url_hash=$(python3 skills/social-intake/intake.py "{url}" --dedup-check 2>/dev/null | grep -v "^\[")

# Check if already processed using engram_recall
engram_recall("url_hash:${url_hash}")
```

**If found:** Reply "⚠️ Already processed on {date}. See {path}" → STOP  
**If not found:** Continue to extraction

---

### Step 2: Content Extraction (Python Helper)

```bash
# Run extraction engine from project root
cd {project_root}
python3 skills/social-intake/intake.py "{url}" --json
```

**The Python script handles:**
- Short link resolution (t.co, xhslink.com, b23.tv, etc.)
- Platform detection
- Platform-specific extraction with fallback chains
- URL normalization and hash generation

**Extraction Methods by Platform:**

| Platform | Primary Method | Fallback |
|----------|---------------|----------|
| Twitter/X | `npx bird read` | Jina Reader |
| YouTube | `yt-dlp --dump-json` | Jina Reader |
| Hacker News | HN Firebase API | - |
| Reddit | Reddit JSON API | Jina Reader |
| 小红书 (XHS) | Jina Reader | (Phase 1 limited) |
| 微信公众号 | Jina Reader | - |
| GitHub | GitHub API + README | Jina Reader |
| Generic | Jina Reader | - |

**Output Structure:**
```json
{
  "url": "original_url",
  "canonical_url": "normalized_url",
  "platform": "twitter|youtube|hn|...",
  "title": "Content title",
  "author": "Author name",
  "date": "Publication date",
  "raw_content": "Extracted text",
  "extraction_method": "Method used",
  "url_hash": "dedup_hash",
  "success": true,
  "error": null
}
```

**Handle Extraction Failures:**

If `success: false`, provide helpful guidance based on error:
- **小红书 image-heavy content**: "Phase 1 limitation. Send screenshot for vision analysis or share key points manually."
- **Paywalled content**: "Extraction blocked. Share key insights manually if valuable."
- **Rate-limited/blocked**: "Platform blocked scraping. Try again later or use manual capture."

---

### Step 3: Enhanced Extraction (Optional)

**For YouTube videos with subtitles:**
```bash
# Try to download subtitles (faster than full transcription)
yt-dlp --write-auto-sub --sub-lang en,zh --skip-download -o /tmp/intake-sub "{video_url}"

if [ -f /tmp/intake-sub.*.vtt ]; then
    # Convert VTT to readable text
    cat /tmp/intake-sub.*.vtt | grep -v '^WEBVTT' | grep -v '^[0-9][0-9]:[0-9][0-9]' > transcript.txt
fi
```

**For audio/video without subtitles:**
```bash
# Extract audio and transcribe (only if <30min duration)
yt-dlp --extract-audio --audio-format wav -o /tmp/audio.wav "{video_url}"
stt(/tmp/audio.wav)
```

---

### Step 4: Content Analysis & Connection Discovery

**Analyze the content and generate:**

```
TITLE: [Descriptive one-line title]

PLATFORM: {platform} | AUTHOR: {author} | DATE: {date}

CATEGORY: tech | trading | product | marketing | research | business | career | life

SUMMARY:
[2-3 sentence distillation of core content]

KEY POINTS:
- [Most important insight #1]
- [Most important insight #2]
- [Most important insight #3]

TAGS: [specific_tech, topic, person_name, project_name, ...]

RELEVANCE TO POTATO'S WORK:
[Brief assessment: How does this relate to current projects/interests?]
[Is this actionable, inspirational, or purely informational?]

CONNECTIONS FOUND:
[Use engram_recall with key concepts/tags to find related content]
{List: "Related to IDEA-20240115-01: {brief connection explanation}"}
{If none: "No existing connections identified."}

ACTIONABLE INSIGHTS:
[What should we DO with this information?]
[Does it change how we build something? Validate/invalidate a decision? Suggest a tool to try?]

{If genuinely no actions: "No immediate action items - archival only"}
{Otherwise, list 1-3 specific, concrete actions with WHY}
```

**Find connections via engram:**
```bash
engram_recall("{key_concepts_from_content}")
engram_recall("{tags}")
```

---

### Step 5: Storage - Three Layers

#### Layer 1: intake/ Directory (ALWAYS)

Every URL gets archived here, regardless of value:

```bash
# Generate filename slug
slug=$(echo "{author}-{id_or_title_slug}" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | cut -c1-60)
date=$(date +%Y-%m-%d)

mkdir -p intake/{platform}

# Write comprehensive archive
cat > intake/{platform}/{slug}.md << 'EOF'
# {title}

- **URL**: {url}
- **Platform**: {platform}
- **Author**: {author}
- **Date**: {published_date}
- **Fetched**: {timestamp}
- **Category**: {category}
- **Tags**: {tags}
- **URL Hash**: {url_hash}
- **Extraction Method**: {extraction_method}

## Summary

{2-3 sentence summary}

## Key Points

{bullet list of key insights}

## Relevance

{brief assessment of value to potato's work}

## Connections

{list of related ideas/projects from engram_recall}
{or: "No existing connections identified."}

## Actionable Insights

{what to DO with this information}
{concrete actions with context}
{or: "No immediate action items - archival only"}

---

## Raw Content

{full extracted text/transcript}
EOF
```

**Store dedup record in engram (ALWAYS):**
```bash
engram_store(
    type="factual",
    importance=0.3,
    content="Intake processed: url_hash:{url_hash} | {url} | {platform} | {title} | intake/{platform}/{slug}.md"
)
```

---

#### Layer 2: Daily Log (ALWAYS)

```bash
cat >> memory/{YYYY-MM-DD}.md << 'EOF'

## 📥 Social Intake: {title}
- **Source**: {url} ({platform})
- **Saved**: intake/{platform}/{slug}.md
- **Key**: {one-line summary}
EOF
```

---

#### Layer 3: IDEAS.md (CONDITIONAL)

**Only write here if the content INSPIRED A NEW IDEA for potato.**

Rules:
- Don't copy the external content itself
- Write the NEW IDEA or INSIGHT that potato had
- Reference the source in a "Triggered by" field

```bash
# Only if truly valuable/inspiring
if [new_idea_was_triggered]; then
    cat >> IDEAS.md << 'EOF'
## IDEA-{YYYYMMDD}-{NN}: {Your New Idea Title}
- **Date**: {date}
- **Triggered by**: {url} ({platform})
- **Category**: {category}
- **Tags**: {tags}

### The Idea
{Description of YOUR idea - not the external content}

### Why This Matters
{Connection to your projects/goals}

### Next Steps
{Actionable items if applicable}

### Source Context
See: intake/{platform}/{slug}.md for full external content

### Status: 💡 New
---
EOF

    # Store the idea in engram (not the external content)
    engram_store(
        type="factual",
        importance=0.7,
        content="New idea: {your_idea_summary}. Triggered by {url}. Tags: {tags}. See IDEAS.md"
    )
fi
```

**If the content connects to an EXISTING idea:**
```bash
# Find the relevant IDEA-XXXXXXXX-NN in IDEAS.md and append:
# "**Connection ({date})**: {url} provides {explanation}. See intake/{platform}/{slug}.md"

# Store connection in engram
engram_store(
    type="connection",
    importance=0.6,
    content="{url} connects to IDEA-{id}: {connection_explanation}"
)
```

---

### Step 6: User Response

```
✅ 已保存: {title}

📝 {one-line summary}

🔖 Category: {category}
🏷️  Tags: {tags}

{If connections found:}
🔗 Connections: {brief list}

{If actionable insights:}
💡 Actionable: {brief list}

📂 Saved to: intake/{platform}/{slug}.md
{If added to IDEAS.md: "💡 New idea added to IDEAS.md"}
```

---

## Tools Required

**External commands:**
```bash
curl                    # HTTP requests
python3                 # Extraction script
npx bird               # Twitter extraction (optional, falls back to Jina)
yt-dlp                 # YouTube extraction (required)
jq                     # JSON parsing
```

**Python dependencies:**
```
requests>=2.31.0
beautifulsoup4>=4.12.0
```

**RustClaw functions:**
```
engram_recall(query)   # Search knowledge graph
engram_store(...)      # Store facts/connections
stt(audio_path)        # Speech-to-text (optional)
```

---

## Phase 1 Limitations

**Current (Phase 1):**
- ✅ Text content from all major platforms
- ✅ YouTube metadata + subtitles
- ✅ Deduplication via URL hashing
- ✅ Knowledge graph integration
- ⚠️ 小红书 limited to text (image-heavy posts incomplete)

**Future (Phase 2):**
- 🔮 Claude vision API for image extraction (小红书, Twitter images)
- 🔮 Automated action item tracking dashboard
- 🔮 Cross-project meta-graph for insight patterns

---

## Examples

### Example 1: Twitter Thread
```
Input: https://twitter.com/sama/status/123456
→ Extract with bird CLI
→ Store to intake/twitter/sama-123456.md
→ Analyze: "AI safety governance insights"
→ Find connection: Related to IDEA-20240115-01 (RustClaw safety)
→ Add connection note to IDEAS.md
→ Reply: "✅ Saved + connected to existing idea"
```

### Example 2: YouTube Tutorial
```
Input: https://youtube.com/watch?v=xyz
→ Extract with yt-dlp (metadata + subtitles)
→ Store to intake/youtube/channel-title-slug.md
→ Analyze: "Rust async patterns we should adopt"
→ Create NEW idea in IDEAS.md: "Apply async pattern X to gid-harness"
→ Reply: "✅ Saved + new idea generated"
```

### Example 3: Low-Value Link
```
Input: https://example.com/generic-news
→ Extract with Jina Reader
→ Store to intake/other/generic-news.md
→ Analyze: "No relevance to current projects"
→ No IDEAS.md entry (just archive)
→ Reply: "✅ Saved to archive"
```

---

## File Structure

```
intake/
├── index.md                    # Searchable index
├── twitter/
│   ├── sama-123456.md
│   └── elonmusk-789.md
├── youtube/
│   └── channel-video-title.md
├── hn/
│   └── item-12345678.md
├── reddit/
├── github/
├── xhs/
├── wechat/
└── other/

IDEAS.md                        # Only ideas triggered by intake
memory/YYYY-MM-DD.md            # Daily activity log
```

---

## Success Criteria

**Automatic activation**: URL detected → skill triggers without user command  
**Deduplication**: Same URL sent twice → second time exits early with reference  
**Rich extraction**: Platform-specific methods provide better data than generic scraping  
**Deep analysis**: Content is summarized, tagged, and connected to existing knowledge  
**Selective curation**: Only valuable insights make it to IDEAS.md (not everything)  
**Fast response**: Entire pipeline completes in <30 seconds for text content

