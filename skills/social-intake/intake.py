#!/usr/bin/env python3
"""
Social Media Intake - Core Extraction Engine
Handles URL deduplication, platform detection, content scraping, and file storage.
Called by RustClaw's social-intake skill for complex extraction tasks.

Usage:
    python intake.py <url> [--dedup-check] [--output-dir ./intake]
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.parse
from dataclasses import dataclass, asdict
from datetime import datetime
from pathlib import Path
from typing import Optional, Dict, List, Any
import requests
from bs4 import BeautifulSoup


# Platform detection patterns
PLATFORM_PATTERNS = {
    'twitter': [
        r'twitter\.com/',
        r'x\.com/',
        r't\.co/',
    ],
    'youtube': [
        r'youtube\.com/',
        r'youtu\.be/',
    ],
    'hn': [
        r'news\.ycombinator\.com',
    ],
    'reddit': [
        r'reddit\.com/',
        r'old\.reddit\.com/',
    ],
    'xhs': [
        r'xhslink\.com/',
        r'xiaohongshu\.com/',
    ],
    'wechat': [
        r'mp\.weixin\.qq\.com/',
    ],
    'github': [
        r'github\.com/',
        r'raw\.githubusercontent\.com/',
    ],
}


@dataclass
class ExtractionResult:
    """Structured result from content extraction"""
    url: str
    canonical_url: str
    platform: str
    title: Optional[str] = None
    author: Optional[str] = None
    date: Optional[str] = None
    raw_content: str = ""
    extraction_method: str = "unknown"
    media_urls: List[str] = None
    video_url: Optional[str] = None
    error: Optional[str] = None
    success: bool = True

    def __post_init__(self):
        if self.media_urls is None:
            self.media_urls = []

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


class URLNormalizer:
    """Normalize and deduplicate URLs"""
    
    TRACKING_PARAMS = [
        'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content',
        'fbclid', 'gclid', 'ref', 'source', '_hsenc', '_hsmi',
        's', 't',  # Twitter/X share tracking params
    ]

    @staticmethod
    def normalize(url: str) -> str:
        """Remove tracking parameters and normalize URL"""
        parsed = urllib.parse.urlparse(url)
        
        # Parse query params
        params = urllib.parse.parse_qs(parsed.query)
        
        # Remove tracking params
        clean_params = {
            k: v for k, v in params.items() 
            if k not in URLNormalizer.TRACKING_PARAMS
        }
        
        # Rebuild query string
        clean_query = urllib.parse.urlencode(clean_params, doseq=True)
        
        # Rebuild URL
        clean_url = urllib.parse.urlunparse((
            parsed.scheme,
            parsed.netloc,
            parsed.path,
            parsed.params,
            clean_query,
            ''  # Remove fragment
        ))
        
        return clean_url

    @staticmethod
    def get_hash(url: str) -> str:
        """Generate hash for URL deduplication"""
        canonical = URLNormalizer.normalize(url)
        return hashlib.sha256(canonical.encode()).hexdigest()[:16]


class PlatformDetector:
    """Detect platform from URL"""
    
    @staticmethod
    def detect(url: str) -> str:
        """Returns platform name or 'other'"""
        for platform, patterns in PLATFORM_PATTERNS.items():
            for pattern in patterns:
                if re.search(pattern, url, re.IGNORECASE):
                    return platform
        return 'other'


class ShortLinkResolver:
    """Resolve short links to canonical URLs"""
    
    @staticmethod
    def resolve(url: str) -> str:
        """Follow redirects and return final URL"""
        try:
            # Use curl to follow redirects (more reliable than requests for some platforms)
            result = subprocess.run(
                ['curl', '-Ls', '-o', '/dev/null', '-w', '%{url_effective}', url],
                capture_output=True,
                text=True,
                timeout=10
            )
            if result.returncode == 0 and result.stdout.strip():
                return result.stdout.strip()
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass
        
        # Fallback to requests
        try:
            response = requests.head(url, allow_redirects=True, timeout=5)
            return response.url
        except Exception:
            return url


class TwitterExtractor:
    """Extract content from Twitter/X"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='twitter')
        
        # Try bird CLI first
        try:
            bird_result = subprocess.run(
                ['npx', '-y', 'bird', 'read', url],
                capture_output=True,
                text=True,
                timeout=30
            )
            if bird_result.returncode == 0 and bird_result.stdout:
                result.raw_content = bird_result.stdout
                result.extraction_method = 'bird-cli'
                TwitterExtractor._parse_bird_output(result, bird_result.stdout)
                return result
        except (subprocess.TimeoutExpired, FileNotFoundError) as e:
            result.error = f"bird-cli failed: {e}"
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
            result.success = False
        
        return result
    
    @staticmethod
    def _parse_bird_output(result: ExtractionResult, output: str):
        """Parse bird CLI output for structured data"""
        # Bird typically outputs tweet text directly
        result.raw_content = output.strip()
        # Extract author if present in format "@username: text"
        match = re.match(r'@(\w+):\s*(.*)', output, re.DOTALL)
        if match:
            result.author = match.group(1)
            result.raw_content = match.group(2).strip()


