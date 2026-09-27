# Authored props and grime follow-up

## Targeted decal correction after layer test

The user rechecked the comparison and identified decals as the unwanted layer.
Repeating grime is unchanged. Wear/stain decals now use 35% of their texture's
alpha; source UVs, dimensions, texture RGB and placement are retained. This is
explicit visual tuning based on the user's feedback, not proof of the original
game's intended opacity and not a recovered native shader constant.

Classification uses the material's authored `decal` texture label for grime,
grunge, stain, oil-dirt, drainage and ground-decal entries. Arrows, logos,
scratches and ramp edge paint retain full authored opacity. A regression test
covers this separation. This is not a blanket fade on every decal material.

Clamped decals also now receive a mip chain, independently of lightmaps, which
remain bilinear mip zero. Previously the shared clamp texture role omitted mips
for both, leaving fine decal alpha detail unfiltered at distance. The new decal
role retains clamp addressing while permitting mip filtering. No map re-export
or owned texture modification is needed. GPU visual verification remains for
the user; source and shader checks cannot confirm the preferred visual strength.

## Grime investigation after user retest

The UV-scale correction did not resolve the user's reported visual mismatch.
The current runtime log confirms the pictured Super Ultra Mega Park area is in
the University package. The separately named MegaPark district inspected earlier
contains different stadium geometry; it is not the correct visual test location.

The actual University's MPboards/MPCutStn materials contain both repeating macro
weathering and separate grime-puddle, long-grunge, water-stain and building-stain
decals. Binary material channel GUIDs agree with the exported bindings. Fifteen
sampled ramp/wood meshes in the current private University package match their
source cache positions, base UVs and independent decal UVs exactly after the
writer's V flip. This rules out a missing decal UV channel in those samples; it
does not establish visual parity or prove which layer causes the reported marks.

An opt-in `SKATE_WEATHERING_COMPARE=1` diagnostic enables F8 to cycle authored
layers, repeating grime off, decals off, and both off. The window title and log
identify the selected mode. The comparison launcher fixes exposure at 2.5 so
automatic brightness adaptation cannot counteract each layer change. Normal
launches retain their original rendering. Material uniforms change only on a
keypress; the original texture flags remain intact for restoration.

This is an isolation tool, not another claimed grime fix. The remaining
strength/scale mismatch needs a user-run layer comparison and ideally a matching
original-game view. No arbitrary opacity or UV scale change was applied.

## Hair rollback

Removed the hair-specific changes from 96adff5 at the user's request: coverage
texture bindings/blending, depth coverage, exported secondary UVs, FLOAT2 vendor
decoder correction and its test. The private character package was regenerated
with the previous exporter. Character probe smoothing remains. The other
worktree's hair implementation was not touched or imported.

## Grime coordinates

The map writer flips both texture rows and base UV V. The world shader formerly
sampled macro/detail maps at `flippedUV * scale`. For a scale of 0.3 this gives
`0.3 - 0.3*v`, but the matching texture-row coordinate is `1 - 0.3*v`: a 0.7
texture-period displacement. MegaPark's authored macro scale is approximately
0.3. This shifts stains onto different parts of the ramps and over decal paint.

Macro and detail UVs now scale in the original coordinate system, then flip V.
The authored opacity and native-reference overlay equation are unchanged:
`saturate((macro.rgb - 0.5) * opacity + 0.5)`, multiplied over the linear
diffuse/decal composite. Reference: scene.hlsl in native renderer commit
f6e0ae87fdfecbadb5c1e36c55d66a744187a3cd. No arbitrary global grime reduction
was introduced. Perceived strength still requires an in-game comparison after
the coordinate correction; adaptive exposure also changes overall brightness.

## Dynamic-object initial placement

Observed source: district Sim RX2 `0xEB001D` records have a 32-byte header,
128-byte records, row-vector affine matrix at +0, world bounds at +64,
instance/locator IDs at +96/+104, template ID at +112 and name offset at +124.
Native TU3 `0x825876D0` (community symbol SimManager::SpawnStatic) loads the
translation at +48 and template ID at +112 before the DMO model lookup.
Mapped image SHA256:
F4AA113EB541BFBA03DBC108CF5AB43F58C965B20FA3B82F9C40938A0AD841C4.

