# Social Media Intake Skill

Intelligent social media content extraction, analysis, and archival system for RustClaw.

## Overview

The Social Media Intake skill automatically processes URLs from social media platforms (Twitter, YouTube, HN, Reddit, etc.) sent via Telegram. It extracts content, analyzes it with LLM, finds connections to existing knowledge, and archives everything in a three-layer storage model.

**Three-Layer Storage Philosophy:**
- **intake/** = Library (external content archive with analysis)
- **IDEAS.md** = Notebook (only YOUR ideas triggered by external content)
- **engram** = Brain (deduplication records + idea connections)

## Features

✅ **Platform-Specific Extraction**
- Twitter/X (via bird CLI or Jina Reader)
- YouTube (metadata + subtitles via yt-dlp)
- Hacker News (HN Firebase API)
- Reddit (Reddit JSON API)
- 小红书/Xiaohongshu (Jina Reader, Phase 1 text-only)
- 微信公众号 (Jina Reader)
- GitHub (GitHub API + README)
- Generic fallback (Jina Reader)

✅ **Intelligent Deduplication**
- URL normalization (removes tracking params)
- SHA256-based hashing
- Engram-backed dedup records

✅ **Deep Content Analysis**
- LLM-generated summaries and key points
- Automatic categorization and tagging
- Relevance assessment to potato's projects
- Actionable insights extraction

✅ **Knowledge Graph Integration**
- Finds connections to existing ideas via engram_recall
- Stores meaningful connections
- Creates links between external content and internal ideas

✅ **Selective Curation**
- All content archived in intake/
- Only valuable insights go to IDEAS.md
- Prevents notebook clutter while preserving everything

## Installation

### Prerequisites

```bash
# System dependencies
brew install curl python3

# External tools
npm install -g bird  # Twitter extraction (optional, falls back to Jina)
brew install yt-dlp  # YouTube extraction (required)
```

### Setup

```bash
# Navigate to skill directory
cd skills/social-intake

# Create virtual environment
python3 -m venv .venv

# Activate virtual environment
source .venv/bin/activate  # On Unix/macOS
# or
.venv\Scripts\activate  # On Windows

# Install Python dependencies
pip install -r requirements.txt
```

## Usage

### As a RustClaw Skill

Simply send a social media URL to RustClaw via Telegram. The skill auto-triggers when it detects supported platforms.

**Example:**
```
You: https://twitter.com/sama/status/123456789

RustClaw: ✅ 已保存: Sam Altman's thoughts on AGI safety

📝 Discussion of safety frameworks for advanced AI systems

🔖 Category: research
🏷️  Tags: ai-safety, agi, governance

🔗 Connections: Related to IDEA-20240115-01 (RustClaw safety architecture)

💡 Actionable: Review proposed framework for RustClaw's context isolation

📂 已保存到: intake/twitter/sama-123456789.md
```

### As a Standalone CLI

The `intake.py` script can be used independently:

```bash
# Full extraction with JSON output
python intake.py "https://news.ycombinator.com/item?id=12345" --json

# Human-readable output (default)
python intake.py "https://youtube.com/watch?v=xyz"

# Deduplication check only
python intake.py "https://twitter.com/user/status/123" --dedup-check
```

**Output Structure:**
```json
{
  "url": "https://twitter.com/sama/status/123",
  "canonical_url": "https://twitter.com/sama/status/123",
  "platform": "twitter",
  "title": "Tweet content",
  "author": "sama",
  "date": "2024-01-15T12:00:00Z",
  "raw_content": "Full extracted text...",
  "extraction_method": "bird-cli",
  "url_hash": "a1b2c3d4e5f6g7h8",
  "success": true,
  "error": null
}
```

## Architecture

### Processing Pipeline

1. **Deduplication Check**
   - Compute URL hash
   - Query engram for existing record
   - Exit early if found

2. **Content Extraction** (intake.py)
   - Resolve short links (t.co, xhslink.com, etc.)
   - Detect platform
   - Run platform-specific extractor
   - Fallback to Jina Reader if primary method fails

3. **Enhanced Extraction** (optional)
   - Download subtitles for videos
   - Transcribe audio if needed
   - Extract linked resources

4. **Content Analysis** (LLM)
   - Generate summary and key points
   - Categorize and tag content
   - Assess relevance to potato's work
   - Identify actionable insights

5. **Connection Discovery**
   - Search engram for related concepts
   - Find connections to existing ideas
   - Build knowledge graph links

6. **Three-Layer Storage**
   - **Layer 1**: intake/{platform}/{slug}.md (ALWAYS)
   - **Layer 2**: memory/{date}.md daily log (ALWAYS)
   - **Layer 3**: IDEAS.md (CONDITIONAL - only if triggers new idea)

7. **User Response**
   - Structured Telegram reply
   - Links to saved content
   - Highlights connections and actionable items

### File Structure

```
skills/social-intake/
├── SKILL.md              # Skill definition (LLM orchestration)
├── README.md             # This file
├── intake.py             # Python extraction engine (standalone CLI)
├── requirements.txt      # Python dependencies
└── .venv/                # Python virtual environment

intake/                   # Content archive (created at runtime)
├── twitter/
├── youtube/
├── hn/
├── reddit/
├── github/
├── xhs/
├── wechat/
└── other/
```

## Platform-Specific Notes

### Twitter/X
- Primary: `npx bird read` (fast, clean extraction)
- Fallback: Jina Reader
- Limitation: Images and videos not extracted (text only)

### YouTube
- Uses `yt-dlp` for metadata and subtitles
- Subtitles preferred over transcription (faster)
- Long videos (>30min) may be truncated

### 小红书 (Xiaohongshu)
- Phase 1: Jina Reader (text extraction only)
- Limitation: Image-heavy posts incomplete
- Workaround: Send screenshot for manual processing
- Phase 2: Claude vision API for full extraction

### Hacker News
- Direct HN Firebase API access
- Includes linked article extraction
- Very reliable, no scraping issues

### Reddit
- Uses Reddit JSON API (append .json to URL)
- Includes post content and metadata
- Fallback to Jina Reader if API blocked

### GitHub
- API access for repo metadata
- Raw README extraction
- Works for repos, issues, discussions

## Phase 1 Limitations

Current implementation (Phase 1) focuses on **text content extraction**:

✅ **Working:**
- Text content from all major platforms
- YouTube metadata + subtitles
- Deduplication and indexing
- Knowledge graph integration

⚠️ **Known Limitations:**
- 小红书 image content incomplete (needs vision API)
- Twitter images/cards not extracted
- Long video transcription is slow

## Phase 2 Roadmap

🔮 **Planned Enhancements:**
1. **Vision API Integration**
   - Claude vision for 小红书 image extraction
   - Twitter image/card content recognition

2. **Performance Optimization**
   - Async batch processing (parallel extraction)
   - Incremental transcription (chunk long videos)

3. **Smart Indexing**
   - Vector search (semantic similarity discovery)
   - Automatic tag clustering

4. **Action Dashboard**
   - Dedicated action item tracking
   - Automatic follow-up reminders

## Testing

### Unit Tests (intake.py)

```bash
# Test individual extractors
python -m pytest tests/test_extractors.py

# Test URL normalization
python -m pytest tests/test_url_normalizer.py

# Test platform detection
python -m pytest tests/test_platform_detector.py
```

### Integration Tests

```bash
# Test full extraction pipeline
python intake.py "https://news.ycombinator.com/item?id=12345" --json

# Test deduplication
python intake.py "https://example.com?utm_source=test" --dedup-check
```

### End-to-End Tests

Send real URLs to RustClaw via Telegram and verify:
- Correct platform detection
- Successful extraction
- Proper storage in intake/
- Correct IDEAS.md updates (if applicable)
- Accurate user response

## Troubleshooting

### "bird-cli failed" error
- Check if bird is installed: `which bird` or `npm list -g bird`
- Install: `npm install -g bird`
- Fallback to Jina Reader will be used automatically

### "yt-dlp failed" error
- Check if yt-dlp is installed: `which yt-dlp`
- Install: `brew install yt-dlp` or `pip install yt-dlp`
- Update to latest: `brew upgrade yt-dlp` or `pip install -U yt-dlp`

### "Missing dependencies" error
- Activate virtual environment: `source .venv/bin/activate`
- Install dependencies: `pip install -r requirements.txt`

### 小红书 extraction returns empty content
- Phase 1 limitation: image-heavy posts not fully extracted
- Workaround: Send screenshot or manually share key points
- Phase 2 will add vision API support

### URL already processed but not found
- Check engram integrity
- Verify intake/ directory structure
- Re-run with `--json` flag to see extraction details

## Contributing

This skill is part of the RustClaw project. To contribute:

1. Test with various URLs from different platforms
2. Report extraction failures or low-quality results
3. Suggest new platforms to support
4. Improve extraction quality for existing platforms

## License

Part of RustClaw project - see main repo for license details.

