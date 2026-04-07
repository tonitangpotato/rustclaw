# Self-Review Round 1: Social Media Intake Skill

**Review Date:** 2024
**Reviewer:** Claude (Self-review)
**Artifacts Reviewed:**
- `.gid/requirements-social-intake.md`
- `.gid/design-social-intake.md`
- `skills/social-intake/SKILL.md`
- `skills/social-intake/intake.py`
- `skills/social-intake/requirements.txt`
- `skills/social-intake/README.md`

---

## Executive Summary

✅ **Overall Assessment:** ACCEPTABLE WITH MINOR FIXES REQUIRED

The design is comprehensive and well-thought-out, but there are **8 critical issues** that need to be addressed before implementation:

1. ❌ Deduplication logic inconsistency between SKILL.md and implementation
2. ❌ Missing `--dedup-check` flag in intake.py 
3. ⚠️ Overcomplicated three-layer conditional storage logic
4. ⚠️ Missing domain classification in intake.py output
5. ⚠️ Unclear action items storage location
6. ⚠️ Missing error handling for short link resolution failures
7. ⚠️ bird CLI dependency not documented/handled gracefully
8. ⚠️ Vision API limitation not properly handled with fallback

---

## Detailed Issues & Fixes

### Issue #1: Deduplication Logic Inconsistency ❌

**Problem:**
- SKILL.md Step 1 says to check dedup with: `engram_recall("url_hash:{url_hash}")` or `engram_recall("{url}")`
- But Step 5 Layer 1 stores dedup record as: `"Intake processed: url_hash:{url_hash} | {url}..."`
- Engram uses semantic search, so exact hash matching may not trigger reliably

**Impact:** High - deduplication may fail, leading to duplicate content storage

**Fix Required:**
Add explicit metadata field to engram storage for reliable hash-based lookup:

```yaml
# In SKILL.md Step 5, Layer 1
engram_store(
  content="Intake processed: {title} | {summary}",
  metadata={
    "type": "social_intake_dedup",
    "url_hash": "{url_hash}",
    "url": "{canonical_url}",
    "platform": "{platform}",
    "timestamp": "{iso_timestamp}"
  }
)
```

And update Step 1 dedup check to:
```bash
# Query engram by metadata filter instead of semantic search
dedup_check=$(engram_query --filter "url_hash:{url_hash}" --limit 1)
if [ -n "$dedup_check" ]; then
  echo "✓ URL already processed (hash: {url_hash})"
  exit 0
fi
```

---

### Issue #2: Missing `--dedup-check` Flag ❌

**Problem:**
SKILL.md Step 1 shows:
```bash
url_hash=$(python intake.py "{url}" --dedup-check)
```

But `intake.py` doesn't implement this flag. The argparse section likely only has standard extraction.

**Impact:** High - dedup check step will fail

**Fix Required:**
Add to intake.py main():

```python
def main():
    parser = argparse.ArgumentParser(description="Social Media Intake")
    parser.add_argument('url', help='URL to process')
    parser.add_argument('--dedup-check', action='store_true', 
                       help='Only return url_hash and canonical_url for dedup check')
    parser.add_argument('--output-dir', default='./intake', 
                       help='Output directory for intake files')
    
    args = parser.parse_args()
    
    # Step 1: Resolve short links
    canonical_url = ShortLinkResolver.resolve(args.url)
    url_hash = URLNormalizer.get_hash(canonical_url)
    
    # If dedup-check mode, return early
    if args.dedup_check:
        print(json.dumps({
            'url_hash': url_hash,
            'canonical_url': canonical_url
        }))
        return
    
    # ... rest of extraction logic
```

---

### Issue #3: Overcomplicated Storage Logic ⚠️

**Problem:**
SKILL.md has a three-layer nested conditional for deciding where to store content:
- Layer 1: Always store dedup record in engram
- Layer 2: Complex decision tree for IDEAS.md vs engram based on multiple conditions
- Layer 3: Conditional archival to CURATE.md

This is difficult for an LLM executor to follow and prone to errors.

**Impact:** Medium - execution errors, inconsistent storage

**Recommendation:**
Simplify to a clearer decision table:

| Condition | Dedup (Layer 1) | Main Storage (Layer 2) | Archive (Layer 3) |
|-----------|----------------|----------------------|------------------|
| `is_quick_ref=true` | ✓ Engram | ✓ IDEAS.md | ✗ Skip |
| `is_actionable=true` | ✓ Engram | ✓ IDEAS.md | ✗ Skip |
| `is_evergreen=true` | ✓ Engram | ✓ Engram | ✓ CURATE.md |
| `is_discussion=true` | ✓ Engram | ✓ Engram | ✗ Skip |
| `is_shallow=true` | ✓ Engram | ✗ Skip | ✗ Skip |
| Default | ✓ Engram | ✓ Engram | ✗ Skip |

