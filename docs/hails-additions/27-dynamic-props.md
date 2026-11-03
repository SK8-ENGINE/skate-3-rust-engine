# 27. Dynamic props (movable DMOs) from upstream PR #15

Status: cherry-picked onto `world/dynamic-props` (from `world/living-world` 5127ae6), builds and tests green on
Windows in both link configurations; not yet checked in game. Planned to ride with the living world (upstream draft
#52) as its dynamic-object part; the retail DMO system is planned in `.local/research/npc/dmo-plan.md`.

## Credit

All prop gameplay here is **laaledesiempre's** work: upstream PR
[#15](https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/15) "[Feature] Dynamic props: collide, grab/drag,
place (retail ObjectMove)" (draft, issue #13). Their 12 commits are cherry-picked with `git cherry-pick -x`, so each
keeps their authorship and records the original hash. #15 is based on their Linux/macOS port (#9); none of #9 is
brought in. The upstream DMO placement exporter (`tools/asset_pipeline/dynamic_props.py`) that #15 extends is
upstream's.

## Problem

Retail districts are full of movable objects (benches, trash bins, dumpsters, ramps, vending machines; 226
placements in DownTown). Upstream main exports their placements but, since `e2b85b6` replaced the renderer, no
longer loads `private/native-props` at all: props are neither drawn nor collidable. #15 makes them live instances
that collide, can be pushed, grabbed, dragged and placed, with layouts saved per map.

## What #15 implements (their phases)

- **Phase 0, live instances:** the exporter writes template geometry once per DMO template and one MOBJ schema 4
  record per placement (schema 3 fields plus a 12-float row-vector affine). Parser and validation in
  `skate-data::skate_map` (`StaticObject.transform`, later `ObjectPhysics`).
- **Phase 1, collision:** each instance's render triangles, baked to world space, form a second `BoardWorld`
  (`skate_world::build_prop_layer`, `GamePhysics::prop_world`), queried beside the map world by board, skeleton
  and wheel queries. Surface tags use the same `audio | physics<<7 | pattern<<12` mapping as the map.
- **Phase 2, rigid bodies** (`physics/prop_dynamics.rs`): TU3 integrator, retail rounded-box mass properties,
  GP pair queries for box vs world / box / skater volumes, a compact impulse pass; moved instances rebake their
  triangle range (`BoardWorld::replace_triangles`, new in skate-core).
- **Phase 3, grab and carry** (`physics/prop_carry.rs`): A on foot toggles grab of the nearest prop
  (#15; since 2026-10-05 a held RB, the retail GrabWorld button, because A is retail sprint, see doc 26 "Props
  pulled toward the player").
- **Phase 4, placement and layouts** (`prop_carry.rs`, `prop_layout.rs`): B while carrying
  enters placement (right stick distance / yaw, DPad height; now releasing RB confirms); poses persist to
  `settings/prop-layouts/<map>.json` and reload on map load. The placeholder HUD diamond (`prop_carry_hud.rs`,
  white = prop in reach, cyan = carrying, yellow = placing) was removed 2026-10-15: retail has no such marker.
- **Later commits:** authored MOBJ physics (density, friction, restitution, damping, sleep flags), Skate 3 style
  drag carry, surface-distance and omnidirectional grab, momentum-style skater push, body-bump speed cap
  (1.2 m/s), retail MovingObject presentation (grab flag, OffBoardPushing 502, MovingObjectNew subtree with the
  MVOBJ_* animations), MovingObjectNew condition leaves, walking from the ObjectMv stick.

Their own design notes (phases 0 to 4) were in `docs/dynamic-props-and-grime.md` on their branch; upstream no
longer tracks `docs/*.md`, so the key points are summarised here.

## Adaptations to current main (our commits and conflict notes)

1. **Renderer (our commit "Props: adapt #15 to main's renderer").** Phase 0's runtime half (the
   `retail_backdrop.rs` loader, `skate_world::spawn_instances` on the material-batched renderer,
   `SceneEntity::spawn_child`) targets code upstream replaced in `e2b85b6`. Re-implemented on the current renderer:
   `skate_world::load_prop_package` (the old loader's render-only checks), `spawn_instances` (one root entity per
   MOBJ record with `PropInstance { id, template, name }` and the record's affine; the template range is merged per
   (render class, slab) in template space with the props package's own `MaterialTable`, shared by every instance of
   that range), `SceneCommands::spawn_with_children` (only the root carries `MapEntity`, children go with it), and
   `PreparedScene::prepare` spawning props for retail districts as before.
2. **Phase 1:** upstream moved the portable collision tests out of `skate_world.rs`; their prop collision test now
   sits at the end of the current test module with its own `material()` helper.
3. **Phase 2:** `BoardWorld::replace_triangles` is kept next to main's water surface queries.
4. **Phases 3 and 4:** main's `solve::advance` wraps `advance_inner` (mod part physics); `grab_rising`, later
   `carry_tick`, is passed through the wrapper. Main's water board drag call before the solve stays.
5. **Old packages (our follow-up commit):** a props package written before MOBJ schema 4 has no placements; the
   loader now says so ("re-run setup (maps)") instead of silently drawing nothing. A test covers the instance spawn
   (two placements of one template: one mesh, two placed roots, children sharing it, only roots marked).

No other changes to their code.

## Setup impact

`tools/asset_pipeline/dynamic_props.py` and `map_writer.py` change, so the **`maps` and `environment` setup
fingerprints change**: existing installs re-run those two groups once (the customiser and other groups are not
affected). Until then the old props package has no placements and the game logs the hint above; nothing else
changes.

## Verification (2026-10-05)

- Builds: dev (`cargo build -p skate-game --bin skate3rust --release --locked`) and release / CI static
  (`+crt-static`, `--no-default-features`, `--target x86_64-pc-windows-msvc`).
- skate-game: 422 pass (incl. the new instance spawn test), 1 known failure `pipelines_accept_valid_group_outputs_when_fingerprint_changes`.
  skate-core 622 pass, 2 known failures. skate-data `--lib --tests` green. skate-mods green except the Skyline
  GLB test (gitignored model). Python: 177 setup tests OK (2 skipped).
- Not yet: in-game checks (see open questions); `--validate-maps` needs installed assets and a refreshed setup.

## Grabbing spins the player; dragging does not work (2026-10-05)

**Problem.** User, verbatim: "when grabbing objects the player spun and had trouble with dragging them around as
they should be able to". Video `2026-10-05 11-43-41.mp4`: 0:06 to 0:11.5 the skater holds a shopping cart (HUD
diamond cyan), bent over it in the MovingObjectNew pose, and skater and cart circle round each other on the spot
while the camera swings round; 0:43 to 1:14 the same with a bin.

**Root cause [code].** In state 502 (OffBoardPushing) with a held prop, `biped_ground::update` rotated the carried
OB_ObjectMv stick by the skater's current ground frame into a world direction and fed it to the walking
controller (`ground_input::calculate`). That controller only walks forward and turns toward its stick
(TurnVsStickAngle). A stick that is not straight ahead therefore always sits at the same angle from the facing:
the skater turns, the target turns with them, and the turn never ends. Pulling back asks for a 180 degree turn
every tick. #15's prop follow kept the prop on a world bearing, so the prop slid round the turning skater.

**Retail [code][data].** The object-move inputs come from 8259C4B0, called by Fill825999F0 with the current
RawControllerInput (raw left stick X/Y, right stick X):
- angle a = atan2(x, -y) wrapped to (-pi, pi], curve key |a| / pi (constant 822F8610 = 1/pi);
- OB_ObjectMvX = x * curve 1A1A7AC37A72DF87, OB_ObjectMvZ = y * curve 05BA8B52C23B3481 (inputlistener,
  PointNegGraphData8; X gain is 1 everywhere, Z dips to about 0.54 on diagonals);
- OB_ObjectMvRot = clamp(right X + sign(a) * curve 9ADFC2E222938C1E (16 points) * s, -1, 1) (clamp constants
  8216DEE0 = -1, 8231A844 = 1). Every Y value of 9ADFC2E222938C1E is 0, so the left stick never turns the
  skater and object; only the right stick does. The scale s (caller f21) is not resolved; it only multiplies
  that zero curve.
The stock MovingObjectNew graph picks MVOBJ push / pull / left / right clips from the OB_ObjectMv angle.
Retail's physics state 502 (string `PhysState_OffBoardPushing`, 0x82080660) is not decoded, so how far retail
moves the pair per second (likely the MVOBJ clips' root motion) is not known.

**Change.**
- `skate-core` `produce_object_move` ports 8259C4B0 with the three shipped curves (`ObjectMoveCurves`, loaded by
  `PhysicsSettings` from the inputlistener collection) and publishes OB_ObjectMvRot from the right stick (was 0).
- `biped_ground::update` (502 with a held prop): the walking stick stays idle (no turn toward the stick). The
  pair's planar velocity comes from `prop_carry::object_move_motion` (left stick, skater frame: push, pull, side
  step) through the controller's velocity override, so contacts and obstacle rejection still apply; the pair
  turns only by OB_ObjectMvRot.
- `PropCarry::follow`: the prop keeps its grab offset fixed in the carrier's frame (pulled in to 0.9 m) and turns
  at the carrier's yaw rate (`PropDynamics::set_yaw_rate`), so it stays in front of the skater while turning.
- Speeds are engine values until state 502 is decoded: push 1.4 m/s, pull 1.0 m/s, side 0.8 m/s, turn 1.6 rad/s
  (`CarryLocomotion`). Mods: `sdk.world.set_tuning('carry', {push_speed, pull_speed, side_speed, turn_rate})`
  next to `grab_bit`, `placement_bit`, `grab_range`; mod disable restores the defaults. All values are per tick
  and deterministic (no wall clock, no randomness), ready for a later network authority.

**Files.** `crates/skate-core/src/input/offboard_intentions.rs` (+ tests), `crates/skate-game/src/physics/`
`biped_ground.rs`, `prop_carry.rs`, `prop_dynamics.rs` (set_yaw_rate, tests), `settings.rs`,
`offboard/settings.rs`, `animation_phase.rs`; `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-mods/src/api.lua`, `sdk/skate.lua`, `sdk/GENERAL_API.md`.

**Verification.**
- `move_object_left_stick_never_turns_the_pair`: 16 left-stick directions for 240 ticks each, yaw rate exactly 0
  and travel on a straight line in the stick's direction; right stick turn bounded by `turn_rate`.
- `dragged_prop_follows_a_straight_push`: 180 ticks of a straight push, the prop stays within 5 cm of the push line
  and 0.9 m ahead.
- `turning_carrier_swings_the_held_prop_with_it`: a 90 degree turn leaves the prop in front (bearing 90 +/- 10
  degrees); #15's world-bearing follow kept the prop at its old bearing (0 degrees).
- `object_move_maps_left_stick_and_right_stick_rotation`, `object_move_z_gain_follows_the_stick_angle_curve`
  (skate-core), `carry_move_object_speeds_set_and_reset` (Lua path and reset) and the skate-mods carry validation.
- `cargo test --locked -p skate-game --bin skate3rust`: 482 pass, 1 known failure
  (`pipelines_accept_valid_group_outputs_when_fingerprint_changes`). The asset-backed
  `raw_x_offboard_jump_connects_input_ground_launch_air_and_landing` passes with the stock data (loads the new
  curves; on-foot walking unchanged).
- Not yet checked in game: hold RB next to the cart or a bin, push, pull, side step with the left stick, turn with
  the right stick.

**Open.** Decode state 502 (PhysState_OffBoardPushing) and the MVOBJ clip root motion to replace the engine
speeds; the prop's retail grip point (hands on the handle) and whether heavy DMOs move slower.

## D9 research: the props' retail shader (2026-10-07; ported 2026-10-08, see D9 port below)

**Problem.** Props look flat next to retail: the recomp shows dents, corrosion and grime highlights on dumpsters and
trash bins that ours lack (user's comparison video, 2026-10-07). Our log says why: "40 of 40 world materials use an
unsupported shader family and render as family 1: dynamicobject.alphatest 2, dynamicobject.default 38". The
converter (`tools/asset_pipeline/retail_material.py` `_retail_shader_family`) has no entry for `dynamicobject.*`, so
it stores family 0 and `MaterialTable::build` (`retail_render.rs`) falls back to family 1. The fallback has no
lightmap for props (`lightmap_uv` is zero in `prop_dynamics.rs`) and its normal / detail / specular reads stay off.

**What the prop materials carry [data].** Read from the converted `native-props/*.skate` material definitions (114
unique prop materials over BlackBoxPark, DownTown, Industrial, MaloofMoneyCup, University):
- Bindings `diffuse`, `normal`, `specular`, `detail` (+ `transparent` for `alphatest`), parameter
  `detailNormalUVScale` (3 or 8 mostly; 4, 5, 10, 12, 15 a few).
- 95 of 114 have their own normal map, all 114 their own specular map. The `detail` slot is a tiled detail NORMAL
  map: `default_normal` (flat) on 100, `detail_normal_corroded` on 12, `detail_normal_grain` and
  `detail_normal_pitted` on one each. So the "dents" are mostly the per-object normal maps plus specular, with the
  corroded / pitted detail normals on some objects.

**What the retail shader does [code, shader microcode].** `shaders_final.big` holds `dynamicobject_defaultPS` /
`defaultVS` plus `simplePS` / `simpleVS`, `skateparkPS`, `foregroundPS`, `highlightPS`, `ghostPS`,
`ghostSkateparkPS`, `heatmapPS`, `shadowPS`. `dynamicobject_defaultPS` (2428 bytes, ps_3_0) read with
`tools/claude-skills/living-world/tools/eb_big_extract.py` + `xenos_disasm.py`:
- Samplers `i_diffuse`, `i_normal`, `i_detail`, `i_specular`; constants `g_vLightDir`, `g_vViewPos`,
  `g_envattributes`, `m_params`, `g_CSMSelfBias`, `CSM_Mat_Row0..2`, `WorldShadow_MatRow`; textures
  `shadowAtlasDepth`, `shadowWorld`. VS: `g_matVP`, `i_detailNormalUVScale`, `i_partArray`, `g_FogColour`, `g_FogK1`.
- Same lighting model as our ported `environment.default` (family 1 in `retail_world.wgsl`): the same literals
  (tangent weights 0.58 / 0.62 / 0.39, scale 2.3956, specular colour 2.1 / 1.8 / 1.5, power 10 + 290 x spec.g),
  detail normal added as `normal.xy*2 + detail.xy*2 - 2`, output `sqrt` (gamma 2). Differences: no lightmap (light
  comes from `g_vLightDir` / `g_envattributes` constants instead), and two 4-tap shadow lookups: the cascaded shadow
  map (`CSM_Mat_*`, `shadowAtlasDepth`) and a world shadow map (`WorldShadow_MatRow`, `shadowWorld`, 256 texel
  steps), so a prop in a building's shade is darkened like the world.
- `i_partArray` in the VS: props are drawn as parts with per-part transforms (check how our converter merges them).

**Plan for D9 (retail port, no approximation).**
1. Converter: classify `dynamicobject.default` / `.alphatest` (and the other variants if any map uses them) as their
   own family; keep the bindings and `detailNormalUVScale`.
2. Renderer: a `dynamicobject` branch that reuses family 1's normal / detail / specular code, with lighting from the
   retail constants instead of the lightmap. Open: where the CPU fills `g_envattributes` and `m_params` (find the
   parameter handles by name like fix18 did for the ped colours), and how `shadowWorld` is built (static world shadow
   atlas) versus our layer-28 dynamic shadow map (also see the bridge car shadow item: the world casts nothing there).
3. `highlightPS` is likely the Move Object highlight and `ghostPS` the placement ghost: check against the object
   move state when D6 / placement work starts.
4. Moddability: the family and its parameters stay data (per material), so a mod prop with the same bindings gets
   the same look; mod graphics keep their own path.
5. Tests: a converter test that the 114 materials classify as `dynamicobject`, a shader test like the existing
   `retail_shader_tests.rs` ones; the user compares against the recomp in game.

## D9 port: dynamicobject_defaultPS as family 15 (2026-10-08)

**Problem.** See "D9 research" above: props rendered as family 1 with no lightmap (the absent lightmap reads as
white), so every face was evenly lit and the normal maps only moved the small kd term. Retail lights props with the
sun, so the relief of the per-object normal maps, the corroded / pitted detail normals and the specular maps reads as
dents and grime, and faces turned from the sun drop close to black (recomp video 2026-10-07: the dumpster side in
its own shade is very dark, the lit edge bright).

**Retail evidence.**
- [code, shader microcode] `dynamicobject_defaultPS.fpo` (2428 bytes) and `dynamicobject_defaultVS.vpo` (1140
  bytes) from `shaders_final.big`. Constant table (D3D CTAB at 0x94 in the PS): c0..c2 `CSM_Mat_Row0`, c3
  `CSM_Mat_Row1`, c4 `CSM_Mat_Row2`, c5..c7 `WorldShadow_MatRow`, c8 `g_CSMSelfBias`, c9 `g_vLightDir`, c10
  `g_vViewPos`, c11..c13 `g_envattributes`, c14..c15 `m_params`; samplers s0 `shadowAtlasDepth`, s1 `shadowWorld`,
  s3 `i_detail`, s4 `i_diffuse`, s5 `i_specular`, s6 `i_normal`. VS: c0..c3 `g_matVP`, c4 `g_vViewPos`, c5
  `g_FogK1`, c6 `g_FogColour`, c7 `i_detailNormalUVScale`, c8 onward `i_partArray`.
- [code] Instruction numbers are ALU slots of the PS. 10..14: detail fetched at uv x `detailNormalUVScale` (passed by
  the VS in interpolator 6). 20..24: raw normal = (2 n.xy + 2 detail.xy - 2, 2 n.z - 1); 25..28 normalised (vnd);
  29..34: world normal from the interpolated frame. 35..37: N.L with c9 and the step N.L >= 0. 58..60, 72..81: signs
  of the light in the unperturbed tangent frame times (0.58, 0.62). 86..88, 96: kd = (vnd . (0.58 sx, 0.62 sy,
  0.39)) x 2.3956 (same literals as `environment.default`).
- [code] Shadow: 53..56 four CSM depth taps at texel offsets (+-1, +-1), 66, 70..82 depth compare and bilinear
  weights; 49..52 four `shadowWorld` taps, 65, 67, 69..82 the same for the world map; 84..85
  S = min(step(N.L) x csm, max(world, `g_CSMSelfBias.w`)).
- [code] Light: 57, 64, 68, 88: a counter light, saturate(N . (-L.x, L.y, -L.z)) x `m_params[1].w`; 92..95, 97:
  light = saturate(N.L) x S + counter + `m_params[1].rgb`; colour = kd x light x diffuse^2 (diffuse squared at 34..36).
- [code] Specular: 61..63, 70, 81, 83 view direction in the tangent frame; 89..91 the pseudo light (0.58 sx, 0.62 sy,
  0.39) reflected about vnd; 66, 71, 92..95 power 10 + 290 x specular.g; 96, 98..100 x (2.1, 1.8, 1.5) x S x
  specular.r. Unlike `environment.default` (world-space light (-0.14, 0.5, 0.9) times lightmap.g) this one is
  tangent-space and shadowed.
- [code] 98, 101: the result is scaled by `m_params[0].y` x the VS fog alpha (1 + `g_FogColour.w` x f), 102 adds the
  fog colour, 103..112 the retail tone curve with `g_envattributes[2].x` and `sqrt`, identical to the tail of
  `defaultenvironment_defaultPS` (70..79), so the engine's tone pass covers it as for the world. c11 and c12 are not
  read.
- [data] `m_params` is authored in the attribulator class `material_dynamicobject` (keys `default` and `alphatest`,
  no parent): row 0 (0.4, 1.0, 0, 0), row 1 (0.04, 0.04, 0.04, 0.0). So the ambient is 0.04 and the counter light is
  off in retail.
- [code] There is no `dynamicobject_alphatestPS`: alpha-tested props use the same pixel shader with the alpha test.
- [code] `shadowWorld` is drawn at runtime by `WorldShadow_defaultVS/PS` ("World Shadow generation",
  "DrawWorldShadowCasterInstances", doc 26 car-shadow section). `g_CSMSelfBias` is set by the engine; its value was
  not traced (the name exists only inside the shader objects; `material_envattributes` at 0x821A0390 is the
  attribulator class behind `g_envattributes`).
- Disassembly notes for the next reader: the scalar ops take operand a from swizzle slot 3 and b from slot 0
  (checked on the bilinear lerps 70/71 and 79/80), scalar ops with an empty write mask still set the previous-scalar
  register (27, 83, 89, 99), and fetch source swizzles are absolute, not relative.

**Change.**
- Converter: `_retail_shader_family` classifies `dynamicobject.*` as family 15; `render_parameters.py` exports the
  `material_dynamicobject` `m_params` rows as `dynamicobject.default` / `dynamicobject.alphatest` into
  `private/render-parameters.json` (other rows unchanged, checked against the stock collections).
- Renderer: `Definition::parse` upgrades packages that stored 0 for `dynamicobject.*` (no map re-export needed);
  `supported()` accepts family 15 only when both `m_params` rows are present, otherwise the existing family 1
  fallback and log line stay (no invented constants). The rows land in `WorldParams.water[0..1]` like the other
  families' `m_params`. `retail_world.wgsl` has a `fam==15u` branch with the expressions above; `g_vLightDir` is the
  authored sun direction (`sun_direction`, as for the character); the CSM is the engine's dynamic shadow map read
  with the same caster light as the world receivers (`fetch_directional_shadow`, flags & 5), gated by
  `frame_state.shadow.w`. Normal maps are now sampled for family 15. The world shadow floor (0.05, 0.09, 0.13) is a
  lightmapped-receiver rule and is not applied: dynamicobject has its own `max(world, g_CSMSelfBias.w)` term.
- Other families: only the normal-map sampling condition gained `|| fam == 15u` and the fog multiplier a new
  `fam == 15u` line; no other family's expressions changed.

**Moddability.** The family and both `m_params` rows are data per material shader (setup data in
`private/render-parameters.json`, read at map load); a mod prop with the same shader name and bindings gets the same
look. There is no mod content layer for world / prop materials yet. Entry point to add: a mod-supplied overlay over
the `MaterialTuning` rows (keyed by shader name) and per-material texture overrides applied in `MaterialTable::build`
before page packing, with the stock rows restored when the mod is disabled.

**Files.** `tools/asset_pipeline/retail_material.py`, `tools/asset_pipeline/render_parameters.py`,
`tools/asset_pipeline/test_environment.py`, `crates/skate-game/src/retail_render.rs` (`DYNAMIC_OBJECT_FAMILY`,
parse upgrade, `supported`, tests), `crates/skate-game/src/retail_world.wgsl`.

**Verification.**
- Python: `test_props_classify_as_their_own_family`, `test_dynamicobject_m_params_are_exported_per_variant`; the
  existing environment, map writer and versions tests pass.
- Rust: `dynamic_object_materials_take_their_own_family`, `dynamic_object_needs_the_retail_m_params_rows`,
  `dynamic_object_request_carries_m_params_and_detail_scale`, `dynamic_object_branch_reads_its_data_not_literals`;
  shader validation (`retail_shader_tests.rs`) and the world shadow floor tests pass unchanged.
- To playtest (needs a setup refresh first so `render-parameters.json` has the two rows; the log line "40 of 40 world
  materials use an unsupported shader family" must be gone): the dumpsters and trash bins in DownTown (recomp video
  2026-10-07, 0 to 58 s). Expect sun-facing sides lit with visible dents and corrosion from the normal / detail maps,
  sides away from the sun much darker (ambient 0.04), specular glints on metal, the player's shadow on props.

**Open questions.**
- `shadowWorld` (static world shadow map) has no engine pass yet: props in a building's or bridge's shade are lit as
  if in sun. Building it means a world-geometry depth pass from the sun (retail `WorldShadow_defaultVS/PS`) and the
  `g_CSMSelfBias.w` floor value from the recomp.
- Props do not cast into the dynamic shadow map, so retail's prop self-shadowing is missing.
- The retail CSM is a 4-tap atlas (three cascades in 1/6 atlas columns); ours uses Bevy's cascade lookup, as for the
  world receivers.
- The `transparent` binding of `dynamicobject.alphatest` is not read by the shader; the alpha test uses the diffuse
  alpha (as before). Check the 2 alpha-tested materials in game.
- Changing `retail_material.py` marks the maps setup step stale, so the next refresh also re-exports maps; the load
  time upgrade means that re-export is not required for this change.

## Contact material blocks, held and free (2026-10-08)

**Problem.** The held prop's `{0.03, 0.02}` block was ported as "replace the combined contact friction with 0.03"
(doc 26, Move Object item 3), labelled NOT RETAIL YET because its reader was not found. The free block was the
authored MOBJ material, the upright / tipped choice retail makes was missing, and prop-vs-prop pairs combined the
authored materials only.

**Retail evidence** (TU3 recomp, re-read 2026-10-08; credit: skate3recomp, rexglue / Xenia based static
recompilation, for the readable PPC):
- 82C53EF8, on the tick the commanded bit changes: DMO+4465 bit 0x02 set -> f1 = 0.03 (0x8208EA80), f2 = 0.02
  (0x821E9580); else bits 0x10 and 0x08 both set -> DMO data (DMO+4380 -> +4) +316 / +324; else +320 / +328. Then
  82C550A8.
- 82C550A8 stores f1, f2 and DMO data +272 to the physics component +48 / +52 / +56 and points every body's +80
  (96-byte body records) at that block.
- 82DC3A68, 82DC4158, 82DC4588 (collision-object builders) copy body +80 words 0 / 4 / 8 to the collision object
  +116 / +120 / +124 (+212..+220 relative to the 82DC3A68 base).
- aaCollision 8277A508 calls 82763078(out, CO_a +116, CO_b +116): out+0 = max (static friction), out+4 = max
  (dynamic friction), out+8 = min (restitution). This is skate-core `combine_contact_materials`.
- 82C54B00: DMO+4465 bit 0x08 = (current transform row 1 y > 0.65, 0x820BB0EC) AND (the passed pose's row 1 y >
  0.65). DMO+4465 bit 0x10 comes from DMO data byte +312 bit 0 (ctor 82C51E28, from the parity review).
- The ground side the game already uses for prop contacts is `PhysicsSettings::floor_material` = {0, 0, 1}
  (agCollision 8277C5D8 context 83034F34 / 38 / 3C), so the max / max / min combine keeps the prop's own block.

**Change.** `MoveCommandRules::body_material` builds the body's own block before the combine: commanded = {held
pair (retail 0.03, 0.02), type restitution}; free = the type's free pair, or its upright pair while the type flag is
set and the body's up axis y > `upright_cos` (0.65). `contact_material` is now only
`combine_contact_materials(body block, other side)`; prop-vs-prop pairs combine both bodies' blocks. The path that
replaced the combined friction is deleted. Pure function of the commanded bit, the template and the up axis
(deterministic, no clock).

Moddability: `carry` gains `upright_cos` and per template `material_free_upright`, `upright_pair`, `restitution`
next to `material_held` / `material_free` / `commanded_material`; validated (pairs finite and non-negative,
`upright_cos` in -1..1, restitution finite and non-negative), read back by `world_tuning:carry`, reset on mod disable
(`carry_move_command_rules_set_and_reset`).

In game the effect is small: against the {0, 0, 1} floor the held prop's dynamic friction goes from 0.03 to 0.02
(retail's second float); a held prop touching another prop now gets max(0.03, the other prop's friction) instead of
a flat 0.03.

**NOT RETAIL YET (updated 2026-10-08, see "Per-type DMO data" below).** The per-type values are now retail data.
Still open: the second upright condition (the passed pose in 82C54B00) is not modelled; we test the current pose
only. A prop whose type data is missing (setup older than the type map) keeps the authored MOBJ material.

### Per-type DMO data (2026-10-08)

**Problem.** The restitution, upright flag and free friction pairs of every prop type were stand-ins (authored MOBJ
friction 0.55 for both, restitution 0.05, flag off), marked NOT RETAIL YET above; record+272 per type was off.

**Root cause / retail source.**
- [code] The DMO constructor 82C51E28 looks the type up through the manager at 0x830854B0 (vtable slot 2, key =
  create params +24) and stores result +204 at DMO+4380; DMO+4380 -> +4 is the type's data block. It reads data
  +312 bit 0 into DMO+4465 bit 0x10 and data +237 into DMO+4466 bit 0x80; 82C4A130 reads data +208 as a 64-bit id.
- [data] That block is the layout of the type's vault record of class `livingworld_dynamicobject_characteristics`
  (skaterschema layout size 352). The schema's field offsets match every read: +208 `Priority` (16-byte ref, the
  id 82C4A130 loads), +237 bool, +272 float (restitution, default 0.5), +312 bool (upright flag), +316 / +324
  floats (upright pair, default 0 / 0), +320 / +328 floats (default pair, default 0.8 / 0.6), +308 `LinearDrag`,
  +336 `AngularDrag`.
