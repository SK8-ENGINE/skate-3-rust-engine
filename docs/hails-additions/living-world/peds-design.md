# Living world: pedestrians design (part of doc 26)

Status: design, 2026-10-04; milestone M0 (data) is done (see `peds-data.md`).
Evidence: `.claude/notes/peds-re.md` (this work) and `.claude/notes/npc-livingworld-re.md` (census, moods, chases,
speech). Tags in this doc: **(code)** from the retail code, **(data)** from the disc data, **(recomp)** observed in the
recomp only. The retail code and data are the source of truth (user, 2026-10-04).

**Delivery (user, 2026-10-04): NPC skaters and all ped work go upstream as ONE PR**, "Living world: NPC skaters and
pedestrians". The milestones below are internal steps of that PR, not separate PRs. Parts marked **[shared]** are
designed once for both peds and NPC skaters (`npc-skaters-design.md` covers the skater side).

Moddability rule (CLAUDE.md, 2026-10-04): every value below comes from setup data as a default that a mod can
override; every system has an engine-facing and a mod-facing entry point; mod disable cleans up.

## 1. Goals and non-goals
- Goal: the shipped game's ambient pedestrians in free skate on DownTown, Industrial, University (and none in parks):
  same census, radii, density, looks, walking on sidewalks and crosswalks, the same reactions to the skater (bump
  stumble / knock-down, warn, chase + takedown, tazer, flee, cheering, slam / trick reactions, greetings,
  conversations, phone calls, sitting and props), the same speech and sounds (already ported, waiting for publishers).
- Also in scope (user, 2026-10-04): **zombie mode** (the "zombie" cheat: `ZombieFollow`, zombie anim set / takedown /
  chase records, no census cap, NPC skaters off) and the **pros / marquee actors standing around free skate** (the
  `marquee` / `marquee_photographer` / `_horn` / `_board` animation sets, `IP_` and `PHOT_` clips).
- Non-goals for this PR: security guards / no-skate zones (Skate 2 only, `npc-livingworld-re` §5), challenge crowds
  (`LargeCrowdGenerator`), world-script ped commands (later, with challenges),
  traffic driving (separate work; peds only need the traffic-light phase clock and a "being honked at" input).
- Never ship game data: the converter reads the user's disc into `assets/private` (like every other group); the
  engine re-implements behaviours in our own code. The retail state-graph XML is read from the user's install at
  runtime (as the skater graphs already are), never committed.

## 2. Data pipeline (setup group `livingworld`) [shared]
New setup group, own fingerprint, optional (on failure the world runs empty and setup reports it):
1. **Tables** (`tools/asset_pipeline/living_world.py`, from `skatercollections` through the existing `vlt.py`):
   `livingworld_census`, `_census_ranges`, `_categorygroups`, `_entitycategories`, `_entities` (+ chase,
   perceptions, navigation, locomotion, moodreactions, moodresults, knowledge, protect), `_moodeventcategories`,
   `_models`, `_entity_animation`, `_entity_takedown`, `_entity_headtracking`, `_handprops*`, `_conversations*`,
   `_load_groups`, `physics_ai`, `livingworld` globals → `assets/private/livingworld/tables.json` with readable field
   names where known and `Hash_*` names otherwise (a mod overrides by the same keys). Ped-model audio fields stay in
   `world_tuning.ped_models` (already exported).
2. **Census layers** per district (world-painter layers `livingworld_npc_census` / `_vehicle_census`, existing
   `census_layers.py` logic) → a coarse grid per district (cell → census record) in `livingworld/<district>.census.bin`.
3. **Road network** (`0x00EB0013`) per district → lanes, segment links, crossings (`roads.json`); waypoint groups
   (`0x00EB001A`) → plugin anchors (`waypoints.json`); NavPower (`0x00EB0027`) → `navmesh.bin` once decoded (M3).
4. **Models**: the 51 ped recipes from `livingworld.big` → GLB per model (both LODs, hair / accessory parts, textures),
   reusing `native_roster` / `character_glb` (binary `.recipe` reader added). Hand-prop models (`zprop_*`).
