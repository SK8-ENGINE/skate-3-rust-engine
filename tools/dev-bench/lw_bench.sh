#!/usr/bin/env bash
# Muted living-world frame-time bench (docs/hails-additions/34-physics-performance.md): release exe, University
# library, the player steps off the board, NPC Skater Physics on, SYSTEM_TIMING + FIXED_TIMING + LIVING_TIMING + a
# frame log; then a summary.
# usage: lw_bench.sh <exe> <out-prefix> [seconds]   (settings: common.sh; MAP=, TELEPORT=, RADIUS=, STEP_OFF=, EXTRA=)
# PLAIN=1: no timing readouts (they keep the system spans on); frame log only.
EXE="${1:-$EXE}"; OUT="$2"; SECS="${3:-150}"
. "$(dirname "$0")/common.sh"
EXE="${EXE:-$R/bin/skate3rust.exe}"
bench_guard
bench_path "$EXE"
[ -z "$PLAIN" ] && export SKATE_SYSTEM_TIMING=1 SKATE_FIXED_TIMING=1
mkdir -p "$(dirname "$OUT")"
cd "${RUN_DIR:-$R}"
rm -f "$OUT.disturbed"
( while sleep 5; do n=$(tasklist | grep -ci "skate3rust\|skate3recomp\|rexglue\|rpcs3"); [ "$n" -gt 1 ] && echo "$(date +%T) $n game processes" >> "$OUT.disturbed"; done ) &
WATCH=$!
SKATE_TEST_STEP_OFF=${STEP_OFF:-300} SKATE_REPORT_CHILD=1 SKATE_NPC_SIM=1 SKATE_NPC_SIM_RADIUS=${RADIUS:-150} \
SKATE3_MODS="$(cygpath -w "$EMPTY_MODS")" \
SKATE_LIVING_WORLD_DEBUG=1 SKATE_FRAME_LOG="$(cygpath -w "$OUT.frames.tsv")" \
  timeout "$SECS" "$EXE" --assets "$(cygpath -w "$INST/assets")" --map "$(cygpath -w "$INST/maps/${MAP:-University}.skate")" \
  --teleport "${TELEPORT:-dist_university_library}" --mute $EXTRA > "$OUT.out" 2> "$OUT.err"
echo "exit $?"
kill $WATCH 2>/dev/null
[ -s "$OUT.disturbed" ] && echo "DISTURBED: another game ran during this run ($(wc -l < "$OUT.disturbed") checks); discard it"
L="$OUT.err"
echo "panics: $(grep -ci 'panicked' "$L")  slots full: $(grep -c 'name slots used' "$L")"
grep "LIVING_WORLD tick" "$L" | tail -1 | sed 's/\x1b\[[0-9;]*m//g' | grep -o "skaters [0-9]* peds [0-9]* vehicles [0-9]*"
py -3.13 "$R/tools/claude-skills/optimisation/tools/frame_log_summary.py" "$OUT.frames.tsv" | sed -n 1,5p
grep "LIVING_TIMING" "$L" | tail -n +20 | sed 's/\x1b\[[0-9;]*m//g' | grep -o "| .*" | tr ' ' '\n' | grep = |
  awk -F= '{s[$1]+=$2; n[$1]++} END {for (k in s) printf "  %-17s %.3f ms/frame\n", k, s[k]/n[k]}'
grep "FIXED_TIMING " "$L" | tail -n +20 | sed 's/\x1b\[[0-9;]*m//g' | grep -o "step_ms=[0-9.]*\|busy_ms=[0-9.]*\|gaps_ms=[0-9.]*" |
  awk -F= '{s[$1]+=$2; n[$1]++} END {for (k in s) printf "  fixed %-8s %.3f ms/step\n", k, s[k]/n[k]}'