- [data] Which record a prop type uses: the template record (RX2 EB000D, 160 bytes, in the `worlddmo.big` model
  assets) holds the template id at +104, the vault class's `default` collection id at +112 and the record key at
  +120. All 136 templates on the disc resolve (0 missing); 40 DownTown, 28 Industrial and 42 University templates
  are placed.
- [code] 82C4B960 sets the Move Object record +272 (stored by 82585F58) to 0 when data +312 is clear, else 1 or 2
  (data +236 set / clear): record+272 non-zero is the same flag as the upright pair.

Values (examples, from the disc): `default` / benches / bins: restitution 0.5, pair 0.8 / 0.6, flag off;
`dumpster_wheeled` 0.3, upright pair 0.2 / 0.1, default pair 0.6 / 0.5, flag on; `shopping_cart_wheeled` 0.5,
upright 0.2 / 0.175, default 0.35 / 0.25, flag on; `basketball` 0.85, 0.85 / 0.8; `us_mailbox` 0.2, 0.7 / 0.6;
`dt_ballstainlesssteel` 0.15, 0.8 / 0.3. Only the two wheeled types carry the upright flag.

**Change.**
- Setup (`tools/asset_pipeline/dynamic_props.py`): the DMO catalog keeps each template's record key
  (`characteristics_key`, EB000D +120) and the props export writes `types` {template id -> record key} into
  `private/native-props/<map>.json`. Only the key is exported; the values stay in the installation's stock vault.
- Engine: `skate_world::load_dmo_types` resolves each key in `private/stock/skater-collections.json`
  (`prop_dynamics::dmo_type_blocks`, parents included) at map load and `PropDynamics::set_type_data` attaches
  `DmoType {key, blocks}` to every body by its template id. The body's free block, upright pair, restitution and
  record+272 come from it; the held pair stays the retail constant {0.03, 0.02}. Log `SKATE_PROP_TYPES: <map>
  types=N props_with_type_data=M/T`; HELD_PROP gains `type=<record name>`.
- Mods: `set_tuning('carry', {by_template = {...}})` entries are keyed by the MOBJ template name or by the type's
  record name (e.g. `dt_garbagebin`); the template name entry wins; each field overrides the retail value, unset
  fields keep it; mod disable restores the retail values (the rules reset, the type data stays on the body).
- Multiplayer: the type data is a pure function of the installation's data and the template id (stable keys,
  plain values); no runtime state.

**Files.** `tools/asset_pipeline/dynamic_props.py`, `tools/asset_pipeline/test_dynamic_props.py`,
`crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/skate_world.rs`,
`crates/skate-mods/src/world_tuning.rs` (docs), `sdk/skate.lua` (docs).

**Verification.**
- Pipeline: `test_characteristics_key_is_the_record_at_120`; `test_every_disc_dmo_type_has_vault_type_data`
  (`SKATE3_DISC`, `SKATE3_ASSET_ROOT`): every disc template has a record with all six fields. 8 / 8 pass.
- skate-game: `dmo_type_blocks_read_the_characteristics_record_with_parents`,
  `retail_type_data_is_the_default_and_mods_override_it` (type data drives the upright pair and record+272; a
  record-name mod entry overrides one field, a template-name entry wins, reset restores retail); full bin 579 pass,
  1 known (`setup::pipelines_accept_valid_group_outputs_when_fingerprint_changes`).
- Asset-backed (`--ignored`): `installed_map_props_resolve_retail_type_data` passes on DownTown (40), Industrial
  (28) and University (42) with a type map built from the disc; `downtown_dragged_props_rest_on_the_floor` passes
  with and without the type data (all six props rest at gap 0.000 and sleep).
- Not run: in game. The installed assets need setup group `maps` re-run to get the type map; until then props
  keep the authored material.

**Open questions.**
- `LinearDrag` (+308) / `AngularDrag` (+336): wired, see "Per-type drag" below. Mass +304, maximum velocities
  +292 / +296 and the inertia scale / offset vectors +16 / +32 (same reader, 82C4E568): wired, see "Per-type mass,
  inertia and velocity caps" below. +228 / +232 are not read.
- Data +237 (DMO+4466 bit 0x80) and +236 (record+272 = 1 vs 2) meanings are not decoded; record+272 is used as a
  flag (any non-zero doubles the target speed, 82D45318). +236 also selects one of two stacked vectors in
  82C54BF0 (at 82C54DA4) and is read by 827A3FD8 (a caller outside the DMO block); what it means is still open.

### Per-type drag (2026-10-08)

**Problem.** Prop bodies used the authored MOBJ damping (0.05 linear / 0.15 angular per second in the project
defaults), labelled NOT RETAIL YET, although the type record carries `LinearDrag` and `AngularDrag`.

**Retail mechanism.**
- [code] 82C4E568 (called twice by the DMO physics component builder 82C4DA40, once per body build, guarded by a
  -1 / -2 "set once" word) reads the type block through component +208 and writes the body's `rw::physics::Inertia`
  (component +88 -> +0 -> +16): +16 = 1 / data +304 (inverse mass), +24 = data +292 and +28 = data +296 (maximum
  linear / angular velocity), +32 = data +308 (`LinearDrag`), +36 = data +336 (`AngularDrag`), then the box inertia
  from the AABB half extents x data +16 + data +32 (82C47FC8). The same function copies the default friction pair
  (data +320 / +328) and restitution (+272) into the material block. The values are stored as they are: no
  multiply by the simulation frequency (unlike the deck's `DeckAngularDrag` x 60 and the ragdoll's
  `AngularDrag` x 60).
- [code] None of the DMO functions that load the type block (the 13 readers of DMO+4380, incl. the commanded /
  free switch 82C53EF8 / 82C54BF0 -> 82C550A8) write Inertia +32 / +36; the switch swaps only the friction block
  (a full write scan of the image was not done). Held, thrown and free props use the same drag; a sleeping
  prop is not integrated at all.
- [code] The integrator RigidBody::DynamicUpdate 82AE6590 (already ported, `dynamic_update_packed`) rebuilds the
  velocity from the step displacement: `v = (v dt) x max(frequency - drag, 0)`, so `v *= 1 - drag dt` per fixed
  step, drag in 1/s, clamped so 60 or more stops the body. The DMO simulation steps at the fixed 60 Hz
  (dt 0x3C888889); ours runs the prop step in FixedUpdate with the same `RetailSimulationStep`, so render fps does
  not change it and the per-second unit is retail's own normalisation.
- [data] Values (stock vault, parents included): most types 0 / 0; `lw_props` and its children (bottles, cans,
  bags, phones, papers) 0.1 / 0.35 (purse 0.5, popcan 0.4, taser_gun 0 / 0.35); `garbagebag` 0 / 0.5; barrels and
  spools (`metal_keg`, `oildrum`, `cable_spool`) 0 / 0.25; `pylon` 0 / 0.2, `drum_pylon` 0.4 / 0.35;
  `concrete_pipe` 0.5 / 0.5; `pj_garbagebin` 0.2 / 0.2; several bins 0 / 0.5..0.6; balls 0 / 0.107..0.15.

**Change.** `PropMaterialBlocks` gains `linear_drag` / `angular_drag` (`dmo_type_blocks` reads `LinearDrag` /
`AngularDrag`); `PropDynamics::body_inertia` gives the integrator the body's mass properties with the type drag, a
mod's `carry.by_template` drag over it (field-wise, by MOBJ template name or type record name, reset on mod
disable), else the authored damping (only props without type data). Pure function of type data, mod rules and
template id. Mod fields `linear_drag` / `angular_drag` (finite, >= 0) in skate-mods `CarryMaterialPatch`,
`sdk/skate.lua` and `api.lua`.

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-game/src/skate_world.rs` (asset test), `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** `type_drag_is_the_body_drag_and_mods_override_it` (type drag reaches the Inertia, one integrator
step scales v and w by (60 - drag) / 60, mod override per field, reset), `dmo_type_blocks_read_the_characteristics_record_with_parents`
(drag inherits per field), `carry_move_command_rules_set_and_reset` (mod `angular_drag`). skate-game 580 pass, 1
known failure (setup fingerprint); skate-mods unit tests pass (the asset-gated `skyline_physics` test needs the
Skyline package). `downtown_dragged_props_rest_on_the_floor` (assets) passes; the installed assets have no type map
yet (setup group `maps` not re-run), so it ran with the authored fallback and `installed_map_props_resolve_retail_type_data`
stops at "type map". Not run in game.
- Move Object grab splines (DMO physics assembly definition +136) are a separate structure, not this record; not
  ported.

**Verification.** Tests assert the retail math, not measured output: `commanded_block_switches_with_the_command_and_zero_commands_wake`
(held block {0.03, 0.02, type restitution}; combine against a low side keeps the block, against the 0.8 / 0.6
side takes the max; block restored when commands stop; per template overrides), `free_block_follows_the_upright_test`
(flag off ignores the pose; flag on: upright pair above 0.65, default pair at exactly 0.65 and below; held ignores
the pose; threshold as data; bit-identical repeat), `carry_move_command_rules_set_and_reset`, skate-mods
`valid_patch` cases.

The prop test fixture's static world used a 0.8 / 0.6 / 0 material, not the game's floor. With the combine in
place that made a held prop's friction 0.6 and six Move Object tests failed (push speed 1.83 vs retail math 2.95 m/s,
yaw rate 0.001 vs 1.97 rad/s, a straight push tipping the cube to up_y 0.984, the bin falling 43.7 m). The fixture
world now uses the game's floor material {0, 0, 1}; no assert was changed. Under it two tests fail, with the old code
as well as the new one (old code measured by putting the baseline file back with the same fixture):