class YouTubeExtractor:
    """Extract content from YouTube"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='youtube')
        
        try:
            # Get metadata with yt-dlp
            metadata_cmd = subprocess.run(
                ['yt-dlp', '--dump-json', '--no-download', url],
                capture_output=True,
                text=True,
                timeout=30
            )
            
            if metadata_cmd.returncode == 0 and metadata_cmd.stdout:
                data = json.loads(metadata_cmd.stdout)
                result.title = data.get('title')
                result.author = data.get('uploader')
                result.date = data.get('upload_date')
                result.raw_content = data.get('description', '')
                result.video_url = url
                result.extraction_method = 'yt-dlp-metadata'
                
                # Check for subtitles
                if data.get('subtitles') or data.get('automatic_captions'):
                    result.extraction_method = 'yt-dlp-metadata+subtitles'
                    result.raw_content += "\n\n[Note: Video has subtitles available for full transcript]"
                
                result.success = True
                return result
        except Exception as e:
            result.error = f"yt-dlp failed: {e}"
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
            result.success = False
        
        return result


class HackerNewsExtractor:
    """Extract content from Hacker News"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='hn')
        
        # Extract item ID from URL
        match = re.search(r'item\?id=(\d+)', url)
        if not match:
            result.error = "Could not parse HN item ID"
            result.success = False
            return result
        
        item_id = match.group(1)
        
        try:
            # Use HN API
            api_url = f"https://hacker-news.firebaseio.com/v0/item/{item_id}.json"
            response = requests.get(api_url, timeout=10)
            
            if response.status_code == 200:
                data = response.json()
                result.title = data.get('title')
                result.author = data.get('by')
                result.date = datetime.fromtimestamp(data.get('time', 0)).isoformat()
                
                # Get text content
                content_parts = []
                if data.get('title'):
                    content_parts.append(f"# {data['title']}")
                if data.get('text'):
                    content_parts.append(data['text'])
                if data.get('url'):
                    content_parts.append(f"\nOriginal URL: {data['url']}")
                    # Optionally fetch external link content
                    try:
                        external = requests.get(f"https://r.jina.ai/{data['url']}", timeout=10)
                        if external.status_code == 200:
                            content_parts.append(f"\n--- External Content ---\n{external.text[:2000]}")
                    except Exception:
                        pass
                
                result.raw_content = "\n\n".join(content_parts)
                result.extraction_method = 'hn-api'
                result.success = True
                return result
        except Exception as e:
            result.error = f"HN API failed: {e}"
            result.success = False
        
        return result