5. **Animation**: nothing to convert: `data/anim/PedestrianSkeletonPres.abin` is already in stock data (462 clips,
   50-bone rig); the ped state graphs are in `data/state/livingworldentities/pedestrian/`.
6. Validation in `--check-assets` (tables load, every census category resolves to models that exist).

## 3. Runtime architecture (Bevy)
Crate split as today: pure rules in `skate-core` (`living_world/`: no ECS, no I/O, `forbid(unsafe)`), formats in
`skate-data`, systems in `skate-game/src/living_world/`.

### 3.1 Resources
- `LivingWorldData` [shared]: the loaded tables (defaults) merged with mod overlays, keyed by retail record names.
- `CensusMap` [shared]: per district census grid + lookup `census_at(pos) -> record`.
- `PopulationManager` [shared]: per entity class (peds, NPC skaters, later vehicles / props) a budget, ring and cull
  radii, the retail speed lerp (`npc-livingworld-re` §1: km/h key, sets A/B, forward offset), spawn attempts per
  console frame (peds: 2 attempts, at most 1 spawn (code)), initial populate (6000 attempts / 600 spawns / ring
  8-80 m (code)), cull test, and seeded RNG (seed per map + session; mods can reseed).
- `PedPool`: capacity 31 (code), slot reuse; despawn = return to pool (entity kept hidden, components reset) so
  spawn costs nothing at runtime.
- `CrowdLod` [shared]: assigns per-entity fidelity tiers by camera distance and budget (see 3.5).
- `TrafficSignals` [shared with traffic]: crossing phase clock from `livingworld.trafficlights` (green 7 / 8 s, amber 1 s,
  all-red 0.5 s) so crosswalks work before traffic exists.

### 3.2 Components (per ped)
- `Pedestrian { slot, entity_record, model_record, category, census_record, seed }`.
- `PedIdentity { voice, gender, kind (adult / teen / jock / bum / granny / tourist / skater / business / worker),
  anim_set (default / female / bum / granny / jock / zombie), handprop }`.
- `PedBrain`: wants, mood counters (bump count with reset, cooldowns), the 48 named timers (`npc-livingworld-re` §5b
  order), perception cache, chase role.
- `PedGraph`: the AI graph instance and the motion graph instance (intents, active states) on the shared runtime.
- `PedMotion`: velocity, suggested speed, nav target, path cursor, avoidance state, root motion from clips.
- `PedAnim`: clip players + blend layers + events (`LeftToeDown` / `RightToeDown`, `BodyFallType`).
- `PedCollider`: capsule proxy (sizes from `livingworld_models` / `physics_ai` fields once named) that joins the
  skater's contact solve (3.6).
- `PedAudio` (exists): filled from `PedIdentity` + `PedAnim` events + `PedBrain` speech values.

### 3.3 Navigation
- **Sidewalk wander on the road network** (data: `Wander` → `WanderMode.Road`): targets along lanes offset to the
  pavement, links followed at nodes, `UseCrossWalk` at crossings (wait while `WalkSignSaysGo` is false, then cross),
  `WanderOnRoad` when on the carriageway. `NoRoadWander` off the network (plazas, campus).
- **Navmesh**: decode NavPower (`0x00EB0027`) polygons for path queries and "is walkable" checks (retail parity).
  Fallback for custom maps and while the decoder is incomplete: our own navmesh baked at setup from the map's
  walkable collision (same agent radius / height / step as the NavPower header: 0.35 / 1.6 / 0.2, unverified), so mods
  and custom maps work identically.
- **Avoidance**: local steering against peds, the skater and NPC skaters, plus the motion graph's `Avoid` reactions
  (step / jump by direction, sit-avoid) triggered by the retail rules.
- Plugins anchor at waypoint groups (sit, ATM, vending, fountain, newspaper box, trash bin, look-at, conversation,
  spectator).