| Test | Old code, 0.8 / 0.6 floor | Old code, game floor | New code, game floor |
|---|---|---|---|
| `dragged_props_rest_on_the_floor_after_release` | pass (bench -0.025, bin -0.043, vending -0.078, rail -0.053 m) | FAIL: bench sinks 37.1 m (bin -0.040, vending -0.078, rail -0.053) | FAIL: vending -0.082 m (limit -0.08; bench -0.024, bin -0.045, rail -0.049) |
| `placement_adjust_confirm_and_sleep` | pass | FAIL: placed prop never sleeps | FAIL: placed prop never sleeps |
| `downtown_dragged_props_rest_on_the_floor` (ignored, assets) | FAIL: 5 props sank | FAIL: same 5 props | FAIL: same 5 props |

Causes measured so far (new code, floor variants): the placement failure needs both the prop's own restitution
(0.05) and its own friction (0.55) to survive the combine: floor {0, 0, 1} fails, {0, 0, 0} and {0.8, 0.6, 1}
pass. After release the placed box
settles into a rocking contact cycle at y 0.428 with v.y about -0.8 m/s after every step and kinetic energy about
0.6, above our rest snap's 0.49 (0.7 m/s), so it never snaps or sleeps in 300 steps. The vending machine's 8 cm
overlap is the known solver gap (parity review item 5: our single impulse pass with shared impulses, 40 %
positional correction and the 5 cm per tick cap versus retail's 25-iteration row solver 82AE27D0); it moves between
-0.078 and -0.082 m with the floor material and is not changed by the block. The 37 m bench sink with the old code
is most likely the same solver gap (inference, not traced): a box whose centre passes the one-sided floor face is
pushed further down; it is sensitive to small changes (gone with the 0.02 dynamic friction). Neither is a
small retail fix: the retail answers are the row solver (item 5) and the retail sleep rule (item 4, no rest snap).
The game uses this floor material and the same prop step, so the in-game builds up to 1e61fe0 most likely show the
same behaviour (placed props can keep rocking without sleeping; a dragged prop can sink); not checked in a game
run.

### Per-type mass, inertia and velocity caps (2026-10-08)

**Problem.** Prop bodies took their mass from the authored MOBJ density x box volume, their inertia from that box,
and had no velocity caps (NOT RETAIL YET), although the same reader that sets the type drag fills the whole
`rw::physics::Inertia` from the type record.

**Retail mechanism.**
- [code] 82C4E568 (DMO physics component, run once per body build by 82C4DA40 for single-part assemblies): with
  r30 = Inertia (component +88 -> +0 -> +16) and the type block at component +208:
  - if any lane of the current inverse tensor is +inf (0x82FB4D70) it is first reset to (1, 1, 1) by 82AE6A00;
  - 82C4E448 writes the body's centre-of-mass frame from data +48 (negated translation, see open items);
  - Inertia +16 = 1.0 / data +304 (`fdivs`, inverse mass); +32 / +36 = data +308 / +336 (drag, section above);
    +24 = data +292 (maximum linear velocity), +28 = data +296 (maximum angular velocity); friction pair +320 /
    +328 and restitution +272 into the material block;
  - box: `h = 0.5 (aabb_max - aabb_min) * data[+16] + data[+32]` per lane (`vmaddfp`, fused), the AABB being the
    component's +48 / +64 vectors (filled in 82C4DA40 from the physics assembly definition's bounds, definition
    +128); if not all of h.x, h.y, h.z are > 0 (the 0x830BDB40 permute mask, set to 00 04 08 08 by 82F83450, ANDs
    the three lane results) h = (1000, 1000, 1000) (0x82256FE8);
  - 82C47FC8(Inertia, h): `k = (1/3) / inverse mass` (0x822F87B8 = 1/3), inverse tensor = reciprocal (vrefp plus two
    Newton steps) of `k (h.y^2 + h.z^2, h.x^2 + h.z^2, h.x^2 + h.y^2)` (the 0x822FB890 permute plus `vrlimi`), and
    Inertia +20 = 1 / the smallest inverse moment (ordered compares, as in ComputeMassProperties).
  So h is the half extent of the box (a solid box of half extents h has I = m/3 (h.y^2 + h.z^2)); the type scales
  the AABB by +16 (default 1.2) and adds +32.
- [code] The caps are applied by the integrator RigidBody::DynamicUpdate 82AE6590 after the drag: angular velocity
  `|w|^2 > (Inertia +28)^2` -> `w *= sqrt(cap^2 / |w|^2)` (vrsqrtefp plus two Newton steps), then linear velocity the
  same with Inertia +24; the capped squared speeds also feed the sleep energy. This is already ported
  (`dynamic_update_packed`, `cap`), so wiring the values is enough; the fixed 60 Hz step keeps it fps-independent.
- [data] Schema layout (skaterschema class `livingworld_dynamicobject_characteristics`, layout 352): +16
  `Hash_F4D1C84C36A854AC` and +32 `Hash_D3CDE380DBB3ADC0` (Vector3), +48 `Hash_D266C6ACE12C4C87` (Vector3), +292
  `Hash_4890392C91829954`, +296 `Hash_BAA01E2BA1237455`, +304 `Hash_E5778CDD4576D890` (floats). Default record:
  mass 100, caps 100 / 100, scale 1.2, offset 0. Across the 230 records: mass 100 (71), 300 (17), 200 (15), 150
  (14), 125 (11), 20, 10, 15, 50, 4, 0.5 ...; caps 100 / 100 on 222 records, 10000 on 5, a few 10..150; scale 1.2
  on 224; offset 0 on 218 (a few lift y by 0.2 or 0.5); +48 non-zero on 76 records (y -0.1 .. -1.5).

**Change.** skate-core `mass::dmo_body_inertia(half_extents, DmoBodyData)` is the 82C4E568 / 82C47FC8 fill (pure,
deterministic). `PropMaterialBlocks` gains `mass`, `maximum_linear_velocity`, `maximum_angular_velocity`,
`inertia_scale`, `inertia_offset`, read by `dmo_type_blocks` (parents included). `PropDynamics::body_inertia`
builds the Inertia from the type data and a mod's `carry.by_template` fields over it; fields neither sets keep the
authored values (a mod mass on a prop without type data uses the class default scale 1.2 / offset 0). The result
is stored per body (`refresh_inertia`, with the world inverse inertia) whenever the type data or the mod rules
change, so contacts, carry and the integrator all use the same mass; mod disable restores retail via
`MoveCommandRules::default()`. The box is the body's authored AABB (`authored_half_extents`), so a mod
`collision_box` override does not change the mass properties. Mod fields (skate-mods `CarryMaterialPatch`):
`mass` (> 0), `maximum_linear_velocity` / `maximum_angular_velocity` (>= 0), `inertia_scale` / `inertia_offset`
({x, y, z}, finite); `sdk/skate.lua`, `api.lua` and the read-back list them. Multiplayer: a pure function of the
type record, the mod rules and the authored box, applied on every peer the same way.

**Files.** `crates/skate-core/src/physics/mass.rs`, `crates/skate-game/src/physics/prop_dynamics.rs`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-game/src/skate_world.rs` (asset test),
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.** skate-core `dmo_body_inertia_is_the_scaled_box_and_falls_back_to_1000` (inverse mass,
moments of the scaled box, +20 = the largest moment, the 1000 fallback); skate-game
`type_body_data_sets_mass_inertia_and_caps`, `dmo_type_blocks_read_the_characteristics_record_with_parents` (mass,
caps and both vectors inherit per field), `carry_move_command_rules_set_and_reset`; skate-mods unit tests (102 pass).
skate-game 581 pass, 1 known failure (setup fingerprint); skate-core 783 pass, 2 failures in code this change does not
touch (`predictive_contacts_and_retention_match_full_scan_for_every_primitive`,
`a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`). Asset tests on a copy of the
installed assets whose prop sidecars got the type map from the disc's `worlddmo.big` (the pipeline's own
`template_meshes`; setup group `maps` not re-run): `installed_map_props_resolve_retail_type_data` passes on DownTown
(40 templates), Industrial (28) and University (42), all with mass > 0 and caps > 0;
`downtown_dragged_props_rest_on_the_floor` passes with the type data (benches 100 kg, bin 20 kg, newspaper boxes
15 kg, worst gap -0.001 m; two of the eight ids grab a neighbouring prop, as before). Not run in game.

**Still NOT RETAIL YET.**
- Centre-of-mass offset data +48 (82C4E448 writes it into the body's mass frame): not ported; our body centre is
  the box centre.
- The AABB: retail uses the physics assembly definition's bounds (definition +128); ours is the AABB of the prop's
  collision instance points. Expected to be the same box for single-box props, not checked per template.
- Multi-part assemblies (82C4DA40's part loop) do not go through 82C4E568; we build every prop as one box.
- Props without type data (older setup without the type map) keep the authored density mass and no caps.

## Retail contact solver and sleep rule for props (2026-10-08)

**Problem.** With the game's real floor material {0, 0, 1} (previous section) three prop tests failed: a placed prop
rocked forever and never slept, the dragged vending machine sank 8.2 cm into the street, and six DownTown props sank
15 to 21 cm after a drag. Root cause (measured): our own contact pass (one impulse pass with shared impulses, 40 %
positional correction, 5 mm slop, 5 cm per tick cap) leaves resting jitter and overlap, and our own rest snap and
30-step cool-down hid part of that. Neither exists in retail (parity review items 4 and 5).

**Retail evidence** [code, recomp TU3]:
- Sleep rule. Integrator 82AE6590 (already ported as skate-core `integrate_body_rates` / `dynamic_update_packed`):
  after damping and the speed caps, E = |v|^2 + s * m^-1 * |w|^2; the counter (body +172) resets to 0 when
  E >= island +172, otherwise it counts up only if E did not rise, capped at island +168. Sleep pass 82DC3130
  (re-read): every active body with counter >= sim +204 moves to the sleeping list, its counter set to island +168,
  at most 100 bodies per call. No rest snap anywhere.
- DMO island values: the DMO simulation ctor 82DC2840 (re-read) copies its parameter block +16 -> island +176
  (solver iterations), +32 -> island +172 (sleep energy), +36 -> island +168; the block from 8275DCC8 holds 25,
  1e-5 (0x8219B100) and 2 (parity review item 4, code + data).
- Solver. The contact stage 82DC30A8 runs 82AE27D0 with island +176 iterations (25 for DMOs) over rows built by
  ContactBatchBuild 82AE10C8, whose targets are displacements (predicted separation v dt + separation + a dt^2,
  restitution -v dt e); position error has its own position-only lane, so an overlap is removed without creating
  velocity. This is the same skate-core path the board already uses (`build_contact_jacobian` with the native
  `vrefp` reciprocal, then `solve_constraints`), so props reuse it unchanged.

**Change** (`crates/skate-game/src/physics/prop_dynamics.rs`):
- Every awake prop's contacts go into one shared row solve per step: one row per manifold point (A = the prop,
  B = the static triangle, another prop, or an asleep prop as an immovable support), built with
  `generate_contact` + `build_contact_jacobian` and solved with `solve_constraints` for `iterations` passes; then
  every awake body integrates with its correction buffers (BatchIntegrator order). Pairs of awake props are built
  once, from the lower index. Body order is the iteration order (deterministic).
- Sleep: the integrator's own counter with the DMO values (energy 1e-5, cap 2) and the sleep pass (counter >= 2,
  at most 100 per step). The rest snap and our 0.5 / 30 values are gone from the default path.
- New `PropSolverSettings` (in `PropTuningTable`, resource `PropTuningSettings`): `row_solver` (true),
  `iterations` (25), `sleep_energy` (1e-5), `sleep_frames` (2), `max_sleeps_per_step` (100), `rest_snap` (false).
  Mods set them with `sdk.world.set_tuning('props', {solver = {...}})` (skate-mods `PropSolverPatch`: validated,
  iterations 1..=256, sleep_frames 1..=10000, sleep_energy 0..=100000, max_sleeps_per_step >= 1, unknown keys
  rejected), read back with `world_tuning:props`, reset on mod disable. `row_solver = false` keeps the engine's older
  impulse pass (with its slop / fraction / cap knobs), `rest_snap = true` its snap.

**Verification** (unit tests, no game run; asserts unchanged):

| Test | Before (72bf058) | Step A only (retail sleep, old contact pass) | Step A + B (this change) |
|---|---|---|---|
| `dragged_props_rest_on_the_floor_after_release` | FAIL: vending -0.082 m | FAIL: vending -0.082 m (bench -0.036, bin -0.058, rail -0.049), none asleep | pass: worst gap 0.000 m for bench, bin, vending, rail; all asleep |
| `placement_adjust_confirm_and_sleep` | FAIL: never slept | FAIL: never slept | pass |
| `downtown_dragged_props_rest_on_the_floor` (ignored, assets) | FAIL: 5 props sank | FAIL: 6 props sank 0.146 to 0.212 m, none asleep | pass: 6 props measured, worst gap 0.000 m, all asleep (2 of the 8 ids grab a neighbouring prop and are skipped) |

Step A alone answers the question "does the rocking remain": yes. Without the snap, the old pass's resting jitter
keeps every prop above E = 1e-5, so nothing sleeps (it also broke four sleep tests); the sleep rule needs the row
solver. Behaviour changes in other tests: `depenetration_is_bounded_per_tick` tested a knob of the old pass and now
runs with `row_solver = false`; the new `retail_rows_push_a_deep_box_out_and_it_settles` shows the retail result:
a box 0.4 m deep in the floor is moved out in one step with zero vertical velocity (position-only lane) and sleeps
on step 2. All other prop tests pass unchanged (push speed caps, no tipping on flat ground, curb tip, yaw rate,
Move Object, NPC pushes, layout); `prop_solver_settings_set_validate_and_reset` covers the mod knobs. skate-game
549 pass, 1 known failure (`setup::pipelines_accept...`); skate-mods 102 pass.

**NOT RETAIL YET.** An asleep prop touched by an awake one is an immovable support and wakes only on a hit closing
faster than 1 m/s (ours; retail merges touching bodies into the island). Contacts are not passed through the
agCollision retention buffer the board uses (8277C23C duplicate removal). Skater mesh contacts are solver rows since
2026-10-25 (see "Skater contacts as solver rows"); the box fallback still uses our impulse transfer. Held placement (`carry_to`) still bypasses sleep.

**Open questions.** Whether retail wakes a sleeping DMO on any contact (island merge rule not read); per-row slop or
cap inside 82AE27D0 (none found in the board port, the review's item 5 note); an in-game check that placed props now
sleep and dragged props stay on the street.

**Regression and fix: Move Object carry (2026-10-08).** With this change the two asset carry tests
(`carry_direction_tests`: `move_object_follows_the_left_stick_in_the_skater_frame`, `move_object_with_the_board_hidden`)
failed with "state flipped 2 times while holding RB": the skater grabbed, then dropped on hold tick 19. The prop was
not the cause: it stayed awake (commanded, sleep counter cleared), on the floor, still to 1e-9 m/s. The cause was a
gate in `biped_ground` that applied the Move Object velocity override only when the follow step exceeded 1e-4 m/s.
The row solver holds the resting prop perfectly still, so once the follow point (+416) reached its target the step
was exactly 0. The gate then handed the root back to the walking approach, which walked the skater 0.04 to 0.07 m per
tick into the prop (root x -254.86 to -255.42 m, grip edge at -255.50 m) until the hold rule (82E08EE8 via
`still_holds`) failed. Under the old impulse pass the resting jitter (about 1e-5 m per tick of follow motion) kept
the step above the gate by chance. Evidence: the same test passes with `row_solver = false`, also with the retail
sleep values, and fails with the row solver and the old 0.5 / 30 sleep values. Fix (`crates/skate-game/src/physics/biped_ground.rs`): the override runs on every
held tick, a zero step included. Retail moves the character to the follow point every tick (82D44A10 -> 82BDF268),
so a skater already at the point stays there (82BDF268 itself is still NOT RETAIL YET, see doc 26). No asserts or
constants changed. After the fix: both carry tests and `downtown_dragged_props_rest_on_the_floor` pass, and all
prop tests (ignored included) pass. The only failure under that name filter is `player_voice_properties`, an audio
test that needs private data the setup does not have. skate-game 549 pass + 1 known failure
(`setup::pipelines_accept...`), skate-mods 102 pass.

## Object Dropper and reset moved objects (2026-10-08, research milestone + reset port)

**Problem.** The LB phone menu (session marker, `crates/skate-game/src/session_marker/`) draws the Object Dropper row
at 0.3 opacity and does nothing with it; retail also lets the player put moved objects back. User: "add to the living
world todo to also add the object spawner int he LB menu (it already contains the option, its just not linked to
anything yet.) There is also an option to reset moved objects once you start moving them around in Retail we will
want that."

**Evidence** (TU3 recomp generated code and the TU3 image; reference only, credit skate3recomp / rexglue / Xenia).
[code] = read in the recomp, [data] = image strings / descriptors.

- Cellphone UI `sub_826682B0` (object ctor `sub_82666A20`, vtable 0x82305B70, state at +52): state 2 + FE input 4 =
  open (`cellphone_activate`), state 3 = open menu. In state 3: FE input 8 closes; **FE input 256 = the Object Dropper
  row**: gate `sub_826691D0`, then the fe sound `FF7F0396F338B735`, close the phone (`sub_82668998`), then the
  manager at global 0x830854A8 vfunc +8 and its result's vfunc +20 (enter the dropper). FE 64 cycles the online
  player list (only with >= 2 players, `sub_82669090`); FE 32768 / 16384 set bytes +1669 / +1670 of the object at
  0x830CFDE4 with their own fe sounds (rows not identified). [code] That 256 is the B row comes from the row text
  order and the recomp scripted-runs note (Object Dropper = LB + B), not from the FE code table. [inferred]
- Dropper gate `sub_826691D0`: the game-mode object at 0x830B7AE8 must have mode (+0) 4 or 5, `sub_82511168(+48)`
  true and byte +323 clear. [code] Which modes 4 / 5 are is not decoded.
- The dropper itself is a full editor, not a spawn list: APT movies `objectdropper/objectdropper.swf` and
  `objectdropper/objectquickmenu.swf`, screen modes FreeCam, Catalog, Manipulate, Next / Previous Category,
  SubCategory, Item, Type, SnapObject, MoveOnLockedAxis, RotateObject, GroupSelect, Hide / ShowSelection, Delete,
  RefreshFengShui, Duplicate, FineTune, Size, Info; quick menu Color, Branding, Style, Options (Snap, Collision,
  Invert X / Y, Cursor speed), Copy, Paste, Undo, Redo; natives EnableObjectDropper, DisableObjectDropper,
  Show / HideObjectDropperUI, HideObjectDropperCursor, object dropper input filters, IsObjectDropperEnabled,
  IsInDropper, IsInCatalogMode, IsInQuickMenu, SetCatalogFilter, getCatalogMap, item record `ObjectDropperItemInfo`.
  [data] Its catalogue source, placement, limits and removal are not decoded (next step: the 0x830854A8 manager's
  vfuncs and the catalogue map).
- Per-object phone actions, handler `sub_82666430` (vtable slot next to the cellphone's, context +12 = mode,
  +48 = the DMO id): in mode 1, FE input 1 = **Upright** (`cMsgUprightDMO` 0x905C8249, gate `sub_82666748`),
  FE input 2 = **Reset** (`cMsgResetDMO` 0x31806EF2, gate `sub_826666A8`), 16384 = `cMsgAddDMOToSessionMarker`
  (0x573CEC45), 32768 = `cMsgRemoveDMOFromSessionMarker` (0x9E9C95A1); mode 2, input 1 = `cMsgTeleporterSignUp`.
  Each plays its own fe sound (keys 7CA2E1082BDE9C0A, 36ABD583773962FD, 9CEBB54BBC07C945, 84F5799EF7C02DF1; names
  not recovered). The reset gate reads a per-DMO record word (offline: manager 0x830854A8 vfunc +28 with the id;
  online: table [[0x830CFD94]+260]+0x8720, record id x 96, word -20 == 0), i.e. the option exists only for an object
  whose record says it can be reset. [code]
- PlayerUI's constructor `sub_82897828` subscribes to cMsgResetDMO, cMsgUprightDMO, cMsgAdd / RemoveDMOToSessionMarker,
  cMsgSetSessionMarker, cMsgClearSessionMarker, cMsgTeleport and the ownership messages (cMsgOwnershipRequest /
  Release): the reset is handled next to the session marker, and moved objects can be attached to the marker. [code]
- **The reset itself** [code]: PlayerUI's cMsgResetDMO handler `sub_8289A048` (online: sends net packet type 22
  {player, DMO id, extra}; offline: the DMO manager at [[0x830CFD94]+212]+22416, vtable 0x82323254 (ctor
  `sub_82C48C88`), slot +36 with (id, 0)). Manager reset `sub_82C4B5F0`: gathers the object's reset set
  (`sub_82C4A788`: the object, skipping one whose spawn record has flag 0x02 unless asked; it then walks further DMOs
  from the record's transform, recursion not fully read), tests the set's spawn volumes against the blocker lists
  at +26512 / +26576 (`sub_82E0A8E0`) and, when something blocks, calls a player-side vfunc +124 (undecoded), then
  runs the worker `sub_82C4B780`: per object, look up its spawn record by the object's 64-bit key (manager vfunc +8,
  table vfunc +12); with a record, the table's vfunc +28 puts the object on the record's transform (record+64) in
  one call (no fade or timer in this path); **without a record (an object that was not spawned from the world
  data, e.g. a dropped one) the object's vfunc +12(0) is called, i.e. it is removed.** The phone gate
  `sub_826666A8` -> manager slot +28 (`sub_82C4B000`) runs the same gather and blocker test and offers Reset only
  when nothing blocks; it does **not** test "moved".
- **Upright** [code]: handler `sub_8289A158` (online packet type 23; offline manager slot +40, `sub_82C4B8C0`): sets
  flag 0x40 at +4464 and float +4376 = 0.0 (0x82165A10) when the DMO's slot 28 test returns 0; the righting runs in
  the DMO update. Decoded and ported in "Upright (self-righting)" below.
- The DMO network sync loop `sub_82588380` posts cMsgResetDMO itself for an owned DMO whose sampled height is below a
  constant (0x822272E0), then waits 46 ticks (+76): retail auto-resets objects that fell out of the world. [code]
- Lua natives table at 0x823132F8: ResetMode, SerializeDMOs, DeserializeAndSaveState, RestoreFromSavedState,
  ClearDMOs, Lock / UnlockDMOs, **ResetChallengeDMOs** (`sub_8283BA78`: online it posts one cMsgResetDMO, offline it
  walks the challenge's DMO groups and calls the DMO manager's vfunc +32 per entry id), SetDMOOwnershipByGrabbing,
  Request / ReleaseOwnershipOfAllDMOs. [code]
- APT getters `GetPhoneListCanShowObjectDropperOption`, `GetPhoneListCanResetAllObjectsOption`,
  `GetPhoneListCanShowPhotographerOption`, `GetPhoneListCanShowMusicOption` exist as strings (0x821F472C..) but no
  direct pointer or lis/addi reference was found, so the "Reset All Objects" row's native handler is not located. [data]

**Change (ported).** The per-object reset, deterministic, one authority:
- `PropDynamics` keeps every body's authored spawn pose (our stand-in for retail's spawn record); `spawn_pose(id)`,
  `reset_to_spawn(id)` (back to the spawn pose in one step, at rest, asleep), `moved_ids()` (id order).
- `GamePhysics::reset_prop(id)` (decoded: instant re-place on the spawn transform) is the one authority: rebakes the
  collision at the spawn pose, drops the id from the layout sidecar (`PropCarry::forget_layout`), logs
  `SKATE_PROP_RESET id=..`. Ids are the stable map prop ids (multiplayer-ready payload).
- `GamePhysics::reset_moved_props()`: mod convenience, `reset_prop` for every moved or placed prop. **Ours, NOT
  RETAIL YET** (no retail reset-all code found).
- Mod entry points: `sdk.world.reset_prop(id)` (`world_reset_prop`) and `sdk.world.reset_moved_props()`
  (`world_reset_moved_props`). A reset is a one-shot world action that leaves nothing behind, so there is nothing to
  clean up on mod disable.

**NOT RETAIL YET.** The Object Dropper editor (catalogue, freecam, placement, snap, group select, delete, quick menu)
is not ported: the row stays at 0.3 and LB + B is not wired, because pressing it in retail closes the phone and
enters that editor. Decoded vs ours for the reset: the instant
re-place on the spawn transform is decoded; ours are the authored map pose as the spawn record, refusing the held
object (retail unknown), no blocker test (retail refuses / acts when the spawn spot is blocked), no reset set
gathering (retail may reset further DMOs with the object), no removal of record-less objects (we have none: no
dropper). "Moved" (pose differs from spawn by more than 1e-4) only feeds the reset-all convenience. No phone row for
the per-object actions (the phone context's object source and row art are missing); Upright and Add / Remove to
session marker are not ported (Upright is, see below).

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/physics.rs`,
`crates/skate-game/src/physics/prop_carry.rs`, `crates/skate-game/src/modding/mod.rs`, `crates/skate-mods/src/vm.rs`,
`crates/skate-mods/src/api.lua`. Research helpers: `.local/research/object-dropper/` (message-name lookup,
lookup8 name guesser).

