# Vehicle SDK v1

`skate-vehicles` pins Rapier 3D 0.35.3 and owns an independent, fixed-step rigid-body
world. The existing skating solver remains authoritative outside vehicles. Rapier
uses the installed map's collision triangles and resolves chassis collisions with
the map and other vehicles. Wheels use Rapier raycast suspension. Each host tick
is split into substeps of approximately 1/120 second or smaller.

## First mod: Mario Kart

Run PLAY-MARIO-KART.bat, enable **Mario Kart** in Mods, resume and press **F10**.
Walk/skate within four metres and press **E** to enter. **W/S** accelerate/reverse,
**A/D** steer, **Space** brakes, **left Shift** handbrakes, **R** rights/resets.
**E** exits once speed is below 3 m/s. F10 replaces a parked kart or resets an
occupied kart. Live engine, speed, brake, steering and grip controls appear in its
own draggable, resizable, scrollable mod window.

Controller: **Y** enter/exit, **RT/LT** accelerate/reverse, **left stick** steer,
**A** brake, **B** handbrake. F10 and R remain keyboard shortcuts in this example.
Release controls before switching between driving and skating.

`mods/mario-kart/kart.glb` is generated from the user-supplied ZIP. It is excluded
from Git. The importer preserves meshes/materials/embedded textures, normalizes
the model to metres and creates named wheel pivots. To regenerate:

```powershell
python tools/prepare_mario_kart.py C:/Users/Daddy/Downloads/kart-de-mario.zip mods/mario-kart
```

NumPy is required by the importer. Run Build-VehicleSDK.ps1 to build/stage the
executable and original mod sources. The helper preserves existing modified mod
files; copy updates manually when it reports a preserved file. The mods directory
sits beside the executable. No model or game assets are committed.

## Lua API

All keys belong to the calling mod. Commands cannot control another mod's vehicle.
The host allows eight vehicles per mod and 32 total. Definitions are validated
before allocation. Commands from failed Lua callbacks are discarded. Disabling,
removing or reloading a mod removes its vehicles and releases its driver.

```lua
sdk.vehicle.spawn('kart', 'vehicle.json', {x, y, z}, heading_radians)
sdk.vehicle.enter('kart')
sdk.vehicle.control('kart', {
    throttle = 1,       -- -1 reverse through +1 forward
    steering = 0.25,    -- -1 right through +1 left
    brake = 0,         -- 0..1
    handbrake = false,
})
sdk.vehicle.tune('kart', {engine_force=1800, max_speed=25, tire_grip=3})
local car = sdk.vehicle.read('kart')
sdk.vehicle.exit('kart')
sdk.vehicle.reset('kart', {x, y, z}, heading_radians)
sdk.vehicle.remove('kart')
```

`read` returns nil for an absent vehicle, otherwise position, quaternion rotation
(x/y/z/w), heading, signed speed in m/s, occupied, ready and phase. Phases are
`parked`, `entering`, `driving`, `exiting`. Snapshots update at host callback boundaries;
a spawn command is visible on a later callback. `input()` returns normalized
keyboard/controller throttle, steering, brake, handbrake, interact and pad_buttons.
Do not pass the full input table to `control`; copy only its four control fields.
`control` must be refreshed; after 0.25 seconds without a command, throttle clears
and a parking brake is applied. Controls are ignored during enter/exit animations.

`tune` supports optional engine_force (0..100000 N), max_speed (1..100 m/s),
brake_impulse (0..10000), steering_angle (0.01..1.2 radians), tire_grip (0.1..20).
Maximum speed limits engine application; it is not an absolute downhill speed cap.

Events passed to `on_event`: `vehicle_spawned`, `vehicle_entering`, `vehicle_entered`,
`vehicle_exited`, `vehicle_exit_blocked`, `vehicle_reset`, `vehicle_removed`.
Each includes owner and key. Map changes use the existing `world_changed` event;
spawn new vehicles in the new world as needed. Invalid assets/commands appear as
mod errors; they retire the mod's vehicles instead of crashing the game.

## Vehicle definition

See `mods/mario-kart/vehicle.json` for a complete working definition. Coordinates
are metres, +Y up, +Z forward, +X right, with heading in radians around +Y.
`half_extents` describes the chassis cuboid. `mass` is kilograms. `model_scale`,
`model_offset` and `model_yaw` affect the model only, not its collider.

Each wheel defines a chassis-local suspension mounting `position`, radius,
steering/driven flags and optional unique model node name. Support is 2..8 wheels
with at least one driven wheel. Wheel nodes should have local +Y steering and +X
axle axes. Pivots must be correctly positioned; the host adds suspension travel,
steering and spin to their rest transforms. The supplied kart importer does this.

Seat and exit are chassis-local offsets. Exit checks the nearby fixed ground and
standing clearance; if unavailable, it returns the rider to the saved entry spot.
The camera follows using camera_distance and camera_height. Model GLBs must embed
all textures and buffers; filesystem/network references inside GLBs are rejected.
A package may total 64 MiB; a model is limited to 32 MiB and an animation file to
16 MiB. Paths stay within the owning mod folder.

## Rider animation support

No animations are supplied. All animation slots default to null. Without a matching
clip, the skater is hidden and entering/exiting completes immediately. This avoids
showing an unrelated standing/skating pose on the kart.

Set `animations.file` to a package-relative JSON file and set slot names:

```json
"animations": {
  "file": "rider.json",
  "enter": "get_in",
  "exit": "get_out",
  "drive": "drive_loop",
  "idle": "seated_idle",
  "reverse": "look_back",
  "brake": "braking",
  "steer_left": "turn_left",
  "steer_right": "turn_right"
}
```

The file is:

```json
{
  "version": 1,
  "bone_names": ["EXACT_NAMES_FROM_sdk.animation.info().bone_names"],
  "clips": {
    "drive_loop": {
      "fps": 30,
      "frames": [["ONE_16_NUMBER_MATRIX_PER_BONE"]]
    }
  }
}
```

The strings in the example frames/bone list are explanatory placeholders.
Actual frames must contain one 16-number, column-major, native model-space matrix
per bone, in the exact native skeleton order. Matrices use metres and +Y up,
matching the existing authored animation convention. They are full poses, not
additive deltas. Obtain names from `sdk.animation.info()`; do not invent bone names.
The renderer performs the existing native-to-GLB skin-basis conversion. Author the
pose relative to the seat offset; enter/exit root motion is visual and does not
move the Rapier chassis. The native skater is restored through its teleport path
when exiting. Files with a mismatched skeleton, invalid matrices or missing named
clips are rejected. Maximum 32 clips, 3600 frames per clip, 1..120 fps.

Enter/exit clips play once and their frame count/fps determines transition duration.
Driving slots loop; brake, reverse and steering states select their respective clips,
with drive as fallback. Missing optional driving slots use the drive clip when
available. Slot switching currently cuts between clips; cross-fades are not included.
Edit vehicle.json or rider.json and reload the mod to install new clips.

## Current boundaries

The native skating simulation is suspended while driving, and normal skating
resumes on exit. Vehicle time pauses with Escape and during replay. Replay entry
and session-marker controls are blocked while driving. Vehicle motion is not
recorded in skating replays. Native skater-versus-vehicle impact forces are not
bridged yet: vehicles collide with the map and one another, but parked vehicles
are not native skating obstacles. This API does not include weapons, damage,
network synchronization, engine audio or a racing ruleset.

Verification uses headless Rapier tests and window-free Lua tests. The game is
not launched automatically; rendering, entry/exit and handling need manual playtesting.
