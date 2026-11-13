# Instant replay: record every actor, 90 s window

Branch `world/replay` (from `world/living-world`). Plan and milestones: the replay todo (M1 record everything, M2
editor, M3 effects, M4 export). This doc covers what has landed.

## M1 step 1: every drawn actor is recorded and played back

### Problem
Upstream's replay (`crates/skate-game/src/replay.rs`, 88ec43e2) records only the player (root, skin, camera) in a
30 s window. During playback the rest of the world freezes: NPC skaters, peds, traffic and props stand where they were
when the replay opened, so a replay of a line past a crowd shows a still picture around a moving skater.

### Retail (evidence)
- Retail records presentation data, not a re-simulation: one DataTracker frame per sim step (60 per second, recomp
  count run 2026-10-19, hook REPLAYREC), at most 5400 frames (`li 5400`) in an 11,534,336 B byte ring that evicts the
  oldest frames (DataTracker `82726170` / `82DBBAF8`) [code]. 5400 frames at 60 per second = a 90 s window.
- Provider id 2 records the living-world census actors (peds, traffic cars, DMO props) and ids 3 / 4 the skaters
  (`sub_827BF090` reads the census containers +0x20 / +0xAA0) [code, research b364].
- Skater poses are LOCAL joint rotations as 4 x i16 quaternions (scale 32767), interpolated per joint with nlerp on
  playback (decoder `8278ABE0`, pose block `8278A4E0`) [code, research b365 to b367].

### Change
- `crates/skate-core/src/replay_buffer.rs` (new, engine independent): one key track per actor, keyed by a stable u64
  id. A key holds the root transform (f32 translation and scale, i16 quaternion) and every joint's local rotation as
  retail's 4 x i16 quaternion; joint translations are stored only where they left the actor's rest translation, scale
  only when not 1. A key is written only when the quantized pose changed (a resting prop costs one key for the whole
  window); after a hold the held pose is repeated at the previous frame so playback holds instead of blending across
  the gap. Playback blends translation / scale linearly and rotations with nlerp, like retail's decoder.
- Limits are data (`ReplayLimits`): `max_frames` defaults to retail's 5400. `byte_budget` defaults to 128 MiB (OURS):
  retail's 11 MB holds its compressed frames, ours are uncompressed i16 rows for every drawn joint, so the budget is
  sized to keep the retail 5400 frames in a busy scene. When the budget fills first, the oldest frames go first, as in
  retail.
- `crates/skate-game/src/replay/world.rs` (new): `ReplayActor { id }` tags NPC skaters, peds and traffic cars with
  `LivingWorldId::to_u64` and props with `3 << 56 | instance id`; `ReplayJoints` lists the joints from each kind's
  bindings (NPC puppet, ped puppet, car wheels). Recording runs in PostUpdate after every presenter, once per new
  player frame (so on the same clock as the player's recording). Playback writes the recorded root and joint
  transforms, hides actors that were not in the world at the playhead (not spawned yet, or already despawned), and
  puts every touched transform and visibility back on exit.
- The player's window follows the same limit: 5400 frames at 60 per second = 90 s (was 30 s).

### Moddability and multiplayer
- Any entity with a `ReplayActor` (and optionally `ReplayJoints`) is recorded and played back; mod props
  (`sdk.world.spawn_prop`) carry `PropInstance` and are recorded like map props.
- Mod API (`sdk.replay`, capability `replay` = 1, all in `crates/skate-mods/src/api.lua`):
  - `sdk.replay.open()` (`replay_open`): opens the replay at the newest frame; refused in multiplayer.
  - `sdk.replay.marker(label)` (`replay_marker`): a marker at the newest frame, label 1..64 characters; at most 50
    (retail provider id 10); markers leave with the window and the mod's markers go on disable.
  - `sdk.replay.set_limits{max_frames, byte_budget}` (`replay_set_limits`): 60..36,000 frames, 1 MiB..1 GiB; nil
    restores the defaults; last writer wins; the override goes when that mod is disabled.
  - Read-only `snapshot.replay_state`: active, cursor, start, end, frames, bytes, max_frames, byte_budget, markers
    (time, label, owner). `snapshot.replay` stays the old boolean.
  - Still to come: a mod-side provider (record per-frame mod values, read them back during playback).
- Ids are the stable network ids (`LivingWorldId::to_u64`, prop instance ids), keys are plain bytes with counts, so
  a recording is serializable. Replay stays refused in multiplayer for now (unchanged).

### Files
`crates/skate-core/src/replay_buffer.rs`, `crates/skate-core/src/replay_buffer/tests.rs`, `crates/skate-core/src/lib.rs`,
`crates/skate-game/src/replay.rs`, `crates/skate-game/src/replay/world.rs`, `crates/skate-game/src/replay/tests.rs`,
`crates/skate-mods/src/api.lua`, `crates/skate-mods/src/vm.rs`, `crates/skate-game/src/modding/mod.rs`,
accessors in `animation.rs`, `living_world/npc_skaters.rs`, `living_world/peds.rs`, `living_world/vehicles.rs`.

### Verification
- skate-core: 7 tests (quantize round trip within 2e-4 rad, keys only on change and holds across gaps, hidden before
  spawn and after despawn, nlerp midpoint, eviction at the frame cap keeping the key that holds at the window start,
  eviction by bytes with exact byte accounting, clock reset clears).
- skate-mods: the replay commands parse and validate (bounds refused), the Lua functions submit them, the capability
  is set.
- skate-game: marker cap, marker eviction with the window, limits ownership and cleanup on disable; the 90 s window
  test, and a world playback test (an actor plays back its recorded root and joint
  mid-frame, an actor that appeared later is hidden, exit restores the live transforms and visibility).
- Not play-tested yet.

### Actors that left the world inside the window
NPC skaters, peds and traffic cars despawn through `crate::replay::retire` instead of `despawn()` (npc_skaters.rs
the two despawn sites, peds.rs the despawn system, vehicles.rs the despawn record and the external despawn). It
moves the actor's loaded scene (the child that holds its recorded joints) under a hidden "ghost" root with the same
replay id, then despawns the actor with its gameplay components. Playback draws the ghost from the track up to the
despawn frame and hides it after; the ghost is dropped once its track leaves the window, never during playback. At
most 64 ghosts (`MAX_GHOSTS`); past that, or when the actor was never recorded, it is simply despawned. The world
reset on a map change keeps a plain despawn (the replay resets with the map). The actor's fade (`NpcFade`, spawn
fade-in, leave fade, the peds' camera-distance fade) is recorded like the audio components and moves to the ghost
with its fade materials, so a skater that faded out before leaving plays back at its recorded alpha instead of
staying invisible. Test: a retired ped plays back as a
ghost before its despawn, hides after it, stays hidden while live and goes with its track.

### Playback audio: the recorded skater state plays again
Retail records no audio and does not mute: its normal audio runs on the recorded presentation state, with "in
replay" gates (research b375, b378).
- Recording: every player frame keeps the audio step `observe` published that physics step (`Cues::raw`, the
  `Riding` record with the full retail `AudioState`; one choke point, every player audio system reads `Cues`).
- Playback: `observe` stops publishing the frozen live state; the feed (`feed_replay_audio`, PreUpdate after the
  replay controls) publishes the recorded steps the cursor crossed going forward, so the host hears rolling, grinds,
  pushes, landings and bails again, at the steps they happened (one-step pulses merge like a live multi-step frame).
  A fast scrub sends at most 8 steps per frame and keeps the skipped pushes; a rewind sends the step at the cursor
  without its pulses.
- Gates: a replay that stands still (paused) is silent, as retail's paused replay (Master.in6 at time scale 0);
  speech stays silent in any replay (retail blocks speech requests in replay); Master.in0 is written every pass, 32767
  in replay and 0 otherwise (checked in the recomp: `sub_824D5160` after the replay flag `sub_8279E180` writes
  input 0 with 32767 or 0), so the retail MixMap swaps the Hall of Meat slow-motion ducks for the replay duck.
- Seeks and rewinds count as camera cuts for the host (Doppler velocity reset).
- World actors: `replay/components.rs` records each actor's audio components with it, on change, keyed by the
  replay id: `TrafficAudio` (engine, speed, load, horn, skid), `PedAudio` (feet, speech state), `NpcSkaterAudio`
  (the NPC's full audio record) and `AudioVelocity`. During playback the value holding at the cursor is written back
  in PreUpdate, before the audio host reads it, and the live values return on exit. Any `Component + Clone +
  PartialEq` can be tracked the same way (`components::track`). Ghosts (actors that despawned inside the window) are
  drawn but stay silent: an audio component on a hidden ghost would make it audible during live play.
- One-shots: `replay/messages.rs` records the bridge's one-shot messages with their frame time (`VehicleHorn`,
  `VehicleAlarm`, `PedTazerEvent`, `PedBodyFallEvent`) and writes each again once when the playback cursor crosses
  its time going forward (not on a rewind). NPC reactions and ped speech stay out (speech, blocked in retail's
  replay). Any `Message + Clone` can be tracked the same way (`messages::track`).
- Vehicle sounds (research b409, 2026-10-21): retail has no vehicle impact sound; its vehicle entry keeps a horn field per recorded frame. Ours now records the EFFECTIVE horn state (`world_audio::VehicleHornState`: the bridge's honk / alarm hold, else the AI's horn) as a component track and plays the recorded value at the playhead (`world_bridge.rs`: live holds neither count down nor take events during a replay, they resume on exit), so a seek or rewind into a sounding alarm plays it; the `VehicleHorn` / `VehicleAlarm` one-shot tracks are gone (they would double). Test `the_horn_state_is_recorded_and_a_replay_plays_the_recorded_value`. Replay speed below 100 % (M2) will
  scale `AudioState.time_scale`; whether retail's pitch follows is open.

## The free camera stays within 40 m of the player

Retail's replay free camera is pulled back when it is farther than 40 m from the player (per-frame limits step
`82683680` -> `82682CB0`, constant at 0x821EB794; research b381). Ours flew anywhere. Now the free camera is projected
back onto the 40 m sphere around the player's recorded position at the cursor after each move.
Test: the leash keeps a camera inside 40 m and pulls a far one onto the sphere.

## Editor buttons and the Finished menu as retail (2026-10-23, research b555 / b387, USER-ASKS 271)

- **A on a Game camera key** opens nothing (`82612C28`: kind >= 3 returns; verified `cmplwi r3,3; bge` to the
  epilogue). Ours opened its key edit; `Editor::reopen_at` now returns None and leaves the edited key alone.
- **B / Back** raise the Finished menu only in playback, the instant replay and the timeline (retail modes 1 / 5 / 6,
  `82614E58`); trim and the key edit modes ignore them (the effects menu commits on B). Ours raised it in every mode
  but the effects menu.
- **Save asks first:** retail's Save row confirms (title = the row, ID_REPLAY_EDITOR_POPUP_REPLAYSAVE "Save Replay?",
  No / Yes; callback `82616000`, verified: No skips the save and closes). Ours exported at once; now Save to PC
  asks the same and No returns to the editor.
- **Film pack Finished menu (research b557, verified the reset calls and the exit's three events):** with the film
  pack's screen, B goes straight to the quit warning and X on the timeline opens its menu (in its effects menu X still
  commits a soft key): Save to PC (with the Save Replay? confirm), Take Photo (the same flow as the base X), Replay
  Mode, Reset Timeline Current Channel, Reset Timeline. Replay Mode is a popup of three toggles that stays up
  (`82615728`): greenscreen (only characters drawn over a flat color: everything the map owns hides except its
  lights, the clear color turns green; retail fills the screen with the constant (0, 1, 0, 1) before the scene while greenscreen is on [code 827FE8D8, b558; the +0x7DF0 read in b557 is a screen fade color], so the default matches retail and stays data; the retail sky dome hides with the world, and the visual indicators (teleporter icons) hide too, as retail skips their draw [code 827FF7D0 -> 827AFC80, b559]),
  player visible (the player's root), skaters visible (NPC skaters and peds; cars and props hide only under
  greenscreen). They are local view state (`Replay::view`) and go back to the defaults when the replay closes
  (`82610828`). The resets (`82C7F728`) ask No / Yes (No reopens the menu) and clear every key of the current channel
  or of all channels (the camera channel: back to the recorded camera); playhead, channel and clip heads stay.
- **Not yet:** Upload (no service); a Lua hook for the replay mode flags; retail's greenscreen color value and its
  other render switches (fog / light blends, one full-screen pass) beyond hiding the world.
- **Tests:** `the_finished_menu_offers_save_to_pc_and_exit` (trim ignores B, the Save confirm), `camera_keys_hold_cut_and_blend`
  (Game key), `the_film_pack_finished_menu_works_as_retail`, `replay_mode_flags_pick_what_draws`,
  `replay_mode_hides_the_world_and_the_player_and_puts_them_back`, `reset_timeline_clears_the_channel_or_all`. Not
  play-tested.

## Camera key edit rates (2026-10-23, research b549)

- **Retail [code + data]:** the editor screen reads the pad, not the camera: `82610F18` per edit mode writes the
  values, `82509558` hands them to the camera, `82C826D0` integrates them (angles in 16-bit units, pitch +-85 degrees).
  Per console frame: Follow / Tripod turn clamp(2.5 s, 3) degrees, distance -clamp(s, 0.2) (no scale: full speed at
  20 % stick) in 0.5..1000, look lift 0.01 in +-1; Free turn clamp(5 s, 1.6) degrees, move clamp(s, 0.2) m per axis
  in the yaw frame, height eased (velocity += 0.015, capped 0.2, applied, then x0.6: 0.0375 m per frame held; offset
  +-10 m); lens 0.5 in 24..75. Verified the 0.015 load (0x820D0190 in `82610F18`).
- **Ours before:** distance and free moves were 0.2 x stick, free turn had half the slope, free height moved 0.2 x
  input per frame with no easing and no range.
- **Change:** `camera_key::edit` uses those rates (CameraRules gains the free turn scale and the height step / cap /
  decay, all data); the editor keeps the eased height per edited key.
- **Tripod [code], research b550:** `82C82A50` stores v = eye - look as yaw / pitch / distance; `82C82E40` with the
  camera's pivot byte set (Follow and Tripod) rebuilds eye = look + v, so the sticks orbit the eye around the look
  point and distance moves it along the sight line (the lens is the zoom); with the byte clear (Free) the eye is the
  pivot. Tripod differs from Follow only in its eye space (world, not riding the skater). Ours edited only the
  Tripod's look lift; now `edit` orbits its world eye around the skater root raised by the look height with the
  Follow caps (the editor passes the root at the cursor). Ours re-derives v every frame (whether retail does while
  the target moves is open).
