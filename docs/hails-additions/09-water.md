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

- `crates/skate-core/src/physics/board_ground.rs` — the board sets collision flag
  bit 25 (`Body872`) and records the contact height (`Body864`,
  `surface_twelve_height`) when any part touches type 12.
- `crates/skate-game/src/physics/wipeout_states/prediction.rs` — the wipeout
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
| Industrial, all parks | 0 | — | — | — |

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

- `crates/skate-data/examples/water_surfaces.rs` (new, diagnostic only: surface types, water heights, `WATER_POINTS`, `WATER_VIEW`, `RENDER_AT`, `MODEL_MATERIALS`, `TEXTURES`, `MATERIAL`; SKATE material ids are 1-based).
- `crates/skate-game/src/physics/player_input/mod.rs` (`publish_water`, unit test).
- `crates/skate-game/src/physics/frame.rs` (call site).
- `crates/skate-game/src/physics/wipeout.rs` (water bail request).
- `mods/water-test-teleport/` (dev-only test mod: F2/F3/F4/F7 into the water, Shift+key view spots, F5 position readout; the "Teleported" line clears after 5 s; committed on the fork for testing, dropped from the upstream branch).
- `crates/skate-core/src/physics/board_world.rs` (`is_water_tag`, water skip in `query_primitives`, `water_surface_at`) and `board_world/tests.rs` (`water_is_not_solid_but_reports_its_surface`).
- `crates/skate-game/src/physics/water.rs` (new), wired from `physics.rs` (`finish_skater`), `skeleton_feedback.rs` and `frame.rs`.
- `crates/skate-game/src/camera/water.rs` (new), `camera/runtime.rs` (`water` argument, `water_vignette`), `camera.rs`, `physics/camera_output.rs` (water bail state).
- `crates/skate-game/src/retail_exposure.rs`, `retail_exposure.wgsl`, `retail_tone.wgsl` (vignette).
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
