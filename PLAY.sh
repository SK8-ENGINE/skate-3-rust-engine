#!/usr/bin/env bash
# Linux counterpart of PLAY.bat / scripts/Launch.ps1. Opens your saved map
# (University by default); pass a .skate file to open that map instead.
set -euo pipefail
map=${1:+$(realpath "$1")}
cd "$(dirname "$0")"
[ -x bin/skate3rust ] || { echo "Game is not built. Run ./BUILD.sh first." >&2; exit 1; }

# Same order as scripts/Resolve-DevAssets.ps1; explicit overrides fail clearly
# instead of silently selecting another install.
resolve_assets() {
    if [ -n "${SKATE3_ASSETS:-}" ]; then
        [ -f "$SKATE3_ASSETS/private/game.json" ] || { echo "Prepared assets not found at: $SKATE3_ASSETS" >&2; return 1; }
        realpath "$SKATE3_ASSETS"; return
    fi
    if [ -f assets/private/game.json ]; then realpath assets; return; fi
    for base in data ../data; do
        [ -f "$base/installation.json" ] || continue
        directory=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1],encoding="utf-8-sig"))["directory"])' "$base/installation.json")
        [[ $directory =~ ^installations/[0-9a-f]{32}$ ]] || { echo "Invalid installation directory in $base/installation.json" >&2; return 1; }
        if [ -f "$base/$directory/assets/private/game.json" ]; then realpath "$base/$directory/assets"; return; fi
    done
    echo "Prepared assets are missing. Run ./SETUP.sh with your Skate 3 ISO or default.xex, or set SKATE3_ASSETS." >&2
    return 1
}
assets=$(resolve_assets)
args=(--assets "$assets")
if [ -n "$map" ]; then
    [[ $map == *.skate ]] || { echo "Not a .skate map file: $map" >&2; exit 1; }
    args+=(--map "$map")
fi
mkdir -p logs mods
log=logs/game-$(date +%Y%m%d-%H%M%S).log
echo "Starting Skate 3 Rust Engine. Use a gamepad; Esc opens difficulty/graphics/pause settings."
echo "Assets: $assets"
[ -n "$map" ] && echo "Map: $map"
echo "Log: $log"
# Dev packages live in repo mods/; the game otherwise only looks beside bin/.
export SKATE3_MODS=$PWD/mods
status=0
bin/skate3rust "${args[@]}" >"$log" 2>"${log%.log}.stderr.log" || status=$?
if [ $status -ne 0 ]; then
    tail -n 30 "${log%.log}.stderr.log"
    echo "Game exited with code $status. Log: ${log%.log}.stderr.log" >&2
    exit $status
fi
