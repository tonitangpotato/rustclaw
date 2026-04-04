# Social Media Intake System - Implementation Summary

**Status**: ✅ Complete  
**Date**: 2026-04-03  
**Implementation Type**: RustClaw Skill (Python Core + LLM Orchestration)

## Overview

The Social Media Intake system provides intelligent, automated content extraction, analysis, and archival for social media URLs. When potato shares a URL in Telegram, RustClaw automatically:

1. **Detects** the platform (Twitter, YouTube, HN, Reddit, 小红书, WeChat, GitHub)
2. **Extracts** content using platform-specific strategies
3. **Analyzes** and generates structured summaries
4. **Discovers** connections to existing ideas in the knowledge graph
5. **Archives** in intake/ directory (library for external content)
6. **Records** insights in IDEAS.md/engram only when new ideas emerge

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                    RustClaw Agent                         │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐ │
│  │         SKILL.md (LLM Orchestration Layer)          │ │
│  │  - Pipeline coordination                            │ │
│  │  - Analysis & connection discovery                  │ │
│  │  - Storage logic (3-layer model)                    │ │
│  └───────────┬─────────────────────────────────────────┘ │
│              │                                            │
│              ▼                                            │
│  ┌─────────────────────────────────────────────────────┐ │
│  │       intake.py (Python Core Engine)                │ │
│  │  - URL normalization & deduplication                │ │
│  │  - Platform detection (7+ platforms)                │ │
│  │  - Content extraction (platform-specific)           │ │
│  │  - Fallback chain (tool → Jina → error)           │ │
│  └─────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

## Components

### 1. intake.py - Core Extraction Engine (~650 lines)

**Responsibilities:**
- URL normalization (remove tracking params)
- Platform detection via regex patterns
- Content extraction per platform
- Error handling and fallback chains

**Platform Extractors:**

| Platform | Primary Tool | Fallback | Features |
|----------|-------------|----------|----------|
| Twitter/X | bird CLI | Jina Reader | Text + video extraction |
| YouTube | yt-dlp | Jina Reader | Metadata + subtitles + transcription |
| Hacker News | Firebase API | - | Post + comments + external links |
| Reddit | JSON API | Jina Reader | Post + metadata |
| 小红书 | Jina Reader | - | Short link resolution + text |
| WeChat | Direct fetch | Jina Reader | Article + author extraction |
| GitHub | GitHub API | Jina Reader | README + repo metadata |
| Other | Jina Reader | - | Universal fallback |

**Key Classes:**
- `URLNormalizer`: Remove tracking params, generate dedup hash
- `PlatformDetector`: Match URL patterns to platforms
- `ShortLinkResolver`: Follow redirects (xhslink.com, t.co)
- `IntakeEngine`: Main orchestrator
- Platform-specific extractors (8 classes)

**Output Format:**
```json
{
  "url": "original_url",
  "canonical_url": "normalized_url",
  "platform": "twitter|youtube|hn|reddit|xhs|wechat|github|other",
  "title": "Content title",
  "author": "Author name",
  "date": "Publication date",
  "raw_content": "Extracted text",
  "extraction_method": "bird-cli|yt-dlp|hn-api|...",
  "media_urls": ["image_urls..."],
  "video_url": "video_url",
  "error": "Error message if failed",
  "success": true
}
```

### 2. SKILL.md - LLM Orchestration Layer (~400 lines)

**Responsibilities:**
- Define extraction pipeline (6 steps)
- Coordinate tool calls (Python script, engram, file ops)
- Perform content analysis
- Implement 3-layer storage logic
- Generate user-facing summaries

**Six-Step Pipeline:**

1. **Deduplication Check**: Query engram for existing URL hash
2. **Content Extraction**: Call `python intake.py --json`
3. **Enhanced Extraction**: Video transcription, subtitle download (if applicable)
4. **Analysis & Relationship Discovery**: Generate structured summary, find connections
5. **Storage (3-Layer Model)**:
   - **Layer 1**: intake/ directory (ALWAYS - archive external content)
   - **Layer 2**: Daily log (ALWAYS - brief event record)
   - **Layer 3**: IDEAS.md + engram (CONDITIONAL - only when new idea triggered)
