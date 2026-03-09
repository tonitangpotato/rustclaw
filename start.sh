#!/bin/bash
# Start RustClaw with secrets from .env and auth-profiles.json
set -e

cd "$(dirname "$0")"

# Load .env
source .env

# Extract OAuth token from OpenClaw auth-profiles
export ANTHROPIC_AUTH_TOKEN=$(python3 -c "
import json
d = json.load(open('$ANTHROPIC_AUTH_TOKEN_FILE'))
print(d['profiles']['anthropic:default']['token'])
")

# Export bot token
export RUSTCLAW_BOT_TOKEN

echo "Starting RustClaw..."
echo "  Auth: OAuth token (${ANTHROPIC_AUTH_TOKEN:0:15}...)"
echo "  Bot: @rustblawbot"

exec ./target/release/rustclaw run -c rustclaw.yaml
