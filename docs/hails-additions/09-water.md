# Water

Branch: `gameplay/water`. Status: **behaviour matched to retail footage (shallow water solid, deep water floats, board floats, water camera with vignette, entry splash, `water.alpha` look approved in play; small bodies calmer and waves slowed, awaiting check).**

## Problem

Water does nothing in play: the skater rides over it or falls through it as if it
were ordinary ground or empty space. The retail game has water-specific behaviour
(the `IsInWater` motion-graph condition, the water ragdoll profile, and the
"special surface" path in the wipeout state).

## Evidence so far

### 1. Water is a retail collision surface type, and it is in the converted maps

Retail collision units carry a 16-bit surface ID. The surface **type** is
`(surface >> 7) & 31`; the engine already treats type 12 as water:

- `crates/skate-core/src/physics/board_ground.rs`: the board sets collision flag
  bit 25 (`Body872`) and records the contact height (`Body864`,
  `surface_twelve_height`) when any part touches type 12.
- `crates/skate-game/src/physics/wipeout_states/prediction.rs`: the wipeout
  trajectory query tests `surface & 0xF80 == 0x600` (type 12).

New tool: `crates/skate-data/examples/water_surfaces.rs` lists the surface types
in each map's embedded RWCM collision and details the type-12 triangles.

```
cargo run --locked --release -p skate-data --example water_surfaces -- data/installations/<id>/maps/*.skate
```

Results (installation `c82bd63f…`):

| Map | Water triangles | Surface ID | Heights | Notes |
|---|---|---|---|---|
| DownTown | 355 (330 flat, all one-sided) | 1591 only | 0.4 m … 48.3 m, many levels | fountains/pools spread over 26 tiles |
| University | 322 (all flat, all one-sided) | 1591 only | 296 at 217.9 m, rest 67.9–71.0 m | reservoir plus smaller pools |
| Industrial, all parks | 0 | - | - | - |

So water collision exists only in DownTown and University. Every collision
stream is already converted (cities use their `cSim_*_high` tiles; parks use
`cSim_Global`), and the volume filter from change 5 cannot drop water, because
water triangles carry a surface ID.

Render materials agree: `water.alpha` / `water.flowingalpha` appear only in
DownTown and `water.default` / `water.flowing` only in University. Industrial's
harbour/sea is only the backdrop `ocean.reflection` mesh
(`assets/private/native-backdrops/Industrial.skate`), with no water collision
under it; the harbour bed is ordinary collision at about y = -3 (see change 4).

### 2. The skater never learns it is in water (the missing link)

The skater-side water signal comes from the collision output:

- `CollisionOutputFields.flag_3481` → `Processed.flags_2488` bit 30
  (`0x4000_0000`), and
- `CollisionOutputFields.scalar_28` → `Processed.collision_scalar_2924`
  (water height)

(`crates/skate-core/src/player/input_phase/publication.rs:253-254`). The wipeout
state reads both (`wipeout_states/lifecycle.rs:81-82`, `update.rs:24-26`) to
enter the "special surface" path (ragdoll profile 10, `below_surface`, the
`IsInWater` motion-graph condition).

**Nothing in the codebase ever writes `flag_3481` or `scalar_28`.** They stay
at their defaults (0), so the water path can never start, even on DownTown and
University where the data exists.

The board already computes the matching values (`collision_flags` bit 25 and
`surface_twelve_height`), and the board manager (`offboard/board_manager/runtime.rs`
`surface()`) and the player state (`player_state/publication.rs`) already read bit 25.
The likely retail behaviour is that the physics output fill copies the board's
`Body872` bit 25 → `Collision+3481` and `Body864` → `Collision+28`. **This
is a hypothesis; the retail write site has not been found yet.**

### 3. The skater's own contacts also classify water

`SkeletonCollision` (82BD4A30, `crates/skate-core/src/physics/skeleton_body/collision_update.rs`)
sets `flags.material_12` and `material_12_height` when any body part touches
type 12. Its flags sit at native 4079/4080/4081 (materials 10/11/12); the
collision output's 3479/3480/3481 use the same layout 600 bytes lower, and
3479/3480 are already the material-10/11 results. This makes the skater
body the most likely retail source of Collision+3481/+28.

### 4. Industrial's sea

The user remembers that falling into Industrial's sea respawned the player in
retail, and suggests the sea may simply be out of reach in retail. Industrial's
harbour floor uses ordinary surfaces (concrete and so on) and its sea has no
collision of any kind on the disc (the proxy archives are visual only), so no
data-driven water detection is possible there. Not handled by this change.

## Change

`PlayerInputRuntime::publish_water` (`crates/skate-game/src/physics/player_input/mod.rs`),
called from `frame.rs` right after `publish_board` (after the packet reset and
the skeleton contact feedback for the frame):

- skater body touching type 12 → `flag_3481 = 1`, `scalar_28 = material_12_height`;
- otherwise board touching type 12 (`collision_flags` bit 25) → `flag_3481 = 1`,
  `scalar_28 = surface_twelve_height`;
- otherwise both stay at their reset values (0).

Project choice: retail's writer is unconfirmed; the skater body wins over the
board because the wipeout water path simulates the body. Water triangles stay
solid one-sided collision, as before.

### Water bail (added after the first in-play test)

First in-play test (University fountain basin): the skater landed on the water
and rode on it; no bail. Expected: the published flag only feeds the wipeout
state, and nothing starts a bail on water. I searched every recovered bail check
(`skate-core/src/player/wipeout/`, the state selector, all motion-graph conditions
in the stock state XML) and found no water/type-12 trigger. The retail
trigger is in code that hasn't been recovered, and no decrypted executable is
available.

`physics/wipeout.rs` `check_after_physics` now requests a bail (request slot 33,
`WATER_BAIL_REASON`, otherwise unused and not a runout reason) when the board
(`collision_flags` bit 25) or the skater body (`flags.material_12`) touches
water, unless the skater is already in WipeoutGround, Teleporting or Sleeping.
It then takes the ordinary path: state flag 65 → `PhysicsWantsWipeOut` →
WipeoutGround, where `Collision+3481` starts the water ragdoll and the
respawn timer. Logged as `WATER_BAIL tick=… state=…`. **Project choice**, not
recovered retail behaviour.

### Water is not solid (skater sinks in and floats, board sinks)

User, from retail footage: the skater sinks into the water and floats; the board
sinks. One retail video frame shows the underwater-skating glitch (riding the
floor under the water surface), so the surface itself does not block anything.
The wipeout buoyancy code (`wipeout_state::body::special_surface`, pushing parts
up when below height + 0.1) agrees.

- `skate-core` `board_world.rs`: `is_water_tag` (type 12), `FLOAT_DEPTH = 0.5`.
  **Water is not solid:** `query_primitives` (the only path that makes physical
  contacts, for board and skeleton) skips water triangles, so bodies pass
  through the surface and rest on whatever lies under it. Ray, line and
  trajectory queries are untouched, so the wipeout's water trajectory check and
  respawn checks still see all water. `water_surface_at(point, above,
  max_depth)` returns the water height over a point (XZ barycentric on water
  triangles); `water_shallow_at` tells whether geometry lies within
  `FLOAT_DEPTH` under a surface point; `deep_water_surface_at` combines them.
  History: making shallow water solid (first per triangle, then per contact
  point) stopped the jitter but the body lay on top of the water like glass
  (user: "looks quite bad"); the jitter was the buoyancy, not the non-solid
  surface, so the surface is non-solid everywhere again and only buoyancy is
  limited to deep water.
- Water used to be detected from contact reports, which no longer exist for deep
  water. `skate-game` `physics/water.rs` restores the same signals from
  position: `mark_board` (after the board ground state is rebuilt) sets
  collision bit 25 and `surface_twelve_height`; `mark_skater` (end of skeleton
  feedback, before the postphysics wipeout checks) sets `material_12` and its
  height. Any water counts here, shallow or deep, so all water still bails. A
  body counts as in water from 0.05 m above the surface down to 4 m below.
