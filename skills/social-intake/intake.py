#!/usr/bin/env python3
"""
Social Media Intake - Core Extraction Engine
Handles URL deduplication, platform detection, content scraping, and file storage.
Called by RustClaw's social-intake skill for complex extraction tasks.

Usage:
    python intake.py <url> [--json] [--dedup-check]
    
Examples:
    # Extract with JSON output
    python intake.py "https://twitter.com/sama/status/123" --json
    
    # Get dedup hash only
    python intake.py "https://example.com" --dedup-check
    
    # Human-readable output (default)
    python intake.py "https://news.ycombinator.com/item?id=12345"
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
import urllib.parse
from dataclasses import dataclass, asdict
from datetime import datetime
from typing import Optional, Dict, List, Any

try:
    import requests
    from bs4 import BeautifulSoup
except ImportError:
    print("Error: Missing dependencies. Install with: pip install -r requirements.txt", file=sys.stderr)
    sys.exit(1)


# Platform detection patterns
PLATFORM_PATTERNS = {
    'twitter': [r'twitter\.com/', r'x\.com/'],
    'youtube': [r'youtube\.com/', r'youtu\.be/'],
    'hn': [r'news\.ycombinator\.com'],
    'reddit': [r'reddit\.com/', r'old\.reddit\.com/'],
    'xhs': [r'xiaohongshu\.com/'],
    'wechat': [r'mp\.weixin\.qq\.com/'],
    'github': [r'github\.com/', r'raw\.githubusercontent\.com/'],
    'weibo': [r'weibo\.com/'],
    'bilibili': [r'bilibili\.com/', r'b23\.tv/'],
}

# Short link domains that need resolution
SHORT_LINK_DOMAINS = ['t.co', 'xhslink.com', 'b23.tv', 'bit.ly', 'tinyurl.com']


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
    url_hash: str = ""
    error: Optional[str] = None
    success: bool = True

    def __post_init__(self):
        if self.media_urls is None:
            self.media_urls = []
        if not self.url_hash and self.canonical_url:
            self.url_hash = URLNormalizer.get_hash(self.canonical_url)

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


class URLNormalizer:
    """Normalize and deduplicate URLs"""
    
    TRACKING_PARAMS = [
        'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content',
        'fbclid', 'gclid', 'ref', 'source', '_hsenc', '_hsmi', 's', 't',
    ]

    @staticmethod
    def normalize(url: str) -> str:
        """Remove tracking parameters and normalize URL"""
        parsed = urllib.parse.urlparse(url)
        params = urllib.parse.parse_qs(parsed.query)
        clean_params = {k: v for k, v in params.items() if k not in URLNormalizer.TRACKING_PARAMS}
        clean_query = urllib.parse.urlencode(clean_params, doseq=True)
        return urllib.parse.urlunparse((
            parsed.scheme, parsed.netloc, parsed.path,
            parsed.params, clean_query, ''
        ))

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
        parsed = urllib.parse.urlparse(url)
        domain = parsed.netloc.lower().replace('www.', '')
        
        is_short_link = any(domain == sl or domain.endswith('.' + sl) for sl in SHORT_LINK_DOMAINS)
        if not is_short_link:
            return url
        
        # Try curl first (more reliable for some platforms)
        try:
            result = subprocess.run(
                ['curl', '-Ls', '-o', '/dev/null', '-w', '%{url_effective}', url],
                capture_output=True, text=True, timeout=10
            )
            if result.returncode == 0 and result.stdout.strip():
                resolved = result.stdout.strip()
                print(f"[Resolved] {url} → {resolved}", file=sys.stderr)
                return resolved
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass
        
        # Fallback to requests
        try:
            response = requests.head(url, allow_redirects=True, timeout=5)
            if response.url and response.url != url:
                print(f"[Resolved] {url} → {response.url}", file=sys.stderr)
                return response.url
        except Exception:
            pass
        
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
                capture_output=True, text=True, timeout=30
            )
            if bird_result.returncode == 0 and bird_result.stdout:
                result.raw_content = bird_result.stdout.strip()
                result.extraction_method = 'bird-cli'
                # Extract author if format is "@username: text"
                match = re.match(r'@(\w+):\s*(.*)', result.raw_content, re.DOTALL)
                if match:
                    result.author = match.group(1)
                    result.raw_content = match.group(2).strip()
                result.success = True
                return result
        except (subprocess.TimeoutExpired, FileNotFoundError) as e:
            result.error = f"bird-cli failed: {e}"
        
        # Fallback to Jina Reader
        try:
            jina_url = f"https://r.jina.ai/{url}"
            response = requests.get(jina_url, timeout=15)
            if response.status_code == 200 and response.text:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
        
        result.success = False
        return result


class YouTubeExtractor:
    """Extract content from YouTube"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='youtube')
        
        try:
            # Get metadata with yt-dlp
            cmd = subprocess.run(
                ['yt-dlp', '--dump-json', '--no-download', url],
                capture_output=True, text=True, timeout=30
            )
            
            if cmd.returncode == 0 and cmd.stdout:
                data = json.loads(cmd.stdout)
                result.title = data.get('title')
                result.author = data.get('uploader')
                result.date = data.get('upload_date')
                result.raw_content = data.get('description', '')
                result.video_url = url
                result.extraction_method = 'yt-dlp-metadata'
                
                if data.get('subtitles') or data.get('automatic_captions'):
                    result.raw_content += "\n\n[Subtitles available - call yt-dlp separately for full transcript]"
                
                result.success = True
                return result
        except Exception as e:
            result.error = f"yt-dlp failed: {e}"
        
        # Fallback to Jina Reader
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
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
        
        match = re.search(r'item\?id=(\d+)', url)
        if not match:
            result.error = "Could not parse HN item ID"
            result.success = False
            return result
        
        item_id = match.group(1)
        
        try:
            api_url = f"https://hacker-news.firebaseio.com/v0/item/{item_id}.json"
            response = requests.get(api_url, timeout=10)
            
            if response.status_code == 200:
                data = response.json()
                result.title = data.get('title')
                result.author = data.get('by')
                result.date = datetime.fromtimestamp(data.get('time', 0)).isoformat() if data.get('time') else None
                
                content_parts = []
                if data.get('title'):
                    content_parts.append(f"# {data['title']}")
                if data.get('text'):
                    content_parts.append(data['text'])
                if data.get('url'):
                    content_parts.append(f"\nExternal URL: {data['url']}")
                
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
        
        try:
            # Use Reddit's JSON API (append .json to URL)
            json_url = url.rstrip('/') + '.json'
            response = requests.get(json_url, headers={'User-Agent': 'RustClaw/1.0'}, timeout=10)
            
            if response.status_code == 200:
                data = response.json()
                # Reddit returns [post_data, comments_data]
                post = data[0]['data']['children'][0]['data']
                
                result.title = post.get('title')
                result.author = post.get('author')
                result.date = datetime.fromtimestamp(post.get('created_utc', 0)).isoformat() if post.get('created_utc') else None
                
                content_parts = [f"# {post.get('title', 'Untitled')}"]
                if post.get('selftext'):
                    content_parts.append(post['selftext'])
                if post.get('url') and post['url'] != url:
                    content_parts.append(f"\nLinked URL: {post['url']}")
                
                result.raw_content = "\n\n".join(content_parts)
                result.extraction_method = 'reddit-json-api'
                result.success = True
                return result
        except Exception as e:
            result.error = f"Reddit JSON API failed: {e}"
        
        # Fallback to Jina Reader
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
            result.success = False
        
        return result


