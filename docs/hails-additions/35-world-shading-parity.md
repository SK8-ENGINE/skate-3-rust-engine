# World shading parity: the retail 4-tap lightmap fetch and the fence render state

Branch `fix/fence-alpha` (off `main`). Follow-up to [32](32-transparent-fences.md) (upstream #58, merged
2026-10-09).

**Scope grew during the work.** This started as the remaining retail-parity items for the see-through fences
(doc 32 open questions: texture fetch swizzles, specular mask, render state, which is ported here too). Reading the fence program showed a
lightmap fetch that every lightmapped world program uses, not only the fence one, so this is now a world shading
parity fix.

## Problem

Baked lighting on world surfaces is sampled once at the lightmap UV. Retail takes four samples and averages them,
which gives slightly softer baked light and shadow edges. Our engine did this only for water (family 33).

## Root cause

`retail_world.wgsl` read the lightmap with one `sample_lightmap` call shared by every family; the 4-tap average
existed only inside the family 33 water branch.

## Evidence (retail programs)

From `data/big/shaders_final.big`, read with the Xenos shader microcode disassembler (instruction slots; fetch
offsets from the texture fetch instruction's offset fields, in texels):

- [code] `transparentenvironment_defaultPS` 23..26: four `tf3` (lightmap) fetches at offsets (+0.5, +0.5),
  (-0.5, +0.5), (-0.5, -0.5), (+0.5, -0.5); 28, 31, 33 sum them, 34 scales by 0.25 (literal), 37 squares.
- [code] The same four corner fetches of `tf3` are in every lightmapped environment program:
  `baseenvironment`, `defaultenvironment`, `transparentdefaultenvironment`, `alphatestdefaultenvironment`,
  `environmentdiffuse`, `decalenvironment`, `decal2environment`, `decalenvironment_tileable`,
  `decalenvironment_simple`, `decalenvironment_simple_tileable`, `baseenvironmentreflective`,
  `baseenvironmentreflective_simple`, `transparentbaseenvironmentreflective`, `transparentenvironment`,
  `transparentenvironmentreflective`, `advertisement`, `water`, `wateralpha`, `flowingwater`,
  `flowingwateralpha` (all `_defaultPS`).
- [code] `flowingwater_defaultPS` 60..63 fetch the four corners at one coordinate, 67..69 add them, 70 scales by
  0.25: the same average as `water_defaultPS` 96..99.
- [code] No corner fetches in `tree`, `treeanimate`, `ocean`, `oceanreflection`, `baseincandescent`,
  `scrollincandescent`, `transparentincandescent`, `trafficlight_one`, `trafficlight_two` or the
  `environmentpark*` programs: these keep one tap.

Fence program items that needed no change (checked, our port already matches):

- [code] `tf4` (diffuse) destination swizzle `0x688` = rgba as fetched; 9, 10, 14 square rgb; 53 writes
  `alpha * alpha`.
- [code] `tf5` (specular map) destination swizzle `0xfc1` = (y, x, masked, masked): the map's red channel is the
  specular mask (42 multiplies the highlight by it), green sets the power, `10 + 290 * green` (30: `290 * g`,
  36: `+ 10`). Our `masks = sample_specular_map(...).rgb` uses `masks.x` as the mask and `masks.y` for the
  power (`retail_world.wgsl`, the `flags & 16u` specular term), so the channels agree.
- [code] The highlight uses the shadowed lightmap's green (41) and is added after the alpha-scaled diffuse term
  (43), so it is not multiplied by alpha; our port does the same.

## Evidence (retail render state of the fence draws)

- [code] The `transparentenvironment` instance's technique setter `sub_82CEC170` matches the technique name hash
  (0x2EA8FB98 default, 0x935F85F9 skatepark) and picks entry 2 of the depth state table 0x8307DDA0, entry 27 of
  the blend state table 0x8307DE80 and entry 1 of the rasterizer table 0x830782DC (checked in the recomp code).
- [code] Those objects are built once in `sub_82CF5A10` (descriptor contents recovered by constant propagation of
  its stores; not re-derived by hand): depth state 2 = test on, LESSEQUAL, write on, stencil off; blend state 27 =
  SRC_ALPHA / INV_SRC_ALPHA / ADD for color and alpha, color write RGB only, alpha test on, GREATEREQUAL,
  reference 16 (read as 16/255, the convention under which the world alpha-test state's reference 30 matches our
  `ALPHA_REF = 30/255`), alpha to coverage off; rasterizer state 1 = the one-sided cull the opaque world
  techniques use.
- [code] The alpha test runs on the shader's output alpha, which for this program is diffuse alpha squared (slot
  53), so texels with diffuse alpha below about 0.25 are discarded.

## Evidence (retail `advertisement.default`)

Billboards (37 materials: DownTown 16, MegaPark 9, University 10, Industrial 2; diffuse and lightmap only, flags
0). They had no family: the converter stored 0, and the renderer drew them as family 1 (environment.default),
logging them as unsupported.

