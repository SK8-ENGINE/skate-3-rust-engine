# 26a: Living world: Overview, plan and modding

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Problem

Skate 3's free skate is full of life: AI skaters riding lines around the player, pedestrians walking and reacting,
traffic on the roads. The engine has none of it. The audio side already waits for it: #32 ports the retail traffic,
ped and NPC-skater sounds and exposes `TrafficAudio` / `PedAudio` / `NpcSkaterAudio`, but nothing publishes them.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK; Xenia), the disc data read
in place, guarded hook runs of the recomp and the user's own play sessions. The retail code is the source of truth
(user: "the recomp doesnt seem to have the peds and npc skaters 100% right so use the retail code as the source of
truth"); the recomp only validates. Addresses are TU3; no game code or data is copied.

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
| `props` | `default` / `by_template[<MOBJ template>]`: every `PropTuning` field plus `collision_box {center, half_extents}`; a template entry starts from the patched default | `PropTuningSettings` |
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
  `word_56`, quad tags); crosswalks for peds (navigation milestone); NavPower; DMO plugin anchors (benches, ATMs,
  fountains are placed objects, not waypoint streams).
- Ped rig: the converted models carry 39 bones, the animation bank 50; matched by name in the ped-body milestone.
- How retail picks among shared-look entities (`sub_826B8B88`).
- Traffic: lane snapping at spawn, the census +144 reader, which of the 4 signal controllers and which light group
  each junction approach uses (V1), the chassis / secondary tint rule of the `vehicle_chassis` shader (V3), the skid
  flag writer, the skitch grab / attach / release numbers, the vehicle bail thresholds.
  New recomp hooks (skitch, lights, connectors, vehicle bails) and a few short play sessions will measure them.