- **Tests:** `free_camera_rates_follow_the_editor_constants`, `tripod_orbits_its_eye_around_the_look_point`. Not
  play-tested.

### The replay cameras do not pass through walls (2026-10-21, research b449)

Problem: the free camera and the editor's Follow / Tripod / Free cameras flew through walls and the ground.

Retail [code, b449]: every evaluated replay camera goes through one constraint (`82C8B698` -> `82683680`): the 40 m
leash first (`82682CB0`), then `82682D80`: a swept sphere of 0.1 m x 1.3 = 0.13 m (0x820641A8, 0x82099484) from the
last accepted position to the new one, and a line-of-sight sweep from the player to the new position. A hit on the
motion sweep slides the camera along the hit plane (the into-wall part of the move removed); if the slide is blocked
too the camera stays where it was. A non-Free camera whose player is hidden is turned back to its look point
(`82670600`); Free keeps its rotation. A move below FLT_EPSILON runs no query. No smoothing, no minimum height.

Change: `replay/camera_collision.rs` `constrain` (pure, a sweep closure; the map query is the gameplay camera's fat
line, `camera/world_query.rs` `line`). The instant replay free camera uses it after each move (`replay.rs` controls);
the editor cameras are constrained while a key is edited and the result is stored in the key (since 2026-10-25, see
"Editor cameras: the constraint runs while editing, playback plays the key"; before that the view was corrected on
playback). The data is `Replay::camera_collision` (`CameraCollision`: switch,
radius, scale, epsilon, leash; retail defaults) and a mod sets it with `sdk.replay.set_camera{collide, radius, leash}`
(nil restores retail's, the override goes when the mod stops; last writer wins). Replaces `leashed`,
`FREE_CAMERA_LEASH` and `Replay::free_camera_leash`.

A snap (retail +0x183, set by the constraint reset `82682A40` -> `82682B20` when an edit opens or the camera changes,
b593) has no last position: only the line of sight runs and the camera is pulled in front of an occluder. Retail writes the held pose back into the key while editing (`82C82E40`, b593). The 0.8 m and world-supplied radii (world flags +0x30DA0 / +0x30C72) and
the sweep's collision layers are open; ours hits the map and its external queries, not actors.

Tests: `camera_collision` (slide along a wall, blocked slide stops, no motion runs no query, a cut behind a wall
pulls in front, leash before collision, a hidden player turns a non-Free camera back, switched off passes through),
`replay::tests` (leash, mod override restore and disable), skate-mods `replay_set_camera` validation and Lua call.

### Editor cameras no longer stick under the ground; Advanced Camera rows highlight (2026-10-25, USER-ASKS 339)

Problem (laptop session `game-20261025-085807`, the user: free camera pushed the camera under ground and it could
not be brought back up or moved in any direction; the tripod camera behaved similarly, its move-up only half working
and locking onto the player from below; selecting the advanced camera section made the selection highlight
disappear). The log showed the key edit
modes (FreeKey, TripodKey) and the sticks arriving, and no camera pose.

Cause, cameras [code, ours]: on a new shot (a cut) our constraint pulls the camera in front of an occluder on the line
of sight from the anchor, the skater root at foot level. The anchor sits on the ground, so the fat line (0.13 m)
starts in contact with it, the map query returns that contact as a hit at fraction 0 (`camera/world_query.rs` `line`
clamps the fraction and reports the triangle's face normal), and the camera was put at the skater's feet, half under
the ground. Every later motion sweep then started in contact too and the blocked slide kept the camera where it was:
it could not move at all, and a Tripod key turned back to its look point looked at the skater from below.

Change: `camera_collision::constrain` is one-sided: a hit counts only for a move into the surface's front face
(`(to - from) . normal < 0`). A move leaving a surface it touches or coming from behind it (the anchor on the ground, a
camera under the map) passes; walls and the ground still stop a camera pushed into them. The instant replay free
camera (C) uses the same constraint.

Cause, highlight [code, research b591]: the editor movies (`replay_editor2` `UpdateEffectsSelection`, and
`replay_editor3` in camera mode) select the AdvFollow / AdvTripod / AdvFree clips only when `Game.IsDemo() == false`
(true grays them out). We did not answer `Game.IsDemo`, so the check failed and rows 4..6 showed no highlight. Our row
indices and states were already right (one flat list of 9: Follow, Tripod, Game, Free, AdvFollow, AdvTripod, AdvFree,
Speed, Shake). Change: `crossbar_host.rs` `game_constant`: `Game.IsDemo` = false (retail's other demo flags are
constant false too).

Log: `REPLAY_CAMERA key new row=.. <key> view=.. root=..`, `REPLAY_CAMERA key reopen <key>`, `REPLAY_CAMERA key done
<key>`, `REPLAY_CAMERA edit <key> turn move height zoom root` every 0.5 s while the sticks or D-pad move a key, and
`REPLAY_CAMERA shot <i> <key> wanted=.. at=.. root=.. relook=..` on every new shot (where the key wanted the camera and
where the constraint put it).

Tests: `camera_collision` `a_cut_from_the_skaters_feet_keeps_the_camera_above_the_ground`,
`a_camera_under_the_ground_can_move_up_and_along_but_not_down_through_it` (with the earlier wall tests),
`crossbar_host` `game_is_demo_is_false`; skate-game 940. Not play-tested yet: the laptop's next replay edit decides.
Open: retail's world query may treat start-in-contact differently; the editor movie's `GetCameraZoomString` is still
empty (b591).

### Peds' held props replay through a drop, a put-away and a cull (2026-10-21, research b453)

Retail [code, b453]: a ped's hand prop is an ordinary DMO in the census list the recorder walks (`826B8830` creates
it there, cap 49; `827BF090` writes a 0x70 record per DMO: id, transform, template key); it is recorded by its own id
for its whole life (picked up, held, dropped, gone) and the ped's record has no prop field.

Ours had three gaps: one track per ped (a second prop reused it), one track in two spaces (hand-space keys while held,
world keys after a drop: scrubbing back before a drop put the dropped model near the world origin), and props put
away or culled were despawned, so they vanished from the frames where they were still held. Change:
`replay/world.rs`: one id per prop object (`HAND_PROP_TAG` + the prop entity's bits) and `keep_model`: a hidden copy
of the model stays under its parent with the recorded id (a `ReplayGhost`, dropped when its track leaves the window)
and the original's track ends; `ped_hand_props.rs` calls it at the drop (the dropped model then starts its own prop
track in world space), at the put-away and at the cull. OURS: retail keeps one id through the drop; ours splits the
held and the dropped life into two tracks, the same outcome on screen. Test
`a_dropped_hand_prop_keeps_its_held_frames_on_a_hidden_copy`; replay tests 35, hand prop tests 8 pass.
Open: the id 2 playback reader, the DMO state entries (`82C48610`).

## Fixes from the first play test (desktop session 2026-10-20)

The user reported that ped hand props lose their attach point or float offset from the ped
during the replay, returning to the hand only at its end, and wobble slightly even when the
replay is paused at the end; later the ped vanished from the replay entirely, so the skater
rides over and bails on nothing.
- **The living world kept running during the replay.** The log showed fixed steps and 26 `PED_DESPAWN reason=Distance`
  lines while the replay was open: peds walked away from the frozen skater and despawned at 70 m, the hit ped (#3)
  at 15:01:50 among them. Retail plays recorded data while the world waits. The living world's fixed chains
  (population, peds, traffic, NPC skaters, ped / vehicle contacts) now run only outside a replay
  (`replay::world_live`).
- **Ghosts.** The cap is 256 (traffic churn could pass 64 in a 90 s window), and every ghost made or refused is logged
  (`REPLAY_GHOST id=... joints=... ghosts=...` / `REPLAY_GHOST refused id=... reason=cap | not recorded | no joints |
  scene not under the actor`), so a vanished actor can be traced in the log.
- **Held hand props** were drawn every frame from the ped's live (frozen) hand while the ped played back from the
  recording, so they floated off and wobbled with the presenter's interpolation. A held prop is now a recorded actor
  of its own (id `5 << 56 | ped serial`, one track per ped whichever prop it holds) and plays back with its ped.
  A ped that despawns keeps its held prop: `retire` moves the ped's scene, the prop included, under the ghost (test
  `a_ghost_ped_keeps_its_held_prop`, 2026-10-29).
Tests: the living world freezes while a replay is open (and runs in apps without a replay).

## Instant replay controls follow retail

Retail's instant replay (editor mode 5; research b386, b387, b389; verified the open function `82610458`
setting mode 5 and the 150-frame check) replaces our own bindings (user 2026-10-20: the controls follow
retail):
- **Back** (View / Select; F6 on the keyboard) opens the replay, only with at least 5 s recorded (retail's 150
  frames of the 30 fps timeline; the unit is inferred); a refusal is silent, as retail. `sdk.replay.open` uses the
  same check (`Replay::can_open`). It no longer closes the replay.
- **B** or **Back** in the replay: retail's Finished menu; with nothing to save or upload that is the quit warning
  (ID_COMMON_WARNING_TITLE / ID_REPLAY_EDITOR_QUIT, No / Yes), raised on the retail menu stage through a small engine
  popup channel (`PopupMods::engine_requests` / `engine_choices`). Yes leaves, No stays. Without the retail menu
  stage (old installs, `SKATE3_RETAIL_MENUS=0`) B / Back leave at once. OURS, a safety until play-tested: Back again
  while the warning is up leaves at once. Fix 2026-10-20 (desktop session: "the close confirmation dialog [is] closed
  instantly"; log: popup 0 shown at 14:32:45.341, choice -1 at .389): the B press that asked for the warning reached
  the popup the next frame and canceled it. The warning now opens when B / Back is released.
- **RB** plays / stops (was A); stick or D-pad input resumes a stopped replay (`82C7FFB8`).
- **D-pad Left / Right** jump to the start / end (was a one-frame step; the frame step stays for the editor).
- **A** (retail: open the full editor) and **Y** (retail: cycle the edit channel, film DLC only) do nothing until
  the editor exists. The free camera is PC only now (C, WASD / Q / E, right mouse; was Y on the pad): retail's
  instant replay has no free camera.
- Not ported: the online instant replay mode. (The open sound is ported: see "The opening sound (2026-10-20)"; research b468 confirmed retail plays it once on Back, behind the same 150-frame check.)
Tests: the controller test (open, scrub, jump, RB, stick resume, B), the quit warning (No stays, Yes leaves) and the
5 s minimum.

## Trigger scrub at retail's rate

Retail's scrub (research b387 / b388; verified `82610F18` reads the mode word and stores the rate at +0x34, and
`825071B8` passes it to the playhead step `82C7EF90`): per console frame the target is (RT - LT) x 0.005, the rate
moves 0.25 of the way to it, clamps to +-0.005, and stops in a dead band (target < 1e-5 and rate <= 1e-4); the
playhead moves by the rate as a fraction of the WHOLE recording, so a full sweep takes about 6.7 s whatever the
length (at most 0.15 of the recording per second). Ours moved a flat 4 s of recording per second, with no easing.
Now `skate_core::replay_edit::ScrubFilter` / `ScrubSpec` (data) run whole 30 Hz console frames (the same movement at
any engine frame rate) and the replay controls use it: a moving scrub stops playback, and it eases out after the
triggers are released. The keyboard arrows count as full triggers. Tests: ease-in, cap, release to zero, 30 vs 144
fps give the same distance; the controller test now ticks whole console frames.

## M2 step 1: the editor model (skate-core)

`crates/skate-core/src/replay_edit.rs`: clips trimmed out of the recording (30 fps timeline frames), each with its
effect values and stepped marker lanes, and retail's length rules. Pure data, no UI yet.
- Opens at 150 frames or more (`8250B8F8`, > 149); timeline 30 fps (`825ADD90`).
- Encoded length = round(sum over clips of span / (speed% x 0.01) / 30 + 0.5) s, shareable at 60 s or less
  (`825ADF88`, `cmplwi 60`).
- Lanes with retail ranges (research b343 / b344 / b377): speed 10..100 % step 10 default 100 (slows only),
  shake 0..3 wrapping, brightness -100..100 step 20, contrast 10..30 step 2, saturation 0..100 step 10, tint R / G /
  B / A 0..250 step 25. The look defaults are the neutral values (OURS until retail's are read).
- A lane's value steps at a marker (no ramp: retail writes the value at the marker and 1 ms later, `8250CC10`); a
  marker on the same frame replaces; trimming drops markers outside the clip; at most 100 markers per lane (retail
  refuses at 100 with "no more markers", `82613500`, research b381). A marker within 0.0089 of the clip span from
  an existing one is "at the cursor" (setting a value edits that marker), and no marker goes within 0.0089 of the
  clip's in / out points (`825084C8`, constants 0x822428EC and 0.9911 at 0x822F8F74; research b382; that the
  normalized unit is the clip span is inferred).
- Data: `EditLimits` and `LaneSpec` hold every number; lanes have stable string ids (`speed`, `tint_r`, ...) for mod
  records and saved edits. A mod may allow speeds over 100 % (ours, not retail).
- Tests: 6 (open threshold, stepping / clamping / wrapping, stepped markers, marker cap, marker spacing, encoded
  length).

### Editor cameras: the constraint runs while editing, playback plays the key (2026-10-25, USER-ASKS 341)

Problem (laptop session `game-20261025-094410`, the user: the tripod moved with the player's distance from
the placed tripod and slid to the chosen spot; the whole editor needs a retail parity pass, since it does not
feel like the retail editor). The log: the key edit moved the Tripod eye
55 m from the skater; on playback `REPLAY_CAMERA shot` put it at the skater root (wanted 55 m out, at = root).

Cause [code, research b593; verified the gate]: retail's
constraint `82683680` returns at once unless the byte 0x83037414 (0x830373F0 + 0x24) is set; the key edit opener
`82614658` (and the other edit openers) set it to 1, the timeline mode `82612C28` clears it. So the leash and world
collision run only while a key edit is open, and the held pose is written into the key (`82683450` -> `82C82E40` ->
`82C84868`); playback plays stored keys with no constraint. An edit opening resets the constraint with snap
(`82682A40` -> `82682B20`, +0x183). Ours did the reverse: the edit was free (up to 1000 m) and playback leashed and
swept the stored eye around the moving skater every frame, so a far tripod was dragged and a cut pulled it onto the
skater.