### 3.4 Behaviour runtime [shared runtime, ped nodes own]
- Reuse `skate-core::graph` (activation, Priority policy, intents, expressions) and `skate-data`'s stategraph decoder
  to run `Pedestrian.xml` (AI) and `MotionGraph_Pedestrian.xml` (motion) from the user's install, with includes and
  template parameters (`knockdown.xml`, `avoid.xml`, `branchsyncturn.xml` …).
- Re-implement the behaviours and conditions the graphs name (160 / 127 registered (code); the ped graphs use a subset:
  MonitorEnvironment, SuggestVelocity, SendSpeechEvent, Wander, NoRoadWander, CheckForRoadTarget, UseCrossWalk,
  WanderOnRoad, RunFromHonker, Scatter, ZombieFollow, PedestrianColliding, GiveUpBeingPrimaryChaser, the chase /
  takedown / tazer set, plugin behaviours, PlayRemappedAnimation, MajorIntentComplete, StopChannel …) as a Rust
  registry `name -> factory`. Handler addresses for each come from the registry tables; port each
  from its code, one at a time, with a unit test per rule.
- Mood → wants, perception (near sphere, cone 120°, line of sight), timers and the escalation rules from
  `npc-livingworld-re` §4-5d (male: warn then angry chase; female: warn + taze or flee; bystanders: nearby collision /
  slam / trick reactions; greetings and conversations).
- **Mod surface**: a mod can register its own behaviour / condition names (Lua callbacks), replace a graph file
  (content overlay), or override per-entity records; unknown names fail at load with a clear error.

### 3.5 Animation and LOD
- Ped animation player for the 50-bone ped rig: sample clips from `PedestrianSkeletonPres.abin` (`skate-data` `Bank`),
  blend (graph `blendTime`s), mirror (`mirrorOverride`), root trajectory for motion, remap logical names through
  `livingworld_entity_animation` (random picks from lists, `tAnimAttributes` windows). Separate from the skater's
  `AnimationBanks` (different rig); shares the clip decoder.
- Knock-downs are animated (no ragdoll) (data): Falling (LyingDown / Crouched / FromStand) → ground cycle → get-up;
  stumbles by direction. `BodyFallType` events drive `PedBodyFallEvent`.
- **LOD** [shared policy]: retail ships 2 mesh LODs per model (data); the model record's 45 / 55 and 65 / 75 m pairs are
  probably the distances (to confirm in code, `sub_827C1188`). Tiers: full (nearest N: skinned, every frame), reduced
  (LOD1 mesh, animation at half rate), far (LOD1, pose update at a low rate). All peds keep the full brain and
  collision (only 31 max, cheap). Audio decides its own 15 / 3 limits (exists).

### 3.6 Physics and the skater
- Each ped is a kinematic capsule driven by its motion; near the skater it joins the skater's contact solve as a
  proxy (the mechanism `physics/network.rs` already uses for remote dynamic bodies), so bails into peds and the
  skater's contacts work through retail contact code.
- Ped reaction rule (code, `sub_82E38FB8`): on contact, kind = knock-down when a contact magnitude exceeds 3.0
  (6.0 in a flagged case), else stumble; direction by angle; re-hits during get-up pick the posture by the get-up
  time (2.0 / 3.17 s; left 1.67 / 2.33 s). Thresholds from `entity_animation` (mod-overridable).