class XHSExtractor:
    """Extract content from 小红书 (Xiaohongshu)"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='xhs')
        
        # Phase 1: Use Jina Reader (limited effectiveness due to anti-crawl)
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
            if response.status_code == 200 and len(response.text) > 100:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"Jina Reader failed: {e}"
        
        # If Jina fails, mark as partial success with helpful error
        result.error = "Phase 1 limitation: 小红书 image content requires vision model (planned Phase 2)"
        result.success = False
        return result


class WeChatExtractor:
    """Extract content from WeChat Official Accounts"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='wechat')
        
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"Jina Reader failed: {e}"
            result.success = False
        
        return result


class GitHubExtractor:
    """Extract content from GitHub"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='github')
        
        # Parse GitHub URL to determine type
        match = re.search(r'github\.com/([^/]+)/([^/]+)/?$', url)
        if match:
            # Repository page
            owner, repo = match.groups()
            try:
                # Get README
                readme_url = f"https://raw.githubusercontent.com/{owner}/{repo}/HEAD/README.md"
                readme_response = requests.get(readme_url, timeout=10)
                
                # Get metadata
                api_url = f"https://api.github.com/repos/{owner}/{repo}"
                api_response = requests.get(api_url, timeout=10)
                
                content_parts = []
                if api_response.status_code == 200:
                    data = api_response.json()
                    result.title = data.get('full_name')
                    result.author = data.get('owner', {}).get('login')
                    content_parts.append(f"# {data.get('full_name')}")
                    content_parts.append(f"Description: {data.get('description', 'N/A')}")
                    content_parts.append(f"Stars: {data.get('stargazers_count', 0)} | Language: {data.get('language', 'N/A')}")
                
                if readme_response.status_code == 200:
                    content_parts.append(f"\n## README\n\n{readme_response.text[:3000]}")
                
                result.raw_content = "\n\n".join(content_parts)
                result.extraction_method = 'github-api'
                result.success = True
                return result
            except Exception as e:
                result.error = f"GitHub API failed: {e}"
        
        # Fallback to Jina Reader for issues, discussions, etc.
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"All methods failed: {e}"
            result.success = False
        
        return result


class GenericExtractor:
    """Fallback extractor for unknown platforms"""
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        result = ExtractionResult(url=url, canonical_url=url, platform='other')
        
        try:
            response = requests.get(f"https://r.jina.ai/{url}", timeout=15)
            if response.status_code == 200:
                result.raw_content = response.text
                result.extraction_method = 'jina-reader'
                result.success = True
                return result
        except Exception as e:
            result.error = f"Jina Reader failed: {e}"
            result.success = False
        
        return result


class ContentExtractor:
    """Main extraction orchestrator"""
    
    EXTRACTORS = {
        'twitter': TwitterExtractor,
        'youtube': YouTubeExtractor,
        'hn': HackerNewsExtractor,
        'reddit': RedditExtractor,
        'xhs': XHSExtractor,
        'wechat': WeChatExtractor,
        'github': GitHubExtractor,
    }
    
    @staticmethod
    def extract(url: str) -> ExtractionResult:
        """Main extraction entry point"""
        # Step 1: Resolve short links
        resolved_url = ShortLinkResolver.resolve(url)
        
        # Step 2: Detect platform
        platform = PlatformDetector.detect(resolved_url)
        
        # Step 3: Extract with platform-specific extractor
        extractor = ContentExtractor.EXTRACTORS.get(platform, GenericExtractor)
        result = extractor.extract(resolved_url)
        
        # Step 4: Normalize URL and compute hash
        result.canonical_url = URLNormalizer.normalize(resolved_url)
        result.url_hash = URLNormalizer.get_hash(result.canonical_url)
        
        return result


def main():
    parser = argparse.ArgumentParser(description='Social Media Intake - Content Extraction Engine')
    parser.add_argument('url', help='URL to extract content from')
    parser.add_argument('--json', action='store_true', help='Output as JSON (default: human-readable)')
    parser.add_argument('--dedup-check', action='store_true', help='Only output URL hash for deduplication')
    
    args = parser.parse_args()
    
    # Dedup check mode
    if args.dedup_check:
        resolved = ShortLinkResolver.resolve(args.url)
        canonical = URLNormalizer.normalize(resolved)
        url_hash = URLNormalizer.get_hash(canonical)
        print(url_hash)
        return 0
    
    # Full extraction
    try:
        result = ContentExtractor.extract(args.url)
        
        if args.json:
            print(json.dumps(result.to_dict(), indent=2, ensure_ascii=False))
        else:
            # Human-readable output
            print(f"URL: {result.url}")
            print(f"Platform: {result.platform}")
            print(f"Method: {result.extraction_method}")
            print(f"Hash: {result.url_hash}")
            print(f"Success: {result.success}")
            if result.title:
                print(f"Title: {result.title}")
            if result.author:
                print(f"Author: {result.author}")
            if result.date:
                print(f"Date: {result.date}")
            if result.error:
                print(f"Error: {result.error}")
            print(f"\n--- Content ({len(result.raw_content)} chars) ---")
            print(result.raw_content[:500])
            if len(result.raw_content) > 500:
                print("... (truncated)")
        
        return 0 if result.success else 1
    
    except Exception as e:
        print(f"Fatal error: {e}", file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
