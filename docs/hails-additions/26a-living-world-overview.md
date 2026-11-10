# 26a: Living world: Overview, plan and modding

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Problem

Skate 3's free skate is full of life: AI skaters riding lines around the player, pedestrians walking and reacting,
traffic on the roads. The engine has none of it. The audio side already waits for it: #32 ports the retail traffic,
ped and NPC-skater sounds and exposes `TrafficAudio` / `PedAudio` / `NpcSkaterAudio`, but nothing publishes them.

## What retail does

Reference: the TU3 (title update 3) build through skate3recomp (by @mchughalex, built on the rexglue SDK; Xenia), the disc data read
in place, guarded hook runs of the recomp and the user's own play sessions. The retail code is the source of truth
(the user's decision: the recomp does not get peds and NPC skaters fully right, so the retail code is the source of
truth); the recomp only validates. Addresses are TU3; no game code or data is copied.

- **NPC skaters.** The manager `sub_8245BA28` (60-tick cycle) keeps 3 ambient skaters (5 AI in all, 7 skater slots
  shared with online players), spawns at the start of an unused recorded human line 60 to 90 m away and culls
  beyond 120 m. Each NPC is a full skater driven by an `AIController` (skater + 1828). The lines are 60 Hz
  recordings: 1,691 unique lines (3,891 per-tile copies; DownTown 760, University 508, Industrial 423; 208.7 km),
  none in the parks. The pool (`characters_marquee` byte +9) is the 33 pros, recruited teammates and community
  skaters, not `ambient_skater_01..09`. Details: [`living-world/npc-skaters-design.md`](living-world/npc-skaters-design.md).
- **Pedestrians.** The census `sub_826B71F0` caps the population from painted census areas (15 per sub-area, 20 per
  district; pool 31), spawns in a 50 to 60 m ring (at 45 km/h; 50 / 80 / 90 m with offset 20 at 80 km/h) and culls at
  70 m. 51 ped models and 26 hand props in `livingworld.big`, one animation bank (`PedestrianSkeletonPres.abin`, 462
  clips, 50 bones), behaviour from state graphs. Knock-down above a contact of 3.0 (6.0 flagged); get-up 2.0 /
  3.167 s; no ragdoll. Per-entity plugin odds (trash bin 0.5 to 0.65) and hand-prop odds are data. Details:
  [`living-world/peds-design.md`](living-world/peds-design.md).
- **Traffic.** The same census drives vehicles (spawn `sub_826B9B90`, cull `sub_826BAAB8`; default caps DownTown 30,
  Industrial 25, University 10). Cars follow the lanes of the road network (`0x00EB0013`) through a state graph
  (FollowingLane, PassingIntersection, Impatience, ChangingLane, PullingOver, StayingParked) with a speed planner
  (`sub_82C3FA08`) and a manoeuvre decider (`sub_82C41CD0`). Lights: green 7 or 8 s, amber 1 s, all-red 0.5 s per
  direction. In the recomp: median 5.8 m/s, p90 14.9 m/s; cars stop and queue for a skater in the lane; a skitched
  car speeds up to about 16.8 m/s. Cars are kinematic (the AI integrates speed along a lane: `+3408` commanded
  acceleration, `+3412` speed, `sub_82C3FF38`); they spawn in an 80 to 100 m ring (2 tries, at most 1 spawn per
  tick) and cull at 110 m; no traffic spawns in zombie mode or online; there are no patrol cars in the Skate 3
  districts. The horn decider `sub_82C40660` writes the horn state the audio reads. Skitching is a per-frame latch on
  the car (`+4403` bits 0x80 / 0x40, `sub_82C34648`, `sub_82C34CD0`); our `skate-core` already has the skater's
  Skitching state (104). 18 car recipes with colour palettes in `livingworld_models`. Details:
  [`living-world/vehicles-design.md`](living-world/vehicles-design.md).
- **Online:** nothing ambient spawns (census spawn pass and AI count gated by the online flags; culling runs).
- **Free Play** (mode 3, `sub_82706B40`): Traffic / Pedestrians / A.I. Skaters options scale the census caps
  (`sub_826B7010`, `sub_826B8A28`) and switch the AI skaters (`sub_8245C548`); career free roam has no switch.

## Verification

- `cargo test -p skate-data --lib --tests --locked`: all pass (line format unit tests on synthetic blobs).
- Data-gated (`SKATE3_ASSET_ROOT` or `SKATE3_AIPATH_BLOBS`): every line decodes; 3,891 copies / 1,691 unique,
  bounding boxes, 60 Hz ratio, every branch resolves, the pack dedupe.
- Python: every `tools/asset_pipeline/test_*.py` plus `tools.test_setup_assets`: 176 OK, 2 skipped (incl. 16 new
  ped / table / census / road tests and 14 skater export tests; with `SKATE3_DISC_ROOT` the disc test checks 3,891 /
  1,691 / 87 / 193 / 42 and 0 warnings). Two export runs are byte-identical.
- Setup fingerprints: `core`, `hud`, `character`, `environment`, `maps`, `audio` identical before and after.
- Rendered two converted peds (`male_jock_1`, `female_adult_1`): textured correctly.

## Plan (milestones inside the one PR)