Change: `Editor::constrain_editing` (after each `edit_key`, against the skater root at the playhead) runs
`camera_collision::constrain` on the edited key's eye and stores the result (`camera_key::set_eye`: Tripod / Free
eye, Follow eye in its eye frame); `edit_last` starts empty when an edit opens (snap).
`Replay::guarded_editor_camera` no longer constrains; it only logs `REPLAY_CAMERA shot <i> <key> at=.. root=..` on
a new shot. The instant replay free camera (C) still runs the constraint every move (retail's other gate writer
`82833CF0` is not identified).

Still open (b593): `82C88A00` and which camera retail shows before a mid-clip first key were answered by b603 / b604
(see "A camera picked on a clip with no keys covers the whole clip" below). The element to
record space mapping of b401 (Follow SPACE_EYE) is settled: see "The Follow eye rides the gameplay camera" below.

Tests: `editor::tests::a_key_edit_stores_the_leashed_eye_and_playback_keeps_it`,
`camera_key::tests::set_eye_round_trips_a_follow_eye`, the `camera_collision` tests.

### Keyed cameras in the instant view (2026-10-26, USER-ASKS 353, research b615, recomp run m353)

Problem (laptop session `game-20261025-171928`, the user: the replay camera went through ramps in the Skate.Park
Stadium full part of the run, after adding camera keys). The Follow key was edited 1.37 m below the root; the
user played it from the Timeline.

Retail [code, b615]: the gate byte 0x83037414 of the constraint
`82683680` is written every frame by the editor update `82610F18` per mode: mode 5 (the instant view) sets 1, mode 1
(RB playback) sets 0, the key edits (7 / 8 / 10) set 1 through their openers, replay entry `82833CF0` sets 1.
[trace, recomp run m353 (hook RCOLL on `82683680`, Mega-Park spawn, a Follow key orbited low, then RB play)]: mode 5
before the editor: gate 1; Follow edit mode 7: gate 1, one correction at commit; RB playback mode 1: gate 0 for
14 s, the function returns at once (no corrections); back to the timeline mode 6: gate 0. So retail Timeline playback
does not keep keyed cameras out of the world either; the instant view does.

Change: `Replay::guarded_editor_camera` runs `camera_collision::constrain` on the evaluated keyed camera every frame
while our editor session is in `EditorMode::Instant` (sweep from last frame's constrained eye, snap on a cut;
Follow / Tripod re-aim at their look point, Free keeps its rotation). The stored key is not changed.

2026-10-29 (USER-ASKS 402): retail's replay camera never clips through the ground or ramps, so the same
constraint now runs in every mode where the clip plays (Playback, Timeline, Trim, instant view); the key edit modes
keep their own edit constraint. `CameraCollision::playback` defaults to true; `sdk.replay.set_camera{playback_collide =
false}` restores the code-gate reading (constrain only the instant view); `collide = false` turns all of it off. The
retail mechanism that keeps a playing camera out (its code gate reads 0 in mode 1 / 6, m353; research b658 leads:
the generic camera pass `828F2B58`, the sink `[S+0x7C]`) is unknown and not needed for the port. Not play-checked.
AVATAR_LOOK (element 0x5C, record 0x830C1594) gates the look point and the constraint `82683680` in `82C8B698`: Follow
`82C7FFB8`, Tripod `82C80298` and Free `82C80540` write 1, Game `82C807F0` writes 0 (research b671 [code]). Ours gives
the same: Game has no pose, so it is never constrained (`camera_key::pose`).

Tests: `replay::tests::keyed_cameras_are_constrained_in_the_instant_view_not_in_playback`,
`replay::tests::a_mod_camera_override_goes_on_restore_and_disable`, skate-mods `replay_set_camera` validation.

### The Follow eye rides the gameplay camera (2026-10-25, USER-ASKS 346 / 349)

Problem (laptop sessions `game-20261025-130256` and `-144414`, the user: at a low angle the advanced follow camera can
be forced under the map when landing jumps; it switches sides well, but the
camera can still peek under the map even from small jumps). The log of the second: an Advanced Follow key at pitch
1.4 degrees, 1.86 m; the eye dipped 0.09 to 0.28 m below the skater root while the root climbed 2.09 -> 3.69 m.

Cause [code, ours]: our Follow eye was an orbit in the skater root's full frame (`camera_key::pose`, the root from
`presentation.rs` `animation_to_world`), so the body pitching or rolling on a ramp or landing swung a low eye into the
ground. There is no smoothing: the view only blends two ticks.

Retail [code + trace, a local research note]: `82C84CF8(take, &m, space, id)` builds
an element's frame; for space 0 an id at or below `[[0x830C1584]+0x24]` is the default subject `[0x83037408]+4`, a
larger id is slot `id - 1` of the actor manager `0x830846C4` (rotation `826813C0`, position `82681870`). Slot 1 is the
gameplay camera's matrix (`82672118([0x83083C38]+0x2FD20)`), slot 3 the flagged skater entry. Follow's setup
`82C7FFB8` (also Advanced Follow, b405) writes eye object 2 and look object 4 (b402). A recomp watch run read the
threshold as 0 in all 68 samples, before, in and after the Replay Editor (a local trace, watch list
`follow_watch.txt`, recomp input script m346_replay_editor) [trace]. So the Follow eye rides the
gameplay camera (position and rotation) and only the look rides the skater. The recorded gameplay camera keeps itself
out of the ground, so an eye placed relative to it does not dive when the body pitches. Editing is unchanged: the
sticks orbit the eye around the look point in world angles (the pivot rebuild, as Tripod); only the stored space
differs.

Change: `CameraKey::Follow { eye, look_height }` stores the eye in the Follow eye frame; `camera_key::Anchors` (the
eye frame and the skater root at the cursor, built from the recorded camera and the root) replaces the bare root in
`key_from_view` / `edit` / `pose` / `set_eye` and the editor (`pick_camera`, `edit_key`, `constrain_editing`,
`pose`); `Replay::editor_camera` takes the recorded camera. The frame is data, `Replay::follow_eye` (`FollowEye::
Camera`, retail, or `Skater`, ours before), and a mod sets it with `sdk.replay.set_camera{follow_eye = 'skater'}`
(nil restores retail's; the override goes when the mod stops). `REPLAY_CAMERA` lines print a Follow key as
`follow eye=[x, y, z] look=..`.

Tests: `camera_key::tests::follow_eye_rides_the_gameplay_camera_and_orbits_the_look_point` (the skater pitching does
not move the eye, the camera moving does, the skater option turns it, the orbit caps), `set_eye_round_trips_a_follow_eye`,
`replay::tests::a_mod_camera_override_goes_on_restore_and_disable`, skate-mods `replay_set_camera` validation.
The advanced flag of `82C7FFB8` only picks the key insert path (`82835F08` arg 0 / 1); the written values and ids
are the same (b671). Open: the laptop retest.

### The Follow eye frame is upright (2026-10-25, USER-ASKS 350, research b605 / b606, recomp run m350)
Problem (laptop session game-20261025-152135): "camera still goes under ground." on c217a21f. An Advanced Follow key
edited low (eye local [-1.89, -0.75, -1.91]); the edit-time constraint held it 0.137 m up (headless repro
`replay::tests::a_follow_key_pitched_down_on_downtown_stays_above_the_ground`), so the dip came on playback: ours rode
the recorded camera's full rotation, and a camera pitching on a landing swung the low eye into the ground.

Retail [trace]: recomp hooks in `hooks_replay.cpp` (category `replay`: `RCAMN` call counts per caller, `RCAMF` the frame
`82C84CF8` returns, `RCAMRAW` the raw matrix `82672118` copies), recomp input script m350_follow_key
(Replay Editor, Edit timeline, rewind, Edit clip, Follow Camera, commit, scrub through the key), trace
(local). At the commit the raw recorded camera (game axes, y up) had row 1 = (-0.135, 0.991,
-0.007), pitched about 7.8 degrees, while the Follow eye frame (space 0, object 2) came back as rows (0.039, 0.999, 0),
(-0.999, 0.039, 0), (0, 0, 1) at the camera's position: the camera's side axis (row 0) with its vertical part removed
and renormalized, up = world up. That is the upright form b605 read in `828F36B0` (+0x194). `82C84CF8` / `82C82A50`
run only on edit events (editor open, commit, exit); `82672118` runs only while the replay is open. The camera-system
path `828F1C90` -> `828F2C38` b606 named did not run at all, in plain replay or in the Follow edit (hook `CAMSYSN`, 0
calls in runs 1 to 4).

Change: `FollowEye::Camera` (retail, default) builds the eye frame upright from the recorded camera (`camera_key.rs`
`upright`); `FollowEye::CameraFull` keeps the full frame (ours from m346 until now), `Skater` the root. Mods:
`sdk.replay.set_camera{follow_eye = 'camera' | 'camera_full' | 'skater'}`.

Tests: `camera_key::tests::follow_eye_frame_is_the_camera_heading_only` (pitch leaves the frame unchanged, roll tips
it only by the flattened side axis, the low eye stays where it was placed, the full frame swings), skate-mods
`replay_set_camera` validation with `camera_full`.
Playback [code b611 + trace run7]: every frame the render update `8279E350` ticks the editor camera `82C8A960` ->
`82C8B9F8` -> `82C8C470` (the per-frame twin of `82C84CF8`, ~490 calls a second per caller). Hook `RCAMP`
(a local trace): the eye call (lr `82C8BC40`) is space 0 object 2 and the look call (lr `82C8B95C`)
space 0 object 4 in all 1056 logged eye calls, with the three flag arguments (r7..r9) 0 throughout: no smoothing and
no extra upright step, so the eye rides slot 1's own upright frame live every frame, as ours does (`camera_key::pose`
per frame from the recorded camera at the cursor).
Open: the laptop retest.

### A camera picked on a clip with no keys covers the whole clip (2026-10-25, USER-ASKS 341, research b603 / b604)

Question (left open by b593 when the tripod fix was confirmed): which camera retail shows before the first camera
key of a clip when that key is placed mid-clip. Ours showed the recorded Game camera there.

Retail [code, research b603, `b604-pose-before-first-key.md`]: the camera type
flags are stored per segment, and an inserted segment takes the element's live value (`82C88A00`); a fresh clip's
camera channel is one Game segment (`82C7F728`). A pick (`82614148`) of the same type inserts a key; of another type
on a clip that is one Game segment it replaces the section with no dialog; otherwise a confirm dialog asks first
(`826142F8`). The replace (`82614658` with replace = 1; verified the calls in the recomp: `82C7FB08` merge,
`82C7FFB8` insert, `82C7FE88`, `82C7F0A8` delete, `82C7FFB8` insert) leaves the section from its start to the
playhead with the picked type, its eye / look records converted into the new camera's space at the pick
(`82C854B8` / `82C856C8` via `82C852F0`). The key edit writers touch only the new key's segment (`82C84868`, state
0xF start record, 0x10 end record), so for a basic (hard) key the part before the key holds the unedited pick pose
and cuts to the edited pose at the key; for an advanced (soft) key the shared record ramps into it.

Change: `Editor::pick_camera`: a non-Game pick on a clip with no keys also puts a key with the same camera, lens and
hardness on the clip's In point (the key at the playhead is the one edited, as before). Data:
`CameraRules::fill_section` (retail on); a mod turns it off with `sdk.replay.set_camera{fill_section = false}`.
Test: `editor::tests::camera_keys_hold_cut_and_blend` (the part before a first Tripod key shows the key's pose).
The keyed-clip case is ported below.

### A camera picked on a keyed clip asks, then replaces its section (2026-10-25, USER-ASKS 341 follow-up, research b609 / b610)

Retail [code, research b609, `b610-section-start-pose.md`; verified the
`82614148` branches in the recomp]:
- **Insert or replace (`82614148`):** rows 0..6 are items 0, 1, 3, 2, 4, 5, 6, compared with the section's kind
  (`82508CE8`). A basic row inserts only over its own camera with hard keys (or the In / Out points) on both sides
  (`82508828`); an advanced row inserts over its own camera. Otherwise the pick replaces.
- **Confirm:** a replace asks "Change Camera" (`ID_REPLAY_EDITOR_REPLACE_CAMERA_TITLE` / `_DESC`, No / Yes; callback
  `826142F8`: Yes replaces, anything else does nothing), except when the channel is one Game segment (`82C7F3D8` == 1,
  kind 3): then it replaces at once (the b603 case above). An advanced-row replace at the 100-key cap is refused
  (`82613500` / `82613598`).
- **Replace (`82C7FB08`, `82C7FFB8` / `82C80298` / `82C80540` / `82C807F0`):** the section runs from the nearest hard
  key at or before the playhead (or the In point) to the next hard key (or the Out point). Its soft keys go, its start
  key takes the new type with its stored eye / look re-expressed in the new camera's space (old space -> world -> new
  space with the frames at the playhead, `82C852F0`; the look height stays between Follow and Tripod), and one key goes
  in at the playhead. Keys outside the section stay.

Change:
- `camera_key::convert_key`: Follow <-> Tripod move the eye between the Follow eye frame and world and keep the look
  height; any camera -> Free is built from the old pose; Free -> Follow / Tripod keeps the eye (look height 0, see
  Open); -> Game is Game; a Game start has no stored pose and takes the new key's.
- `Editor::pick_replaces` / `pick_asks` / `pick_camera` (the decision, the section replace, then the key at the
  playhead); `replay.rs`: `CameraAsk` raises the confirm (`camera_popup`) like the marker questions, an open confirm
  owns the pad, Yes picks.
- Data: `CameraRules::fill_section` (retail on) covers the whole section rule; a mod's
  `sdk.replay.set_camera{fill_section = false}` makes every pick a plain insert again.

Tests: `editor::tests::a_camera_pick_on_a_keyed_clip_replaces_its_section` (keys hard 2, soft 4, soft 6, hard Free 8,
soft Free 10; Follow at 5 asks, clears the soft keys, turns the 2 s Tripod key into Follow with the same world eye and
look height, keeps 8 and 10; Advanced Tripod over Tripod only inserts; `fill_section = false` only inserts);
`camera_keys_hold_cut_and_blend` now expects the Advanced Free pick inside the Tripod section to turn its 5 s key into
Free. Not play-tested.
Open (b610, b613, code only, no trace yet):
- Free start -> Follow: retail stores the world look point in the Follow eye frame but labels it object 4, so each
  frame it is re-applied at the skater root (the look lands about one camera distance past the skater); -> Tripod: the
  look is the pick-time point carried with the skater. ANSWERED 2026-10-29 (b655): retail's edit update zeroes the
  lateral look every frame, and a pick always opens the edit, so a look height is all that survives, as ours keeps it;
  see "Look lift since the pick".
