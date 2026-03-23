#!/bin/bash
# Start RustClaw
# OAuth tokens are managed dynamically from macOS Keychain (auto-refresh)
set -e

cd "$(dirname "$0")"

# Load .env for non-OAuth secrets
source .env

# Export bot token
export RUSTCLAW_BOT_TOKEN

echo "Starting RustClaw..."
echo "  Auth: OAuth from macOS Keychain (auto-refresh)"
echo "  Bot: @rustblawbot"

exec ./target/release/rustclaw run -c rustclaw.yaml
