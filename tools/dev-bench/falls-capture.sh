#!/usr/bin/env bash
# Dev only: one muted --verify capture of a DownTown spot with a fixed camera (Better Water falls checks, ask 395).
# Usage: falls-capture.sh <out.png> <look on|off> "<px,py,pz>" "<ex,ey,ez>" "<lx,ly,lz>" [checkout with bin/, default this one]
# EXTRA="body_calming = false" adds water_look keys for the run.
. "$(dirname "$0")/common.sh"
I="$INST"
B=${6:-"$R"}
MODS="$R/.local/dev-bench/falls-mods"; M="$MODS/falls-cam"; mkdir -p "$M"
[ "$2" = on ] && E=true || E=false
cat > "$M/mod.json" <<'J'
{ "id": "dev-falls-cam", "api": 2, "name": "Falls camera (dev)", "version": "0.1.0", "author": "Hails",
  "description": "Dev only: fixed camera for --verify captures of falls (written per run by falls-capture.sh).", "entry": "main.lua" }
J
cat > "$M/main.lua" <<L
local SPOT = { player = { $3 }, eye = { $4 }, look = { $5 } }
local frames = 0
return {
  on_update = function(event)
    frames = frames + 1
    if frames == 120 then
      sdk.graphics.water_look({ enabled = $E${EXTRA:+, $EXTRA} })
      sdk.player.teleport({ position = SPOT.player, heading = 0, velocity = { 0, 0, 0 } })
    end
    if frames >= 120 then sdk.camera.set(SPOT.eye, SPOT.look) end
  end,
}
L
cd "$B" && SKATE_REPORT_CHILD=1 SKATE_VERIFY_AT=${AT:-14} SKATE3_MODS="$MODS" \
  timeout 180 ./bin/skate3rust.exe --assets "$I/assets" --map "$I/maps/DownTown.skate" --teleport dist_downtown_memorial --mute --verify "$1" > "$1.log" 2>&1
echo "exit=$?"; sed 's/\x1b\[[0-9;]*m//g' "$1.log" | grep -a -E "Lua \[|GAME_VERIFY" | head -3