- Takedowns knock the skater down (`npc-livingworld-re` §5: attempt window 3 s, entry table, "KAPOW"); the skater-side
  response must reuse our wipeout states (find retail's `IsTakeDownByBoard` path first). Tazer drop 0.3 s.
- Peds are not tracked by trigger volumes in retail (PR #50 notes §9); keep it that way (a mod can opt in).

### 3.7 Networking [shared]
- Retail spawns **no ambient peds or traffic in an online game** (code, 2026-10-04, `peds-re.md` Answers), so the
  retail-parity default online is an empty living world. Peds online are an opt-in (mod / setting); then they are
  simulated by the session host (or locally when offline); clients receive a compact snapshot (slot, record ids, position, heading, state / clip ids)
  at a low rate with interpolation (`skate-net` interpolation), plus reliable events (knock-down, warn, chase,
  takedown, tazer, speech) so audio and reactions match. Interactions from a remote skater are resolved on the host.
- Determinism is not required across clients; seeds keep a host's world stable across reconnects.

## 4. Mod surface (designed in, not bolted on)
- **Content (no Lua)**: `living_world.json` at the mod root (capability `living_world_content` = 1): override any
  table record by retail name (census max / weights, ranges, entities, models, moods, chase, takedown, animation
  remaps), add entities / models (own GLB with the ped rig), add or replace graph files, per-map data for custom maps
  (`<map>.living_world.json`: census boxes, sidewalk paths, crossings, waypoints). Same merge and conflict rules as
  `audio.json` (first mod by id wins per identity, conflicts in the mod menu).
- **Runtime (capability `living_world` = 1)**: `sdk.living_world.spawn(key, {entity, pos, heading, behaviour?})`,
  `despawn`, `query{radius, kind}`, `set_density(area|all, scale)`, `set_census(record, patch)`,
  `force_state(key, state)`, `set_want(key, want)`, `register_behaviour(name, fn)` / `register_condition(name, fn)`,
  `info()`. Mod peds count against the pool unless `slots = 'own'` (like world audio).
- **Events (capability `living_world_events` = 1)**: spawned, culled, collided {kind, direction, speed}, knocked_down,
  got_up, warned, chase_start / chase_end {reason}, takedown {success}, tazed, fled, greeted, conversation, speech.
- **Engine-facing**: Bevy messages for the same events, components readable by any system, `LivingWorldAudio.expected`
  set at map load.
- **Cleanup**: everything a mod spawned or overrode is removed / restored on disable (same epoch scheme as world audio).
- `check_mod` validates `living_world.json` against the install's tables.

## 5. Retail-parity plan (what is measured against what)
| Item | Source of truth | Check |
|---|---|---|
| Census caps, weights, ranges, speed lerp | data + code | unit tests on the exported tables; headless run: alive count, first / last sighting distances vs code values (recomp sanity: 13-15 alive, 50-62 m / 62-72 m) |
| Spawn rate | code (2 attempts / 1 spawn per console frame, normalised to 30 fps) | headless count over time |
| Looks / variety | data (models, categories) | visual check by the user in game |
| Walking speed, wander, crosswalks | data (graphs, SuggestVelocity), code (behaviours) | headless: speed median ~1.3 m/s; crosswalk waits with the light phases |
| Collision reaction | code (`sub_82E38FB8`, thresholds 3 / 6) | unit tests; scripted recomp sanity (PEDCOLL) |
| Moods / escalation / chases / tazer | code + data (`npc-livingworld-re` §4-5d) | unit tests per rule; existing recordings as sanity |
| Speech, footsteps, body falls, tazer audio | ported (`world-ped-audio.md`) | existing audio tests once peds publish |
Per-frame retail processes run at the 360's ~30 fps cadence normalised to any engine frame rate (memory "console
cadence").

## 6. Test plan
- `skate-core` unit tests: census lookup and budget, speed lerp, ring sampling, cull, collision kind / direction rule,
  re-hit posture, timers, mood escalation, chase end reasons.
- `skate-data` tests: `.recipe` reader, NavPower decoder (round-trip counts), road network parser (gated on assets,
  explicit skip messages per the repo scheme).
- Python tests for the `livingworld` setup group (fixtures, no game data).
- Headless e2e (`--verify` style, muted): load DownTown at Aletown, run 120 s standing then walking a fixed path;
  assert population stats within the code-derived bounds, no NaN, frame time budget (perf: 31 peds × brain + 15 skinned).
- `regression-check` before handing to the user: all maps load, parks have no peds, audio unchanged when peds are off,
  multiplayer smoke.
- `check_mod` + a dev mod (`mods/living-world-test`, off by default): spawns peds, overrides density, listens to events.
- User play test last (looks, feel, reactions); verdicts quoted verbatim.

## 7. Milestones inside the ONE PR (sizes: S ≤ 1 day, M 2-4 days, L 1-2 weeks of work)
| # | Milestone | Size | Depends on | Shared with NPC skaters |
|---|---|---|---|---|
| M0 | Setup group `livingworld`: tables, census grids, road network, waypoints; ped GLBs (+ binary recipe reader); `--check-assets` | M | - | tables, census, pipeline frame: yes |
| M1 | Living-world core: `LivingWorldData`, `CensusMap`, `PopulationManager`, pools, seeded RNG, debug overlay, mod content overlay + runtime spawn / despawn / events skeleton | M | M0 | yes (one manager, per-class budgets) |
| M2 | Ped body: model spawn with variation, ped animation player (clips, blend, mirror, remap, root motion), walk / idle / start / stop, `PedAudio` publisher (footsteps from clip events), LOD tiers | L | M1 | LOD policy and clip decoder: yes |
| M3 | Navigation: sidewalk wander on roads, crosswalks + signal clock, NavPower decode (or baked fallback navmesh), avoidance | L | M1 | avoidance of skaters: yes |
| M4 | Behaviour runtime: ped AI + motion graphs on `skate-core::graph`, behaviour / condition registry, MonitorEnvironment, perception, timers, mood → wants | L | M2, M3 | graph host: yes (if NPC skaters use it) |
| M5 | Skater interaction: capsule proxies in the skater solve, collision kind / direction, stumbles and knock-downs, warn / chase / takedown (skater knocked down) / tazer / flee, bystander reactions, speech events | L | M4 | events and mod API: yes |
| M6 | Plugins and props: sit, conversation, phone, ATM, vending, fountain, newspaper, trash bin, spectate, look-at; hand props and throws; RunFromHonker input | M | M4 | spectate of skaters: yes |
| M7 | Multiplayer (retail default: no ambient peds / traffic online; opt-in host-simulated snapshots + events), mod API completion (`sdk.living_world`, `check_mod`), docs (`docs/hails-additions/NN-living-world.md`), PR checklist | M | M5 | yes |
| M8 | Zombie mode (cheat toggle: zombie anim set, `ZombieFollow`, zombie chase / takedown, no census cap, NPC skaters off) and standing pros / marquee actors (marquee animation sets, placement) | M | M4, M5 | NPC skaters off in zombie mode |
Order of value: M0-M2 give visible walking peds with footsteps; M3-M5 make them retail; M6 is polish; M7 finishes the PR.

## 8. Questions for the user: answered 2026-10-04
1. ~~Time of day?~~ Fixed (user). No time-based census.
2. ~~Zombie mode?~~ The "zombie" cheat (id 5): peds chase and attack you, yellow screen, free skate only (user).
   **In scope.** Code: `IsZombieMode` = cheat query `*(0x830CFD94)+212` vfunc 156; it also lifts the census cap and
   turns ambient NPC skaters off (`peds-re.md` Answers).
3. ~~Online peds?~~ **No ambient peds or traffic spawn in an online game (code):** the census spawn pass needs the
   online byte `0x830B7AE8+323` and `0x83082929` clear (`sub_826B71F0`); culling still runs; no living-world network
   replication exists. Runtime-unverified (servers gone). Traffic / ped levels players "choose" are the offline Free
   Play options (No / Few / Some / Heavy pedestrians, No / Low / Medium / High traffic → census density 0..1 in 0.1
   steps, mode 3 only).
4. ~~Hand-prop throws?~~ Angry peds throw what they hold (user). Peds also throw held items into trash cans: plugin
   `usetrashbin` (`HasDisposableHandProp`, not on road / near crosswalk / chasing → `ThrowHandPropAtTrashBin` at a
   `waypoint_usetrashbin`), part of M6.
5. ~~Pros / marquee actors standing around?~~ **In scope now** (user): added as milestone M8 (§7).
## 9. Risks
- NavPower decoding effort (fallback navmesh keeps M3 unblocked).
- Ped rig skinning: the rx2 skeleton must match the 50-bone animation hierarchy (check the bind order in M0).
- Performance on low-end GPUs (issue #46): LOD tiers and a density setting (non-retail, off by default).
- Recomp is not retail: never tune to recomp timings (memory "recomp is not retail").