- Game section records: a fresh clip copies the take's live elements (likely zeros, unproven); a Game key from a pick
  holds the old camera's local pose then zeros. Ours keys a Game start from the view, which matches retail's eye for
  zeros; retail's look is the root origin, ours root + target height.
- Settling hook (RCAMCV, in b613): `82C856C8` / `82C854B8` entry and exit plus `82C7F728` exit, scripted Free then
  Follow and Game then Follow picks.
- The popup's cancelable flag and default (assumed cancelable like the other editor questions); a pick inside the
  minimum spacing during a replace.

### Replay popups own the pad (2026-10-25, USER-ASKS 342)

Problem (laptop session `game-20261025-114137`, the user: opening options (X while editing a clip)
did not block input to the replay, so inputs could unpause it or change the clip in the background). The
log: with the film pack menu (X) up, D-pad presses moved the clip (`REPLAY_CAMERA shot` lines, a refused marker), the
A that picked Replay Mode also switched the editor Timeline -> Effects, and the B that closed it switched back.

Cause [code, ours]: `replay::controls` stops editor and transport input while the quit warning, a marker question or
the photo flow waits, but not while a film pack popup does (`film_ask`), and on the frame an answer arrives the
press that gave it went on to the editor. Retail's popup takes the pad while it shows (`0x825A4F00`: input blocked
until the next show, as `popup_host.rs` notes).

Change: the gate also covers `film_ask`, and a frame with a replay popup answer runs no editor or transport input.
New log line `REPLAY_EDITOR play cursor=..` / `pause` whenever the replay starts or stops (`log_transport`).
Test: `the_film_pack_finished_menu_works_as_retail` (A, D-pad and RB under the open menu change nothing).

## M2 step 2: editor session and key bindings (skate-core)

`skate_core::replay_edit::session`: retail's editor modes (the values of `[DAT_83067084]+0x30`: 1 playback, 2 trim,
5 instant, 6 timeline, 7 / 8 / 9 / 10 camera key edit, 11 effects) and a binding table (mode, FE key index, action,
next mode) with retail's proven bindings as the default (research b386 / b387 / b389; `826127B8` input handler,
`82610458` opens mode 5 from Back during play): instant A -> timeline, D-pad Left / Right pressed jump to the ends;
timeline B Finished, X photo, RB play (back to the timeline at the clip's end), LB held trims until released, L3
panel, released D-pad Up / Down remove / drop a marker, Left / Right jump to the ends, Start help; effects Up / Down
item, Left / Right value, B commits. Unproven transitions (camera key edit entry, modes 0xC / 0xD) are left out. The
table is data a mod can replace. Not wired to the game yet: our current replay controls keep working until the
FE 12 host lands (retail differences listed in the plan: Back opens only, B opens Finished, D-pad jumps, 150-frame
minimum). Test: retail's mode transitions.

## M3 step 1: the look lanes as a color matrix override (skate-core)

`ReplayEdit::look_at(clip, frame, base)` turns a clip's look lanes into what the screen uses: the world grade's
color matrix record with the lanes' terms replaced, plus an RGBA tint overlay. Retail (research b380): the lane
consumers (`82686DF0` brightness, `82687FD8` contrast, `826888B8` saturation) write an override copy of the world
color block that the post FX update uses instead of the base while set (verified `82686DF0` stores the near and
far add terms at +0x7C00 / +0x7C40); brightness is the matrix add term on R, G, B; the values replace, never
multiply; lane 6 is a separate RGBA screen tint. Our `colour_matrix::build` takes the same roles as retail's builder
`827F13D8`, so the override record goes straight into it. A lane at its neutral default keeps the world's term
(OURS: when retail counts a lane as set is not read), so zombie mode's grade stays under an untouched replay. Not
wired to the renderer yet (M3 step 2, with the editor UI). Test: untouched lanes keep the grade; grayscale,
brightness and contrast replace both bands; a tint marker shows from its frame.

### Open
- The camera frame's opacity / blur / shake are not used by the renderer, so the recorded transform and FOV are all
  playback needs. (Held hand props are recorded: "Held hand props" above.)
- Retail editor cameras (for M2, research b376 / b377): ICELITE keyed values (eye, look, dutch, lens mm), lens
  10..500 mm, default 24; shake levels 0 / 0.35 / 1.0 / 1.0 for amplitude and frequency (`8250A6E8`); effect lanes
  step at marker times (speed +10 % up to 100 %, shake 0..3, brightness, contrast, saturation, tint).
- The player is still recorded as full `Mat4` skin rows (about 45 MB at 90 s); moving it into the quantized buffer
  would cut that to about a fifth.

## Ghosts sound in playback (2026-10-29, research b670)

### Problem
Actors that despawned inside the recorded window (ghosts) were drawn in playback but stayed silent.

### Retail [code, research b670]
- The world audio reads per-frame recorded data, never the live actor, and the same readers run live and in replay.
  Provider 2's reader (`827A76F0` -> `827BADD8`) keeps presentation objects per recorded id: created when an id is
  in the frame (`827BB568`), destroyed when it is missing (`827BB640`). The traffic audio `824B2A28` and the ped audio
  manager `824F2890` resolve their actors through that frame data, so a car or ped the census already removed sounds
  again for the frames that hold it. Vehicles have no impact sound at all (b409).

### Change
- `replay::components::track_on_ghosts::<T>`: `track` plus `ghost_values::<T>`, which inserts the recorded value on a
  ghost while playback shows it (`ReplayBuffer::shown`: recorded, not hidden, before its despawn; no pose decode) and
  removes it when it leaves the frame and on exit. Registered for `TrafficAudio`, `PedAudio`, `NpcSkaterAudio`,
  `AudioVelocity` and `VehicleHornState`; the audio bridge needs no change, and live play never sees a ghost's audio.
  Any tracked component can opt in the same way.

### Verification
- `replay::tests::a_ghost_sounds_in_playback_while_it_is_in_the_frame`: silent live, the recorded value in the frame,
  removed after its despawn time and on exit. skate-game replay 61 passed. Not heard in game yet.
- The replayed tazer / body-fall one-shots name the live ped; a ghost keeps `GhostOf(live entity)` and the bridge hands
  such a one-shot to it during playback (`world_bridge.rs` `ped_of`). skate-game 980 passed, skate-core 1248.
- Open: whether AI skaters are in retail's recorded skater list (b670, medium).

## Look lanes: retail's saturation is an offset (2026-10-29)

### Problem
The look lanes were ported in M3 steps 1 and 2 (above), but the saturation lane kept our own meaning: 100 = x1
(default), 0 = grayscale. Retail's look defaults had not been read.

### Retail [code, verified]
- The editor starts brightness and saturation at 0 and speed at 100 (`82506CD8`: editor +0x1A0, +0x1A4, +0x19C);
  the lane steps are `8250ADF8` (brightness -100..100 by 20, contrast 10..30 by 2, saturation 0..100 by 10).
- The saturation consumer `826888B8` copies SATURATE_NEAR (0x40) raw into the override block `+0x7C10` (slot 4) and
  SATURATE_FAR (0x3F) into `+0x7C50`, which the builder never reads.
- `827F3D20` calls the band builder `827F13D8` for both bands with saturation = slot 4 + 1.0 (`0x8231A844`); the
  builder's third argument is the saturation ((1 - s) times the luma weights).
- So lane value v (x 0.01, b379) gives saturation 1 + v / 100: 0 leaves the colors alone, 100 doubles their distance
  from luma. Retail's lane cannot desaturate.

### Change
- `skate_core::replay_edit::LaneSpec` gets `offset` (effect = value x scale + offset, data like the rest of
  `EditLimits`); saturation is 0..100 by 10, default 0, scale 0.01, offset 1.0. Contrast (10 = x1) and brightness were
  already retail's.

### Verification
- `replay_look_replaces_the_world_grade_terms_it_sets`: saturation 100 gives 2.0 in both bands (pure red -> (1.7,
  -0.3, -0.3)), a committed 0 gives 1.0 (the identity). skate-core `replay_edit` 15 passed, skate-game replay 60
  passed. Not seen in game yet.
- Open: contrast starts at 0 in `82506CD8` (below its range; a clip value presumably replaces it before use).

## Laptop session 2026-10-20 (water clock, HUD message log)
- Problem: Better Water kept moving while the replay was paused. Cause: the water shaders' clock (`retail_render.rs`
  `advance_frame_state`) read real time. Change: `WaterClock` follows the replay cursor going forward only (paused or
  rewound playback holds the water; forward-only because the ocean frame counter never steps back), real time in
  play. Test `water_clock_holds_while_a_replay_is_paused`.
- Open: after watching a takedown in the replay, a later chase message ("&@%#!!!", 16:38:09) was posted and hidden on
  time by the central HUD runtime but never seen (user: "No, nothing showed"). Not the FE HUD flag reset, the PC
  overlay, the replay message track or the cameras (read, not proven in game). New log line on a message's first
  frame: `HUD_CENTRAL draw draws= hud_draws= fe_hud= overlay= hud_off= failed=` (`scoring_hud.rs`).
- 2026-10-29: that first-frame line reads `draws=0` for every message (the movie draws nothing on its first frame), so
  it cannot tell a shown message from a lost one: 6 such lines in the sessions since, none after a replay. New line on
  the hide: `HUD_CENTRAL shown frames= drawn= hidden= peak_draws=` (frames it was up, frames it drew, frames the HUD was
  hidden, most draw slots). Cause still open; the next lost message shows `drawn=0` and whether `hidden` covered it.

## Extended Replays (PC overlay, not retail; user 2026-10-20)
- Ask: "Perhaps we can add an extra option for extended length with a slider from 2 mins to 4 mins? "Extended Replays"".
- Change: PC overlay Extras > Replay: "Extended Replays" (Retail 1:30 / On) and "Replay Length" (2:00 to 4:00, 30 s
  steps; `graphics_menu.rs` `REPLAY_LENGTHS`, saved in the overlay settings file). `replay.rs` `set_user_length`: the
  recorder's base limits become the player's length (frames at 60 Hz, byte budget scaled by the same factor from
  retail's 90 s / 128 MB). A mod's `sdk.replay.set_limits` still wins; clearing it returns to the player's length.
  Log `REPLAY_LENGTH user= max_frames= byte_budget= mod=`. Test `extended_replays_scale_the_window_and_a_mod_still_wins`.
- Memory: 90 s of a busy world filled ~125 MB (laptop session 2026-10-20), so 4 min is ~335 MB.

## Laptop session 2026-10-20 11:35 (still props, peds at a distance, ped alerts)
- The user reported that the bench and the ped both vanished at the start of the replay and reappeared when they
  became relevant again; the bench pops back into view when the replay comes back within about
  5 to 10 m; and the alert over a ped is always shown during replays and should probably be hidden.
- Bench: a still prop records a key only on a change; its first key came from its `Added` tick, which most render
  frames skip (no new 60 Hz replay frame), so the prop had no key until it next moved and played back hidden. Change:
  `replay/world.rs` records any actor without a track. Test `a_still_prop_added_between_replay_frames_still_plays_back`.
- Ped (#3, alive the whole window): `present_ped_pose` kept running its draw distance fade and LOD against the
  camera's distance to the ped's frozen live body, so a replayed ped only drew near its live spot. Change: no
  distance fade or LOD drop while the replay is open (every recorded ped draws at full detail).
- Ped alerts: hidden while the replay is open (`ped_alerts.rs`), like the rest of the gameplay HUD.

## Retail instant replay screen, step 1 (FE 12 Replay2 on the APT stage; b390 / b391; user 2026-10-20)
- Ask: "then finish with the retail ui parity for the replay".
- Setup: `tools/prepare_menu_movies.py` exports `replay2/replay_editor2` and `replay2/replay_help` (menu set version
  11; this engine still reads 5..11, older engines refuse an 11 set, so an install refreshed with this build needs
  the other launcher versions updated too).
- Host `replay_menu_host.rs` answers `FEReplayMenu.*` from a per-frame `ReplayFeed` (`replay.rs`): IsInstantReplayMode,
  Get Cursor / Playhead Position (0..1), GetEncodedLength (`ID_REPLAY_EDITOR_LENGTH` "Estimated Video Length: {0:time}";
  NOT RETAIL YET: our m:ss for the undecoded `{0:time}`), IsEncodedLengthValid (825ADF88: under 61 s), CanSaveMedia
  (true on PC), CanDisplayShowHide (instant), Ready.
- Stage (`retail_menu_stage.rs`): opens Replay2 when the replay opens (the movie in the set and the template in the
  tables, else our Bevy panel stays), shows it after its IntroComplete with ScreenManager.ShowScreen (b391: retail never
  calls InstantDone), posts SetState 999 on cursor moves and 18 / 19 on play / stop edges, closes it with the replay.
  The Bevy panel shows only without the retail screen (`RetailReplayUi`).
- Found by the verify run: a first-frame panel state (1003) hid the panel ("Show" prompt), so panel / play states go
  only on changes; which of 1002 / 1003 hides is open (the L3 toggle is not ported).
- Verified: stage test `retail_replay_editor_opens` (loads, intro, the six natives answered, none left), host test
  `natives_answer_from_the_feed_and_edges_become_states`, a verify screenshot with the screen drawn (timeline,
  transport row, Instant replay bar, Estimated Video Length, A / X / B / Start prompts, Hide).
- Next: Start = help (FE 13 ReplayHelp), L3 hide / show, the prompts' actions (A edit timeline = the editor, X photo,
  B Finished popup with a "Save to PC" row), the `{0:time}` format, the L3 icon missing next to Hide.

## Retail instant replay screen, step 2: help (Start) and hide / show (L3) (2026-10-20)
- The replay movie's own SetState (read from its actions with `tools/claude-skills/retail-menus/tools/apt_dump.py`)
  names its states [data]: 999 kMovePlayHead, 1000 / 1001 kMoveStartMarker / kMoveEndMarker, 1002 / 1003
  kShowEditor / kHideEditor (the whole panel; the movie then shows its "Show" prompt), 18 / 19
  kStartPlaybackReplay / kStopPlaybackReplay, 20 kOnlineInstantReplayMode, 21 / 22 kShowToggle / kHideToggle (only
  the Hide / Show prompt, `ForceShowHideToggle`). Step 1 had 1002 / 1003 the wrong way round.
- L3 (H on the keyboard) hides / shows the panel: kHideEditor / kShowEditor on the edge. With our free camera on, L3
  stays its speed modifier. The Bevy fallback panel hides too.
- Start opens the help (FE 13, template ReplayHelp, `replay2/replay_help`, exported since menu set 11): it asks
  `FEReplayMenu.GetHelpMode` (its kInstantReplayMode = 0, two pages) and turns the page on SetState(0)
  (`NextHelpPage`); A turns the page, B or Start closes it, nothing else reaches the replay meanwhile. Only with the
  retail screen.
- Verified: stage tests `retail_replay_editor_opens`, `retail_replay_help_opens` (GetHelpMode answered, page turn
  leaves nothing unanswered), host test (L3 states), a verify screenshot of "Help - Instant Replay" over the replay
  (`SKATE_VERIFY_REPLAY_HELP=1`). Not play-tested yet.

## Retail instant replay screen, step 3: Take Photo (X) (2026-10-20)
- Retail X (`826147C8`, b386): stops playback, asks with a confirm popup (callback `82614940`) and saves the photo.
  Ours: the same with retail's strings (Take Photo / "Save Photo?" No / Yes, then "Your photograph has been saved
  successfully." OK; NOT RETAIL YET: the popup title / options are ours). The photo is the world image the game renders
  into (`graphics_menu::SceneTarget`, captured with `Screenshot::image`), so it holds no UI; saved as a PNG to
  `Pictures\Skate 3 Rust` (`SKATE_PHOTO_DIR` overrides; retail saves to My Media). P on the keyboard. Log
  `REPLAY_PHOTO saving <path> at <cursor>`.
- B stays retail: without sign-in / storage retail's Finished shows the quit warning (b387), which is ours; the
  Finished menu with our "Save to PC" row comes with the video export.
- Verified: `retail_screen_keys_help_panel_and_photo` (L3, Start / A / B help, X photo flow), a verify run with
  `SKATE_VERIFY_REPLAY_PHOTO=1` wrote a 1280 x 800 PNG of the world without UI. Not play-tested yet.

## Retail instant replay screen, step 4: prompt icons and the length text (2026-10-20)
- The screen's prompts ask for HUD-style button names (`L3_Click_hud` for Hide, `EnableShowHide`; the transport row
  likewise); `prompt_glyphs::resolve` now reads `<name>_hud` as `<name>`, so Hide shows its L3 glyph and the transport
  buttons their RB / LT / RT / D-pad glyphs (all were blank). Test `every_retail_button_has_an_equivalent_on_every_family`.
