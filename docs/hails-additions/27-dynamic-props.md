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
- **Phase 4, placement and layouts** (`prop_carry.rs`, `prop_layout.rs`, `prop_carry_hud.rs`): B while carrying
  enters placement (right stick distance / yaw, DPad height; now releasing RB confirms); poses persist to
  `settings/prop-layouts/<map>.json` and reload on map load; a small HUD diamond.
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
`.claude/skills/living-world/tools/eb_big_extract.py` + `xenos_disasm.py`:
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
agCollision retention buffer the board uses (8277C23C duplicate removal). Skater pushes are still our impulse
transfer, not solver rows. Held placement (`carry_to`) still bypasses sleep.

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
- Score `sub_82C4A130` / `sub_82C55160` [code]: `s1 = 250 x priority x k_flag` (keepalways or flag 0x10: 2^31),
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
