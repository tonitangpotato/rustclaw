#!/usr/bin/env bash
# rolling-restart.sh — rebuild RustClaw and rolling-restart all three agents
#
# Usage:
#   ./scripts/rolling-restart.sh              # rebuild + restart all
#   ./scripts/rolling-restart.sh --no-build   # skip cargo build, just restart
#   ./scripts/rolling-restart.sh --only main  # restart only one (main|agent2|marketing)
#   ./scripts/rolling-restart.sh --gap 5      # wait N seconds between restarts (default 3)

set -euo pipefail

# ---- config ----
REPO="/Users/potato/rustclaw"
BINARY="$REPO/target/release/rustclaw"

# label : pretty-name pairs
AGENTS=(
  "com.rustclaw.agent:main"
  "com.rustclaw.agent2:agent2"
  "com.rustclaw.agent-marketing:marketing"
)

# ---- args ----
DO_BUILD=1
ONLY=""
GAP=3

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) DO_BUILD=0; shift ;;
    --only)     ONLY="$2"; shift 2 ;;
    --gap)      GAP="$2"; shift 2 ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *) echo "unknown arg: $1" >&2; exit 1 ;;
  esac
done

log() { printf '\033[1;36m[rolling-restart]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*"; }
err() { printf '\033[1;31m[err]\033[0m %s\n' "$*" >&2; }

# ---- build ----
if [[ $DO_BUILD -eq 1 ]]; then
  log "cargo build --release in $REPO"
  ( cd "$REPO" && cargo build --release )
else
  log "skipping build (--no-build)"
fi

if [[ ! -x "$BINARY" ]]; then
  err "binary not found or not executable: $BINARY"
  exit 1
fi

BIN_MTIME=$(stat -f '%Sm' -t '%Y-%m-%d %H:%M:%S' "$BINARY")
BIN_SHA=$(shasum -a 256 "$BINARY" | awk '{print $1}' | cut -c1-12)
log "binary: $BINARY"
log "  mtime: $BIN_MTIME   sha256[0..12]: $BIN_SHA"

# ---- restart loop ----
restart_one() {
  local label="$1" name="$2"
  log "→ restarting $name ($label)"

  # kickstart -k restarts the service if loaded; fall back to stop/start.
  if launchctl kickstart -k "gui/$UID/$label" 2>/dev/null; then
    log "  kickstart ok"
  else
    warn "  kickstart failed, trying stop+start"
    launchctl stop  "$label" 2>/dev/null || true
    sleep 1
    launchctl start "$label" 2>/dev/null || {
      err "  failed to start $label"
      return 1
    }
  fi

  # brief health check: is it running?
  sleep 1
  local pid
  pid=$(launchctl list | awk -v l="$label" '$3==l{print $1}')
  if [[ -n "$pid" && "$pid" != "-" ]]; then
    log "  $name up, pid=$pid"
  else
    warn "  $name has no pid yet (may still be starting)"
  fi
}

restarted=0
for entry in "${AGENTS[@]}"; do
  label="${entry%%:*}"
  name="${entry##*:}"
  if [[ -n "$ONLY" && "$ONLY" != "$name" ]]; then
    continue
  fi
  restart_one "$label" "$name"
  restarted=$((restarted+1))
  if [[ -n "$ONLY" ]]; then break; fi
  sleep "$GAP"
done

if [[ $restarted -eq 0 ]]; then
  err "no agents matched --only=$ONLY"
  exit 1
fi

log "done. restarted $restarted agent(s)."
