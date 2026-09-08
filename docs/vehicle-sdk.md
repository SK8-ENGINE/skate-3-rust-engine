# Vehicle SDK v1

`skate-vehicles` pins Rapier 3D 0.35.3 and owns an independent, fixed-step rigid-body
world. The existing skating solver remains authoritative outside vehicles. Rapier
uses the installed map's collision triangles and resolves chassis collisions with
the map and other vehicles. Wheels use Rapier raycast suspension. Each host tick
is split into substeps of approximately 1/120 second or smaller.

## First mod: Mario Kart

Run PLAY-MARIO-KART.bat, enable **Mario Kart** in Mods, resume and press **F10**.
Walk/skate within four metres and press **E** to enter. **W/S** accelerate/reverse,
**A/D** steer, **Space** brakes, **left Shift** handbrakes, **R / right-stick click** rights/resets.
**E** exits once speed is below 3 m/s. F10 replaces a parked kart or resets an
occupied kart. Live engine, speed, brake, steering and grip controls appear in its
own draggable, resizable, scrollable mod window.

Controller: **Y** enter/exit, **RT/LT** accelerate/reverse, **left stick** steer,
**A** brake, **B** handbrake, **right-stick click** reset while driving. The reset bind can be changed to left-stick click in the mod settings. F10 remains the keyboard spawn shortcut.
Release controls before switching between driving and skating.

`sdk/examples/mario-kart/kart.glb` is generated from the user-supplied ZIP. It is excluded
from Git. The importer preserves meshes/materials/embedded textures, normalizes
the model to metres and creates named wheel pivots. To regenerate:

```powershell
python tools/prepare_mario_kart.py <kart-source.zip> sdk/examples/mario-kart
```

NumPy is required by the model importer. Editable sources live in `sdk/examples/`;
`Build.ps1` packages them into top-level `mods/*.zip` and stages a release.
The project launcher loads top-level mods/; a standalone executable defaults to its
adjacent mods directory. See [mod-packages.md](mod-packages.md) for the ZIP layout,
validation, update rules and development-folder support. No game assets are committed.

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
engine_volume (0..1), brake_impulse (0..10000), steering_angle (0.01..1.2 radians), tire_grip (0.1..20).
Maximum speed limits engine application; it is not an absolute downhill speed cap.

Events passed to `on_event`: `vehicle_spawned`, `vehicle_entering`, `vehicle_entered`,
`vehicle_exited`, `vehicle_exit_blocked`, `vehicle_reset`, `vehicle_removed`,
`vehicle_bailed`.
Each includes owner and key. Map changes use the existing `world_changed` event;
spawn new vehicles in the new world as needed. Invalid assets/commands appear as
mod errors; they retire the mod's vehicles instead of crashing the game.

## Vehicle definition

See `sdk/examples/mario-kart/vehicle.json` for a complete working definition. Coordinates
are metres, +Y up, +Z forward, +X driver-left, with heading in radians around +Y.
`half_extents` describes the outer chassis collision bounds. `mass` is kilograms. `model_scale`,
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

The local kart package supplies fitted clips. Other definitions default their animation slots to null. Without a matching
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
Driving slots loop; brake and reverse select their respective base clips, with drive
as fallback. Steering continuously blends that base towards the left/right pose.
Vehicle phase changes blend with native bone-local interpolation; brake/reverse
base-clip changes currently cut, so author compatible seated poses.
Edit vehicle.json or rider.json in the authoring folder, rebuild its ZIP, then reload
the mod (or wait for automatic rescan) to install new clips. Never edit .cache.

## Current boundaries

The native skating simulation is suspended while driving and resumes on exit.
Offline vehicle time pauses with Escape; online physics continues while menus are
open and unattended controls fall back to braking. Replay entry and session-marker
controls are blocked while driving. Vehicle motion is not recorded in skating replays.