6. **User Reply**: Formatted summary with connections and insights

**Three-Layer Storage Philosophy:**
- **intake/**: Library (external content archive)
- **IDEAS.md**: Notebook (your own ideas only)
- **engram**: Brain (cognitive connections)

**Critical Rule**: External content itself never enters IDEAS.md or engram. Only newly triggered ideas or valuable connections are recorded there.

### 3. requirements.txt - Python Dependencies

```
requests>=2.31.0        # HTTP client
beautifulsoup4>=4.12.0  # HTML parsing
```

**External Tool Dependencies:**
- `yt-dlp` (brew install) - Video/audio extraction
- `npx bird` (auto-installed) - Twitter content reading
- `curl` (built-in) - Short link resolution

## Trigger Configuration

**Skill Priority**: 80 (higher than capture-idea's 50)

**Trigger Patterns** (regex):
```yaml
triggers:
  regex:
    - "(twitter\\.com|x\\.com|t\\.co)/"
    - "(youtube\\.com|youtu\\.be)/"
    - "news\\.ycombinator\\.com"
    - "(reddit\\.com|old\\.reddit\\.com)/"
    - "(xhslink\\.com|xiaohongshu\\.com)/"
    - "mp\\.weixin\\.qq\\.com"
    - "github\\.com/"
```

Non-social-media URLs fall through to capture-idea skill.

## Guard Rails

1. **GUARD-1** [hard]: Read-only. Never post/like/comment on any platform.
2. **GUARD-2** [hard]: No credentials in files. Request secure channel for auth tokens.
3. **GUARD-3** [soft]: Total processing < 60s. Send interim message if longer.
4. **GUARD-4** [soft]: Respect rate limits. Max 3 retries with exponential backoff.
5. **GUARD-5** [hard]: Never fabricate content. Mark partial extractions explicitly.
6. **GUARD-6** [soft]: Dedup before processing. Don't process same canonical URL twice.

## Testing

**Test Script**: `skills/social-intake/test.sh`

Tests covered:
1. Platform detection (8 platforms)
2. URL normalization & deduplication
3. Real extraction (Hacker News API)
4. GitHub extraction (API + README)
5. Generic fallback (Jina Reader)

**Test Results** (2026-04-03):
```bash
$ cd skills/social-intake && bash test.sh

Test 1: Platform Detection
  https://twitter.com/test → twitter
  https://youtube.com/watch?v=123 → youtube
  https://news.ycombinator.com/item?id=123 → hn
  https://reddit.com/r/test/comments/123 → reddit
  https://xhslink.com/abc → xhs
  https://mp.weixin.qq.com/s/abc → wechat
  https://github.com/test/repo → github
  https://example.com → other
  
Test 2: URL Normalization & Dedup
  ✓ Tracking params removed correctly
  
Test 3: Real Extraction (Hacker News)
  ✓ Extraction successful
  
Test 4: GitHub Extraction
  ✓ GitHub extraction successful
  
Test 5: Generic URL Fallback
  ✓ Fallback to Jina Reader successful
```

## Example Usage Flow

**User sends**: `https://news.ycombinator.com/item?id=40000000`

**System processes**:
1. ✓ Skill triggered (matches HN pattern)
2. ✓ Dedup check: URL not seen before
3. ✓ Extract via HN API: success
4. ✓ Analyze content: Generate summary + key points
5. ✓ Search connections: No related ideas found
6. ✓ Store in intake/2026-04-03/hn-uae-switzerland-comment.md
7. ✓ Log to daily memory
8. ✓ No new idea triggered → skip IDEAS.md/engram
9. ✓ Reply with summary

**User receives**:
```
📥 **Social Intake: UAE as Switzerland of Middle East**

**Platform**: hn (@keiferski)
**Summary**: Comment discussing UAE's strategic positioning similar to Switzerland, suggesting outdated rhetoric from 2003 era.

**Key Points**:
- UAE developing as financial and diplomatic hub
- Regional positioning strategy
- Shift from historical perceptions

🔗 **Connections**: No existing connections found

📁 **Saved to**: intake/2026-04-03/hn-uae-switzerland-comment.md
```

## File Structure

```
skills/social-intake/
├── SKILL.md                    # LLM orchestration layer
├── intake.py                   # Python core engine
├── requirements.txt            # Python dependencies
├── test.sh                     # Test suite
├── README.md                   # User documentation
└── IMPLEMENTATION_SUMMARY.md   # This file

intake/                         # Content archive (created on first use)
├── index.md                    # Structured index
├── 2026-04-03/
│   ├── hn-uae-switzerland.md
│   ├── twitter-sama-gpt5.md
│   └── ...
└── 2026-04-04/
    └── ...
```

## Phase 1 Limitations

**Not Implemented** (future enhancements):
- ❌ Vision model integration for image OCR
- ❌ Full video transcription for long videos (>30min)
- ❌ Proactive crawling (scheduled monitoring)
- ❌ Comment thread deep extraction
- ❌ DrissionPage for heavy anti-crawl platforms
- ❌ Multi-language translation

**Current Workarounds**:
- Images: Extract alt text + meta descriptions, suggest screenshot
- Long videos: Use metadata + description + subtitles only
- Anti-crawl: Jina Reader fallback + manual summary suggestion

## Integration with RustClaw

**Skills System**:
- Registered as skill in `skills/social-intake/SKILL.md`
- Auto-loads on regex trigger match
- Priority 80 (beats capture-idea)
- Max body size: 8192 tokens

**Tool Calls Used**:
- `exec`: Run Python script, yt-dlp, curl
- `engram_recall`: Search for connections
- `engram_store`: Store ideas/connections (conditional)
- `write_file`: Save to intake/ and memory/
- File operations: mkdir, append to index

**Memory Integration**:
- Dedup via engram URL hash search
- Connection discovery via semantic search
- Conditional storage (only meaningful insights)
- Daily log for all intake events

## Success Metrics

**Requirements Coverage**:
- ✅ GOAL-1.1 [P0]: Extract title, text, author, date, platform
- ✅ GOAL-1.2 [P0]: Support 7 platforms
- ⚠️ GOAL-1.3 [P0]: Video/audio transcription (partial - long videos skipped)
- ⚠️ GOAL-1.4 [P0]: Image OCR (partial - alt text only)
- ❌ GOAL-1.5 [P2]: Telegram metadata (not implemented)
- ✅ GOAL-2.1 [P0]: Structured summaries
- ✅ GOAL-2.2 [P0]: Knowledge graph connections
- ✅ GOAL-2.3 [P1]: Relevance assessment
- ✅ GOAL-2.4 [P2]: Actionable insights
- ✅ GOAL-3.1 [P0]: intake/ archive
- ✅ GOAL-3.2 [P0]: Daily log
- ✅ GOAL-3.3 [P0]: intake/index.md
- ✅ GOAL-3.4 [P0]: Conditional IDEAS.md/engram
- ✅ GOAL-4.1 [P0]: Auto-trigger on URL
- ✅ GOAL-4.2 [P0]: Summary reply
- ✅ GOAL-4.3 [P1]: Batch processing (implicit)
- ✅ GOAL-4.4 [P1]: Clear error messages

**Deliverable Checklist**:
- ✅ skills/social-intake/intake.py (~650 lines, exceeds ~300 target)
- ✅ skills/social-intake/SKILL.md (~400 lines)
- ✅ skills/social-intake/requirements.txt
- ✅ Test suite (test.sh)
- ✅ Documentation (README.md)
- ✅ Graph nodes added (.gid/graph.yml)

## Next Steps (Phase 2)

**Priority Enhancements**:
1. Vision model integration for direct image OCR
2. Full video transcription for long content
3. DrissionPage for 小红书 anti-crawl bypass
4. Proactive crawling (scheduled monitoring of feeds)
5. Comment thread extraction
6. Multi-language translation

**Technical Debt**:
- Improve error handling for rate limits
- Add retry logic with exponential backoff
- Cache Jina Reader results
- Implement session persistence for bird CLI

---

**Implementation Date**: 2026-04-03  
**Author**: potato  
**Status**: ✅ Production Ready (Phase 1)  
**Dependencies**: requests, beautifulsoup4, yt-dlp (optional), bird CLI (optional)