**Fix:** Rewrite SKILL.md Step 5 with this clearer table format.

---

### Issue #4: Missing Domain Classification ⚠️

**Problem:**
- SKILL.md describes detailed domain classification (Tech/AI, Design/Product, etc.)
- But `ExtractionResult` dataclass in intake.py doesn't have a `domain` field
- It's unclear whether domain is LLM-only or should be in structured output

**Impact:** Low - domain classification will still work via LLM, but inconsistent with design

**Fix Required:**
Either:
1. Add `domain: Optional[str] = None` to ExtractionResult dataclass
2. Or clarify in SKILL.md that domain is LLM-classified, not from intake.py

**Recommendation:** Add to dataclass for consistency:
```python
@dataclass
class ExtractionResult:
    # ... existing fields ...
    domain: Optional[str] = None  # LLM-classified: tech, design, business, etc.
```

---

### Issue #5: Unclear Action Items Storage ⚠️

**Problem:**
SKILL.md Step 6 has detailed action items extraction logic, but doesn't specify:
- Where are action items stored? (Separate file? In intake file? Engram metadata?)
- What format? (TODO.md? Structured JSON?)

**Impact:** Medium - action items may be lost or inconsistently stored

**Fix Required:**
Add explicit storage location in SKILL.md Step 6:

```yaml
# If action_items extracted:
action_items_file="./intake/{url_hash}-actions.md"
cat > "$action_items_file" << EOF
# Action Items - {title}
Source: {url}
Extracted: {timestamp}

{action_items}
EOF

# Also store in engram for recall
engram_store(
  content="Action items from {title}: {action_items_summary}",
  metadata={"type": "action_items", "source_url_hash": "{url_hash}"}
)
```

---

### Issue #6: Short Link Resolution Error Handling ⚠️

**Problem:**
`ShortLinkResolver.resolve()` in intake.py can fail silently and return original URL.
But SKILL.md doesn't have fallback instructions for when resolution fails.

**Impact:** Low - most short links will resolve, but failures are silent

**Fix Required:**
Add error reporting to intake.py:

```python
def resolve(url: str) -> str:
    # ... existing logic ...
    
    # If resolution fails, return original URL with warning
    print(f"[WARNING] Short link resolution failed: {url}", file=sys.stderr)
    return url
```

And add to SKILL.md Step 1:
```bash
# If canonical_url == original_url and original is short link, log warning
if [[ "$canonical_url" == "$url" ]] && [[ "$url" =~ (t\.co|xhslink|b23\.tv) ]]; then
  echo "⚠️ Warning: Short link resolution may have failed" >&2
fi
```

---

### Issue #7: bird CLI Dependency Not Documented ⚠️

**Problem:**
- intake.py uses `npx -y bird read {url}` for Twitter extraction
- But this requires Node.js and npx to be installed
- requirements.txt only has Python deps
- No setup verification or graceful degradation documented

**Impact:** Medium - Twitter extraction may fail silently on systems without Node

**Fix Required:**

Add to `skills/social-intake/README.md`:

```markdown
## Dependencies

### Python Dependencies
Install via: `pip install -r requirements.txt`

### Optional External Tools
- **Node.js + npx** (for Twitter extraction via bird CLI)
  - Install: `brew install node` (macOS) or `apt install nodejs npm` (Linux)
  - Fallback: Uses Jina Reader if bird CLI unavailable
  
- **yt-dlp** (for YouTube metadata)
  - Install: `brew install yt-dlp` or `pip install yt-dlp`
  - Fallback: Uses Jina Reader if unavailable

- **curl** (for short link resolution)
  - Usually pre-installed on Unix systems
  - Fallback: Uses Python requests library
```

Add to intake.py TwitterExtractor:
```python
# Add better error message
except FileNotFoundError:
    result.error = "bird-cli not found (requires Node.js/npx). Install: npm install -g bird"
    print(f"[INFO] bird CLI not available, falling back to Jina Reader", file=sys.stderr)
```

---

### Issue #8: Vision API Limitation Not Handled ⚠️

**Problem:**
- SKILL.md mentions "Phase 1 limitation - image OCR requires vision model integration"
- But no fallback behavior defined for image-heavy platforms (Instagram, Pinterest)
- intake.py doesn't flag image-heavy content for later processing

**Impact:** Low - image-heavy content will be extracted but incomplete

**Fix Required:**

Add to ExtractionResult:
```python
@dataclass
class ExtractionResult:
    # ... existing fields ...
    is_image_heavy: bool = False  # Flag for content needing vision model
    image_urls: List[str] = field(default_factory=list)  # Extracted image URLs
```

