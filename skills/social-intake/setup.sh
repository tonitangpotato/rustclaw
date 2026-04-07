#!/bin/bash
# Setup script for social-intake skill
# Run this once to install dependencies

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "🔧 Setting up social-intake skill..."

# Check Python version
if ! command -v python3 &> /dev/null; then
    echo "❌ Error: python3 not found. Please install Python 3.8+"
    exit 1
fi

PYTHON_VERSION=$(python3 --version | cut -d' ' -f2 | cut -d'.' -f1-2)
echo "✓ Found Python $PYTHON_VERSION"

# Install Python dependencies
echo "📦 Installing Python dependencies..."
pip3 install --user -r requirements.txt

# Check optional tools
echo ""
echo "🔍 Checking optional extraction tools:"

if command -v yt-dlp &> /dev/null; then
    echo "✓ yt-dlp found (for YouTube extraction)"
else
    echo "⚠️  yt-dlp not found - YouTube videos will use fallback method"
    echo "   Install with: pip3 install --user yt-dlp"
fi

if command -v npx &> /dev/null; then
    echo "✓ npx found (for Twitter extraction)"
else
    echo "⚠️  npx not found - Twitter will use fallback method"
    echo "   Install Node.js from: https://nodejs.org/"
fi

if command -v curl &> /dev/null; then
    echo "✓ curl found (for link resolution)"
else
    echo "❌ curl required but not found"
    exit 1
fi

# Make intake.py executable
chmod +x intake.py
echo "✓ Made intake.py executable"

# Create intake directory structure
mkdir -p ../../intake/{twitter,youtube,hn,reddit,github,xhs,wechat,other}
echo "✓ Created intake directory structure"

echo ""
echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "  1. Ensure RustClaw has engram_recall and engram_store functions"
echo "  2. Test with: python3 intake.py 'https://news.ycombinator.com/item?id=1' --json"
echo "  3. Load skill in RustClaw and send a social media URL via Telegram"
