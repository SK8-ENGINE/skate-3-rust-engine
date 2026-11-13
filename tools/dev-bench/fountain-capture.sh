#!/usr/bin/env bash
# Dev only: one muted --verify capture with the fountain-cam mod (Better Water basin / canal look checks).
# Usage: fountain-capture.sh <map> <teleport id> <out.png>   (the mod is mods/fountain-cam; edit its SPOT per capture)
. "$(dirname "$0")/common.sh"
I="$INST"
cd "$R" && SKATE_REPORT_CHILD=1 SKATE_VERIFY_AT=${AT:-14} SKATE3_MODS="$DEV_BENCH/mods" \
  timeout 180 ./bin/skate3rust.exe --assets "$I/assets" --map "$I/maps/$1.skate" --teleport "$2" --mute --verify "$3" > "$3.log" 2>&1
echo "exit=$?"; sed 's/\x1b\[[0-9;]*m//g' "$3.log" | grep -a -E "Lua \[|GAME_VERIFY" | head -3
