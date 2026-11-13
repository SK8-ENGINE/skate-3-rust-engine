# 21: Low camera: the retail "Camera Angle" setting (upstream issue #3)

Branch: `feature/low-camera` (from `main` 4488651). Upstream issue: SK8-ENGINE/skate-3-rust-engine#3, requested by
@HittaJP ("In the original Skate 3 there is a 'Low' camera angle setting ... It seems like the camera angle is the
'High' option from Skate 3 with no way to change it.").

## Problem

The engine always plays the High camera. `physics/camera_output.rs` passed a constant `camera_type: 1` to the stock
camera graph, so there was no way to get Skate 3's Low camera.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK), the converted disc data and the
game's string tables. Addresses are TU3.

- **The option.** Game Settings has a selector row `ID_GAMESETTINGS_CAMERA_ANGLE_TOGGLE`, shown as **"Camera Angle"**,
  with the values `ID_GAMESETTINGS_CAMERA_HIGH` / `ID_GAMESETTINGS_CAMERA_LOW` (**"High"** / **"Low"**). It is option
  index 11 of the Game Settings option table (`0x83026EC8`, name + widget type pairs; the camera row is a
  `selector`). In the English string table it sits with the control options ("Inverted / Normal", "Cameraman Mode",
  "Camera Angle", "High", "Low", "Control Settings").
- **First boot.** The boot flow asks for it: "Choose your camera:" (`ID_BOOTFLOW_CHOOSECAMERA`) with two preview movies,
  `data/movies/cameraLow_english_ntsc.vp6` and `cameraHigh_english_ntsc.vp6` (played by `sub_82609F30`, argument 0 =
  Low). So a retail profile has no silent default; the player picks one.
- **Where the value lives.** The camera preferences object `*(0x83085450)`, built by `82DF61F0`, holds the camera type
  at +20 (the constructor writes 0). The same object holds the look-inversion bytes +24/+25 and the air bytes +36..+38
  that our `CameraPreferences` already models.
- **What reads it.** Only the camera graph condition `IsCameraTypeActive` (registered by `82F93718`, factory
  `82DFBE18` stores the authored `type` at +28, evaluate `82DF4F70` returns `type == [prefs + 20]`).
- **What it changes.** The stock graph `data/script/camera/Default_cameragraph` (a setup asset):
  `CameraLow` = `IsCameraTypeActive type=0` **or** `IsInObserverMode` → `cameragraph_low.xml`; `CameraHigh` =
  `IsCameraTypeActive type=1` → `cameragraph_high.xml`. The two sub-graphs pick different stock shots from the
  `camera_shots` collection (also setup data): riding `bl_chase` vs `bl_high_chase`, grinding `bl_grind` (+ ledge left /
  right) vs `high_grind` (+ left / right), manual `bl_manual` vs `high_manual`, air `bl_air` vs `bl_high_air`, skitching
  `bl_skitching` vs `bl_high_skitching`; the low graph also has road, race, turning and security shots the high graph
  doesn't use. Wipeouts, drop-in, offboard, hippy jumps, plants and re-entry use the same shots in both. The rig
  settings (`camera` dynamics, trackers, positioner, compass) are shared; only the shots differ.

Representative shot values (`camera_shots`, inherited fields resolved; distance m, angles degrees):

| shot | camera | distance | elevation | framing pitch | board offset | lens |
|---|---|---|---|---|---|---|
| `chase_flat_slow` | Low (in `bl_chase`) | 1.8 | 7 | −4 | 0.9 | 12 |
| `chase_flat_fast` | Low | 2.5 | 5 | −2 | 0.9 | 12 |
| `chase_up_slow` | Low | 2.1 | −5 | −10 | 0.9 | 12 |
| `high_chase` | High (in `bl_high_chase`) | 3.0 | 30 | 3 | 1.0 | 12 |
| `high_chase2` | High | 4.4 | 26.6 | 5 | 1.0 | 12 |
| `grind_flat_slow` | Low | 1.5 | 7 | −10 | 0.6 | 12 |
| `high_grind` | High | 2.9 | 17 | 5 | 1.0 | 12 |
| `far_manual` / `close_manual` | Low | 2.35 / 1.1 | 13 / −5 | −2 / −8 | 0.9 / 0.5 | 12 |
| `high_manual` | High | 2.25 | 19 | 0 | 1.0 | 12 |

So Low is closer (about 1.5–2.5 m instead of 3–4.4 m), nearly level with the board (−5..+7° instead of 17–30°) and
looks slightly up; the field of view (lens 12) is the same.

### Measurement in the recomp

A short muted scripted run of the recomp (a local data folder, watch list `cam_watch.txt`:
`0x83085450 @ +20`) read the camera type once a second from boot through gameplay and the Career menu: **0 (Low)** in
every sample with the local save, which matches a profile set to Low. Driving the Career menu by script to flip the
option did not work (only the first menu input of a run registered, twice), so the write itself was not observed; no
static writer of +20 was found either (the value presumably arrives with the profile's settings block).

## Change

- `camera/angle.rs` (new): `CameraAngle` (`Low` = graph type 0, `High` = 1) and the resource `CameraAngleSettings`
  (the player's choice, saved in the installation's `settings/camera.json` next to `graphics.json`, as `{"angle": "low" | "high"}`; a mod's forced angle; mod shot
  tunings). `active()` is what the camera uses. `sync_runtime` (PreUpdate, after the pause menu) copies the active
  type and the tunings into `CameraRuntime` every frame, so a map reload, which rebuilds the runtime, keeps them.
- `physics/camera_output.rs`: `camera_type` comes from `CameraRuntime::camera_type()` instead of the constant 1.
- `camera/runtime.rs`: `camera_type` / `set_camera_type`, `has_shot`, `set_shot_tunings` (re-selects the current shot
  once so a tuned shot in use updates through the normal shot transition).
- `camera/shot_data.rs`: a tuning layer over the stock shots, applied when a shot is loaded (also inside blend trees).
- Pause menu (`graphics_menu.rs`): **SKATER > Camera angle High / Low** (Left / Right / A), saved at once. Switching
  live is safe: the graph re-evaluates its priority states on the next tick and blends to the other branch's shot.
- Default: **High**, the engine's behaviour so far. Retail has no silent default (the boot flow asks), so no shipped
  value exists to copy; the retail constructor's 0 (Low) is overwritten by the player's choice before play.

### Mod surface (Lua API 2, `sdk.capabilities.camera` = 4)

- `sdk.camera.angle()` → `{selected, active, owner, shot, tuned}` (also `sdk.snapshot.camera_angle`): the player's
  setting, what the graph uses, the mod forcing it, the current stock shot and the tuned shots with their owners.
- `sdk.camera.set_angle("low" | "high" | nil)`: force the angle; nil hands it back to the player. One mod at a time.
- `sdk.camera.tune_shot(shot, patch | nil)`: replace stock values of one shot by retail attribute name and unit:
  `PositionDistance`, `PositionElevation`, `PositionHeading`, `FramingLensLength`, `FramingRoll` / `Yaw` / `Pitch`,
  `ReferenceBoardOffset`, `SmoothingDirection` / `Elevation` / `Yaw` / `Pitch`, `TransitionTime`. Unknown names, out of
  range values and unknown shots fail the command. One mod per shot.
- Everything a mod set is released when it is disabled, fails, or the mod runtime resets (`clear_owner` in all four
  cleanup paths of `modding/mod.rs`). The player's saved setting is never changed by a mod.

Engine-facing: other systems read `Res<CameraAngleSettings>` (`active()`, `selected`, `forced_by()`) or
`CameraRuntime::camera_type()`.

## Files

- `crates/skate-game/src/camera/angle.rs` (new), `camera.rs`, `camera/runtime.rs`, `camera/shot_data.rs`
- `crates/skate-game/src/physics/camera_output.rs`, `physics/camera_angle_tests.rs` (new)
- `crates/skate-game/src/graphics_menu.rs`, `crates/skate-game/src/modding/mod.rs`
- `crates/skate-mods/src/presentation.rs` (`CameraAngle`, `CameraShotTuning`), `vm.rs` (commands, capability,
  snapshot default), `api.lua`; `sdk/skate.lua`, `sdk/ENGINE_API.md`

## Verification

Runs (2026-10-04, Windows, release):
- `cargo test --locked --release -p skate-mods --lib`: 59 passed (the `skyline_physics` integration test needs the
  Skyline GLB, which a fresh checkout doesn't have).
- `cargo test --locked --release -p skate-game --bin skate3rust`: 314 passed, 2 failed, both pre-existing
  (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`, `sky_shader_validates`).
- Asset-backed `-- --ignored camera`: 4 passed, including the new test (standing camera height: High 1.85 m, Low
  0.59 m, back to High 1.84 m after the high shot's slow elevation smoothing settles).
- In game (University, muted capture): `settings/camera.json` = low → log `Camera angle: Low`, no errors,
  `physics_failed:false`.

Tests:
- `camera::angle::tests` (graph type mapping, saved setting round trip and bad values, mod force / release / disable,
  tuning ownership and generation), `camera::shot_data::tests::tuning_replaces_only_named_fields_in_retail_units`,
  `graphics_menu::tests::sections_expose_only_real_rows_and_all_maps` (the new row).
- skate-mods: `presentation::tests::camera_shot_tuning_uses_retail_names_and_rejects_bad_values`,
  `vm::driving_extension_tests::camera_angle_api_crosses_the_lua_boundary` (real Lua → commands, rejected input).
- Asset-backed (`--ignored`, `SKATE3_ASSET_ROOT`): `camera_angle_switches_between_the_stock_low_and_high_graphs`: a
  standing skater through the real stock graph gets `bl_high_chase`, then `bl_chase` after switching to Low (camera
  lower), a mod tuning reload keeps `bl_chase`, and High returns to the same height.
- In game (@Hailey-Ross, 2026-10-04), riding with Low selected: "just tried out the low camera build and it looks
  great!" Default decided: "default should be high".

## Open questions

- The retail writer of `prefs + 20` and the value a fresh profile stores before the boot-flow choice were not found
  (static search and two scripted menu runs). The engine keeps High as its default (decided 2026-10-04: "default should
  be high").
- The first-boot "Choose your camera" screen (with the two preview movies) is not ported; the menu row covers the
  setting.
- `IsInObserverMode` also forces the low graph in retail; our condition maps it to the subject's special-effect flag
  (unchanged by this work).

## Camera slow motion: retail's subject gate (2026-10-23 / 24, USER-ASKS 294, 326)

- **Problem:** desktop session 2026-10-23 17:57 (hails-additions 28181a30), MegaPark full: the user reported significant lag
  while jumping the mega ramp, with the rendering slowing down mid jump, and, asked whether retail slows down there,
  that it does not happen in retail.
- **Evidence:** the air 17:03:27.9 to 17:03:31.5 (3.6 s): the game drew 210+ frames a second throughout, but the
  simulation ran 39 and 48 ticks a second (FIXED_TIMING, `FixedMain::run_fixed_main` x39 / x48): slow motion.
- **Cause:** the stock camera graph's `SlowMotionController` (settings `slowmotion_controller` / `default`,
  `fps_at_scale_one`) sends simulation-rate requests on a long air; `physics::frame::advance` applied them to the
  tick timer (`SimulationClock`, ported from `Simulation::OnRequestSetSimRate` 82857EC8). Upstream code since
  a47da5bb (2026-09-06). Retail does not slow down on the mega ramp, so a retail condition that keeps these requests
  out (or makes them inert) is missing from the port; not found yet.
- **First change (2026-10-23, replaced on 2026-10-24 below):** the requests were logged (`CAMERA_SLOW_MOTION begin` /
  `end` in `camera/graph.rs`, `CAMERA_SLOW_MOTION request` in `physics/frame.rs`) and did not change the tick rate.
- **Tests:** camera 38, physics 222 OK; the data-gated
  `stock_skater_startup_builds_the_graphs_and_physical_body_from_the_same_rig` now expects the normal period after a
  request (it then stops at tick 600 on "Canonical world has no authored query metadata", a test-world limit
  unrelated to the clock).
- **Recomp trace (2026-10-24, USER-ASKS 326; trace b584 run1, hooks `hooks_slowmo.cpp`, research
  b573 / b584):** a scripted Stadium - Full mega ramp air (camera height 36.6 m on the deck, 13.2 m at the bottom of
  the roll-in, 23.8 m at the top of the air, landing at 51 s; screenshot "Very High Gap, Air Height 13.0 ft"). The
  controller's Begin ran at the takeoff (47.98 s) and posted 1/60 (the curve at 0); 82857EC8 kept the simulation at
  1/60; Update ran through the air (65 logged calls) with its elapsed time staying 0 and posted nothing; End posted
  1/60 after the landing (50.55 s). So retail starts the controller on this air but never slows the simulation [trace].
  The gate that stops Update is its second one [code, b573 + the trace]: the subject's component at `+0x710` was null
  (the first gate, online, passed, as Begin posted). Our controller has no such gate and its Update follows the curve
  (`skate_core::camera::slow_motion`), hence 39 / 48 ticks.
- **The subject (research b586, trace b584 run2):** `+0x710` of an Actor (vtable 823004A8,
  ctor 82590DC0) is its PhysicalPlayerHiLOD, always allocated by the ctor, so a live Actor always passes [code]. The
  controller's subject is word 1 of the CameraGraph's context block, copied from the camera system's subject
  (`camsys+0xA0`) when the graph is built (82DF49B0, built once by 82DF6590) [code]. In the run that word was 0 in all
  60 logged Updates while `camsys+0xA0` held a live subject (`SMSUBJ` / `CAMSUBJ` lines) [trace]: the copy is empty,
  so Update returns before it advances, every time. (The recomp boots straight into gameplay; a console goes through
  the front end first. The user's report that it does not happen in retail agrees.)
- **Change (2026-10-24):** `CameraGraph::slow_motion_subject` (false, retail's empty copy) gates the controller's
  Update (`slow_motion_update`: no request and no time passes without it), and the frame applies the requests again
  (Begin / End only ask for the normal 1/60, so nothing changes in play; a slower one is logged as
  `CAMERA_SLOW_MOTION request`). Test `camera::graph::slow_motion_tests`; the startup test expects an injected 1/35
  request to apply again. Open: whether retail's copy is ever set (another boot path, replays); a mod switch for it.
