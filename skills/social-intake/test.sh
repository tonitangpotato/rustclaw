#!/bin/bash
# Test script for social-intake extraction engine

set -e

echo "=== Social Media Intake - Test Suite ==="
echo ""

# Test 1: Platform Detection
echo "Test 1: Platform Detection"
echo "--------------------------"
urls=(
    "https://twitter.com/test"
    "https://youtube.com/watch?v=123"
    "https://news.ycombinator.com/item?id=123"
    "https://reddit.com/r/test/comments/123"
    "https://xhslink.com/abc"
    "https://mp.weixin.qq.com/s/abc"
    "https://github.com/test/repo"
    "https://example.com"
)

for url in "${urls[@]}"; do
    platform=$(python3 intake.py "$url" --json 2>/dev/null | jq -r '.platform')
    echo "  $url → $platform"
done

echo ""

# Test 2: URL Normalization (Dedup)
echo "Test 2: URL Normalization & Dedup"
echo "----------------------------------"
echo "  Original: https://example.com?utm_source=twitter&ref=abc"
hash1=$(python3 intake.py "https://example.com?utm_source=twitter&ref=abc" --dedup-check 2>/dev/null)
echo "  Hash: $hash1"

echo "  Cleaned:  https://example.com"
hash2=$(python3 intake.py "https://example.com" --dedup-check 2>/dev/null)
echo "  Hash: $hash2"

if [ "$hash1" == "$hash2" ]; then
    echo "  ✓ Tracking params removed correctly"
else
    echo "  ✗ Hash mismatch!"
fi

echo ""

# Test 3: Real Extraction (HN - no auth needed)
echo "Test 3: Real Extraction (Hacker News)"
echo "--------------------------------------"
result=$(python3 intake.py "https://news.ycombinator.com/item?id=40000000" --json 2>/dev/null)
platform=$(echo "$result" | jq -r '.platform')
method=$(echo "$result" | jq -r '.extraction_method')
success=$(echo "$result" | jq -r '.success')
author=$(echo "$result" | jq -r '.author')

echo "  Platform: $platform"
echo "  Method: $method"
echo "  Success: $success"
echo "  Author: $author"

if [ "$success" == "true" ]; then
    echo "  ✓ Extraction successful"
else
    echo "  ✗ Extraction failed"
fi

echo ""

# Test 4: GitHub Extraction
echo "Test 4: GitHub Extraction"
echo "-------------------------"
result=$(python3 intake.py "https://github.com/rust-lang/rust" --json 2>/dev/null)
platform=$(echo "$result" | jq -r '.platform')
method=$(echo "$result" | jq -r '.extraction_method')
title=$(echo "$result" | jq -r '.title')
success=$(echo "$result" | jq -r '.success')

echo "  Platform: $platform"
echo "  Method: $method"
echo "  Title: $title"
echo "  Success: $success"

if [ "$success" == "true" ]; then
    echo "  ✓ GitHub extraction successful"
else
    echo "  ✗ GitHub extraction failed"
fi

echo ""

# Test 5: Fallback (Generic URL)
echo "Test 5: Generic URL Fallback"
echo "-----------------------------"
result=$(python3 intake.py "https://example.com" --json 2>/dev/null)
platform=$(echo "$result" | jq -r '.platform')
method=$(echo "$result" | jq -r '.extraction_method')
success=$(echo "$result" | jq -r '.success')

echo "  Platform: $platform"
echo "  Method: $method"
echo "  Success: $success"

if [ "$method" == "jina-reader" ]; then
    echo "  ✓ Fallback to Jina Reader successful"
else
    echo "  ✗ Unexpected method: $method"
fi

echo ""
echo "=== Test Suite Complete ==="