- [code] `advertisement_defaultPS`: diffuse squared; lightmap boxed over four corners and squared (19..30); `min`
  with the shadow plus (0.05, 0.09, 0.13) (31); `max` with the global constant `g_ViewDotLight.z` (c6.z) (32);
  times diffuse (33) and `m_params.y` (c10, slot 34); fog (35); then the reduced output curve (36..41), the same
  instruction sequence as `environmentdiffuse_defaultPS` (our family 8). No `kd`, no normal, specular, detail,
  macro or cube map; output alpha 0. Verified by hand in the disassembly.
- [code] Render state (technique setter `sub_82CAA0F8`, decoded from the recomp code): depth state 0, blend state 0,
  rasterizer state 1, the plain opaque world state.
- [code] `g_ViewDotLight` is one global float4 shared by every draw, bound by name hash 0xE552F1C3 in
  `sub_826DD6B8` to `*(0x83083C60)+0x44370` (verified by hand in the recomp code).
- [trace] Read in the recomp on Industrial (watch list, 2026-10-09): .y 0.02 and .z 0.4 constant, .w 0, .x
  changes with the view (0.32..0.59). The recomp is not retail, but these are the game's own constants.
- [trace + code] Writer: recomp before/after probes on the renderer's methods (`src/research/hooks_render.cpp`,
  four rounds) narrowed it to `sub_828012D0` (skate3_recomp.38.cpp:57749), reached from the per-frame update
  `sub_827FF7D0` through `sub_82800FF8`. It stores (`stvx` at view + 0x44370)
  `(bias + scale * dot(L, light), tree floor, light floor, 0)`; L is row 2 of the camera matrix the caller copies
  (`sub_827A0C70`), and the four inputs are looked up by hash from the VLT `rendering` row `default` (the same in
  every district): light (0.5, 0, -0.879), (bias, scale) (0.5, 0.2), tree floor 0.02, light floor 0.4.
- [trace] Checked: a recomp run logging the camera's view direction (listener record `*(0x830CFDD4)+32`) next to
  .x matches `0.5 + 0.2 * dot(view direction, light)` to four decimals; the opposite sign does not fit.
- [code] `tree_defaultPS` / `treeanimate_defaultPS` slots 6..7 / 9..10: `max(lightmap, g_ViewDotLight.y) *
  g_ViewDotLight.x`. Our trees used a fixed 0.3435 / 0.02 from an earlier reference capture.

## Evidence (retail `incandescent.transparent`)

Lit signs (9 materials on Industrial, `obj_SignsAds_*`: a `diffuse` binding and an `exposure` scalar of 0). They
had no family either and drew as family 1, which wrongly added the lightmap, shadow and `kd`, and wrote alpha 1.

- [code] `transparentincandescent_defaultPS` (verified by hand): one diffuse fetch (tf3, rgba); diffuse squared
  (3) times `m_params.y` (c3, slot 4); fog (5); the full output curve (6..13); output alpha = diffuse alpha (14).
  No lightmap, shadow or `kd`.
- [code] Technique setter `sub_82CED6D8` (verified by hand: depth entry 2, blend entry 24, rasterizer entry 1).
  Blend state 24 (descriptor decoded from the recomp code, not re-derived by hand): SRC_ALPHA / INV_SRC_ALPHA / ADD for color and alpha,
  RGBA writes, no alpha test; depth state 2 writes depth; rasterizer 1 is one-sided.

## Evidence (retail traffic lights)

DownTown only: `trafficlight.one` (15 materials) and `trafficlight.two` (17), `diffuse` binding only. They had no
family and drew as family 1, with every lamp showing its "off" texels.

- [code] `trafficlight_one_defaultPS` and `trafficlight_two_defaultPS` (verified by hand): one diffuse fetch, NOT
  squared, times `m_params.y` (slot 3), fog (4), the full output curve (5..12), alpha out (13).
- [code] Vertex programs (decoded with a hand-set literal base, since our disassembler cannot read .vpo headers; not re-derived by hand): the
  only difference is the constant `g_TrafficLightsStatus_1` or `_2`. Each vertex reads lamp slot
  `floor(4 * uvA.z)` and uses its second UV set when that component is above 1.0.
- [data] Our converter already carries both: uvA.zw arrive as the decal UVs (`decal.x` = lamp slot), the second
  UV set as the lightmap UVs (`tools/asset_pipeline/map_writer.py:216-219`), so no converter or map change.
- [code] Technique setters `sub_82CE9BE8` / `sub_82CEA368` (verified by hand): depth entry 2, blend entry 26
  (opaque, RGB writes), rasterizer entry 0. Entry 0 is built in `sub_82CF5A10` (store into the table checked by
  main at `skate3_recomp.83.cpp:70585`); its cull word is 4 against the world's 5, so it does not cull (decoded by
  the recomp code).
- [code] The two status constants are bound by name hash (0xC8E04C37, 0xC8E04C34) in `sub_826DD6B8` to the
  renderer fields `*(0x83083C60)+0x443E0` / `+0x443F0` (from the recomp code).