class RedditExtractor:
    """Extract content from Reddit"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='reddit')
        
        # Convert to old.reddit.com for easier parsing
        json_url = url.replace('www.reddit.com', 'old.reddit.com')
        if not json_url.endswith('.json'):
            json_url += '.json'
        
        try:
            response = requests.get(
                json_url,
                headers={'User-Agent': 'Mozilla/5.0'},
                timeout=10
            )
            
            if response.status_code == 200:
                data = response.json()
                
                # Reddit returns [post_data, comments_data]
                if isinstance(data, list) and len(data) > 0:
                    post_data = data[0]['data']['children'][0]['data']
                    
                    result.title = post_data.get('title')
                    result.author = post_data.get('author')
                    result.date = datetime.fromtimestamp(post_data.get('created_utc', 0)).isoformat()
                    
                    content_parts = [f"# {result.title}"]
                    if post_data.get('selftext'):
                        content_parts.append(post_data['selftext'])
                    if post_data.get('url') and post_data['url'] != url:
                        content_parts.append(f"\nLink: {post_data['url']}")
                    
                    result.raw_content = "\n\n".join(content_parts)
                    result.extraction_method = 'reddit-json-api'
                    result.success = True
                    return result
        except Exception as e:
            result.error = f"Reddit JSON API failed: {e}"
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
            result.success = False
        
        return result


class XiaohongshuExtractor:
    """Extract content from 小红书 using Playwright (headless browser).
    
    XHS blocks direct note access for unauthenticated users (error 300031).
    Strategy: Navigate to /explore first, then use client-side routing to load
    the note in a modal overlay, which renders the full content.
    """
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='xhs')
        
        # Resolve short links
        if 'xhslink.com' in url:
            canonical_url = ShortLinkResolver.resolve(url)
            result.canonical_url = canonical_url
            url = canonical_url
        
        # Extract note ID from URL
        note_id_match = re.search(r'/(?:discovery/item|explore)/([a-f0-9]+)', url)
        if not note_id_match:
            result.error = "Could not extract note ID from URL"
            result.success = False
            return result
        
        note_id = note_id_match.group(1)
        explore_url = f"https://www.xiaohongshu.com/explore/{note_id}"
        result.canonical_url = explore_url
        
        try:
            from playwright.sync_api import sync_playwright
            import time as _time
            
            with sync_playwright() as p:
                browser = p.chromium.launch(
                    headless=True,
                    args=['--disable-blink-features=AutomationControlled']
                )
                ctx = browser.new_context(
                    user_agent='Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36',
                    viewport={'width': 1440, 'height': 900}
                )
                page = ctx.new_page()
                page.add_init_script('Object.defineProperty(navigator, "webdriver", {get: () => undefined})')
                
                try:
                    # Step 1: Load explore homepage first (direct note URLs are blocked)
                    try:
                        page.goto('https://www.xiaohongshu.com/explore', timeout=15000)
                    except Exception:
                        pass  # Timeout on load event is OK, SPA may keep loading
                    _time.sleep(3)
                    
                    # Step 2: Navigate to the specific note via client-side routing
                    page.evaluate(f'window.history.pushState({{}}, "", "/explore/{note_id}")')
                    page.evaluate('window.dispatchEvent(new PopStateEvent("popstate"))')
                    _time.sleep(2)
                    
                    # If pushState didn't trigger the note modal, try clicking the note card
                    note_container = page.query_selector('#noteContainer')
                    if not note_container:
                        # Try direct navigation as fallback
                        try:
                            page.goto(explore_url, timeout=15000)
                        except Exception:
                            pass
                        _time.sleep(3)
                        note_container = page.query_selector('#noteContainer')
                    
                    # Step 3: Extract content from the modal/page
                    title_el = page.query_selector('#detail-title')
                    result.title = title_el.inner_text().strip() if title_el else None
                    
                    # Author
                    author_el = page.query_selector('.username') or page.query_selector('.author .name')
                    result.author = author_el.inner_text().strip() if author_el else None
                    
                    # Main content
                    desc_el = page.query_selector('#detail-desc') or page.query_selector('.note-text')
                    content_text = desc_el.inner_text().strip() if desc_el else ''
                    
                    # Tags from content
                    tags = re.findall(r'#\S+', content_text)
                    
                    # Interaction counts
                    interactions = {}
                    for name, selectors in [
                        ('likes', ['.like-wrapper .count', 'span.like-count']),
                        ('collects', ['.collect-wrapper .count', 'span.collect-count']),
                        ('comments', ['.chat-wrapper .count', 'span.comment-count']),
                    ]:
                        for sel in selectors:
                            el = page.query_selector(sel)
                            if el:
                                interactions[name] = el.inner_text().strip()
                                break
                    
                    # Images
                    for sel in ['#noteContainer img', '.swiper-slide img', '.note-image img']:
                        img_els = page.query_selector_all(sel)
                        for img_el in img_els:
                            src = img_el.get_attribute('src') or img_el.get_attribute('data-src') or ''
                            if src.startswith('//'):
                                src = 'https:' + src
                            if src.startswith('http') and ('xhscdn' in src or 'xiaohongshu' in src):
                                if src not in result.media_urls:
                                    result.media_urls.append(src)
                    
                    # Video
                    video_el = page.query_selector('video source') or page.query_selector('video')
                    if video_el:
                        video_src = video_el.get_attribute('src') or ''
                        if video_src.startswith('//'):
                            video_src = 'https:' + video_src
                        if video_src.startswith('http'):
                            result.media_urls.append(video_src)
                    
                    # Build structured output
                    parts = []
                    if result.title:
                        parts.append(f"# {result.title}")
                    if result.author:
                        parts.append(f"**Author:** {result.author}")
                    if content_text:
                        parts.append(f"\n{content_text}")
                    if tags:
                        parts.append(f"\n**Tags:** {' '.join(tags)}")
                    if interactions:
                        parts.append(f"\n**Interactions:** {json.dumps(interactions, ensure_ascii=False)}")
                    if result.media_urls:
                        parts.append(f"\n**Media:** {len(result.media_urls)} items")
                    
                    result.raw_content = '\n'.join(parts)
                    result.extraction_method = 'playwright'
                    result.success = bool(content_text or result.title)
                    
                    if not result.success:
                        # Last resort: get full noteContainer text
                        if note_container:
                            full_text = note_container.inner_text()[:5000]
                            if full_text and len(full_text) > 50:
                                result.raw_content = full_text
                                result.success = True
                                result.extraction_method = 'playwright-container'
                
                finally:
                    browser.close()
                    
        except ImportError:
            result.error = "Playwright not installed. Run: pip install playwright && playwright install chromium"
            result.success = False
        except Exception as e:
            result.error = f"XHS extraction failed: {e}"
            result.success = False
        
        return result


class WechatExtractor:
    """Extract content from WeChat official accounts"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='wechat')
        
        # Try direct fetch first
        try:
            response = requests.get(
                url,
                headers={'User-Agent': 'Mozilla/5.0'},
                timeout=15
            )
            
            if response.status_code == 200:
                soup = BeautifulSoup(response.text, 'html.parser')
                
                # Extract title
                title_tag = soup.find('meta', property='og:title')
                if title_tag:
                    result.title = title_tag.get('content')
                
                # Extract author
                author_tag = soup.find('meta', attrs={'name': 'author'})
                if author_tag:
                    result.author = author_tag.get('content')
                
                # Extract content
                content_div = soup.find('div', id='js_content')
                if content_div:
                    result.raw_content = content_div.get_text(separator='\n', strip=True)
                    result.extraction_method = 'direct-fetch'
                    result.success = True
                    return result
        except Exception:
            pass
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"WeChat extraction failed: {e}"
            result.success = False
        
        return result


