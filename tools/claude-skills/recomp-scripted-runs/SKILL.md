---
name: recomp-scripted-runs
description: Run repeatable, unattended gameplay sessions in a local skate3recomp research build to collect data (audio trace, screenshots) - scripted pad input, a recorded teleport route, coordinate teleports, background mode so the machine stays usable. Use whenever retail behaviour must be measured again (sounds per action, timings), instead of performing and announcing actions by hand.
---

# Scripted recomp runs (unattended data collection)

Why: hand-marked sessions ("ollie now", "grinding now") are slow, tiring and imprecise (marks lagged actions by
about 4 s). A research build of the recomp can play a pad script by itself, write exact `MARK` lines into the trace,
screenshot its own window, and run in the background without touching the real controller, focus or speakers.

Rules from skill `recomp-audio-trace` apply: your own legal copy of the game, your own recomp build, no licence on
skate3recomp (local only, never copy its code), credit skate3recomp, rexglue and Xenia. The script player, background
mode and trace hooks are research additions to the recomp (the `research-hooks` branch linked from
`tools/recomp-code-search/README.md` in the repo, or your own equivalent); a stock recomp build does not have them.

## Pieces
| what | where |
|---|---|
| runner (boot -> script -> trace -> stop) | `tools/recomp_script_run.sh` here (Git Bash; needs `RECOMP_EXE`, `RECOMP_GAME_DIR`) |
| window screenshots, dev-build stop | `tools/screen_loop.ps1`, `tools/stop_dev_recomp.ps1` |
| Challenge Map teleport script generator | `tools/make_location_script.py` |
| build helpers | `recomp_{env,configure,build}.bat` (skill `recomp-audio-trace`; reconfigure after adding sources) |
| script player + route recorder (C++) | recomp SDK `third_party/rexglue-sdk/include/rex/input/input_script.h`, called from `src/kernel/xam/xam_input.cpp` |
| trace hooks | recomp `src/research/hooks_*.cpp` + `trace_common.h` (skill `recomp-research`), `rex/audio/audio_trace.h` |
| your scripts / routes / sessions | a gitignored folder, for example `.local/recomp/{scripts,routes,sessions}/` |

## Run
```bash
export RECOMP_EXE=<recomp checkout>/out/build/clang-relwithdebinfo/skate3.exe RECOMP_GAME_DIR=<game-install>
bash <skill>/tools/recomp_script_run.sh <ABSOLUTE script.txt> <ABSOLUTE out/trace.tsv> [max_seconds=120] [autostart_ms=4000] [extra recomp flags]
# env: BACKGROUND=1 (default) | MUTE=true (default) | SHOTS=1 (default; 0 = no screenshots) | SKATE3_TRACE=<categories>
```
- Refuses to start if any `skate3.exe` runs. Stops only the dev build (by exe folder) when `script end` appears or
  time runs out.
- Boots with `--skate3_demo_path --skate3_demo_path_signed_in` (straight to gameplay with your save).
- Outputs next to the trace: `<name>.out/.err` (game log), `<name>_shots/shot_<unix ms>.jpg` (every 2 s), the trace,
  and `<name>.f32` (the audio capture). Look at shots to see what really happened.
- Trace `MARK` lines: `script loaded`, `gameplay active; script autostart...`, `script start`, one per labelled step,
  `script end`. Analyse windows between MARKs (exact, unlike hand marks).
- Put hard time limits on every run and wait loop, and check for leftover processes afterwards. Run one district or
  batch per background job, so a killed loop cannot orphan a recomp.

## Back up the recomp save first
Scripted runs boot the signed-in profile (`--skate3_demo_path_signed_in`), and menus, session markers and settings
changed by a script are saved into it. Before every run copy the profile folder under `%APPDATA%\skate3` (the
profile save is small; the large content folder does not need copying) and hash it; after the run compare. Restore
only from that backup, only while the game is closed, and hash-check after; never try to put settings back by
navigating menus.

## Background mode (default)
`SKATE3_BACKGROUND=1`: window shown without activation at the bottom of the z-order (`window_win.cpp`), real pad
zeroed before the demo path's synthetic presses and keystrokes dropped (`xam_input.cpp`), `--audio_mute` (mutes only
the SDL output; EA's mixer and the hooks still run), 960x540 window, below-normal priority. Screenshots use
`PrintWindow(PW_RENDERFULLCONTENT)` on the recomp window, so they work while it is behind other windows (not when
minimised). GPU/CPU load remains: keep runs short and run one game instance at a time.