- [trace] Read in the recomp (watch list, DownTown Aletown, 2026-10-09, a local data folder):
  each component is 0 or 2.0; slots 0 red, 1 amber, 2 and 3 green; a 17 s cycle: direction 1 green 7 s, amber
  1 s, all red 0.5 s, then direction 2 the same. This matches the `trafficlights` controller timing already in our
  living-world notes (0.5 / 8 / 0.5 / 7 / 1 s).

## Evidence (retail `animated.flag`)

The DownTown memorial flag (6 materials). It had no family and drew as family 1, static.

- [code] `vertexanimate_defaultPS` (verified by hand): lightmap (tf3, one tap) squared, times `g_ViewDotLight.x`
  (c0.x), times diffuse squared (5..8); fog; the full output curve; alpha = diffuse alpha (18). No shadow, no
  `m_params`.
- [code] `vertexanimate_defaultVS` (decoded with a hand-set literal base, not re-derived by hand; our disassembler
  cannot read .vpo headers): per axis `w * amplitude * sin((t - w) * frequency + phase)` with t =
  `g_fAnimationTime` (c7), weights w = (E2.z - 0.5, E2.w - 0.5, 1 - E2.z - E2.w) from the second TEXCOORD's zw,
  amplitudes c8.w / c9.w / c10.w, frequencies c9.xyz, phases c10.xyz.
- [data] `m_params` of the VLT row `material_animated` / `flag`: c8 (0, 1, 0, 0.1), c9 (10, 9, 5, 0.1), c10 (1, 6, 5,
  0.2).
- [code] Technique setter `sub_82CF1528` (decoded from the recomp code): depth 2, blend 1 (alpha test, no blending), rasterizer 0
  (no culling).
- [data] Its second TEXCOORD is four normalized shorts; our decoder kept only xy (the lightmap UVs).

Our engine before this change: `AlphaMode::Blend` made Bevy turn depth write off
(`vendor/bevy_pbr/src/render/mesh.rs`, the `BLEND_ALPHA` arm) and write alpha with Bevy's blend, and blended
materials had cutoff -1, so nothing was alpha-tested; the shader's test compared the raw diffuse alpha.

## Change

- `retail_material_bindings.wgsl`: new `sample_lightmap_box(slot, uv)`: four taps at the half-texel corners of
  the lightmap, averaged.
- `retail_world.wgsl`: families 1 to 8, 13 and 16 read the lightmap through it; the water branch (families 30
  and 33) uses it at its refracted lightmap UV. Trees (9, 10), proxy (11), incandescent (12, 14), ocean (31,
  32) and the unknown-shader fallback (0) keep the single tap.
- `retail_render.rs`: new render class `BlendedDepthWrite` for family 16 (whatever the material's stored alpha
  mode): blended, depth write on, color writes RGB only, one-sided culling; cutoff `TRANSPARENT_ALPHA_REF` =
  16/255 (`class_and_cutoff`).
- `retail_world.wgsl`: for family 16 the alpha test compares the output alpha (diffuse alpha squared).
- Advertisements: new family 17 (`ADVERTISEMENT_FAMILY`). The converter maps `advertisement.default` to it
  (`retail_material.py`), packages that stored 0 are upgraded on load, and the shader gives it the box lightmap,
  shadow, `m_params`, `kd` = 1 and the reduced output curve. The light floor (slot 32) is `max(L, view_dot_light.z)` with the
  retail 0.4 as the default of a new `FrameStateData::view_dot_light` row (shared by every draw, like retail).
- `g_ViewDotLight`: `advance_view_dot_light` computes it every frame from the retail camera's forward axis and
  `ViewDotLightParams` (exported by `render_parameters.py` as `rendering.default` into `render-parameters.json`;
  the retail values are the fallback). Trees (families 9 and 10) now take their lightmap scale and floor from it,
  so their brightness follows the view direction as in retail.
- Billboard multiplier: `m_params.y` of the VLT `material_advertisement` row is 0.35 (0x3EB33333; the other world
  classes carry 1.0, a local research note, row verified by hand). `render_parameters.py` now
  exports the advertisement rows into `render-parameters.json`, and family 17's `surface.w` takes `m_params.y`
  from there (`material_multiplier`; retail 0.35 when an install predates the export). Before this our billboards
  were about 2.9 times too bright.
- Memorial flag: family 21 (`ANIMATED_FLAG_FAMILY`). The vendored extractor decodes the second TEXCOORD's zw
  (`decode_secondary_texcoord_zw`) into the flag's otherwise empty decal UV set; `render_parameters.py` exports the
  `animated.flag` rows; the world and depth vertex stages add the sway (`flag_sway`), the pixel stage the
  lightmap / `g_ViewDotLight.x` terms; class `CutoutTwoSided` at the world alpha reference. Installs that predate
  the export keep the flag on the fallback (the family needs its three rows).