**Verification.** `reset_to_spawn_returns_moved_body` (moved list, pose, rest, sleep, unknown id);
`world_tuning_commands_deserialize_and_validate` extended with both reset commands and their Lua wrappers.

**Open questions.** The dropper catalogue source and limits (manager 0x830854A8, `getCatalogMap`); what game modes
4 / 5 are; where the per-object phone context comes from (nearest or held DMO); retail's reset transition; the
"Reset All Objects" row handler; the fe sound names.

## Upright (self-righting) (2026-10-08)

**Problem.** The phone's per-object Upright (cMsgUprightDMO) was decoded only as far as the flag it sets; the
righting itself, its limits and its effect on Move Object commands were unknown, and the engine had no Upright.

**Evidence** [code] (TU3 recomp generated code and image constants; reference only, credit skate3recomp / rexglue /
Xenia):
- Start: DMO manager slot +40 `sub_82C4B8C0` resolves the object to its DMO (slot 6) and, when the DMO's slot 28
  (`sub_82C564D8`: physics component -> body, returns body field +28) is 0, sets DMO+4464 |= 0x40 and timer
  DMO+4376 = 0.0. The phone gate `sub_82666748` (offline: manager slot +32 `sub_82C4B578`) offers Upright on the same
  slot 28 test; online it reads the DMO record word -12. It does not test the tilt.
- Window: DMO update `sub_82C56780`, while 0x40: timer += 1/60 (0x820849C8); timer > 2.0 (0x82060C50) clears 0x40
  (that update still runs). Then, with a dynamic body, `sub_82C573D0(body, pose, out)`.
- Righting `sub_82C573D0`: angle between the pose's up row and world up (0, 1, 0) (0x82139A20, `sub_8296EBB0`), in
  degrees (x 57.2958, 0x82084620). Under 10 deg (0x821963E4) it returns 0 and the update clears 0x40 and the timer.
  Otherwise: axis = normalize(up x world up); if that is degenerate (every component <= 1.19e-7, 0x820BA9C0) or the
  tilt is above 120 deg (0x82256FE0), the axis is the body's own X (0x82139A10) when A.x > A.z, else its Z
  (0x82139A30), A = the vector at the physics state block +72. Capped tilt = min(tilt, 70 deg) (**the constant 70**
  at 0x820BB1E0 x 0.0174533 at 0x8206D110: a 70 degree cap). Gain = lerp(3 (0x82063B08), 5 (0x821F1790), t),
  t = clamp(|A| - 0.1 (0x820641A8) - 1.0 (0x8231A844), 0, 1). Target spin = axis x gain x max(capped - 5 deg
  (0x820BB1D8), 0). Command = (target - w_axis - 0.1 w_perp) x 60 (0x821FF080), w = body angular velocity (state
  block +76, +48) split along / across the axis.
- The command goes through the DMO's own angular slot 37 `sub_82C52EE0` (skipped while the slot 27 lock is set or the
  body has no dynamics) -> `sub_82D9CCF0`: wake (82ADF7B8), write the angular accumulator +160; DMO+4465 |= 0x02
  (commanded, so the commanded material block applies).
- Move Object: the yaw sink `sub_82C52E68` refuses the angular command while 0x40 is set; the linear sink
  `sub_82C52DC0` is not gated.

**Change.**
- `PropUprightSettings` (props tuning domain, `sdk.world.set_tuning('props', {upright = {...}})`): window_seconds 2.0,
  tick_seconds 1/60, stop_angle_deg 10, max_angle_deg 70, dead_band_deg 5, gain_min 3, gain_max 5,
  gain_blend_start 1.1, off_axis_spin 0.1, command_rate 60, fallback_angle_deg 120, block_yaw true; validated (finite,
  >= 0, window and tick > 0), reset on mod disable with the rest of the domain.
- `upright_command(basis, w, a, settings)`: the 82C573D0 arithmetic as a pure function.
- `PropDynamics::upright(id)` opens the window (per-body plain-data timer, `Option<f32>`); the prop step runs the
  82C56780 pass first (before the skater pushes and the retail row solve), for sleeping bodies too: timer, timeout,
  stop under 10 deg, else wake, mark commanded, replace the angular accumulator and integrate `w += C dt` like the
  Move Object yaw command. `apply_move_command` drops the yaw part while the window is open.
- `GamePhysics::upright_prop(id)` is the one authority (logs `SKATE_PROP_UPRIGHT id=..`); mod entry
  `sdk.world.upright_prop(id)` (`world_upright_prop`). One-shot action; the window ends by itself within 2 s.

**NOT RETAIL YET.** The slot 28 gate (body field +28) is not decoded: ours refuses only an unknown id or a body without
dynamics. The vector A at state block +72 is not identified: ours uses the body-space inverse inertia diagonal (it
picks the fallback axis and the gain blend). The angle helper `sub_8296EBB0` is read as acos of the normalised dot.
The phone row is not wired (same as Reset).

**Files.** `crates/skate-game/src/physics/prop_dynamics.rs`, `crates/skate-game/src/physics.rs`,
`crates/skate-game/src/modding/mod.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`, `crates/skate-mods/src/vm.rs`, `crates/skate-mods/src/api.lua`.

**Verification.** `upright_command_matches_retail_constants` (10 deg stop, 70 deg cap, 5 deg dead band, gains 3 / 5,
x 60, 0.1 off-axis, X / Z fallback above 120 deg); `upright_rights_a_tipped_box_within_the_window` (a cube on its
side rights and the window closes under 10 deg before 2 s, still upright 4 s later);
`upright_window_times_out_at_two_seconds` (gains 0: closes after the retail number of 1/60 updates);
`upright_blocks_move_object_yaw_during_the_window` (yaw refused, linear applied, `block_yaw` knob, yaw back after
the window); `prop_upright_settings_set_validate_and_reset`; `world_tuning_commands_deserialize_and_validate`
extended. Existing prop tests unchanged.

**Open questions.** Body field +28 (Upright gate) and the state block +72 vector; whether the integrator scales the
+160 accumulator by inverse inertia (our port treats it as an angular acceleration, as for the Move Object yaw).

## Props created mid-game (2026-10-10)

**Problem.** Prop physics only had bodies for the props the map places at load (`PropDynamics::new` over the MOBJ
records, one fixed collision `BoardWorld` from `build_prop_layer`). Retail creates DMOs while the game runs: a ped's
hand prop (vending can, newspaper) becomes a physics DMO when it is thrown at a bin or dropped, and is removed
later (doc 26g "Ped hand props"; `ThrowHandPropAtTrashBin`, `DropHandProp`). The throw port needs a body for the can.

**Change** (engine path only; the throw itself is the next step):
- `skate-core` `BoardWorld::append_triangles`: appends triangles after the existing ones; existing indices, order
  and query meshes stay as they were, the new range gets its own 64-triangle query meshes (the portable world's
  grouping), the packed surface list grows with it.
- `PropCollisionLayer::empty` / `add_instance` / `retire_instance`: an instance from template-space triangles at a
  pose (edge features from its own triangles through `welded_edge_features`, the welding and edge pairing moved out
  of `portable_world` unchanged); retiring parks the triangles at `HELD_PARK` and keeps the slot, the next instance
  with the same triangle count reuses it, so repeated cans do not grow the world.
- `PropDynamics::build_body` (the body construction moved out of `new` unchanged), `spawn_body` (awake, with the
  spec's linear and angular velocity, the prop type's tuning and type data like a map prop), `remove_body`,
  `next_runtime_id` (ids from `RUNTIME_PROP_ID_BASE` = `0x2000_0000`, below the grab-scene tags; deterministic in
  creation order so a host can hand them to peers), `copy_spec`. Runtime bodies are never "moved" (no authored
  pose: no reset, no layout entry); map props are never removed by this path.
- `GamePhysics::spawn_runtime_prop` / `remove_runtime_prop`: the single authority; creates the layer and dynamics
  on maps without placed props. A render entity with `PropInstance { id }` follows the body
  (`sync_prop_transforms`). Logs `SKATE_PROP_SPAWN id=.. template=.. instance=..` / `SKATE_PROP_REMOVE id=..`.
- Mod entry: `sdk.world.spawn_prop(key, from, {position, yaw, velocity, spin})` (`world_spawn_prop`) copies map
  prop `from` (model, collision, physics block, type data) and throws it; `sdk.world.remove_prop(key)`
  (`world_remove_prop`); a mod's props are removed on disable (`modding/world_props.rs`, 64 per mod, 256 total).

**Retail parity.** Retail's released hand prop is an ordinary physics DMO, so it gets the same box body, contact
solver, sleep rule, type data and skater pushes as a map prop. Ours: the id range (retail addresses DMOs by
manager slot) and the slot reuse. Not decoded yet: the released DMO's physics block and type record for each hand
prop template, its removal timing.

**Verification.** `skate-core` `appended_triangles_are_queried_and_existing_ones_kept`; `skate-game`
`runtime_prop_is_thrown_lands_and_is_removed` (empty map: flies with its velocity, lands, sleeps, collides, is
removed and the slot reused) and `runtime_spawn_keeps_map_props` (map prop body and triangles unchanged, map props
refuse removal); `skate-mods` command validation and the Lua calls. The map load path is the same code moved into
`build_body` / `welded_edge_features`; all existing prop tests pass. Not checked in game yet.

## DMO streaming: the census core (2026-10-10, D3 step 1)

**Problem.** Every placed prop exists from map load to the end of the session; retail streams DMOs with the living
world census: they spawn near the player and go beyond the cull radius, within a 49-object pool.

**Evidence** (research b97; main-checked: the cull's horizontal distance (`826BAD98`: the height lane is swapped out by
the perm at `0x822FB890` before `vmsum3fp`), the pool test `subfic 49`, the five weights and the range records in the
export, the priority records):
- Census pass `sub_826B7980` [code]: circle from the `dynamicobjects` range record (`livingworld_census_ranges`:
  0 / 90 / 100 m; `skatepark_dynamicobjects` 150 / 200, `extended_challenge_dynamicobjects` 100 / 130 [data]); retail
  DMO records have no speed keys and no forward offset.
- Cull `sub_826BAD98` [code]: horizontal distance beyond the cull radius, cullable objects only, the whole linked group
  (`sub_82C52600`); then at most one eviction per pass while the pool holds 49 or more.
- Spawn `sub_826B9D58` [code]: placements within the outer radius (max 256), touching neighbours within 20 m join so
  clusters come together, 49 spawns per pass (100 in fill mode); with the pool full a placement must outscore the
  eviction front, which then goes.
- Score `sub_82C4A130` / `sub_82C55160` [code]: `s1 = 250 x priority x k_flag` (keepalways or flag 0x10: 2^31; a LIVE
  object always 2^31, see "Live props evict farthest first"),
  `s2 = 300 x -n x k_flag`, `s3 = 450 x -distance / (k_flag x k_view^2)`, compared as `(s1 + s2 + s3) x k_view`;
  `k_view` 2.5 in front of the camera, `k_flag` 2.0 with flag 0x10 [data `livingworld.dynamicobjects`]. Type
  priorities (`livingworld_dynamicobject_priority`, referenced by each characteristics record's `Priority`): of 230
  types, 98 `mediumpriority` (200), 84 `highpriority` (450), 47 `lowpriority` (1), 1 `keepalways` [data].

**Change.** skate-core `living_world/dmo.rs`: `DmoStreamSettings` (range, weights, cap 49, budgets 49 / 100, group
radius 20; retail defaults, data-driven), `DmoPlacement`, `DmoView`, `score` / `score_value`, `DmoStream::step`
(cull with groups, one eviction, spawn with group expansion and evict-if-better; held ids exempt), `set_position`
(keep where left). Decisions are serialisable (`Spawn` / `Cull` / `Evict` with the map prop id) for a host.

**NOT RETAIL YET / open.** Wired into the game in step 2 below. The score's count `n` (`sub_82C503F0`) is 1; the overlap blocker is not modelled; touching is bounding spheres (the
retail test's factor 0.5 is not decoded); the eviction queue order is the live score; the safety layer
(`livingworld_dmo_safety`) is not found; multiple observers are ours.

**Verification.** skate-core `placements_spawn_inside_90_and_cull_beyond_100_horizontally`,
`touching_props_spawn_and_cull_as_a_group`, `the_score_follows_the_retail_weights`,
`the_pool_cap_evicts_the_lowest_score_once_per_pass`, `the_spawn_budget_is_49_per_pass_and_100_when_filling`,
`streaming_is_deterministic`.

## DMO streaming in the game (2026-10-10, D3 step 2)

**Change.** On by default, as retail (`SKATE_DMO_STREAM=0` turns it off; `LivingWorldSettings.dmo_stream`):
`living_world::dmo_stream::stream_dmos` runs the census core on the map's placed props (census slot 2 of 4, the first pass
after a map load fills from nothing with budget 100, every prop it does not pick goes dormant). A culled or evicted prop
goes **dormant** (`GamePhysics::stream_prop` -> `PropDynamics::set_dormant`): body asleep and still, collision parked at
`HELD_PARK`, model hidden (`sync_prop_transforms`), out of the obstacle list, grab search, upright, box-vs-box contact
and grab scene; a spawn brings it back asleep at its **authored pose** (`respawn_authored`, retail). Ped plugins skip
dormant props and a ped using one lets go (plugin id low 32 bits = DMO body id). The held prop and props in the
player's saved placement layouts (#15, not retail) are never streamed. Range and weights come from the export
(`tables.json`), type priorities from each characteristics record's `Priority` RefSpec (`dmo_type_priority`).
Logs: `DMO_STREAM fill=.. spawned=.. culled=.. evicted=.. live=..`, `DMO_STREAM_CHANGE` (debug).