## Script format (`SKATE3_INPUT_SCRIPT`)
One step per line: `<ms> [tokens...] [# label]`; `#` lines and blanks ignored; a step without tokens is a neutral
pad. Tokens: `a b x y lb rb start back up down left right l3 r3 lt rt`, sticks `lx= ly= rx= ry=` (-1..1, up/right
positive), and `gameplay` = neutral until gameplay has run `<ms>` without a pause/loading break (put it after a
teleport). Durations can be given in polls (`Np`, about 60 polls/s) so hitches don't shift inputs; start scripts with
`600p gameplay` + `120p` settle and use autostart 8000 on the first load-in. Labels become `MARK` lines. Starts on
L3+R3 on the real pad, or `SKATE3_INPUT_SCRIPT_AUTOSTART_MS` after gameplay has been steady that long.

## Bails on demand
From an on-board spawn near a drop edge (for example the Mega-Park): `y` off the board, turn (`lx=1` for 600 ms) and
walk about 1 s, place a **session marker** (LB opens the marker menu: Go To Marker = up, Place Marker = down, Object
Dropper = B; `300 lb`, `300 lb down`). Per attempt:
1. Go to the marker (`300 lb`, `3000 lb up`; a tap does not respawn), settle about 2.5 s.
2. Run toward the edge for 1.6-2.0 s tapping `a` with `ly=1` (2.4 s overshot), then `300 ly=1 rt`, `400 ly=1 rt l3 r3`
   plus `lt` (both sticks + both triggers): this forces the bail at the edge at once (bail state about 20 ms after the
   input; 6 of 6 in a test run, ordinary-height tumbles). With `rt l3 r3` only, the skater often jumped across and
   stood on the far edge, and the bails that did happen were long free falls. Fallback: `x` 2.5 s later (jump off the
   edge), then hold `l3 r3 lt rt` in the air.
3. Wait about 6 s (the bail starts about 1.8 s after the input on the slow path), `300 a` to get up, 2.5 s neutral.

The game does not respawn while the skater is down or in the arms-out landing stance, so get up first and check a
shot shows them standing. Verify bails from the trace (`FIRSTHIT` state `+676`, `BAILSTEP`), not from the script.
Riding counterpart: a marker on the board at the spawn and a few ~10 s pushing segments (straight, carve right, carve
left); check which segments end against a wall.

## Getting somewhere: record a route once
The route recorder writes a script from the first moment of gameplay while someone navigates by menu to a spot; paste
it at the top of a session script, shorten the first idle, add `6000 gameplay # at <spot>`. Launch the recorder from
Explorer, not a terminal (a terminal crash once took the game down). Arrival can be on foot carrying the board: press
`y` to get on before board actions, and check a shot, because arrival state varies (the same route arrived on board
in one run and on foot in another). The PCU Library route in the demo-path cvar help can start a challenge instead,
depending on the save.

## Teleport anywhere: Challenge Map > Locations
- Menu path from gameplay: `start`, `a` (Challenge Map), `rt`, `rt` (Locations tab), `down` x district, `a`, `down`
  x location, `a`, `a`, `a`, then `4000 gameplay # at <Name>`. Districts: 0 Downtown (14 locations), 1 Industrial
  (12), 2 University (10), then skate.School, skate.Park (8), Maloof Money Cup (2), Black Box Park, and DLC areas.
- Generator: `py -3.13 tools/make_location_script.py <out> <district> <listen s> <i>:<Name>...`.
- Batches of 4-5 locations per recomp boot; chain districts as separate background jobs with hard time limits, never
  two recomps at once. A batch can occasionally die during boot (trace holds only "script loaded"): check each
  batch's MARK lines and re-run batches without `at` marks.
- A second teleport in one boot after a bail stalled a script once; keep bail runs to one teleport per boot.
- A second Challenge Map teleport after placing a session marker did not happen in one run: the menu key sequence
  that worked for the first teleport drifted after the marker menu. Screenshot-check each menu step before relying
  on a repeat, or use one teleport per boot.
