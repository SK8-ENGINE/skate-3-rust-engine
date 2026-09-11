# Skyline DRIVE — Model Collision Repair 4.3.1

## No separate collision assets

The car now uses `skyline.glb` for BOTH graphics and collision. Its existing
`skyline_mesh` body node supplies the source triangles. The engine builds multiple
convex parts at runtime and keeps them in RAM, under one rigid body with the
existing authored mass, center of mass and inertia. No collision GLB, collision
JSON, authoring exporter or embedded collision-point list is required or written.

The separate `skyline_collision.glb`, `collision_geometry.json`, and
`COLLISION_ASSET.md` from 4.3.0 have been removed. Replace the previous mod folder
rather than merging into it to remove those obsolete files as well. Keep only
one enabled package with id `examples.skyline`.

This is not the old single hull pointed at a different file: `type="model"` uses
the real triangle geometry, full scene transforms and Parry convex decomposition.
The legacy `type="mesh"` API remains available for older mods but is NOT used here.
Model options in Lua are max_hulls=32, resolution=96 and concavity=0.0025. These
are approximation controls, not a promise of a millimetre-accurate surface. The
runtime-generated result has not been measured or playtested in this environment.
The first spawn can pause while cooking; unchanged respawns reuse an in-memory
cache. Cache entries are invalidated by changed model bytes or options. No disk
collision cache is made, so a new process cooks the first instance again.

The selected collision node is the rigid body mesh, not the separately animated
wheel nodes. Tires retain the existing raycast-suspension/reduced-order tire
physics, rather than being baked into the chassis or changed into rigid wheel
bodies. The input scene/model units and geometry are not rescaled or overwritten.

## Required engine update

Install the matching Model Collision Repair source update, then successfully
rebuild BOTH game targets. An old installed executable cannot acquire native
commands by replacing `api.lua`. This mod checks capabilities originating in the
native VM BEFORE sending `physics_debug`, scene-node or model-spawn commands.
The root SDK API remains version 2; named capabilities distinguish this extension.

Both multiplayer clients need the rebuilt engine and the same enabled mod files.
The new object protocol transmits the resolved compound geometry, not a path to
a required collision file. It does not transfer the visual GLB/audio assets or
execute another player's Lua. Different package bytes are deliberately rejected.

## Preserved features and controls

F10 spawns. E / controller Y enters or requests exit. R / Start resets.
H toggles driving diagnostics. C / controller X changes camera. J toggles the
actual solid colliders: green local, amber remote, cyan center of mass. The hull
view draws the generated convex parts, not an unrelated display/proxy model.

Independent wheel spin, front steering and suspension animation remain enabled.
Door entry radius remains 1.20 m. Safe exit requests outside, upright on-foot
placement; blocked exit keeps occupancy. The five-second re-entry cooldown starts
after confirmed exit. Camera, dashboard, audio, tire solver, engine, gearbox,
clutch and aero settings are retained. Original visual and audio assets have
not been changed. The existing hidden-driver presentation remains unchanged.

## What was and was not tested

41 assertions passed using the actual updated Lua API/mod through Lua 5.4 and a
test host. This includes rejecting partial/old native capabilities before queuing
new commands, selecting the same render/collision GLB, wheel animation, exits,
entry cooldown and debug commands. A 120-tick comparison produced the same
submitted force/torque histories as the prior driving baseline for identical
inputs. This is NOT proof of identical handling after collision geometry changes.

All 19 original GLB/audio/sound-bank assets are byte-identical. The render-body
input has 17,123 source positions (8,489 after exact position welding), 19
primitives and 16,532 nondegenerate triangles. Those are source-model counts,
NOT a measured cooked-hull output.

Rust compilation, the actual VHACD cook, Bevy rendering, native BoardWorld contact,
Windows execution and two-client networking were NOT run here. The source repair
includes Rust regression tests and `validate_model`, which runs the real cook,
Rapier spawn and compact replication round trip on your machine without producing
collision files. Use J and test roof standing/walking and vehicle impacts after
successful compilation; no unconditional no-penetration claim is made.