`worlddmo.big` supplies reusable templates. Their EB000D 160-byte tInstance
records bind template IDs to EB0001 model sections. Model mesh tables reference
EB0023 sections, which reference the vertex declarations. The exporter follows
those indices, never mesh order or material display names. Material texture
GUIDs come from the binary channel bindings: the DMO texture namespace's high
bit is absent from display-name suffixes, so suffix matching loses textures.

The exporter composes model, template and locator matrices in row-vector order;
normals use the inverse transpose. Reflections reverse triangle winding. It
neither snaps objects to terrain nor changes their authored matrices. Structured
JSON alongside each presentation package retains IDs, matrices, source hashes,
offsets and unresolved locators for a future dynamic runtime.

Offline restored placements:

| Map | Placed | Unresolved |
| --- | ---: | ---: |
| University | 479 | 0 |
| DownTown | 786 | 14 |
| Industrial | 162 | 18 |
| BlackBoxPark | 12 | 0 |
| MaloofMoneyCup | 31 | 0 |
| Remaining five maps | 0 | 0 |

All University placements resolve. 478 of 479 transformed render bounds agree
with their authored locator bounds within 5 cm (most within millimetres). One
open dumpster differs by about 14.5 cm; its authored matrix is retained, since
render bounds and assembly/animated bounds need not coincide.

The 32 unresolved Downtown/Industrial locators reference six template IDs absent
from the supplied DMO catalog. The named assets are also absent from parkassets
by those IDs, and district presentation template tables do not define them.
They remain explicit unresolved records; no substitute geometry is invented.

Runtime loads private/native-props presentation supplements alongside the
existing backdrop packages. This restores visible initial placement only:
objects are not yet pushable, droppable or collidable. Their current materials
use the existing PBR fallback, not the native dynamicobject lighting shader.
No static district collision, gameplay or animation logic was changed.

### Phase 0: live instances

The exporter no longer bakes every locator into world-space triangles.
Template geometry is written once per DMO template in model space, and each
placement becomes a MOBJ schema 4 record (schema 3 fields plus a 12-float
row-vector affine per object; schema 3 remains supported with an identity
basis and origin-only placement). Instance transforms are the composed
`model_matrix @ template matrix @ locator matrix`. The MOBJ name carries
`template_id/locator_name` so the runtime can group instances by template.
The JSON sidecar with full 64-bit IDs, matrices and source hashes is kept.

`parse_render_only` accepts and validates static MOBJ extensions (physics
flags are still rejected). At spawn, packages with MOBJ records take a new
`spawn_instances` path: one root entity per instance with a `PropInstance
{ id, template, name }` marker and its own `Transform`, mesh/material entities
as children. Geometry meshes and PBR/retail materials are built once per
(template range, material) and shared through cached handles — per-material
batching is intentionally broken per instance, but no mesh, texture or
material is duplicated. Packages without MOBJ records keep the old batched
path, so existing baked packages still load; re-export regenerates them in
the instance format.

### Phase 1: static prop collision

No authored DMO collision mesh is recovered from `worlddmo.big` (the district
sim RX2s carry clustered-mesh collision, but the template cache has none
verified), so each instance reuses its template's render triangles as its
collision volume, baked into world space with the instance transform at load
(`skate_world::build_prop_layer`). Reflections flip winding to keep
outward normals; degenerate render triangles are skipped. The authored locator
bounds are not needed: broadphase bounds come from the placed triangles.

Props stay out of the map `collision_world`. They live in a second
`BoardWorld` (`GamePhysics::prop_world`, built by the shared portable
triangle pipeline: 1 mm welding, reconstructed adjacency, contiguous-range
broadphase metadata) so Phase 2 can rebuild moved instances without touching
static map geometry.
The solve phase queries board and skeleton volumes against both worlds; wheel
line queries keep the nearer hit of the two worlds. Triangle tags carry the packed surface ID of the prop's render
material (same `audio | physics<<7 | pattern<<12` mapping as static
collision), so wheel surface classification keeps working.

