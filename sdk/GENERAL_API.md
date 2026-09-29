# General engine API

Manifest API remains `2`. Feature discovery uses compiled `sdk.capabilities`:
`engine_access=1`, `command_results=1`, `native_bodies=1`,
`input_override=1`, `player_physics=2`, `player_overlap=1`, `landed_details=1`.
The host has no injury, vehicle or challenge rules. Those live in Lua.

## Reading systems

`sdk.engine.systems()` lists available system names. `sdk.engine.read(name)`
reads the latest snapshot, without executing a new simulation step:

| Name | Data | Write interface |
| --- | --- | --- |
| player | Mode, bail, stance, pose, velocity, scoring and transition counters | Existing player teleport, participation, attachment and gesture controls |
| rig | Native bodies, anatomical joints, solver contacts and joint loads | `sdk.rig`, typed `sdk.bodies` impulses |
| bodies | This mod's Rapier bodies and contacts | `sdk.physics` creation, geometry, forces, velocities, transforms, joints and motors |
| input | Raw pad, keys and 18 mapped action values | `sdk.input.override_action` |
| graphs | Live action/motion controller states, state times and active behavior IDs | `sdk.graphs.set_enabled` |
| animation | Tick, pose generation and bone count | No arbitrary animation replacement or pose injection |
| scoring | Current player scoring observations | No arbitrary native score/collector mutation |
| world | Current map observation | Existing mod geometry and volume APIs; no rewriting map assets |
| camera | Current camera observation | Existing follow, watch, mirror, rig, set, capture and release controls |
| network | Session identity, peers and mod state | Existing state publication, entity replication and authority-controlled operations |
| commands | This mod's most recent command receipts | `sdk.commands.request` |

The existing graphics, audio, UI, settings, assets and timer interfaces remain
available. Menus can create a named pause-menu section and nested pages using
`sdk.ui.menu(key, {section=..., title=..., items=...})`.

Catalogs are larger and requested explicitly:

```lua
sdk.engine.inspect("catalog", "graphs") -- or "scoring"
-- On a later callback:
local result = sdk.commands.result("catalog")
if result and result.ok then
    local action = result.value.action
end
```

Graph catalogs expose authored names, hierarchy, transitions, operations and
runtime behavior IDs. `states[].enabled`, `transitions[].enabled` and
`behaviors[].enabled` reflect runtime gates. `authored_enabled` and operation
metadata describe the loaded asset. Binding `states[].behaviors` uses operation
IDs; use `behaviors[].id` for behavior gates. Scoring catalogs expose loaded
definition IDs, labels, categories and authored points, not a substitute scoring
implementation.

## Commands and results

```lua
local token = sdk.commands.request("release_joint", {
    kind="player_joint", joint=joint.index,
    options={free_swing=true, free_twist=true, drive_enabled=false,
             possession_enabled=false, descendants=true}
})
-- A later snapshot, after the host executes the command:
local result = sdk.commands.result("release_joint")
if result then
    if result.ok then sdk.log("Applied") else sdk.log(result.error) end
end
```

A request wraps any validated native command table. The `kind` and payload are
the same as the public wrappers in `crates/skate-mods/src/api.lua`; the full
schema is `Command` in `crates/skate-mods/src/vm.rs`. Results contain `token`,
`ok`, `error`, `value`, and simulation `tick`. Reusing a key replaces its slot;
the Lua accessor ignores an older token. Maximum 64 result keys per mod.
Receipts remain readable until replacement or mod/world cleanup. Nested requests
are rejected. Invalid Lua/schema arguments still fail callback validation.

`ok` means the host accepted and applied that command. It does not mean a remote
peer acknowledged a network message or a multi-tick operation finished. Host
execution failures become receipts instead of disabling the mod. Commands are
not rollback transactions. Ordinary wrappers remain fire-and-forget, with their
existing error behavior. Skyline uses a receipt for chassis creation before
binding graphics, audio and driving state to that body.

## Native bodies and constraints

All native IDs are **zero-based**; Lua arrays are one-based. Discover IDs from
observations instead of assuming animation bone IDs equal physical part IDs.

Use `sdk.rig.read({"tick", "ragdoll", "contacts"})` to request only those
top-level fields. `sdk.rig.contacts()`, `parts()` and `joints()` also convert only
their requested section into Lua. Calling `sdk.rig.read()` without arguments
still returns the complete rig, including joint solver data.

```lua
local rig = sdk.rig.read()
local part = rig.parts[1]
local body = sdk.bodies.read({kind="skater", index=part.index})
sdk.bodies.impulse(body, {0,10,0}, body.position)
sdk.bodies.angular_impulse({kind="board", index=0}, {0,1,0})
sdk.bodies.impulse({kind="mod", key="crate"}, {0,10,0})
```

Native skater/board reads expose position, XYZW quaternion, world linear/angular
velocities, inverse mass/inertia and state flags. Skater parts also expose the
mapped bone name, joint index, effective collision/material/drive settings and
the direct part override. Impulse vectors use world coordinates: N s for linear,
N m s for angular; an optional world point adds the corresponding torque. Native
impulses are local-player only and rejected while attached or suspended. Commands
apply between simulation steps. Subsequent native controls and constraints still
participate. Bodies with no dynamic mass reject impulses.

`sdk.rig.configure_joint(index, options)` replaces a mod-owned override:

- `swing_limit`, `twist_limit`: radians in `[0.01, pi]`.
- `free_swing`, `free_twist`: release the corresponding angular limit.
- `enabled=false`: omit the entire joint constraint, including its linear anchor.
- `drive_enabled=false`: omit the child's animation bone drives.
- `possession_enabled=false`: omit board-holding drives involving the child.
- `descendants=true`: extend those **drive suppressions** down the physical
  joint hierarchy. Joint angle/enable changes still affect the specified joint.

Free angular limits retain linear attachment. This is what Broken Bones uses.
Native parameters and packed joint frames are readable but are not writable raw
memory. Limits apply to a temporary joint copy, so native defaults remain intact.
Drive-enable observations include ancestor and part suppression; `true` permits
native drives but does not create a drive that the current native state omits.

`sdk.rig.configure_part(index, options)` replaces a physical part override:
`motion="dynamic"|"frozen"|"static"` (native flags 4/2/1), `collision=bool`,
`friction=0..10` (both material coefficients), `animation_drives=bool`, and
`possession_drives=bool`. These affect the native solve. They do not replace the
whole player's movement state, mass preset, rendering or animation output.
Freezing a physical part does not freeze its animation target.

`reset_joint(index)` and `reset_part(index)` release owned overrides.
`sdk.rig.reset()` releases **all** this mod's joint and part overrides. Direct
joint/part slots, action slots and graph gates reject a different owner's write.
Omitted fields follow native behavior. Mod failure, disable/unload and world
changes restore owned controls. Released impulses are physical changes and are
not undone. Another mod's ancestor drive suppression can still affect a child.

## Contacts and joint loads

`sdk.rig.contacts()` reads the most recent completed native solve. `rig.tick`
is the cursor; do not count the same snapshot again in multiple callbacks.
Contacts include:

- `id`, `phase`: stable body-pair identity and `begin`, `stay`, `end`.
  Multiple manifold points share the pair lifecycle.
- `a`, `b`: board/skater body index, static world, external solid or remote actor.
  External solids include `solid_id` and, when locally known, mod `owner`/`key`.
  Remote actors include peer ID and board/skater part identity.
- World point and normal; normal/friction/total forces; impulse (`force * dt`);
  combined static/dynamic friction and material tags.
- `relative_velocity_before_solve`: A minus B velocity at the contact point,
  including angular velocity, sampled from the bodies before this solve.
  `closing_speed=max(0,-dot(relative_velocity,normal))`.

These are solver samples, not exact continuous time-of-impact measurements.
Resting support can have substantial force, so injury rules should also require
closing speed. `end` reports have zero force/impulse/velocity. At most 128 active
contact reports plus enough end reports to reach 256 are published per tick;
`contacts_truncated` signals omitted reports. Pair tracking still examines all
contacts; truncation does not create false contact ends.

Each joint's `load` is absent if no eligible row was solved. Otherwise it exposes
world linear/angular impulse and force/torque, plus the packed `solver_words`.
The host projects native accumulators (bytes 32/48) using axes at 192/240 and
divides displacement-producing corrections by dt for impulses, then dt again for
forces. Signs are for the child/A body. Angular values describe the direct
angular constraint couple, excluding the lever-arm torque from the linear anchor.
These are constraint reactions, not a bone-damage model.

Rapier `sdk.physics.read(key).player_overlapping` tests enabled native player
shapes against the body's actual collider shapes. Sensors use intersection;
solids include a 0.025 m contact tolerance. Suspended/attached players are excluded.
This differs from `sdk.volumes`, which checks observed player root positions.

## Input and graph control

`sdk.input.override_action(id, value)` overrides mapped gameplay actions 64–81
with a value in `[-1,1]`; nil releases the slot. It applies at the next input
publication and lasts until released or cleaned up. It leaves raw pad/key
observations intact and respects menu/debug input suppression.

`sdk.graphs.read("action"|"motion")` reads the live controller.
`sdk.graphs.set_enabled(graph, "state"|"transition"|"behavior", id, bool|nil)`
sets or restores an eligibility gate. It does not force a state jump, bypass
native conditions, inject callbacks, or guarantee an immediate exit from an
already-active state. Use `commands.request` with `kind="graph_gate"` when the
mod needs an explicit rejection result. Native graph execution still owns state
entry/exit and behavior lifecycle.

## Bundled examples and remaining boundaries

Broken Bones selects physical limb joints using observed names/topology and
requires both closing speed and impulse. It releases angular limits and limb
drives, keeps linear anchors, reports success only after a receipt, and provides
a Heal action. Injury state is published through generic mod network state; the
existing native pose stream carries the resulting physical pose. Peers must run
the mod to display injury summaries. It does not simulate fractures medically,
detach meshes, or add blood rendering.

Simon Says uses confirmed settlement metadata (`landed_trick_base`,
`landed_spin_degrees`, `landed_clean`, `landed_sketchy`) and explicit round
baselines. Wipeout uses persistent kinematic solids and actual collider overlaps.

The API now provides these concrete lower-level interfaces, not arbitrary access
to every Rust resource. Arbitrary animation blending/pose injection, scoring
collector replacement, raw graph operation replacement, native topology creation,
and direct mutation of another peer's native joints remain unexposed. New needs
should extend these engine interfaces rather than adding mod-specific functions.

Contact lifecycle refers to force-bearing native solver reports: an end means no positive normal response was reported for that pair. It is not proof that the collider shapes have geometrically separated.