- Length text (research b397; verified `ID_COMMON_METRIC_TIME` =
  "s" and `sub_824FE900` in the recomp): retail's `{0:time}` (handler 824FE900) is the number grouped by 3
  (`8290E370`) + " " + `ID_COMMON_METRIC_TIME`, not a clock: "Estimated Video Length: 42 s". NOT RETAIL YET: the
  default decimals (0 assumed). The general `{0:time}` / distance / speed types in `fe_language.rs` are a follow-up.

## M2 step 3: the editor timeline and the Pro Edit Menu on the retail screen (2026-10-20)
- A (panel shown, retail screen, at least 150 timeline frames) leaves the instant replay for the editor's timeline
  (`82613988`: mode 6, SetState 21, 9; b386). `replay/editor.rs` puts skate-core's model on the recording: one clip
  over it in 30 fps timeline frames, the session's mode, and the effects row.
- Timeline (mode 6): RB plays the clip from the cursor at its speed lane and stops at the out point (from the in
  point when at the end); LB held (mode 2, SetState 1005 / 21) trims: left stick x the in point, right stick x the out
  point, at retail's rate (0.005 of the timeline per 30 Hz console frame at full stick, b387), the playhead on the head
  that moved (the movie draws the heads at the playhead, b398); D-pad Down / Up released drop / remove a marker at the
  cursor; D-pad Left / Right released skip to the previous / next marker, else the in / out point (the screen's
  "Skip to previous / next marker"); B Finished, X photo, Start help (help mode 1), L3 hide. Keyboard: Space, L,
  M / N, comma / period, Enter.
- A opens the Pro Edit Menu (mode 0xB, SetState 22, 4): 9 rows (b398, item table `8302CCE0`): 0..6 cameras (A selects;
  NOT PORTED YET, Left / Right do nothing there), 7 speed (10..100 % step 10, `8250ADF8`), 8 shake (0..3 wrapping,
  `8250AB28`); Up / Down move the row (SetState 5), Left / Right change the value at the cursor (SetState 6), B back
  to the timeline (SetState 21, 9). It opens on the game camera row (2), the only camera our keys have.
- Natives (b398, AS integers): GetEditIndex, GetSimpleSpeedSetting, GetShakeCamType, GetCurrentCameraEditIndex (2),
  GetStartHeadPosition / GetEndHeadPosition (30 fps frames); GetCameraZoomString empty (NOT RETAIL YET).
- Verified: editor test `opens_over_the_recording_trims_marks_and_slows`, host test (mode states), verify screenshots
  of "Edit timeline" (marker and in / out point prompts, "Estimated Video Length: 10 s") and the Pro Edit Menu
  (`SKATE_VERIFY_REPLAY_AT=12 SKATE_VERIFY_REPLAY_EDITOR=timeline|effects`). Not play-tested yet.
- Open: the camera keys (follow / tripod / free and the advanced ones, b376), shake applied to the camera, the look
  lanes into the grade (M3), the markers drawn on the timeline (retail draws them from C++, the drawer not found yet,
  b398), the Finished menu with "Save to PC" (M4).

## M2 step 4: camera shake from retail's recorded clips (2026-10-20)
- Retail [code, b376 / b399 / b400]: the Shake row's levels play short recorded handheld-camera clips from
  `data/scene/ICELite.bin` (in `scene.big`; 14 ICELite sub-entries, index 0 empty), 30 keys per second, six channels
  (SHAKE_QUAT_X/Y/Z, SHAKE_POS_X/Y/Z, 16-bit, -1..1). Low = clip 3 ("Jog") at 0.35, Medium = clip 3 at 1.0, High =
  clip 4 ("RunFast") at 1.0 (amplitude = frequency). The evaluator `82C8AF50` builds a rotation (key i to i + 1,
  per-component lerp, `w = sqrt(1 - |xyz|)`) and a translation (key i only) and multiplies them into the camera
  matrix.
- Setup: `tools/prepare_replay_shake.py` (in the hud stage, `prepare_runtime_huds.py`) writes
  `private/hud/replay-shake/replay_shake.json`; the asset check offers "replay camera shake" until a refresh adds it.
- Engine: `skate_core::replay_edit::shake` (clips, levels as data, `sample`), `Replay::shake` (the shake lane at the
  cursor, timed from the in point), applied in `camera.rs` (camera space). NOT RETAIL YET: retail's gain is
  `2 x amp x sin(R)` with R a global angle whose writer is not found; ours holds `2 sin(R)` at 1, and normalizes the
  quaternion.
- Verified: `off_and_empty_clips_do_nothing_and_keys_step_at_the_clip_rate`, the asset check test, the exporter on the
  owned disc (13 of 14 clips with keys; Jog 119, RunFast 110), the game logs `REPLAY_SHAKE 13 clips`. Not play-tested.

## M2 step 5: the basic cameras (Follow, Tripod, Game, Free) (2026-10-20)
- Retail [code, b401 / b402]: A on a camera row of the Pro Edit Menu opens the camera's key edit mode: Follow
  `82614658` (mode 7, movie state 1), Tripod `82614A18` (8, state 2), Game `82614B70` (9, kDefaultCameraMode 10), Free
  `82614C78` (10, state 7); A commits (21, 9). Follow: eye and look in the skater object's frame (space 0); Tripod: a
  world eye looking at the skater; Free: all world; Game: the recorded camera, lens only. Per console frame (no dt):
  orbit 2.5 degrees per stick unit capped at 3, pitch +-85, distance 0.2 m cap within 0.5..1000 m, look height 0.01 m
  within +-1 m; Free turn cap 1.6 degrees, move 0.2 m; lens 0.5 mm within the editor's 24..75 mm (default 24).
- Ours: `replay/camera_key.rs` (CameraKey, CameraRules as data, `key_from_view`, `edit`, `pose`, `lens_fov`), one key
  per clip in `replay/editor.rs`, the edit modes in `replay.rs` `editor_controls`, the pose and lens applied in
  `camera.rs` (before the shake), `GetCurrentCameraEditIndex` from the clip's camera, the edit states posted by the
  host. Rates are normalized to 30 Hz so any fps gives the same motion.
- One pose per clip is retail for the basic cameras (b404: their setup clears the channel's keys and the sampler
  `82C89A50` holds the value without keys). NOT RETAIL YET: the skater root for retail's objects 2 / 4, the pad mapping
  (left stick turns, right stick distance / move, D-pad up / down height, left / right lens; retail's axis table
  unread), the lens to FOV (24 mm high gate). The advanced cameras (rows 4..6, keyed poses on the cubic curve below)
  came later: see "M2 step 7: camera keys".
- Retail's key curve (b404, for the advanced cameras): keys at u16 fractions of the clip, cubic Hermite per segment
  `v(u) = ((a u + b) u + m0) u + p0` (a = m0 + m1 - 2d, b = 3d - 2 m0 - m1), tangents linear (m = d) unless the
  element's CUBIC flag is set, then the non-uniform Catmull-Rom (Bessel) tangent across a soft key, times the key's
  TANGENT (0..8, default 1); hard keys cut; some elements are stepped.
- Verified: `follow_orbits_in_the_skater_frame_with_retail_caps` (orbit pose, 90 degrees in one second at full stick,
  pitch and lens clamps), full bin 850 passed. No screenshot (the user is gaming: no windows). Not play-tested.

## M2 step 6: the timeline strip (markers on the timeline) (2026-10-20)
- Retail [code, b403]: the movie's mCustomRender clip has `_type` "CustomTimeline"; the APT custom render dispatcher
  `825D4828` calls `82509B80`, which draws in the clip's space (x = normalized time x 817) every frame: `82508E08` one
  untextured band per camera segment from y 3 to the clip height, colored by the camera type (normal / selected,
  alpha 0.65 / 0.85; selected = no key under the playhead, not instant mode), and `82509298` one 13 x 51 quad per key
  after the first with `key_default` / `key_glow` (under the playhead within 0.00889) or `marker_default` /
  `marker_glow` (hard markers); in the camera edit modes the key under the playhead is not drawn.
