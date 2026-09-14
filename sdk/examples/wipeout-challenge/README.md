# Wipeout Challenge 3.1

Requires the updated game build with `player_overlap` capability. Enable the
mod, then use **Gamemodes > Wipeout**: Start, Restart at start pad, or Stop and
return. T starts an idle run; R restarts. Each local run is a personal survival
challenge, not a shared multiplayer race or leaderboard.

Start builds the supplied four-pad sky course 40m above the saved starting
location. Restart reuses that anchor rather than lifting another 40m. Stop and
unload restore the original position and remove all course bodies and meshes.
A map change removes the course without teleporting into the previous map.

Hazards are persistent kinematic solids moved every fixed tick, not repeatedly
removed/respawned static boxes. Hits use enabled native board/skater collision
shapes through `sdk.physics.read(key).player_overlapping`. The first hazard
entry is forgiven; the next separate entry ends the run. Sustained contact does
not count repeatedly. One second of recovery groups simultaneous impacts.
Water sensors and falling off the course end the run immediately. Water volumes
extend below their original surface to catch fast falls. Timed mode clears after
the selected waves; survival repeats them. The obsolete safe-marker settings
were removed because they never provided a physical safe area.
