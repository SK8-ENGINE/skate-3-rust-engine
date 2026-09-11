# Physics + graphics modding layer (API 2)

Low-level Rapier + BoardWorld primitives. **No vehicle type** — assemble cars, carts,
and props from bodies, joints, meshes, and `sdk.net` rules in Lua.

## Primitives

| Area | Surface |
| --- | --- |
| Physics | `spawn/remove`, `add_collider`, forces/impulses/torques, pose/velocity, `read()` |
| Queries | `raycast` with `filter="all"` or `"ground"`, `velocity_at`, `effective_inv_mass`, `spring_ray`, `local_ang_accel_impulse` |
| Contacts | **`touching()`** (steady pairs incl. `ground`), `contacts()` (begin/end edges) |
| Joints | `revolute`, `prismatic`, `joint_motor`, `joint_spring`, `remove_joint` |
| Graphics | `sdk.graphics.mesh`, visibility, remove; `sdk.ui.text` |
| Player | `read`, `attach`, `detach` — attach pauses native skate for that player |
| Network | `sdk.net.publish/read` — JSON rules (driver seat, scores); **not** physics replication |

Manifest `"api": 2`.

## Fixed tick (owner client)

1. Map ground injected into `DynamicsWorld` (`ensure_ground`)
2. Per mod: physics snapshot from **last** Rapier step → `on_fixed_update`
3. Lua queues commands (forces, spawns, motors, …)
4. `begin_force_frame` → apply commands (removes before spawns) → skater proxies → `step`
5. Contacts drained; meshes/attach synced

`read()`, `touching()`, and `contacts()` during `on_fixed_update` reflect the **previous**
step. Commands you issue this tick take effect on the next step.

## Spawn height

Use the same raycast API with `filter="ground"` for terrain height. The default
`filter="all"` still hits dynamic bodies, mod bodies, and skater proxies.

```lua
local hit = sdk.physics.raycast(
  {x, 500, z},
  {0, -1, 0},
  {max_distance=1000, filter="ground"}
)
local ground_y = hit and hit.point[2] or 0
```

Exclude one or more owned bodies without changing the filter:

```lua
local hit = sdk.physics.raycast(origin, direction, {
  max_distance=100,
  filter="all",
  exclude={"chassis", "wheel_fl"},
})
```

Respawn pattern (multiplayer-safe): queue `remove` on frame *N*, `spawn` on frame *N+1*
so teardown is applied before placement queries.

## Contacts

```lua
for _, t in ipairs(sdk.physics.touching()) do
  if t.a == "wheel_rl" and t.b == "ground" then ... end
end
```

`contacts()` fires `started=true/false` for pair changes only; do not use it as a ground
counter.

## Drive assembly (reference pattern)

Built from primitives on the **owner** sim; remotes follow `dyn:{mod}:{body}` poses.

1. **Sensor chassis** — `sensor=true`, no terrain contact; carries mass/inertia
2. **Wheel bodies** — separate dynamic bodies with mesh/sphere hulls, `friction≈1.3`
3. **Prismatic suspension** — chassis↔wheel, axis down, `joint_spring` target = rest length along axis
4. **Revolute axle** — same anchors; rear `joint_motor` for drive
5. **No** scripted tire impulses on wheel contact points

Validate with `crates/skate-mods/tests/physics_api.rs` (`wheel_motor_on_ground_moves_chassis`).

## Example mod

`sdk/examples/physics-sandbox/` — crate, WASD forces, ground contact HUD, E attach, R reset.

```powershell
cargo run --locked -p skate-mods --example check_mod -- sdk/examples/physics-sandbox
cargo test --locked -p skate-mods physics_api
```

Copy the folder into `mods/` (or set `SKATE3_MODS`).

## Multiplayer

| Replicates | Does not replicate |
| --- | --- |
| Body poses (`dyn:…` APPLICATION) | Joints, motors, forces, spawns |
| Matching mod fingerprint | Per-tick command stream |

Use `sdk.net.publish` for shared rules (who drives, scores). Owner simulates physics;
late joiners see poses, not a full rigid-body replay.

## Dual-world collision

Skater BoardWorld volumes ↔ Rapier kinematic proxies each fixed tick. Dynamics shapes
export into `network_proxies` for native skate collision. Player hit / wipeout bridging
from Rapier contacts is still planned.

## Deferred

- `sdk.player.hit`, play-state masks, portal render targets
- Shapecast / multi-ray helpers
- Packaged ZIP helper (use loose folders + `check_mod`)