- The recomp can run slower than real time under heavy tracing (1500 s of wall time covered about 9 minutes of game
  time in one run): size time limits by game time reached, not by wall time.

## Teleport to any coordinate: `SKATE3_TP_OVERRIDE`
`SKATE3_TP_OVERRIDE="x y z yaw[; x y z yaw ...]"` in the runner's environment: each Challenge Map menu teleport (lr
`8286483C` first / `82864A08` later) lands at the next pose instead of the menu location (the last pose repeats).
Yaw is in degrees, forward = (sin yaw, 0, cos yaw), in the same world coordinates as the engine's teleports. Logs
`TPOVR` in any trace category (lr, menu destination, menu forward x z, override, yaw). Unset = off, no cost.
- Use a menu location in the right district (`make_location_script.py`, any index) for each pose, so the right
  district streams in. Keep the poses in a file next to the evidence.
- Give y a few metres above the ground; the skater drops. Read the settled position from `WPPOS`
  (`SKATE3_TRACE=audio`; its listener is the camera, about 1.7 m behind the pose).
- Points outside the playable area (map rims) ghost the skater and freeze them in place (the out-of-bounds re-place
  loop); the camera view is still fine for scenery shots.

## Verify actions from the trace, not from the script
A scripted input is not proof the action happened. Check POSTs (audio objects) between MARKs; the first caller-chain
address falls inside the object's constructor (from upstream PR #4's notes): `824AF498` foot drag (obj 0),
`824AF678` wheel skid (1), `824AF8C8` grind, `824AFAD8` Class_Flips (4), `824AFDD0` seams (6), `824AFF48`
squeaks/powerslide, `824B0080` treatments (9), `824B0248` rolling rattle (10), `824B0388` speed wind (12),
`824B0520` speed rattle (13), `824B0670` board slide, `824B7070` body slide, `824B71C0` cloth_trick (22), `824B72D8`
cloth falls, `824B73E0` footstep. SPLC (Skate_Collisions / sk8_foley) plays show as `SPLC` lines (bank index, id,
callers). Plus the screenshots. To check whether a particular sample really played, match the `PLAY` line's 48
sample bytes against your own extracted bank files.

## Limits of scripting
- **Replaying a free-form recording does NOT work:** a few minutes of recorded skating, replayed (in ms, then per poll
  with teleport sync), went off course within seconds (the emulated game is not deterministic enough for long
  open-loop replays). Use recordings only for menu routes. For actions: short scripted segments from a fixed spawn,
  each verified by POST objects + screenshots. Identical scripted grind runs ended about 1.3 m apart, so scripted rail
  grinds are luck.
- **Open-loop wandering doesn't work:** scripts can't steer to a target. Rolling, grinds, NPC encounters and traffic
  need **passive sessions**: a person plays the instrumented build normally (sound on, their pad, normal window and
  priority, demo-path boot) while it writes `trace.tsv`, `pad.txt` (recorder), screenshots every 2 s
  (`screen_loop.ps1 -KeepPriority`) and the audio capture. The trace's first line `CLOCK 0.0 <unix ms>` aligns trace
  time with screenshot names. Useful presets: audio (`audio,dsp`), everything for NPC/traffic work
  (`audio,dsp,npc,world,traffic,aiskater`, about 25 MB per minute; peds talking, horns and skids are audio events
  caused by NPC/vehicle state, so trace them together), NPC/world only (`npc,world,traffic`), high-volume audio
  detail (`audio,audiox,dsp`), skitching and junctions (`traffic,skitch,npc`), grinds (`grind`), character records
  (`cas`).
- While someone is playing on the machine, launch nothing that opens a window or steals focus.

## Summarising a session by sound event
A useful first report for an audio session: events = POST (audio object, named by constructor address) and SPLC
(bank index -> bank by majority of attached samples; id -> record/container). Attach each sample start to the latest
compatible event (`.bnk` samples -> an SPLC within 80 ms, other banks -> a POST within 350 ms). Level per voice =
the product of the active GAIN modules' peak target within 0.4 s times the strongest SEND (voices also feed buses at
0; `.abk` voices ramp in, so take peaks, not first values). Group by (event, gameplay caller) and list samples with
median / max level and example times with the nearest screenshot; look at those shots to name unknown groups.

