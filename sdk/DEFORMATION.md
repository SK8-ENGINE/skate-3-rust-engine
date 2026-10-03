# Generic impact deformation

Feature discovery: `sdk.capabilities.deformation == 1`. Manifest API stays 2.
This is an impact-driven plastic deformation approximation on top of rigid-body
physics. It is not a complete node/beam soft-body solver or fracture simulation.
The engine has no vehicle, panel, bumper or wheel rules.

```lua
sdk.physics.spawn("object", {
  body_type="dynamic", mass=100,
  shape={type="model", path="object.glb", object="shell"},
  deformation={
    yield_speed=2, compliance=0.045, radius=1.2,
    max_displacement=0.55, max_step=0.22, cooldown=0.10,
    resolution={9,5,17},
  },
})
sdk.graphics.mesh("visual", {
  path="object.glb", body="object", deform_nodes={"shell"},
})
```

`deformation` is optional on any box, convex or resolved model/compound body.
Sensors cannot deform. The mod supplies which scene node names (including their
descendants) follow the body field through `deform_nodes`. An empty list keeps
graphics rigid. Bind static GLB node transforms; animate other nodes separately.
Match the model-collider scale with the graphics instance scale.

| Parameter | Meaning / valid range |
| --- | --- |
| yield_speed | Normal contact impulse divided by this body's mass; m/s, 0.5–100. This is a gameplay yield criterion, not a measured material stress. |
| compliance | Metres of crush per m/s above yield, 0.001–0.2. |
| radius | Maximum lateral impact region in metres, 0.1–5. Actual obstacle surface queries determine which samples move. |
| max_displacement | Maximum cumulative offset from each lattice point's rest position, 0.01–2 m. |
| max_step | Maximum dent depth per accepted impact, 0.001–max_displacement m. |
| cooldown | Minimum interval between damage updates, 0.05–2 seconds. |
| resolution | Lattice counts on X/Y/Z, each 2–25, at most 2048 nodes total. |

Defaults are shown above. Recreating the body repairs it; there is no automatic
healing. Body pose updates alone retain damage. Mass, authored inertia, velocity,
joints, extra colliders and body identity are preserved during a dent. Extra
colliders remain rigid; only the primary shape deforms.

## Geometry and performance

The strongest qualifying contact pair is processed per body/substep. Its solved
normal impulses select the depth; rays against the other collider select the
obstacle footprint. Thus a narrow post and a broad wall produce different dents.
Queries use the actual map triangle mesh or the other object's solid collider,
including rotation. Physics-deformation fields use collider-local coordinates;
render bindings account for node hierarchy, graphics transform and collider offset.

At creation, large convex pieces are split into smaller convex regions, bounded
by the existing 64-part/8192-vertex limits. The preparation is cached for repeated
spawns. On impact, only changed convex pieces are rebuilt; there is no runtime
VHACD. The resulting shared shapes are used by Rapier and the native skater bridge.
Both geometry representations sample the same field, but convex pieces and grid
resolution still approximate small dents. This is not triangle-exact collision.

Visual vertices cache their eight lattice weights. GLB meshes become private to
the instance only when affected, retaining materials, UVs and shadow participation.
Changed primitives update normals/tangents and culling bounds. Unchanged damage
does no mesh work; unaffected primitives skip cloning and uploading. Damage work
is bounded native CPU work on the simulation/render update threads, not Lua
vertex loops, GPU readback, or an always-running soft-body simulation.

## Multiplayer

Only the owner computes damage. Body definitions carry the resulting solid
geometry and the lossless binary lattice offsets in the existing authenticated,
fragmented durable application state. Geometry/field share one revision; late
joiners receive the current state. Receivers retain and move their previous solid
until all new fragments are available. Damage-only revisions update its collider
in place. Remote objects do not independently accumulate damage.

Compact body-definition encoding is now SBD4. Every peer needs the rebuilt engine
and matching mod package. Existing envelope and packet budgets still apply;
oversized definitions are rejected, not silently simplified. Network latency can
delay remote damage until that revision is complete.

## Research basis and deliberate differences

- [BeamNG physics](https://www.beamng.com/game/about/physics/) and
  [flexbody documentation](https://docs.beamng.com/modding/vehicle/sections/flexbodies/):
  a lower-resolution deforming structure drives the visible mesh; permanent yield
  is distinct from elastic motion and breakage. We use a shared lattice and
  permanent displacement, without implementing BeamNG's spring/mass simulation.
- Victor Cepeda's Saints Row vehicle deformation article, *Game Developer*,
  [March 2012](https://media.gdcvault.com/GD_Mag_Archives/GDM_March_2012.pdf):
  simpler collision pieces can be linked to higher-detail visual damage. We warp
  convex pieces instead of using their removable collision-piece/morph system.
- [Rapier contact documentation](https://rapier.rs/docs/user_guides/templates_injected/advanced_collision_detection/):
  use solved impulses and manifold geometry. The pinned Rapier 0.35.3 uses
  body-local solver anchors; this implementation resolves them through
  `solver_contact_world_points`, rather than assuming a world-space `point` field.

## Validation tools

`cargo test -p skate-dynamics deformation` covers obstacle shape, rotated impacts,
resting-contact rejection, solid/mesh agreement, bounded damage, wire round-trip,
late join and in-place replica updates. `cargo test -p skate-game deformation`
checks mesh isolation, materials, rigid-node exclusion and rebinding.

`cargo run -p skate-mods --example probe_deformation -- mods/Skyline_Drive_Mod/skyline.glb`
runs actual-model wall/post collisions and prints CPU timings. The ignored
`actual_asset_deformation_cpu_cost` game test times the welded asset mesh path.
These are CPU probes, not an in-game FPS or live multiplayer measurement.