- **Final rule, from retail footage (RPCS3, University channel):** in the 5–10 cm
  channel retail bails you and you **lie on top of the water** (body and board on
  the surface, water camera, respawn ~3.5–4 s, no splash); in the deep Aletown
  canal you sink in and float. So water is **solid where shallow** and non-solid
  where deep, decided per contact point: `query_primitives` drops a water contact
  only if `water_shallow_at(contact point)` finds no geometry within `FLOAT_DEPTH`
  (0.5 m) under it. On-foot support uses the same rule (`contact_toolkit` line hit
  point / nearby centre, `ground_query` lines via `Mesh.world`). Body buoyancy and
  the board's buoyancy/drag apply only over deep water (`deep_water_surface_at`),
  so nothing pushes a body resting on solid shallow water (no jitter). Traces:
  University channel, lowest body part 67.996 on a 67.94 surface, calm (0.06 m/s);
  Aletown, body floats (8.49–9.04 vs 8.93), board floats (8.91). The two
  intermediate versions below (shallow water solid per triangle; then all water
  non-solid with a "floor pass" for bodies in water) are superseded; the
  floor-pass version was removed.
- *(superseded)* **Shallow floors:** a body lying in University's 5–10 cm channel
  still looked like it lay on top of the water (user GIFs). While a body is in
  water, geometry within `FLOAT_DEPTH` (0.5 m) under a water surface no longer
  holds it: `BoardWorld::set_water_floor_pass`, set by `solve.rs` for the board
  query when a board part is in water and for the skeleton query during a water
  wipeout (`special_surface`). The body sinks past the channel bottom and floats
  like in deep water (trace: parts 67.43–68.05 against a 67.94 surface, settled
  bobbing 0.28 m/s, same as the reservoir); the board sinks to the plaza below
  (66.22). Ground deeper than 0.5 m still holds. Buoyancy and board drag apply
  in any water again (the deep-only masking below was an intermediate step).
- The wipeout buoyancy runs per part only where the water is deep
  (`special_surface_where`, `water::part_floats`). User report: in the shallow
  channel the body jittered; a part resting on the floor 5 cm under the surface
  was pushed up every step (the retail push includes a constant +0.1 term),
  lifted off, lost the push and its water drag at the line, fell back, repeat.
  Masked, the body lies in the channel on the concrete with its parts half
  under the water (part centres at the surface), calm (headless trace: mean
  part speed 0.05 m/s once settled). Deep water is unchanged: the body floats about 1 m
  deep with the top just above the surface (reservoir trace).
- `apply_board_drag` (just before the solve, after every state sets its drag):
  board parts over deep water get at least 0.1 linear/angular drag per step and
  no buoyancy, so the board sinks slowly (reservoir: ~1.5 m/s, settles on the
  bottom). Project-chosen value.
- DownTown's reachable deep water: the Aletown canal next to the Aletown
  spawn (user, from the PS3 version in RPCS3): open water at 8.93 m, 1.12 m deep,
  3.3 m below the quay, x -125..-205, z 436..490. Trace: body floats (parts
  8.50–9.02), board on the canal bed (7.90), camera 1.8 m above. Water triangles a
  global scan reports as 20 m deep elsewhere in DownTown lie under walkable ground
  (buried planes), not open water. University's reservoir is out of bounds in
  retail (user).
- **Retail reference (PS3 version in RPCS3, Aletown canal, user GIFs):** the
  board **floats** at the surface next to the skater (an earlier YouTube clip
  suggested it sinks; the emulator footage is the reference); the body floats
  spread-eagle just under semi-transparent water; a white mist plume plus droplet
  sprites, ~3-4 m across, lasts ~0.7 s, with a small puff where the board hits
  first; no ripple rings; the camera goes high above and behind, looking down
  ~60 degrees and drifting toward overhead, with a dark rounded vignette;
  respawn after ~1-3.5 s; Thrasher "Hall of Meat" points count while floating.
  Retail water close up: even steel blue-grey with fine dark streaks
  (~10-20 cm) that shimmer in place (PCA loop) with a slow drift.
- **Board floats** (`apply_board_drag`): besides water drag, board parts in
  water get buoyancy that cancels gravity with the part centre at the surface,
  rising linearly to 3 g at 5 cm below and zero 5 cm above. Trace (Aletown):
  deck settles at 8.91 against an 8.93 surface. Project-chosen values.
- **Ripple scale:** family 33 (`water.alpha`: DownTown fountains, Aletown canal)
  uses 4x the decoded normal-map scale. Compared with `--verify` captures (scratch
  camera mod aimed at the canal) against the RPCS3 frames: 1x gives ~1 m blobs,
  4x matches the streak size, 8x is too fine and noisy. Colour/contrast still
  differ (ours darker navy with bright reflection bands; retail an even steel
  blue). Project choice; the user asked for a loose match.
- **Water camera (final):** `camera/water.rs` `WaterView`, applied to the final
  frame in `CameraRuntime::advance`. Driven by the physics water state, not
  proximity: `physics/camera_output.rs` passes the wipeout's `surface_height`
  while in a water bail (`WipeoutGround` + `special_surface`). User report: the
  earlier proximity test (water surface within 0.3 m under the root) fired while
  standing on DownTown's fountain walkway, because water triangles reach under
  it. The shot: 3 m above the water, 1.75 m behind the skater (60 degrees down),
  drifting to 0.9 m (about 73 degrees) over 6 s, aimed 0.2 m above the root;
  blends in over 0.25 s and out over 0.5 s; direction fixed at entry. A camera
  under any water surface is still lifted 0.3 m above it.
- **Vignette:** the shot's weight (x0.85) goes through
  `retail_exposure` (`Settings.timing.z` -> meter compute -> `state.w`) to the
  tone pass (`retail_tone.wgsl`), which darkens the edges
  (`smoothstep(0.55, 1.3, |uv*2-1|)`), corners nearly black as in retail.
- **Splash:** `water_splash.rs` (`WaterSplashPlugin`). Setup extracts the
  game's particle sprites from the owned disc
  (`tools/asset_pipeline/particles.py`, environment group: `miscboot.big`
  `particletextures.rx2`, names from the `<name>.Texture` strings in texture
  order: steam, dust, pebble, grass, leaf, fluff, cameraflash, water) to
  `assets/private/particles/*.png`. On the frame the skater root or the board
  deck enters **deep** water moving down faster than 1 m/s, a plume of `water`
  sprites spawns at the surface: 13 x strength sprites (strength = speed/8,
  0.4-1.2; board x0.45), random roll and spin, rising 1.6-3.8 m/s, spreading
  0.4-1.6 m/s, drag and light gravity, growing 0.8-1.3 -> 1.8-2.8 m, alpha
  0.45-0.72, life 0.55-0.85 s; unlit alpha-blended `StandardMaterial` quads
  billboarded to the gameplay camera. No splash in shallow water (retail shows
  none). A first version with a generated soft blob plus droplet dots "looked
  like snow" (user); the retail sprite fixed that; the first sprite version was
  "a bit strong" and was reduced (18 -> 13 sprites, lower alpha/brightness).
  Without the sprite file there is no splash (logged).
- **Water colour (tried, rejected):** adding a steel-blue body colour under the
  lighting and toning reflections down (strengths 1-3) made the water milky and
  flat; the user judged it worse, so it was removed.
