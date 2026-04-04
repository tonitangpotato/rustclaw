# Social Media Intake Skill

Intelligent social media content extraction, analysis, and archival system for RustClaw.

## Overview

The Social Media Intake skill automatically processes URLs from social media platforms, extracting content, analyzing it for connections to existing knowledge, and archiving it in a structured format. It distinguishes between external content (stored in `intake/`) and personal ideas triggered by that content (stored in `IDEAS.md`).

## Components

### 1. `intake.py` - Python Core Engine (~300 lines)
Platform-specific content extraction with fallback chains:
- **Platform Detection**: Automatically identifies Twitter/X, YouTube, HN, Reddit, 小红书, WeChat, GitHub
- **URL Normalization**: Removes tracking parameters, generates deduplication hashes
- **Extraction Methods**: 
  - Twitter: bird CLI → Jina Reader
  - YouTube: yt-dlp metadata/subtitles
  - HN: Official Firebase API
  - Reddit: JSON API
  - GitHub: API + raw.githubusercontent.com
  - 小红书: Jina Reader (+ DrissionPage for Phase 2)
  - WeChat: Direct fetch + Jina Reader
  - Fallback: Jina Reader for unknown platforms
- **Output**: Structured JSON with metadata, content, and extraction status

### 2. `SKILL.md` - LLM Orchestration Layer
High-level workflow definition that RustClaw's LLM agent follows:
- Deduplication check (search engram for existing URL)
- Content extraction (calls Python engine)
- Video transcription (yt-dlp + whisper for audio content)
- Content analysis (LLM generates structured summary)
- Knowledge graph search (finds related ideas/projects)
- Three-layer storage:
  - `intake/` - Archive of all external content (ALWAYS)
  - `IDEAS.md` - Personal ideas triggered by content (CONDITIONAL)
  - `engram` - Cognitive connections (CONDITIONAL)
- User response with summary and connections

### 3. `requirements.txt` - Python Dependencies
- `requests` - HTTP client
- `beautifulsoup4` - HTML parsing

## Installation

### Python Dependencies
```bash
cd skills/social-intake
pip install -r requirements.txt
```

### External Tools
```bash
# For video/audio extraction (optional but recommended)
brew install yt-dlp

# For Twitter (no installation needed, uses npx)
# First run will auto-download bird CLI
npx -y bird --version
```

## Usage

### Automatic Activation
The skill automatically activates when potato sends a URL from a supported platform in Telegram. No manual trigger needed.

**Supported platforms:**
- Twitter/X: `twitter.com`, `x.com`, `t.co`
- YouTube: `youtube.com`, `youtu.be`
- Hacker News: `news.ycombinator.com`
- Reddit: `reddit.com`, `old.reddit.com`
- 小红书: `xhslink.com`, `xiaohongshu.com`
- WeChat: `mp.weixin.qq.com`
- GitHub: `github.com`

### Manual Testing
```bash
# Test extraction with JSON output
python intake.py "https://news.ycombinator.com/item?id=12345678" --json

# Test deduplication hash
python intake.py "https://example.com/article" --dedup-check

# Human-readable output
python intake.py "https://github.com/rust-lang/rust"
```

## Storage Structure

```
intake/
├── index.md                          # Searchable index of all intake
├── 2026-04-03/
│   ├── twitter-sama-gpt5.md         # Archived tweet
│   ├── youtube-rust-async.md        # Video metadata + transcript
│   └── hn-new-llm-paper.md          # HN post + external article
└── 2026-04-04/
    └── xhs-productivity-tips.md     # 小红书 post

IDEAS.md                              # Only ideas triggered by intake
└── IDEA-20260403-01: {Your idea inspired by Twitter post}

memory/
└── 2026-04-03.md                    # Daily log mentions intake events
```

## Key Design Principles

1. **Three-Layer Storage Model**
   - `intake/` = Library (external content, always saved)
   - `IDEAS.md` = Notebook (your ideas only, conditional)
   - `engram` = Brain (connections and insights, conditional)

2. **No Duplicate Processing**
   - URL hash-based deduplication
   - Checks engram before extraction
   - Normalized URLs (removes tracking params)

3. **Graceful Degradation**
   - Platform tool → Jina Reader → web_fetch fallback chain
   - Partial extraction marked clearly
   - Honest error reporting (never fabricate content)

4. **Respect for Platforms**
   - Read-only operations (no posting/liking/commenting)
   - Rate limit compliance
   - No aggressive retries

5. **Context-Aware Analysis**
   - Searches existing knowledge graph
   - Identifies connections to past ideas
   - Assesses relevance to current projects
   - Only creates new IDEAS.md entry if genuinely inspired

## Example Flow

**User sends:** `https://twitter.com/sama/status/1234567890`

**System processes:**
1. ✓ Dedup check (not seen before)
2. ✓ Platform detection → Twitter
3. ✓ Extract via bird CLI
4. ✓ Analyze content with LLM
5. ✓ Search engram for connections → finds IDEA-20260401-03
6. ✓ Store to `intake/2026-04-03/twitter-sama-gpt5.md`
7. ✓ No new idea triggered → skip IDEAS.md
8. ✓ Log to `memory/2026-04-03.md`
9. ✓ Reply with summary + connection note

**User receives:**
```
📥 Social Intake: Sam Altman on GPT-5 Progress

Platform: twitter (@sama)
Summary: Update on GPT-5 training timeline...

Key Points:
- Training on schedule for Q4
- Focus on reasoning depth
- Multimodal from ground up

🔗 Connections:
Related to IDEA-20260401-03 (AI agent reasoning)

💰 Potential Value: High - relevant to RustClaw design

📁 Saved to: intake/2026-04-03/twitter-sama-gpt5.md
```

## Limitations & Future Work

**Phase 1 Limitations:**
- Image OCR requires vision model (not yet integrated)
- Long videos (>30min) skip full transcription
- 小红书 anti-crawl may block some content
- No proactive crawling (passive intake only)

**Future Enhancements:**
- Claude vision integration for image content
- DrissionPage for heavy anti-crawl platforms
- Automated periodic crawling of key sources
- Multi-language translation
- Full comment thread extraction

## Related Files

- `.gid/requirements-social-intake.md` - Detailed requirements (14 goals, 6 guards)
- `.gid/design-social-intake.md` - Technical design document
- `DESIGN.md` - RustClaw architecture overview (includes skill section)

## License

Part of the RustClaw project. See parent repository for license.