Vehicles collide with the map, other cars and nearby native player/board proxies.
Native skating collision queries include car chassis. Matching enabled packages
replicate vehicles, tuning, wheel/rider poses, occupancy and engine audio. Crash bails
return to normal native ragdoll networking. See [Multiplayer mod SDK](multiplayer-mods.md)
for owner simulation, latency limits, hitbox approximations and late joins. Driving
another player's vehicle, passengers, weapons, damage and race rules are not built in.

Verification uses headless Rapier tests and window-free Lua tests. The game is
not launched automatically; rendering, entry/exit and handling need manual playtesting.

### Rider transitions and steering
The host blends vehicle phase changes over 0.4 seconds and the return to vanilla over
0.5 seconds, using the game's bone-local translation/scale lerp and quaternion slerp.
Steering input is smoothed and blends the current driving pose towards steer_left or
steer_right continuously, so these slots should contain compatible seated poses.
Vehicle camera hand-offs ease over 0.5 seconds. While driving, chassis, wheels, rider
and camera share the same interpolated fixed-step motion sample, avoiding relative
jitter from separate camera damping. Resets discard the old motion sample.
The board root is scaled away during vehicle playback and restored by the vanilla blend.
The local Mario Kart package supplies fitted entry/exit, seated and steering clips;
its fitted rider.json is included in source control with the prepared kart model.

## Ramp clearance and mass distribution
These are shared definition fields, available to any mod, not Mario-specific code:

| Field | Meaning | Default |
| --- | --- | --- |
| `collider_offset` | Chassis-local collision centre, metres | `[0,0,0]` |
| `collider_rounding` | Rounded edge radius; below 95% of smallest half extent | `0` |
| `chassis_friction` | Body contact friction, 0..2 | `0.3` |
| `center_of_mass` | Chassis-local mass centre, metres | `[0,0,0]` |
| `inertia_half_extents` | Box dimensions used for mass distribution, independent of contact shape | null: use half_extents |

Rounding stays inside half_extents. Shorten low front/rear overhangs so wheel contact
can lift the chassis before the body catches the ramp. Keep the collider large enough
to protect the cockpit; the visual model does not define collision. Inertia dimensions
let a shorter collision shape retain stable pitching/rolling behaviour. A lower centre
of mass helps resist nose-diving under braking. These fields require respawning.
The example passes headless 20/30/40-degree incline tests plus braking and steering tests;
this does not guarantee every map seam or vertical ledge is traversable. Wheels remain
raycasts, and vertical walls remain obstacles. No teleporting or artificial ramp boost is used.

## Engine sound
Opt in per definition:

```json
"engine_audio": { "enabled": true, "volume": 0.45, "idle_pitch": 0.7, "max_pitch": 2.8 }
```

The host synthesizes an original layered exhaust pulse with filtered noise. No downloaded
sound asset or extra file is required. It plays for occupied local and remote vehicles, with distance attenuation for remote engines; parked engines
are silent. Pitch follows speed and absolute throttle, including reverse and free revving
while stopped. Full throttle increases volume; releasing it smoothly drops the engine back
towards idle. Pitch/volume changes are smoothed, Escape/replay fades it silent, and exiting
or disabling the mod fades/removes the voice. This is a driver-focused mono sound, not a
spatial multi-car mixer or a simulated gearbox. There is currently no custom sample slot.

Volume is 0..1, idle_pitch 0.25..2, max_pitch idle_pitch..5 (multipliers of the 80 Hz
synth fundamental). All are validated. `sdk.vehicle.tune(key,{engine_volume=0.5})`
changes volume live; use zero to mute. Mario Kart exposes this in its mod settings.

## Authoring and reuse
See [Mixamo vehicle animation workflow](mixamo-vehicle-workflow.md) for the full sequence,
calibration, Blender fitting, native export and packaging commands. The host's animation
slots, steering blend, stance hand-offs, board hiding and shared motion interpolation
apply automatically to every vehicle definition. The example fitting scripts contain
kart geometry targets; adjust those targets for another vehicle without changing the host.