- **Water look (contrast, not colour):** measured water-only crops of the
  RPCS3 canal frame (`rpcs3_2lxnl5eGDi.gif` frame 0) against our capture from
  the same side of the canal. The mean colour already matched, which is why
  adding a body colour only washed it out. Contrast was the difference:

  | | mean sRGB | luma p5/p50/p95 | broad stdev (1/8 scale) | fine detail (vs 4 px blur) |
  |---|---|---|---|---|
  | retail | 71, 84, 95 | 56 / 84 / 93 | 6.3 | 5.3 |
  | before | 81, 89, 100 | 30 / 89 / 140 | 28.7 | 11.8 |
  | final | 75, 89, 100 | 74 / 86 / 105 | 7.1 | 5.2 |

  Rendering one shader term at a time showed that the broad bright and dark
  bands come from the environment-cube reflection. The lightmap, diffuse and
  alpha terms are even. Final changes, family 33 only (`retail_world.wgsl`,
  all project choices matched to the footage and approved by the user in play):
  - **Cube blur:** the cube is read 3 mips blurrier. Tested +2/+3/+3.5/+4/+6/+9:
    +3 matched the broad and fine statistics, and +4 or more was flat.
  - **Tint:** the reflection is multiplied by (0.82, 1.0, 1.04). Unmodified, it
    read grey (user: "not quite the same colour").
  - **Anti-tiling and swells:** the normal used one fine layer at the 4x ripple
    scale, which tiled visibly over large stretches (user screenshots of the
    DownTown fountain). The normal is now the fine PCA wave plus the same wave
    from a second sample (rotated 37 degrees, 0.613x scale, offset), plus 2x
    the large slow layer's deviation (broad swells). The user picked this
    variant ("less flat water looked really good").
  - **Smaller bodies move less (user request):** `water_bodies.rs` groups the
    triangles of family 33 materials that share vertex positions into bodies,
    because a map splits one body into chunks on a 100 m grid. Each material
    gets its body's area in `WorldParams.decal.y`. The shader scales
    `size = log2(area / 100) / log2(40)` (0 at 100 m², 1 at 4000 m² and up):
    fine waves x(0.6 + 0.4 size), swells x size². DownTown bodies: Aletown
    canal 4368 m² (5 chunks, full motion), DownTown fountain 1268 m²
    (size 0.69), others 99-4599 m². University's fountain channel is family
    30 and unchanged.
  - **Slower waves (user request: canal "a tiny bit too fast"):** the PCA wave
    animation (30 frames at 30 Hz, shared with the ocean) now has a second copy
    for family 33, `FrameStateData.pca_slow`. It runs at 0.75x
    (`WATER_PCA_RATE`), interpolated between frames (PCA weights combine
    linearly) and looping from the last frame into the first. The ocean keeps
    the retail rate. The frame state grew from 144 to 256 bytes.
  - **Tried and dropped:**
    - Dark streaks from the reflection dipping 0.05 above flat. They matched the
      canal statistics, but at steeper views (the fountain) they became
      repeating blotches.
    - A version keyed on normal tilt gave crescent shapes.
    - A horizon threshold on the reflection elevation never triggered.
  - Method: a temporary env hook in `FrameStateData.clock.w` picked the term or
    variant without a rebuild, then was removed. The statistics script crops
    water only, with no walls or walkway. Body areas came from a temporary
    log.
- **Captures without the user:** `SKATE_VERIFY_AT` (seconds, default 4) sets when
  `--verify` takes its screenshot. With a scratch mod that teleports the skater
  into the canal (or calls `sdk.camera.set`) this captured the splash, camera
  and vignette at chosen moments.
- Trace note: skeleton part 0 ("root") does not collide; judge penetration by
  parts 1..23 (an early reading of root height wrongly suggested tunnelling).

Maps without water have an empty water list, so nothing changes there
(`check_maps.py`: all 10 maps ok, collision triangle counts unchanged).

First in-play test of this (user GIF, walking into the DownTown fountain on
foot): the skater stood on the water briefly ("like cement"), then sank, and
the camera followed the body under the surface, showing only the underside of
the water. Two more changes:

- **On foot:** on-foot ground support does not come from contacts but from
  query scenes, which still treated water as floor. They now skip water too:
  `offboard/contact_toolkit/world.rs` (`line`, `nearby`) and
  `offboard/ground_query/lines.rs` (biped ground lines).