Add to SKILL.md Step 4.5 (new step):
```yaml
# Check if content is image-heavy and needs vision processing
if [[ "$platform" =~ (xhs|weibo) ]] && [ ${#image_urls[@]} -gt 2 ]; then
  echo "📸 Image-heavy content detected (${#image_urls[@]} images)" >&2
  echo "⚠️ Phase 1 limitation: Image OCR requires vision model" >&2
  echo "TODO: Add to vision processing queue" >> ./intake/vision-queue.txt
fi
```

---

## Architecture Review

### ✅ Strengths

1. **Comprehensive platform coverage** - Twitter, YouTube, HN, Reddit, GitHub, WeChat, XHS, Weibo, Bilibili
2. **Robust fallback strategy** - Multiple extraction methods per platform
3. **Clean separation** - Python for extraction, Bash for orchestration, LLM for analysis
4. **Deduplication strategy** - URL normalization + hash-based checking
5. **Metadata preservation** - Author, date, platform tracking
6. **Engram integration** - Good use of knowledge graph for recall

### ⚠️ Concerns

1. **Complexity** - 3-layer storage logic is error-prone
2. **LLM dependency** - Heavy reliance on LLM for classification/summarization (performance risk)
3. **External tool dependency** - bird, yt-dlp, npx not guaranteed to be available
4. **Error handling** - Some silent failures possible

### ✅ Edge Cases Handled

- Short link resolution (t.co, xhslink.com, etc.)
- URL normalization (tracking params removed)
- Platform-specific extraction methods
- Fallback to Jina Reader

### ❌ Edge Cases NOT Handled

- Rate limiting from external APIs (Twitter API, Reddit API)
- Paywalled content (WSJ, Medium, etc.)
- Login-required content (private GitHub repos, private X posts)
- Large video files (YouTube downloads disabled by design)
- Non-English content (may need translation)

---

## Does It Solve the Problem?

**Original Goal:** potato 在 Telegram 转发社交媒体 URL，RustClaw 自动抓取、分析、归档

✅ **Yes, the design solves the stated problem:**
- ✓ Auto-triggers on URL detection in Telegram messages
- ✓ Extracts content from major platforms
- ✓ Deduplicates to avoid reprocessing
- ✓ Summarizes and analyzes with LLM
- ✓ Stores in appropriate locations (IDEAS.md, engram, CURATE.md)
- ✓ Integrates with existing knowledge graph

**But with caveats:**
- ⚠️ Phase 1 doesn't handle image-heavy content well
- ⚠️ Requires external tools (bird, yt-dlp) to be pre-installed
- ⚠️ Complex storage logic may confuse LLM executor

---

## Conflicts with Existing Architecture

### ✅ No Major Conflicts Found

The design integrates well with existing RustClaw architecture:
- Uses standard SKILL.md format
- Leverages engram for memory
- Follows IDEAS.md and CURATE.md patterns
- Uses Telegram as message bus (existing pattern)

### ⚠️ Minor Concerns

1. **Python in Rust project** - intake.py is a Python script in a Rust project. This is acceptable for Phase 1 but should be noted.
2. **File-based storage** - Uses IDEAS.md and CURATE.md, which is fine but may need migration to database later
3. **LLM cost** - Heavy LLM usage per URL may incur costs at scale

---

## Recommendations

### Must Fix Before Implementation

1. ❌ Fix deduplication logic (Issue #1) - Add metadata-based engram query
2. ❌ Add `--dedup-check` flag to intake.py (Issue #2)
3. ⚠️ Simplify storage logic (Issue #3) - Use decision table
4. ⚠️ Document external tool dependencies (Issue #7)

### Should Fix (Can Be Deferred)

5. ⚠️ Add domain field to ExtractionResult (Issue #4)
6. ⚠️ Clarify action items storage (Issue #5)
7. ⚠️ Add short link error handling (Issue #6)
8. ⚠️ Add vision API limitation handling (Issue #8)

### Future Enhancements (Out of Scope)

- Rate limiting protection
- Paywalled content handling
- Multi-language support
- Async batch processing
- Database migration from file-based storage

---

## Verdict

**Status:** ✅ APPROVED WITH REQUIRED FIXES

**Action Items:**
1. Fix deduplication logic in SKILL.md (must)
2. Add --dedup-check flag to intake.py (must)
3. Simplify storage decision logic (should)
4. Document external dependencies in README (should)

**Estimated Fix Time:** 1-2 hours

**Ready for Implementation After Fixes:** YES

---

## Sign-off

After the 4 must/should fixes are applied, this design is ready for implementation.

The design is well-architected, comprehensive, and solves the stated problem effectively. The issues found are mostly implementation details rather than fundamental design flaws.

**Reviewer:** Claude (AI Assistant)  
**Status:** CONDITIONAL PASS - Requires fixes to Issues #1, #2, #3, #7