- Ours: `apt_scene.rs` emits a `REPLAY_TIMELINE` placeholder with the clip transform; `retail_menu_stage.rs`
  `timeline_draws` expands it from the replay feed (one band for the clip's camera, the markers as keys) with
  `TimelineLook` (retail's colors, sizes, snap; data). The textures are retail's, already in the menu set.
- NOT RETAIL YET: the band bottom (clip height 30 assumed), our markers live on the speed lane (retail's keys are camera
  channel keys carrying the values), no hard markers (Advanced cameras), one segment per clip.
- Verified: `timeline_draws_the_camera_band_and_the_keys`, full bin 850 before the test was added. No screenshot (the
  user is gaming). Not play-tested.

## The opening sound (2026-10-20)
- Retail plays UI sound 0x394015B68783BE9E as Back opens the instant replay (`825EB998`, b389); in our front-end sound
  table that key is the fe record `editor_select` (level 0.88). `replay.rs` sends `FrontendSound::named("editor_select")`
  on open (`OPEN_SOUND`, data). Test: `retail_quit_warning_and_minimum_length` checks the message. Not heard yet.

## M4 step 1: the export encoder (Save to PC) (2026-10-20)
- User: "Add `windows` (Recommended)" (for "Save to PC": Windows' own encoder). `skate-game` now depends on `windows`
  =0.58.0 on Windows only (the version already in the lockfile; one added lockfile line), features MediaFoundation,
  Com, LibraryLoader, Shell (for the save dialog next).
- `replay/export.rs`: `ExportSettings` (size, 30 fps, bitrates, 48 kHz; data), `VideoSink` with two backends chosen at
  open: Media Foundation (H.264 + AAC in an .mp4 through the sink writer; its functions loaded at run time from
  mfplat.dll / mfreadwrite.dll, research b383, so the exe still starts on N editions without them; top-down BGRA frames,
  interleaved 16-bit stereo sound, hardware encoders allowed) and the fallback, PNG frames + a WAV in `<name>_frames`.
  Log `REPLAY_EXPORT` when the fallback is taken.
- Verified: `exports_frames_and_sound_to_a_file` wrote a real 2 s .mp4 (1.19 MB) with Windows' encoder on this PC, no
  window. Built in the release (/MT, static) link setup since the replay merged into `hails-additions`.
- Next (done, sections below): render the clip frame by frame (replay cursor at the export rate, the world image read back from
  `SceneTarget`), the sound rendered offline from the replay audio, the save dialog, and the Finished menu with a
  "Save to PC" row (needs the game window to test: not while the user plays).

## M4 step 2: Save to PC (the Finished menu, the save dialog, lock-step rendering) (2026-10-20)
- B / Back in the replay (instant or editor) now opens our Finished menu with the retail strings: title
  `ID_REPLAYEDITOR_TITLE`, rows "Save to PC" (ours; data) and `ID_REPLAYEDITOR_EXIT` (retail's Exit = the quit, b387);
  B cancels. Retail's Upload / Save Replay rows need sign-in and storage and are not offered. Without the exporter
  resource the quit warning stays.
- Save to PC: the Windows save dialog (`IFileSaveDialog`, .mp4, its own thread), then `replay/export_run.rs` renders
  the clip (the editor's in / out points, else the recording) lock-step at 30 fps: per output frame the replay cursor
  is set, a one-shot `Readback` of `SceneTarget` brings back that frame's world image (rows unpadded from 256 bytes,
  RGBA -> BGRA, cut to an even size), and exactly 48000 / 30 samples of sound are pulled from the audio runtime the
  replay feeds (256-sample blocks, the fraction carried); the audio device plays silence meanwhile
  (`DEVICE_MUTED`), so the two never take the same blocks. Frames and sound go over a bounded channel to the encoder
  thread (`export::open_sink`: Media Foundation .mp4, else PNG + WAV). The controls are off while it runs; a popup
  says where the file went (or why it failed). Logs `REPLAY_EXPORT start / done / failed`.
- Verified: `the_finished_menu_offers_save_to_pc_and_exit`, `readback_rows_are_unpadded_and_swapped_to_bgra`, the
  encoder test; full bin 854 passed. Verify switch: `SKATE_VERIFY_REPLAY_EXPORT=<file.mp4>`.
- First game run (2026-10-20 evening, the user paused gaming): wgpu refused the readback (the scene image had no
  COPY_SRC usage); fixed in `graphics_menu.rs` (a usage flag only, rendering unchanged). Then a 10.5 s replay exported
  in about 9.4 s to a 16 MB .mp4 (1280 x 800, 30 fps, H.264 + AAC). Checked: the encoder test now decodes a frame back
  (Media Foundation's source reader) and finds a white top half on top (upright, colors right); the game export's first
  frame decoded to PNG shows the world without UI; its audio decodes to 10.5 s of 48 kHz stereo (quiet ambience, the
  skater standing still). The save dialog itself (B > Save to PC) is not clicked through yet; the user tests it.

## M2 step 7: camera keys, the key curve and the advanced cameras (2026-10-20)
- Retail [code, b405]: there is no separate advanced input mode. A camera row's setup `82C7FFB8(take, advanced,
  existing)` inserts a key at the playhead: basic rows (Follow / Tripod / Free) a **hard** key (both sides hard: the
  pose holds and cuts at the key), advanced rows a **soft** key (the camera blends through it on the channel curve).
  Then the same edit states as basic (7 / 8 / 10), but the movie gets states 12 / 13 / 14 instead of 1 / 2 / 3. An
  insert is refused within a minimum spacing of another key or the In / Out point (scaled by the clip length). In the
  timeline (mode 6, `82612C28`), A over a key marker seeks onto it and, on release, re-opens that key in the advanced
  state 12 / 13 / 14 without inserting; A elsewhere inside the clip opens the effects menu, where a camera row adds
  the key; D-pad Down release adds a marker (cap 100, `82613500`, popup `82613598`). b405 also corrects b404:
  `82C810A8` / `82C813B0` are effect lane setters 4 / 5, not key handlers; no pad path edits a key's CUBIC / TANGENT.
- Key curve (b404): `skate_core::replay_edit::curve`, retail's cubic Hermite per segment with linear tangents, or the
  Bessel (non-uniform Catmull-Rom) tangent across a soft key when the element is CUBIC, times the key's TANGENT; hard
  keys cut. Test `linear_by_default_cubic_across_soft_keys_and_cuts_at_hard_keys`.
- Ours: `replay/editor.rs` keeps a sorted camera key track (`CamKey`: frame, camera, hard / soft) instead of one key
  per clip. `pick_camera` inserts or replaces the key at the playhead, `reopen_at` re-edits the key under the playhead
  (edit states 12 / 13 / 14 posted by `replay_menu_host.rs`), `pose` returns the key in effect (a soft key blends into
  the next soft key of the same camera), `segments` feeds the timeline band (`retail_menu_stage.rs`: one band per
  segment, the recorded camera before the first key) and the markers come from the keys. D-pad Down drops a hard key
  holding the camera in effect (refused at a key, at the clip ends or at the cap). The advanced rows, the key kind
  and the pose blend are in `replay/camera_key.rs`. Cap, spacing and rates are data (`CameraRules`, editor limits),
  so a mod can override them.
- Fixes from the user's Follow edit screenshot: the right stick now orbits and the left stick moves in / out; the
  `DPad_UpDown` / `DPad_Arrows` prompts show the plain D-pad glyph (`prompt_glyphs.rs`); the Follow / Tripod cameras
  aim 1 m above the skater root (`CameraRules::look_target_height`).
- Deleting: see "Deleting keys" below (b407). NOT RETAIL YET: the stick mapping
  (retail's axis table is unread); the look effects (DLC lanes) are not ported.
- Verified: `camera_keys_hold_cut_and_blend`, the curve test, `follow_orbits_in_the_skater_frame_with_retail_caps`
  (orbit center now includes the look height); skate-core 1176 and full bin 855 passed. Verify switches
  `SKATE_VERIFY_REPLAY_EDITOR=markers` (two markers, the playhead glows on the second) and `=camera` (Follow picked at
  the playhead, then its edit mode). Not play-tested by the user yet.
- Look target (b406, 2026-10-20): Follow / Tripod look at the look object's origin plus LOOK_Z straight up; every
  frame `82C80A78` writes one constant (the "no input" value, very likely 0.0) to LOOK_X / LOOK_Y (elements 3 / 4)
  and writes LOOK_Z (element 5) back unchanged [code, verified in the recomp]. LOOK_Z defaults to 0.0 (`82903840`)
  and the D-pad moves it within +-1 m [code, research]. There is no fixed look height in the camera code: the target is
  the camera target entry flagged 0x400000 in the list at `[0x83083C38]+0x2E680` (its world matrix translation)
  [code, research, medium-high]. Which part of the skater that entry is (root, pelvis, a raised point) is OPEN; the
  cheapest proof is a short recomp trace of `826824F8` next to the skater root. Until then our
  `look_target_height` (1 m) stands for the target's height over the root, still NOT RETAIL.
  Recomp trace (2026-10-20, hook `CAMTGT` on the fork's research-hooks, trace research b406/camtgt.tsv`): the
  flagged entry is the skater's root at foot / deck level [trace]: standing on the board it sits 0.08 m above the
  board body, after stepping off it stays 0.09 m above the same ground while the board lies 3.6 m behind, it does not
  bob while pushing or walking, and it falls with the skater rolling downhill. With LOOK_Z reset to 0 when a camera
  is picked (b405, `82C7FFB8`), retail's Follow / Tripod look at the skater's feet until the D-pad raises it (max
  1 m). User 2026-10-20: "Retail: feet": `look_target_height` is now 0 (data, a mod may raise it); the D-pad lift
  still raises the aim per key.
- Deleting keys (b407, 2026-10-20) [code, verified the key id switch in the recomp]: b405 read `82612C28`'s case
  numbers as hex; they are decimal key ids, so D-pad Up released (32) calls `82613650`, the delete, and D-pad Left
  released (34) is the seek to the start. `82613650` asks first: over a clip marker (a hard key)
  ID_REPLAY_EDITOR_DELETE_CLIP / _DESC, over a key marker (a soft key) ID_REPLAY_EDITOR_DELETE_KEY / _DESC, No / Yes;
  no marker = an error sound. Yes removes the key (`82613910` -> `82C7FD60`); a clip marker also takes the soft keys
  in the clips on both sides (`82C7FB08`; retail's own text says so: "it will also remove all Key Markers in the
  clips on both sides"). D-pad Down asks ID_REPLAY_SAVEWARNING_TITLE / ID_REPLAY_EDITOR_POPUP_CLIPMARKER before a
  clip marker lands inside a run of soft keys (`82508828`). All six strings checked in the exported menu text.
- Ours: `Editor::marker_kind`, `drop_needs_confirm`, `remove_marker` (the sweep) in `replay/editor.rs`; the questions
  are engine popups (`replay.rs` `marker_popup`, callback `engine:replay.marker`), applied straight away without the
  retail menu stage. The merged clip's reset: see "Clip delete reset (b460)" below.
- Delete sounds (b459, 2026-10-21) [data: ids matched in the audio manifest]: `editor_up_marker`
  (0x9DEF3BCCEF922B96) as either question opens (none on Yes), `core_error` (0x47F720222F2CAED4) when no marker is
  under the cursor. Ours: `replay.rs` `editor_controls` hands the sound to `controls`, which plays it. b459 also read
  the clip-delete reset (camera channel only: lens to 24 clamped 24..75, channel 1 element 0xE to 0.1, speed 100,
  shake 0), the per-channel delete with the film DLC (active channel only; internal channels 5 = speed, 4 = shake),
  and the hit window (+-0.00889 of clip time, clamped to [0.00889, 0.99111]).
- Clip delete reset (b460, 2026-10-21) [code, medium-high on the whole-clip outcome: `82C82490` is the base game's
  normal per-clip speed setter]: on the camera channel, a clip marker delete writes the defaults at the merged
  clip's key: lens 24 (clamped 24..75), near clip 0.1 (`CLIPPING_NEAR`, no counterpart in ours), speed 100, shake
  off. Without the film pack the whole merged span (previous hard key to the next) starts at the defaults; with it
  only the lane value in effect at the cursor resets. Look lanes and soft key deletes are untouched. Ours:
  `skate_core::replay_edit` `reset_span` / `reset_at` (lane defaults from `EditLimits`, data), `Editor::remove_marker
  (time, channels)` resets speed / shake and the lens (`CameraRules`), `MarkerAsk::channels`. Tests
  `a_clip_delete_resets_the_merged_span_to_the_lane_defaults` (skate-core), `deleting_a_clip_marker_takes_the_key_
  markers_on_both_sides` (now checks the reset), `a_clip_delete_with_the_film_pack_resets_only_the_values_at_the_
  cursor`; the trim test now expects speed 1.0 / shake 0 after deleting the only key. NOT PORTED: the hit window
  clamp (same outcome for inner keys). Open: a recomp check of the whole-clip outcome (speed 50 on clip B, delete
  marker A|B, read the merged clip's speed).
  Test `deleting_a_clip_marker_takes_the_key_markers_on_both_sides`.

## M3 step 2 research: the look lanes (b408, 2026-10-20)
- Retail [code `82507438`, research; our code lines verified]: with the film DLC the edit channel (Y) is 0
  camera, 1 speed, 2 shake, 3 brightness (setter `82C81750`), 4 contrast (`82C810A8`), 5 saturation (`82C813B0`), 6 a
  four-value tint (`82C81AC8`); without the DLC only speed and shake. No vignette, grain or letterbox lane exists.
- Element defaults / ranges [code `82C8FAD8`, research]: brightness 0 (-1..1), contrast 1 (0..50) with midpoint 0.5
  (0..1), saturation 0 (0..1), tint X / Y 0 (-1..1), Z 1 (0..1); per-lane OVERRIDE flags (0x71..0x77, default 0).
- A committed value also sets its lane's OVERRIDE flag at the key, and the renderer applies a lane only where the flag
  is on [code, research; brightness setter read in full, the others by pattern; the check could not find the
  flag write in the recomp's `sub_82C81750` by a quick search: UNVERIFIED]. Ours counts a lane as set when it differs
  from its default (`replay_edit.rs` `look_at`, already labeled OURS).
- Sampling [code `82C87640` / `82C8E680`, research]: look values are interpolated (linear between soft keys), the flags
  stepped; ours steps every lane (`value_at`).
- Renderer (b380, re-read): a set lane replaces its term in an override copy of the world color block, built into the
  post FX matrix by `827F3D20` / `827F13D8`; the tint goes to a separate overlay. OPEN: who sets the +0x7E00 gate,
  the far band's extra saturation term, how the tint overlay blends, the DLC row labels.
- Port rule: explicit per-lane override flag with each commit, linear look values between keys, `look_at` into
  `colour_matrix::build` during playback, the tint as an overlay after the grade; lane list, defaults, ranges and
  interpolation per lane as mod data. NOT PORTED: the rows exist only with the film DLC; whether we offer them is the
  user's call (DLC support is the next work item). User 2026-10-20: "Always on" (offer them on PC without the DLC).
- M3 step 2, part 1 (2026-10-20): `skate_core::replay_edit`: `LaneSpec::interpolate` (look lanes ramp linearly between
  markers, speed / shake stay stepped; data), `Clip::set` + `lane_set` (a look lane applies once a value was committed
  on the clip or a marker sits at or before the frame: retail's stepped OVERRIDE flag; a committed neutral value
  still overrides the world), `lane_effect`, `look_at` on both. The game's `colour_matrix::update_grade` takes the
  editor's look during a replay (`Replay::look`). Test
  `look_lanes_ramp_between_markers_and_a_committed_neutral_value_still_overrides`. NOT YET: rows to edit the looks
  (b410 found the DLC editor movie `replay_editor3` with the tint / brightness rows in the user's Create Park Upgrade
  package), the tint overlay (retail's blend is open). Before the first marker a look holds the clip value (whether
  retail's In point key ramps into it is open). Retail's look defaults differ in meaning for saturation (SATURATE
  default 0, range 0..1) from ours (100 = x1); unresolved.
- Look rows UI (b410, 2026-10-20): retail gates every DLC editor branch on product CCF0007, the Skate.Create Upgrade
  Pack (`82518AD0` with the string at 0x822FD708; main counted 7 of its 32 call sites loading that string), not a
  Filmer Pack [code]. The user's Create Park Upgrade package carries `DLC_PRODUCT,CCF0007`, the DLC editor movie
  `source/screens/replay2/replay_editor3`, `replay_help2` and the creator language tables with the channel labels
  (ID_REPLAYEDITOR_CHANNEL_CAMERA .. _TINT) [data, verified in the package]. Retail: Y cycles the channel
  (Camera, Speed, Shake, Brightness, Contrast, Saturation, Tint), one panel per channel; new natives GetModeText,
  GetMode, GetModeSliderValue, GetCurrentTint, the tint getters [code, research]. The base movie `replay_editor2` cannot
  show the look rows. Port: the DLC pipeline extracts replay_editor3 + its language table from the user's package,
  CCF0007 counts as owned on PC (user: always on), the stage loads replay_editor3 when present; without the package
  our own one-line channel / value overlay (ours). Details in research b410.
- replay_editor3 handlers (b411, 2026-10-20) [code; verified the state 24 loads in `82612328` and Y's state 23
  after `82507FC8` in `82612C28`]: Y cycles the channel 0..6 (wraps) and posts state 23 (kUpdateMode by
  elimination); entering the effects panel posts 22 then 4 for every channel; on the Tint channel Up / Down move the
  component cursor R, G, B, A (wraps) and post 24 (kRefreshTint by elimination), other channels post 5; Left / Right
  post 6 and snap a tint to steps of 25 (`(v +- 25) * 0.04 + 0.5`, truncated, x 25; clamp 0..250). New natives:
  GetModeValue (raw brightness / contrast / saturation, else -1), GetCurrentTintValue (raw component, else -1).
  Template 71 = `replay2/replay_editor3.swf` (with CCF0007), 72 = `replay2/replay_editor2.swf`. The movie's own constants (read 2026-10-29 from our export `replay2/replay_editor3.json`, the frame-1 setMember
  pushes): kEffectsMode 4, kRefreshEffectsMenu 5, kUpdateEffectValues 6, kUpdateMode 23, kRefreshTint 24, kReplayMode_Camera
  0, _Speed 1, _Shake 2, _Tint 6: every reading by elimination above holds. Open: whether B reverts edits.
- DLC package reader (2026-10-20): `tools/owned_game/stfs.py` reads the user's STFS (Xbox 360 package format) packages (header, file table,
  consecutive files past the interleaved hash tables); tested on the Create Park Upgrade package: the 95,990,656-byte
  `creator_00000000.big` copies out and parses (1262 entries, `creator0.ver` = `695423:sk8build`). It carries
  `data/fe/source/screens/replay2/replay_editor3.*`, `replay_help2.*` and the creator language pair
  (`LANGUAGE_Labels_creator_skate3ng-dlc_creator.BIN` + English); the labels table ends with one NUL where the disc's
  end with two (one string short of the parser's count).
- Tint overlay (b413, 2026-10-20) [code; verified `827FE770`'s counter read at +0x347AC, its blend state call
  `82CFC990` and the draw `824FA4A0`]: retail draws lane 6 as one constant color on a full-screen quad, straight alpha
  (SRC_ALPHA / INV_SRC_ALPHA), after the scene and before the front end's Render2D, so the HUD, menus and the editor
  are never tinted; the setter `82C81AC8` sets TINT_OVERRIDE, the consumer `82688E78` copies R, G, B, A to the overlay
  and clears it when the override is off [code, research]. Ours: the grade gets a ninth row (RGBA, alpha 0 = none) and
  the tone pass mixes it in as its last step (`retail_tone.wgsl`), the same result as the quad drawn right after the
  grade, under the UI, and carried into Save to PC. Open: whether retail's tint lands before or after its grade
  (inferred after, medium).

## M2 step 8: edits land on commit, hard and soft lane keys (2026-10-21, research b461 to b466)

### Problem
Our editor wrote a marker at the cursor on every Left / Right press, stepped every lane except the looks, and let a
value set on an empty lane apply to the whole clip. Retail does none of that.

### Retail (evidence)
- Left / Right only step the menu's live value (`8250ADF8` / `8250AB28` call nothing but the DLC check) [code]. The
  live values load from the take only when the effects menu opens (`82508930` state 0xB -> `825079A0`; without the
  pack speed and shake, with it the active channel only) [code].
- The commit is `82507438`. Base game: B closes the menu and writes speed and shake into the segment under the
  playhead, unless a marker is under it; leaving a camera key edit (states 7..10) runs the same commit. Film pack: B
  cancels; A writes (a soft key at the playhead only when the segment touches a soft key, `82508828`), X always adds a
  soft key, except on shake; over a hard key nothing [code].
- Lanes hold hard keys (cuts: D-pad Down, `82C7F610` on channels 0, 5, 4 together without the pack,
  `82835F08(take, 1)` on the active channel with it) and soft keys (A / X). Each segment has a start and an end record;
  a soft key shares one record with the segment before it (`82C892D8`, `82C879D8`) [code].
- Playback `82C89A50`: interpolated elements ramp linearly from a segment's start to its end record (TIME_SCALE speed
  registered interpolated, `82C8FAD8`; the looks; shake amplitude / frequency), stepped elements take the start
  (SHAKE_TYPE); int elements round floor + (fraction >= 0.5, `8209975C`) [code, b463 / b464].
- A fresh instant replay opens on a blank take: no keys on any lane (`82C88428`; `82836AB8` is unreachable) [code,
  b466]. With no keys the sampler reads the lane default, so a commit that adds no key to an empty lane is lost:
  in the base game, speed set and closed with B before any D-pad Down marker stays 100 % [code, medium-high, b465;
  no recomp trace yet].
- A in the timeline over a hard key, or at the clip's ends, plays `core_error` and does nothing (`loc_826130AC`);
  over a soft key the playhead snaps to it (`825088B8`) [code].

### Change
- `skate_core::replay_edit`: `LaneKey { frame, value, end, soft }`, `Clip::ends`, `Clip::segment`,
  `Clip::cut_both_sides`; `ReplayEdit::insert_hard` (keeps the curve), `commit` (-> `Commit::Segment / SoftKey /
  Lost`), `remove_key` (a hard key takes the soft keys around it), `sample` (per segment ramp), `value_at` (retail
  rounding), `lane_effect`. Speed ramps (`LaneSpec { interpolate: true, round: true }`).
- `replay/editor.rs`: `Editor::live`, `load_live`, `adjust_row` (live only), `commit_base`, `commit_pack`,
  `channel_lanes`, `a_target`; D-pad Down / Up and the delete question work on the active channel's lane with the
  pack. `replay.rs` binds B / A / X and the key edit exit to them; the camera shake ramps amplitude and frequency
  across a segment.
- Data (`EditLimits`, Lua-reachable with the other limits): `round_threshold` (0.5), `empty_lane_commit` (false =
  retail; a mod may let an empty-lane commit write the whole clip), per lane `interpolate` / `round`.

### Verification
skate-core `replay_edit` 15 tests (new: spacing and cap on keys, the lost empty-lane commit and its mod switch, the
base commit between clip markers, soft keys with the speed ramp, half-up rounding and the hard-key split), skate-game
replay 44 tests (new: the film pack lane channels). Not play-tested.

### The encoded length and the heads (b472) and the replay screen natives (b470)
- Retail `8250B590` [code]: per segment of the speed lane (hard and soft keys alike, positions over the whole take),
  the frames inside the in / out heads divided by the segment's START value x 0.01 (a ramp is not averaged), truncated
  per segment, summed; seconds = trunc(sum x 1/30 + 0.5). Heads only clip: a segment straddling the in head counts
  at its own start value, so retail keeps every key when a head moves. Ours: `Clip::take` (the span the lanes run
  over), `trim` keeps the keys, `encoded_seconds` as above with `EditLimits::length_round_bias` (0.5). A lane without
  keys counts as one segment at the clip's value (ours; b472 did not read the empty-lane count).
- The replay screen feed (`replay.rs` `publish_feed`): the length is `encoded_seconds()` while the editor is open,
  and `IsEncodedLengthValid` reads the feed's `length_valid` from `EditLimits::share_max_seconds` (the host's own 60 s
  constant is gone). `CanDisplayShowHide` is true in editor modes 5, 6 and 1, as retail `825ADF00` (was instant
  only). Retail's `GetCursorPosition` returns a frame number where ours returns the fraction; the movie ignores it
  (it draws from GetPlayheadPosition), so it stays.
- Tests: `encoded_length_counts_speed_segments_inside_the_heads` (b472 cases B, C, C2, D, E), the host natives test.

### Open
- Recomp check 2026-10-29 (new rexglue SDK build, hook `RTAKE` in `hooks_replay.cpp` on `82C84CF8`): a fresh instant replay's take reads speed 100 (word 0x64 at take+0x74) with the speed / shake lanes (channels 4, 5) empty, as b465's blank-take reading says. The rest (speed 50, B, play) is not measured: `82C84CF8` runs only at replay enter / exit, and the blind editor script drifted into the exit dialog. Next: a hook on the commit `82507438` and a script checked step by step against shots.
- What shake level type 3 vs 4 looks like (the shake consumer is not read).

## Save to PC: its own string id (2026-10-21, b470 item 6)

The Finished menu's "Save to PC" row (ours; retail's rows need sign-in) was a literal English string. It is now
`ID_HAILS_REPLAY_SAVE_TO_PC`, with "Save to PC" as a default under the disc's language table
(`apt_text::ENGINE_STRINGS`), so a language table or mod text with the id replaces it. Test
`our_string_ids_have_text_a_language_table_can_replace`. b470 item 5 (2026-10-21, later): the editor and the instant
replay's share check now read one `Replay::edit_limits` (retail defaults), and `sdk.replay.set_limits{share_max_seconds
= n}` (1..600 s) sets its share limit; nil or the mod's disable restores 60 s. Test in `replay/tests.rs` (limits).

## Code reference: replay_edit.rs

Detail moved out of the code comments of `crates/skate-core/src/replay_edit.rs`; each comment there links its section here.

### Replay editor model

The replay editor's model (M2): clips trimmed out of the recording, each with its effect lanes of hard and soft keys, and retail's length rules. Pure data, engine independent; the game draws and plays it.

Retail (FEReplayMenu host `823011D8`, editor state `DAT_83067084`; research b343, b344, b376 to b381):
- The editor opens once the recording holds at least 150 frames (`8250B8F8`, > 149).
- The timeline counts 30 fps frames (`825ADD90`, 30.0 at 0x820D4924).
- Encoded length = round(sum over clips of (end - start) / (speed% x 0.01) / 30 + 0.5) s; sharing needs <= 60 s (`825ADF88`, `cmplwi 60`).
- Effect values per clip (record +0x50 speed, +0x54.. the look): speed 10..100 % step 10 default 100 (slows only; `82506CD8` stores 100), shake 0..3 wrapping (`8250A6E8`), brightness -100..100 step 20, contrast 10..30 step 2, saturation 0..100 step 10, tint R / G / B / A 0..250 step 25 (`8250AB28`, `8250B218`).
- Effect lanes (b461 to b466): per lane hard keys (cuts, D-pad Down) and soft keys (A / X commits). Each segment between keys has a start and an end value and ramps linearly on interpolated lanes (speed, the looks); a soft key shares its value with the end of the segment before it. Left / Right only step a live value; a commit writes it (the base game's B: the segment under the playhead; the film pack's A / X: a soft key). A fresh clip has no keys, and a write that adds no key to an empty lane is lost, as in retail.
- Lane values reach the screen as: brightness = the color matrix add term, contrast, saturation (near and far bands), each replacing the world block while set; tint = an RGBA screen overlay (b380).

Every limit is data ([`EditLimits`], [`LaneSpec`]) so a mod can change them (for example allow speeds over 100 %).

### Trigger scrub

Retail's trigger scrub, data driven. Per console frame (`frame_hz`, the 360's ~30 fps): target = (RT - LT) x `max_rate`, the rate moves `smoothing` of the way to it, clamps to +-`max_rate`, and stops inside the dead band; the playhead moves by the rate as a fraction of the whole recording (`82610F18` -> `825071B8` -> `82C7EF90`, research b387 / b388).

### Lane commit

Commit `value` on `lane` at `frame` (retail `82507438` -> the lane setters, b461 / b463 / b464 / b465):
- over a hard key: nothing;
- over a soft key: that key's segment takes the value (start and end; a soft next key shares it);
- `add_key` (X always; A unless the segment is cut on both sides): a soft key at `frame` with the value, its segment ending on it when it ends at a cut (the segment before now ramps into it);
- else the segment under the playhead takes the value (the base game's B).

On a lane without keys a write that adds no key is lost, as in retail, unless [`EditLimits::empty_lane_commit`] says otherwise. A soft key that has no room is not added and the write falls back to the segment (retail's insert fails and the setter takes its single write).

### Clip look

The look at `frame`: the world grade `base` with the clip's set look lanes replacing their terms (retail's consumers `82686DF0` / `82687FD8` / `826888B8` write an override copy of the world color block, research b380 / b408): brightness = the add term on R, G, B, contrast, saturation, in both bands (retail's far band takes the near saturation). Values ramp between markers. Plus the tint lanes as an RGBA screen overlay (0 alpha = none).

### Encoded length

Retail's encoded length in seconds (`8250B590`, research b472): per segment of the speed lane (hard and soft keys alike), its frames inside the heads stretched by the segment's START value (a ramp is not averaged), truncated to whole frames; the sum at the timeline rate, plus `length_round_bias`, truncated. A lane without keys is one segment at the clip's value (ours: b472 did not read retail's empty-lane count).

## Code reference: replay/world.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/world.rs`; each comment there links its section here.

### Replay world actors

Every drawn actor in the replay: NPC skaters, peds, traffic cars and props are recorded as drawn (root and joint local transforms, after their presenters) and played back from the recording instead of freezing, as retail's DataTracker records the census actors and skaters every frame (b364, `sub_827BF090`). Mods reach it through the components: any entity with a [`ReplayActor`] (and optionally [`ReplayJoints`]) is recorded.

## Code reference: replay/editor.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/editor.rs`; each comment there links its section here.

### Drop marker

D-pad Down released (b461 / b462): a hard key at the cursor that keeps the values in effect. Without the pack a camera key holding the camera in effect plus hard keys on speed and shake (`82C7F610`: channels 0, 5, 4 together); with the pack a key on the active channel only (`82835F08(take, 1)`). Refused at a key, at the clip's ends (the marker spacing) or at the cap (retail's error sound / popup, `82613500` / `82613598`).

### Remove marker

D-pad Up released, answered Yes (b407, `82613910` -> `82C7FD60`): remove the key at the cursor on the active channel. A clip marker also takes every soft key in the clips on both sides, up to the next hard key each way (`82C7FB08`; retail's own text, ID_REPLAY_EDITOR_POPUP_CLIPMARKER). On the camera channel a clip marker delete also resets the merged clip (research b460, `825086F0`): the surviving key before it gets the default lens (clamped to the lens range), and speed / shake take their lane defaults: over the whole merged span without the film pack (retail removes the marker on channels 0 / 5 / 4 / 1 together), only the value in effect at the cursor with it (`channels`). Look lanes and soft key deletes are untouched. Retail's near clip (channel 1 element 0xE, 0.1) has no counterpart here. Values are data (`CameraRules`, the lanes' `LaneSpec::default`).

## Code reference: replay_edit/shake.rs

Detail moved out of the code comments of `crates/skate-core/src/replay_edit/shake.rs`; each comment there links its section here.

### Replay camera shake

The replay editor's camera shake (the Pro Edit Menu's Shake row; research b376, b399, b400). Retail plays short recorded handheld-camera clips (`data/scene/ICELite.bin`, 30 keys per second, six channels: rotation x / y / z and position x / y / z, each -1..1) through the evaluator `82C8AF50`, which builds a rotation + translation and multiplies it into the camera matrix (camera space). Levels (`8250A6E8`): Off; Low = clip 3 ("Jog") at 0.35; Medium = clip 3 at 1.0; High = clip 4 ("RunFast") at 1.0; the same value is the amplitude and the frequency. Every value is data (mod-overridable).

### Shake sample

The shake at `t` seconds into the take: (rotation quaternion `[x, y, z, w]`, translation `[x, y, z]`) in camera space, or None when off or the clip is empty. Key position `f = t x freq x key_rate`, keys `i = floor(f) mod n` and `i + 1`; rotation per key `(qx g, qz g, -qy g)` with `w = sqrt(1 - |xyz|)` (retail's double square root, confirmed in the recomp), lerped per component without renormalizing; translation `(-pz g, -px g, py g)` from key `i` only (retail reads key `i` twice) [code `82C8AF50`, b399].

## Code reference: replay_buffer.rs

Detail moved out of the code comments of `crates/skate-core/src/replay_buffer.rs`; each comment there links its section here.

### Replay buffer

Instant replay recording of every drawn actor (NPC skaters, peds, cars, props, mod bodies).

Retail records presentation data, never a re-simulation: one tracker frame per sim step (60 / s), at most 5400 frames in a byte ring that evicts the oldest (DataTracker `82726170` / `82DBBAF8`, b345 / b346). Skater poses are LOCAL joint rotations as 4 x i16 quaternions (scale 32767), interpolated per joint between frames with nlerp (b365 to b367, decoder `8278ABE0`). This buffer keeps one track of keys per actor, keyed by a stable u64 id, and writes a key only when the quantized pose changed, so a resting prop costs one key for the whole window.

Engine independent: the caller supplies frame times and poses as plain arrays.

## Code reference: replay/camera_key.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/camera_key.rs`; each comment there links its section here.

### Replay basic cameras

The replay editor's basic cameras (Pro Edit Menu rows 0..3; research b401 / b402): Follow keeps its eye in the gameplay camera's frame (eye object 2) and looks at the skater, Tripod keeps its eye in the world and looks at the skater, Free is a world pose, Game is the recorded gameplay camera with only the lens to change. Controls run per console frame (30 Hz) with retail's caps (`82C826D0`, `82509558`). Every limit and rate is data ([`CameraRules`]).

A basic camera is one pose for the whole clip: its setup clears the channel's keys and the sampler holds the live value without keys (b404, `82C89A50`); keyed poses are the Advanced cameras'. The look object 4 is the skater root (b406, traced). NOT RETAIL YET: the pad mapping (retail's axis table `[pad+0xC]+4` is unread), and the lens to field of view (a 24 mm high film gate).

## Code reference: replay/camera_collision.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/camera_collision.rs`; each comment there links its section here.

### Replay camera collision

The replay cameras' world constraint (research b449): retail runs it on every evaluated replay camera (`82C8B698` -> `82683680`): first the 40 m leash to the player (`82682CB0`), then a swept sphere from the last accepted camera position to the new one with a slide along the hit plane, plus a line-of-sight sweep from the player to the camera (`82682D80`). No smoothing and no minimum height: corrections land in full on the same frame. Every value is data ([`CameraCollision`], `sdk.replay.set_camera`). NOT RETAIL YET: the 0.8 m / world radius triggers, the sweep's collision layers (ours: the map and its external queries, no actors) and who raises retail's snap request (+0x183; ours: a camera cut, see [`Constrained`]).

## Code reference: replay_edit/curve.rs

Detail moved out of the code comments of `crates/skate-core/src/replay_edit/curve.rs`; each comment there links its section here.

### Replay key curve

Retail's replay camera key curve (research b404, sampler `82C89A50`): keys at fractions of the clip (u16 / 65535 of its length; the clip's start and end are implicit keys), one cubic Hermite per segment `v(u) = ((a u + b) u + m0) u + p0` with `d = p1 - p0`, `a = m0 + m1 - 2d`, `b = 3d - 2 m0 - m1`. Tangents are linear (`m = d`) unless the element's CUBIC flag is set at the key; then, across a soft key, the non-uniform Catmull-Rom (Bessel) tangent `m = d dn / (dn + dc) + dN (dc / dn) dc / (dn + dc)` (`82C89828`, epsilon 1e-5), scaled by the key's TANGENT (0..8, default 1). A hard key has its own in and out values (a cut); stepped elements hold the segment's start value. Pure math, engine independent.

## Code reference: replay/export_run.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/export_run.rs`; each comment there links its section here.

### Replay export run

Driving a replay export (M4; research b383): the Finished menu's "Save to PC" row asks where to save (the Windows save dialog on its own thread), then renders the clip lock-step: each output frame sets the replay cursor, reads the world image back from `SceneTarget` (`Readback`, rows padded to 256 bytes), pulls exactly one frame of sound from the audio runtime (48000 / fps samples; the device output is muted meanwhile so the two never race for blocks) and hands both to the encoder thread (`export::open_sink`) over a bounded channel. Ours on PC; retail's Finished menu offers Upload / Save Replay only when signed in (b387).

## Code reference: replay/export.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/export.rs`; each comment there links its section here.

### Replay export encoder

Replay export ("Save to PC", ours on PC): frames and sound into a video file. Windows' own encoder (Media Foundation: H.264 + AAC in an .mp4, research b383) when it is there; else the frames as PNG files and the sound as a WAV in a folder of the same name, so an N edition without the Media Feature Pack still exports. Media Foundation's functions are loaded at run time (`LoadLibraryW` / `GetProcAddress`): linking them would stop the game from starting where the DLLs are missing. Settings are data (`ExportSettings`).

## Code reference: replay/components.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/components.rs`; each comment there links its section here.

### Replay component tracks

Per-actor component tracks: the world actors' audio state (traffic engines, ped feet and speech state, NPC skaters' audio records, velocities) recorded with the actors and written back during playback, so the world's sound follows the recording as retail's audio runs on the recorded actor state (b375, b378 channels A1..A3). Values hold until they change (stepwise, no blending). Any `Component + Clone + PartialEq` registered with [`track`] works the same, a mod-facing hook for later providers.

## Code reference: replay_edit/session.rs

Detail moved out of the code comments of `crates/skate-core/src/replay_edit/session.rs`; each comment there links its section here.

### Replay editor session

The replay editor's session: its mode and the key bindings that move between modes, as data with retail's defaults. Retail keeps the mode in the editor state (`[DAT_83067084]+0x30`); FE screen 12's input handler `826127B8` switches on it with FE key indices, and its per-frame update `82610F18` runs the held actions (research b386, b387, b389). Only transitions the research proved are in [`retail_bindings`]; the rest (camera key edit entry, modes 0xC / 0xD, the Finished popup's items) come with the screen host.

## Code reference: replay_menu_host.rs

Detail moved out of the code comments of `crates/skate-game/src/replay_menu_host.rs`; each comment there links its section here.

### Replay screen natives

The instant replay screen's natives (FE 12, template Replay2, `replay2/replay_editor2`; research b385 / b390 / b391): `FEReplayMenu.*` answered from a per-frame snapshot of the replay ([`ReplayScreenFeed`], published by `replay.rs`), and the screen states the C++ posts back (the facade's SetScreenState, applied by `retail_menu_stage` after the call returns), as the Trick Guide host does.