Shared core: population engine (`skate-core::living_world`, seeded, Free Play scaling, slots shared with online
players; **done, milestone 2**), crowd renderer and kinematic proxies, `sdk.living_world`. NPC skaters: replay tier, AI, simulated tier.
Pedestrians: body and animation, navigation, behaviour runtime, skater interaction, plugins and hand props.
Traffic (V0 to V8): data (vehicle tables, 18 car GLBs and tints, lanes / junctions / signals), road graph and signal
clock, vehicle census, cars on screen with `TrafficAudio`, the driver (planner, queues, horns, skids, manoeuvres,
parking, alarm), car colliders in the skater solve (roof landings, bails), **skitching** (grab conditions, the car's
skitch state, the skater's side, mod hooks, fixture tests from two recorded sessions), the vehicle mod surface. Then Free Play, zombie mode and the
standing pros, multiplayer (retail default: nothing online; opt-in host-authoritative). The PR description keeps the
checklist.

## Modding

Designed in, not bolted on: retail values are setup data a mod can override by key (tables, lines, profiles,
roads), stable ids, and every system gets a mod-facing entry point next to the engine one with cleanup when the mod
stops. Extends engine modding; there is no retail to match.

Lua (API 2, capability `world_tuning` = 1): `sdk.world.set_tuning(domain, patch)` patches a typed domain while
the mod runs (`nil` restores the mod's patch), `sdk.world.tuning(key, domain)` reads the values in effect. Every
field is optional and defaults to the shipped value; per field the first mod to write it wins. Patches are held
per mod in `WorldTuning` (serialisable JSON) and each change rebuilds the domain into its one authority resource.
When a mod stops, fails or reloads its patches go (`modding::world_tuning::clear_owner`; all mods gone:
`clear_all`).

| Domain | Fields (shipped value) | Resource |
|---|---|---|
| `living_world` | `npc_draw_distance` (1.0, 0.25..4), `skater_fade {fade_in_seconds 1, fade_seconds 1, despawn_alpha 0.2}`, `ped_fade {distance {45, 55}, fade_in_seconds 1, enabled true}` (a model record's own pair still wins), `skater_clips {[phase or phase.Style] = clip}` (empty = shipped picks), `skater_clips["trick.<scorable name>"] = trick animation base` (empty = Tricks.xml picks), `skater_blend_seconds {[phase or default or trick_takeoff or trick_air] = s}` (empty = 0.2 s; tricks 0.05 / 0.1 s), `skater_stance {[record id hex or record name] = regular or goofy}` (empty = the measured retail table; unknown records goofy; read at spawn), `skater_line_chain {radius 4, max_candidates 16, blend_seconds 0.2, keep_facing false}` (line end chaining; root blend onto the new line after a branch or chain, 0 = cut; keep_facing: fix 16 facing carry-over, mod option, not retail), `ped_obstacles {enabled true, min_half_extent 0.2, moving_speed 0.4, recut_fraction 0.25, detour_margin 0.1, step_height 0, held_is_obstacle true, moving_solid true}` (props and mod bodies as ped obstacles; a held prop stays one, retail; `moving_solid` is the NOT RETAIL YET stand-in for the NavPower moving avoider), `npc_skater_props {enabled true}` (NPC skaters push dynamic props), `ped_vehicle_contact {enabled true, push true}` (traffic cars push peds out of the way; no knock-down in retail), `npc_tricks {mode profile, gate_window 300, min_air_frames 50}` (NPC ollie / flip slots re-picked from the profile, or `recorded` / `none`), `skater_trick_profiles {[character key or profile name] = {regular, nollie}}` (empty = the disc's tables), `npc_simulated {enabled false, radius 40, max 3}`, `skaters` / `pedestrians` / `vehicles {enabled true, density 1}` (0..4, scales the census caps), `ambient_skaters 3` (0..8), `free_play {traffic, pedestrians, ai_skaters}` (absent = career free roam; retail Free Play: 0..1, 0 removes them at once) | `LivingWorldSettings`, rebuilt via `reset_mod_overrides()` so the player's menu draw distance returns |
| `props` | `default` / `by_template[<MOBJ template> (MOBJ: the map's placed object records)]`: every `PropTuning` field plus `collision_box {center, half_extents}`; a template entry starts from the patched default | `PropTuningSettings` |
| `carry` | `grab_bit` (28, RB), `placement_bit` (20, B), `grab_range` (2.0 m); Move Object: `push_speed` / `pull_speed` / `side_speed` (3.0 / 2.0 / 2.5), `turn_rate`, `grip_reach`, `linear_clamp` (20), `yaw_clamp` (6), `relatch` (0.1), `slew_per_tick` (4), `linear_controller` / `yaw_controller` ([20, 0, 40, 0.1]), the four curves, `let_go_distance` (1.0); slot 9 application: `commanded_material` ([0.03, 0.02] static / dynamic friction), `upright_cos` (0.65), `apply_at_com`, `yaw_replaces_torque`, `ignore_vertical`, `wake_on_command` (true), `by_template[<MOBJ template>] = {material_held, material_free, material_free_upright, upright_pair, restitution}` | `CarrySettings`, pushed into `PropCarry` and `PropDynamics` each tick (survives map loads) |

Events (`on_event {name = "living_world", event = ...}`, `modding::living_world_events`): `spawn` / `despawn` with the
serialisable record a future host sends (`WireRecord`: kind, stable id, tick, position, heading, seed, choice /
reason), `npc_trick` (id, line, node, recorded, chosen), `npc_line_end` (id) and `vehicle_contact` (the
`VehicleContactEvent`). Tests: `living_world_messages_become_mod_events`,
`living_world_density_counts_and_free_play_set_merge_and_reset` (merge, the values reach the population config,
invalid values rejected, reset), skate-mods validation cases.

Not exposed yet: road district selection for mod maps (the loader picks the district by map name), census range
overrides (`data_config`), spawn tables, models per kind, routes, behaviour overrides. The ped mirrored-animation fix has no values.

## Credits

skate3recomp by @mchughalex (rexglue SDK, Xenia), the reference for how the retail code is used; DumbadsSkate3ModdingTools by Ethanw05 (credits to SunJay,
Dumbad, RenderWareGavin and Tuukkas) for the AIPATH field names, NavPower constants and trigger types, used as a
format reference, no code copied; @andrewnakas' `mx/vehicle` fork as prior work on a (player-driven) vehicle Lua API,
described, not copied.

## Open questions

- Props look (D9, ported 2026-10-08, to playtest): the props' `dynamicobject.default` / `dynamicobject.alphatest`
  materials now render with their own family 15, a port of `dynamicobject_defaultPS` (sun N.L with the dynamic
  shadow, `m_params` ambient, tangent-space specular, detail normal). Needs a setup refresh (environment step) for the
  `m_params` rows; without them the old family 1 fallback and log line remain. Open: retail's static world shadow map
  (`shadowWorld`) has no engine pass yet, so props in building shade stay sunlit. Details: doc 27, "D9".
- Population core (milestone 2): retail reads the census count once per spawn pass (`r23` in `sub_826B9940`), so
  during the initial populate the cap would not bind and only the factory (pool 31) would; the recomp sessions show
  at most 15 peds in 15-cap areas, so we re-read the count per spawn (identical outside the initial populate). Also
  open: the vehicle pool size; the character pool's release rule (`sub_8245B400`; we release the oldest unused
  entry that fits no nearby line, and load at once instead of streaming); the line / profile capability bits
  (profile `+144..+146` unnamed; fit = allowed-skater bit or a matching flag bit); the per-skater stuck despawn
  (`skater+1804` vfunc 76 < 0.2) and the requested-character queue; mode 2 (45 m / 20 m cull, 4-30 m ring); the
  forward offset uses the horizontal velocity direction; spawn points are not snapped to the nav mesh / ground yet.
- Teammate looks at runtime: what writes the binding (recruit menu, save importer or mod).
- AIPATH: branch weight meaning, node flag bit 4, orientation order, `m_ID` bytes 6 to 15; the 38 `ai_skater`
  tunables.
- Roads: decoded in V0 except the meaning of a few raw fields (junction `flag_04`, connector `f32_50`, segment
  `word_56`, quad tags); crosswalks for peds (navigation milestone); NavPower; DMO (dynamic prop) plugin anchors (benches, ATMs,
  fountains are placed objects, not waypoint streams).
- Ped rig: the converted models carry 39 bones, the animation bank 50; matched by name in the ped-body milestone.
- How retail picks among shared-look entities (`sub_826B8B88`).
- Traffic: lane snapping at spawn, the census +144 reader, which of the 4 signal controllers and which light group
  each junction approach uses (V1), the chassis / secondary tint rule of the `vehicle_chassis` shader (V3), the skid
  flag writer, the skitch grab / attach / release numbers, the vehicle bail thresholds.
  New recomp hooks (skitch, lights, connectors, vehicle bails) and a few short play sessions will measure them.

## The Skyline example mod starts switched off (2026-10-19)

**Problem.** Laptop session 2026-10-19 18:04: the Skyline example car (`mods/Skyline_Drive_Mod`, `examples.skyline`,
upstream since 2026-09-11) loaded at every start without the player turning it on, spawned at the player's spawn
point, blocked peds walking by (`PED_BLOCKED ... (mod body)`, the car is a ped navigation obstacle) and cost 2 to 3 ms
of each fixed step (`modding::fixed`). The user had not turned it on and asked to switch Skyline off both
for our upstream PR and in hails-additions.

**Change.** Its `mod.json` sets `"enabled_by_default": false` (the field the dev test mods already use,
`crates/skate-mods/src/schema.rs`), with the `mod.json` line of `SHA256SUMS.txt` updated (sums are over the LF form).
It stays in `mods/` and can be turned on in the mod menu, through a saved preference, or for one run with
`SKATE3_MODS_ENABLE=examples.skyline`. A player who already turned it on keeps their saved preference.

**Tests.** skate-mods (106 pass); `skyline_physics` still needs the SDK example's local GLB (known, unrelated).

## Code reference: skate-mods world_tuning.rs

Detail moved out of the code comments of `crates/skate-mods/src/world_tuning.rs` (the `sdk.world.set_tuning` schema); each comment there links its section here.

### World tuning domains

World tuning writes (capability `world_tuning` = 1): typed patches a mod applies to the living world, dynamic props and prop carrying while it runs (`sdk.world.set_tuning(domain, patch)`).

Every field is optional; an absent field keeps the game's value (retail by default). Patches are held per mod; when several mods patch the same field the first writer wins, and a mod that stops (or sends `nil`) gives its fields back. The schema lives here so the command is validated at the serde boundary; the engine (`skate-game` `world_tuning`) applies it.

Domains:
- `living_world`: `npc_draw_distance`, `skater_fade {fade_in_seconds, fade_seconds, despawn_alpha}`, `skater_line_chain {radius, max_candidates, blend_seconds, keep_facing, facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds, fakie_spawn_seconds, fallback_valid_m}` (NPC skater line end: retail 4 m / 16, the mode 1 fallback's valid-line distance 6 m; root blend onto the new line after a branch or chain, engine default 0.2 s, 0 = cut), `ped_fade {distance = {near, far}, fade_in_seconds, enabled}`, `skater_clips {[<phase> or <phase>.<style>] = <stock clip name>}` (NPC skater puppet clip per replay phase; phases in [`NPC_SKATER_PHASES`]; a clip whose name holds `_CYC` loops), `skater_blend_seconds {[<phase> or default] = seconds}` (NPC skater crossfade into a phase's clip; stock graph default 0.2 s, 0 = cut; also `trick_takeoff` 0.05 s / `trick_air` 0.1 s), `skater_clips["trick.<scorable name>"]` = a trick animation base (`<base>_G` / `<base>_A`), `skater_clips["fakie_channel"]` = the stock tree overlaid while riding fakie (`B_FAKIE_CHANNEL`), `skater_stance {[<record id hex> or <record name>] = "regular" | "goofy"}` (NPC skater natural stance, read once at spawn; retail values from the measured table, unknown records goofy), `skater_stance_events {board_backward, mirrored, switch = <clip attribute name>}` (the trick clip attributes that toggle an NPC skater's stance bits; retail `animboardbackward` / `mirrored` / `switch`, empty = that toggle off), `ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, target_snap, step_height, held_is_obstacle, moving_solid}` (props and mod bodies as ped navigation obstacles; retail on / 0.2 / 0.4 / 0.25 / target_snap 0.5 / held props stay obstacles; `moving_solid` is our stand-in for NavPower's moving avoider, default on), `npc_skater_props {enabled}` (NPC skaters push dynamic props like the player; retail on), `ped_nav {steer_rate, turn_edges, stall_seconds, stuck_distance, stuck_seconds, give_up_seconds, yield_patience, crosswalk}` (ped navigation, see `PedNavPatch`), `ped_body {enabled, parts, look_ahead, range}` (peds as solid bodies in the skater's contact solve, collision group 11; retail on, three capsule parts posed from bones: `parts` = [{from, to, centre, radius, half_length, mass}], `look_ahead` seconds of ped velocity, `range` m from an observer, `reaction_part` the part whose speed after the solve decides the ped's reaction, retail 0 = the torso), `ped_vehicle_contact {enabled, push}` (traffic cars touching peds; retail on / on: the ped is pushed out of the car, no knock-down), `npc_tricks {mode, gate_window, min_air_frames}` (the trick an NPC skater does at a recorded ollie / flip slot: `"profile"` = retail, re-picked from the character's profile table when the recorded air is long enough; `"recorded"` = the line's own trick; `"none"` = no ollies / flips; retail windows 300 / 50 recorded 60 Hz frames), `skater_trick_profiles {[<character key> or <ai_skater_profiles name>] = {regular, nollie}}` (each a list of `{trick = <EScorableID 0..332>, weight}`; replaces that table, an absent table keeps the disc's; a character key wins over a profile name), `skaters` / `pedestrians` / `vehicles {enabled, density}` (per kind; density 1 = retail, 0..=4, scales the census caps), `ambient_skaters` (NPC skaters offline, retail 3, 0..=8), `free_play {traffic, pedestrians, ai_skaters}` (retail Free Play mode: traffic and peds 0..1 scale the caps, 0 removes them at once, `ai_skaters` on / off; absent = career free roam), `npc_simulated {enabled, radius, max, respawn_seconds, respawn_min, respawn_max, placement_hold_seconds, respawn_hold_seconds}` (NPC skaters near the player as full physics skaters driven by their AI record; default off until play-tested, 40 m, 3; bail respawn after 5 s clamped to 1.5..7.9 s; mode 7 walk back `walk_back` (retail on), `walk_arrive_distance` (2.4 m, 0..=20), `walk_stuck_ticks` (600, 0..=36000); takeoff board yaw `board_yaw` (retail on), `board_yaw_dead_zone` (0.31416 rad, 0..=pi/2), `board_yaw_mag_scale` (2/pi = 0.63662, 0..=100); air steer `air_steer` (retail on), `air_steer_dead_zone_deg` (2, 0..=180), `air_steer_full_deg` (10, 0..=180); `manual_value` (the side an NPC holds a manual with: + nose, - tail; retail 0.5, 0..=1); `bump` (player and simulated NPCs collide as full skaters, retail on); `reactions` (NPC skaters react to wipeouts within `reaction_radius` m: slam / slam_b / crash lines, retail on, 15, 0..=100)), `skater_path {line_end_t, lost_radius, line_end_lost_radius, lost_max_speed}` (the simulated NPC skaters' path follower: the on-path line end trigger past this fraction of the second-last segment, retail 0.8, 0..=1; the lost tests at 6 m and, after a line end, 4.5 m, below 14 m/s), `npc_walk_back {...}` / `npc_line_controller {...}` (the NPC skaters' walk-back controller and line controller: every field of `ControllerBSettings` / `ControllerSettings` by its Rust name, retail defaults, floats >= 0, counts <= 36000; see `NpcWalkBackPatch` / `NpcLineControllerPatch`); `npc_avoid {enabled, max_entries, skitch_cooldown_ticks, radius_skater, radius_pedestrian, radius_vehicle, radius_prop, cone, wide_cone, wide_cone_distance, skater_radius, speed_margin, stop_gap, stop_gap_far, stop_cone, stop_gap_prop, side_on_angle, floor_headroom, skitch_cos, low_prop_height, low_prop_time, step_off_cap, step_off_time}` (the NPC skaters' retail obstacle avoider, `skate_core::living_world::avoid`; metres, radians, m/s, seconds).
- `props`: `default` and `by_template[<MOBJ template name>]`, each a [`PropTuningPatch`].
- `carry`: `grab_bit`, `placement_bit`, `grab_range`, and the Move Object tuning while holding a prop (retail defaults from attribute class 3EDA5B140604613D): `push_speed`, `pull_speed`, `side_speed` (target m/s at full left stick, retail 3.0 / 2.0 / 2.5), `turn_rate` (constant yaw gain replacing the retail inertia curve), `grip_reach` (m between the grab edge and the skater), `linear_clamp` / `yaw_clamp` (command clamps, 20 / 6), `relatch` (rad, 0.1), `slew_per_tick` (4), `yaw_rate_feedback` (60: the per-tick facing change times this is the measured yaw rate the yaw controller tracks; 0 = off), `linear_controller` / `yaw_controller` (`[p, filtered, d, filter]`, 20 / 0 / 40 / 0.1), the curves `lever_rotation`, `lever_yaw`, `mass_speed`, `inertia_yaw_gain` (`[[8 x], [8 y]]`), `let_go_distance` (m, 0 = off = retail), `drop_board` (grabbing a prop drops a carried board, retail true), `follow_step` (0.1 m), `hold_angle_limit` / `hold_max_angle_to_horizontal` (80 / 50 deg), `hold_box_extents` ([0.9, 0.8, 1.01]), `record_272_speed_scale` (2.0), `grab_end_exclusion` (0.25 m), the hand IK `hand_ik_enter` (0.7216, 0 = never), `hand_ik_curve` (`[[8 x], [8 y]]`), `root_yaw_gain` (root yaw ease over the time held, `[[8 x], [8 y]]`), `hand_ik_rate` (0.2 per tick), `hand_ik_reach` (0.65 m) and per template `record_272`. `grip_reach` sets the retail follow reach (0.65 m). Contact material blocks: `commanded_material` ([0.03, 0.02] static / dynamic friction), `upright_cos` (0.65) and per template `material_held`, `material_free`, `material_free_upright`, `upright_pair`, `restitution`, `linear_drag`, `angular_drag` (per second, retail DMO data +308 / +336 of the type), `mass` (kg, +304), `maximum_linear_velocity` / `maximum_angular_velocity` (+292 / +296) and `inertia_scale` / `inertia_offset` (+16 / +32).
- `shadows`: `world_floor = {r, g, b}`, the lightest a dynamic object's shadow can make the baked world (each 0..=1, in the shader's squared lightmap space). Retail {0.05, 0.09, 0.13}: the constant every retail world receiver shader adds to its shadow-map visibility before taking the minimum with the baked lightmap.
- `backdrop`: `visible` (bool), the district's global presentation model (Industrial's sea, the far sea planes, distant tree walls). Retail draws it (true). `proxy_terrain` (bool), the far-proxy hills retail leaves drawn where no full-detail cell pairs with them (Industrial's south hills under the tree wall). Retail true.
- `respawn`: `air_timeout_ticks` (integer, 1..=[`MAX_AIR_TIMEOUT_TICKS`]), the fixed 1/60 s ticks a skater may spend in the air before the checkpoint respawn (retail 300 = 5 s, `CalcSuggestedState` `count > 300`), e.g. after falling off the map.
- `exposure`: the auto-exposure meter. `meter_weights = {r, g, b}` (each 0..=1; retail {0.3, 0.4, 0.3}, the channel weights retail's bloom downsample dots its tone-mapped value with) and `meter_scale` (0..=[`MAX_METER_SCALE`]; retail 2.515, the evaluator's average scale).
- `ghost`: the skater fade-in after every placement (respawn, teleport, marker return, spawn). `enabled` (bool, default false: the skater is solid when the screen shows it), `fade_in_seconds` (0..=[`MAX_GHOST_FADE_IN_SECONDS`], retail 1.0; 0 = no fade) and `hold_alpha` (0..=1, retail 0.68, the opacity the fade waits at while retail's hold condition is set; that condition is not decoded yet, so it has no effect now).
- `video_screens`: the park videoscreens' live feed ([doc 35](35-world-shading-parity.md#videoscreen-live-feed)). `enabled` (bool, default true), `width` / `height` (16..=[`MAX_VIDEO_FEED_SIZE`], default 512 x 288), `rate_hz` (1..=240, default 30) and `max_distance` (1..=10000 m, default 300). The defaults are provisional, not retail values yet.
- `prop_sounds`: the dynamic props' impact sounds ([doc 27](27-dynamic-props.md#prop-sounds)). `enabled` (bool, retail true) and `types`: per prop type key (the characteristics record name, e.g. `dumpster`) any of `material` / `secondary` (0..=143), `use_contact_material`, `impact_min` / `impact_max` (m/s), `cooldown` (0..=600 console evaluations), `speed_attenuation` / `spin_attenuation` (above 0) over the type's retail record; a type without one starts from a silent record.

### AI skater push fields

AI skaters push while slower than their line (retail on): below `push_factor` x the line's speed (0..=1, retail 0.97), each reducing condition multiplies by `push_reduce` (0..=1, retail 0.3), faster than `push_fast_speed` m/s reduces (0..=100, retail 11), a slope under `push_downhill_slope` reduces (-10..=10, retail -0.22), no push from `push_max_heading_deg` off the line (0..=180, retail 40); `push_right_foot` picks RightPush (default) or LeftPush. `auto_pump` (ours, off = retail): NPC skaters pump down slopes with the player's auto pump intents (doc 26c "DLC map lines and opt-in pumping").

### Ped behaviour fields

The ped behaviour runtime: `enabled`, `mood` (the mood system raises wants), `wander_speed` (m/s), `warn_seconds`, `know_about_seconds`, `mood_want_memory_seconds`, `conversation_turn_seconds`, `conversation_gather_seconds` (s); 0..=100 each (retail 2.0, 3.5, 30.0, 30.0, 3.0, 30.0); `run_from_honker_distance` (m sideways of a honking car's line) and `run_from_honker_speed` (m/s), 0..=100 (retail 10.0, 6.0); `warn_speech`, the warn's speech value 0..=127 (retail 53); `object_flee` (a prop moving at a ped makes it flee, retail on), `object_flee_radius` (m), `object_flee_min_speed` (m/s), `object_flee_lane_margin` (m), 0..=100 (retail 8.0, 0.75, 0.75; doc 26g "Object flee"). `flee_leg` (m), `flee_arrival_radius` (m), `flee_threat_speed_gate` (m/s), `flee_side_blend`, 0..=100 (retail 15.0, 2.0, 1.0, 0.5; doc 26g "Flee legs go round rails and walls"). `flee_releg_on_nav_done`: a new leg when the path ends or fails short of the goal (retail true; doc 26g "Flee re-leg when the path ends").

### Traffic horn fields

One traffic driver's horn: `blocked_time`, `obstacle_time` (s, retail 4 / taxi 1, 2), `approach_speed_kmh` (retail 5, taxi 10), `approach_seconds` (time to the obstacle for the approach horn, retail 2), each 0..=100; `enabled_chance` / `blocked_long_chance` (0..=1, the per-car rolls of driver bits 0x01 / 0x02; retail 1.0 / 0.5, normal 0.3 and reckless 0.8 long). Read when a car spawns.

### Ped navigation fields

Ped navigation (`skate_core::living_world::peds::wander`): `steer_rate` deg/s (measured 255, 0..=1000), `turn_edges` deg (retail 25 / 70 / 115 / 160, ascending, 0..=180), `stall_seconds` (0.5), the still re-path `stuck_distance` m (0.13) / `stuck_seconds` (0.5) (ours on walking peds, the still timer's values), `give_up_seconds` (ours, 2.0, 0 = off), `yield_patience` (ours, 2.0) (0..=60 each), `crosswalk` "off" (retail) or "walk_signal".

### Ped obstacle fields

Ped obstacle rules (`skate_core::living_world::peds::ObstacleParams`; retail: on, 0.2 m minimum half extent, no cut above 0.4 m/s, re-cut after 0.25 x the smallest half extent; ours: 0.1 m detour margin, 0 m step height). `held_is_obstacle`: a prop held by Move Object (or an attached mod body) stays an obstacle (retail true). `moving_solid`: a moving object blocks a ped's step (NOT RETAIL YET stand-in for NavPower's moving avoider; default true).

### Skater line chain fields

NPC skater line end (`skate_core::living_world::replay::ChainConfig`): continue on an unused line whose start node is within `radius` m (retail 4.0; 0 = fade out at every line end), among at most `max_candidates` (retail 16); `blend_seconds`: the drawn root moves onto the new line over this time after a branch or chain (engine default 0.2 s, 0 = cut, at most 10 s); `keep_facing`: mod option, not retail (default false): the skater keeps the way it faces (forward or fakie) across a branch or chain by riding the new line turned round (fix 16). `facing_rule`: one of [`NPC_SKATER_FACING_RULES`]: `riding_entry` (default, retail: the recorded skater frame, turned while a flip latched on entering riding is set, held across switches; a body against its travel on the ground is drawn riding fakie with the stock fakie channel) or `per_node` (not retail, the fix 23 rule: each node folded onto the board's riding direction). `steer_dead_zone_deg` / `steer_full_deg`: retail AI steer ramp (`ai_skater` 2 / 10 deg), data for the simulated tier (the replay tier does not steer). `fakie_high_speed` / `fakie_low_speed` (m/s), `fakie_slow_seconds`, `fakie_spawn_seconds` (s): retail's riding-fakie rule (stock motion graph `UpdateRidingFakie`: 1.0 / 0.5 / 0.2 / 1.0): the NPC is drawn riding fakie (the stock fakie channel over its riding clip) when it rolls against its board's forward above the high speed, or above the low speed for longer than the slow time, never in the first spawn seconds. A very high speed turns the fakie drawing off.

### Upright fields

Upright self-righting (doc 27, Upright); retail defaults: `window_seconds` 2.0, `tick_seconds` 1/60, `stop_angle_deg` 10, `max_angle_deg` 70, `dead_band_deg` 5, `gain_min` 3, `gain_max` 5, `gain_blend_start` 1.1, `off_axis_spin` 0.1, `command_rate` 60, `fallback_angle_deg` 120, `block_yaw` true. Numbers finite and >= 0; the window and tick > 0.

### Prop solver fields

Prop contact solver and sleep rule (retail DMO simulation, 8275DCC8 -> 82DC2840): `row_solver` true = retail row solver (false = the engine's older impulse pass); `iterations` (retail 25, 1..=256); `sleep_energy` (retail 1e-5); `sleep_frames` (retail 2, 1..=10000); `max_sleeps_per_step` (retail 100); `rest_snap` (engine snap, not retail, default false). The bail clamp (retail 82C530F0: a bailing actor's contact caps the prop's velocity): `bail_clamp` (on), `bail_linear_factor` (retail 5.0, cap = factor / mass m/s), `bail_angular_factor` (retail 15.0), `bail_heavy_mass` (retail 200 kg: from this mass on the prop is stopped), `bail_blend` (retail 0.5). `walkable_mass` (retail 5.5 kg): lighter props are collision type 14, which the on-foot walker does not step on. `skater_push_box` (default false): true pushes props by their box (the older engine model, not retail) instead of their mesh (doc 27 "Skater vs prop mesh"). `skater_rows` (default true, retail): skater mesh contacts are rows in the prop solve against its floor friction; false = the older impulse push; `board_row_mass` (8 kg, 0..=1000): the board side's mass in those rows (doc 27 "Skater contacts as solver rows"). Ground contact reduction (retail 82779EB0, doc 27 "Ground contact reduction"): `ground_reduction` (on), `reduction_normal_dot` (retail 0.999), `reduction_plane_distance` (retail 0.01 m), `reduction_chunk` (retail 50 records).

## Code reference: modding/living_world_events.rs

Detail moved out of the code comments of `crates/skate-game/src/modding/living_world_events.rs`; each comment there links its section here.

### Living world mod events

Living-world events for mods (doc 26, "Modding"): `on_event {name = "living_world", event = ...}`.

- `spawn` / `despawn`: every NPC skater, ped and vehicle the population creates or removes, with the same serialisable fields a future host sends (`WireRecord`: kind, stable id, tick, position, heading, seed, the choice; for a despawn the reason);
- `npc_trick`: an NPC skater's trick choice (`id`, `line`, `node`, `recorded`, `chosen` as catalog names);
- `npc_line_end`: an NPC skater ran out of line;
- `vehicle_contact`: a car pushed a ped (`VehicleContactEvent`);
- `ped_hit`: a skater knocked a ped down or made it stumble (`id`, `kind` Knockdown / Standing, `direction` FromFront / FromBack / FromLeft / FromRight, `closing` m/s);
- `ped_chase`: a ped's chase group changed or it told the player its chase state (`id`, `chasee`, `kind` join / join_refused / leave / primary / group_end / state_warn / state_chase / state_tired / state_giveup / state_other, `reason` for group_end: 0 default, 1 aggressivecapture, 2 returntopatrolzone, 3 lostinterest);
- `ped_avoid`: a ped decided to dodge a skater coming at it (`id`, `mover` (players `PLAYER_TARGET_BASE + n`), `kind` jump / step, `dir` forward / left / right / back, `seated`); tuning `ped_brain.skater_avoid_*`, `skater_avoid_standing` false keeps the dodge to seated peds;
- `ped_taze`: a ped's tazer hit `target` (`id`, `target`);
- `ped_takedown`: a ped's takedown attempt ended (`id`, `target`, `success`);
- `ped_speech`: a ped's AI graph changed its speech value (`id`, `value`, `state`: the graph state; `variant` / `list_value`: a conversation turn's, else null).

Engine-facing first: the same messages drive the engine systems; this only forwards them. Extends engine modding; there is no retail to match.

## Code reference: modding/world_tuning.rs

Detail moved out of the code comments of `crates/skate-game/src/modding/world_tuning.rs`; each comment there links its section here.

### World tuning engine side

World tuning writes (capability `world_tuning` = 1, schema `skate_mods::world_tuning`): the same entry for mods (`sdk.world.set_tuning(domain, patch)`) and engine systems ([`set`]).

Patches are held per owner in [`WorldTuning`] (serialisable JSON, in arrival order). After every change the domain is rebuilt from the shipped values plus the merged patches (first writer wins per field) and written into the one authority resource of that domain:
- `living_world` -> `LivingWorldSettings` (rebuilt with `reset_mod_overrides`, so the player's menu draw distance comes back when no mod sets one),
- `props` -> `PropTuningSettings`,
- `carry` -> `CarrySettings`,
- `shadows` -> `retail_render::WorldShadowSettings` (dynamic shadow floor on the baked world),
- `backdrop` -> `retail_backdrop::BackdropSettings` (the district's global presentation model),
- `respawn` -> `physics::respawn::RespawnSettings` (air timeout before the checkpoint respawn),
- `exposure` -> `retail_exposure::ExposureMeter` (auto-exposure meter weights and scale).
- `ghost` -> `skater_ghost::GhostTuning` (skater fade-in after placements).
- `video_screens` -> `video_screens::VideoScreenSettings` (videoscreen feed size, rate, distance, on / off).
- `prop_sounds` -> `game_audio::prop_sounds::PropSoundSettings` (impact sounds on / off, per-type record overrides).
- `decals` -> `retail_render::DecalSettings` (strength of every world decal, live). A mod that stops, fails or reloads loses its patches ([`clear_owner`]); [`clear_all`] when every mod goes.

## Code reference: living_world/opt_ins.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/opt_ins.rs`; each comment there links its section here.

### Living world opt-ins

The living world features still being tested: zombie mode and the grab visualizer (`SKATE_VISUALIZE_GRABS=1`) are off by default; NPC skater physics is on by default since 2026-10-25 (USER-ASKS 338: "Lets turn on by default with the ability to toggle it off"; a settings file from before takes it once through `defaults_version`, so a later "off" sticks). The player switches them in the PC overlay (Opt-Ins), in effect at once and saved in `settings/graphics.json`; `SKATE_ZOMBIE=1` and `SKATE_NPC_SIM=1` turn one on for a run. Prop grab and skitching are not opt-ins: they are always on in the game (`LivingWorldPlugin`). A mod may still change zombie mode and NPC skater physics (`world.set_tuning("living_world", ...)`); when it stops, the player's choice is back (`reset_mod_overrides`). The same list holds the **Challenges** opt-in (off by default, `SKATE_CHALLENGES=1`; the player's choice only), which is not a living world feature: [doc 42](42-challenges.md#change-the-challenges-opt-in-living_worldopt_insrs).

## Code reference: sdk/skate.lua

The `sdk.world.set_tuning` reference moved out of the comments of `sdk/skate.lua`.

### Lua world tuning reference

```text
World tuning (capability `world_tuning`): typed patches while this mod runs (every field optional;
an absent field keeps the shipped value; first writer wins per field; restored when the mod stops,
fails or reloads). Domains:
'living_world': npc_draw_distance (0.25..4, 1 = retail), skater_fade {fade_in_seconds, fade_seconds,
  despawn_alpha} (retail 1, 1, 0.2), skater_line_chain {radius, max_candidates, blend_seconds, keep_facing,
  facing_rule, steer_dead_zone_deg, steer_full_deg, fakie_high_speed, fakie_low_speed, fakie_slow_seconds,
  fakie_spawn_seconds} (NPC skater line end:
  continue on an unused line starting within radius m, retail 4 and 16; radius 0 = fade out at every
  line end; blend_seconds: the drawn skater moves onto the new line over this time after a branch or
  chain, default 0.2, 0 = cut; keep_facing: mod option, not retail, default false: the skater keeps the
  way it faces, forward or fakie, across a branch or chain by riding the new line turned round;
  facing_rule: 'riding_entry' (default, retail: the recorded skater frame, turned while a flip
  latched on landing / spawn / getting on the board is set, held across switches; a body drawn
  against its travel on the ground is drawn riding fakie with the stock fakie channel, like retail)
  or 'per_node' (not retail: each node folded onto the board's riding direction); steer_dead_zone_deg / steer_full_deg: retail AI
  steer ramp, 2 and 10, kept for the simulated tier, unused by the replay tier; fakie_high_speed,
  fakie_low_speed (m/s), fakie_slow_seconds, fakie_spawn_seconds: retail's riding-fakie rule (stock
  1, 0.5, 0.2, 1): drawn fakie when rolling against the board's forward above the high speed, or above
  the low speed for longer than the slow time, never in the first spawn seconds; a very high speed
  turns the fakie drawing off), ped_fade {distance = {near, far} (45, 55; a model record's pair
  wins), fade_in_seconds (1), enabled}, skater_clips {[phase or 'phase.Style'] = stock clip name}
  (NPC skater clip per replay phase: rolling, crouched, air, air_trick, ground_trick, off_board;
  also 'fakie_channel' = the stock tree overlaid while riding fakie, default 'B_FAKIE_CHANNEL';
  a clip whose name holds _CYC loops; an unknown clip falls back to the shipped pick; also
  'trick.<trick id name>' = a stock trick animation base, e.g. ['trick.kickflip'] = 'B_HEELFLIP_IN',
  played as <base>_G on the ground then <base>_A in the air, for every recorded trick slot of that trick),
  skater_blend_seconds {[phase or 'default'] = seconds} (NPC skater crossfade into a phase's clip,
  the player's graph transition curve; stock default 0.2, 0 = cut, max 10; 'trick_takeoff' 0.05 and
  'trick_air' 0.1 set the transitions into a trick's ground and air clips),
  ped_obstacles {enabled, min_half_extent, moving_speed, recut_fraction, detour_margin, step_height,
  held_is_obstacle, moving_solid} (props and mod bodies as ped navigation obstacles; retail on, 0.2, 0.4,
  0.25, held props stay obstacles (true); ours 0.1, 0, moving objects block a ped's step (true)),
  npc_skater_props {enabled} (NPC skaters push dynamic props like the player; retail on),
  ped_vehicle_contact {enabled, push} (traffic cars push peds out of the way; retail on / on, no knock-down),
  npc_tricks {mode, gate_window, min_air_frames} (the trick an NPC skater does at a recorded ollie / flip
  slot: "profile" = retail, re-picked from the character's profile table when the recorded air lasts more
  than min_air_frames before the landing; "recorded" = the line's own trick; "none" = no ollies / flips;
  retail windows 300 / 50 recorded 60 Hz frames),
  skater_trick_profiles {[<character key> or <ai_skater_profiles name>] = {regular = {{trick = <id 0..331>,
  weight}, ...}, nollie = {...}}} (replaces that table; an absent table keeps the disc's; a character key
  wins over a profile name),
  npc_simulated {enabled, radius, max} (NPC skaters near the player as full physics skaters driven by their
  AI record; default off until play-tested, no distance limit as retail, 3),
  npc_walk_back {<any ControllerBSettings field>} (the NPC skaters' walk-back controller: e.g. start_locate_radius /
  goal_locate_radius (m, retail unbounded), arrive_h (2.4 m), stuck_waypoint_ticks (600); floats >= 0, counts
  <= 36000, push_period >= 1),
  npc_line_controller {<any ControllerSettings field>} (the NPC skaters' line controller: e.g. resnap_frames (240),
  stuck_limits (six stage limits in ticks, retail {30, 30, 2, 180, 30, 3}), safe_run (3, >= 1)),
  skaters / pedestrians / vehicles {enabled, density} (density 1 = retail, 0..4, scales the census caps),
  ambient_skaters (NPC skaters offline, retail 3, 0..8),
  free_play {traffic, pedestrians, ai_skaters} (retail Free Play mode: traffic / peds 0..1, 0 removes them at once;
  ai_skaters on / off; leaving it out keeps career free roam).
  Events: on_event {name = "living_world", event = "spawn" | "despawn" (record = kind, id, tick, position,
  heading, seed, choice / reason), "npc_trick" (id, line, node, recorded, chosen), "npc_line_end" (id),
  "vehicle_contact" (contact = car, ped, speeds, position, normal, depth, reaction), "ped_hit" (id, kind =
  "Knockdown" | "Standing", direction = "FromFront" | "FromBack" | "FromLeft" | "FromRight", closing m/s)}.
'props': default and by_template[<MOBJ template name>] = {contact_padding, penetration_slop,
  penetration_correction, max_depenetration_per_tick, restitution_threshold, skater_push_mass,
  push_transfer, body_push_speed, board_push_speed, penetration_push_speed, stuck_release_ticks,
  collision_box = {center = {x,y,z}, half_extents = {x,y,z}}}; a template entry starts from the default.
'carry': grab_bit (28, RB), placement_bit (20, B), grab_range (2.0 m), and moving a held prop:
  push_speed (1.4 m/s), pull_speed (1.0 m/s), side_speed (0.8 m/s) at full left stick,
  turn_rate (1.6 rad/s) at full right stick X, grip_reach (0.35 m between the skater and the
  dragged prop's near face). Per prop type: by_template[<MOBJ template name or type record name,
  e.g. 'dt_garbagebin' (HELD_PROP type=)>] = {material_held, material_free, material_free_upright,
  upright_pair, restitution, record_272, linear_drag, angular_drag (per second), mass (kg),
  maximum_linear_velocity (m/s), maximum_angular_velocity (rad/s), inertia_scale = {x,y,z},
  inertia_offset = {x,y,z} (box inertia from the half extents x scale + offset)}; unset fields
  keep the type's retail values (the disc's livingworld_dynamicobject_characteristics record),
  restored when the mod is disabled.
'shadows': world_floor = {r, g, b} (each 0..1; retail {0.05, 0.09, 0.13}): the lightest a dynamic
  object's shadow (skaters, cars, props, mod graphics) can leave on the baked world. Retail adds it
  to the shadow map and keeps the darker of that and the baked lightmap, so shadows that fall into
  baked shade (a car on a bridge over shaded ground) leave no mark; 0 = full-strength shadows.
'backdrop': visible (retail true): the district's global presentation model (Industrial's sea
  around the docks and harbour, the far sea planes, distant tree walls); false hides it.
  proxy_terrain (retail true): the far-proxy hills retail draws where no full-detail cell
  replaces them (Industrial's south hills under the tree wall); false hides them.
'respawn': air_timeout_ticks (retail 300, integer 1..216000): fixed 1/60 s ticks a skater may stay
  in the air before the game sends them to the last safe checkpoint (retail: 5 s, e.g. falling off
  the map). The wipeout auto reset keeps its own vault times.
'exposure': the auto-exposure meter. meter_weights = {r, g, b} (each 0..1; retail {0.3, 0.4, 0.3}):
  how much each channel of the tone-mapped frame counts towards its brightness; meter_scale (0..100;
  retail 2.515): the factor on the centre-weighted average before it is compared with the area's
  target. A higher reading lowers the exposure.
'ghost': the skater fades in after every placement (checkpoint respawn, teleport, session marker
  return, spawn). enabled (default false); fade_in_seconds (retail 1.0, 0..60, 0 = no fade): seconds from
  placement to fully solid; hold_alpha (retail 0.68, 0..1): the opacity retail waits at while its hold
  condition is set (not decoded yet, so it has no effect now).
'decals': opacity (retail 1.0, 0..1): strength of every world decal (stains, wear, graphics) over its
  surface. Retail blends each decal at its own texture alpha; lower values fade them. Applies at once.
```
