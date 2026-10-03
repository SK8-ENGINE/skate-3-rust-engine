#!/usr/bin/env bash
# Muted crash smoke test: run the game on each map for N seconds (windowed, --mute), then grep the log.
# usage: tools/regression-checks/smoke_maps.sh [seconds] [map ...]
#   default 25 s; default maps: StartPark University DownTown MegaPark
# Environment: SKATE_EXE (default bin/skate3rust.exe), SMOKE_OUT (default .local/smoke).
# Output: <SMOKE_OUT>/<stamp>/<map>.log and one summary line per map (also in summary.txt).
# Runs from Git Bash on Windows (uses taskkill to make sure the game is gone between maps).
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXE="${SKATE_EXE:-$ROOT/bin/skate3rust.exe}"
SECS="${1:-25}"; shift || true
MAPS=("$@"); [ ${#MAPS[@]} -eq 0 ] && MAPS=(StartPark University DownTown MegaPark)
INST=$(ls -d "$ROOT"/data/installations/*/ 2>/dev/null | head -1)
if [ -z "$INST" ] || [ ! -x "$EXE" ]; then echo "need an installation under data/installations and $EXE"; exit 2; fi
OUT="${SMOKE_OUT:-$ROOT/.local/smoke}/$(date +%Y%m%d_%H%M%S)"; mkdir -p "$OUT"
for m in "${MAPS[@]}"; do
  log="$OUT/$m.log"
  SKATE_REPORT_CHILD=1 SKATE3_MODS="$ROOT/mods" timeout "$SECS" "$EXE" --assets "$ROOT/assets" --map "$INST/maps/$m.skate" --mute >"$log" 2>&1
  code=$?
  taskkill //F //IM "$(basename "$EXE")" >/dev/null 2>&1
  clean=$(sed 's/\x1b\[[0-9;]*m//g' "$log")
  panics=$(grep -ciE "panicked|panic" <<<"$clean"); errors=$(grep -c " ERROR " <<<"$clean")
  nonfinite=$(grep -cE "NONFINITE|Non-finite|physics_failed:true" <<<"$clean")
  ready=$(grep -c "SKATE_RENDER_READY" <<<"$clean"); native=$(grep -c "Game audio: native" <<<"$clean")
  echo "$m exit=$code panics=$panics errors=$errors nonfinite=$nonfinite render_ready=$ready native_lines=$native" | tee -a "$OUT/summary.txt"
done
echo "logs: $OUT"