- Videoscreens (`incandescent.videoscreen`, MaloofMoneyCup 7 and MegaPark 1 materials): `videoscreen_defaultPS` is
  byte-identical to `baseincandescent_defaultPS` (verified by hand), so they join family 12; the converter maps them,
  stored 0 is upgraded on load. Family 12 now applies `m_params.y` (`surface.w`) like retail (slot 4); its row
  comes from `render-parameters.json` (videoscreen 0.25, already exported; `incandescent.default` 1.0, unchanged).
  Render state (decoded from the recomp code): technique setter `sub_82CF1BE0`, depth 0 / blend 0 / raster 1, the same as
  baseincandescent (`sub_82CAED90`), so family 12's class fits. In retail the big park screens show a live view:
  checked on the PS3 version in RPCS3 (user, 2026-10-09): the screens show the skater's view without the HUD. The recomp
  draws the static placeholder texture ("JUMBO TRON TEMP") instead. The live feed: see "Videoscreen live feed".
- Lit signs: new family 18 (`INCANDESCENT_TRANSPARENT_FAMILY`), converter mapping and on-load upgrade like 17;
  the shader takes the family 12 path (`lin = d`) plus `m_params` and alpha = diffuse alpha. New render class
  `BlendedDepthWriteRgba` (blend state 24: depth write, all channels written, no alpha test).
- Traffic lights: families 19 / 20 (`TRAFFIC_LIGHT_ONE_FAMILY` / `_TWO_`), converter mapping and on-load upgrade.
  The shader picks each vertex's UV set from `FrameStateData::traffic_lights[family - 19][floor(4 * color.x)]`
  and draws the unsquared diffuse. `TrafficLightCycle` (a resource holding the retail phase table, looped on
  game time) writes those rows every frame; a mod or the traffic controllers can replace the table or the clock.
- Both depth-writing classes blend alpha with SRC_ALPHA / INV_SRC_ALPHA like retail (Bevy's default blend uses
  ONE for the source alpha).

## Files

- `crates/skate-game/src/retail_material_bindings.wgsl` (`sample_lightmap_box`, `flag_sway`, frame-state rows)
- `crates/skate-game/src/retail_world.wgsl` (4-tap lightmap, families 12 and 16 to 21, tree terms, flag sway)
- `crates/skate-game/src/retail_depth.wgsl` (flag sway in the depth prepass)
- `crates/skate-game/src/retail_render.rs` (families, render classes and cutoffs, `material_multiplier`,
  `ViewDotLightParams`, `TrafficLightCycle`, frame-state rows, tests)
- `crates/skate-game/src/retail_character.rs` (test fixture: the new frame-state rows)
- `crates/skate-game/src/skate_world.rs` (`SKATE_RENDER_READY` counts the new classes)
- `tools/asset_pipeline/retail_material.py`, `tools/asset_pipeline/test_environment.py` (families)
- `tools/asset_pipeline/render_parameters.py` (advertisement, animated.flag and `rendering.default` rows)
- `tools/vendor/university/tools/vanilla_map_extraction/tools/retail_lightmap_uv.py`,
  `.../prepare_hawaiian_dream.py` (the flag's sway weights)

## Setup impact

`retail_material.py` (families 12 and 16 to 21; also in the `maps` group) and `render_parameters.py` (billboard,
flag and `rendering.default` rows) are in the `environment` setup group, and the extractor change for the flag's
sway weights is in the vendored parsers shared by the `character`, `environment` and `maps` groups
(`tools/asset_pipeline/versions.py`). So the next setup refresh rebuilds those groups, characters included.
Existing installs look the same before the refresh: stored family 0 is upgraded on load, the billboard
multiplier and `g_ViewDotLight` fall back to the retail values, and the flag stays on the fallback family until
its rows and weights are in the data.

## Moddability

- Families follow the material's shader name (map data, mapped in `tools/asset_pipeline/retail_material.py`), so
  a mod or custom map material that names one of these shaders gets the retail shading, render state and lightmap
  filtering with its own textures. The fixed numbers in the shaders (tap offsets, weights, the 16/255 fence
  reference, the output curves) are the retail programs' and states' values.
- Material multipliers (`m_params`), the flag's sway rows and the `g_ViewDotLight` inputs come from the setup data
  (`render-parameters.json`); a mod can override them there, and the retail values are the fallback.
- `g_ViewDotLight` and the traffic light status live in the shared frame state; `ViewDotLightParams` and
  `TrafficLightCycle` are resources a mod or another system (the traffic controllers later) can replace.

## Retail parity and tests

1:1 with the retail programs and render states listed above; values the game sets at run time
(`g_ViewDotLight`, the traffic light status rows) were read from the running game in the recomp and checked
against the code.

Tests (`retail_render::tests`): `lightmapped_world_families_average_four_lightmap_taps`,
`classes_split_on_culling_blending_and_alpha_testing`, `transparent_environment_shader_matches_retail`,
`advertisement_takes_its_own_family`, `incandescent_transparent_takes_its_own_family`,
`traffic_lights_take_their_own_families`, `traffic_light_cycle_matches_the_recomp_reading`,
`view_dot_light_follows_the_camera_like_retail`, `animated_flag_takes_its_own_family`. The naga validation test
`world_shader_validates_under_non_uniform_material_slots` (`retail_shader_tests.rs`) covers the edited shaders.
Python: `tools/asset_pipeline/test_environment.py` checks each shader's family.

## Verification

- `cargo test -p skate-game --locked` (2026-10-09, on `main` 2e166977 plus this change, families 17 and 18 and
  the frame-state rows, trees and the memorial flag included): 492 passed, 1 failed, 179 ignored. The failure is the known pre-existing upstream one,
  `setup::tests::pipelines_accept_valid_group_outputs_when_fingerprint_changes`
  ([PULL-REQUESTS](PULL-REQUESTS.md)). The new and changed tests pass, and
  `retail_render::shader_tests::world_shader_validates_under_non_uniform_material_slots` validates the edited
  shader.
- In game (user, 2026-10-09, Industrial Ghetto Spot, build with the 4-tap lightmap, fence render state and
  family 17): "after running around for a moment it looks much better on the fence. i can't really tell on the
  baked lighting but im probably just missing it." (The 4-tap average is a sub-texel softening, so a small
  difference is expected.)
