---
name: social-intake
description: Intelligent social media content intake - extract, analyze, and archive external content with knowledge graph integration
version: "1.1.0"
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
- **intake/** = Library (external content archive)
- **IDEAS.md** = Notebook (your own ideas only)
- **engram** = Brain (cognitive connections and insights)

**Rule**: External content itself never goes into IDEAS.md or engram. Only if it triggers a NEW IDEA or creates a VALUABLE CONNECTION does that idea/connection get recorded.

## Trigger Conditions

This skill automatically activates for URLs from:
- Twitter/X (twitter.com, x.com, t.co)
- YouTube (youtube.com, youtu.be)
- Hacker News (news.ycombinator.com)
- Reddit (reddit.com)
- 小红书 (xhslink.com, xiaohongshu.com)
- 微信公众号 (mp.weixin.qq.com)
- GitHub (github.com)

Priority 80 (higher than capture-idea's 50) ensures social media URLs are handled by specialized extraction logic.

## Processing Pipeline

### Step 1: Deduplication Check

**Prevent duplicate processing:**
```bash
# Get URL hash
url_hash=$(skills/social-intake/.venv/bin/python skills/social-intake/intake.py "{url}" --dedup-check)

# Search engram for existing record
engram_recall("url_hash:{url_hash}")
# OR search engram for the exact URL
engram_recall("{url}")
```

**If found:**
- Reply: "⚠️ This URL was already processed on {date}. See {reference}."
- STOP here, don't re-process

**If not found:**
- Continue to extraction

### Step 2: Content Extraction

**Call Python extraction engine:**
```bash
# Extract with JSON output
skills/social-intake/.venv/bin/python skills/social-intake/intake.py "{url}" --json
```

**Expected JSON output:**
```json
{
  "url": "original_url",
  "canonical_url": "normalized_url",
  "platform": "twitter|youtube|hn|reddit|xhs|wechat|github|other",
  "title": "Content title",
  "author": "Author name",
  "date": "Publication date",
  "raw_content": "Extracted text content",
  "extraction_method": "Method used",
  "media_urls": ["image_urls..."],
  "video_url": "video_url_if_applicable",
  "error": "Error message if failed",
  "success": true
}
```

**Handle extraction failure:**
If `success: false`:
```
⚠️ Content extraction failed: {error}
Method attempted: {extraction_method}

Suggestions:
- For 小红书: Try sending a screenshot instead (vision model can read it)
- For paywalled content: Share the key points manually
- For video: I can transcribe if you send the audio
```

### Step 3: Enhanced Extraction (If Applicable)

**For videos (YouTube, Twitter video):**
```bash
# Check if duration is reasonable (< 30 minutes)
# If video_url exists and content needs transcription:

# Try to download subtitles first (faster than transcription)
yt-dlp --write-auto-sub --sub-lang en,zh --skip-download -o /tmp/intake-sub "{video_url}"

# If subtitles exist, read them
if [ -f /tmp/intake-sub.*.vtt ]; then
    cat /tmp/intake-sub.*.vtt
    rm /tmp/intake-sub.*
else
    # No subtitles - extract audio and transcribe
    yt-dlp --extract-audio --audio-format wav -o /tmp/intake-audio.wav "{video_url}"
    stt(/tmp/intake-audio.wav)
    rm /tmp/intake-audio.wav
fi
```

**For images (小红书, visual content):**
```
⚠️ Note: Phase 1 limitation - image OCR requires vision model integration
Current: We extract text descriptions and alt text
Future: Direct image → text via Claude vision API
Workaround: Send screenshots directly to chat for vision analysis
```

### Step 4: Content Analysis & Relationship Discovery

**Generate structured analysis:**

Analyze the extracted content and create:

```
TITLE: [One-line descriptive title]

PLATFORM: {platform} | AUTHOR: {author} | DATE: {date}

SUMMARY:
[2-3 sentence summary of the core content]

KEY POINTS:
- [First key point]
- [Second key point]
- [Third key point]

CATEGORY: [tech/business/product/research/lifestyle/other]

TAGS: [tag1, tag2, tag3, ...]

POTENTIAL VALUE:
[Assessment of relevance to potato's projects and interests]
[Any actionable insights or opportunities identified]
```

**Find knowledge connections:**
```bash
# Search for related existing ideas and projects
engram_recall("key concepts from content")
engram_recall("{tags}")

# Note any connections found:
# - Related IDEAS.md entries
# - Relevant project tasks
# - Similar patterns or themes
```

**Critical judgment - Does this trigger a NEW idea?**

Ask yourself:
1. Does this external content **inspire a new insight** for potato?
2. Does it **add value** to an existing idea/project?
3. Is there an **actionable connection** worth recording?

If NO to all three → Store in intake/ only, DO NOT write to IDEAS.md
If YES to any → Proceed to conditional storage (IDEAS.md + engram idea record)

**Note**: A dedup record is ALWAYS stored in engram (see Layer 1 below), regardless of whether a new idea is triggered. This ensures dedup works for every processed URL.

### Step 5: Storage - Three-Layer Model

#### Layer 1: intake/ Directory (ALWAYS)

**Every processed URL is archived here:**

```bash
# Generate slug from author + post ID or title (max 60 chars, lowercase, hyphens)
# For Twitter: {author}-{tweet_id}  For HN: {item_id}  For others: title slug
slug=$(echo "{author}-{id_or_title}" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | cut -c1-60)
date=$(date +%Y-%m-%d)

# Create platform directory
mkdir -p intake/{platform}

# Write comprehensive archive file
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

{bullet list of key points}

## Potential Value

{relevance assessment to potato's interests and projects}

## Connections Found

{List any related ideas/projects discovered via engram_recall}
{If none: "No existing connections identified."}

---

## Raw Content

{full extracted content - the actual scraped text}

{If video transcript: Include timestamped transcript}
{If images: List image URLs}
EOF
```

**Store dedup record in engram (ALWAYS — ensures dedup check works):**
```bash
engram_store(
    type="factual",
    importance=0.3,
    content="Intake processed: url_hash:{url_hash} | {url} | {platform} | {title} | intake/{platform}/{slug}.md"
)
```

**Update intake index:**
```bash
# Initialize index if it doesn't exist
if [ ! -f intake/index.md ]; then
    echo '# Intake Index' > intake/index.md
    echo '' >> intake/index.md
    echo '| Date | Platform | Title | Tags | Link |' >> intake/index.md
    echo '|------|----------|-------|------|------|' >> intake/index.md
fi
# Append entry
echo "| {date} | {platform} | {title} | {tags} | [link](intake/{platform}/{slug}.md) |" >> intake/index.md
```

#### Layer 2: Daily Log (ALWAYS)

```bash
# Append brief event to today's memory log
cat >> memory/{date}.md << 'EOF'

## Social Intake: {title}
- **Source**: {url} ({platform})
- **Saved to**: intake/{platform}/{slug}.md
- **Key**: {one-line summary}
- **Value**: {brief potential value note}
EOF
```

#### Layer 3: IDEAS.md + Engram (CONDITIONAL ONLY)

**Only write here if:**
- New idea was inspired by this content, OR
- Valuable connection to existing idea/project was discovered

```bash
# If new idea triggered:
if [new_idea_inspired]; then
    # Prepend to IDEAS.md - write YOUR idea, not the external content
    cat >> IDEAS.md << 'EOF'
## IDEA-{YYYYMMDD}-{NN}: {Your Idea Title}
- **Date**: {date}
- **Triggered by**: {url} ({platform})
- **Category**: {category}
- **Tags**: {tags}

### The Idea
{Description of YOUR new idea/insight - NOT summary of external content}

### Why This Matters
{How this connects to your projects/interests}

### Potential Next Steps
{Actionable items if applicable}

### Source Context
External content: {title} by {author}
See: intake/{platform}/{slug}.md

### Status: 💡 New
---
EOF

    # Store the IDEA (not the external content) in engram
    engram_store(
        type="factual",
        importance=0.7,
        content="New idea inspired by {url}: {your_idea_description}. Tags: {tags}"
    )
fi

# If valuable connection found:
if [connection_to_existing_idea]; then
    # Add connection note to existing idea in IDEAS.md
    # Find the relevant IDEA-XXXXXXXX-NN and append:
    # "**Connection ({date})**: {url} provides {what_value}. See intake/{date}/{platform}-{slug}.md"
    
    # Store connection in engram
    engram_store(
        type="factual",
        importance=0.6,
        content="Connection: {url} relates to {existing_idea} - {connection_description}"
    )
fi
```

**Key principle**: IDEAS.md stores YOUR thoughts, not other people's content. The external content lives in intake/, and IDEAS.md just references it.

### Step 6: Reply to User

**Format the response:**

```
📥 **Social Intake: {Title}**

**Platform**: {platform} ({author})
**Summary**: {2-3 sentence summary}

**Key Points**:
{bullet points}

🔗 **Connections**:
{List related ideas/projects found, or "No existing connections found"}

💡 **New Idea**:
{If a new idea was triggered, describe it here}
{Otherwise: "Archived for reference - no immediate insights triggered"}

📁 **Saved to**: intake/{date}/{platform}-{slug}.md
```

**Include actionable suggestions if applicable:**
```
💰 **Potential Value**: {assessment}

🎯 **Suggested Actions**:
- {action item 1}
- {action item 2}
```

## Error Handling & Fallback Chain

**Extraction Priority Chain:**
1. Jina Reader (https://r.jina.ai/{url}) — most reliable cross-platform
2. Platform-specific tool (yt-dlp for video, HN API, etc.)
3. Direct web_fetch
4. Manual intervention request

**Note**: bird CLI has been removed — too unreliable. Jina Reader is the primary method for Twitter/X.

**For videos exceeding 30 minutes:**
- Skip full transcription (too slow/expensive)
- Use metadata + description + available subtitles only
- Note in response: "⏱️ Long video - using metadata and description only. Reply with 'transcribe' if you need the full transcript."

**For failed extractions:**
- Be honest about what failed
- Suggest alternatives (screenshot for images, manual summary for paywalled content)
- Don't fabricate content (GUARD-5)

**Rate limiting / Anti-crawl blocks:**
- If platform blocks scraping, note it clearly
- Suggest user-provided alternatives
- Don't retry aggressively (GUARD-4 - respect robots.txt)

## Guard Rails

1. **GUARD-1** [hard]: Never post, like, comment, or write to any social platform. Read-only operations only.

2. **GUARD-2** [hard]: No login credentials in files. If a platform requires auth, request user to provide session token via secure channel.

3. **GUARD-3** [soft]: Total processing time < 60 seconds from URL to reply. If extraction takes longer, send interim message.

4. **GUARD-4** [soft]: Respect rate limits. Don't hammer failed requests. Max 3 retry attempts with exponential backoff.

5. **GUARD-5** [hard]: Never fabricate content. If extraction fails or is partial, mark it clearly. Partial content marked as `[PARTIAL EXTRACTION]`.

6. **GUARD-6** [soft]: Dedup check before processing. Don't process the same canonical URL twice.

## Dependencies & Installation

**Python packages** (install once):
```bash
cd skills/social-intake
pip install -r requirements.txt
```

**External tools** (install via homebrew):
```bash
# For video/audio extraction
brew install yt-dlp
```

**Test extraction engine:**
```bash
# Test the Python engine directly
skills/social-intake/.venv/bin/python skills/social-intake/intake.py "https://news.ycombinator.com/item?id=12345678" --json
```

## Platform-Specific Notes

**Twitter/X:**
- Jina Reader is the primary extraction method (most reliable)
- bird CLI removed — community tool, frequently broken
- May fail on private accounts
- Video tweets: yt-dlp can extract video/audio

**YouTube:**
- yt-dlp works for most videos
- Prioritize subtitles over audio transcription (faster and more accurate)
- Long videos (>30min): metadata + description only

**小红书:**
- High anti-crawl protection
- Short links (xhslink.com) require redirect resolution
- Images are core content - Phase 1 limitation: text-only extraction
- Suggest screenshot workaround for image-heavy posts

**微信公众号:**
- Article links have expiration (token-based)
- Archive immediately before link expires
- No login required for public articles

**GitHub:**
- Fully public, no auth needed
- README extraction via raw.githubusercontent.com
- API rate limit: 60 req/hour (unauthenticated)

**Reddit:**
- JSON API works without auth for public posts
- old.reddit.com more reliable than www.reddit.com

**Hacker News:**
- Official Firebase API, completely open
- External links may need separate Jina Reader fetch

## Example Usage

**User sends:**
```
https://twitter.com/sama/status/1234567890
```

**Skill processes:**
1. ✓ Dedup check (not seen before)
2. ✓ Extract via bird CLI
3. ✓ Analyze content
4. ✓ Search for connections (finds related IDEA-20260401-03)
5. ✓ Store in intake/twitter/sama-gpt5-thoughts.md
6. ✓ No new idea triggered → skip IDEAS.md
7. ✓ Log to daily memory
8. ✓ Reply with summary + connection note

**User receives:**
```
📥 **Social Intake: Sam Altman on GPT-5 Progress**

**Platform**: twitter (@sama)
**Summary**: Update on GPT-5 training timeline and capabilities. Emphasizes focus on reasoning and multimodal understanding over pure scale.

**Key Points**:
- Training progressing on schedule for Q4 release
- Focus on reasoning depth vs pure parameter count
- Multimodal integration from ground up

🔗 **Connections**:
Related to IDEA-20260401-03 (AI agent reasoning architecture)
This aligns with your idea about cognitive scaffolding for agents

💰 **Potential Value**: High - directly relevant to RustClaw's reasoning layer design

📁 **Saved to**: intake/twitter/sama-gpt5-thoughts.md
```

## Future Enhancements (Out of Scope - Phase 1)

- Vision model integration for direct image OCR
- Automated periodic crawling (proactive intake)
- Full video transcription for long-form content
- Comment thread extraction
- Multi-language translation
- DrissionPage integration for heavy anti-crawl platforms

---

**Design Philosophy**: Capture everything, analyze deeply, connect intelligently, but only create NEW knowledge artifacts when genuine insights emerge. The intake/ directory is your library - reference material. IDEAS.md is your notebook - original thoughts only.