## Real game sound capture
`SKATE3_AUDIO_CAPTURE=<file>` (recomp `src/audio/sdl/sdl_audio_driver.cpp`) appends the final mix as stereo float32
LE 48 kHz, taken before `--audio_mute`; `CAPTURE <frames>` trace lines every ~0.5 s align it to trace time. Captures
lag their trace by roughly 110 ms: search per-hit windows. Use it to cut out retail's real pops/landings and compare
with the engine's render (the trace alone lacks pitch/filters/mix). Never build listening/A-B pages from it; measure
internally.

## Recomp artefacts to discount
- After a teleport the recomp ends the loading screen before the streams finish: for a few seconds the previous
  area's state lingers (for example the audio location set). It does not affect location-set or ambience
  measurements taken after the settle.
- On long teleport runs the character's hair and clothing get left lying on the ground.
- Audio-thread stalls of up to about 1 s happen; voices queued during one start together.
- A stall at the end of a session may not be in the trace (the batched writer and the capture stop first); if it
  recurs, rerun without the heaviest category (`dsp`).

## Closing the recomp and menus from a script
- Escape or F1 (keyboard binds `bind_skate3_menu` / `bind_skate3_menu_alt`) opens the recomp's own settings overlay,
  which has "Quit to the desktop" and is controller-navigable; Escape backs out level by level.
- A research build can add a held pad chord for it: cvars `skate3_menu_pad_chord` (for example `lb+rb+back`;
  `+`-joined from `a b x y lb rb lt rt l3 r3 back start up down left right`; empty = off) and
  `skate3_menu_pad_hold_ms` (1000). It reads the pad the game reads (after the input script), so scripts can trigger
  it; a 0.3 s tap does not open it, a 1.5 s hold does. Start would open Skate 3's own menu; Back alone opens Instant
  Replay, so the replay screen appears underneath while holding (harmless when quitting).