- In game (user, 2026-10-09, DownTown Aletown, build with traffic lights 19 / 20 and the retail light cycle):
  "Yep the lights render now yay."
- Not done: the cross-map regression check of the converter side (spawns, collision, map validation) needs a
  setup refresh, which was skipped (a refresh from this branch also rebuilds other setup groups of the shared
  install). Collision and spawns are not touched by this change; the converter only assigns families and exports
  setup rows. No side-by-side recomp shot was taken.

## Videoscreen live feed

Problem: in retail the park screens (MegaPark 1, MaloofMoneyCup 7 `incandescent.videoscreen` materials) show the
skater's view without the HUD (the user checked this on the PS3 version in RPCS3, 2026-10-09); ours
showed the static "JUMBO TRON TEMP" texture.

Evidence (retail, research b622 / b623, 2026-10-26):
- All 8 materials bind the same diffuse texture, GUID 0x000020d103e3870a (`blackScreenLogo_`, an ordinary resource
  in `worldDIST_MegaPark.big`), so retail runs one feed shared by every screen [data; counted again in our
  converted maps: Maloof 7, MegaPark 1]. The GUID is not in the TU3 (title update 3) image and no code builds it [code+data], so the
  feed reaches the texture through a generic path.
- One SceneRenderView only: constructor `sub_82811B88` has a single caller, `sub_826D35F8` [code, rechecked].
- Ruled out as the producer: the render singleton's +0xC9D0 and +0x19940 members are shadow (light-space) views
  (`sub_827FCC60` builds +0x19940 from the last CSM (cascaded shadow map) split and the light basis written by `sub_827FFFB0`) [code];
  `sub_82790000` is the skater photo renderer (`PhotoEffects::GeneratePhoto` 0x8219D4D8,
  `SkaterPresentationPhoto::GeneratePhotoRequest` 0x821B81B4; only caller `sub_8278F4C0`) [code+data, rechecked].
- The frame function `sub_827FF7D0` draws the HUD (Render2D, `825D9168`) after every scene pass, so any scene-side
  copy is HUD-free [code].
- Not found yet: the producer itself, so the feed's resolution, update rate and camera pose are not retail values.
  The recomp is not a reference here (it draws the placeholder). Leads: research b623.

Change (`video_screens.rs`):
- `MaterialTable::build` lists the videoscreen materials; their `mode.y` gets `VIDEO_SCREEN_FLAG` (1024), and every
  map spawn adds one `VideoScreen` entity per screen material (id `<map>/<material index>`, world bounds).
- One feed camera (`FeedCamera`) follows the gameplay camera's pose, field of view and tone components each frame
  and renders into an image; a render-world copy (`copy_feed`, after the graph) puts it into `VIDEO_FEED`, which
  every `WorldMaterial` binds (14 / 15). The world shader takes the screen's diffuse from the feed while
  `frame_state.video_feed.x` is 1 (`sample_video_feed`: V flipped back, square root so the program's diffuse
  squaring returns the linear feed); the static texture stays the picture when the feed is off. The copy keeps the
  camera from rendering into the texture its own screens sample.
- The feed camera tone-maps with the shared exposure state without metering (`ToneOnly`), so the main view's
  adaptation is unchanged. UI never targets it (image target); the two "any 3D camera" queries
  (`graphics_menu.rs`, `peds.rs`) exclude it.
- Cost: the camera is active only while a screen is in the gameplay camera's frustum within `max_distance`, so
  other maps never render it, and draws only on frames a feed update is due (`rate_hz`, frame-independent): on the
  other frames `idle` empties its visible lists and its shadow cascades' lists, so the view stays alive but draws
  nothing. Switching the camera itself per feed frame (the first version) let Bevy's texture cache drop the view's
  textures (unused for 3 frames) and re-create them every feed frame.