- **Camera:** `camera/water.rs` `WaterView`, applied to the final frame in
  `CameraRuntime::advance`. While the skater root is in water (from 0.3 m above
  water (any depth: a deep-only version let the low bail shot dip under the
  5 cm channel's surface, user GIF), the camera eases to at least 1.8 m above the surface, 3 m
  further back horizontally (first tuned 1.2 m / 1.5 m; user: still too close), and is re-aimed at the skater (blended by how far it
  moved). A camera under water for any other reason is lifted 0.3 m above the
  surface. It lifts fast (0.1 s) and settles back slowly (0.35 s). Values are
  project choices; the retail footage only shows the camera pulled back above
  the water.

Headless trace (`tests/water_drop.rs`, ignored diagnostic: drop into
University's fountain basin): the bail starts at the surface; the body sinks to
about 0.5 m, buoyancy brings it back to the surface, and it settles about 0.7 m
down, with the top of the body just above the water, until the respawn. The
camera ends at surface + 1.2 m (before the retune). At that spot the board rests on a solid concrete
ledge 5 cm below the water surface (`query_thin_line` under it: water 67.942,
concrete 67.888, floor 66.099), so it does not sink further; elsewhere it sinks
to the floor.

### Water rendering: extract the ocean animation table from default.xex

Water (family 33) and ocean (family 31) shaders need `assets/private/ocean-pca.json`,
which nothing in setup created (see open question 5), so 52 water/ocean materials
fell back to static shading (black for `ocean.default`, which has no diffuse).

- `crates/skate-data/src/xex/`: XEX2 unpacker (retail AES-128 file-key and CBC
  payload decryption, "normal" LZX or "basic" decompression) producing the
  mapped base image. No new dependencies; AES is checked against FIPS-197 and
  NIST SP 800-38A vectors. The owned disc `default.xex` (normal encryption,
  LZX) unpacks to an 18,022,400-byte image at 0x82000000 in about 0.2 s
  (sha256 `ce1e3ae5…`, not the TU3 image `ocean_pca.py` expects).
- `crates/skate-data/src/ocean_pca.rs`: locates cPCAWaterAnimationData's table
  the way the code addresses it (`lis` + D-form pairs forming two addresses
  0x168 apart within 32 instructions, data plausibility check), so it works for
  any build. The disc build has exactly one match: means at 0x82FC3978, weights
  at 0x82FC3AE0 (TU3: 0x830118D8 / 0x83011A40); frame 0 mean is
  (127.55, 251.83, 127.54), an "up" normal. JSON layout matches `ocean_pca.py`.
- `skate3rust --extract-ocean-pca <default.xex> <out.json>` (`main.rs`, runs
  before any game initialization; prints `OCEAN_PCA_READY`).
- Setup: `ocean_pca.convert` runs it in the `environment` group
  (`asset_exports.environment`, new `game_exe` argument; `versions.py` adds
  `ocean_pca.py` to the group). Failure is optional content, like other
  environment parts. Changing the environment recipe refreshes that group once.

### Water animation was frozen: per-frame shader state never reached the GPU

With the table in place the water still did not move (user). The shared
`FrameStateData` buffer (clock, PCA frame, shadow floor) was updated by
replacing the `ShaderStorageBuffer` asset's data every frame. Bevy 0.18's
`GpuShaderStorageBuffer::prepare_asset` then creates a new GPU buffer
(`create_buffer_with_data`), but each world material's bind group cloned the
first buffer at preparation and never sees the new one, so every world shader
read the initial state: clock 0, no PCA frame. Pre-existing upstream bug; it also
froze family 14 UV scrolling and the shadow floor colour.

`retail_render.rs`: the buffer is created once with `COPY_DST`; `FrameStateData`
is extracted to the render world (`ExtractResourcePlugin`) and
`write_frame_state` writes it in place with `RenderQueue::write_buffer` in
`RenderSystems::PrepareResources`. Confirmed in play: the water animates.

### Water time

The user found the water over-animated. The animation itself is time-based,
not frame-rate based, and the PCA frame rate matches retail (the disc build's
update routine advances one frame per 1/30 s, nearest frame, rows /255, which is
what we do). The same routine also keeps the water shader's time: +1/60 per call
(once per 30 Hz frame), restarting after 5. So retail water time runs at half
real-time speed and loops every 10 s. `retail_render::water_time` reproduces
that in `clock.z`, used only by the water path (families 30/33); family 14 keeps
real time (its retail time source is unconfirmed). Behaviour re-implemented,
not copied; the disassembly was reference only.

The 5-unit restart then showed as a hard reset every 10 s (user): each scroll
layer jumps by speed × 5 (0.05 tile for still water, 0.5 tile for flowing
water's 0.1 layer). The PCA animation is not involved (its 29→0 step equals a
normal frame step). Project choice: keep the half-speed rate but loop over
1000 units (≈33 min). Every authored scroll speed (render-parameters
`water[1]`: 0.01, 0.2, 0.1; ocean 0.4, 0.22) is a multiple of 0.001, so each layer
completes whole tiles per period and the restart is invisible; t stays small
for f32 precision. A unit test checks the rate and that every authored speed is
seamless.

### University water looks darker than DownTown's (data, not a bug)

`RENDER_AT` shows the materials at each test spot: University fountain
`water.flowing` (family 30), University reservoir `ocean.default` (31),
DownTown fountain `water.alpha` (33, transparent). University's flowing water
uses a pure black base texture and a reflection cube authored nearly black
(both the DXT1 256x1536 and the B5G6R5 32x192 copies decode to about 9,12,15),
so it shows only sun highlights; DownTown's has a dark-blue base and a bright
sky cube. Texture decoding was checked and is correct. The reservoir's
reflection is scaled by olm² × fresnel × 0.2 (retail tuning) and reads very dark.
No retail reference was found to compare; the user accepted the look for now.

### Industrial's sea rendered as a black void (global presentation model)

User (2026-10-08): "the ocean by the docks does NOT render correct and is just a
black void". Location from their 2026-10-07 13:58 session: Industrial, about
x -400..-560, z -70..-270, near y 0.

Root cause, two parts:

1. The sea is not in the district map. Retail's world record points each
   district at one extra global model (world fields `951898F6C0FA6856` model,
   `CA5A157A65E75934` textures) in `miscload.big`. For Industrial that is
   `data/content/world/models/DIST_Water.rx2`, 27 meshes [data]: mesh 7
   `ocean.default` (family 31, the sea surface: 204 vertices, x -8166..8005,
   z -4980..8529, y -7.7..-3.3, with normal, normal2, environment cube,
   lightmap and macro overlay), 13 `ocean.reflection` sheets (family 32, the
   harbour reflections, top at y -6.5, e.g. mesh 16 at x -827..-525,
   z -271..-77), distant shore and pier geometry (`environment.default` /
   `environmentsimple.*`, x down to -1131, z up to 1141, in no district
   stream), one reflective building and the tree wall. The district map has no
   water or ocean material at all (`water_surfaces` render scan).
   `tools/asset_pipeline/backdrop.py` exported this model with a shader
   whitelist (trees, then also `ocean.reflection` and
   `environment.reflective_simple`), so the sea surface and the far shore
   were never exported.
2. Nothing drew the package anyway. Upstream e2b85b64 (2026-09-12, renderer
   rewrite) deleted `retail_backdrop.rs` and its call in `map_render.rs`
   together with the prop package; the props were restored later (PR #15),
   the backdrop never was. No upstream commit, PR or issue mentions removing
   it on purpose. So since then the tree walls and DownTown's/University's far
   sea planes were missing too, and Industrial's harbour showed the clear
   colour.

Change:

- `backdrop.py`: `presentation_meshes` keeps every mesh of the global model
  (retail draws the whole model). Industrial's package grows from 15 to 27
  meshes; DownTown gains its three small presentation meshes; University is
  unchanged. Changing `backdrop.py` changes the environment recipe, so setup
  refreshes that group once.
- `retail_backdrop.rs` (new): loads `private/native-backdrops/<map>.skate`
  (render-only reader; rejects collision, lights, doors, rails), logs
  `SKATE_BACKDROP: <map> triangles=… materials=…`.
- `skate_world::spawn_backdrop`: the same retail material path and draw
  partitioning as the district (`spawn_static`), without lights or the
  `SKATE_RENDER_READY` line; every draw carries `Backdrop`.
- `map_render::PreparedScene::prepare`: retail districts load it after the
  district, before the props and the sky. Map retirement removes it with the
  map (`MapEntity`, staged assets).
- Moddability: `BackdropSettings { visible }` (retail true) is the one
  authority; mods use `sdk.world.set_tuning('backdrop', {visible = false})`
  (world tuning domain `backdrop`, `sdk.world.tuning(key, 'backdrop')` reads
  it), first writer wins, a stopped mod gives it back. The package itself is
  a content file. Multiplayer: presentation only, no gameplay state.

Verification: `--verify` captures with a scratch camera mod, camera at
(-520, 30, -90) looking at (-580, -7, -170) (player teleported to
(-470, 2, -150)). With `sdk.world.set_tuning('backdrop', {visible = false})`
(the state before this change) the harbour is a flat black area; with the
backdrop it shows the sea and the tree wall on the hills; user: "THERE WE GO IT
GOT THE OCEAN". The log shows `SKATE_BACKDROP: Industrial triangles=7768
materials=27` and no unsupported-family fallback for the backdrop materials.
DownTown (`triangles=860 materials=4`) shows no change or artifacts at the
Aletown spawn; University (`triangles=1587 materials=2`) now draws its far sea
plane (`environment.reflective_simple`, y -62), which reads pale blue-white
from the hills: no retail reference checked yet. Tests:
`retail_backdrop::tests` (visibility follows settings, missing package),
`modding::world_tuning::tests::backdrop_visible_by_default_set_and_reset_on_disable`,
skate-mods `world_tuning` patch validation, `test_backdrop.test_every_global_mesh_is_kept`,
and the asset-backed `industrial_backdrop_covers_the_docks_sea` (`--ignored`,
`SKATE3_ASSET_ROOT`): passes on the new export, fails on the old one.

### Floating trees in Industrial's south hills (far-proxy terrain)

User (2026-10-08, after the sea fix): "the water looks good in the rust engine,
i can see the floating tree's still in the distance".

Root cause [data]: the tree wall (`DIST_Water.rx2` mesh 14, 358 billboard
clusters at z -500..-986) stands on far-proxy terrain from
`data/content/proxyIndustrial_100_Proxy.big` (stream `Industrial_100_Proxy`:
284 `cPres_X_Z_high_proxy` cells, `proxyworld.default` hills plus their own
trees and environment meshes). Setup never read the proxy streams. 59 proxy
cells (x -2350..850, z -650..-950) have no full-detail district cell; the
other 225 overlap district cells (17.6 % of their vertices more than 0.5 m
above the full-detail surface), so drawing them all would poke through.

Retail rule [code, TU3]: the proxy world manager pairs each full-detail cell
with its proxy cell by name (`sub_8247EF40`, format `cPres_%d_%d_high_%s` at
0x82251A04 with `proxy`, map at manager +848, maintained by the streamer's cell
pass `sub_8247CE58` through `sub_8247EF40` / `sub_8247F060`). When the streamer
activates a full cell (`sub_8247BB50` -> `sub_82C985A8`) it posts activate
(0x4C5724D2) for the full cell and deactivate (0x4158EE18) for the paired proxy
cell; `sub_8247BD28` is the reverse path. So a proxy cell draws only while its
full-detail cell is not active. The engine keeps every district cell loaded,
so the retail result here is: only the unpaired proxy cells draw (Industrial
59, DownTown 0, University 0).

Change:

- `backdrop.py`: `proxy_drawn_files` (the rule above; unpaired files such as
  `cPres_Global_proxy` stay) and `convert_proxy`, which runs the district
  converter on the proxy stream (presentation only, its own Tex table) and
  writes the unpaired cells to `private/native-backdrops/<map>.proxy.skate`.
  The proxy Tex table lists its textures with bit 63 of the asset id set;
  `proxy_texture_keys` keys them by the id the materials use.
- `retail_backdrop.rs`: `load_proxy_package`, `ProxyTerrain` draw tag;
  `skate_world::spawn_proxy_terrain`; `map_render` loads it after the backdrop.
- Moddability: `BackdropSettings::proxy_terrain` (retail true) in the same
  `backdrop` world tuning domain: `sdk.world.set_tuning('backdrop',
  {proxy_terrain = false})`, first writer wins per field, reset when the mod
  stops. The package is a content file a mod can replace. Multiplayer:
  presentation only.

Verification: Industrial's package holds 59 `proxyworld.default` meshes
(477 triangles, 3.9 MB with only the textures they use; the unpaired cells
carry no trees of their own, those are in the global model); the log shows
`SKATE_BACKDROP: Industrial.proxy triangles=477 materials=59`. A muted
`--verify` capture from The Tanker (player (-440.6, 22.4, 36.5), eye
(-437.6, 24.6, 39.1), look (-455.6, 21.0, 23.3)) now shows green hills under
the tree wall where the trees floated against the sky before, as in the recomp
shot from the same spot; the sea is unchanged.

Tests: `test_backdrop.test_proxy_cell_draws_only_without_full_detail_partner`,
`test_proxy_texture_ids_drop_the_tex_table_flag`,
`test_proxy_cells_per_district_on_owned_disc` (`SKATE3_DISC`; Industrial 59,
DownTown 0, University 0), `retail_backdrop::tests::proxy_terrain_visibility_is_its_own_switch`,
`modding::world_tuning::tests::proxy_terrain_defaults_to_retail_set_and_reset_on_disable`,
skate-mods `world_tuning` patch validation.

Retail's in-water rule at the docks is not part of this change: the harbour
has no water collision (section 4) and the free-skate water/out-of-bounds
reset is skater state (`IsInWater` action-graph condition, not traced; see
`.claude/notes/triggers-volumes-re.md` section 4).

### Falling into the Industrial sea (retail air timeout)

Problem: riding off Industrial's dock edge drops the skater through the backdrop sea. Nothing said
why or when they came back, and the air limit could not be changed by a mod.

Retail at the sea [user's RPCS3 footage, `.local/research/in-water/rpcs3-*.jpg`]: jumping off the
docks or the tanker, the skater is reset about 1.4 to 3 s after leaving the edge, as they reach
the water, not after 5 s: bail camera (vignette, Hall of Meat counter), a hard cut, then the
skater fades in as a ghost at a safe spot next to where they jumped; no splash. That is a
different trigger from the air timeout below. Industrial has no water collision (surface type 12)
[code], so the trigger is not the type-12 water path either.

Mechanism found (sea reset barrier) [code, data]: an invisible collision floor at y -4.0 under the
whole Industrial sea, surface 768 = physics type 6 `physics_unrideable` (11,411 triangles,
x -3679..1839, z -492..1457), present in our converted `Industrial.skate`. Not a trigger volume,
kill height or timer. In retail the contact path `sub_82DB6EC0` -> `sub_82DB8120` (0x82DB7C58) ->
`sub_82DB80C8` sets PlayerState+69 (teleport request) for type 6 (types 9/12 set +65, bail); for a
ragdoll (category 300) SkeletonCollision+4068 is set on a type-6 touch (`sub_82BD4A30`), copied to
Collision+214 (`sub_82BD60C8`) and turned into +69 at 0x82DB81CC. +69 -> flags+2472 bit 18 ->
`CalcSuggestedState` state 702 -> checkpoint manager `sub_82592518` / `sub_825926F8`. The
checkpoint search (`sub_82BFB3F8`) rejects types 5/6/9/12/13. The same type 6 also covers
Industrial's quay walls (772) and roofs, and areas on other maps. Our engine had already ported
the whole chain (`skate-core` `player/input_phase/special_surface.rs`, the board/feet/plant
branches and the ragdoll +214 branch; `skate-game` `physics/skeleton_feedback.rs` publishes +214),
so the reset itself works; only the logged reason was wrong (`requested`). Full research:
`.claude/todo/ocean-docks-black-void.md`, section "Retail sea reset barrier".

Retail air timeout [code]: `PhysicalPlayerStateChanger::CalcSuggestedState` (`0x82D8ADE8`) counts
frames in physical category 200 (air) at selector+44; the count holds while flags+2468 bit 3
(animation packet flag 10375) is set and restarts on any other category. At `0x82D8B034`,
`count > 300` sets selector+57 (request teleport) and keeps the state; the next tick takes state
702 (Teleporting) and the checkpoint manager (`sub_82592518`, reply `sub_825926F8`) places the
skater at the best recorded safe checkpoint. 300 ticks at the fixed 1/60 s step = 5.0 s after
leaving the ground. It ends a fall only where no type-6 floor or other ground is below (the sea
floor above ends a fall into Industrial's sea long before). A skater who bails mid-fall (wipeout, category 300)
restarts the count and gets the wipeout auto reset instead (vault `physics_wipeout`:
TeleportMinTimeForAutoReset 3.5 s, TeleportMinTimeAfterSettlingForAutoReset 2.5 s,
TeleportMaxTime 20.0 s, TeleportAutoResetFadeOutTime 0.5 s [data]). Both were already ported;
this change adds diagnosis, a mod entry point and a respawn event. Behaviour at retail defaults
is unchanged.

Change:
- `skate-core` `player/selector`: the limit is a selector setting `AirTimeoutFrames`
  (default `RETAIL_AIR_TIMEOUT_FRAMES` = 300); the comparison stays `air_frames > limit`.
- `skate-game` `physics/respawn.rs`: the request sites record why a checkpoint teleport was asked
  for (selector request: `air_timeout`, wipeout output byte 69: `wipeout_auto_reset`, a type-6
  `physics_unrideable` contact: `boundary`, anything else: `requested`; `water` stays reserved for
  type-12 water, which no decoded path resets; the first request before the reply is kept).
  `publish_special_surface` returns a `BoundaryContact { state, packed_surface }` when its type-6
  or ragdoll +214 branch raises +69 (no behaviour change); `player_state/publication.rs` records
  it with `note_boundary`. Log lines:
  `AIR_TIMEOUT_RESPAWN air_frames=.. position=.. checkpoint=.. on_board=..` (position = deck at
  the request tick) and `BAIL_CHECKPOINT reason=.. position=..` for every checkpoint respawn; for
  `boundary` it adds `state=<selected state> surface=<packed surface, or none for the board and
  ragdoll branches> from=<skater root at the request>`.
- Multiplayer-ready event: `PlayerRespawn { player_id, tick, reason, checkpoint, heading,
  on_board, air_frames }` (serde, `reason` as snake_case), written by the owning simulation after
  its fixed 1/60 s physics tick (`emit_respawns`). Local player id 0. No networking.
- Moddable: world tuning domain `respawn`, field `air_timeout_ticks` (integer 1..216000, retail
  300) through `sdk.world.set_tuning('respawn', {air_timeout_ticks = n})`; validated at the serde
  boundary, first writer wins, removed when the mod stops. `RespawnSettings` is pushed into the
  live selector before each physics tick, so a map load keeps it. The wipeout auto-reset times
  were already vault data and stay there. Documented in `api.lua` and `sdk/skate.lua`.

Tests:
- `skate-core` selector: `air_timeout_requests_the_checkpoint_on_the_301st_air_frame_at_retail_default`
  (no request on air ticks 1..300, the bit 3 hold, request on 301, reset on ground) and
  `air_timeout_setting_moves_the_request_tick`; the existing teleport priority test is unchanged.
- `skate-mods` world tuning: `respawn` patch validation (0, 216001, fractions, negatives and
  unknown fields rejected).
- `skate-game` world tuning: `respawn_air_timeout_defaults_to_retail_set_and_reset_on_disable`.
- `skate-game` `physics/air_timeout_tests.rs` (asset-backed, `--ignored`): a skater moved 5 km off
  DownTown falls with no ground below and respawns with reason `air_timeout`, `air_frames` 301,
  301..320 ticks after leaving the ground; the event serialises with `"reason":"air_timeout"`.
- `skate-core` special surface: `type_six_contacts_report_a_boundary_request` (board type 6,
  ragdoll +214, feet packed 768 report the branch; types 0/9/12 and ragdoll water do not).
- `skate-game` `physics/respawn.rs`: `type_six_board_contact_resets_with_reason_boundary`,
  `type_six_ragdoll_contact_resets_with_reason_boundary` (core branch -> `note_boundary` ->
  checkpoint reply -> `PlayerRespawn` with `"reason":"boundary"`) and
  `earlier_request_before_the_reply_keeps_its_reason`.
- `skate-game` `physics/boundary_tests.rs` (asset-backed, `--ignored`, `SKATE3_MAP` = Industrial):
  finds an open-sea column whose top surface is the type-6 floor, drops the skater 3 m above it
  and expects the first respawn with reason `boundary` within 300 ticks. Run 2026-10-08: column x -3600 z -400,
  floor y -3.999, surface 768; respawn `boundary` 48 ticks after the drop, back on board.

Open items:
- The sea reset mechanism is decoded (type-6 floor, above; reason `boundary`). Still open: the
  ghost fade-in after the cut is missing in ours (leads: camera subject opacity / vault
  `FXSubjectOpacity` computed but not applied; `GetTeleportFadeInProgress`,
  `TeleportAutoResetFadeOutTime`), and the timing is not measured against the footage.
- The air timeout itself (5 s, camera during the fall and the respawn) is still to be confirmed in
  retail footage away from the sea; values stay from the code until then.

### Skater fades in after every placement (retail "ghost")

Problem: after a checkpoint respawn (for example from the Industrial sea) retail draws the skater
see-through for a moment, then solid ("ghost"). Ours drew them solid at once.

Root cause (retail, decoded from the TU3 code, located with one muted recomp run; reports
`.local/research/report-ghost-writer.md`, `report-ghost-fade-2.md`, `report-ghost-trace.md`):
- Every skater placement component keeps a fade-in timer in seconds (owner +1868). The
  place-skater handler `sub_825926F8` sets it to 0 and counts the placement (+1864); the
  component constructor `sub_82590DC0` starts it at 0, so a new skater fades in too.
- Each tick `sub_82594488(owner, dt)` publishes the opacity: timer < 1.0 -> clamp(timer, 0, 1),
  except that the timer stays put above 0.68 while state byte 71 is set; then timer += dt. A
  fade-out timer (+1872) starts at FLT_MAX (off).
- The opacity goes into the 208-byte per-skater render record (+190 = opacity * 255) and reaches
  the character shaders as `i_params.x`, which scales the output alpha.

Change:
- `skate_core::player::ghost` (`GhostFade`, `GhostSettings`): the timer and curve, generalised
  as opacity = clamp(timer / fade_in_seconds, 0, 1) (identical at the retail 1.0 s). Time-based,
  so the curve is the same at any tick rate.
- Every completed teleport of our skater ends in one reset (`physics/frame.rs`, the `teleported`
  branch): checkpoint respawns, wipeout auto resets, teleport menu / map / mod teleports and the
  session marker return. It now counts the placement (`respawn::Runtime::placements`, retail
  +1864). `skater_ghost::emit_placements` turns a new count, or a newly loaded skater (spawn, map
  change), into a serialisable `SkaterPlaced { player_id, placement }` message; `step_fades`
  restarts that player's fade and steps all fades on the fixed tick.
- Drawing: while the local player's opacity is below 1, every mesh under `PlayerRoot` (body,
  hair, clothes, board parts; `CharacterMaterial`, the customiser `SkaterMaterial` and plain
  `StandardMaterial`) draws a blended copy of its own material with the alpha scaled
  (`CharacterMaterial`: `tint.a`, which the shader already multiplies into its output alpha).
  At 1.0 the mesh gets its original material handle back and the copy is dropped, so solid
  frames draw exactly the materials they drew before. The retail character binding uses the
  original material when a mesh is bound mid fade.
- Moddable: world tuning domain `ghost` {`enabled`, `fade_in_seconds`, `hold_alpha`}, retail
  defaults true / 1.0 / 0.68, validated (0..60 s, 0..1), first writer wins, reset when the mod
  stops; documented in `api.lua` and `sdk/skate.lua`.
- Multiplayer-ready: fades are kept per stable player id and driven only by placement messages;
  render-only, no networking.

NOT RETAIL yet:
- The hold condition (state byte 71, opacity waits at 0.68) is undecoded; it is never set here.
- The render-side smoother `sub_8278C4D8` (15/s up, 5/s down toward the published opacity) is
  not ported; the 1 s ramp is slower than it, so the drawn value follows the published one.
- NPC skaters use the same retail component, but in our engine they do not go through the
  player placement path; their spawn fade-in is the living-world `NpcFade`. Remote players in
  multiplayer stay solid until placements are sent for them.
- Blended skaters do not write depth, so overlapping limbs can show through each other during
  the fade.
- Presentation differs, the code does not: in retail the timer starts at the placement, not at
  the teleport request (recomp trace: the ramp starts about 6.4 s after a Challenge Map teleport
  request, at the post-load placement), so after a menu teleport most of the 1.0 s runs behind
  the loading screen. The user's RPCS3 video (teleport to The Tanker,
  `.local/research/in-water/rpcs3-menu-teleport-tanker-10fps.jpg`) shows the skater see-through
  for only about 0.3 s after the loading screen clears. Ours also starts at the completed
  placement (and at the new skater after a map load), but our teleports have no loading screen,
  so the whole 1 s is visible in ours. Open item.

Tests: `skate_core::player::ghost::tests` (curve, frame-rate independence, restart, disable,
hold, validation), `skater_ghost::tests` (every placement restarts the fade; blended copy while
faded, original handle and untouched material when solid; message round trip),
`modding::world_tuning::tests::ghost_fade_defaults_to_retail_set_and_reset_on_disable`.

Files: `crates/skate-core/src/player/ghost.rs` (new), `crates/skate-game/src/skater_ghost.rs`
(new), `physics/frame.rs`, `physics/respawn.rs`, `physics/skater.rs`, `retail_character.rs`,
`retail_render.rs`, `main.rs`, `modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

### Auto-exposure meter reads retail's input (frame brightness)

Problem: at The Tanker our sky is about 1.46x darker and our sea about 2.2x darker
than RPCS3 and the recomp, while the lit deck is about 1.6x brighter. The code
comparison is in `.claude/todo/frame-brightness.md`. Sky, final copy, ocean maths
and the exposure evaluator all match retail. The meter input did not: ours
metered linear HDR Rec.709 luminance capped at 16 (described in the code as a
"portable" meter).

Retail (TU3):
- `sub_827F0D00` locks a surface (texture at +664 of the object at
  `this + 4 * (this[460] + 117)`, via `0x82A79048`) and passes width, height, pitch
  and bits to `sub_827F0B78`.
- `sub_827F0B78` walks every pixel of that surface (16 bytes, 4 pixels per step).
  Each big-endian u32 becomes `u32 / 2^24` (`vcuxwfp128 ..,24`), so its top byte,
  times 1/255 (vector at 0x8232F5D0). It multiplies that by the centre weight
  `(c - |x*y|)^2` and sums. `sub_827F0D00` multiplies the sum by 2.515
  (0x821A01E8), divides by w*h, and runs the evaluator.
- The top byte of an A8R8G8B8 word read big-endian (0xAARRGGBB) is alpha. The
  bloom downsample writes that alpha. `bloom_dof_tap4_minusthresholdPS`
  (shaders_final.big) squares 4 taps of the scene target (which holds
  sqrt(tm/2)), averages them, saturates each channel (`mul_sat`), then sets
  `oC0.w = dp3_sat(r1.zxy, (0.3, 0.3, 0.4))`, i.e. 0.3 R + 0.4 G + 0.3 B of
  sat(tm/2). `bloom_dof_tap4_minusthreshold_alphalumPS` averages 4 taps of all four
  channels (fetch swizzle xyzw) and saturates them.
- Which level is locked: not traced. The level does not change the mean, because
  every level is a box average of sat(tm/2), which is already in 0..1. It only
  changes the spatial sampling and adds one 8-bit rounding per level (at most
  0.5/255 each, small against the 0.25 target). The low bytes of the word add at
  most 1/255 of R to the reading. Confidence that the meter reads this alpha:
  medium-high. The surface format at +664 was not traced to its creation.

Change (`crates/skate-game/src/retail_exposure.wgsl`, `retail_exposure.rs`):
- Each meter sample is now the tone curve at the current exposure: tm/2 per
  channel, saturated, dotted with the meter weights, saturated, and rounded to 8
  bits (`meter_luminance`, mirrored in the shader). The centre weighting,
  2.515 scale, evaluator and 30 Hz time basis are unchanged.
- NOT RETAIL: the meter samples a 16x16 bilinear grid of our HDR target, not the
  console's 4-tap downsample chain.
- Moddable: the new world tuning domain `exposure` (`ExposureMeter`):
  `meter_weights = {r, g, b}` (retail {0.3, 0.4, 0.3}) and `meter_scale` (retail
  2.515), first writer wins, reset when the mod stops. It is readable via
  `sdk.world.tuning(key, 'exposure')`. `RETAIL_EXPOSURE_METER` is logged on every
  change.

Verification:
- Unit tests: `retail_exposure::tests` checks the meter against retail's formula
  written out from the shader (sqrt/square round trip, `r1.zxy` weights, 8-bit;
  within one 8-bit step), the knee (xe = 1 reads 0.5), that a bright sky reads
  far below the old HDR meter, and the retail defaults.
  `modding::world_tuning::tests::exposure_meter_defaults_to_retail_set_and_reset_on_disable`
  and the skate-mods patch validation cover the mod domain. The shader passes the
  WGSL validation test.
- Capture at The Tanker (muted `--verify`, scratch asset root with a fresh
  backdrop export): sky median 129,182,224, tone-inverted G xe 0.302 (before
  0.310); sea 90,113,132 (before 100,123,138); lit deck 157,149,142 (before
  152,145,140). RPCS3: sky 168,213,250 (xe 0.454), sea 125,175,210, deck
  128,115,102 (xe 0.108). The camera framing differs from the before shot.
- Result: the retail meter barely moves our exposure here (sky about 0.97x of
  before). Below xe 1, tm/2 is close to xe, and the blue-weighted 0.3/0.4/0.3
  roughly cancels the Rec.709 green weight on the sky. The meter only differs
  where pixels are bright (xe > 1). So the meter is not the main cause of the
  1.46x sky gap. The deck is the stronger lead: retail's lit deck is at xe 0.108
  while its sky is at 0.454. Ours is at 0.19 with the sky at 0.30, so our lit
  world surfaces are about 2.6x brighter relative to the sky. That raises our
  meter and pulls our exposure down. Next: world surface radiance (lightmap
  sample / squaring, sun term) at the deck. Not changed here.

### World shader families 7 / 8 lost their kd term (and 2 / 6 checked)

Problem: the world surface radiance check (`.claude/todo/frame-brightness.md`,
"World surface radiance vs retail") found two possible mismatches in
`retail_world.wgsl`: families 7 / 8 multiplied the lightmap by the flat-normal kd
0.93429 (about 7 % darker grass cards, foliage and simple diffuse surfaces), and
families 2 / 6 did not normalise the tangent normal before kd.

Which retail program each family is [data, VLT `skatercollections.vlt`, effect
field `effect`, class and key names checked with `tools/asset_pipeline/vlt.py`
`hash64`]: family 1 `environment.default` -> `baseenvironment`; family 2
`environmentsimple.default` -> `defaultenvironment`; family 6
`environment.reflective_simple` -> `baseenvironmentreflective_simple`; family 7
`environmentsimple.alphatest` -> `alphatestdefaultenvironment`; family 8
`environmentsimple.diffuse` -> `environmentdiffuse` (family numbers from
`tools/asset_pipeline/retail_material.py`).

Retail [code, `shaders_final.big`, `*_defaultPS.fpo` read with `xenos_disasm.py`]:
- `environmentdiffuse_defaultPS` 26-34 and `alphatestdefaultenvironment_defaultPS`
  27-35: sum of 4 lightmap taps times 0.25, squared, `min` with CSM visibility plus
  the (0.05, 0.09, 0.13) floor, times diffuse^2, times `c9.y` (m_params[0].y), then
  the fog `mad`. No kd term.
- `defaultenvironment_defaultPS`: kd is built from the raw `2n - 1` tangent normal
  (38), weighted by the signs of the tangent-frame sun times (0.62, 0.58) (56-61),
  plus 0.39 times the raw z (63, 64, 66), times 2.3956 (67). The `rsq` at 46-55
  normalise the view vector, the world normal used for specular, and the
  tangent-frame sun whose signs only are used. So kd is NOT normalised.
- `baseenvironmentreflective_simple_defaultPS`: the same, kd from the raw `2n - 1`
  (14-22, 45, 58-63, 74-76). Not normalised.
- `baseenvironment_defaultPS` (family 1) does normalise (29-33, kd from the
  normalised normal at 66).

Change (`crates/skate-game/src/retail_world.wgsl:322-325`): kd
starts at 1.0 for families 7 / 8 (`select(0.93429, 1.0, fam == 7u || fam == 8u)`),
so their radiance is `min(lightmap^2, vis + floor) * diffuse^2 * m_params[0].y`
as in retail. Families 2 / 6 already take kd from the raw normal (`fam != 2u &&
fam != 6u` guard on the normalise), which matches retail, so they are unchanged.
The research note's mismatch 2 was a misreading of which vector the `rsq`
normalises.

Verification: new tests in `crates/skate-game/src/retail_shader_tests.rs`,
`simple_diffuse_and_alphatest_families_have_no_kd_term` and
`tangent_normal_is_normalised_for_kd_only_where_retail_does`, pin each family's
kd to the retail program above; `world_shader_validates_under_non_uniform_material_slots`
validates the WGSL. `cargo test --locked -p skate-game --bin skate3rust -- shader_tests decals retail_shader world_tuning`: 29 passed, 0 failed (2026-10-08).

Open (found while reading, not changed):
- Our perturbed world normal for specular / reflection uses `max(raw.z, 0.05)`
  (`retail_world.wgsl:344`); retail
  `defaultenvironment_defaultPS` 40-44 and `baseenvironmentreflective_simple_defaultPS`
  18, 23-24 use the raw z. Check `baseenvironment` too before changing.
- `baseenvironmentreflective_simple_defaultPS` fetches no detail map, while ours
  lets family 6 use the detail normal when flag 128 is set. What flag 128 marks
  is not confirmed.
- m_params[0].y is still the constant `surface.w = 1` for world families (retail
  data 1.0 for every lit world material). Making it data-driven and mod-reachable
  is a separate job.

### Stains and wear decals drawn at retail strength

Problem: lit world surfaces measured about 2.6x too bright against the sky
compared with retail (`.claude/todo/frame-brightness.md`, "Deck input probe"). At
the Industrial Tanker deck (-440.6, 22.1, 36.5) the surface is material
`environment.decal` (our family 3): a white threadplate diffuse with the dark
`decal_wear_sp_id_concstains_02_d` stain decal on top.

Root cause: `stain_opacity()` in `retail_render.rs` (upstream commit 79ea829c,
"Reduce weathering decal strength") scaled the decal alpha by 0.35 for every decal
texture whose name contained grime, grunge, stain, oildirt, drainage or
ground_decals. Its own comment called it "an explicit visual tuning choice, not a
recovered native material constant". So the dark stains were drawn at 35 %, and
the deck came out about 2.7x brighter than retail where the stain covers it.

Retail [code, `shaders_final.big`, `decalenvironment_defaultPS.fpo` read with
`xenos_disasm.py`]: line 57 computes `art^2 - d`, line 59 multiplies it by `art.a`
and adds `d`, so `d = mix(d, art^2, art.a)` with no strength constant. The decal
texture's own alpha is the only weight.

Measured [data, offline probe on the dev install's `Industrial.skate`, 5x5 grid
around the deck]: sunlit lightmap raw 0.80 to 0.87, diffuse raw 0.26 to 0.64,
`lm^2 * kd * d^2` about 0.13 (0.08 with the grunge macro), which reproduces our
frame (about 0.086); retail implies about 0.043. With the stain at full strength
(stain alpha 1.0, art^2 about 0.07, d about 0.25) ours goes from 0.187 to 0.070,
the retail value. The lightmap UVs, the abs() on them and the diffuse decode
were checked and are not the cause.

Change:
- `crates/skate-game/src/retail_render.rs`: `stain_opacity()` removed; every
  material's `decal.x` is 1.0 (retail). New resource `DecalSettings` (retail
  `RETAIL_DECAL_OPACITY` 1.0), published every frame as `frame_state.clock.w`
  (`advance_frame_state`).
- `crates/skate-game/src/retail_world.wgsl`: the decal blend weight is
  `art.a * p.decal.x * frame_state.clock.w`, so the strength changes live with
  no material rebuild.
- Mods: new world tuning domain `decals` (`crates/skate-mods/src/world_tuning.rs`
  `DecalsPatch`, `crates/skate-game/src/modding/world_tuning.rs`):
  `sdk.world.set_tuning('decals', {opacity = 0.35})` brings back the lighter
  look; first writer wins; the mod's patch is dropped when it stops. Documented in
  `sdk/skate.lua` and `crates/skate-mods/src/api.lua`.

Verification: `retail_render` test `world_decals_default_to_retail_full_strength`,
`world_tuning` test `decals_default_to_retail_set_and_reset_on_disable` (default,
first writer, invalid values, reset on disable), and the WGSL validation tests in
`retail_shader_tests.rs`. Every map gets darker stains, so a regression check on
all maps and a matched-pose Tanker capture follow.

Open:
- Family 4 (tileable decals) shares this blend in our shader; the other retail
  decal programs (tileable, simple_tileable, environmentparkdecal) are not read yet.
- Per-texture decal strength for mods (only a global strength now).

## Verification

- Unit test `physics::player_input::tests::water_contact_prefers_the_skater_body_height`.
- `cargo test --locked -p skate-game --release --bin skate3rust -- water_contact`: passes.
- Release build staged into `bin\`; `--test-world --check-assets` → `SKATE_ASSETS_READY`.
- In play, first build: rode on top of the water, no bail (led to the water bail above).
- In play, second build: bails on University fountain basin (F2), University reservoir (F3) and DownTown fountain (F4). Log shows `WATER_BAIL ... state=KnownAir` on each drop.
- After the frame-state fix: University no longer falls back for any water or
  ocean material (52 -> 26; the rest are adverts, transparent environment
  pieces etc., logged by shader since this change); `RETAIL_OCEAN: loaded 30
  authored PCA frames`; water animates in play on all three spots (user).
- `tools.asset_pipeline.test_ocean_pca`: runs `convert` through the real
  `spawn()` (text-mode pipes) for success and failure.
- `retail_render` tests pass (water time unit test included) except
  `sky_shader_validates`, which was a known upstream failure (since fixed, doc 10).
- Asset-backed wipeout tests (`--ignored wipeout`): 4 pass; `marker_reply_restores_on_foot`
  fails identically with these changes stashed (pre-existing).

## Open questions

1. Where does retail write `Collision+3481` / `+28`? The wiring above is the
   best match to the evidence; it is not confirmed against retail.
2. Answered: water is not solid in retail (user footage). Implemented above; drag
   value and the 4 m depth limit are project choices.
3. Industrial's sea: retail respawned the player (user's memory), possibly
   out of reach in retail. No collision data exists for it; see section 4.
5. Water rendering (resolved, see above; kept for the record): static texture on the fountains, black on the
   University reservoir). Cause found: the log says "52 of 8546 world
   materials use an unsupported shader family and render as family 1". Water
   (family 33) and ocean (family 31) shaders require `assets/private/ocean-pca.json`
   (`MaterialTuning::supported`, `read_pca`). Nothing in setup (here or on
   upstream `60efdef`) creates that file; `tools/asset_pipeline/ocean_pca.py`
   needs a decrypted, decompressed TU3 executable image (hard-coded sha256 and
   table addresses 0x830118D8 / 0x83011A40). The disc `default.xex` is XEX2 with
   normal encryption and LZX compression, and setup never unpacks it. Fallback
   family 1 shows the static diffuse; `ocean.default` has no diffuse, hence black.
   This affects upstream users equally.
6. Splash: done (entry plume from the retail sprite). Retail shows no ripple
   rings. The Thrasher "Hall of Meat" counter seen while floating in retail is a
   separate scoring feature, not checked here.
7. `water.alpha` look: blurred and tinted cube, anti-tiled normal with swells,
   calmer small bodies, waves at 0.75x (see above). The user approved the
   canal look; the size calming and slower waves are waiting on their in-play
   check.
4. What is surface type 13 (DownTown 88, University 17,790 triangles)? It may be
   related (shallow water or a splash surface) or something unrelated, such as grass.

## Files

- `crates/skate-core/src/player/selector/mod.rs` (`AirTimeoutFrames`, `RETAIL_AIR_TIMEOUT_FRAMES`), `crates/skate-game/src/physics/respawn.rs` (`RespawnReason`, `PlayerRespawn`, `RespawnSettings`), `crates/skate-game/src/physics/air_timeout_tests.rs` (new), `crates/skate-game/src/physics/player_state/selection.rs`, `crates/skate-game/src/physics/player_state/wipeout_output.rs`, world tuning `respawn` domain (`crates/skate-mods/src/world_tuning.rs`, `crates/skate-game/src/modding/world_tuning.rs`).
- `crates/skate-data/examples/water_surfaces.rs` (new, diagnostic only: surface types, water heights, `WATER_POINTS`, `WATER_VIEW`, `RENDER_AT`, `MODEL_MATERIALS`, `TEXTURES`, `MATERIAL`; SKATE material ids are 1-based).
- `crates/skate-game/src/physics/player_input/mod.rs` (`publish_water`, unit test).
- `crates/skate-game/src/physics/frame.rs` (call site).
- `crates/skate-game/src/physics/wipeout.rs` (water bail request).
- `mods/water-test-teleport/` (dev-only test mod: F2/F3/F4/F7 into the water, Shift+key view spots, F5 position readout; the "Teleported" line clears after 5 s; committed on the fork for testing, dropped from the upstream branch).
- `crates/skate-core/src/physics/board_world.rs` (`is_water_tag`, water skip in `query_primitives`, `water_surface_at`) and `board_world/tests.rs` (`water_is_not_solid_but_reports_its_surface`).
- `crates/skate-game/src/physics/water.rs` (new), wired from `physics.rs` (`finish_skater`), `skeleton_feedback.rs` and `frame.rs`.
- `crates/skate-game/src/camera/water.rs` (new), `camera/runtime.rs` (`water` argument, `water_vignette`), `camera.rs`, `physics/camera_output.rs` (water bail state).
- `crates/skate-game/src/retail_exposure.rs`, `retail_exposure.wgsl` (retail meter input, `exposure` domain), `retail_tone.wgsl` (vignette).
- `crates/skate-game/src/water_splash.rs` (new), `main.rs`, `app.rs` (plugin).
- `tools/asset_pipeline/particles.py` (new), `test_particles.py`, `asset_exports.py`, `versions.py` (environment group).
- `crates/skate-game/src/verification.rs` (`SKATE_VERIFY_AT`).
- `crates/skate-game/src/physics/offboard/contact_toolkit/world.rs`, `offboard/ground_query/lines.rs` (water skipped for on-foot support).
- `crates/skate-game/src/tests/water_drop.rs` (ignored diagnostic), registered in `physics.rs`.
- `crates/skate-game/src/retail_render.rs` (frame state written in place, `water_time`, per-shader fallback log), `retail_world.wgsl` (water uses `clock.z`; family 33 ripple scale, cube blur and tint, anti-tiling and swells, size calming, `pca_slow`), `retail_material_bindings.wgsl` (`FrameState.pca_slow`), `retail_character.rs` (test initialiser).
- `crates/skate-game/src/water_bodies.rs` (new: water body areas, tests), `main.rs`.
- `tools/asset_pipeline/test_ocean_pca.py`.
- `crates/skate-data/src/xex/{mod,aes,lzx}.rs`, `crates/skate-data/src/ocean_pca.rs`, `crates/skate-data/src/lib.rs`.
- `crates/skate-data/examples/xex_unpack.rs` (diagnostic: unpack + locate table).
- `crates/skate-game/src/main.rs` (`--extract-ocean-pca`).
- `tools/asset_pipeline/ocean_pca.py` (`convert`), `asset_exports.py`, `install.py`, `versions.py`.
- Industrial sea / backdrop: `crates/skate-game/src/retail_backdrop.rs` (new, tests), `skate_world.rs` (`spawn_backdrop`, `spawn_static`), `map_render.rs`, `main.rs`, `app.rs`, `modding/world_tuning.rs` and `modding/engine_access.rs` (domain `backdrop`), `crates/skate-mods/src/world_tuning.rs` (`BackdropPatch`), `crates/skate-mods/src/api.lua`, `sdk/skate.lua`, `tools/asset_pipeline/backdrop.py` (`presentation_meshes`), `test_backdrop.py`, `crates/skate-data/examples/water_surfaces.rs` (reads render-only packages).
