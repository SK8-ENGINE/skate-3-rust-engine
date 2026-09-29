#!/usr/bin/env bash
# Deterministic spawn-idle benchmark for macOS: records a fixed capture with
# no input, then prints frame-time stats. Same spawn + settings every run,
# so the numbers are A/B-comparable across engine or settings changes.
# Usage: ./scripts/bench-macos.sh [seconds=20] [map.skate]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SECONDS_CAPTURE="${1:-20}"
MAP="${2:-}"

EXE="bin/skate3rust"
if [[ ! -x "$EXE" ]]; then EXE="target/debug/skate3rust"; fi
if [[ ! -x "$EXE" ]]; then
  echo "Game is not built. Run ./scripts/build-macos.sh first." >&2
  exit 1
fi

ASSETS="$ROOT/assets"
if [[ -f "$ASSETS/installation.json" ]]; then
  INSTALL_DIR="$(ASSETS="$ASSETS" python3 -c "import json,os; print(json.load(open(os.environ['ASSETS']+'/installation.json'))['directory'])")"
  ASSETS="$ASSETS/$INSTALL_DIR/assets"
fi
if [[ ! -f "$ASSETS/private/game.json" ]]; then
  echo "Converted assets missing at $ASSETS. Prepare them per docs/macos.md first." >&2
  exit 1
fi

TRACE="/tmp/sk8bench-$$.json"
rm -f "$TRACE"
ARGS=(--assets "$ASSETS")
if [[ -n "$MAP" ]]; then ARGS+=(--map "$MAP"); fi
# Startup + delay + capture + export margin, then stop the game by PID
# (pattern-kill could match unrelated processes; wait reaps quietly).
"$EXE" "${ARGS[@]}" --trace "$TRACE" --trace-delay 10 --trace-seconds "$SECONDS_CAPTURE" > logs/bench-run.log 2>&1 &
GAMEPID=$!
sleep $((45 + SECONDS_CAPTURE))
kill -TERM "$GAMEPID" 2>/dev/null || true
wait "$GAMEPID" 2>/dev/null || true
if [[ ! -f "$TRACE" ]]; then
  echo "Benchmark failed: no capture. See logs/bench-run.log" >&2
  exit 1
fi
python3 tools/analyse_performance_trace.py "$TRACE" | python3 -c "
import json,sys
d = json.load(sys.stdin)
f = d['frame_interval']
to_fps = lambda ms: 1000.0 / ms if ms > 0 else 0.0
print(f\"frames : {f['count']}\")
print(f\"mean   : {f['mean_ms']:.2f} ms  ({to_fps(f['mean_ms']):.0f} FPS)\")
print(f\"p50    : {f['p50_ms']:.2f} ms  ({to_fps(f['p50_ms']):.0f} FPS)\")
print(f\"p95    : {f['p95_ms']:.2f} ms  ({to_fps(f['p95_ms']):.0f} FPS)\")
print(f\"p99    : {f['p99_ms']:.2f} ms  max {f['max_ms']:.2f} ms\")
print('top CPU spans:')
for r in sorted(d['cpu_spans_inclusive'], key=lambda r: -r['total_ms'])[:8]:
    print(f\"  p50={r['p50_ms']:7.2f} ms  {r['category']}/{r['name']}\")
"
