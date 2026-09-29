#!/usr/bin/env bash
# macOS launcher. Mirrors scripts/Launch.ps1 without Windows assumptions.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

EXE="bin/skate3rust"
if [[ ! -x "$EXE" ]]; then EXE="target/debug/skate3rust"; fi
if [[ ! -x "$EXE" ]]; then
  echo "Game is not built. Run ./scripts/build-macos.sh first." >&2
  exit 1
fi

# Resolve converted assets: a conversion base holds installation.json
# pointing at the current installation; a bare prepared-asset dir is used
# as-is. (Passing the base itself fails with missing input.cfg.)
ASSETS="$ROOT/assets"
if [[ -f "$ASSETS/installation.json" ]]; then
  INSTALL_DIR="$(ASSETS="$ASSETS" python3 -c "import json,os; print(json.load(open(os.environ['ASSETS']+'/installation.json'))['directory'])")"
  ASSETS="$ASSETS/$INSTALL_DIR/assets"
fi

# Optional map argument: ./scripts/launch-macos.sh /path/to/University.skate
ARGS=(--assets "$ASSETS")
if [[ $# -gt 0 ]]; then
  case "$1" in
    *.skate) ARGS=(--assets "$ASSETS" --map "$1"); shift;;
    --*) ARGS=("$@"); set --;;
  esac
fi
if [[ $# -gt 0 && "${1:-}" == *.skate ]]; then ARGS+=(--map "$1"); fi

mkdir -p logs
LOG="logs/game-$(date +%Y%m%d-%H%M%S).log"
echo "Starting Skate 3 Rust Engine (Metal). Controller, or keyboard (see docs/macos.md); Esc opens menus."
echo "Log: $LOG"
"$EXE" "${ARGS[@]}" 2>&1 | tee "$LOG"