- Closing the process from outside (a launcher's "exit game") is safe for the data: the trace is written
  continuously.
- **Start** in free roam opens the Career > Main menu (Challenge Map, Edit Skaters, ...); a second Start does not
  close it, `b` does. Game logic (frame update `82859E70`) stops while it is open.
- **Back** opens Instant Replay / Replay Editor (a loading screen first; the first use shows a help page). Back inside
  the editor asks "exit the Replay Editor?" with No selected: `down` then `a` to leave. Game logic stops there too.
  Watch lists still log there (skill `recomp-research`).

## Trace line catalogue (field lists: each hook file's header and `recomp-research/tools/trace.py` FIELD_COUNTS)
- Every hook writes one `HOOKARMED` line on its first call (proof of installation).
- Audio (`audio`): `POST` (audio object posts), `SPLC` (Splice plays), `PLAY` / `GAIN` / `SEND` (voices; `PLAY`
  "level" is a counter and `SEND` values aren't gains), `CONTACT` (`sub_824B86E0`: impact speed `+208`, wheel classes
  `+448..+460`, surface `+620` = the engine's audio surface id minus 1), `CSET` (contact set pick: kind, variant, id,
  tier), `GREC` (rolling grain bed per frame, local player, `sub_824C6BD8`), `TREAT` / `SEAMPAT` / `SEAMHIT`
  (Class_Treatment / Class_Seams; last two fields = object and local mask: 4 = local72, the gate), world one-shots
  `WPPOS` (listener, set key, mode, state; every 500 ms and on key change) / `WPSET` / `WPINT` / `WPFIRE`, zone
  ambience `AMBST` / `AMBBED` / `AMBXF`.
- DSP (`dsp`): `MOD <kind> <owner> <module> v1 v2 v3` per-voice PITCH (requested rate, scale, ratio) / LPF / HPF
  (cutoff) / SHELF (corner, gain) / PEAK (freq, gain, Q); owner joins to the `PLAY`/`GAIN` player.
- High-volume audio (`audiox`): `GRECX` (state `+332` word: air, push plant, plant edge bytes; slip `+232`; revert
  `+690`; owner revert counter `+1516`; listener camera position and view direction), `FIRSTHIT` (bail first-hit
  global, state `+676` bail / `+677` end; on change + 1 Hz), `SKID` (the held wheel-skid packet per frame plus a
  release line), `EMITSLOT` (active world emitters, at most 10/s each), bail impacts `BAILLOCAL` / `BAILSTEP` /
  `BAILREG`, `COLLPOST` (every collision-manager message with the local72 test), `PLAYERPOST` (posts, Splice starts
  and wheel stream starts/stops per per-player SFX object, for both Player instances: the local rider and the NPC
  skater within 30 m of the camera; MixMap key `0x400100xx` = instance 0, `0x400108xx` = instance 1), marker/teleport
  flow `TPMARK` / `TPDEC` / `TPSTREAM` / `TPFX` / `FEREQ` / `GSTATE` / `GEVENT` / `HUBMSG`, `TPOVR` (coordinate
  teleports).
- World (`world`): trigger volumes `TRIGLOAD` / `TRIGVOL` / `TRIGUNLOAD` / `TRIGMGR` / `TRIGADD` / `TRIGREM` /
  `TRIGENTER` / `TRIGEXIT` / `TRIGQRY` (each tracked entity's query points) / `TRIGENT` / `TRIGGRP`, dynamic hulls
  `HULLENTER` / `HULLEXIT` / `HULLENT` (never fired in free skate), teleports `TELE*` (menu teleports do not use the
  TeleportPlayer path; they go through `sub_828646E0` -> `sub_82864918`), `WPKEY` (region layer key changes).
- NPC (`npc`): spawn/cull `NPCSPAWN` / `NPCCULL`, moods `NPCMOOD` (every positive result) / `MOODOUT` (mood event id,
  for example bump, collision nearby, slam nearby, trick nearby, presence, greeted; and the resulting want: angry
  chase, warn, flee, taze, ...), ped timers `PEDTIMER` (named; the name list is a table in the image) / `PEDTIMERS`
  (10 s counts of the periodic presence and trespass checks), positions `PEDXYZ`, perception `PEDSEE`, chase /
  takedown `SECCHASE` / `SECTAKE` / `SECTAZE` / `SECZONE` (the generic chase code peds use; Skate 3 has no guards),
  ped audio `PEDAUD` (the PedestrianSFX process `sub_824D8078`: footsteps, voice model, speech value, materials).
  `MOODOUT` values: mood event id (word 2) 10 = skater bump, 11 = collision nearby, 2 = slam nearby, 6 = trick
  nearby, 8 = presence, 16 = greeted; want enum 1 = angry chase, 2 = warn, 10 = flee, 12 = taze. The `PEDTIMER` name
  list is the table at `0x82064BCC`. `TELEFP` (first pass) lists teleport-path candidates.
- Ped navigation (`pedstuck`): `STILLOBS` (the bot's still timer queues its own obstacle, `82EA17E8` from lr
  `0x82E9747C`), `NAVCUT` (`8292ADE8` from lr `0x82EA126C`), `PATHOK` (path check `82E95FD0` from lr `0x82E975A8`;
  invalid results always, valid ones only after a `STILLOBS`), `PATHREQ` (path request `82E957F8`, any lr, per bot at
  most 4/s with a skipped count), `STILLREL` (`82E939A0`). Teleport to each spot with `SKATE3_TP_OVERRIDE`.
- Traffic (`traffic`, `skitch`): `VEHSTATE` (per vehicle at most 4/s plus on manoeuvre or lane change: target speed
  `+3408`, manoeuvre `+4396`, target lane `+4380`, alarm `+3716`, position + forward from the world matrix
  `*(vehicle+164)+16`), `VEHAUD` (the TrafficEngine update `sub_824D6478`'s vehicle audio record: rpm, acceleration,
  speed, listener distance, horn, skid, relative speed), horns/skids/engine/lights `TRAF*`, junctions `VEHJUNC` /
  `VEHCONN`, skitching `SKITCH*`, `VEHBAIL`. NPC skaters skitch cars unprompted in busy spots (Crystal Towers), so a
  short scripted proof run fires the skitch hooks without the player.
  Also `TRAFPROG` and `TRAFLIGHT2` (traffic program and light state).
- NPC skater spawn push (`aiskater`): `AIPUSH <ms> board | lr | vx vy vz` = the push `82C04168` when called from the
  AI hold branch `824701F8` (lr in `0x824701F8..0x82470718`); `BUMPQ <ms> owner | result | after_push` = IsBumped
  `82BA7310`, every true result and every evaluation within 1 s after an `AIPUSH` (at most one line per owner per
  30 ms). Answers whether a retail AI skater reads as Bumped right after its spawn push.
- NPC skater boards (`aiskater`): `SKATEB` per board at most 4/s (contact pattern of w0-w3 / front truck / back truck
  / deck: `0000110` grind, `0000001` slide, `0011000` manual, `0000000` air; contact point of the first touching
  part; deck velocity; part index), `SKATER` state flags, `AIPC922` (path controller test byte). PCU Library has the
  most NPC skaters.
- Character records (`cas`): `CASACTOR` (actor constructor `82590DC0`, slot index), `CASID` (record id + name),
  `CASCHAR`, `CASSET` (getter `82590B50`: stance byte `+120`, `+121` male, style), `CASREC` (the slot record once).
  The index is the SLOT (0 player, 1-3 NPC skaters), refilled per spawn.
- Grinds (`grind`): `GRMGR` (manager pre-update `82D8A828`, only while grinding and 30 ticks after), `BRDPOSE` (after
  the board contact pass `82C07D20`: contacts, closing velocity, contact count, part normals / points / old
  velocities; full rate inside `SKATE3_GRIND_BOX="xmin xmax zmin zmax"`, else 2/s per board), `GRPOST` (closing check
  `82D90AB8` request bytes).
- Render (`render`): `DOFRAME` / `DOLIST` / `DOAPPLY` (one frame of effect applies and scene-view draw lists),
  `VDLWRITE` / `VDLVAL` (writer probes for a render global).
- Physics (`physics`): solver iterations `ITERSET` / `ITERTICK`, board/body lines.
- Wheel contacts (`physics`, on only when `SKATE3_WHLC="x z radius [maxr]"` is set): `WHLC` = every wheel / board
  volume vs world triangle pair that `8277BC58` hands to `8277B720` whose fixup ran (accepted and rejected): body,
  volume ids / type / radius / centre, triangle (ids, corners, normal, runtime flags, the three decoded edge cosines
  and the bytes rebuilt from them), the fixup region (0 face, 1 e0, 2 e1, 3 v1, 4 e2, 5 v0, 6 v2) and result, the
  normal before / after, count, separation, the first point pair and the return value (25 fields). `WHLCARM` once
  (proves it fires), `WHLCSUM` at 1 Hz (lines, dropped, candidates, accepted; capped at 2000/s). Use it to compare
  retail's runtime triangle flags and edge cosines at one spot with the engine's (skill `recomp-research`, worked
  example).
- HUD minimap (`render`): `MMDRAW` (minimap draw `82665388` at 1 Hz: lr, component, f1 / f2, r4 / r6 / r7, the r5 map
  matrix as 16 floats, callers), `MMITEM` (icon draw `8268CB40`, at most 10/s: item fields +0x10 x z, +0x20 angle,
  +0x24 scale, +0x28 visible, +0x30 fade, +0x40 rgba), `MMQUAD` (`824FA550` from the icon / pointer draws: both
  16-float argument blocks).
- Diagnostics for the per-player gate (`audiox`): `LOCALTEST` logs each object's local / NPC decision when it
  changes; `BAILCAND` holds the rate-limited raw lookup inputs of the bail hooks.
- Per-player hooks must gate on the local-rider byte (`local72`) or they log nearby NPC skaters too; filter older
  traces accordingly.
- Trace size: pass only the categories a run needs; location and ambience runs need `audio` only, while `physics`
  and the DSP `MOD` lines dominate the size.

## Gotchas
- Edit C++ with an editor tool: Python/shell edits turned `\t`/`\n` escapes into literal characters several times.
- Bash double quotes expand PowerShell's `$_`; keep PowerShell in `.ps1` files.
- Pass absolute paths everywhere (the runner changes directory; relative script paths silently start nothing).
- The trace header is compiled into both the exe and the runtime DLL: a single writer plus a shared time zero
  (`SKATE3_AUDIO_TRACE_T0`) keeps lines whole and on one time base.