- Defaults (provisional, not retail): 512 x 288, 30 per second (console cadence), 300 m.
- The feed decides before Bevy builds the directional shadow cascades (`schedule` runs before
  `UpdateDirectionalLightCascades`): a camera switched on after that build has no cascade and `prepare_lights`
  panics (vendored bevy_pbr `light.rs:1508`, hit in the first capture).
- Measured 2026-10-26 (dev build, RTX 4080 SUPER, MegaPark, camera on the screen, 35 s `--verify` runs): feed off
  3.55 ms / 1 % low 5.33 ms; feed at 30 per second 3.65 ms / 5.62 ms; feed every frame (`rate_hz` 240) 4.23 ms /
  8.75 ms. That first version re-created the feed's textures per feed frame (render prep peaks 8 to 14 ms, one
  33.6 ms; worst frame in 5 s 31.6 ms, 0.1 % low 15.2 ms). With `idle` (same pose, same day, noisier machine): feed
  on 3.86 ms / 1 % low 5.99 / 0.1 % low 6.89 / worst 9.4 ms, render prep at most 2.4 ms in any hitch; feed off
  4.05 / 7.73 / 9.04 / 9.3 ms. A laptop measurement is open.

Moddability: world tuning domain `video_screens` (`sdk.world.set_tuning('video_screens', {enabled, width, height,
rate_hz, max_distance})`), first writer wins per field, restored when the mod stops. Screens carry stable ids for a
later per-screen source (own camera / image), which is not built yet. Multiplayer: presentation only; each machine
shows its own player's view.

Tests: `video_screens.rs` (cadence at 30 / 60 / 144 / 240 fps, frustum and distance gate),
`modding/world_tuning.rs` (`video_screens_set_merge_and_reset_on_disable`), skate-mods patch validation.

## Open questions

