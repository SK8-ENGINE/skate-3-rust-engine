# Living world: vehicles design (ambient traffic and skitching; part of doc 26)

Status: design, 2026-10-04. V0 (vehicle data) done 2026-10-05: [`vehicles-data.md`](vehicles-data.md); findings below
updated where V0 changed them.
Part of the ONE living-world PR (user, 2026-10-04: "Lets begin work on adding vehicles to the game as well ... IF you
need more data, just ask. finding your way to vehicles automated will be tough." then "add this to the living world
work"). Skitching is an explicit part of it (user, 2026-10-04).
Evidence: `.claude/notes/npc-livingworld-re.md` §1, §2, §3, §6, §6b-6f and the new §7 (this pass);
`.claude/notes/world-traffic-audio.md` (traffic audio port, car alarm trigger); `world-audio-hookin-spec.md`.
Tags: **[code]** retail code read in the TU3 static recompilation (reference only, addresses given), **[data]** the
user's disc data, **[trace]** measured in the recomp (the recomp is not retail; timing differs). Code wins.
Credit: the TU3 static recompilation (skate3recomp / rexglue / Xenia) as the code reference; road and AIPATH layouts
cross-checked where possible against DumbadsSkate3ModdingTools by Ethanw05; the andrewnakas `mx/vehicle` fork as
prior work for the mod-facing shape (described, not copied: no licence).

Moddability rule (CLAUDE.md, 2026-10-04): every value below is a setup-data default a mod can override, every system
has an engine-facing and a mod-facing entry point, and a mod's changes are undone on disable.

## 1. What retail does

### 1.1 Population (census) [code + data]
- Manager: the living-world census tick `sub_826B71F0` calls the vehicle pass `sub_826B7760` with the range record at
  census `+112` (`livingworld_census_ranges` record `vehicles`). Ranges are lerped by the local skater's speed (km/h)
  exactly like peds (`sub_826B7D60`), but both vehicle sets are equal [data]: **spawn ring 80-100 m, cull 110 m,
  forward offset 0** [data, confirmed in the recomp: `sub_826B9B90` f1/f2 = 80/100, `sub_826BAAB8` f1 = 110].
- Cull first (`sub_826BAAB8`, r5 = 1), then spawn (`sub_826B9B90`) only when:
  census byte `+168` bit 0x08 (vehicles enabled for this mode) [code `sub_826B7760`], **not zombie mode**
  (`*(0x830CFD94)+212` vfunc +156 = the cheat query the peds notes name `IsZombieMode`; traffic stops spawning in
  zombie mode) [code], the manager byte `[+76]+121` set and the **online byte `0x83082929` clear** (no traffic spawns
  online) [code `sub_826B9B90`].
- Spawn attempts: **2 per census tick, at most 1 spawn**; initial populate (flag r7): **6000 attempts, up to 600
  spawns, ring 8-80 m** (constants `0x82099250` = 8, `0x820E5748` = 80, shared with peds) [code]. Each attempt: a
  random point in the ring (`sub_82E17508`, world `0x83085484`), then `sub_826B8B88` with census layer 3 (vehicle
  census) picks a category by weight and checks the cap, then the vehicle manager's factory (vtable +8) gets a spawn
  record (id, 3 basis rows, position, flag bytes; byte 0 = initial-populate flag) [code]. Where the factory snaps the
  point to a lane is not read yet (open, V1).
- Caps [data, `livingworld_census`, shipped districts only]: DownTown `dwntwn` max **30**, Industrial `indust` **25**,
  University `univ` **10** (default 25). Extra field `FFE5E258BD468196` (dwntwn / indust 20, univ 5, default 15) is
  copied to census `+144` for type-3 records by `sub_826B7E98` [code]; its reader is not found (candidate: the moving
  or parked share, open).
- Categories [data]: dwntwn hatchbacks .1, minivans .1, muscles .075, sedans .2, sports .175, suvs .125, taxis .1;
  indust adds pickups .175 (sedans .175, suvs .15, minivans .075); univ hatchbacks .125, minivans .1, muscles .075,
  pickups .125, sedans .2, sports .15, taxis .125. **No patrol cars in the Skate 3 districts**: `vehicle_class_patrol`
  appears only in the Skate 2 district groups (`district_*_vehicles_census`) and `dlc_vehicles` (max 0).
- Free Play mode 3: `sub_826B7010` copies `clamp01(0x830B7AE8+332)` (the Traffic option No/Low/Medium/High) to census
  `+140`; the vehicle cap is multiplied by `+140` in the census lookup `sub_826B8A28` (type 3) [code]; when `+140` drops
  below 1.19e-7 the vehicle manager (`+2704`) culls everything at once [code]. Outside mode 3 the scale is 1.0.
- Painted areas [data]: `livingworld_vehicle_census` layer, DownTown 89.7 ha, Industrial 52.5 ha, University 96.8 ha;
  parks have none.
- Recomp check [trace]: 17 vehicles tracked in 140 s (152302); 27 vehicles in 164620.

### 1.2 Models [data]
- `livingworld.big`: **18 vehicle recipes** (`recipe/vehicle/*.recipe` + readable `.xml` siblings, schema 6.0,
  `type="Vehicle"`): fourdoor_sedan_01, hatchback_01, minivan_01, mongo_patrol_01, muscle_car_01, older_sedan_01,
  pickup_truck_01/02, reda_car, sedan_4door_02/03, sports_car_01/02/03, suv_01/02, taxi_sedan_01, z_pipeline_lw_vih.
  Each: one LOD (`lod idx=0`), body mesh (`Accessory`), windows (`Equipment`), materials `vehicle_chassis` and
  `vehicle_glass` (diffuse / environment / normal), 73 textures under `vehicle/texture/` (all referenced). Only
  `reda_car` (a marquee prop) has a separate `Misc` wheels mesh, so ambient car wheels are part of the body mesh,
  rigidly skinned to an 8-bone rig (`Vehicle_Root`, `Chassis`, six wheel bones) so they can spin [data, V0].
- Shaders: `vehicle_*`, `vehiclelight_*` (head / tail lights) and `vehicle_glass_*` (VS/PS, `0x820AC584..`); effect
  instances `vehicleEffectInstance` etc. Our retail renderer needs a car material path (chassis tint + glass).
- `livingworld_models` vehicle records (`vehicle_taxi01`, `vehicle_sedan01` ...): recipe name, two **tint palettes**
  (vec4 lists `12026E2EED18CC8D` = `chassis_colours`: 2-10 colours, `DF76D7D773857EDB` = `secondary_colours`: 1-10;
  base records blue / red = the body atlas's paint mask), a vec3 `F983F2518B335286` (a per-class value, 1.9 x 1.7 x 4.0
  for all sedans and sports cars, not the mesh size; open), a float `FD7A66142F16B9CC` (= the wheel bone height,
  i.e. the wheel radius, on most cars [data, V0]), base record `vehicles`: `9FCFDBEA` / `73B6874C` = (65, 75)
  (LOD distances, as for peds), 20 / 20 / 40 / 40, `F6897CC7` = 3. `VehiclesDB.abin` (6.6 KB, stock anim) exists:
  probably wheel / door clips (open).
- Entities (`livingworld_entities`, graph `state/livingworldentities/vehicle/Vehicle.xml`): each points to a model, a
  spec (`livingworld_vehicle_characteristics`) and a driver (`livingworld_vehicle_drivers`):

  | entities | spec | driver | engine record (audio, settled) |
  |---|---|---|---|
  | sedan01-04, old_sedan, hatchback01 | vehicle_spec_family01 | driver_normal | c01_family01 |
  | minivan01 | vehicle_spec_minivan01 | driver_normal | c05_truck01 |
  | sports01-03 | vehicle_spec_sports01 | driver_fast | c03_sports01 |
  | muscle01 | vehicle_spec_sports01 | driver_reckless | c03_sports01 |
  | taxi01 | vehicle_spec_taxi01 | driver_taxi | c04_taxi01 |
  | suv01-02, pickup01-02 | vehicle_spec_truck01 | driver_normal | c05_truck01 |
  | patrol01 (Skate 2 only) | vehicle_spec_taxi01 | driver_normal | c04_taxi01 |
- Drivers [data, V0]: modelled into each car's body mesh (a dark low-poly figure in the left front seat); no driver
  model, part or texture exists. The user sees drivers in retail; they come with the car mesh (D4 answered).

### 1.3 Road network (`0x00EB0013`) [data, decoded in V0; layout in `vehicles-data.md`]
- V0 corrections: there are 76 segments (DownTown 46, Industrial 26, University 4); the header's second count is the
  lane-run count. Lanes run from `node_b` to `node_a` (`node_a` = destination). Junction = inner / outer quad, 8 end
  records (approaches 0-3, exits 0-3, per-lane connector lists), connectors (Hermite, from / to end and lane); lane
  geometry = about 4 m Hermite pieces with edges and arc tables. 33 junctions, 138 connectors, 3,831 pieces. The text
  below is the design pass's first reading.
- Per tile object (82 on the disc): bbox, intersection count, segment count, table offsets (`living_world.parse_roads`
  in the M0 worktree). Segments (0x40 B) are **directed**: id, node A + end, node B + end, length, width A / B,
  **speed limit 14.167 m/s (51 km/h) or 13.889 m/s (50 km/h)**, `word_52` = lane count (8 m wide = 2, 4 m = 1)
  [data, 76 segments over the 3 districts (V0)]. Reverse segments come in pairs (A808 / CD1A).
- Intersections (new this pass, `roads_probe.py`): two quads (inner junction box and an outer box ~3 m larger:
  probably the crosswalk ring), per-approach 0x38-byte records (id, counts, offset, three packed words, probably
  signal group ids), then a connector block: f32 turn speed (13.889 seen; also 10.3, 11.7) + count + **0x70-byte turn
  connectors** = start, end, start tangent, end tangent (a cubic curve), length, index 1..n (12 at a 4-way junction
  with one lane per approach = 4 approaches x left / straight / right) [data, hypothesis on field meaning]. Lane
  samples every 4 m with position, next position and direction (notes §2).
- Runtime: object LivingWorld+0xC0A0, global `0x830854C0`, 248-byte segment records (the vehicle code multiplies lane
  indices by 248 in `sub_82C376E8`) [code]. Global `livingworld.roadnetwork` (1.15, 1.3, 10, 10, 5, 4; getter
  `sub_826B3738`) [data], meaning open.

### 1.4 Traffic lights [code + data + trace]
- V0 [code `sub_826B1540`]: 4 signal controllers (the 20 timers = 4 x 5), each with two light groups and the cycle
  all-red 0.5, green 7, amber 1, all-red 0.5, red 8 s; odd controllers start with the other group. The junction ->
  controller / group binding is not in the road data (V1).
- `livingworld.trafficlights` (7, 0.5, 0.4, 1) read by `sub_826B1540`; per junction phase timers through
  `sub_82E156D8`. Recomp [trace, TRAFPHASE at load]: 20 timers in two orders **0.5 / 8.0 / 0.5 / 7.0 / 1.0** and
  0.5 / 7.0 / 1.0 / 0.5 / 8.0 s: per direction green 7 or 8 s, amber 1 s, all-red 0.5 s (interpretation).
- Cars read the junction state through `sub_82E14EE0` / `sub_82E11E90` from FollowingLane (`sub_82C376E8`) and set the
  junction approach state `+4392` (1, 2, 5) [code]; peds read `WalkSignSaysGo` at crossings (peds design §3.3).
  No traffic-light sounds exist (audio spec).

### 1.5 Driving [code + trace]
Per-vehicle think `sub_82C3D918`: `sub_82E0C4A8`, manoeuvre decider `sub_82C3D830` -> `sub_82C41CD0`, `sub_82C42348`,
`sub_82C40B70`, `sub_82C414A8`, horn decider `sub_82C40660`, `sub_82C40970`. Then the state graph (data, `Vehicle.xml`):
FollowingLane <-> PassingIntersection / Impatience -> ChangingLane / PullingOver -> StayingParked -> PullingOut
(handlers notes §3). Every driving state ends with **look-ahead `sub_82C412D8` -> speed planner `sub_82C3FA08` ->
integrator `sub_82C3FF38`**.
- **Vehicle fields (new):** `+3408` = **commanded longitudinal acceleration (m/s²)**, `+3412` = **speed (m/s)**
  (`sub_82C3FF38`: speed += accel x dt; accel forced to 0 above the cap `+3688` = (`+3680` + `+3684` when `+4402`
  bit 0x02) x f2; both zeroed and flags cleared when |accel| and speed are tiny) [code]. The mover interface at
  `+144` (vtable `0x82322240`) exposes them: slot +88 returns `+3408`, +92 `+3412`, +84 lane, +76 segment index, +72
  segment id, +68 distance (pattern of `sub_82C34588..82C34628`) [code]. The trace's "−76 … +5" values are
  accelerations (−76 = an instant stop request). The audio record's `+144` (acceleration) comes from here.
- Speed planner `sub_82C3FA08` (skipped while `+3424` bit 0x08): base = lane target from the look-ahead plus the
  driver offset (driver record `+60`); following: when `+4403` bit 0x10 (a lead / obstacle ahead) and both speeds
  exceed `D20826F1` (20 km/h), braking accel = (max(lead speed − `3AB7FC7C` x 0.2778, 0)² − v²) / (2 x gap + 0.001)
  with gap fields `+3756` / `+3760` / `+3728` and spec fields `256A412E` (20), `D49FC490` (20), `F682D359` (5),
  `DD49E3C2` (5); stop line: with a stop distance f2, accel = −v² / (2 x (d − f2) + 0.001), and in state 4 an immediate
  stop (`−v`) that sets `+3424` bit 0x08 [code]. In junction state 2 it sets `+4403` bit 0x20 [code].
- Acceleration is rate limited [trace, 47CAEDA0 at 84 s]: 0.2 m/s² per 0.33 s steps up to ~2.3 m/s² from a stop
  (driver / spec fields `328B9F46` 2-3, `5EFF8715` 2-2.5, `75822921` 2.5-3 are the candidates for max accel / brake /
  jerk, read by layout, open).
- Manoeuvre decider `sub_82C41CD0` -> `+4396`: 1 lane change on a timer, 2 overtake a slow / stopped car, 3 pull over
  (driver `559BA807` = 0.02, taxi 0.04 per decision); target lane `+4380` [code]. Recomp [trace]: cars **wait** behind a
  blocking skater (no lane change / overtake seen in 152302), queue and go in step.
- Parked: StayingParked; pull out after the driver's `988BB0F6` (30 s, taxi 20 s) and a clear road, never while the
  alarm sounds (`sub_82C3A3A8`) [code]. Parked cars come from pulling over; no spawn-parked path found (open).
- **Horn decider `sub_82C40660` -> `+3420` (the audio horn state)** [code]: 6 = alarm (parked + alarm flag); otherwise
  only when `+4401` bit 0 (honk allowed): junction state 5 -> 3; blocked time `+3704` > driver `BC1A827C` (4 s, taxi
  1 s) -> 4 or 5; obstacle ahead (`+4402` bit 0x08, `+3616` = 0) present for `+3708` > `FE83E2E0` (2 s) -> 2 and the
  obstacle entity is notified through its vtable +100 (the ped's `IsBeingHonkedAt` / `RunFromHonker` input);
  approaching an obstacle within 2 x the gap at more than `40540E1A` km/h (5, taxi 10) -> 1.
- Skids: the audio skid flag (`+160` of the audio record) follows hard braking; the writer is `sub_824B2A28`'s input
  (open: which vehicle bit). Trace: Traffic_Skid 139 starts / 8 min.
- Recomp speeds [trace]: moving median 5.8-6.0 m/s, p90 14.7-14.9 m/s, max 15-18.6 m/s.

### 1.6 Physics and contacts [code + trace]
- **Traffic cars are kinematic**: speed is integrated by the AI (`sub_82C3FF38`), position comes from the lane cursor
  on the road network; no wheel / suspension simulation drives them [code]. They have a collision interface (vehicle
  `+136`, vtable `0x82322218`) whose callback `sub_82C3C150` receives contacts (car alarm, "hit by" mask `+4248`,
  side bit `+4401` 0x20) [code].
- Skater vs car: the wipeout code has a separate vehicle contact term (debug string `0x820CFF14` "Vehicle Contact (%f)
  Skater Contact (%f) Ped ..."), Hall of Meat `VehicleWipeoutScorer`, challenge queries `GetNumVehiclesHit(OfType)`,
  text "Player collided with a vehicle" [code strings]. Landing on a roof works (the car is solid) and **the car stops**
  (obstacle ahead / on it) [trace 152302 at 85 s].
- Car vs car: no collision response needed; spacing comes from the planner (queues) [code + trace].
- Car vs ped: no knock-down path found; cars honk (kind 2) and the ped runs (`RunFromHonker`, ≤ 30 s) [code].
- Car alarm: ported on the audio side (`VehicleImpact` / `VehicleParked` / `CarAlarmRule`, 0.1 threshold, 8 s).

### 1.7 Skitching [code + trace]
- **Car side.** The vehicle interface slot 128 (`sub_82C34648`) sets `+4403` bit 0x80 ("attached this frame"); the
  per-frame pass `sub_82C34CD0` copies bit 0x80 into 0x40 ("attached last frame") and clears 0x80 again through the same
  slot [code]. So the skater side re-asserts the grab every frame, and **0xC0 = held**. The vehicle constructor
  `sub_82C3B7C8` and the reset `sub_82C3B268` clear both [code].
- While held [trace, 7 skitches in 164620 / 161156 + 6 in 160126 (deleted) + 47CAEDA0 in 153038 (deleted, summary kept)]:
  the car keeps running its planner (it still brakes for lights and cars: negative accelerations during skitches), but
  with a higher speed cap: from 0-9 m/s at the grab it reaches **14-17.6 m/s** (peak up to 22.7 m/s in 160126) in 3-11 s,
  acceleration ramping 0.2, 1.4, 3.0, 4.6, 4.9 m/s². Which cap field the 0xC0 state raises is open (V6 RE:
  `+3680` / `+3684` / spec fields; `+4402` bit 0x02 adds `+3684`).
- Release [trace]: 2 of 9 measured releases brake hard to a stop (−7.25 → 6.2 m/s², 47CAEDA0; −7.31 in 164620); the
  others resume normal driving at 6-16 m/s. The difference is the traffic situation (planner), not a skitch rule
  (hypothesis).
- **Skater side, already in our engine:** physical state `Skitching` = 104 (`skate-core::player::state`, state object
  1772); `CalcSuggestedState` (`0x82D8ADE8`) enters it from the ground when processed flags 2476 bit 21 (off-board
  hand flag `+304`) and 2480 bit 22 (first grab candidate published by `0x82D740F8`, record `+1888`, interactable
  `+2464`) are set and the exit countdown is 0; leaves on flag loss, wipeout or `IsOffGroundSkitching` (`0x82D8BBB8`);
  a 2 s post-skitch timer (`skitch_timer_1376`) carries the skitched object id; wipeout data has `skitch_contact`,
  `skitch_scalar`, `skitch_arms_scalar`, `skitch_acc_scalar` [code, already ported]. Graph names: `IsSkitching`,
  `IsSkitchingWithAbsorb`, `IsEnteringSkitch`, `SkitchingPosition`, `IsSkitchShimmying`, `SkitchingBehaviour`,
  `EnterSkitchingBehaviour`, `SkitchShimmyingBehaviour`, `SkitchAntic`; clips `B_{R,M}_SkitchPush{,_LH,_RH}_{INTO,CYC}`
  (`0x8227B770..`). Stats: `Furthest_Skitch`, `Longest_Skitch`. Peds: `SkaterIsSkitching` (street guards warn, Skate 2).
- So a skitch is a **grab-candidate interaction**: the car must publish a grabbable rear target (the grab spline /
  interactable record the skater's candidate search finds) and accept the per-frame "held" call. Grab range, side,
  speed limits, the button and the attach offset come from the candidate search and the skitch state; those reads
  are V6's RE (leads: `0x82D740F8`, `register_candidate 0x82762AB0`, the PhysState_Skitching class, `0x820991A8`
  debug strings "distToSkitch / speedToSkitch / timeToSkitch" unreferenced in the image).

## 2. Where our engine is
| need | exists | gap |
|---|---|---|
| census / population | shared core designed (docs 26/27, M1 of peds) | vehicle rules on it |
| data export | `livingworld` group (milestone 1): 29 vault classes, census grids incl. the vehicle layer, `roads.json` / `roads.bin` (segments decoded, nodes / intersections raw), waypoints | vehicle classes (`livingworld_vehicle_characteristics`, `_drivers`), the 18 car recipes -> GLB, lane samples + connectors, signal groups |
| kinematic bodies | `skate-dynamics` (Rapier island: kinematic proxies, spawn / remove), `physics::network::Proxies` (remote bodies in the skater's contact solve) | a car proxy (box / convex hull) keyed by vehicle |
| skater skitch | `PhysicalStateId::Skitching`, selector rules, input-phase timers, wipeout skitch terms | a grab candidate provider for cars, the attach constraint, the graph behaviours if missing |
| audio | `world_audio::TrafficAudio` + `game_audio` traffic host (#32): engine RPM, horn 1-5, alarm, skids, Doppler, model -> record table, nearest-4 within 40 m; `VehicleImpact` / `VehicleParked` / `VehicleAlarmStarted` | a publisher |
| mod surface | `sdk.world_audio.spawn('car1','traffic',...)` (audio-only cars), skate-dynamics Lua cars | `sdk.living_world.vehicles` |
| rendering | retail world shaders, multiplayer skinned looks for characters | a static car mesh path with tint and glass |

## 3. Architecture

### 3.1 Data (setup group `livingworld`, additions) [shared group]
1. Tables: add `livingworld_vehicle_characteristics`, `livingworld_vehicle_drivers` to `tables.json` (the vehicle
   rows of `livingworld_models`, `_entities`, `_census*` are already there). Names for known fields, `Hash_*` for the
   rest (a mod overrides by the same keys).
2. Roads: decode lanes (4 m samples per lane), intersections (quads, approaches, connectors with tangents and lengths,
   signal groups) into `roads.bin` v2: a compact graph `{segments, lanes[], connectors[], junctions[], signals[]}`,
   keeping retail ids. `skate-data::roads` parses it (format tests on synthetic blobs, data-gated counts on the disc).
3. Models: the 18 car recipes -> GLB (body + windows, materials tagged chassis / glass, LOD0) through the
   `native_roster` / recipe reader the peds M0 adds; tint palettes stay in the table export.
4. `--check-assets`: every vehicle census category resolves to entities with a model, spec and driver.

### 3.2 Pure rules: `skate-core::living_world::vehicles` (no ECS, no I/O)
- `VehicleCensus` on the shared population engine: ring / cull from the `vehicles` ranges, 2 attempts / 1 spawn per
  console tick (30 Hz, frame-rate independent), initial populate 6000 / 600 / 8-80 m, cap = census max x density
  (Free Play), gates (enabled, online, zombie), seeded RNG.
- `RoadGraph`: lanes, successors through connectors, lane snapping for a spawn point (nearest lane sample within the
  ring, direction from the lane), speed limit per lane.
- `LaneCursor`: segment, lane, distance; advance by speed x dt; pick a connector at a junction (seeded); cubic curve
  evaluation for connectors; lane change as a lateral blend over the spec's distance.
- `SignalClock`: per junction phase sequence from `livingworld.trafficlights` (green 7 / 8, amber 1, all-red 0.5) with
  a deterministic offset; queries "may enter" for an approach; the peds use the same clock for walk signs.
- `Driver`: the ported planner (`sub_82C3FA08`), integrator (`sub_82C3FF38`), horn decider (`sub_82C40660`),
  manoeuvre decider (`sub_82C41CD0`), impatience and pull-over / park / pull-out timers, each a pure function of a
  `VehicleBrain` struct (the retail fields by name: accel, speed, cap, gap, timers, flags) and the spec / driver
  records. The state graph runs on the shared `skate-core::graph` host with `Vehicle.xml` from the user's install
  (as the peds do), with a Rust behaviour / condition registry (7 states, conditions `82C39CF0..82C3A850`).

### 3.3 Engine: `skate-game::living_world::vehicles`
- Components: `Vehicle { slot, entity_record, model, spec, driver, seed }`, `VehicleBrain`, `LaneCursor`,
  `VehicleTint { chassis, secondary }`, `TrafficAudio` (existing), `VehicleProxy` (collider handle), `Skitchable`.
- Systems (FixedUpdate before the skater physics, console-cadence normalised): census -> spawn / cull (pool, entities
  hidden and reused), brain tick, cursor advance -> `Transform` (position on the lane at ride height, heading from
  the lane tangent, pitch from the samples), proxy update (kinematic, velocity set so contacts see the motion), audio
  publish (`TrafficAudio.speed` = `+3412`, `load` = `+3408`, `horn` from `+3420`, `skidding` from the brake rule),
  `VehicleParked` while StayingParked, `VehicleImpact` from proxy contacts, reaction to `VehicleAlarmStarted`.
- Obstacles: the look-ahead sees cars ahead on the same lane / connector, the local skater, NPC skaters and peds in
  the lane corridor (that is what stops cars for the skater and makes them queue).
- Renderer: static GLB per model, chassis tint from the palette (seeded pick), glass material, LOD0 to 75 m then a
  cheaper draw (retail has one LOD; we keep one and cull at the census 110 m).
- Collision: one kinematic box / convex hull per car from the GLB bounds (or `F983F2518B335286` once confirmed) joined
  to the skater's contact solve through the `physics::network::Proxies` mechanism; landing on a roof works through
  the normal ground contact; the skater's wipeout uses the vehicle contact term.

### 3.4 Skitching (car side + skater side)
- Car: a `Skitchable` grab target (the rear bumper spline / points, from the model bounds) published to the skater's
  grab-candidate search; a `held` flag set every frame by the skater while attached (0x80), latched to 0x40 by the
  vehicle pass, exactly as retail; the planner raises its cap while held (field from V6 RE) and handles release
  through normal planning.
- Skater: the existing `Skitching` state; the attach constraint holds the board at the car's rear target with the
  retail offset; speed comes from the car (the board is carried); steering / shimmy from the skitch behaviours; release
  on button up, wipeout, `IsOffGroundSkitching`, or the target disappearing (car culled / turning away beyond the
  limits); the 2 s post-skitch timer and stats (`Furthest_Skitch`, `Longest_Skitch`).

### 3.5 Multiplayer
- Retail: no traffic online [code]. Default online = no traffic. Opt-in (mod / setting): host-simulated, clients get
  (slot, entity, lane cursor, speed, accel, horn, tint) at a low rate plus events; cursor extrapolation on clients.

## 4. Mod surface (designed in)
Same patterns as the peds / NPC skaters (`sdk.living_world`), plus vehicles:
- **Content** (`living_world.json`, capability `living_world_content`): override any vehicle record by retail name
  (census caps / weights, `livingworld_vehicle_characteristics`, `_drivers`, `livingworld_models` tints, the road
  speed limits, `livingworld.trafficlights`), add entities / models (own GLB + spec + driver + `aud_traffic_engine`
  record or a mod sound), per-map roads for custom maps (`<map>.living_world.json`: lanes as polylines, junctions with
  connectors and signal phases).
- **Runtime** (capability `living_world`): `sdk.living_world.vehicles.spawn(key, {entity, lane | position+heading,
  speed, parked})`, `despawn`, `query{radius}`, `set_density(scale)` (Free Play scale), `set_census(record, patch)`,
  `set_driver(key, patch)`, `force_state(key, 'PullingOver' | ...)`, `route(key, {lanes...})`, `honk(key, kind)`,
  `register_behaviour / register_condition` (shared), `info()`. Keys are owned by the calling mod with per-mod and
  global limits and removed on disable (the shape the andrewnakas `mx/vehicle` SDK uses for player cars: keys per
  mod, limits, removal on disable; described as prior work and credited, not copied).
- **Events** (capability `living_world_events`): vehicle_spawned, culled, honked {kind}, stopped_for {who},
  parked, pulled_out, impact {speed, by}, alarm, skitch_grab {vehicle, skater}, skitch_release {reason, distance,
  duration}.
- **Skitch tunables as data**: grab range / angle / speed limits, attach offset, held speed cap, release rules
  default to the retail values (V6) and are overridable per vehicle entity and globally.
- **A Lua-driven car joins traffic**: a mod vehicle (skate-dynamics car) can register as an obstacle and as
  `Skitchable`, and publish `TrafficAudio` (already possible through `sdk.world_audio`).
- **Cleanup**: everything a mod spawned or overrode is removed / restored on disable (epoch scheme of world audio).

## 5. Retail-parity plan
| item | source of truth | check |
|---|---|---|
| ring 80-100, cull 110, 2 / 1 per tick, initial 6000 / 600 / 8-80, caps 30 / 25 / 10, weights, Free Play scale, online / zombie gates | code + data | unit tests on the exported tables; headless census run vs code bounds; recomp sanity (17-27 vehicles per session) |
| lane speeds, lane counts, junction connectors | data | data-gated decode tests (76 segments, connector counts per junction; V0 `tests/roads_data.rs`) |
| light phases | data + trace | unit test: 0.5 / 8 / 0.5 / 7 / 1 cycle; cars stop on red; peds cross on walk |
| planner / integrator / horn / manoeuvres | code | unit tests per formula with the code constants; replay fixtures from VEHSTATE (164620, 161156): speed from accel within 0.1 m/s |
| stopping for the skater, queues | code + trace | headless: a scripted skater stands in a lane; cars stop behind it and queue (152302 numbers as a sanity range: stops 2-5 s, holds 14-20 s while blocked) |
| skitch | code + trace | unit tests of the 0x80 / 0x40 latch; fixture windows from `skitch_fixture.py` (peak 14-17.6 m/s in 3-11 s); the user's session last |
| audio | ported (#32) | existing tests + the bridge fed by the vehicle system |
| looks | data | the user in game |

## 6. Test plan
- `skate-core` unit tests (no data): census rules (ring, cull, attempts, initial populate, cap x density, gates),
  `SignalClock` cycles, `LaneCursor` (advance, connector curve length vs stored length, successor choice determinism),
  planner cases (free road, following a lead, stop line with margin, state-4 instant stop), integrator clamp and
  zeroing, horn decider truth table (kinds 1-6 with the driver thresholds), skitch latch, frame-rate independence at
  30 / 60 / 144 fps.
- `skate-data` tests: `roads.bin` v2 parser on synthetic blobs; data-gated: segment / connector / junction counts per
  district, every lane sample within its tile bbox, every connector start / end on a lane end.
- Python tests for the export additions (fixtures, no game data).
- Trace fixtures (numbers only, no game data): `skitch_164620.json`, `skitch_161156.json` (from the recomp sessions; added with the skitch tests)
  (from `skitch_fixture.py`), VEHSTATE speed series of a few cars as JSON for the planner replay. The raw
  `153038` session (vehicle 47CAEDA0) is deleted; its derived summary `veh_47CAEDA0.txt` stays as a sanity range.
- Headless e2e (muted): DownTown, stand at a junction 120 s: vehicle count within caps, spawn / cull distances, cars
  stop on red, no car inside another, no NaN; frame time budget (30 cars x brain + proxies).
- `regression-check`: all maps load, parks have no cars, skater feel unchanged with traffic off (state-log identity),
  audio unchanged when traffic is off, multiplayer smoke (no traffic online).
- Dev mod `mods/living-world-test`: spawns cars on a loop, overrides density, listens to events (incl. skitch).

## 7. Milestones inside the ONE PR
Sizes: S ≤ 1 day, M 2-4 days, L 1-2 weeks of work.
| # | milestone | size | depends on |
|---|---|---|---|
| V0 | Data: vehicle tables in `tables.json`, 18 car GLBs (+ tints), `roads.bin` v2 (lanes, junctions, connectors, signal groups), `skate-data::roads`, `--check-assets` | M | peds M0 (group, recipe reader) |
| V1 | Road graph + lane cursor + signal clock in `skate-core` (unit tests; RE: lane snapping in the vehicle factory, `livingworld.roadnetwork` fields, signal group mapping) | M | V0 |
| V2 | Vehicle census on the shared population core (rules + gates + Free Play scale), pool, debug overlay | S | shared core (peds M1), V1 |
| V3 | Cars on screen: renderer (static mesh, tint, glass), kinematic movement on lanes at lane speed, lights obeyed, `TrafficAudio` publishing (engine + Doppler audible) | M | V2 |
| V4 | Driver: planner, integrator, look-ahead (cars, skater, NPCs, peds), queues, horn decider (+ `IsBeingHonkedAt` to peds), skids, manoeuvres (lane change / overtake / pull over), park / pull out, `VehicleParked` / `VehicleImpact` / alarm read-back; state graph on the shared graph host | L | V3, peds M4 graph host for the shared runtime |
| V5 | Physics: car proxies in the skater's contact solve (roof landings, bails with the vehicle contact term, Hall of Meat scorer hook), cars stop when hit / blocked | M | V3, peds M5 proxies |
| V6 | RE for skitching (static, no game): grab-candidate search for cars (range, side, speed, button), attach offset, held speed cap field, release rules, the skater-side behaviours the graph names; write findings to the notes | M | none (can run in parallel from now) |
| V7 | Skitching: `Skitchable` car target + held latch + raised cap; skater side on the existing `Skitching` state (attach, carry speed, steer / shimmy, release, 2 s timer, stats); events; tests with the fixtures | L | V4, V5, V6 |
| V8 | Mod surface for vehicles (content overlay, runtime API, events, skitch tunables, cleanup), `check_mod`, docs, multiplayer default (none online, opt-in snapshots) | M | V4, shared M7 |
### 7b. Skitching milestones in detail (V6 research, V7 build)
| aspect | known now | V6 reads (static) | V7 builds / tests |
|---|---|---|---|
| grab conditions | the skater enters `Skitching` when the off-board hand flag (2476 bit 21) and a published grab candidate (2480 bit 22, `0x82D740F8`) are set [code, ported] | candidate search range, side / angle window, max relative speed, the button (hand grab input), car-side target geometry | `Skitchable` target on every car; unit tests for the window |
| attach offset | board carried by the car [trace: car and skater move together for up to 348 m] | offset of the hand / board to the car target, shimmy range (`IsSkitchShimmying`) | attach constraint; test: offset constant over a fixture window |
| car while held | `+4403` 0xC0 latch (`sub_82C34648` sets 0x80, `sub_82C34CD0` latches 0x40) [code]; speed-up to 14-17.6 m/s in 3-11 s, accel ramp to ~4.9 m/s², still brakes for traffic [trace] | which cap field 0xC0 raises (`+3680` / `+3684` / spec), who calls slot 128 (the skater's skitch update) | latch unit test; replay test: speed curve within the fixture ranges (`skitch_164620.json`, `skitch_161156.json`) |
| release | button up, wipeout, `IsOffGroundSkitching` [code, ported selector]; car brakes hard (≈6.2 m/s²) or keeps driving [trace] | release by distance / angle / speed, collision release, what makes a car brake after release | release reasons as events; test the 2 s post-skitch timer |
| skater side | state 104, input-phase skitch timer, wipeout skitch terms [code, ported] | the skitch behaviours' steering and push (`B_*_SkitchPush_*` clips), speed handed back to the board on release | animation set, steering, speed carry-over; state-log identity when traffic is off |
| mod hooks | event and tunable shape in §4 | none | `skitch_grab` / `skitch_release` events, tunables as data, mod cars skitchable |
| data | sessions 164620 (5) and 161156 (2) clean; 153038 raw deleted (summary only) | none | D1 session settles what the code leaves open |

Order of value: V0-V3 give visible, audible traffic that obeys lights; V4-V5 make it retail; V6-V7 add skitching;
V8 finishes the PR. V6 can start immediately (static RE only).

## 8. Risks
- Road decode depth (connectors / signal groups are hypotheses): mitigated by the probe tool and data-gated tests.
- Skitch feel depends on exact retail numbers that sit in skater-side code not yet read (V6) and on a session (D1).
- Performance: 30 cars + 31 peds + NPC skaters; cars are cheap (kinematic, one proxy), the planner is O(cars on lane).
- Our renderer has no vehicle material path yet; retail shaders `vehicle_*` must be mirrored (tint + glass + lights).
- The recomp is not retail: no tuning to recomp timings; fixtures are sanity ranges, code constants win.

## 9. Data requests for the user
Only what code, disc data and the existing sessions cannot answer. All are user-played sessions in the recomp with
`.local\recomp\PLAY_TRACE_ALL.bat <label>` (categories audio, npc, world, traffic, aiskater, dsp; screenshots on).
Each needs the hooks proven first in a background run of ours (memory "verify hooks before sessions"); the new
hooks they need are listed in `.claude/todo/vehicles.md`.
- **D1. Skitch session (needed for V7), ~5 min, DownTown streets (e.g. near Aletown / Hotel District).** Grab 6-8
  cars: from a stop, at walking pace, at full pushing speed; from the left rear, centre and right rear corners; try to
  grab a car that is too fast or too far (and say so out loud or note the time); hold one skitch through a junction
  and a turn; release by letting go, by bailing, and by riding until something forces you off; one skitch on a taxi.
  Needs the new SKITCH hooks (grab candidate, attach offset, release reason) plus VEHSTATE.
- **D2. Car contacts (V5), ~3 min, any road with traffic.** Ride slowly into the side of a moving car, hit one hard
  head on at speed, ollie onto a roof and ride off, stand on foot in front of a car until it honks, and push a ped
  into the road in front of a car if you can (does a car ever hit a ped?). Needs the VEHHIT hook (already written)
  and a new bail-cause hook on the vehicle contact term.
- **D3. Junction watch (V1 / V4), ~3 min, standing still on the corner of a 4-way junction with lights in DownTown.**
  No input for 3 minutes, then cross on foot twice. Gives light phases per approach, queue lengths, turns at the
  junction (which connector), honks and peds waiting at the crossing. Needs a TRAFLIGHT state hook (junction id,
  phase, time) and a VEHCONN hook (connector choice).
- **D4. Visual question (no trace, a yes / no from memory or a screenshot):** are there visible drivers inside the
  ambient cars, and do cars have working head / tail lights or brake lights? (No driver model reference exists in the
  vehicle data; the `vehiclelight` shader exists.)
- **D5. Free Play traffic levels (optional, V2 check), ~2 min:** set Free Play Traffic to Low, stand on a DownTown
  street 60 s, then High for 60 s. Confirms the density scale against the code. Needs only VEHSTATE.