**Evidence, the pose** (research b98): retail spawns a culled DMO from its placement record +0, and nothing writes the live
pose there (the only writer after creation, `sub_82C50EE8`, writes authored matrices); the live pose goes into a
separate moved-pose map (table+0x9090, 512 entries, `sub_82C50950` per tick for records with flag 0x40) that only
rebuilds a record at a chunk reload [code; end-to-end medium]. So a knocked-over prop is back in place when the player
returns. The safety layer is census painter layer 14, read by `sub_82C55A50` (max of the `livingworld_dynamicobject_safety`
values at the centre and four points, any forbidden = 0); it never resets an object, it only stops a moved pose from
being remembered and weighs the moved-pose priority [code / data].

**NOT RETAIL YET / open.** The moved-pose map and chunk reloads are not modelled (so "keep where left" never happens);
the skatepark / extended challenge range records (150 / 200, 100 / 130) are not selected (how retail picks them is
open), all maps use `dynamicobjects`; retail's 49 pool also counts hand props (ours: separate pools); the safety layer
is not exported. Not seen in game yet.

**Verification.** skate-game data-gated `dmo_streaming_keeps_49_live_props_round_the_spawn` (DownTown: 786 placed props,
29 live within 90 m of the spawn after the fill, 757 dormant and out of the obstacle list; 300 m away 29 culled and 8
spawned; a prop moved 2 m comes back at its authored pose).

## Census camera and hand prop release from the tick (2026-10-22)

- **Problem:** two fixed-step systems read poses drawn in `Update` (interpolated with the overstep, propagated in
  `PostUpdate`), so their result depended on the frame sequence (research b531 section 5.3):
  `dmo_stream::stream_dmos` took the census camera from any `Camera3d`'s `GlobalTransform`;
  `ped_hand_props::sync_hand_props` took the throw release test position and the released body's start pose from the
  held prop's `GlobalTransform` (the hand as drawn, one frame behind when a frame runs two steps).
- **Root cause:** retail runs the census, the ped hands and the camera in the same 30 Hz frame, so both read that
  tick's state. Ours split simulation (fixed) and drawing (frame).
- **Change:** the census view uses the tick's camera, `CameraRuntime::presentation_frame()` (position and `At`, the
  same frame `presentation::capture` stores; manual cam applied). The hand prop pose is the ped's tick `Transform`
  times the hand bone from `ped_globals(.., ahead = 0, ..)` times the record offset, the pose `present_ped_pose` draws
  at zero overstep. The drawn model is used only for the collision shape relative to its own root, which does not
  depend on the pose.
- **Files:** `crates/skate-game/src/living_world/dmo_stream.rs`, `ped_hand_props.rs`, `peds.rs` (`PedPuppet.followers`
  visible to the crate).
- **Effect:** a census pass or a throw differs from before by at most one frame of camera or hand motion; the result
  is now the same for any frame rate (the prerequisite for the fixed-step identity test, b531 section 6).
- **Verification:** `cargo test -p skate-game` 906 passed, 0 failed (assets installed, dc2813f3); not seen in game yet.

## DMO streaming: one collision rebuild per pass (2026-10-13)

- **Problem:** the first streaming pass on DownTown took 412 ms (`FRAME_HITCH ... dmo_stream:412.4`; research
  `.local/research/peds/b111-dmo-eviction.md`). Every streamed prop rebaked its collision with
  `BoardWorld::replace_triangles`, which rebuilds the whole query index; the fill pass streams ~790 props.
- **Change** (research `.local/research/b114-prop-stream-batching.md`, main checked the index build):
  `BoardWorld::replace_triangles_batch` applies all edits in order and rebuilds the mesh bounds and the index once;
  `replace_triangles` is a batch of one. `PropCollisionLayer::rebake_many`, `GamePhysics::stream_props` and
  `stream_dmos` collect a whole pass (and the streaming-off restore) into one call. The streaming decisions and their
  order are unchanged (they match retail; b111).
- **Why identical:** `QueryIndex::new` depends only on the final mesh bounds (build order restarts at 0..n, ties by
  index, query results sorted); a rebake never moves a triangle range, and a later edit of the same prop wins in both
  versions. Nothing reads prop collision between the props of a pass. The one difference: a batch checks every edit
  first, so a failure changes nothing (the single call could leave a stale index after copying triangles).
- **Verification:** skate-core `board_world::broadphase_tests::batched_replacements_match_one_call_per_edit` (63
  edits over meshes that straddle props, repeated and unchanged props, far parks: triangles, bounds, meshes and index
  identical to one call per edit and to a fresh build, same line query hits) and
  `a_failed_batch_changes_nothing_and_an_empty_batch_is_a_no_op`. Muted DownTown run: first pass 412 ms -> 35.5 ms.
- Not changed (same API possible later, each with its own proof): the per-tick rebake of moving props
  (`prop_dynamics.rs`) and the layout load.

## DMO streaming: a moved prop is kept, not evicted (2026-10-17)

**Problem.** User (laptop session game-20261017-143646): "props reset their position WAY TOO EASILY. in retail they
only reset if you get far away or hit the reset button". The log: 21,037 evictions against 353 distance culls in 13
minutes of DownTown; every eviction respawns the prop at its authored pose, so a knocked prop snapped back.

**Retail [code] (research b184, `.local/research/b184-dmo-eviction-queue.md`).** The census evicts the eviction
queue's front once per tick while the pool is full (`sub_826BAD98`), as ours does. The queue is a min-heap on the live
score (`sub_82C49C80` insert from attach `sub_82C52718`, re-scored every manager tick `sub_82C49DB0`), and the live
score `sub_82C55160` scores an object as kept when its own state says so (`sub_82C485C0`: DMO+0x1170 bit 0x20;
`sub_82C56AE8`: state word DMO+0x112C == 1; or record flag 0x10), so a disturbed prop never becomes the front. Only
untouched props go, and they come back where they already were.

**Change.** `DmoStream::step_kept` takes the live objects' keep state and scores a kept live object like a keep
placement (s1 = 2^31, the keep weight) when picking the front; `step` passes none. `dmo_stream` keeps every prop
moved off its authored spawn pose (`PropDynamics::moved_ids`). NOT RETAIL YET: "moved" is our reading of retail's
state 1 until a trace settles it; the overlap blocker (`sub_82C49EB0`) and the current-position score are not ported.

**Verification.** skate-core `a_kept_live_object_is_never_evicted_while_others_can_go` and the 7 other dmo tests;
muted DownTown run: no errors (29 live at the spawn, under the cap, so no evictions while standing). Not play-tested.

## What wakes a resting prop: the bail kick (research, 2026-10-17)

**Question.** Retail's DMO contact callback `sub_82C54618` calls the velocity kick `sub_82C530F0` only for some
touchers; which, and is a resting prop static to the skater otherwise (the saved stage A patch
`.local/patches/stage-a-resting-props-static.patch` assumed so)?

**Retail [code] (`sub_82C54618`, Ghidra + generated code).** The touching collidable is `[event+0x4C]`, its owner
`[collidable+0x20]`. The owner is looked up for the interface whose type id is `sub_82586478` (returns the name
"IActor", 0x823000F0) through `sub_82965630`, a search of the owner's sorted (type, interface) pairs. Only an IActor
toucher can kick, and only when the collidable's kind `[[collidable+0x18]+0x18]` is 3 or 5 (or 4 with byte
`[[actor->vfn14()+0x48]+0x137]` set) and byte `[[actor->vfn14()+0x1C]+0x4C]` is set; other touchers go into the
prop's contact list instead.

**Trace (laptop session recomp-all-20261017-175036, 9.2 min).** 1910 PROPHIT, 465 of them the player's board or
body against a prop, and only 17 kicked: one burst at 271717 to 271754 ms, 50 ms after a ped collision (PEDCOLL ped
47BA3950 at 271667 ms). The hook's v1 kind / flag fields read the owner instead of the collidable and the IActor call,
so they say nothing about the gate; v2 (recomp research hooks, PROPHIT `actor` / `actor_vfn14`) logs the real inputs.

**User (2026-10-17).** About the moment at 271.7 s: "Hit and then bailed as a result". About rolling into props:
"depends on the object. large dumpsters move less, smaller props move easier. Vending machines are heavy."

**Reading.** Rolling into a resting prop moves it by mass (ordinary contact physics, as ours already does through
the reduced mass of the pair in `push_from_skater`; since 2026-10-25 a contact row in the shared solve, see "Skater contacts as solver rows"); the kick is an extra impulse while the skater bails. So the stage
A patch (resting prop static to the skater) is wrong and stays unapplied.

**The "kick" is a clamp [code] (research b206, `.local/research/b206-prop-bail-kick.md`, read from the generated
code because Ghidra garbles the multiply).** `sub_82C530F0` does not add an impulse: with w = the motion record's
+0x7C (read as inverse mass, not decoded) and d = the DMO's vtable slot 6 vector (`82C48230`, a velocity; the
callback records a contact only when |d| >= 0.25): s = 5w and the spin is clamped to 15w when the mass (1/w) is
under 200, else s = 0 and the spin is zeroed; n = d / |d|, mag = 0.5 * (max(s, |d|) + s); the linear velocity is
pulled into the ball of radius mag around n * mag. Both setters wake the body. [trace] All 17 calls hit one prop with
mag = 5.0. So a bail contact caps a prop's speed (and spin) rather than throwing it; the contact solve does the
pushing. Constants [data]: 5.0 (0x821F1790), 15.0, 200.0, 0.5, 0.25.