## Code reference: replay/messages.rs

Detail moved out of the code comments of `crates/skate-game/src/replay/messages.rs`; each comment there links its section here.

### Replay one-shot messages

One-shot messages recorded with their frame time and written again when the playback cursor crosses that time going forward: the world's one-shot sounds (horns, car alarms, tazers, ped body falls) come back in the replay, as retail's audio runs on the recorded state (b375, b378 channel A4). Speech is not recorded (retail blocks speech in replay). Any `Message + Clone` registered with [`track`] works the same.

## Code reference: replay.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/replay.rs`; each comment there links its section here.

### Editor keys

The editor's own keys (research b386, modes 6 / 2 / 0xB); true = the frame's input is used up. RB plays from the cursor (from the in point when at the out point), LB held trims with the sticks, the D-pad released drops / removes markers and jumps to the in / out points, A opens the effects menu (Up / Down row, Left / Right value, B back to the timeline).

## Help screen, Start and the panel while the retail screen loads (2026-10-22, USER-ASKS 256)

Research b500; verified our side (retail_menu_stage.rs Start
handling, replay.rs help block and panel gate).

- **Problem (laptop session 2026-10-22 10:25):** "Opening the Help menu allows you to do things behind it, for
  instance when hitting A for the next page it opens other things behind the help screen" and "When opening the
  replay, the old menu appears for a moment before the retail ui appears."