Deck probes, camera, grind, offboard and climbing queries still see only the
static map world. Prop contacts report `CollisionBody::StaticWorld`; nothing
moves or pushes back yet. The props package is parsed twice at load (render
spawn and collision layer), matching its presentation-supplement status: a
missing or invalid package leaves props uncollidable instead of failing the
map.

Validation: unit test with a slab template and two instances (translated and
rotated) covering line queries, rotation of the footprint, packed surface
tags, and a wheel-sphere contact manifold. In-game verification remains.

Validation: synthetic record, rotation/scale/normal and invalid-reference tests;
original-data template resolution and placement-bound comparison; offline map
readers and shader composition. Game/recomp was not launched. GPU execution,
appearance, performance and gameplay interaction are not validated here.

### Phase 2: dynamic bodies

Every prop instance gets a dynamic rigid body (`physics::prop_dynamics`),
built on the recovered TU3 integrator (`integrate_body_rates`): gravity and
cool-down/sleep come from the retail simulation step, and mass properties use
the retail rounded-box finalize path (`primitive_mass_properties`) with the
instance's scaled template AABB. Density defaults to 100 kg/m³, the MOBJ
schema 3 authored default; damping is the authored 0.05 linear / 0.15 angular.
All props are dynamic by default — the exported packages carry no physics
flag, and enabling that flag was rejected.

Bodies start asleep at their authored pose, so a resting district costs one
AABB test per skater volume per tick. The narrowphase reuses the recovered GP
pair query (`primitive_pair_contacts`): box vs static-world triangles, box vs
box for other props, and box vs the skater's board/skeleton volumes for
pushes. Contact response is a compact custom impulse pass producing
`RetailReactionCorrections`; the retail compiled-row contact solver
(`build_contact_jacobian`) documents itself as not gameplay-ready, so it is
not used. Restitution only applies above a 1 m/s closing speed so resting
contacts settle instead of jittering.

A skater volume overlapping a prop transfers a fraction (0.5) of the closing
speed as an impulse at the contact point and wakes the body; the skater's own
response still comes from the exact re-baked triangle layer. Awake props treat
asleep props (and the static world) as immovable; a hard hit (closing speed
above 1 m/s) wakes the supporting prop. Prop-vs-prop impulses are split by
inverse mass.

After each awake body's integration, its instance's triangle range is re-baked
in the prop collision layer (`PropCollisionLayer::rebake` +
`BoardWorld::replace_triangles`): adjacency flags and edge cosines are
invariant under rigid motion, so only vertex positions are recomputed and the
broadphase bounds/query index rebuilt. `sync_prop_transforms` then publishes
each body's template-origin pose to the spawned Bevy entity (rotation and
translation only; authored scale stays on the entity).

Phase 2a limitations: box-approximated bodies mean stacking is approximate;
a skater standing on an asleep prop does not wake it (no weight transfer);
deck probes, camera, grind, offboard and climbing queries still see only the
static map world plus the re-baked prop triangles. Phase 3 (grabbing) needs
identification of the prop by `PropInstance.id` (`PropDynamics::by_id`), a
constraint or kinematic-follow toward the hand, wake on grab and re-sleep on
drop.

Props use their own simulation step (`prop_simulation`): the board's step has
`cool_down = 0`, which would put props to sleep instantly, and the retail
freezing-energy threshold is tuned for the board's mass, so props use
`cool_down = 30` and `minimum_energy = 0.5`. Resting bodies snap their
velocities to zero below the energy threshold — gated on actually having
contacts, otherwise the snap zeroes the first ticks of a fall (g·dt is far
below the threshold) and the prop descends at g·dt² per tick forever — and
count those snapped ticks toward cool-down directly, because the integrator's
own counter compares against the previous energy, which the snap just zeroed.
The reaction-vector convention is per-tick displacement
(`linear = (force·dt + v)·dt + reactions[0]`, stored velocity rescaled by
`frequency − drag`), so impulse deltas enter `linear_displacement` as Δv·dt.

Validation: unit tests cover fall + settle + sleep with re-baked triangle
queries, a skater sphere pushing a resting prop awake, and long-idle
stability (no sinking, no divergence); the full `skate-game` suite is green.
In-game verification remains.