## Rider hitbox and crash ejection

Every vehicle can use the shared `rider_safety` definition. It is enabled by default;
explicit example values are shown below. Changes require respawning the vehicle.

```json
"rider_safety": {
  "enabled": true,
  "offset": [0, 0.5, 0],
  "radius": 0.25,
  "half_height": 0.25,
  "crash_delta_v": 6,
  "hit_impulse": 180,
  "inverted_up_y": -0.2,
  "inverted_seconds": 0.2,
  "eject_up_speed": 2
}
```

A solid capsule is attached to the occupied chassis, centred at `seat + offset`.
It covers the seated torso/head, including roof/ground/overhang contact when rolled.
It is not a sensor: contacts affect the vehicle. It adds no mass, so existing mass and
inertia tuning remains authoritative. Empty vehicles have no active rider collider.
The capsule is an approximation, not separate animated hand/foot hitboxes; adjust it
for your cockpit and expected character proportions. It is active through entry/exit
as well as driving, and is removed from collision when ownership ends. Disabling
rider_safety disables both this collider and automatic ejection.

Ejection triggers on any of:

- A chassis collision changing linear velocity by at least crash_delta_v in a physics
  substep. This is delta speed in m/s, not total driving speed; braking/ordinary ramps
  should not meet the default threshold.
- A rider-capsule contact impulse at least hit_impulse, in N·s. This is an impulse
  threshold, not force in newtons, and is evaluated on actual Rapier contacts.
- Chassis-local up having world Y below inverted_up_y continuously for inverted_seconds.
  The timer clears when upright again. Reset clears pending impacts/inversion history.

The host stops driver controls, releases occupancy and fades engine audio. It queues
an actor reset just above the seat, enters the native Wipeout ragdoll after normal reset
initialization, and seeds the native body and board velocities. Seat point velocity
includes the vehicle's angular motion. Pre-impact velocity is retained when the car
stops abruptly; if a hit accelerates the car from rest, the stronger post-impact point
velocity is used instead. A small world-up launch speed helps clear the seat. Output
linear/angular speeds are bounded to 60 m/s and 15 rad/s. This is a momentum hand-off,
not a fully coupled passenger rigid-body simulation or an exact seated ragdoll pose.
The crash visual hand-off lasts 0.12 seconds; normal entry/exit blends remain unchanged.
Native map collision and ordinary bail recovery take over. After release, native rider
collision against the Rapier car itself is still not bridged, as described above.

Lua receives **vehicle_bailed** instead of the normal vehicle_exited event:

```lua
on_event = function(event)
    if event.name == 'vehicle_bailed' then
        -- event.key is this mod's vehicle key; reason is crash/rider_impact/inverted.
        sdk.log('Ejected: ' .. event.reason)
        -- event.position: world seat position at detection, metres
        -- event.velocity: carried world velocity including launch lift, m/s
        -- event.angular_velocity: world angular velocity, rad/s
    end
end
```

The vehicle remains spawned and can be reset or entered again after bail recovery.
Use vehicle.read().occupied/phase for ownership; do not keep sending driving input
while unoccupied. Physics events are delivered through the existing mod callback queue.
The shared Rust simulation also exposes set_occupied and take_ejection for host tests;
Lua cannot directly write native ragdoll bodies or bypass the hand-off lifecycle.

Bounds: offset components ±5 m, radius 0.1..1 m, half_height 0.05..1 m (capsule cylinder
half-length; total height is 2*(half_height+radius)), crash_delta_v 1..50 m/s,
hit_impulse 10..10000 N·s, inverted_up_y -1..0.5, inverted_seconds 0.05..3 s,
eject_up_speed 0..10 m/s. Nonfinite values are rejected. Headless tests cover wall
momentum retention, overhang rider hits and sustained inversion versus empty vehicles.
Native rendering/recovery and unusual map geometry still need manual playtesting.
