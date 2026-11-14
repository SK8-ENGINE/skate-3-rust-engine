#!/usr/bin/env bash
# Muted NPC Skater Physics run with the player idle (MAP=<name>, default DownTown; TELEPORT=<teleports.json id>, e.g.
# dist_university_library), then a summary of the NPC log lines.
# SKATE_REPORT_CHILD=1: no crash-report supervisor, so the timeout kill opens no diagnostic report window.
# usage: npcsim.sh <exe> <out-prefix> [seconds]   (settings: common.sh; MODS=<mods folder>, default none; RADIUS=, STEP_OFF=, EXTRA=)
EXE="${1:-$EXE}"; OUT="$2"; SECS="${3:-150}"
. "$(dirname "$0")/common.sh"
EXE="${EXE:-$R/bin/skate3rust.exe}"
bench_guard
bench_path "$EXE"
mkdir -p "$(dirname "$OUT")"
cd "${RUN_DIR:-$R}"
SKATE_TEST_STEP_OFF=${STEP_OFF:-300} SKATE_REPORT_CHILD=1 SKATE_NPC_SIM=1 SKATE_NPC_SIM_RADIUS=${RADIUS:-150} SKATE3_MODS="$(cygpath -w "${MODS:-$EMPTY_MODS}")" \
  timeout "$SECS" "$EXE" --assets "$(cygpath -w "$INST/assets")" --map "$(cygpath -w "$INST/maps/${MAP:-DownTown}.skate")" ${TELEPORT:+--teleport "$TELEPORT"} --mute $EXTRA \
  > "$OUT.out" 2> "$OUT.err"
echo "exit $?"
L="$OUT.err"
echo "replays:   $(grep -c '<- replay' "$L")"
echo "bails:     $(grep -c 'NPC_SKATER_BAIL' "$L")"
echo "path:      $(grep -c 'NPC_SKATER_PATH' "$L")"
echo "sim loads: $(grep -c 'NPC_SKATER_SIM_LOAD' "$L")"
echo "hitch>100: $(grep -o "FRAME_HITCH frame=[0-9]* ms=[0-9.]*" "$L" | awk -F= '{ if ($3+0 > 100) n++ } END { print n+0 }')"
echo "panics:    $(grep -ci 'panicked' "$L")"
grep 'NPC_SKATER_BAIL' "$L" | grep -o '[A-Za-z]* -> [A-Za-z]*' | sort | uniq -c | sort -rn | head