- Videoscreen feed camera (settled 2026-10-26, USER-ASKS 379): measured from the user's RPCS3 clip of Maloof
  (clip of 2026-10-26 11:06, 0:15 / 0:17 / 0:45; research b643), the screen shows nearly the
  same framing as the gameplay view (same heading, lens and tilt; at most about 0.8 m pulled back), so the feed
  keeps copying the gameplay camera (the user's decision: keep as is). The TU3 360 code has no second world camera for it
  (research b626 / b638). Resolution and update rate are still not measured.
- Pass and sort order of the fence draws in retail is not traced yet (two recomp trace runs did not capture a
  fence apply; the bind slot is `sub_82CA5C60`, the per-frame renderer update `sub_827FF7D0`);
  Bevy sorts them back to front in its transparent phase. With depth write and alpha test on, order matters
  less.
- Memorial flag: not checked in game yet (needs a setup refresh for the weights and rows). `g_fAnimationTime`
  counts game seconds like our `clock.x` (read in the recomp, 2026-10-09: about 0.82 per wall
  second while the recomp ran below full speed, not frames); the vertex program decode was not re-derived by
  hand. The vendored parser change moves the fingerprint of every setup group that includes the parsers
  (`versions.py` PARSERS: character, environment, maps), so a refresh re-extracts characters too.
- `model_default` (University, 1 material) is EA's placeholder for a missing material ("Pipeline Missing
  Material", no textures), not a retail shader; nothing to port. No other world shader is left on the fallback.
- Traffic lights (drawn two-sided, class `OpaqueTwoSided`): the retail cycle's start phase and whether
  each crossing has its own offset are not known (the recomp shows one shared pair of rows for the whole map).
  Once the living-world traffic controllers land, they should drive `TrafficLightCycle` so cars and lamps agree.

## Code reference: retail_backdrop.rs

Detail moved out of the code comments of `crates/skate-game/src/retail_backdrop.rs`; each comment there links its section here.

### District backdrop

The district's global presentation model: retail's world record points each district at one extra model outside its world stream (world fields `951898F6C0FA6856` model / `CA5A157A65E75934` textures, for example `DIST_Water_Industrial`). It holds Industrial's sea (`ocean.reflection` around the docks and harbor), the far sea planes of DownTown and University (`environment.reflective_simple`) and the distant tree walls. Setup exports it to `private/native-backdrops/<map>.skate` (`tools/asset_pipeline/backdrop.py`).

Without it Industrial's harbor shows the clear color: a black void.

Far-proxy terrain: retail also streams `proxy<District>_100_Proxy.big` (`proxyworld.default` hills plus their trees) and swaps it per grid cell: while the full-detail cell `cPres_X_Z_high` is active its partner `cPres_X_Z_high_proxy` is deactivated (TU3 proxy world manager sub_8247EF40, streamer sub_8247BB50 -> sub_82C985A8). The engine keeps every district cell loaded, so setup exports only the unpaired proxy cells to `private/native-backdrops/<map>.proxy.skate` (Industrial's south hills under the tree wall; DownTown and University have none). Its draws carry [`ProxyTerrain`] and follow `BackdropSettings::proxy_terrain`.

Moddability: `BackdropSettings` is the one authority; mods reach it through `sdk.world.set_tuning('backdrop', {visible = false})` (world tuning domain `backdrop`), and a disabled mod gives it back (retail: visible). The package itself is a content file a mod can replace.

## Code reference: colour_matrix.rs

Detail moved out of the code comments of `crates/skate-core/src/colour_matrix.rs`; each comment there links its section here.

### Color matrix records

Retail post FX color matrices (`post_fx` vault records `colour_matrix`, `colour_matrix_zombiemode`, ...; research b100). Each record has a near and a far band; a band's values build one 3x4 affine color matrix with `sub_827F13D8` [code]: contrast about a midpoint plus an add, then saturation with luma weights (0.3, 0.6, 0.1), then a per-channel multiply: `out = multiply * Sat_s(contrast * (x - midpoint) + midpoint + add)`.

The multiply-last order is [inferred] from the instruction order (b100); the base record is the identity.

### Tone pass inputs

What the retail tone pass applies (`postfx_visualfxPS`, research b101): near rows (c7..c9), far rows (c10..c12), the depth weights `(mult.x, mult.y, add.x, add.y)` (c13 / c14: `wA = sat(z * mult.x + add.x)`, `wB = sat(z * mult.y + add.y)`, linear in view depth between the record's distances, `827F3D20`), `(camera near, 0, 0, 0)` and the replay's tint overlay RGBA (alpha 0 = none).

## Code reference: retail_render.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/retail_render.rs`; each comment there links its section here.

### Advertisement family 17

Shader family of the retail `advertisement.default` materials (`advertisement_defaultPS`): billboards. Diffuse^2 times the boxed, shadowed lightmap (slots 19..31), raised to a global light floor (slot 32), times `m_params` (slot 34); no `kd`, no normal or specular map, plain opaque state, and the reduced output curve of `environmentdiffuse` (slots 36..41).

### Incandescent transparent family 18

Shader family of the retail `incandescent.transparent` materials (`transparentincandescent_defaultPS`): lit signs. Diffuse^2 times `m_params` (slots 3..4), fog, the full output curve, output alpha = diffuse alpha (slot 14); no lightmap or shadow. Drawn with blend state 24: blended, depth write on, all four channels written, no alpha test (technique setter `sub_82CED6D8`).

### Traffic light families

Shader families of the retail `trafficlight.one` / `trafficlight.two` materials (DownTown). Byte-identical pixel programs: unsquared diffuse times `m_params` (slot 3), fog, the full output curve; the vertex programs pick each lamp's UV set from `g_TrafficLightsStatus_1` / `_2`. Opaque blend state 26 (RGB writes), depth state 2, rasterizer state 0 (no culling).

### Animated flag family 21

Shader family of the retail `animated.flag` material (DownTown memorial flag): `vertexanimate_defaultPS` (lightmap^2 * diffuse^2 * g_ViewDotLight.x, fog, full curve, alpha = diffuse alpha) with the cloth sway of `vertexanimate_defaultVS` (`m_params` c8..c10 in `water[0..2]`, weights in the decal UV set). Blend state 1 (alpha test, ref 30), rasterizer 0 (two-sided).

### World shadow floor

Retail dynamic-shadow floor on the baked world, RGB. Every retail world receiver shader (`defaultenvironment_defaultPS`, `environmentdiffuse_defaultPS`, `baseterrain_defaultPS`, the decal / reflective / transparent variants and `water_defaultPS` / `flowingwater_defaultPS` in `shaders_final.big`) computes `min(lightmap^2, csm_visibility + (0.05, 0.09, 0.13))`: the same constant for every caster and receiver, with no per-caster height or depth window. A shadow that lands in baked shade darker than this floor therefore leaves no mark, which is what keeps a car on a bridge from printing its shadow onto the shaded ground below (the city geometry itself casts nothing into this map).

### ViewDotLight inputs

Inputs of `g_ViewDotLight`, from the VLT `rendering` row `default` (the same in every district): `sub_828012D0` stores `(bias + scale * dot(camera forward, light), tree_floor, light_floor, 0)` (`stvx` at view + 0x44370; the four values looked up by hash). Checked in the recomp: the logged .x matches the camera's view direction with this formula. Loaded from `render-parameters.json` (`rendering.default`), so a mod can change them; the retail values are the fallback.

### Fountain donor binding

Better Water fountains (water_look.rs `fountain_water`, not retail): every family 30 material whose detail, macro and decal slots are empty gets the map's largest family 33 body (the donor) bound there: its normal in macro, its `normal2` in detail, its diffuse in decal, all repeating, and its `water` rows in `fountain`. Bound at load whatever the look, so the toggle stays a run-time switch; the presence flags in `mode.y` stay as authored, so the retail program never reads these slots.

### Water scroll time

Water scroll time: retail's `g_fAnimationTime`, which sub_82791628 accumulates without a reset at 1.0 per second (recomp watch, doc 09), times the look's `scroll_rate`. Loops over a period at which every authored scroll speed (all multiples of 0.001) completes whole texture repeats, so the loop is invisible, while keeping t small enough for f32 shader precision. `seamless_clock = false` publishes 0x82790858's restarting value instead (a jump every 10 s; never shown by retail).

### Traffic lamp states

The lamp states the retail renderer uploads as `g_TrafficLightsStatus_1` / `_2`: one row per crossing direction, one component per lamp slot (0 red, 1 amber, 2 green, 3 lit together with green), 2.0 = on.

Read in the recomp (DownTown Aletown, a local data folder): a 17 s cycle, direction 1 green 7 s, amber 1 s, all red 0.5 s, then the same for direction 2, matching the `trafficlights` controller timing in the living-world notes. Every traffic light shares the two rows, so all crossings show the same phase. A resource so a mod, or the traffic controllers once they drive cars, can replace the table or the clock.

## Code reference: retail_world.wgsl (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/retail_world.wgsl`; each comment there links its section here.

### Water look extra layers

Water look: one fine layer at a larger ripple scale tiles visibly over large stretches (DownTown fountain), so add the same PCA wave from a rotated sample at 0.613x and broad swells from the large layer, matched by eye to RPCS3 footage. Smaller bodies move less: `decal.y` is the body's area (water_bodies.rs), 100 m² and below calmest, 4000 m² and up (Aletown canal) full.

### Fountain shading

Water look fountains: flat family 30 pixels (basins) take the map's large-water program, with the donor's PCA textures (macro = normal, detail = normal2; its diffuse in decal is not read) and rows (p.fountain, bound by retail_render::bind_fountain_donor) in world space at fountain.x meters per tile; steep pixels (cascades) keep their own flow and gain foam. Blended by slope so stepped fountains do not seam.

### Park projected lightmap

29..30, 54..56, 66..67, 77..78: min(lerp(dynamic color, 1, blurred visibility), lerp(exponential color, 1, saturate(10 N.L) * exponential visibility), projected lightmap). Retail has two shadow terms; our one shadow map feeds both (ours). The park's projected lightmap (skateparkProjectedLightmap, one tap, used linearly; b439 / b440 / b444): g_skateparkProjectedTransform from the world's bounds, the ground projection along the sun (L = the sun direction, toward the sun): u = (z - z0) / dz + y lz / (ly |dz|), v = (x - x0) / dx - y lx / (ly |dx|). Texture rows are flipped at export (map_writer), so v is flipped back. No bounds: neutral 1.

## Code reference: retail_shader_tests.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/retail_shader_tests.rs`; each comment there links its section here.

### Kd of families 7 and 8

Families 7 / 8 are `environmentsimple.alphatest` / `environmentsimple.diffuse` (`tools/asset_pipeline/retail_material.py`), whose VLT effects are `alphatestdefaultenvironment` / `environmentdiffuse`. Both retail programs (`shaders_final.big`, instructions 27-34 / 26-34) compute `min(avg4(lightmap)^2, vis + floor) * diffuse^2 * m_params[0].y` with no kd term, so kd is exactly 1 for them. The other families without a normal map keep the flat-normal kd `0.39 * 2.3956` of the normal-mapped programs.

### Kd normal normalization

Which programs normalize the tangent normal before the kd dot product. `environment.default` (family 1, VLT effect `baseenvironment`) does (`baseenvironment_defaultPS` 29-33: rsq of the combined normal, kd from the normalized xy at 66). `environmentsimple.default` (family 2, effect `defaultenvironment`) and `environment.reflective_simple` (family 6, effect `baseenvironmentreflective_simple`) do not: `defaultenvironment_defaultPS` takes kd from the raw `2n - 1` (38, 61, 63, 67) and `baseenvironmentreflective_simple_defaultPS` from the raw `2n - 1` (14-22, 45, 63, 74-76); their rsq instructions normalize only the view vector, the world normal used for specular / reflection and the tangent-frame sun whose signs pick the kd weights.

## Code reference: retail_material_bindings.wgsl (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/retail_material_bindings.wgsl`; each comment there links its section here.

### Flag sway

Cloth sway of the retail `animated.flag` (family 21), vertexanimate_defaultVS slots 7..17: per axis `w * amplitude * sin((t - w) * frequency + phase)`, with the weights from TEXCOORD1 zw (exported as the decal UVs, `color.xy`, V flipped: zw = (color.x, 1 - color.y)) and `m_params` c8..c10 in `water[0..2]` (amplitudes c8.w / c9.w / c10.w, frequencies c9.xyz, phases c10.xyz). Zero for every other family.

## Code reference: retail_exposure.rs / retail_exposure.wgsl

- The meter reads what retail's CPU meter (`sub_827F0B78`) reads: the alpha of the bloom downsample
  (bloom_dof_tap4_minusthresholdPS), `dp3_sat(sat(tm/2), (0.3, 0.4, 0.3))` (`meter_luminance`), stored 8-bit and read
  back as byte / 255, center-weighted, scaled by 2.515 and fed to the native multiplicative evaluator (`sub_827F0D00`).
- NOT RETAIL: it samples a 16x16 bilinear grid of the HDR target instead of the console's 4-tap downsample chain (box
  averages; same mean).
- The weights and the scale are a mod tuning domain (`exposure`, `ExposureMeter`).