**Open:** which actor state the gate byte `[[IActor vfn14()+0x1C]+0x4C]` is (bail / ragdoll from the timing and the
user's account, not decoded; the PROPHIT v2 hook logs the IActor and its vfn14 for one bail-into-prop session);
collidable kinds 3 / 4 / 5 (b206: maybe our skeleton part groups; we drop group 4 from world volumes,
`physics/skeleton_colliders.rs:31-34`); then the port: after `push_from_skater` (`prop_dynamics.rs` actor loop),
when the pushing actor bails, apply the clamp and wake the prop, values data-driven with these defaults.

## The bail clamp, ported (2026-10-17)

- **Retail [code] (b206, b209, b210):** w is the body's inverse mass (the motion record AddForce `82D9CA30` also
  scales by; written from 1 / the type mass, `82C4E568`); d is the prop's own velocity as its transform tracker
  measures it (`[dmo+0xA4]+0x90` = last update's displacement / dt, `82E0C578`), about 0 for a resting prop. So for a
  resting prop the cap is 5 / mass m/s (20 kg: 0.25 m/s) and 15 / mass rad/s, and from 200 kg on the prop is stopped.
  Main checked in the callback decompile that the |d| >= 0.25 test guards only the non-actor contact record: the actor
  branch calls `82C530F0` and returns before it.
- **Change:** `prop_dynamics::bail_clamp` (pure, the formula above) runs once per prop per tick when a bailing pusher's
  volume touched it this tick, with d = the prop's velocity before this tick's pushes, then wakes it.
  `PropDynamics::set_bailing` takes the bailing pushers; `GamePhysics::step_props` passes the local skater while
  `board_wiping_out` is set (our gate: retail's actor byte is not decoded). Values in `PropSolverSettings`
  (`bail_clamp`, `bail_linear_factor` 5, `bail_angular_factor` 15, `bail_heavy_mass` 200, `bail_blend` 0.5), mod keys
  in `set_tuning('props', {solver = {...}})` with validation, read back by `get_tuning`, reset on mod disable.
- **Tests:** `bail_clamp_caps_light_props_and_stops_heavy_ones`, `a_bailing_pusher_does_not_throw_a_bin`; prop and
  tuning tests pass (82). Not play-tested.
- **NOT RETAIL YET:** NPC skaters do not report a bail to the prop step (replay NPCs have no bail state, simulated
  NPCs do not push props); the gate is our wipeout state, not retail's byte; d is the solver velocity, not the
  tracker's displacement.

## Open questions

- Retail parity: every DMO is dynamic and box-approximated; retail drives DMOs through `LWDynamicObjectMan` with
  per-type characteristics (230 `livingworld_dynamicobject_characteristics` records), priorities, census spawn /
  cull rings and safety areas. Planned as milestones D0 onward (dmo-plan).
- Grind probes see only the static world (props not grindable yet), multiplayer is host-local (#15 known
  limitations); memory on DownTown is heavy (#15 notes).
- Board stuck inside a prop (bench near the Aletown spawn, frame drop): #15's push nudged a prop every tick while a
  skater volume sat inside its render-AABB box, so a bench never slept and rebaked (and rebuilt the prop layer's
  query index) every tick. Fixed 2026-10-05 with a bounded, data-driven push and depenetration (`PropTuning`,
  per prop type overrides, resource `PropTuningSettings`); see doc 26 "Board stuck inside a prop". Still open: the
  render-AABB box is solid under seats where the mesh is open; retail's collision for these DMOs is not recovered.
- Grab button: A (#15's choice) was the retail sprint button and grabbed props while running; it is now a held RB
  (GrabWorld, the gate of retail 82D324B0), release drops. Placement (B) is still #15's choice. See doc 26.
- Move Object speeds (push / pull / side / turn) are engine values until retail state 502 is decoded; see
  "Grabbing spins the player" above.

## Code reference: prop_dynamics.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/prop_dynamics.rs`; each comment there links its section here.

### Prop body model

Dynamic rigid bodies for DMO prop instances (Phase 2a).  Every spawned prop gets a box body built on the TU3 rigid-body integrator (`integrate_body_rates`): gravity and cool-down/sleep come from the retail simulation step; the mass properties are the retail rounded-box finalize path (`primitive_mass_properties`) with the instance's scaled template AABB. Density is the authored MOBJ per-object value (`ObjectPhysics`, schema 3+); friction, restitution and linear / angular drag come from the prop type's retail DMO data ([`PropDynamics::set_type_data`]), with the authored MOBJ values (project defaults friction 0.55, restitution 0.05, damping 0.05/0.15) only for props whose type data is missing.  Narrowphase uses the recovered GP pair query (`primitive_pair_contacts`): box vs static-world triangles, box vs box for other props, and box vs the skater's board/skeleton volumes for pushes. Contact response is the retail row solver the board uses (ContactBatchBuild 82AE10C8 rows, 25 iterations of 82AE27D0, `PropSolverSettings`); the engine's older impulse pass stays selectable as a mod option. Sleep is the retail counter (integrator 82AE6590, sleep pass 82DC3130) with the DMO island values.  Props start asleep and cost one AABB test per skater volume per tick. A skater contact or a moving prop wakes them. Moved instances re-bake their triangle range in the prop collision layer so skater queries stay exact. The held (carried) prop is exempt from both skater pushes and the rebake: while carried it is velocity-driven and its layer triangles are parked far below the world so they cannot push the carrier.

### Prop tuning

One value set is the default for all props; `PropTuningTable` can replace it per prop type (the MOBJ template name, the stable id a mod uses). The defaults are #15's constants plus the caps from doc 26 "Board stuck inside a prop"; no retail values for DMO contact response are recovered yet.

### Upright window

Retail DMO self-righting ("Upright", doc 27 "Upright"). The phone's per-object Upright posts cMsgUprightDMO; the DMO manager slot +40 82C4B8C0 sets DMO+4464 bit 0x40 and zeroes the timer DMO+4376. While the bit is set the DMO update 82C56780 adds `tick_seconds` to the timer and clears the bit once it exceeds `window_seconds`, then (same update) 82C573D0 measures the angle between the body's up row and world up: below `stop_angle_deg` the bit and timer are cleared; otherwise it builds an angular command that the DMO's own angular slot 37 (82C52EE0 -> 82D9CCF0) writes into the body's angular accumulator (+160), waking the body and marking it commanded. External yaw commands (82C52E68) are refused while the bit is set; the linear Move Object command (82C52DC0) is not gated. The defaults are the retail constants; every field is a mod knob (`sdk.world.set_tuning('props', {upright = {...}})`), reset on mod disable.

### DMO island settings

Island settings of the DMO simulation. Retail 8275DCC8 passes a 52-byte block to the simulation ctor 82DC2840, which copies +16 -> island +176 (solver iterations), +32 -> island +172 (sleep energy) and +36 -> island +168 (sleep counter cap; also the sleep pass threshold sim +204). The defaults are those retail values; every field is a mod knob (`sdk.world.set_tuning('props', {solver = {...}})`), reset on mod disable.

### Bail clamp 82C530F0

Retail's bail clamp `82C530F0` on one prop: `w` = inverse mass (motion record +0x7C, b209), `d` = the prop's own velocity as its transform tracker measures it (`[dmo+0xA4]+0x90`, last tick's displacement / dt, written by `82E0C578` from the DMO update, b210); ours: the prop's velocity before this tick's skater pushes. The linear velocity is pulled into the ball of radius `mag` around `n * mag` (n = d / |d|, mag = blend x (max(cap, |d|) + cap), cap = factor x w); the spin is capped at the angular factor x w, or zeroed for a heavy prop (and the linear cap is 0). Returns (linear, angular); `None` for a static body (w = 0).

### Contact material block

Friction pair `[static, dynamic]` of a retail body contact material block. The block is three floats at physics component +48 / +52 / +56 = {static friction, dynamic friction, restitution (DMO data +272)}, written by 82C550A8 and pointed at by every body's +80; the collision-object builders 82DC3A68 / 82DC4158 / 82DC4588 copy it to CO +116..+124 and aaCollision 8277A508 combines two objects' blocks with 82763078 (static max, dynamic max, restitution min = [`combine_contact_materials`]).

### Per type material data

Per prop type material data. Retail reads these from the DMO type data (DMO+4380 -> +4, ctor 82C51E28): the layout of the type's vault record of class `livingworld_dynamicobject_characteristics` (see [`dmo_type_blocks`]). Each body carries the retail values of its type (resolved at map load from the installation's stock vault); a mod's `carry.by_template` entry overrides them field by field. A `None` left after both falls back to the authored MOBJ material (only props whose type record is missing, e.g. an older setup without the type map: NOT RETAIL YET there).

### Box inertia shape

Box inertia shape (DMO data +16 scale, +32 offset): 82C4E568 takes the body's AABB half extents x scale + offset as the box 82C47FC8 fills the inverse tensor from. `None` = the class default record's [`DMO_DEFAULT_INERTIA_SCALE`] / zero when another body field is set, else the authored box inertia.

### DMO type record layout

Retail values of one DMO type, read from its record of [`DMO_TYPE_CLASS`] (parents included). The DMO constructor 82C51E28 keeps the record's layout at DMO+4380 -> +4; schema layout offsets (skaterschema): +272 restitution (`Hash_5CCD5998E03C299B`), +312 upright flag (`Hash_C4D8A03586A31915`, -> DMO+4465 bit 0x10 and 82C4B960's record+272), +316 / +324 upright pair (`Hash_E0101A9DFD63DEE9` / `Hash_CDA7A31C5EDBEB6E`), +320 / +328 default pair (`Hash_6E0BB4F5881A4841` / `Hash_086956BCA2187458`), read by 82C53EF8 / 82C550A8; +308 `LinearDrag` and +336 `AngularDrag`, read by 82C4E568 into the body's Inertia (+32 / +36), with +304 mass (`Hash_E5778CDD4576D890`), +292 / +296 velocity caps (`Hash_4890392C91829954` / `Hash_BAA01E2BA1237455`) and the +16 / +32 inertia box scale / offset vectors (`Hash_F4D1C84C36A854AC` / `Hash_D3CDE380DBB3ADC0`). The held pair is the retail constant (`commanded_material`), not type data.

### Body material choice

The body's own contact material block (82C53EF8 / 82C54BF0 -> 82C550A8): while commanded {held pair, restitution}; otherwise the free pair, which for a type with the upright flag (data +312 bit 0) is the upright pair while `up_y > upright_cos` (82C54B00) and the default pair when tipped. Pure function of the commanded bit and the up axis.

### Body inertia 82C4E568

Body `index`'s Inertia (82C4E568, filled once when retail builds the body and never switched by the commanded block): with type data, mass (data +304), velocity caps (+292 / +296), drag (+308 / +336) and the box inertia of the AABB half extents x +16 + +32 ([`dmo_body_inertia`]); a mod's `carry.by_template` fields over it. Fields neither sets keep the authored MOBJ values (density mass, box inertia, damping, no caps: NOT RETAIL YET, only without type data). Pure function of the type data, the mod rules and the authored box, so every peer gets the same.

### Prop grab objects

Every prop with grab splines as a grab-scene object of the world-object provider (scene `+4084`, query mode bit 0x04; single-player worlds use it, not the DMO provider, b54): template origin and basis as the frame, points scaled like the body, linear velocity as the record vector, type-2 records. Ids: object `PROP_GRAB_TAG | grab slot`, spline / geometry `PROP_GRAB_TAG | grab slot << 6 | index` ([`Self::prop_of_grab_key`] maps a key back to the prop id). NOT RETAIL YET: the assembly is a stand-in with the object id (the `+172` object is not identified).

### Prop grab assembly 82585F58

82585F58 (b227): the part (assembly+8) is the prop's one body, its rates (part+0x4C) the body's rigid body: words 56..65 = its Inertia (inverse tensor, inverse mass, spherical, max linear / angular velocity, drags; the tensor's w lane is not read), +144 = angular velocity, +160 = centre of mass. Move Object reads +216 only as a null gate (82D442D0 / 82D44A10), so the stable prop id serves as part and rates identity.

### Prop world query

Prop volume against static world triangles: the physics/world query (TU3 8277B720 via triangle dispatch 8277BC58), NOT the GP volume-pair query 82AD43A8. The world query forwards the query context's object byte (+61) into triangle fixup (82AD3130), which for objects accepts a contact across a welded flat edge (cosine 1, no convex bit) while the normal stays within convexity_epsilon of the face (acos(1 - 0.01) = 8.1 deg) and does not reject disabled vertices. The pair query passes false and drops every such contact: a box lying a few degrees tilted on a tiled floor lost all floor manifolds and fell through (docs 26, "contact gap"). Limit = the body's own padding, the same gap the static resolver accepts (`gap > contact_padding` is skipped there); no velocity prediction, as the prop solver has no speculative rows.

### Move Object command slot 9

Move Object command for the held prop, as retail interface slot 9 applies it (spec section 7: 8275FF00 queue -> flush 827601F0 -> DMO handler 82C4C370 / 82C4C3E0 -> sinks 82D9CC78 / 82D9CCF0), once per 60 Hz physics step, before this step's contact solve: - skipped for an unknown, locked or non-dynamic body (retail gates DMO+4464 bit 0x08 and component+36; props have no lock state yet); - wakes the body and clears its sleep counter on EVERY command (82ADF7B8), zero or not; - linear: an acceleration with no mass factor at the centre of mass (`v += L dt`, no torque; the accumulator +144 already holds gravity, which our integrator adds for every prop); the vertical argument is dropped (82C4C370 forwards only &linear); - angular: the yaw command replaces the angular accumulator (+160 = (0, Y, 0), no inertia factor): any torque queued on the body is discarded and `w += (0, Y, 0) dt`; contacts still change pitch and roll in the solve; - marks the body commanded, so its parameter block switches to the commanded block on its next step (82C53EF8).  The command lasts one step: the carry re-sends it every tick and our prop step runs once per tick (no substeps). `grip` is used only when a mod turns `apply_at_com` off. Returns false if the id is unknown.

### Upright start

Start the self-righting window of one body (retail cMsgUprightDMO -> DMO manager slot +40 82C4B8C0: DMO+4464 |= 0x40, timer DMO+4376 = 0). Retail sets it only when the DMO's slot 28 test (82C564D8, physics body field +28) returns 0, the same test that offers Upright on the phone (82666748 -> 82C4B578); that field is not decoded, so ours refuses only an unknown id or a body without dynamics (NOT RETAIL YET). Restarting an open window zeroes the timer, as retail does. Returns false if refused.

### Upright pass 82C56780

The DMO update's righting pass (82C56780), once per step before the contact solve, for every body with an open window (asleep or not): the timer advances and closes the window past `window_seconds` (that update still sends its command); a tilt under `stop_angle_deg` closes it and zeroes the timer; otherwise the command goes through the DMO's angular slot 37 (82C52EE0): skipped for a body without dynamics, else wake, mark commanded (DMO+4465 0x02) and replace the angular accumulator (82D9CCF0 writes +160), integrated like the Move Object yaw command (`w += C dt`).

### Prop contact rows

Retail contact solve for every awake prop at once: one row per manifold point (A = the prop, B = the triangle or the other prop, normal from B toward A as the pair queries return it), built by the retail ContactBatchBuild (82AE10C8: targets in displacement units, predicted separation v dt + separation + a dt^2, restitution -v dt e) and iterated `iterations` times by 82AE27D0 (contacts only). Returns the per-body correction buffers, indexed by body (the integrator turns the +0 / +32 pair into velocity, +16 / +48 into position only). No slop, no correction fraction, no per-tick cap (retail has none).

### Move Object yaw rate PID

A held right stick turns the prop at the retail yaw-rate target and no faster. 82D45318 [code, 0x82D45BC8..0x82D45CD0]: yaw error = w - 60 x (facing change since the previous tick, +384), PID 20 / 0 / 40 with the output accumulating (integral action) and clamped to 6 rad/s^2. The retail math on a free yaw body (`yaw_rate_feedback_settles_at_the_target_rate` in skate-core) spins up at 0.1 rad/s per tick, peaks 0.6 % over |w| and settles at |w| with zero steady error; the expectation here is |w| from the command itself (curve BFB3BEF0BB2661C0(|lever|) x rot x gain +1160).

### Upright command values 82C573D0

82C573D0 values: stop under 10 deg, axis up x world-up, speed 3 x (min(tilt, 70 deg) - 5 deg) for |A| <= 1.1, command (target - w_axis - 0.1 w_perp) x 60, fallback to the body's X / Z axis above 120 deg.

### Held prop as ped obstacle

Retail [code]: the hold sets only DMO+4464 bit 0x20; the obstacle-off gate is a different word (+144+4252), so a held prop stays a NavPower obstacle (cut when slower than 0.4 m/s, moving avoider when faster). Ours: cut, or solid for the ped's step while moving. The test's control is the rule "held = ignored": the ped then walks into the prop.

## Code reference: prop_carry.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/prop_carry.rs`; each comment there links its section here.

### Prop carry model

Buttons (raw controller flag bits, see `CarryButtons`): holding the retail GrabWorld button (RB) grabs the nearest prop in reach and keeps it while held; releasing drops it. B toggles placement mode while carrying, and releasing the grab button in placement mode confirms the ghost pose.

A is the retail sprint button: the derived controller's held timer for action 80 (slot 20) publishes `OB_Sprint` (8259AA8C, `offboard_intentions.rs`), so it must never grab. Retail gates the offboard grab-object decision (82D324B0, `biped_ground/grab.rs`) on Processed2476 bit 22, GrabWorld, which the input listener emits while raw flag bit 28 (action 73, RB) is held (8259AF68): a held button, not a toggle.

Carry is retail Move Object (state 502, research note move-object-retail.md): the held prop stays a normal dynamic body (gravity, contacts, tipping) and every fixed tick receives one bounded command, a horizontal linear term plus a yaw term from two velocity-tracking controllers (82D45318, `skate_core::player::offboard::move_object`), through `PropDynamics::apply_move_command`. The prop leads and the skater follows its grab edge (82D45D30): `PropCarry::skater_target` is read by `biped_ground`. The held prop neither receives skater pushes nor pushes the skater back: `PropDynamics::set_held` exempts it from volume pushes and its collision-layer triangles stay parked at `HELD_PARK` until the drop rebakes them.

Placement mode (Phase 4): the prop keeps following a target pose relative to the carrier, frozen mid-air by the per-tick velocity overwrite, while the right stick adjusts distance (Y) and yaw (X) and DPad up/down adjusts height. Releasing the grab button confirms: the prop is dropped at the ghost pose and the layout sidecar is rewritten. B cancels back to plain carry. Yaw applies a direct orientation snap; position still moves by velocity so contacts and the rebaked triangle layer keep working.

Layout persistence (`prop_layout.rs`): confirming a placement records `id -> (origin, basis)` and saves `settings/prop-layouts/<map>.json` next to the asset root (same convention as `settings/gameplay.json`). Loading a map teleports saved bodies to their stored poses before the first sync.

Restrictions: grabbing requires `BipedGround`; a single prop at a time; the grab is kept while the held grab record qualifies (retail 82D44A10 -> CanGrabSpline 82E08EE8 at the grip, see `still_holds`), and dropped when it stops qualifying or when leaving the on-foot states (`BipedGround` / `OffBoardPushing`; never saves). While held, the retail grab-object byte (`OffBoard304`, published in `player_state/publication.rs`) keeps the selector in `OffBoardPushing` and the MotionGraph in MovingObjectNew, so carrying must not treat state 502 as leaving the ground. Drop keeps the current velocity, so releasing while moving throws gently; re-sleep is the natural cool-down.

Locomotion while holding: retail's producer 8259C4B0 turns the raw left stick into OB_ObjectMvX/Z and the right stick X into OB_ObjectMvRot. The command reads them in the prop's latched grab-edge frame: Z pushes / pulls along the edge normal, X slides along the edge, an off-centre push turns the prop through the lever-arm curves, the right stick turns it.

Multiplayer: a `PropCarry` is one player slot's state; its Move Object controller is plain data (`controller_snapshot`), the command is a pure function per fixed tick. Prop bodies and layouts are host-local; a host would replicate held id, body poses / velocities and layout writes. No transport is implemented.

### Interim grab record

The interim grab record of a held prop: which vertical box face is held (local axis index and side) and the grip position along its edge, both in the prop's own frame so the grip turns with the prop. NOT RETAIL YET: the stand-in for props without authored grab splines (and for every prop while `SKATE_PROP_GRAB` is off); props with splines carry by their authored record ([`PropCarry::frame_for`]). The face toward the skater at grab time is the grab edge.

### Carry buttons

Which raw controller flag bits drive carrying. The bits index the packed button word of `DerivedControllerInput` (word 6 previous, word 13 current; bit = 101 - action for actions 74..81, bits 28..31 for actions 73..66). Defaults are the retail buttons; a host setting or mod may rebind them.

### Ground grab decision

The Ground grab decision this tick (82D324B0, `biped_ground::grab`): `Some(Some(descriptor))` when a validated record qualified (state +144, descriptor +152 / +156), `Some(None)` when none did or in another state. The game always passes one: a carry then starts only from the Move Object enter's bind of the published record (`bind_published`, b233). `None` (host tests, the HUD) has no decision and keeps the interim pick (nearest prop within `grab_range`, its nearest record).

### Grab frame per tick

The grab frame and record of prop `id` this tick. Props with authored grab splines (with `SKATE_PROP_GRAB=1`): the records are rebuilt from the body's current pose (82D44A10 path A); without a binding (`bound` `None`) the best record from the skater position (82D4D150 mode 0) is bound with a new grip (82D444A0 full), with one the record of the same descriptor keeps its grip (82D444A0 continue); the frame is the record at the grip (82D45D30). Other props: the interim box edge `edge` (the face toward the skater when `None`). Bindings are updated in place.

### Regrab path B

Path B of 82D44A10: the prop's best other record from the skater reference (82D4D150 mode 1 without the held descriptor) that passes 82E08DB8, bound with a new grip (82D444A0 full). NOT RETAIL YET: the candidates are the held prop's own records (retail asks the owner's validated records), owner flag 0x40 and the held record's +196 / +216 are taken as set.

### Carry weight and orbit logs

CARRY_WEIGHT once per grab (b284: a held prop's weight is only these two curve outputs, set at the grab; a light dumpster means a light mass or yaw inertia here). CARRY_ORBIT while turning: retail turns the prop about its centre and the skater walks round it, so the skater's angle round the centre should follow the prop's heading.

### Follow step

Follow step after the command (this tick's latch): +416 steps toward the edge and that step is the displacement handed to the character move (82D44A10 -> 82BDF268). NOT RETAIL YET: 82BDF268 itself (sweep, step-up, weight +1124); here the skater root is moved by the same step (the walking job's velocity override in biped_ground), so the live body offset never feeds back.

## Code reference: skate-core move_object.rs

Detail moved out of the code comments of `crates/skate-core/src/player/offboard/move_object.rs`; each comment there links its section here.

### Move Object mechanism

Move Object (off-board grab and push, physics state 502): the per-tick command retail 82D45318 sends to the held dynamic object, as a pure function over plain data (no ECS, no I/O).

Retail mechanism [code, TU3 recomp]: every 60 Hz tick the held object gets a bounded command, a horizontal linear term plus a yaw term only, from two velocity-tracking controllers (PhysicsControllerData, update 82D4E118). It is never position- or velocity-forced, and there is no pitch, roll or vertical term (lift gain FDE807D9B85A6AC2 = 0), so gravity, contacts and tipping stay with the normal rigid-body simulation. The skater follows the object's grab edge (82D45D30), not the other way round.

Every tuning value is data: attribute collection class `3EDA5B140604613D`, key `default` (loaded by skate-game); the [`MoveObjectTuning::default`] values are the stock values for tests and a missing collection. Image constants (slew 4.0 per tick 0x82257308, filter 0.9 / 0.1, blocked decay 0.95 and gain 0.5, lever coupling -0.25 0x8208ED00) are fields too, so a mod can reach every number.

Multiplayer: [`MoveObjectController`] is the whole per-carrier state, plain `Copy` data with a flat `to_array` / `from_array` form; the command is a deterministic function of (tuning, state, input), run once per fixed tick.

### PhysicsControllerData

`Sk8::Physics::PhysicsControllerData`: four floats. 82D4E118 [code]: `filtered = (1 - filter) * filtered + filter * error`; `output += proportional * error + filtered_gain * filtered + derivative * (error - previous)`. The output accumulates; the clamp is not inside the controller, but the caller writes the bounded command back into the output (anti-windup, see [`command`] steps 8 and 10).

### Hold qualification

Hold qualification while grabbing (82D44A10 -> 82E08EE8 at the grip distance +1128), from physics_state_offboard `default` [data]: grab box half extents GrabBoxSizeGrabbing (+0), its offset GrabBoxOffset (+32), GrabSplineAngleLimitGrabbing (+452, degrees, approach angle) and GrabSplineMaxAngleToHorizontalGrabbing (+436, degrees, edge slope).

### Skater follow

Skater follow (82D44A10 before 82BDF268) [code]: the follow point (+416) steps toward the target frame's edge point (+192) moved `follow_reach` (0.65, image 0x820BB0EC) along the latched frame toward the skater, at the skater root height plus `follow_height` (0.72, 0x8220E144). The step is the anchor velocity (+624) x `tick` plus the rest clamped to `follow_step` m (0.1, 0x820641A8; 82BD41B0).

### Anchor velocity

Anchor velocity (+624, 82D46610) [code + data]: the anchor is the grip point moved `anchor_reach` toward the skater (96ECC98838ECCC11 = 0.7, this collection), y dropped; v = keep x v + new x (anchor change / tick) with keep 0.85 (0x822250F8) and new 0.15 (0x820994B4).

### Hand IK while holding

Hand IK of the held update (82D46610 sets +1200 bit 0x40, 82D45008 steps the weight +1132) [code + data]: on once `hand_ik_enter` (+1180, 5E35DB02BE697A58 = 0.7216) > 0, the state time is at most `hand_ik_window` (the larger x end of curves 1348E9A1F213B42D / 702F25BA3A5AAA56, 1.0) and 1 - `hand_ik_curve` (702F25BA3A5AAA56) at the state time exceeds `hand_ik_threshold` (0.1, 0x820641A8). The weight moves `hand_ik_rate` (0.2, 0x82099280) per tick; the targets are clamped to `hand_ik_reach` (0.65, 0x820BB0EC) around the animated hands (82BD9728 / 82BD97D0).

### Command 82D45318

Port of 82D45318 steps 1 to 10, signs as in the code: lever = dot(-edge, centre - grip), rot = min(1, sign(lever) curve(|lever|) MvZ + MvRot), w = -(curve(|lever|) rot gain), fwd += 0.25 lever w, yaw command sent as (0, out, 0) about +Y (slot 9 angular sink, right-handed like our world). The yaw error is the target minus the measured yaw rate (the facing change since the previous tick x 60, same rotation sense: +Y turns +Z toward +X), so the feedback is negative. Edge direction: retail builds +320 from the hand points (82D45D30, sense not traced); taking it as `side_of(forward)` is the only choice for which an off-centre push turns the object the way the push torque does (r x F about +Y). With it a positive OB_ObjectMvRot gives a negative yaw rate (clockwise seen from above, a right turn for the skater). NOT RETAIL YET: the edge sense.

### Heading latch

Heading latch [code, 0x82D455E0..0x82D4573C]: turning (|w| >= 0.1, 0x820641A8) stores the latch every tick (0x82D45714). Otherwise the wrapped angle between the facing and +368 (8296EBB0) is the drift; above the re-latch threshold (557FA142008FD7CE) the latch is stored, below it the store is skipped (0x82D4570C). The drift does not enter the yaw error. The latched side row is +400 = -(+144) [code, b240 / b241]: +144 = Y x forward = `side`, so the stick's x moves the pair to the skater's right, (-f.z, 0, f.x).

### Yaw controller

9. Vertical: lift gain x height error; stock 0 and not applied. 10. Yaw controller [code, 0x82D45BC8..0x82D45CD0]: 8296EC98(+384, now, axis (0, 1, 0) at 0x82139A20) = acos(dot) of the normalised facings, 2 pi - acos when cross(+384, now) . axis < 0 (2 pi at 0x821647F0), 0 for a vector shorter than 1e-2 (|v|^2 vs 1e-4 at 0x8209BE90); wrapped to [-pi, pi) with 1 / 2 pi (0x82139A60) and 2 pi (0x82139A50); then +384 = now (every tick, both latch branches). Rate = wrapped x 60 (0x822F860C); error = w - rate. For an upright object the signed angle about +Y is the heading change, which is what the port measures.

### Board on grab

Board part of the Move Object enter 82D442D0 (0x82D44304..0x82D44364) [code, TU3 recomp]: a switch on SkateboardController+448 (`cmplwi 5`, above 5 skips). 0 (stopped), 1 (held) and 5 (on the ground at the feet) let go and become 2; 4 (being retrieved) hides and becomes 3; 2 (free) and 3 (hidden) are kept. +444 is zeroed before either call, the new state is written after it. So grabbing an object drops a carried board where it is.

### Still holds

Held re-qualification of 82D44A10 [code]: CanGrabSpline 82E08EE8 on the held record (state+720) at the grip distance (+1128) along it, from the skater reference point (+272, bone 23), with the grab box placed on the skater frame (Player+192) by 82D2E250 from GrabBoxSizeGrabbing / GrabBoxOffset and the *Grabbing angle limits in degrees (x 0x3C8EFA35). False = retail stops holding (flag 0x20 clears, no command is sent). `skater_frame` rows: right, up, forward, position.

## Code reference: living_world/dmo.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/dmo.rs`; each comment there links its section here.

### DMO streaming core

Dynamic objects (DMOs: benches, bins, cones, ...) streamed by the living-world census, as retail does (research b97; doc 27 "DMO streaming").

Per census pass (`sub_826B7980`) [code]: 1. the circle from the `dynamicobjects` range record (same lerp as peds; retail data has no speed keys and no forward offset for DMOs); 2. cull (`sub_826BAD98`): a live, cullable DMO whose HORIZONTAL distance to the centre exceeds the cull radius goes, with its whole linked group (`sub_82C52600`); then, once per pass, when the pool holds `cap` (49) or more live objects, the front of the eviction queue goes; 3. spawn (`sub_826B9D58`): placements within the outer radius, expanded by touching neighbours within 20 m so clusters come together, up to `budget` (49, 100 in fill mode) per pass; with the pool full a placement spawns only when its score beats the eviction front's, which is evicted for it.

The score (`sub_82C4A130` placements / `sub_82C55160` live) weighs priority, a count and the distance, with the five `livingworld.dynamicobjects` values (2.5 in front of the camera, 2.0 for the keep flag, 250, 300, 450).

NOT RETAIL YET: the score's count `n` (`sub_82C503F0`, meaning open) is 1; the overlap blocker (a live DMO in the placement's volume) is not modelled; touching (`factor 0.5`) is bounding spheres; the eviction queue is ordered by the live score (its insert point is not read); multiple observers are ours (retail runs this for one player).

### DMO keep state

[`Self::step`] with the live objects' keep state: a live object for which `kept` is true is scored as kept (retail's live score `sub_82C55160` sets the keep flag from the object's own state, research b184), so it is never the eviction queue's front while unkept objects are live. Retail still evicts the front every pass; only untouched props go, and they come back where they already were.

## Code reference: player/offboard/move_object/held_update.rs

Detail moved out of the code comments of `crates/skate-core/src/player/offboard/move_object/held_update.rs`; each comment there links its section here.

### Held update rebind

The rebind block of Move Object's held update 82D44A10, the hand points of 82D45D30 and the frame blend 82D46218 / 82D463D8 (research b64 [code, TU3 recomp]; main checked the 0.5 hand factor and 82D43B20):
- every tick: the push latch (+1200 bit 0x02, set once |Player+736|^2 > 0.04) and the collision timer +1176 (+1/60 while Player+2484 bit 26 is set, else 0);
- the block runs only with the hand flag (Player+2476 bit 21), the timer at most 0.4 s and the latch clear; path A (a published best record and CanGrabSpline 82E08EE8 on the held record at the grip) refreshes the record (grip kept, no blend); path B (owner flag 0x40, 82D4D150 mode 1 without the held descriptor, 82E08DB8, the held record's +196 / +216 non-zero) re-grabs with a new grip, zeroes the anchor velocity and starts a frame blend only when it already held last tick; anything else loses the hold (+1200 bit 0x20 clears);
- 82D45D30: the hand points sit at grip +/- 0.5 x |hand bone 3 - hand bone 7| along the record, clamped to [0, length];
- 82D46218: the blend runs only when the jump D = |target - current| reaches T = threshold x 60, at rate -T / D per tick; else the frame snaps.

Multiplayer: [`RebindState`] and [`FrameBlend`] are plain per-skater data.

### Held hand IK

The hand IK of the held update: +1200 bit 0x40 (`enabled`) and the weight +1132. 82D46610 (82D469C8..82D46C9C) sets the bit during the grab's enter window ([`super::MoveObjectTuning`] `hand_ik_*`); only 82D444A0 full (a grab or a path B re-grab) clears it, so a re-grab after the window leaves the hands without IK, as in retail. 82D45008 moves the weight toward 1 while on, toward 0 while off. Plain per-skater data.

## Code reference: player/offboard/move_object/held_record.rs

Detail moved out of the code comments of `crates/skate-core/src/player/offboard/move_object/held_record.rs`; each comment there links its section here.

### Held record and grip

The held grab record of Move Object (state +720) and the grip along it (+1128), research b62 [code, TU3 recomp]:
- 82D444A0(state, full): `full` (enter 82D442D0 and the path B re-grab) copies the record's reversed bit (rec+200 bit 0x20) into +1200 bit 0x04 and places the grip at the nearest arc distance (82D2CFE8) of the skater reference (+272, bone 23), clamped to [h, length - h] with h = min(GrabSplineEndExclusion, length / 2). Always: the side test c = cross(rec+80 - ref, rec+64 - ref).y; c < 0 swaps the ends, negates the edge direction (+112) and toggles the reversed bit; when the record's bit then differs from +1200 bit 0x04 the grip is mirrored (length - grip) and the bit copied, so it stays on the same physical point of the edge.
- Path A of the held update 82D44A10 refreshes the record from the object's current pose every tick and runs 82D444A0 without `full`.
- 82D45D30 evaluates the record at the grip (82D2D2B0, arc distance).

Multiplayer: [`HeldGrip`] is the whole per-skater state (plain `Copy` data); the record itself is rebuilt from the object every tick.

## Code reference: physics/grab_view.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/grab_view.rs`; each comment there links its section here.

### Grab visualizer

The grab visualizer (opt-in "Visualize Grabs", `SKATE_VISUALIZE_GRABS=1`): what the player can grab, drawn in the world from the same records the game decides with, and logged.
- Every record in grab range (the grab owner's validated set: the props near you on foot, the car spots near you on board) is an amber line along its spline chord, with a dot at each end.
- The one a grab would take now is green: a thick line, a ring pulsing around its middle and an arrow pointing down at it. On foot that is the record the Ground grab decision qualified (82D324B0); on board the car spot the skitch latch holds (82D39D98).
- `GRAB_VIEW` (when the state, the ready record or the records in range change): each record with its owner (prop id and template, or car spot), distance and height, and the nearest prop with grab splines when nothing is in range.

A viewer aid only: it reads the decision, never feeds it, so play is the same with it on or off.

## Code reference: living_world/dmo_stream.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/dmo_stream.rs`; each comment there links its section here.

### DMO streaming game side

DMO streaming in the game (doc 27 "DMO streaming"): the map's placed props run through the retail DMO census (`skate_core::living_world::dmo`, census slot 2 of 4) and a culled or evicted prop goes dormant (`GamePhysics::stream_prop`: body asleep and still, collision parked, model hidden); a spawn brings it back at its authored pose, as retail respawns a culled DMO from its placement record (research b98: nothing writes the live pose into the record; the moved-pose map only rebuilds records at a chunk reload, not modelled). Props in the player's saved placement layouts (#15, not retail) are never streamed, so layouts survive.

On by default, as retail streams its DMOs; `SKATE_DMO_STREAM=0` (or a mod) turns it off, and then every placed prop exists all the time. Logs `DMO_STREAM` (counts per change) and `DMO_STREAM_CHANGE` per decision.

## Code reference: physics/prop_layout.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/prop_layout.rs`; each comment there links its section here.

### Prop layout sidecar

Sidecar persistence for placed prop layouts (Phase 4).

One versioned JSON file per map at `settings/prop-layouts/<map>.json` beside the asset root, the same user-data convention as `settings/gameplay.json`. Only confirmed placements are recorded; the map package itself is never modified. A missing, corrupt or foreign-map file leaves the authored poses untouched rather than failing the load.

## Code reference: grind_world/props.rs

Detail moved out of the code comments of `crates/skate-game/src/grind_world/props.rs`; each comment there links its section here.

### Prop grind rails

Grind rails carried by dynamic props (DMOs), retail provider 82C4C4E0. Each DMO part keeps its rails in its own model space (setup 82C535C0 fills the rail map at DMO+1796 from the part's EB001B list); every query re-places them with the prop's current matrix, so a moved or knocked prop takes its rails along and no grind state is attached to the prop.

## Code reference: modding/world_props.rs

Detail moved out of the code comments of `crates/skate-game/src/modding/world_props.rs`; each comment there links its section here.

### Mod world props

Mod-created dynamic props (doc 27 "Props created mid-game"): `sdk.world.spawn_prop` copies a map prop (model, collision, physics block, type data) through the engine's runtime prop path (`GamePhysics::spawn_runtime_prop`, the same path a released hand prop takes) and `sdk.world.remove_prop` / mod disable remove it again.

## Code reference: physics/biped_ground.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/physics/biped_ground.rs`; each comment there links its section here.

### Prop walk-over log

Diagnostics for walking onto props (bench walk-over bumps): which prop the walker's contact target or edge is on, the step height above the feet, and whether the step was taken or why not. Written when the prop or the outcome changes. Outcomes: `step` (target contact on the prop, rise over 5 cm: the contact classifier's step threshold 82D837C8), `flat` (target on the prop, no rise), `no-candidate` (packet flag 1024: no candidate before the forward limit), `obstacle` (contact correction pushes the walker back), `no-target`.

### Move Object skater follow

Move Object (502): the stock graph swaps OBGround's locomotion AttachIntents for MovingObjectNew's OB_ObjectMv set, and retail moves the OBJECT from them (82D45318, prop_carry) while the skater follows the object's grab edge: 82D45D30 builds the skater's target frame from the grab record, 82D46218 / 82D463D8 blend toward it, 82BDF268 moves the character there. The frame blend is a snap at retail data (it only blends when the frame jumps >= 60 m per tick: physics_state_offboard +448 = 1.0 x 60, 82D46218). The root yaw is NOT that frame: 82D46610 writes +1124 = wrapped angle to the edge frame x curve 023D73966B8669A5 at the time held, and 82BE1058 (from 82BDF268) turns the character by it, so the facing eases in over the first 0.5 s of a grab (0.18 of the angle at the grab) and turns the whole angle from then on (b243 / b245). The skater target is the root moved by the step of the retail follow point +416 published by prop_carry (the step is bounded there, 82BD41B0), one tick behind the prop. NOT RETAIL YET: 82BDF268's translation (the +288 hands step, then a pull toward +416 of at most 0.3 m); the walking job's velocity override moves the root onto the target each tick. The walking stick stays idle.

### Follow move contact bit

NOT RETAIL YET (part of the follow move above): the walking controller's approach (82D7F458..FDD0) steps toward the contact target projected onto the FACING line, so following a prop that slides along its edge would come out as a forward step. Retail 502 never runs that job (82D4E2F8 is only submitted from BipedGround 82D30D30); it moves the character with 82BDF268, not decoded. Until it is, the follow move drops the target-contact bit (2) for this job only (support contact bit 1 still keeps the feet on the ground; no 0.3 m step-up).

## Code reference: physics.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-game/src/physics.rs`; each comment there links its section here.

### Reset one prop

Reset ONE object (retail cMsgResetDMO, doc 27 "Object Dropper and reset"): the phone's per-object Reset posts it, PlayerUI's handler 8289A048 calls the DMO manager's reset (vtable 0x82323254 slot +36, 82C4B5F0), whose worker 82C4B780 looks up the object's spawn record and puts the object back on the record's transform in one step (no fade or tween in that path). Ours: the authored pose, at rest and asleep, collision rebaked, the saved layout entry dropped. Refused (false) for an unknown id and for the held object (NOT RETAIL YET: retail's held case is undecoded). The single authority for resets.

### Spawn push acceleration

An AI spawn push: [`Self::set_context_velocity`], and the acceleration sampler's previous velocities take the pushed value, so the push itself reads as no acceleration. Outcome-matched: retail's spawn-placed skaters ride off and do their recorded tricks; with the one-step velocity jump read as acceleration, every placed NPC played the non-interruptable `RidingIdle.Bumped` clip for ~0.6 s and a recorded trick inside it never played (b194).

## Code reference: physics/mass.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-core/src/physics/mass.rs`; each comment there links its section here.

### DMO body inertia fill

TU3 82C4E568 Inertia fill for a DMO body from its local AABB half extents: inverse mass = 1 / mass (+16), caps and drag copied as is (+24 / +28 / +32 / +36), and the box inertia of 82C47FC8 on `h = half_extents * inertia_scale + inertia_offset` (vmaddfp, fused). When any of h.x, h.y, h.z is not > 0 (NaN included) retail uses (1000, 1000, 1000) instead (0x82256FE8). 82C47FC8: k = (1/3) / inverse mass, inverse tensor = 1 / (k (h.y^2 + h.z^2), k (h.x^2 + h.z^2), k (h.x^2 + h.y^2)) with two Newton steps on the estimate, +20 = 1 / the smallest inverse moment.

## Code reference: input/offboard_intentions.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-core/src/input/offboard_intentions.rs`; each comment there links its section here.

### Object move inputs

Object-move inputs while carrying a prop (retail Move Object mode), 8259C4B0 called from Fill825999F0 with the current RawControllerInput (derived words 7..13). The stock OffBoard AG forwards all three to the MG, where MovingObjectNew attaches them to the same-named skeleton-input channels:
- OB_ObjectMvX = raw left X * x_gain(|a|/pi);
- OB_ObjectMvZ = raw left Y * z_gain(|a|/pi);
- OB_ObjectMvRot = clamp(raw right X + sign(a) * rotation(|a|/pi) * s, -1, 1).

`a` is atan2(x, -y) wrapped to (-pi, pi]. The scale `s` (caller f21) is not resolved; it only multiplies the rotation curve, which is all zero in the shipped data, so the left-stick term is left out.

## Code reference: player/offboard/grab_scene/qualify/polyline.rs (fork additions)

Detail moved out of the fork-added code comments of `crates/skate-core/src/player/offboard/grab_scene/qualify/polyline.rs`; each comment there links its section here.

### Spline point at along

82D2D550: the spline point at a signed position `t` along the record, from the middle of its ends (the skitch frame step 82D48148 and hand targets 82D4A378; research b148). At or past +-half (`+180`) the ends (+64 / +80); otherwise u = t + half (reverse bit `+200` 0x20: half - t), and the points are walked first to last, each segment counting its length projected onto the first -> last direction (floored at 1e-4, 0x8209BE90); the first segment that takes the total past u holds the point, stepped back inside it, to world by the record frame. Unlike 82D2D2B0 (`at_distance`) the order never reverses and lengths are projected.

## Skater vs prop mesh

2026-10-22, living world todo `living-world-ramp-prop-ride.md` (USER-ASKS 255), research b501
(`.local/research/b501-ramp-prop-ride.md`).

- **Problem (laptop session 2026-10-22, Black Box park):** "Trying to skate one of the ramp props didn't work at all,
  the prop moved and my character fell inside of it." (DMO_Glbl_mediumQuarterPipe_1002, vault quarterpipe_medium,
  250 kg; ridden at 16:41:01, wipeout reason 1, the body sank into it.)
- **Cause [code, main checked]:** the skater push collided the skater's volumes with the prop's box (the AABB of its
  collision triangles). On a ramp the box includes the air above the slope, so a board on the slope was "inside" and
  shoved the prop along the box normal with the full closing speed every tick (under the PROP_SHOVE log threshold);
  once the prop slid away, its one-sided triangles never pushed the skater back out.
- **Retail [code from b484 / b485 / b234, inferred for the outcome]:** the skater meets a DMO through its own mesh
  (ClusteredMesh, one-sided, edge codes), never a box; the DMO callback 82C54618 adds no actor impulse.
- **Change:** `prop_dynamics.rs` `skater_mesh_manifold`: a skater volume tests the prop's posed triangles in the prop
  layer with the world triangle test (`primitive_triangle_world_contacts`, the same one-sided test the skater's ground
  uses); the deepest contact drives the push. The box stays as the cheap bounds reject and as the fallback for a prop
  with no triangles. Mesh contacts get no overlap nudge (the nudge is ours).
- **Still ours (NOT RETAIL YET), replaced 2026-10-25 by the rows below:** the push itself was the impulse model above
  (share of the closing speed through the reduced mass), not skater rows in the prop solve.
- **Test:** `riding_up_a_ramp_prop_does_not_shove_it` (a wheel gliding up a wedge prop's slope leaves it in place).

### Skater contacts as solver rows (2026-10-25, USER-ASKS 357, research b617)

- **Problem (desktop session game-20261025-200102, Black Box park, DMO_Glbl_mediumQuarterPipe_1003, 250 kg):** "ramp
  prop on blackbox map still moves when I skate into it." "I tried to ride into it in its original position and it
  didn't work. and then i moved it and tried again and it moved worse." Log: rides at the spawn pose 02:09:11 to
  02:09:25 (the board reaches the wood about 0.9 m up, then goes airborne), carried 02:09:28 to 02:09:34 (set down at
  [-4.57, 1.03, 9.17], asleep), rides 02:09:36 to 02:09:40. No line logged the prop's pose outside carrying.
- **Cause [code, main checked]:** the mesh change above still pushed with our impulse: closing speed x reduced mass
  (75 kg pusher) x `push_transfer` 0.5, capped at `board_push_speed` 6 m/s (`prop_dynamics.rs` defaults), applied
  before and outside the prop solve, and to resting (asleep) props too. The old test glided along the face (no closing
  speed), so it never saw a board driving into the slope.
- **Retail [code, b617; builds on p20, b182, b230, b231, b234]:** board parts, skeleton bodies and DMOs are bodies of
  one `rw::physics` simulation; a skater-vs-DMO pair is an ordinary two-body contact row (8277A508 -> 82AD43A8 ->
  rows 82AE10C8 / 82AE1778 / 82AE1AE8 -> solve 82AE27D0; materials 82763078). No push mass, transfer share or speed
  cap. 82AE1AE8 fills each side's mass terms only under its own `state & 4` (re-checked in Ghidra): a sleeping DMO is
  an immovable support. Nothing in the physics core wakes a body on contact (only 82ADF7B8, no contact caller).
- **Change:** with `props.solver.skater_rows` (default on) a skater volume's mesh contacts are rows in the prop's row
  solve (`row_corrections`), solved in the same iterations as its floor rows: the volume is a body with its own
  velocity, its own reaction slot and mass `board_row_mass` (8 kg, our board solver's `board_mass`) for board volumes,
  the actor's push mass (75 kg default, peds 10) for body volumes; only the prop's correction is used. A closing touch
  wakes a resting prop (OURS: retail's wake source is not found; the user's account is that props move by mass); 2026-10-28, asked whether riding into a resting cone or trash can without bailing moves it in retail, the user: "Yes", so research b619's bail-only reading is incomplete and is not ported). A
  bailing actor's contacts add no rows (the bail clamp handles them). The box fallback (props with no mesh) and
  `skater_rows = false` keep the impulse push. Mod keys `set_tuning('props', {solver = {skater_rows, board_row_mass}})`,
  validated (mass 0..=1000), read back, reset on mod disable.
- **Diagnostics:** `PROP_PUSHED prop= moved <m> by <n> skater pushes (...) from [..] to [..]` when a skater-touched
  prop falls asleep again.
- **Test:** `a_board_driving_into_a_heavy_ramp_does_not_slide_it` (a board at 6 m/s driving into a 250 kg wedge's
  slope for 30 ticks: under 2 cm as rows); prop tests pass, data-gated `downtown_dragged_props_rest_on_the_floor`,
  `dmo_streaming_keeps_49_live_props_round_the_spawn` and `installed_map_props_resolve_retail_type_data` pass.
- **Open:** retail's wake source for a resting DMO (BODYWAKE hook on 82ADF7B8, spec b231); whether the skeleton is
  dynamic or keyframed while riding (body volume mass); the retail slide distance of a ridden 250 kg ramp (no trace);
  why the first ride at the spawn pose went airborne at 0.9 m (needs the new log).

## Prop watch line

(2026-10-22, for the recomp comparison in `.local/research/b509-dmo-rows-trace-spec.md`)

- `SKATE_PROP_WATCH="x y z r"` (spaces or commas): the first tick a prop's centre is within `r` m of the point, the
  nearest one is armed (`PROP_WATCH_ARM id= template= distance= tick=`); from then on it writes
  `PROP_WATCH tick= id= pos=[] v=[] w=[] up=[] contacts= asleep=` every physics tick, held or not.
- `contacts` counts the prop's static triangle manifolds plus prop pairs this tick (not contact points).
- The recomp counterpart is the `DMOROWS` line (b509): compare time to sleep, final pose, slide distance and peak
  speed after a wake.

## Ground contact reduction

(2026-10-22, research b508 to b510, b512, b517 to b520)

- **Retail [code; a recomp trace settled which sets a prop gets]:** a prop's contacts with the ground set (the map's
  static collision and the fused skate.Park pieces) go through `82779EB0` before the rigid body sim solves them: records
  of one body pair are grouped by normal (dot at least 0.999, `820ED800`) and plane (the nearer of the two point pairs
  within 0.01 m of the head's plane, `820D71E8`), tested against the group's first record; a group of more than four
  keeps four points chosen by `827798F8` (the deepest point, the farthest from it, the widest triangle, the point
  farthest outside it; ties keep the lowest index); kept records keep their order. The job buffer holds 50 records; a
  full buffer is reduced and emitted and the next record starts a new one (nothing is dropped, b520). The terrain set
  (our props-only IgnoreFuser pieces) is not reduced. Trace (b509 run 4, the Stadium - Full construction barrier on a
  fused piece): 4 to 6 ground rows per step, none from the terrain set.
- **Ours before:** every triangle's contact points became solver rows.
- **Change:** `skate_core::physics::contact_reduction` (`reduce`, `reduce_chunk`), used by the row solver
  (`prop_dynamics.rs` `row_corrections`): per prop body, the contacts with the step's world in our fixed candidate order
  (mesh index, then triangle, then point), in chunks of 50, then the props-only pieces' rows unreduced (rows keep the
  old ground-then-terrain order). The older impulse pass (`row_solver = false`, not retail) is unchanged.
- **Differences kept:** retail's record order follows the collision KD tree's leaves (b519), ours the mesh / triangle
  order: identical on flat ground apart from equal ties, can differ on nearly coplanar curved surfaces; retail can share
  a 50-record chunk between up to five props of one job, ours chunks per prop; float order is plain, not fused.
- **Moddable:** `props.solver.ground_reduction` (on), `reduction_normal_dot` (0.999), `reduction_plane_distance` (0.01),
  `reduction_chunk` (50).
- **Tests:** `contact_reduction` unit tests (a coplanar grid keeps four with the deepest point, small groups and other
  planes stay whole, chunks, determinism); the prop suites and the asset-gated Downtown prop tests pass with it on.
  Not play-tested.
## Prop sounds
User report (2026-10-26, RPCS3 clip of a dumpster pushed through DownTown): props made no sound at all: no rolling,
no thumps going down stairs, no hits on other things. Retail has two sound objects per prop on MixMap slot 8
("Dynamic", 5 instances) fed by an audio-side table the physics fills every frame. Research: b625 (first pass,
re-checked), the m-props findings (main), b628 (loop packets), b629 (loop banks), b630 (pushed prop object). Credit:
the retail behaviour was read from the TU3 code with skate3recomp (rexglue, Xenia) and Ghidra + Xenon360.

### What retail does [code]
- Audio table entry per prop and frame (sub_827A3FD8 / sub_827A8AD8): linear and angular velocity, the template's
  characteristics key, and the frame's strongest contact: |relative velocity . normal| per contact point (the DMO
  collision callback 82C54618) and that contact's surface (tag & 0x7F, minus one; 143 = none).
- Impact = the largest of the last three frames' strongest contacts (sub_82489848, a ring per prop).
- The manager sub_824F4108 gives at most 5 props a sound object: the pushed prop, the hardest hit, the fastest, then
  table order; dropped props release theirs.
- Each prop's record: the characteristics' RefSpec at 0x88 (a type that rolls, bool 0x154) or 0xB8 (sliding) names
  a prop SFX record (class B05F7C74C63ACFB8, 141 records on the disc, e.g. dumpstergrunge, mailbox, staircase):
  material, secondary material, impact min / max (m/s), cooldown (frames), two optional attenuations.
- Impact post (sub_824E3BE0): after the cooldown and above the record's floor, t = clamp((impact - min) / (max -
  min), 0, 5); material A against the contact's surface through the collision manager (sub_82486EF0), with the
  materials' ALTERNATE band and window records (the poster's flag 1 + material byte +0x39; our export's `alt`); a
  tier-2 side posts again as tier 1 scaled by the window record's +40; the secondary material posts against none
  above 0.1 km/h. The message's +40 byte is 1.
- Held loops (part B, not yet ported): c_dynamic_rolling_objects / c_dynamic_sliding_objects (34-word packets,
  patch word w9 = record 0x13C; banks Dynamics_Objects_Roll / Slide_A (patches up to 15) / Slide_B (18 and up)).
  Pushed prop loops (part C, not yet ported): the moveable object sub_824E48D0 with per-prop streamed banks.

### Ours (part A, 2026-10-26: impacts)
- Physics (`physics/prop_dynamics.rs`): `note_audio_contact` keeps each body's strongest |closing speed| and the
  other surface's tag between audio reads (both solvers); `take_audio_samples` hands the awake props to the audio;
  `dmo_type_audio` resolves each type's record from the stock vault when the types load.
- Audio (`skate_audio::world::props`): `ContactPeak` (the ring), `pick` (the manager), `ImpactPoster` (the post) and
  `PropSfxRecord`; `collision.rs` gains the flagged band / window / scale lookups (`*_flagged`, `Records`).
- Game (`game_audio/prop_sounds.rs`): once per console evaluation, after the NPC skaters' process: rings, pick,
  posts into the local player's collision manager (`PlayerAudio::post_collisions`, retail has one manager). Log line
  `PROP_IMPACT prop=<id> impact <m/s> (min max) materials tiers levels posts`. `SKATE_PROP_SOUNDS=0` turns it off.
- Setup (`audio_export.py` `collision_tuning`): each material's `alt` window / band records when it sets the flag.
  Installs set up before this have none: the poster then reads the default records (no error); re-export audio to
  get them.

### Ours (part B, 2026-10-26: rolling / sliding loops)
- `skate_audio::world::props::PropLoop`: sub_824E2790's start / release (above 0.2 m/s, in contact, the same record
  as the last evaluation; a sliding loop only with a non-zero speed word), the 34-word packets (`loop_packet`: patch
  w9 = record 0x13C, w13..w34 from the record's words (sliding) or its 22 optional attributes (rolling); research
  b628) and the update's rewrites from the Dynamic owner's MixMap outputs (sub_824E2AF8 / sub_824E2E80: pan, pitch with
  the record's 0xB0 scale, levels 1 / 5 or 8 / 9, the filters).
- `game_audio/prop_sounds.rs`: the five slot-8 instances, each with the owner record's state (sub_824B5558: moved
  state, the moved loop speed rising by record 0x130, "in contact" held 15 evaluations over the game time scale),
  the position block ctl(8,0,g) and DynamicObjectPhysics in0 (the moved prop's spin); a moved prop plays its +0x88
  record (none for the dumpster: its pushed sound is the moveable object, part C). Log line `PROP_LOOP prop=<id>
  start <class> patch <w9> speed word <w4>`.
- Banks `Dynamics_Objects_Roll` / `Slide_A` / `Slide_B` (export `world_audio.WORLD_BANKS`), loaded at the first loop.

### Ours (part C, 2026-10-26: the pushed prop)
Retail [code, research b630, b632, b639, b640, b641]: while the player moves a prop (owner moved state + contact) the
moveable object (process sub_824E48D0, update sub_824E5F18, MixMap slot 8 object Moving) starts a short start loop
(c_sliding_start / c_rolling_start, bank DMO_Starts) on the first moved frame, streams the prop's own main bank (the
record's 0x48, e.g. Moveable_Metal_Box_Large_2 for the dumpster) and then holds a main loop (c_moveable_sliding_objects
50 words / c_moveable_rolling_objects 54 words, patch w9 = record 0x13C, the record's two 37-field sets) and plays
random one-shots from a Splice bank (record 0x18 -> class 74F2, e.g. SK82_Dumpster_Randoms). The dynamic-object
manager (CSTATEMGR_DynamicObject, the same one that picks the five SFX objects) adds a random pitch judder
(sub_824F5EB8) and owns two bus graphs `Sub0 -> Gai0 -> FSS0 -> PI20 -> PI20 -> Pn21 -> Sen0 (SFX Master)` that the
banks' templates route the main loops (codes 2048 / 512) and the sliding start loop into: random gains by speed, a
push / pull frequency shift (record 0xA4 / 0xA8), an LFO peaking EQ and a jittered second EQ (record class DCD364 via
0x30); retail quirks kept (graph B's second EQ centre is never written; its jitter reads graph A's base gain; the
second centre's timer runs twice per tick).
- `skate_audio::world::moveable`: packets (incl. the cross-set words), the start / main updates from the Moving
  outputs, `Judder`, `EqManager`, the one-shot interval; `bus/dynamic.rs`: the two graphs (dsp gain / fss / peaking /
  pan), `Output::Dynamic`, the mixer routes codes 2048 / 512 mono into them.
- `physics/prop_dynamics.rs` `moving_type`: the record sets, banks, one-shot and manager records; setup exports every
  bank the records name (`world_audio.prop_banks`: 27 AEMS banks + 9 Splice banks).
- `game_audio/prop_sounds.rs`: the start loop, the main bank through the prefetch worker then the main loop, the
  judder and EQ ticks, the one-shots (Splice host). Log lines `PROP_MOVE prop=<id> start|main ...`.
- Inferred, not traced: push vs pull = the sign of the prop's velocity against the local skater's (retail: against
  a player vector at +0x10); the Pn21 pan = the main loop's azimuth (retail: the last routed voice's).

### Prop sound banks
Problem (2026-10-29 session, DownTown, pushing a bench with peds seated on it): the first push froze the game for
1.85 s and the first sliding loop 2 s later for 4.84 s. The hitch lines put both in `prop_sounds::frame_pre`,
`audio_parts=[bank_load]`: the start bank (`DMO_Starts`) and the three loop banks (`Dynamics_Objects_Roll`,
`_Slide_A`, `_Slide_B`) were read and their WAVs decoded on the game thread at first use (cold HDD, a trace session
writing to the same drive). The prop's own main bank already went through the prefetch worker and did not stall.

Change: `prefetch_shared_banks` queues those four banks on the prefetch worker ([doc 11, "Emitter bank prefetch"](11-audio.md#emitter-bank-prefetch-2026-10-03-follow-up))
from a map's first frame, again after every prefetch clear (map change, content change). `Native::ensure_bank` at the
first push / loop then takes the decoded data, so only `load_bank` runs on the game thread and the runtime sees the
same calls in the same order. A bank still queued when the push comes loads on the game thread as before.
Test: `shared_prop_banks_are_prefetched_and_load_without_decoding` (data-gated).

### Not modelled / open
- Prop-on-prop contacts post against "no material" (143): retail's other-body surface for a DMO pair is not traced.
- The two runtime terms of the sliding choice (characteristics bool 0x14C with entry +0x4C; entry +0x4E with +0x4F)
  need DMO flags we do not have; the type's rolls flag alone decides.
- "Pushed" (entry +0x20, writer not traced) = the local player holds the prop with Move Object or last pushed it.
- The whoosh-by (sub_824E3FA0) is one shared mechanism for props, peds, skaters and vehicles (b634) and follows separately; the tipped splash (sub_824E43A8, b633) too.

### Mods
World tuning domain `prop_sounds` ([doc 26a](26a-living-world-overview.md#world-tuning-domains)): `enabled` and per prop type key the record fields over the retail record (a type without one starts silent, so a mod can give a prop a sound); restored when the mod stops. Multiplayer: the posts are decided from fixed-step samples keyed by the stable prop instance id, so peers and the replay regenerate the same sounds.

## Carried props keep their moveable sound (2026-10-28, USER-ASKS 389)
- Problem (user, 2026-10-27): "dragging the blue dumpster on wheels did not sound correct", "dragging the yellow metal
  dumpster sounded almost right, its a little off".
- Root cause (session game-20261027-073605): the moveable object (start + main loop on the prop's own bank) runs only
  while the audio entry says "moved by the player" (+0x20). Ours read `commanded`, which the physics step clears every
  step, before the audio pass reads it. A carried prop counted as pushed only on reads that landed before a step.
  Result: DMO_DT_DumpsterMedium_1000 was carried 7 times and got 1 start and no main loop; DMO_DT_DumpsterSmallYellow_1014
  was carried 4 times and got nothing; the shopping cart's loop restarted about once a second. The dumpsters played only
  the generic dynamic sliding loop.
- Checked and not the cause: rolling vs sliding. The disc data (livingworld_dynamicobject_characteristics bool
  Hash_7E9838A590602713) marks every dumpster and cart as sliding, wheeled ones included; only round props (globe,
  pipes, cans, boulders) roll.
- Change: `take_audio_samples` marks the held prop, and any prop commanded in the last step, as pushed
  (`prop_dynamics.rs`). Test `audio_samples_mark_the_carried_prop_pushed_for_the_whole_carry`.
- Umbrella table "keeps its drag / roll sound in water": the table rested on the canal floor (0.88 m deep) and kept
  jiggling under the buoyancy samples, so it fired floor impacts and restarted its sliding loop. Same cause as the
  sunk benches (USER-ASKS 390); fixed with that item, not here. Not play-checked.

## Sunk props come to rest; moved props cull where they are (2026-10-28, USER-ASKS 390, 394, research b651)
- Problem (user, 2026-10-27): "the water bob might be a bit much, pushed a metal bench in the water and its bobbing
  kinda weird"; "the benches i pushed in the water never got cleaned up, but the dumpster did."
- Jiggle, root cause (ours, Better Water): every buoyancy / drag sample went through `apply_impulse`, which wakes the
  body and resets its sleep counter; and a sunk prop resting on the canal floor got a fresh small velocity from the
  samples every step (about 0.004 m/s), so the integrator's energy test never counted toward sleep. Bench
  2079070948 logged 588 loop starts and dozens of small impacts in that state; the umbrella table's "sound in the
  water" (ask 389) was the same.
- Change: water impulses use `add_impulse` (no wake); a prop denser than the water that touched the floor last step and
  moves slower than `prop_settle_speed` (Better Water setting, 0.2 m/s, mod key in the water look patch, 0..5) gets no
  water forces, so it rests and sleeps like on land. Test `a_prop_sunk_in_shallow_water_comes_to_rest`.
- Cleanup, retail [code, b651]: retail has no water reset. The census culls a live DMO by its CURRENT position
  (`826BAD98`, vfunc +16, 100 m) and respawns it at its placement. Ours culled by the authored placement, so the
  dumpster (placed ~95 m away) was culled and respawned while the benches (placed near the canal) stayed.
- Change: `DmoStream::live_at` (skate-core `living_world/dmo.rs`) holds moved objects' current origins, set by the host
  each pass (`living_world/dmo_stream.rs`, `PropDynamics::origin_of`); the cull and the live score read it, a spawn
  keeps the placement. Log line `DMO_STREAM_MOVED <decision> at [x, y, z]` for a moved prop that goes away. Test
  `a_moved_object_culls_by_its_current_position`.
- Open: retail groups the cull by the live object's linked group (`sub_82C4A5C0`), ours by authored touching
  clusters; whether a push breaks retail's link is not read. A `dmo_stream` world tuning domain (cull 100, spawn 90,
  pool 49) for mods is not added yet. Not play-checked.

## Live props evict farthest first (2026-10-29, ask 401, research b657)
- Problem (follow-up of the 2026-10-14 churn report, `.claude/todo/living-world-follow-ups.md`): which prop a full pool
  (49) evicts. Ours scored live non-keep props with `s1 = 250 x placement priority`, so the eviction front was ranked by
  priority and could be a near, in-view prop.
- Retail [code, b657 `.local/research/b657-dmo-eviction.md`]: the live score `82C55160` gives every queued (live) object
  `s1 = 2^31`; the placement priority only ranks spawn candidates (`82C4A130`). So the front is the lowest
  `((2^31 + s2) + s3) x k_view`: props behind the camera before props in front, and among them the farthest from the
  reference. One front kill per cull call while live >= 49 - reserved.
- Change: `skate-core` `dmo::live_score` (s1 = 2^31) for the eviction front and the spawn-victim test; a non-keep
  candidate no longer outscores a live prop, so a full pool turns over only through the per-pass eviction. Weights,
  cap, ring and cull stay mod data (`dmo_stream`). Test: `the_pool_cap_evicts_the_lowest_score_once_per_pass`
  (updated: the farthest unheld prop goes first despite a higher priority). Not play-checked.
- `n` (`DMO+0x1124`) is the prop's safety cost, not a part count (research b658, 2026-10-29; offsets and record values
  checked by main in Ghidra and the converted collections). Attach `82C52718` calls `82C55858` (secondary interface
  DMO+0x24, vtable 0x82322DE8 slot 0xD4) before the queue insert, so a queued prop is never unset. That re-scores the
  safety cost `DMO+0x1120` (`82C55A50`: painter layer 14 at the prop and 4 sample points, class
  `livingworld_dynamicobject_safety`, unsigned max from 1, any 0 gives 0) and sets `n = 0` if the cost or `cfg` is 0,
  else `max(cost, cfg)`, `cfg` = 5 (`roadnetwork`) when `DMO+0x74` is set, else 1. Records: default 1, lowcostarea 2,
  mediumcostarea 3, highcostarea 4, roadnetwork 5, extremecostarea 0xFFFFFFFF (reads as -1: evicted first),
  forbiddenarea 0. Ours keeps `n = 1` (the default area) until layer 14 is exported. Open: what sets `DMO+0x74`;
  camera vs player as the distance reference.
