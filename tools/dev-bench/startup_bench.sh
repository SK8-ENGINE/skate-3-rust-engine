#!/usr/bin/env bash
# Start-up timeline bench (docs/hails-additions/08-setup-performance.md#start-up-pass-2026-11-03-user-asks-471):
# a muted start, the STARTUP_STEP lines (ms since main) and the first frames' costs.
# usage: startup_bench.sh <exe> <out-prefix> [seconds]   (settings: common.sh; MAP= default University, TELEPORT=)
# COLD=1 starts with a cold file cache (evict.ps1; EVICT_SKIP=<folder names>, default speech); TIMING=1 adds
# SKATE_SYSTEM_TIMING (the first SYSTEM_TIMING line = the Startup systems); EXTRA= more game arguments.
EXE="${1:-$EXE}"; OUT="$2"; SECS="${3:-40}"
. "$(dirname "$0")/common.sh"
EXE="${EXE:-$R/bin/skate3rust.exe}"
bench_guard
bench_path "$EXE"
[ -n "$TIMING" ] && export SKATE_SYSTEM_TIMING=1
[ -n "$COLD" ] && EVICT_SKIP="${EVICT_SKIP-speech}" powershell -NoProfile -ExecutionPolicy Bypass -File "$DEV_BENCH/evict.ps1" "$(cygpath -w "$INST")" "$(cygpath -w "$(dirname "$EXE")")"
mkdir -p "$(dirname "$OUT")"
cd "${RUN_DIR:-$R}"
SKATE_TEST_STEP_OFF=300 SKATE_REPORT_CHILD=1 SKATE3_MODS="$(cygpath -w "$EMPTY_MODS")" \
  timeout "$SECS" "$EXE" --assets "$(cygpath -w "$INST/assets")" --map "$(cygpath -w "$INST/maps/${MAP:-University}.skate")" \
  --teleport "${TELEPORT:-dist_university_library}" --mute $EXTRA > "$OUT.out" 2> "$OUT.err"
echo "exit $?  panics: $(grep -ci 'panicked' "$OUT.err")"
sed 's/\x1b\[[0-9;]*m//g' "$OUT.err" | grep -o "STARTUP_STEP.*\|STARTUP_LOADING done.*\|FRAME_HITCH frame=[1-5] ms=[0-9.]* .*main_ms=[0-9.]*" | sed 's/^/  /'
grep -o "map_fingerprint[^ ]* [0-9a-f]*" "$OUT.err" | head -1 | sed 's/^/  /'
if [ -n "$TIMING" ]; then
  sed 's/\x1b\[[0-9;]*m//g' "$OUT.err" | grep -m1 "SYSTEM_TIMING frames" | grep -o "slow .*" | tr ' ' '\n' | sed -n '2,26p' | sed 's/^/    /'
fi