- **Cause:** one Start press opened our help AND pushed the pause menu (FE 54) underneath (the log has both in the same
  frame); with the pause menu up, the help's input block no longer ran, so A and the D-pad drove the hidden pause menu.
  The Bevy panel showed until the retail movie reported its intro complete.
- **Retail [code, b500]:** the help's handler 82616B40 takes A (next page) and B (close) only; input goes to the top
  screen only (825EC0E8); Start (825EB998) pushes no pause menu while a screen is on top.
- **Change:** `retail_menu_stage.rs`: Start pushes no pause menu while the replay screen is open. `replay.rs`: the help
  closes on B only; `RetailReplayUi` carries "the retail screen is loading" and the Bevy panel stays hidden then (up to
  3 s, then it shows as the fallback). Log lines `REPLAY_HELP open / page n / close` and `REPLAY_EDITOR mode a -> b
  channel x -> y`.
- **Editor camera ("I couldn't move the camera around and pressing Y appeared to do nothing"):** by retail's design the
  sticks move the camera only while editing a camera key (A on the timeline, a Follow / Tripod / Free row, A); the
  Game row changes the lens only; Y (Create pack) cycles the edit channel, and after Y moves off "camera", A opens that
  channel's rows instead of the camera rows (b386, b401, b402, b405, b411). Ours follows that. The new REPLAY_EDITOR
  line shows which mode a session reached.

## Teleporter Yes did nothing: the replay took every popup answer (2026-10-23, USER-ASKS 286)

- **Problem:** play session 2026-10-23 13:39 (University, Harley Stadium teleporter): "the prompt triggered and it
  asked me if I wanted to teleport in, yes or no. But when I chose Yes, nothing happened."
- **Evidence:** the log has `RETAIL_MENUS popup 0 (engine:teleporter)` and `popup 0 choice 1 (engine:teleporter)`
  (1 = Yes, `teleporters::chosen_exit`), then no `SKATE_TELEPORTER` line.
- **Root cause:** the replay's popup handler (`replay.rs`, since df85d911) emptied the whole engine answer queue
  (`std::mem::take(&mut popups.engine_choices)`) and kept only its own answers. When it ran before
  `teleporters::prompt` (the two systems are not ordered), the teleporter's answer was lost.
- **Change:** the replay takes only answers whose callback starts with `engine:replay.` and leaves the rest queued.
- **Test:** `replay::tests` (the quit warning test) queues a replay answer and a teleporter answer: the replay acts
  on its own, and the teleporter answer is still queued afterwards.
- **Files:** `crates/skate-game/src/replay.rs`, `crates/skate-game/src/replay/tests.rs`.

## Free camera look distance and the camera type conversions (2026-10-28, research b613, b652)
- Problem: our Free keys stored only eye, yaw and pitch. Converting Free to Follow or Tripod reset the look height to
  0, and converting Follow or Tripod to Free threw away the look distance.
- Retail [code]: the Free look point is P = eye - D * dir(yaw, pitch). D is the camera field C+0x24, set by
  `82C82A50` to |eye - look| when Free is picked (`82C80540`). The recomp stores `fsqrts` to +36, with yaw at +0x1E
  and pitch at +0x22 (verified). `82C82E40` rebuilds P with D unchanged on every edit, and the `82683680` leash
  write-back keeps D. A converted key keeps P (b613): Follow keeps it in the eye frame, re-applied at the skater root;
  Tripod keeps it skater-local.
- Change (`replay/camera_key.rs`):
  - `CameraKey::Free` gains `distance`, and `free_look` gives P.
  - Follow / Tripod -> Free takes the old look point's distance.
  - Free -> Follow / Tripod sets the look height from P's height (Follow: in the eye frame; Tripod: above the root).
  - Free keys blend the distance between keys.
- OURS: our look model has no lateral look offset, so P's lateral part is dropped; retail drops it too on the first
  edit tick (b655, "Look lift since the pick"). A Free key made from a Game view takes its distance to the skater root;
  retail uses the Game camera's look object 3 (actor slot 2, not identified).
- Test: `free_look_distance_follows_retail`. Research: research b652,
  research b613. Not play-checked.

## Look lift since the pick (2026-10-29, research b655)
- Retail [code, Ghidra research b655/a.txt`]: the Follow / Tripod edit update `82C80A78` writes 0.0 to the look
  elements 3 and 4 every frame and keeps element 5 (the look height). The D-pad lift `82C80EA0` adds the step to an
  accumulator at C+0x38 (reset at every pick), clamps it to +-1.0 (`0x8231A844` / `0x8216DEE0`), and moves the key's
  look height only while the accumulator stays inside the range (no partial step at a bound).
- Ours before: the key's ABSOLUTE look height was clamped to +-1, so a converted height above 1 m (Free -> Tripod
  from a high look point) was cut on the first edit.
- Change: `camera_key.rs` `lift_look` with `FreeHeight::lift` (the per-edit state, reset when another key or a pick
  starts editing). `CameraRules::look_range` / `look_step` stay mod data (`sdk.replay.set_camera`).
- Test: `look_lift_is_limited_since_the_pick_not_absolutely`. Not play-checked.
- Open: the first edit tick's record vs the converted key (a recomp hook on `82C84868`). The Tripod tilt question
  (b655 item 3) is answered in the next section.

## Tripod look height along the skater's tilt (2026-10-29, research b659)
Problem: picking Tripod from Free measured the look height as the world Y of the look point P above the skater root,
so a skater tilted at the pick (ramp, air) got a different height than retail.

Root cause (retail, Ghidra; research b655 section "b659"): the pick
(`82C80298` -> `82C856C8` target (0, live 0x58) -> `82C84CF8`) reads the skater through `82682638`, the full rotation
rows of the skater's actor entry (pitch and roll kept), so the stored height is along the skater's tilted up. Each
frame the aim (`82C8B9F8` -> `82C8C470`, look object 4) reads the skater through `826813C0`, which rebuilds the frame
with a look-at (`8296F0A8`), heading only: the height is applied along world up. Verified both accessors in
Ghidra (the look-at tail in `826813C0`, none in `82682638`); that the look-at's up is world up rests on b611's trace
of the same tail for the camera slot [inferred, medium-high].

Change: `replay/camera_key.rs` `convert_key` (Free -> Tripod) takes the height as the y of
`root.rotation.inverse() * (P - root)`; `Anchors::look` (world up) is unchanged; the key stays `{ eye, look_height }`.
Follow picks were already heading-only (b611).

Verification: test `tripod_pick_measures_height_along_the_tilted_skater`. Not play-checked. Open: a recomp exit
trace of `826813C0` on a ramp would prove the world-up frame; whether our recorded root carries the full tilt
(`replay.rs` root blend) is unverified.