class GitHubExtractor:
    """Extract content from GitHub"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='github')
        
        # Parse URL to determine type (repo, issue, discussion)
        parsed = urllib.parse.urlparse(url)
        path_parts = [p for p in parsed.path.split('/') if p]
        
        if len(path_parts) >= 2:
            owner, repo = path_parts[0], path_parts[1]
            
            # If it's a repo page, get README
            if len(path_parts) == 2 or (len(path_parts) == 3 and path_parts[2] in ['tree', 'blob']):
                return GitHubExtractor._extract_repo(owner, repo, url, result)
            
            # If it's an issue/discussion, use Jina Reader
            elif 'issues' in path_parts or 'discussions' in path_parts:
                return GitHubExtractor._extract_issue(url, result)
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
        except Exception as e:
            result.error = f"GitHub extraction failed: {e}"
            result.success = False
        
        return result
    
    @staticmethod
    def _extract_repo(owner: str, repo: str, url: str, result: ExtractionResult) -> ExtractionResult:
        """Extract repo README and metadata"""
        try:
            # Get repo metadata
            api_url = f"https://api.github.com/repos/{owner}/{repo}"
            response = requests.get(api_url, timeout=10)
            
            if response.status_code == 200:
                data = response.json()
                result.title = f"{owner}/{repo}"
                result.author = owner
                result.date = data.get('created_at')
                
                content_parts = [
                    f"# {owner}/{repo}",
                    f"Description: {data.get('description', 'N/A')}",
                    f"Stars: {data.get('stargazers_count', 0)}",
                    f"Language: {data.get('language', 'N/A')}",
                    f"Topics: {', '.join(data.get('topics', []))}",
                ]
                
                # Try to get README
                readme_url = f"https://raw.githubusercontent.com/{owner}/{repo}/HEAD/README.md"
                readme_response = requests.get(readme_url, timeout=10)
                if readme_response.status_code == 200:
                    content_parts.append(f"\n--- README ---\n{readme_response.text}")
                
                result.raw_content = "\n\n".join(content_parts)
                result.extraction_method = 'github-api+readme'
                result.success = True
                return result
        except Exception as e:
            result.error = f"GitHub repo extraction failed: {e}"
            result.success = False
        
        return result
    
    @staticmethod
    def _extract_issue(url: str, result: ExtractionResult) -> ExtractionResult:
        """Extract issue/discussion content"""
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
        except Exception as e:
            result.error = f"GitHub issue extraction failed: {e}"
            result.success = False
        
        return result


class GenericExtractor:
    """Fallback extractor for unknown platforms"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='other')
        
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
            else:
                result.error = f"Jina Reader returned status {response.status_code}"
                result.success = False
        except Exception as e:
            result.error = f"Generic extraction failed: {e}"
            result.success = False
        
        return result


class IntakeEngine:
    """Main extraction orchestrator"""
    
    EXTRACTORS = {
        'twitter': TwitterExtractor,
        'youtube': YouTubeExtractor,
        'hn': HackerNewsExtractor,
        'reddit': RedditExtractor,
        'xhs': XiaohongshuExtractor,
        'wechat': WechatExtractor,
        'github': GitHubExtractor,
        'other': GenericExtractor,
    }
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        """Main entry point for content extraction"""
        # Normalize URL
        canonical_url = URLNormalizer.normalize(url)
        
        # Detect platform
        platform = PlatformDetector.detect(canonical_url)
        
        # Get appropriate extractor
        extractor_class = IntakeEngine.EXTRACTORS.get(platform, GenericExtractor)
        
        # Extract content
        result = extractor_class.extract(url)
        result.canonical_url = canonical_url
        
        return result


def main():
    parser = argparse.ArgumentParser(description='Social Media Intake - Content Extraction Engine')
    parser.add_argument('url', help='URL to extract content from')
    parser.add_argument('--dedup-check', action='store_true', help='Only output URL hash for dedup check')
    parser.add_argument('--output-dir', default='./intake', help='Output directory for saved content')
    parser.add_argument('--json', action='store_true', help='Output result as JSON')
    
    args = parser.parse_args()
    
    # Dedup check mode
    if args.dedup_check:
        url_hash = URLNormalizer.get_hash(args.url)
        print(url_hash)
        return 0
    
    # Normal extraction mode
    result = IntakeEngine.extract(args.url)
    
    if args.json:
        print(json.dumps(result.to_dict(), indent=2, ensure_ascii=False))
    else:
        # Human-readable output
        print(f"Platform: {result.platform}")
        print(f"Canonical URL: {result.canonical_url}")
        print(f"Method: {result.extraction_method}")
        print(f"Success: {result.success}")
        
        if result.title:
            print(f"Title: {result.title}")
        if result.author:
            print(f"Author: {result.author}")
        if result.date:
            print(f"Date: {result.date}")
        if result.error:
            print(f"Error: {result.error}")
        
        print(f"\n--- Content ---")
        print(result.raw_content[:500] + "..." if len(result.raw_content) > 500 else result.raw_content)
    
    return 0 if result.success else 1


if __name__ == '__main__':
    sys.exit(main())
