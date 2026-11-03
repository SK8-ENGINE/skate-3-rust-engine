# 26h: Living world: Traffic

Part of doc 26, the living world (index: [26-living-world.md](26-living-world.md)). Branch `world/living-world`.

## Change: milestone V0, vehicle data

What retail ships, read from the disc and the code for this milestone (details and formats:
[`living-world/vehicles-data.md`](living-world/vehicles-data.md)):
- **Cars** [data]: 18 vehicle recipes (`livingworld.big`), each a body (`Accessory`) and windows (`Equipment`) on an
  8-bone rig (root, chassis, six wheel bones), one LOD, materials `vehicle_chassis` / `vehicle_glass`; positions are
  half floats. The visible **drivers are modelled into the body mesh** (a dark low-poly figure in the left front
  seat); there is no driver model or texture, and no light material (no head / tail / brake lights).
- **Palettes** [data]: `livingworld_models` gives each car model 2-10 chassis colours and 1-10 secondary colours; the
  base records hold blue / red, the colours of the body atlas's paint mask.
- **Specs and drivers** [data + code]: `livingworld_vehicle_characteristics` (6 records) and
  `livingworld_vehicle_drivers` (5); fields with a reading code site are named (horn timers and speed, pull-over
  chance, parked time, alarm impulse / duration, the following rule's speeds), the rest keep `Hash_*` names.
- **Road network** [data, all 82 objects]: decoded completely: per district directed segments (lane runs go from
  `node_b` to `node_a`, so `node_a` is the destination), lane geometry as about 4 m Hermite pieces with edges and
  arc-length tables, junctions with 8 end records (the approaches and exits of node ends 0-3, each lane listing the
  turn connectors it may take) and the turn connectors (Hermite curves from an approach lane to an exit lane):
  DownTown 46 segments / 19 junctions / 88 connectors, Industrial 26 / 10 / 46, University 4 / 4 / 4. Milestone 1's
  77th segment was a phantom (the header's second count is the lane-run count, the third the segment count).
- **Traffic lights** [code `sub_826B1540`]: 4 signal controllers, each with two light groups and the cycle all-red
  0.5, green 7, amber 1, all-red 0.5, red 8 s (the other group's green + amber), odd controllers offset by half a
  cycle; values from the `livingworld.trafficlights` record. Which approach uses which controller and group is not in
  the road data (V1 reads the binding from the code).

Change:
- Setup group `livingworld` (same group, same vault conversion; only its fingerprint changes): `tables.json` gains the
  two vehicle classes, `vehicles/<recipe>.glb` (18 cars: rig, body / windows / wheels primitives tagged with their
  role and material kind, diffuse and normal maps), `vehicles.json` (models with palettes and stable palette ids
  `<model>/chassis/<i>`, entities with model / spec / driver, the vehicle census resolved to recipes), `roads.bin` v2
  (a little-endian road graph), `roads.json` v2 and `roads_raw.bin` (the old verbatim pack, unchanged). The export
  fails the group (world runs empty) if a painted vehicle census record does not resolve to built cars with a spec
  and a driver. Recipe reader: the LOD word is the material instance count (two on `reda_car`'s wheels); single-
  instance output is unchanged, so every ped file stays byte-identical.
- `skate-data::roads`: `RoadGraph::parse` / `to_bytes`, lookups by retail id (segment, junction), the connectors a
  lane may take at its destination, the segment a connector leads onto, Hermite evaluation and arc-length
  parameters. No I/O, no engine types.

Moddability: every value stays setup data a mod overrides by key (tables by class / record / field, palettes by
stable id, cars by recipe name with the same bone names and primitive tags, roads by retail id with
`RoadGraph::to_bytes` for custom maps); the mod surface itself is V8. Multiplayer: every car model, palette entry,
segment, junction and connector keeps a stable retail id, and the export is deterministic (two runs byte-identical).

Files: `tools/asset_pipeline/{living_world_vehicles,living_world_roads,test_living_world_vehicles}.py`,
`tools/asset_pipeline/{living_world,versions}.py`, `crates/skate-data/src/{roads,lib}.rs`,
`crates/skate-data/tests/roads_data.rs`, `docs/hails-additions/living-world/{vehicles-data,vehicles-design,peds-data}.md`.

Verification (milestone V0):
- `py -3.13 -m unittest tools.asset_pipeline.test_living_world_vehicles`: 13 OK; all pipeline tests (every
  `tools/asset_pipeline/test_*.py` + `tools.test_setup_assets`): 190 OK, 2 skipped.
- `cargo test -p skate-data --release --locked --lib --tests`: all pass; with `SKATE3_ASSET_ROOT` on the export,
  `--test roads_data` (3) and `--test living_world_data` (4) pass: shipped counts, continuous pieces ending at their
  junction, every connector in its lane lists and leading onto a segment, every lane into a junction has a way on.
- Export on the user's disc: 35 s, 18 / 18 cars, validation clean; ped files and GLBs byte-identical to milestone 1;
  `roads_raw.bin` byte-identical to milestone 1's `roads.bin`; fingerprints of `core`, `hud`, `character`,
  `environment`, `maps`, `audio` identical before and after.
- Rendered the taxi and sports_car_01 without their windows: the driver figure sits in the body mesh.


## Change: milestone V1, road graph, lane cursor, traffic signals and junction entry

**Problem.** Traffic needs the road network as a graph a car can drive on, retail's traffic lights and retail's rule for
when a car may enter a junction, before the census (V2) and the driver (V4) can use them.

**What retail does (code read, TU3 recomp as reference).**
- Road network object (global `0x830854C0`, vtable `0x8230C6A8`): `+8` vehicle by id, `+16` junction by node, `+24`
  lane run by id, `+32` segment by id, `+36` / `+40` signal controller by index (`sub_8269B2F0`, controllers 320 bytes
  apart) [code].
- **Signal binding** [code `sub_82E11E90`]: the controller an approach obeys is the connector's `from_end` (connector
  `+84`), so every signalled junction shares the 4 controllers: node ends 0 / 2 run together, 1 / 3 together, the whole
  city switches in step. The light is consulted only when the junction's `flag_04` (header `+0x244`) is set, so
  `flag_04` = "has traffic lights" (24 of 33 shipped junctions) [code + data].
- **Signal programmes** [code `sub_826B1540`, `sub_82E156D8`]: even controllers all-red 0.5, red 8 (= green + amber),
  all-red 0.5, green 7, amber 1; odd controllers all-red 0.5, green 7, amber 1, all-red 0.5, red 8. Walk list: a green
  splits into walk green `(1 - 0.4) x 7` = 4.2 and walk amber `7 x 0.4` = 2.8, every other phase is walk red. Jump
  targets `+304` / `+308` (all-red before red) and `+312` / `+316` (all-red before green).
- **Signal tick** [code `sub_82E158D8`]: a fixed 1/60 s per call (`0x820849C8`); below 0 the next phase starts and
  inherits the overshoot, except on the car list's wrap to phase 0 (overshoot dropped, walk list forced to phase 0).
  Called for all 4 controllers by the manager `sub_826B2C18` from the living-world update `sub_826BDB50` in the world
  tick `sub_82859E70`, which runs as a **fixed 60 Hz step** [trace: 60 ticks per second while the recomp renders at
  ~345 fps], so the cycle is 17 s of game time. The f32 carries make the odd controllers' walk light lead the car light
  by one tick (253 / 167 / 1 / 59 ticks), exactly as recorded.
- **Green wave** [code]: a priority vehicle (`sub_8269B328` sets it, `sub_8269B338` clears it; caller not found) within
  50 m (`0x8220E13C`) of a signalled junction, or inside one, calls `sub_826B3C88(end)`: controllers of the end's parity
  jump to their all-red before green, the others to their all-red before red, and the lights freeze (`+13382`) once
  the end's controller is green, until the priority is cleared.
- **Connector choice** [code `sub_82C376E8`]: when a car has no connector or changed segment it picks one from its
  approach lane's list and stores it at `+4388`. With vehicle flag `+4401` bit 0x02 (constructor `sub_82C3B7C8` writes
  0xCE, no code clears it, so every car) the connector whose exit lane has the smallest `load / exit length` wins (exit
  segment `+136`, a vec4 per lane; strict `<`, first wins a tie). The other branch (flag clear, unused) picks
  `trunc(u32 x 100 / 2^32) mod n` from the world RNG (`0x822F94B0`).
- **Junction entry** [code `sub_82E11E90`, run by FollowingLane once the stop line is within look-ahead + speed x 1 s;
  conflict scans `sub_82E11C78` / `sub_82E11980`]. Results (the VEHJUNC codes; stored as junction state `+4392`):
  - 0 go;
  - 1 signal: red or amber, or green with `speed x remaining green < distance to the line - length / 2`; a right turn
    (`to_end == from_end - 1`) skips the light (turn on red). A car stopped at the line therefore only goes on green when
    its front is within half its length of the line;
  - 2 approach: faster than the connector's entry speed (connector `+0x50`, V0's `f32_50`: the road speed on straight
    connectors, 1.4-2.7 m/s on turns) while still beyond its look-ahead; at an unsignalled junction the entry speed is
    0.1 m/s (`0x820641A8`), a stop sign;
  - 3 yield: another connector from my lane in use; a car inside the junction merging into my exit lane (left turns
    check lanes up to theirs, right turns and straight on the lanes from theirs outwards); or a crossing / oncoming flow.
    Cars still on their approach count only by distance (closer first, a tie goes to the lower id), at unsignalled
    junctions for every flow and at signalled ones for the oncoming flow of left turns and straight on. A blocker with
    mover flag `+96` (the skitch speed-cap bit) makes FollowingLane set state 5 (horn 3);
  - 4 blocked: no room on the exit lane (`my length > rear car distance + speed x 1 s - its length / 2`), the car ahead
    on my connector closer than my minimum gap, or a missing approach / exit segment.
  - The light check is skipped (`r7 = !sub_82C344D0`) while the player skitches the car (`+4403` bit 0x80) or for a car
    with `+4401` bit 0x10 faster than the driver's `Hash_F142ABBFBEDA71E2` km/h (taxi 40); no code read sets bit 0x10.
- **Lane leader** [code `sub_82E14EE0`]: each lane of a segment (`+104`) and each connector (`+8`) lists its cars newest
  first; the leader is the next older car, none for the front car, the rearmost car for one not on the lane.
- Lane geometry [data]: a piece's centre curve is the road middle; lane `k` of `n` sits at `(k + 0.5) / n` from the left
  edge (4 m lanes, lane 0 left); connectors start and end exactly on those lane points. Right turns leave from the right
  lane, left turns from the left lane.

**Evidence.** proof1 / proof2 VEHJUNC (89 result changes): straight on red / amber always 1, green 0 (1 when the green
cannot be cleared), right turns never 1, unsignalled right turns 2, light skipped with the flag off. TRAFLIGHT2: all
500 recorded phase changes reproduced tick for tick. TRAFPROG: the programme layout above.

**Change.** `skate-core::living_world::traffic`:
- `RoadNetwork::build(&RoadInput)`: segments / junctions / connectors sorted by retail id (stable dense indices on every
  machine), ends resolved to segments, references checked; `lane_frame` (fractional lanes for lane changes),
  `connector_frame`, `next_connectors`, `connector_exit` / `approach`, `adjacent_lanes`, `nearest_lane` (engine helper).
- `LaneCursor`: lane or connector plus distance; `advance` carries the overshoot across pieces, onto the chosen connector
  and onto its exit lane (choosing the next connector on entry, as retail); stops at dead ends; `set_lane`.
- `choose_connector` with `ConnectorChoice::{LeastLoaded (retail), Random}` and a load function.
- `SignalClock`: 4 `Controller`s built from `SignalTimings` (data), the f32 tick, a 60 Hz accumulator so any engine
  frame rate gives the same ticks, phase-change records, `controller_for_end` / `light_for` (binding), `walk_for_end`,
  `request_green` / `clear_priority` (green wave) and `priority_end`.
- `junction_entry(&EntryQuery)` returning `Entry::{Go, Signal, Approach, Yield, Blocked}` plus the flagged-blocker bit;
  `Occupancy` (newest-first lists, `leader`, `lane_load`); `VehicleSnapshot` (the mover values the query reads).
`skate-data::roads`: `RoadGraph::traffic_input()`, `signal_timings(&tables)`.

**Moddability.** Durations come from the `trafficlights` record (a content overlay changes the city's lights;
`SignalClock::set_timings` rebuilds live); `ticks_per_second` is a field; the connector choice takes a policy and a
load function; `request_green` gives a mod (or a scripted car) retail's green wave; `RoadInput` is plain records a mod
map can fill (or `RoadGraph::to_bytes` for the file format). Stable ids: `SegmentId`, `JunctionId`, `ConnectorId`
(junction + retail index), `LaneId`.

**Multiplayer readiness.** No hash-map iteration, no frame-time dependence; the light state is a pure function of the
tick count and timings (a client can rebuild it from the host's `SignalClock::ticks`); random draws take an explicit
seeded `Rng`.

**Tests.** 18 unit tests on synthetic graphs (graph build / rejects, lane frames, cursor continuity across pieces,
connectors and segments, connector choice, programme layout, recorded tick gaps, 30 / 60 / 144 / 240 / 29.97 Hz
identical, controller alternation, approach binding, green wave, junction results for lights, stop sign, yields,
blocks, distance and id priority, left turn vs oncoming, leader, priority distance); 1 skate-data unit test; 6
data-gated tests (shipped counts and 24 signalled junctions, connector endpoints on lanes, every lane reachable,
cursors over the whole network, 4 controllers with the shipped timings, recorded timelines from the traces).

**Open questions.** See the list below.

## Change: milestone V2, the vehicle census
Ported from the code: the census vehicle pass `sub_826B7760` / `sub_826B9B90` (ring 80-100 m, cull 110 m, 2 attempts /
1 spawn per pass, initial 6000 / 600 in 8-80 m, cap per district x Free Play traffic, none online / in zombie mode /
with the census enable bit clear), the entity roll (`sub_826B8B88`), the **vehicle limit of 15** (census `+148`,
`sub_826B83C8`: DownTown's cap of 30 never fills; the recomp never showed more than 15 cars), and the factory's road
placement (`sub_82C36300`): the car goes on the road surface under the ring point (point in a piece's road
triangles, `sub_826B3B18`), at that piece's start, at least 15 m from both segment ends, on a lane where the next car
ahead starts 15 m further and the car behind ends 7.5 m (plus 1 s of its speed) earlier (`sub_82E14928`), one fitting
lane picked at random, then a 20 m overlap check. Cars drive their segment's direction; no heading is drawn.
Spawn records (`SpawnChoice::Vehicle`) carry the census record, category, entity, model, palette indices
(`vehicles.json` `palette_ids`), the retail segment id, lane and distance: everything a client needs. Rules are data
(`PlacementRules`, `CensusKindConfig` limits) a mod can override; `LivingWorld::update_lane` is the V3 hook.
Files: `skate-core/src/living_world/{traffic/spawn.rs, census.rs, config.rs, population.rs, mod.rs}`,
`skate-data/src/living_world.rs` (`vehicle_catalog`), `skate-data/tests/vehicle_census_data.rs`,
`skate-game/src/living_world/mod.rs`. Open: palette pick (not in the code read; seeded per id), spawn speed (0),
the overlap extents formula (radius = half the larger side), the pool size behind the limit, the meaning of
`FFE5E258BD468196` (census `+144`).

## Change: milestone V3, cars on screen
Census cars become visible, moving traffic that obeys the lights and is heard through #32's engine audio.

- **Follower** (`skate-core::living_world::traffic::follow`): per 60 Hz world tick, every car (in key order) runs
  the V1 junction query once the stop line is within its look-ahead plus one second of speed, brakes to the line on
  Signal / Yield / Blocked (`-v^2 / (2 (d - f2) + 0.001)`, `sub_82C3FA08` [code]), slows to the connector's entry
  speed on Approach, keeps `min_gap` behind the car ahead on its path (same lane, its chosen connector, the exit lane),
  and integrates speed with retail's integrator (`sub_82C3FF38` [code]: speed += accel x dt, accel 0 above the cap;
  cap = lane speed limit, 14.167 / 13.889 m/s [data], 17.0 while skitched [trace, V7]). Pull-away accel ramps at
  0.8 m/s^3 [trace] up to the spec field `Hash_328B9F4685A14018` (2.0-3.1, [data], meaning a candidate); planning
  decel = `Hash_758229215579C6D1` (2.5-3.0, candidate); hardest braking 7.3 m/s^2 [trace]. Connectors by retail's
  least-loaded rule (V1). Cars at a dead end leave.
- **V3 simplifications until V4**: cars are the only obstacles (skater, NPCs, peds are V4 / V5); the look-ahead is the
  comfortable stopping distance (retail `+3516` not read); following is "brake to the lead's speed by `min_gap`
  (2 m, engine value)" (the "lead-minus-20-km/h term" read here is in fact the skater-behind rule, see
  "Skitching step 6"); a car that got Go and can no longer stop comfortably commits
  (amber dilemma zone); no lane changes, overtakes, horns, parking, skids.
- **Engine-side safeguard, not retail**: two lanes of one approach merging into one exit lane. Retail's junction
  query never scans the car's own approach (`sub_82E11E90` passes only ends from_end + 1 / + 2 / + 3 to
  `sub_82E11C78` / `sub_82E11980` [code]); the spacing comes from the look-ahead (V4). Until then the follower treats a
  car inside the junction on another connector into the same exit lane, nearer the exit, as the car ahead.
- **Look** (`skate-game::living_world::vehicles`): the car GLB as a scene, glTF materials on render layers 0 / 28 (the
  mod-graphics path); wheel bones spin by distance / `wheel_hint` [data]. Drivers are in the body mesh, cars have
  no lights [data, V0]. Paint and glass: superseded on 2026-10-11 by the retail vehicle shaders (see
  [Car materials](#car-materials-the-retail-vehicle-shaders-2026-10-11)); V3 tinted a texture copy with an inferred
  rule and a 2.0 gain and drew the glass at 0.55 opacity.
- **Collision**: a kinematic box per car from the GLB bounds joins the skater's contact solve
  (`physics::network::Proxies`); solid only, bails and roofs are V5.
- **Audio**: `TrafficAudio` per car: engine = the entity's spec `engine_audio` record, speed = `+3412`, load = the
  commanded accel `+3408`; `AudioVelocity` for Doppler.
- **Multiplayer (no networking)**: a car's motion is a function of its spawn record, the world tick, the signal
  clock's tick count (one per world tick since the world loaded) and the other cars; the connector choice reads the
  occupancy, so a client runs the whole car set from the host's spawn / despawn records (plus the host's tick and
  signal tick count) rather than one car. Retail runs no traffic online [code]; that default stays.
- **Mod surface**: `VehicleOverrides` (model per entity, GLB per model, colour per palette id, follower numbers per
  entity, connector choice, tint gain) and `TrafficEvent` (spawned, despawned, junction answer, entered junction,
  entered lane); `sdk.living_world.vehicles` bindings are V8.
- **Tests**: 7 core follower tests, 7 headless engine tests, 1 data-gated DownTown run (30 cars, 120 s, 0 entries on
  red, no overlaps, moving median 6.4 m/s / p90 14.2 vs the recomp's 5.8 / 14.7).

## Cars flying off, population gone, 2026-10-05

**Problem.** First play video of the living-world build (DownTown): "At a certain point in the first map shown
the cars leave the ground and fly off and all peds and vehicles cease to exist." The session log is empty; the
video is the only record. User: "The cars appear to fly away when I jump as im going uphill, not clear if its
related."

**Evidence.**
- [video] Frames 21.5-24.5 s (4 fps, zoomed): a green and a red car hang about two storeys up in front of the
  building on the left of the uphill street, moving left, while other cars further away sit on the road. From
  about 28 s no cars and no peds are seen until the map is changed; the frame rate falls from ~290 to ~50.
- [data] The road graph itself is clean: no non-finite values, no height steps (worst lane height vs its own
  road edges 0.09 m, connector bulge 0.19 m), connectors meet their lanes within 0.1 mm.
- [data] Compared with the DownTown ped walk mesh (`navmesh.bin`), every DownTown lane point lies on the ground
  (2093 points, worst 1.11 m), but the other districts in the same `roads.bin` overlap DownTown in x / z at other
  heights: University roads 16-18 m **above** the DownTown ground (segment `6A581EC75CE57FC5`, 820 m long, and
  two more), Industrial roads 47-59 m below it. 1173 of 3284 lane points of the whole file are off by more than
  1.5 m. The districts are separate worlds sharing coordinates.
- [code, engine] The world built its road network from the whole file (`traffic_input()`), and the car
  placement (`road_under`, port of `sub_826B3B18`) takes the first piece in network order whose road
  triangles hold the ring point horizontally. A ring point over an overlapping University road (when that
  road comes first in id order, or no DownTown road is under the point) put the car on that road, 16-18 m in the air; cars could also be routed across
  junction ends into another district. Inference, not read in the code: retail loads the road
  object per district, so its `sub_826B3B18` only sees the loaded district's roads.
- Nothing ties a car's height to the player after placement (pose = lane frame only); the ring point takes the
  focus height, but `road_under` ignores height, so the jump is most likely a coincidence.

**Root cause.** Floating cars: other districts' roads in the DownTown network (high confidence). Population
gone: not proven from the video. Found and closed on the way: an infinite focus position (or speed) put every
NPC beyond the cull in one pass (`dist2 = inf`), emptying the whole population at once.

**Change.**
- `skate-data::roads::RoadGraph::district_traffic_input(name)`: segments and junctions of one district only;
  a junction end whose segment is in another district becomes a map edge. `load_data` builds the network from
  the loaded map's district (a map with no road district gets no roads, so no cars, as before without a census).
  A mod's own road graph still goes through `traffic_input()` / `RoadNetwork::build`.
- Guard (engine, not retail): `census_pass` drops observer circles with a non-finite centre, cull or spawn
  radius; with none left the pass neither culls nor spawns, so a broken focus can never empty the population.
  The game logs `LIVING_WORLD census focus is not finite ...` when it happens. A traffic car whose state goes
  non-finite is removed alone with a `LIVING_WORLD traffic: car #N has a non-finite state ...` warning before it
  reaches the census.

**Files.** `crates/skate-data/src/roads.rs`, `crates/skate-data/tests/road_districts_data.rs` (new),
`crates/skate-core/src/living_world/{population.rs, tests.rs}`,
`crates/skate-game/src/living_world/{mod.rs, vehicles.rs}`.

**Verification.** `district_traffic_input_keeps_only_that_districts_roads` (skate-data unit),
`downtown_world_gets_only_downtown_roads_and_they_lie_on_the_ground` (data-gated: DownTown 0 points off,
whole file 1173 off), `a_non_finite_focus_never_empties_the_population` (skate-core; fails without the guard:
the infinite focus culls everything, passes with it; a real teleport still culls). skate-core living_world
92/92, skate-game living_world 28/28, `cargo build --locked` (dev) ok. Not yet seen in game.

**Open.** Why the population vanished in the video is not proven: the census focus is the board's deck body
(`gather_observers`), not the walking player, and the fix only guards a non-finite focus; a finite but wrong
focus (the deck left behind or moved while off the board) would still cull everything by the retail rules.
The FPS drop at ~28 s is not explained. A debug-log session (`SKATE_LIVING_WORLD_DEBUG=1`) of the same street
would settle both.

## Car shadows from a bridge printed on the ground below, 2026-10-08

**Problem.** Session 2026-10-07 12:14 (DownTown, player about [42.6, 15.8, 353]). User: "I did find a spot where
vehicle shadows from the bridge overhead were appearing below".

**Root cause.** Dynamic objects (skaters, traffic cars on layer 28, mod graphics) cast into one dynamic shadow map
that the baked world receives (`retail_character.rs` "Dynamic object shadows onto baked world", upstream ddc3028).
The world receiver keeps the darker of the baked lightmap and `visibility + floor`. Our floor was an adapter: the
player's nearest irradiance probe `sh[0]` (eased over 0.35 s). Under that bridge the probe is (0.0157, 0.0196, 0.0275),
3 to 5 times darker than retail's constant, so a car on the bridge darkened the bridge's baked shade below it.

**Retail evidence.**
- [code, shader microcode] `data/big/shaders_final.big`, read with `tools/claude-skills/living-world/tools/eb_big_extract.py`
  and `xenos_disasm.py`. Every world receiver pixel shader samples one blurred shadow atlas (`shadowAtlasBlurred`,
  `CSM_Mat_Row0..2`, `g_CSMBlurBias`): `defaultenvironment_defaultPS`, `environmentdiffuse_defaultPS`,
  `baseterrain_defaultPS`, `baseenvironment*`, `decal*environment*`, `transparent*`, `building_*`, `advertisement`,
  `water_defaultPS`, `flowingwater_defaultPS`. Each computes visibility = saturate(depth step + 1 - blurred value),
  adds the literal set {0.05, 0.09, 0.13} per channel and takes the minimum with the squared lightmap (e.g.
  `environmentdiffuse_defaultPS` instructions 24 to 31, `defaultenvironment_defaultPS` 62 and 64). Following the
  register swizzles back to the lightmap fetch gives R 0.05, G 0.09, B 0.13 in every one of them, including the water
  shaders, whose lightmap sits in G,B,R registers so the literal pool reads 0.09, 0.13, 0.05.
- [code] No height cut-off, receiver depth window or per-caster range in the receiver: one constant for every caster
  and receiver. The city world shaders (`defaultenvironment`, `environmentdiffuse`, `baseterrain`, ...) have no
  `shadow` technique, so the world never occludes the dynamic map; casters are `vehicle*_shadowPS`, character, ped,
  `dynamicobject_shadowPS`, `environmentpark*_shadowPS` and `videoscreen_shadowPS`.
- [code] Peds and dynamic objects additionally read a static world shadow map (`shadowWorld`, `WorldShadow_MatRow`,
  drawn by `WorldShadow_defaultVS/PS`, TU3 strings "World Shadow generation" / "DrawWorldShadowCasterInstances" at
  0x821A02D4 / 0x821A02EC). That is a receiver map for objects, not an occluder for the world.
- So retail's answer to the bridge is the floor: a dynamic shadow falling into baked shade at or below
  (0.05, 0.09, 0.13) leaves no mark.

**Change.** The world shadow floor is the retail constant `RETAIL_WORLD_SHADOW_FLOOR` (0.05, 0.09, 0.13) for every
receiver (lightmapped families, flowing water and water), held as data in `WorldShadowSettings`. The probe adapter
and its easing are gone. The water floor (family 33) was the literal in register order (0.09, 0.13, 0.05) applied to
RGB; it now uses the same RGB floor. Character shading, the two directional lights, their cascades and biases, the
layer-28 caster set and the shader's visibility term are unchanged (the character shader never read this floor).

**Moddability.** `sdk.world.set_tuning('shadows', {world_floor = {r, g, b}})` (each 0..1; 0 = full-strength dynamic
shadows), first writer wins, rebuilt to retail when the mod stops; `sdk.world.tuning(key, 'shadows')` reads it. Engine
systems use `modding::world_tuning::set` with the same domain.

**Files.** `crates/skate-game/src/retail_render.rs` (constant, `WorldShadowSettings`, `enable_world_shadows`, tests),
`crates/skate-game/src/retail_character.rs`, `crates/skate-game/src/retail_world.wgsl`,
`crates/skate-game/src/modding/world_tuning.rs`, `crates/skate-mods/src/world_tuning.rs`,
`crates/skate-mods/src/api.lua`, `sdk/skate.lua`.

**Verification.**
- Tests: `world_shadow_floor_is_the_retail_constant`, `baked_shade_at_the_floor_hides_a_dynamic_shadow_and_sunlit_ground_takes_it`
  (the retail receiver expression: shade at the floor is untouched by a full shadow, the old probe floor darkened it,
  sunlit ground still darkens to the floor), `every_world_shadow_read_uses_the_shared_floor` (shader source),
  `world_shadow_floor_defaults_to_retail_set_and_reset` (mod patch, first writer, reset), schema cases in
  `skate-mods` `patches_parse_validate_and_reject_unknown_fields`; existing shader validation tests
  (`retail_shader_tests.rs`) still pass.
- To playtest (rendering not checked by eye): DownTown under the bridge at about [42.6, 15.8, 353] with traffic on the
  bridge: no car shadows on the shaded ground. Elsewhere: the skater's and cars' shadows on sunlit ground are now a
  little lighter and bluish (retail floor instead of the local probe), and in deep baked shade they fade out as in
  retail. Compare with the recomp at the same spot if they look off.

**Open questions.**
- The retail visibility term (depth step plus `1 - blurred` from an exponential-blurred atlas, two cascades) is still
  Bevy's PCF lookup in ours; only the floor is ported here. The sign convention of the scalar-constant subtract was
  read as constant minus register (the only reading that leaves unshadowed receivers at full light).
- Retail's cascade extent and which vehicles are submitted to the shadow pass (CPU side, "Shadow Map Cascade" /
  "DrawShadowCasterInstances") were not traced; ours keeps one 24 m cascade.

## Cars hit peds: retail reaction, 2026-10-08

**Problem.** Traffic cars drove straight through peds: nothing tested a car against a ped. The user remembered (from
long ago, "user's memory is old, confirm in code") that a ped hit by a car ragdolls, then fades out or gets up and
flees.

**Retail evidence** [code, TU3; addresses are evidence only, nothing copied].
- Every contact on a ped's collision body goes through the ped's contact callback `sub_82E38FB8` (ped vtable
  `0x8232BE80`). It first asks vtable slot +164, `sub_82E38400`, for a contact kind 0..5. That classifier takes the
  other body's owner (`[[contact+76]+32]`) and runs the interface cast `sub_82965630` with the type getter
  `0x82C34050`, which returns `0x823220B8`, the type record named `IVehicle` (string at `0x823220B8`, `vehicle` at
  `0x823220C4`). A vehicle owner returns kind **2** at once: no speed, angle or flag test.
- The callback's switch (`0x82E39230`) sends kinds 1 and 2 to one block (`0x82E3926C`): it reads the pose of the
  ped's collision body (`sub_82585CB0`), subtracts the body-to-root offset (`[ped+5756]+19872`) when the ped's slot
  +180 says so, keeps the root's own height (the `vrlimi` keeps y), skips the result if it is not finite or out of
  range (`0x822F88D4`), and writes it as the ped's root position. That is all: no reaction kind or direction
  (`ped+2496` / `+2500`), no `Collision` intent (`PedestrianColliding`), no knock-down speech, no brain flag (the
  `+3196` bit 0x80 at the top is set only for kinds 4 and 5).
- The knock-down / stumble path (3.0 / 6.0 thresholds, `Collision.Knockdown` motion graph, animated, not ragdoll) is
  kind **5**, an `IActor` owner (type getter `0x82586478` -> `0x823000F0`, `IActor`), i.e. the skater; kind 4 is an
  actor contact on body part 1 or 2 (acted on only while `[ped+5756]+140` is 7); kind 3 (the object at `ped+5916`) sets `+3278` bit 0x80.
- The car side, `sub_82C3C150` (the vehicle collision interface at `+136`): a parked car's alarm test on the contact
  impulse; for an `IActor` toucher a bit in the "hit by" mask `+4248` and, for a contact ahead of the car, `+4401` bit
  0x20. It does not stop, honk or post a sound there.
- Peds run from cars only through the horn: the horn decider `sub_82C40660` honks (kind 2) after an obstacle has been
  ahead for 2 s and notifies the obstacle (the honked-at input, `RunFromHonker`, at most 30 s); doc 26 V4, not ported.

**Verdict.** The user's memory is refuted for TU3 (confidence high for the ped side: the classifier and the switch
are read end to end; medium that no other system adds a reaction, no other `IVehicle` test was found in the ped
code). A car shoves a ped out of its way (the car is kinematic with infinite mass, the ped's body is pushed, its root
follows) and the ped walks on. No ragdoll, no knock-down, no fade, no flee from the contact itself. Not checked in a
recomp run (no hook placed; peds rarely stand in a lane).

**Change.**
- `skate-core::living_world::peds::vehicle_contact`: `RetailContactKind` and `retail_response` (the classifier's
  kinds and the callback's switch), `VehicleContactParams` (retail defaults `enabled = true`, `push = true`),
  `detect` (a ped cylinder against a car's oriented box: overlap depth, normal and the pushed feet position with the
  height kept), `closing_speed`.
- `skate-game::living_world::vehicle_contacts`: `ped_vehicle_contacts` (`FixedUpdate`, after `advance_peds` and
  `drive_traffic`) tests every ped against every car's box (`car_box`: the GLB bounds, the same box as the car's skater
  proxy), peds and cars in id order; a contact pushes the ped out and keeps it on its navmesh (`constrain_move`), moves
  the drawn ped in the ground plane only, publishes `VehicleContactEvent` (tick, car and ped `LivingWorldId::to_u64`,
  car speed, closing speed, position, normal, depth, reaction; `Serialize` / `Deserialize`) and logs
  `VEHICLE_CONTACT car=#.. ped=#.. speed= closing= at=[..] normal=[..] depth= reaction= tick=` once per car and ped per
  second.
- Multiplayer: one system decides; it is a pure function of the ped and car states, which already follow from the
  spawn records and the tick, so a host and a client compute the same pushes; the event is the record a host would
  send.

**Moddability.** `sdk.world.set_tuning('living_world', {ped_vehicle_contact = {enabled, push}})`: `enabled = false`
turns the detection, event and log off; `push = false` reports the contact (`reaction = reported`) without moving
the ped, so a mod can react itself. First writer wins per field; the domain is rebuilt to retail when the mod stops.
`VehicleContactEvent` is the hook the planned `sdk.living_world` events read.

**NOT RETAIL YET.** The ped body is a cylinder of the NavPower agent radius and height (0.35 / 1.6 m [data]); retail's
Havok ped shape (`sub_82E26430`) is not decoded. The push is the smallest separation in the ground plane; Havok's
penetration recovery is not decoded. The navmesh stands in for the world collision of the pushed body. The car side
(hit-by mask, the planner stopping for an obstacle ahead, the horn and the ped's `RunFromHonker`) is V4.

**Files.** `crates/skate-core/src/living_world/peds/vehicle_contact.rs`, `crates/skate-core/src/living_world/peds/mod.rs`,
`crates/skate-game/src/living_world/vehicle_contacts.rs`, `crates/skate-game/src/living_world/mod.rs`,
`crates/skate-game/src/living_world/peds_tests.rs`, `crates/skate-game/src/modding/world_tuning.rs`,
`crates/skate-mods/src/world_tuning.rs`.

**Verification.** skate-core: `retail_vehicle_contact_follows_the_body_and_never_knocks_down`,
`a_ped_in_front_of_the_bumper_is_pushed_forward`, `a_ped_beside_or_clear_of_the_car_is_not_touched`,
`a_ped_inside_the_box_leaves_through_the_nearest_side`, `a_turned_car_pushes_along_its_own_axes`. skate-game:
`living_world_cars_push_peds_out_of_the_way_and_report_the_contact` (push, event fields, determinism, serialisation,
the two mod options), `living_world_car_box_matches_the_car_proxy`,
`ped_vehicle_contact_is_mod_reachable_and_reset_on_disable`. Not playtested.

**Open questions.**
- Whether retail ped bodies are actually displaced by a kinematic car in the Havok solve (the callback only follows
  the body); a recomp hook on `sub_82E38FB8` with kind 2 would show it. Peds seldom stand in a lane, which is why the
  user may remember a different game.
- The skater's car-hit bail rules (vehicle contact term `0x820CFF14`, 9.0 limits) are V5.

## Skitching: research and the tow spring (2026-10-09, groundwork)

**Retail [code] (`.local/research/npc/b26-skitching.md`, `b27-skitch-state.md`, `b30-skitch-pull-release.md`; main
checked the state id, the vault keys of the spring, its constants and the update gate).** Cars carry grab splines;
the grab query (`GrabSplineQueryManager`) publishes grab records at ProcessedPhysIn +1888 / +2176 (ours:
`player/post_input.rs:180` `publish_candidates_82d740f8`); with the hand flag (+2476 bit 21, not the animation's
GrabWorld bit 22) and a ready record the selector enters Skitching (104; ours: `player/selector/ground.rs:48`). The
state's update (`sub_82D477C0`, vtable `0x82327398`) runs only while +2480 bit 22 is set and holds the skater to
the car with a velocity-target spring (`sub_82D4B500`, board force tag 6), not a pin; release is the selector's
(hand flag lost -> 100, vehicle contact > 9.0 -> 300, off the ground -> 100). The car side receives a per-frame
"held" message (`sub_82C361E8`: +20 % speed cap, a latch for the player).

**Change (groundwork only).** skate-core `riding::skitching`: `SkitchSpringSettings` (vault
`physics_state_skitching/default` retail values) and `tow_spring` (`sub_82D4B500`); `SkitchSubMode` (`sub_82D49580`,
`b31-skitch-submode-hold.md`, main checked its vault getters): modes 0 settle, 1 inside the grab range, 2 at the
edge, 3 stepping off, 4 let go (readings inferred), with the 0.5 s settle timers, the 0.4 m edge band, the stick
toward / outward tests and the outward car-acceleration push time; the riding skater's skitch query
(`sub_82D39D98`, `b33-skitch-wiring.md`, main checked our publication of the hand flag): time to a candidate spline
`max(0, (ahead - 1.0) / max(closing, 0.5))`, latch below 0.1 s, the re-grab cooldown that skips the last car
(`SkitchQuerySettings`, `choose_skitch`). The hand flag (2476 bit 21) is already published from the ground state's
latch byte (`player/input_phase/publication.rs:262`, `riding/grounded/state/output.rs:159`); nothing sets the latch
yet. The car grab splines are authored on the disc (`b34`, `b35`: RW4 GRABDATA `0x00EB001F` in the
vehicle model-part `.rx2` files of `livingworld.big`; main ran the parser over the disc: 17 car models, one rear-edge
Bezier spline each, 12 to 24 control points, direction (0, 0, -1), z -1.93 to -3.12 m in the model frame; grab
record type 1): `tools/asset_pipeline/grab_data.py` parses them (not yet called by the vehicle exporter). Not wired: no state-104 handler,
no hand-flag producer, no car grab splines yet.

**Verification.** skate-core `the_tow_spring_follows_the_speed_curve_and_is_capped`,
`the_sub_mode_tracks_the_grab_range`, `the_skitch_query_latches_the_first_spline_within_reach`; pipeline
`test_grab_data` (synthetic RX2).

**Open.** The skitch frame (`sub_82D48148`, research b32 running) and the state fields' meanings, the along-car hold
chain (`82D48C98`: car acceleration + stick + rest servo -> offset target; b31 decoded the arithmetic), the car grab splines (research b34), the latch wiring in the ground update (`ground_runtime/update.rs:117`,
gated by owner +12836 bit 0x40, open) and the car-side hook.

## Cars knock the skater down, and brake when hit (V5 start, 2026-10-09)

**Retail [code] (`.local/research/npc/b37-traffic-v5-car-hits.md`; main checked the contact debug string and our
vehicle group).** The skeleton contact pass (`sub_82BD4A30`) keeps the largest relative normal speed against
vehicle-group (8) bodies; the Ground / Air wipeout checks (`sub_82D90C98`) bail the skater (reason 7, WipeoutGround)
above `Wipeout_GroundVehicleContact` (9.0; `Wipeout_GroundSkitchingContact` while skitching), and touching a vehicle
shrinks the other wipeout limits (`Wipeout_GroundVehicleScalar` 0.35). The car side (`sub_82C3C150`): an actor (the
skater; peds are not actors) hitting the car ahead of it latches `+4401` bit 0x20 and the planner brakes hard
(accel = -speed) until the car stands; a parked car's alarm needs `alarm_impulse` (0.1).

**Change.** The traffic car proxy was in contact group 0 (static world), so the already-ported car-hit bail never
fired: it is now `VEHICLE_GROUP` (8). The skeleton contact collector also returns the vehicle-group solids the body
touched (`SkaterRuntime::vehicle_hits`); `apply_vehicle_hits` maps them to the follower car and latches
`Car::hit_brake` when the contact is ahead of the car (ours: along its velocity; retail's axis, the car's vtable
+24, is inferred forward); `follow::step` brakes at -speed and clears the latch at a stop. Log `VEHICLE_HIT`.

**Verification.** skate-game data-gated `a_traffic_car_knocks_the_skater_down_above_the_contact_limit` (DownTown,
a kinematic car box: 12 m/s group 8 bails at tick 6 (group 0: tick 7 through the generic limits); 4 m/s group 8
bails at tick 22 because that box never brakes, group 0 never); `living_world_traffic_proxy_is_the_model_box`
asserts the group; skate-core `a_car_hit_by_the_skater_ahead_brakes_to_a_stop_then_drives_on`. Not play-tested.

**Open.** Roofs (no roof code found; the car is a moving surface in group 8 now), the hit-by mask's reader, a board-only car contact.

## Traffic: obstacles ahead (V4 look-ahead, 2026-10-09)

**Retail [code] (`.local/research/npc/b36-traffic-v4-driver.md`, `b41-v4-obstacles-corridor.md`,
`b42-v4-leftovers.md`; main checked the characteristics / driver values, 0.68 and 0.025).** Every frame the traffic
manager refills two obstacle lists (`sub_826B2EE0`): skaters (radius 0, soft) and world actors (peds radius 0.5 x
body extents z, soft; movable props radius half their smallest extent, hard; traffic cars add nothing). Each car's
look-ahead quad (`sub_82C400A8`) runs from its centre to the bumper plus speed + standoff, flared on the turning side
(`02A6` 2.0 x speed ratio); a record touching its side / front edges or inside it is in the way
(`sub_82C41BD0`, `sub_82C41A90`); its free distance is `|pos - car| - radius - 0.68 x speed`; soft records count
below 20 km/h only for opted-in drivers (all stock drivers). The planner brakes for the nearest one:
`-v^2 / (2 (d - standoff) + 0.001)` within one second of travel plus the standoff, `-v` inside it.

**Change.** skate-core `living_world::traffic::obstacles` (`Obstacle`, `CarFrame`, `look_ahead_quad`, `in_quad`,
`in_the_way`, `nearest`, `obstacle_accel`); `Car::obstacle` (the free distance) feeds `follow::step`; skate-game
`look_ahead` builds the lists from the observers, the peds and the active DMO footprints (`PedObstacles`) and sets
each car's nearest obstacle once per frame before the ticks.

**Engine choices.** The ped radius is our fallback ped radius (0.35; retail's per-ped extents are collision data),
props carry no corner points, a bailing skater's four extra points are not added, the turn widening is off (the
turn side `+3748` is open), and the lists use last frame's poses. The standoff is `+3668` (length x
`Hash_30313EA6F282C874`, b688; the engine `min_gap` before 2026-10-29).

**Verification.** skate-core `the_look_ahead_reaches_speed_plus_standoff_beyond_the_bumper`,
`the_nearest_obstacle_brakes_the_car`; skate-game living_world tests (69). Not play-tested.

**Open.** The turn side, the per-ped radius, the skater-behind zone (quad B, 40 m). The horn: next section.

## Traffic: the horn, and peds running from it (V4, 2026-10-09)

**Retail [code] (`.local/research/npc/b36-traffic-v4-driver.md` sections 2, 4, 5, 8, `b41-v4-obstacles-corridor.md`
section 3, `b44-horn-honker.md`, `.local/research/peds/b45-runfromhonker-flee-timeout.md`; main re-read the honk
receiver `sub_82E3C3D0` and the shape of `826A1358`).**
- Limiter kind `+4392`: each driving-state update sets 0 (free) or 2 (stop point), the lead check 1 (behind a lead
  whose own kind is 1 or 5) or 3 (any other lead), the look-ahead 4 (obstacle); the nearest limit wins. The junction
  answer goes into the same field (1 signal, 2 approach, 3 yield, 4 blocked, 5 a yield to a flagged car), so a car
  queued behind one waiting at a red light counts as waiting itself.
- Timers: blocked `+3704` grows while inside the standoff behind a kind-3 lead and slower than
  `honk_approach_speed_kmh` (else 0 or held); obstacle `+3708` grows while the limiter is the obstacle, not inside a
  lead's standoff, and slow (no obstacle: 0).
- Horn decider `sub_82C40660`, every frame, first match: horn disabled (driver bit 0x01) 0; junction wait (5) kind 3;
  blocked > `honk_blocked_time` kind 4 (driver bit 0x02) or 5; an obstacle with record flag 0 (skater / ped):
  obstacle timer > `honk_obstacle_time` kind 2 plus the honked-at notify to the record's handle, else under 2 s to it
  and faster than the approach speed kind 1. The horn sounds while the decider returns a kind; the sound per kind is
  the `Traffic_Horn` AEMS program's (our native evaluator runs it).
- Driver bits (`sub_82C42348`): percent rolls `rand() % 100 + 1 <= chance x 100` on the driver record
  (`Hash_B5C60C1D43899F74` enabled: 1.0, taxi 0.2; `Hash_7C6B48BD9ADF8E6E` long: 1.0, fast 0.0, reckless 0.5).
- The notify `sub_82E3C3D0` only writes the car id into the ped brain's honker (`+3232`; peds only, the skater's and
  cars' records carry no handle). `IsBeingHonkedAt` takes Wander (off the road / at intersections) and WanderFollow
  into RunFromHonker: Begin motion intent 5; Update every frame while the car exists: goal 10 m sideways of the car's
  line on the ped's side (strict `dot > 0`, a tie goes to the minus side), speed 6.0 (3.0 within 2 m). The op's
  `timeout` 30 is never read; only Wander's Begin clears the honker.

**Change.** skate-core `traffic::horn` (`HornParams`, `DriverBits::roll`, `HornTimers::update`, `decide`, limiter
kinds), `Car` gets `driver`, `limiter`, `horn_timers`, `horn`, `honk_target`, and `obstacle` is now an `ObstacleHit`
(distance, soft, ped id); `follow::step` sets the limiter kind (junction answer, lead, obstacle), runs the timers
and the decider. `FollowParams.horn` comes from the entity's driver record (`livingworld_vehicle_drivers` in
tables.json). skate-core `peds::honk::run_goal`, brain op `RunFromHonker` (intent 5). skate-game: the look-ahead
records carry ped ids; the driver bits are rolled from the spawn seed; `TrafficAudio.horn` gets the horn state every
frame (a mod's `VehicleHorn` still plays on top; the car alarm is untouched); `TrafficEvent::Horn` on a change and
`TrafficEvent::HonkedAt` every frame of kind 2; `think_peds` sets the honker and runs the ped to the goal (run gait,
log `PED_HONKED`). Mod values: `living_world.traffic_horn {[<driver record> or all] = {blocked_time, obstacle_time,
approach_speed_kmh, approach_seconds, enabled_chance, blocked_long_chance}}` (read at spawn) and
`ped_brain.run_from_honker_distance` / `run_from_honker_speed`.

**Engine choices.** "Inside the standoff behind a lead" is `gap <= min_gap + stop_margin` (our follower settles at
`min_gap`, retail stops exactly at the standoff); the junction answer is applied before the lead and obstacle
checks (retail's order between FollowingLane and the limiter step is not traced); RunFromHonker uses the car's
forward axis (retail reads `[car+164]+144`, velocity or an axis, open), sets the route every frame like retail's
path request, and skips the navmesh cast the flee legs use.

**Verification.** skate-core horn tests (`a_car_stuck_behind_a_lead_honks_after_the_blocked_time`,
`an_obstacle_gets_the_approach_horn_then_the_long_one_with_a_notify`, `a_disabled_horn_and_the_junction_wait`,
`the_percent_roll_matches_retail_bounds`), follower test
`a_car_stuck_behind_a_standing_car_honks_but_a_red_light_queue_does_not`, `the_ped_runs_ten_metres_sideways_on_its_own_side`,
`run_from_honker_posts_its_intent_and_keeps_the_honker`; skate-mods `traffic_horn` validation. Not heard in game yet.

**Open.** What packs `+3420` into the audio list entry (`sub_82C485A8` caller); the car vector RunFromHonker reads;
the steering type's speed quantisation (6 stays 6, 3 becomes 2 in type 0); `IsOnRoad` / `IsOnIntersection`
(absent, answer false, so Wander's on-road test never holds the honk back); the driver bits' other uses (0x04, 0x10).

## Skitching step 1: car grab splines in the vehicle data (2026-10-09)

**Retail [code + data] (`.local/research/npc/b34-car-grab-splines.md`, `b35-car-definition-resource.md`; main ran the
parser over the disc).** Car grab splines are authored per model: RW4 GRABDATA (`0x00EB001F`) in the first part arena
of the recipe that has one (`82C2A8C8`), registered as grab records of type 1. On the disc 17 of the 18 car recipes
have exactly one rear-edge spline (Bezier chains of 12 to 24 control points at z -1.7 to -3.1, direction (0, 0, -1));
`reda_car` has none.

**Change.** `tools/asset_pipeline/living_world_vehicles.py` `recipe_grab_splines` writes `vehicles.json
models.<record>.grab_splines = [{points, direction, bounds, flags}]` (model space, the GLB's frame; exporter VERSION
2); skate-game `VehicleModel.grab_splines` (`CarGrabSpline`), a spline that is not whole Bezier segments is dropped.

**Verification.** Python `GrabSplines.test_the_first_part_arena_with_grabdata_gives_the_splines` (+ the grab_data
tests); skate-game `grab_splines_and_driver_horn_values_load`. The dev install's vehicles.json got the field by hand
(same function) until the next setup refresh.

**Open (next steps, `.local/research/npc/b46-skitch-port-map.md`, `b47-skitch-frame-transforms.md`).** The grab
query's provider slots: `82760508` treats the mode as a bitmask (0x02 the provider at scene `+4088`, 0x04 the one at
`+4084`; our offboard port is the 0x04 one); which provider enumerates cars and its gates are not decoded yet, nor
whether a car record carries an assembly. Then the riding skitch query, state 104 (registry, dispatch, a no-op exit
like retail's `82B61BB8`, its own publication) and the car side.

## Skitching step 4a: the state-104 frame step, and the GRABDATA header fix (2026-10-09)

**Retail [code] (`.local/research/npc/b32-skitch-frame.md` section 1, `b47-skitch-frame-transforms.md` section 1).**
`sub_82D48148` re-orients the grab record to the board, keeps a three-frame history of the grab edge (direction,
up, side, midpoint), measures the skater's along-edge coordinate in the previous frame and builds the axis point
from it (b47 corrects b32: no relative transform there), maps the current grab point into the previous pose with
`inv(current) * previous`, and derives the car velocity at the grab location (two-frame difference), the tow speed,
the distances and rates the tow spring reads, the side target (0.5 m along the side axis) and the "tows fast" gate
(`Hash_1F85F500908C5E17`, 3.0).

**Change.** skate-core `riding::skitching::frame` (`FrameInput`, `FrameState`, `FrameOutput`, `FrameSettings`,
`step`; `to_world` / `to_local`); the car-motion frame (256 = 448) is kept for the hand targets, 384 (identity in
retail) is left out. `tools/asset_pipeline/grab_data.py`: GRABDATA header +12 is the count of enabled entries (byte
+70 non-zero, the length of the direction array), not the entry count again; 3 parkassets props with disabled
entries failed before (main re-ran all 102 with GRABDATA: all parse).

**Engine choices.** A zero vector normalises to zero (retail's epsilon vector at `0x830BD350` is not read).

**Verification.** skate-core `a_standing_car_gives_the_axis_point_and_no_tow`,
`a_car_pulling_away_tows_and_the_grab_point_is_compared_in_the_previous_pose`,
`a_board_facing_the_other_way_swaps_the_endpoints`; Python grab_data tests (disabled-entry header case added).

**Open.** Not wired: the state-104 handler (pre-step gate, sub-mode, along chain, forces, publication) and the car
provider. The grab providers are now known from `82857FB0` (b49, in progress): scene `+4084` (mode bit 0x04) is the
type-2 world-object provider (or the DMO manager on the alternate path), `+4088` (bit 0x02) the vehicle provider
(vtable `0x82322514`: `82C36068` single, `82C35B98` box, `82C35840` radius), so cars answer only mode 255 queries.

## Skitching step 2: cars in the grab scene (2026-10-09)

**Retail [code] (`.local/research/npc/b49-grab-providers.md`; main checked the provider vtable against the image and
the gates in `82C35B98`).** `82857FB0` fills the grab scene's two provider slots: `+4084` (query mode bit 0x04) gets
the type-2 world-object provider (or the DMO manager on an alternate path), `+4088` (bit 0x02) the vehicle provider
(vtable `0x82322514`: `82C36068` single record, `82C35B98` box query, `82C35840` radius query). `82760508` treats the
query mode as that bitmask, so only mode 255 (the riding skitch query) reaches cars. The vehicle box query gates each
car by a sphere (query radius + 15 m around the car's origin) only, walks the car's spline list (`car+200`, count
`+212`), needs byte +68 > 1 and +70 != 0, a segment hit (`82ADD910`, segments of 1e-5 or less skipped) and the car's
assembly (`car+172` chain), then builds a type-1 record (`82585F58`) with the car matrix and velocity and keeps it if
CanGrabSpline passes.

**Change.** skate-core `grab_scene`: `Provider::Vehicle`, `query` takes the mode as a provider bitmask (cars first,
then the existing world-object loop), the registry insists car records are type 1. skate-game: `Registry::set_cars` /
`GamePhysics::set_grab_cars`, `living_world::vehicles::push_vehicle_grab_splines` (each fixed tick before physics:
every car's splines with its current transform and velocity; ids `CAR_GRAB_TAG | serial`, spline ids
`CAR_GRAB_TAG | serial << 3 | index`), `CarGrabSpline.flags` into the record's word 60 [inferred].

**Engine choices.** NOT RETAIL YET: the car's assembly is a stand-in carrying the car's id (the `car+172` object is
not identified); spline ids are stable per car instead of retail's global counter; the box query and CanGrabSpline
are applied as in the world-object loop (whether the skitch query takes the box or the radius path is open).

**Verification.** skate-core `cars_answer_only_the_vehicle_bit_and_world_objects_only_the_other`,
`a_car_needs_its_assembly_and_must_be_within_the_sphere`; skate-game
`a_car_enters_the_grab_scene_with_its_rear_spline_in_world_space` (115 living_world / offboard tests pass).

**Open.** The riding skitch query (`82D39BB8`) that submits mode 255 and latches a car, state 104's handler, the car
side (held flag, tow reaction), flag `0x83082929` (which maps take the DMO path).

## Skitching step 3: the riding skitch query (2026-10-09)

**Retail [code] (`.local/research/npc/b33-skitch-wiring.md` section 2; main read the ground update branch, the box
flag of `82D2E250` and the query constants).**
- The ground update `82D37C88` branches on Processed `+2476` bit 22 (the grab input): set, it runs the skitch query
  `82D39BB8`; clear, it invalidates the grab owner (`82D749D0`). Our ground update always invalidated.
- The query box is `82D2E250(frame +192, (0, 0.86, 0), (0.48, 0.45, 2.0), out, true)`: with the flag set the box
  keeps the frame's own forward (row 2, not flattened), right = row 0 flattened and normalised, up = forward x right,
  placed at the origin + forward x (offset z + extent z) + up x offset y (so it reaches 2 m ahead).
- When the owner's validated results are ready (`+12836` bit 0x40) the latch `82D39D98` walks them: orients the
  endpoints, CanGrabSpline, the re-grab cooldown (Processed `+2848` / `+2592`), the time to the spline
  `max(0, (dot(P - pos, fwd) - 1.0) / max(dot(vSkater - vCar, fwd), 0.5))`, a bind per candidate, and the latch
  (`+2729`, `+2560` type, `+2564` id) on the first one under 0.1 s.
- It always submits `82D74200(owner, 255, pos, box, ...)` with margin 0.25 (`0x820991A0`) and the angles 60 / 30
  degrees (`0x8209919C` / `0x82099198` x the degree constant `0x8206D110`), cap 5. Mode 255 reaches the cars.

**Change.** skate-core `ground_sync::skitch_bounds` (the flag-true box), `SkitchQuerySettings` gains the limits and
cap, candidate ids are u32 (+192). skate-game `ground_runtime::skitch` (`query_shape`, `latch`, `query`), the ground
update calls it instead of the invalidate while bit 22 is set, `GroundSettings.skitch` reads the reach and latch time
from `physics_state_skitching` (retail values as defaults), the grab owner exposes its validated records.

**Engine choices.** The latch is computed but NOT written yet (`skitch::LATCH_ENABLED = false`): it would make the
selector request state 104, which the registry still refuses (the transition returns an error). NOT RETAIL YET:
type-2 world objects are not latched (their box `82D2CF68` is not decoded), the spline point is the closest point of
the endpoint chord, `+2668` stays 0 and `82D91298(state+28, 2)` is not called.

**Verification.** skate-game `the_box_reaches_two_metres_ahead_along_the_frame_forward`,
`a_close_car_latches_and_a_farther_one_only_binds` (time 0.5 s binds, 0.05 s latches, the cooldown skips the last
car only), `world_objects_are_not_latched_yet`. Not run in game (the riding grab now submits a query each frame;
regression check pending).

## Skitching step 4b: the hold target and the release impulses (2026-10-09)

**Retail [code + data] (`.local/research/npc/b52-skitch-target-impulses.md` sections 1-2; main checked the impulse
vault keys).** `82D4B8C0` builds the hold target (the side-shifted previous frame at the hand's along coordinate),
its horizontal direction (the tow spring's next axis), the yaw between the grab point and the target, and the lean
yaw (`940`); `82D4BCF8` turns the skater's facing toward the target at up to 180 degrees per second, scaled by the
larger of a turn-in curve over the skitch time and a gain over the tow speed. On release `82D4B1D0` pulls in
(rate 856 - 2.0, never outward, never faster than the tow speed) and `82D4B378` pushes off (856 + 2.5), one frame of
force tag 6.

**Change.** skate-core `riding::skitching::target` (`step`, `pull_in_force`, `push_off_force`, `TargetSettings` with
the vault values, `wrap`, `yaw`).

**Engine choices.** The signed angle `8286CD88` is the standard angle a -> b about the axis (internals not decoded);
`6FB7A3D992163663` (the speed gain) is not in the collections by hash, the stored graph between the other two keys
(`HeadingAdjustVsSpeed`) is used for it (unverified).

**Verification.** skate-core `facing_the_target_needs_no_yaw`, `a_target_to_the_side_turns_at_most_the_rate_cap`,
`release_impulses_pull_in_never_outward_and_push_off`. Not wired: state 104's handler is next.

## Skitching step 4c: the hold step and the release (2026-10-09)

**Retail [code + data] (`.local/research/npc/b53-skitch-hold-step.md`, b51 section 2; main checked the 1344 masks in
`82D49580`: the per-frame clear is 0x60, b31's 0x18 was wrong).** `82D49D70` keeps the hand's along target (904):
while 1344 bit 0x08 is set it follows the clamped position, or ratchets with a stick past +-0.5; the bit clears when
the skater rests near the posed hand and sets again when the hand is 0.2 m off or after 2 s; the posed hand (908)
chases the target with a step of 0.25 x the error, changing by at most 0.03 per tick. `82D49580` sets the release
bits per sub-mode (1 pull in, 2 / 3 push off; 2 -> 4 clears them, 3 -> 4 keeps push off). The tail `82D4AFB8` lets go
(sub-mode 4) without the grab input or, in sub-modes 1 / 2, with both hands off once the 0.5 s hold timer ran out;
in sub-mode 4 it queues the impulse on the first frame, ends the state after three, and refills the 0.5 s re-grab
block.

**Change.** skate-core `riding::skitching::hold` (`HoldState::{reset, step, mode_bits, tail, released}`,
`HoldSettings` with the eight vault values and the release frame count, `Impulse`).

**Verification.** skate-core `the_posed_hand_eases_toward_the_target_in_capped_steps`,
`the_stick_ratchets_the_target_and_rest_clears_the_bit`, `release_queues_one_impulse_then_ends_after_three_frames`.
Not wired: state 104's handler (registry, dispatch, enter / exit, forces, publication) is next.

**Open.** The meaning of 2476 bit 22 in the tail (the grab input, read as such), why reset skips 872, the vector at
1168 in the tail.

## Skitching step 4d: the along-the-bumper hand chain (2026-10-09)

**Retail [code + data] (`.local/research/npc/b30-skitch-pull-release.md` section 4, `b31-skitch-submode-hold.md`
section 2).** `82D48C98` low-passes the car's acceleration along the grab edge (0.9 / 0.1), turns it into a hard event
past 40 and an excess over a dead-band curve, adds the stick's push (10) or, without stick or event, a velocity servo to
rest, integrates the hand's velocity (capped at 1, or the stick gain against an opposing event; unbounded in the
event's direction) and moves the hand target along the edge, clamped to the range and snapped onto the latched edge
point when it crosses it.

**Change.** skate-core `riding::skitching::shimmy` (`ShimmyState::step`, `ShimmySettings` with the five vault values).

**Open.** The car acceleration `a` is an input: its derivation from the frame history (b30's reading predates b47's
frame layout) is being re-read (b55), as are the tow spring's body-height term and the stick axis 924.

**Verification.** skate-core `the_stick_moves_the_hand_along_the_bumper_at_most_one_unit_per_second`,
`a_hard_car_acceleration_is_an_event_and_the_gate_resets`.

## Skitching step 4e: state 104 in the game (2026-10-09)

**Retail [code] (`.local/research/npc/b50-skitch-prestep-update.md`, `b51-skitch-forces-publish.md`, `b55-skitch-inputs.md`;
main checked the publication gate, the 7199.999 constant and the skeleton load in the spring).** State 104 (vtable
`0x82327398`): Enter `82D47278` + reset, an empty Exit, Update `82D477C0` (hands / forearms / head out of collision for
5 frames, the gated body: record copy, off-ground pre-step, stick 924, frame step, sub-mode, hold step, along chain,
forces, then the tail; always the reckoning and the skeleton ground update), publication `82D4C078`. b55 settled the
along chain's input (the grab point's along displacement over the last frame x 7199.999, frames 128 vs 64 after the
shift), the stick (turn and spin, sign-asymmetric) and the spring's height (z of the skeleton's raw part-0 global,
Skeleton+14208).

**Change.** skate-game `physics::skitch_state` (`SkitchState`, `SkitchSettings`, `enter`, `update`, `output`); the
state registry supports 104 with transitions to and from the other states; `frame.rs` dispatches it; the transition
enters / exits it; the publication writes flag 304 (ready and not released), the car handle (36 / 40) and the re-grab
timer (292); `AnimatedSkeleton.raw_part0_global` keeps the raw part-0 global for the spring.

**Board forces (b56, main checked the Ground call sites).** `skitch_state::compose_board` composes `82D4AC38` with the
Ground input builder: capture, tow spring (tag 6), slide friction (tag 1, heading time = the sub-mode time), tilt
without history (0 off the ground), speed wobble (Ground's COM height; skipped off the ground), anti-flip, truck
targets, heading (balance and spin both set), manual (powersliding off), the manual and anti-flip displacements and the
yaw correction through the axis displacement `82C07328`; sub-mode 4 only tilt, wobble and truck targets.

**Engine choices.** NOT RETAIL YET: the grab point is the point on the record's chord; the board forward is the deck
forward; the lean (`82D4A0C0`) and the extra output fields (animation 132 / 136 / 140, ground 280 / 284 / 288 / 308,
state byte 53) are not composed yet. The riding latch is opt-in: start the game with `SKATE_SKITCH=1` to try it.

**Verification.** skate-game `skitching_is_connected_to_ground_and_back`; the skate-core skitching tests; skate-game
bin 664 pass (only the known setup fingerprint failure).

## Skitching step 5: the held car (2026-10-09)

**Retail [code + trace] (`.local/research/npc/b57-held-car.md`; main checked the cap reads in `82C3FF38`).** Every gated
state-104 frame `82C361E8` marks the car held (`+4402` bit 0x02; `+4403` bit 0x80 when the holder is the player);
`82C34CD0` moves both to their "last tick" bits and clears them every tick. A held car's speed cap is
`(1.0 + 3680 + 3684) x the lane cap` (3684 = 0.2 in the recomp trace, so 1.2x); a player-held car skips traffic lights
in its junction query and the slow-speed soft obstacles in its look-ahead (`82C344D0`); junctions treat a held car as
flagged (mover getter `82C34598`).

**Change.** skate-core `Car::held` / `player_held` and `FollowParams::held_cap_add` (0.2): the cap scale gains the add
while held, the junction query's light check is off while the player holds the car, the snapshot's `flagged` is the
held bit. skate-game: `drive_traffic` clears and sets the bits every tick from the local skater's state 104
(`SkitchState::held_car`, decoded from the grab spline id) and the look-ahead's soft opt-in is off for a player-held
car.

**Open (not ported).** The skater scan's skip while held, the 2.5 s no-follow grace after a release (`82C34B30`,
characteristics `4727CF78`), no pull-over while held (`82C41CD0` / `82C3D830`), the traffic manager's held start / end
calls (vt+84 / vt+88), the entered-on-red flag (`+3424` bit 0x04, `82C3B500`), NPC skaters as holders.

**Verification.** skate-core `a_held_car_drives_up_to_twelve_tenths_of_the_cap_and_the_player_skips_the_light`.

## Skitching step 4f: the lean (2026-10-09)

**Retail [code + data] (`.local/research/npc/b58-skitch-lean-outputs.md` part 1; main re-read the constants).**
`82D4A0C0` turns the target step's lean yaw (940) into a lean angle (944): target = the sign of 940 times the curve
`30FDFCE185CD3D4F` of |940| in degrees (0 while Processed 2488 bit 0x00800000), approached at 0.25 per update but at
most 2 degrees; past 2 degrees the skeleton's board-offset orientation (Skeleton+15696) becomes a rotation of 944 about
Y for 15 updates (+16389 = 1, +16392 = 15), the height channel untouched. `82BDD630` (ours `SkateboardOffset`) blends
it out.

**Change.** skate-core `riding::skitching::lean` (`step`, `LeanSettings`), `SkateboardOffset::refresh_orientation`;
skate-game `skitch_state` keeps 940 / 944 and refreshes the board offset each gated frame.

**Verification.** skate-core `the_lean_eases_in_two_degrees_at_most_and_writes_past_the_threshold`.

**Outputs (b58 part 2).** Published now: animation 136 (`skitch_shimmy_136` = the posed hand's step x 60), ground 280
(`skitch_grab_height_280`), 284 (`skitch_absorb_284` = 956) and 288 (`skitch_along_288` = 936). Their readers are the
skitch graph nodes (IsSkitchShimmying, SkitchShimmyingBehaviour, SkitchingBehaviour "Crouch" / "absorbspeed",
IsSkitchingWithAbsorb), not evaluated by our graph host yet. Open: animation 132 (988) / 140 (984) and ground 308 (996)
from the hand-target step `82D4A378` (b59 decoding), state byte 53.

## Skitching step 4g: the hands (2026-10-09)

**Retail [code + data] (`.local/research/npc/b59-skitch-hand-targets.md`; main checked the IK write path).**
`82D4A378` places each hand on the grab edge at the posed hand position plus that hand's skeleton-local offset, lets a
hand go when the lean yaw leaves its bound (-60..35 / -35..60 degrees) or its shoulder is more than 0.9 + 0.7 m from
its grip point (through the skeleton and the car's last-frame motion), moves the hand IK weights toward on / off by
+0.03 / -0.2 per update, follows the world grab height into 988 (gain 0.1, cap 0.8) and hands the grip points (plus
the car's motion over one tick, clamped to 0.9 m from the shoulder) to the hand IK (`82BD9728` / `82BD97D0`). b59
corrects b58: the targets are stack vectors, not state+704 / +720.

**Change.** skate-core `riding::skitching::hands` (`HandState::step`, `HandSettings` with the vault values);
skate-game `skitch_state` runs it after the hold step, feeds the hand-off flags to the sub-mode and the tail (replacing
the per-sub-mode stand-in), writes the hand IK through the handplant's slots (limb 2 / 3) and publishes 984 / 988
(animation 140 / 132); `AnimatedSkeleton.raw_part_globals` keeps every part's raw global (was part 0 only).

**Verification.** skate-core `hands_grip_the_edge_and_let_go_when_out_of_reach`; skate-game skitch / handplant /
animated tests. Not seen in game yet.

## Skitching step 4h: the skitch animation graph nodes (2026-10-09)

**Retail [code + data] (`.local/research/npc/b60-skitch-graph-nodes.md`; main read the `anim_skitching/default` record
and loaded the stock graph).** The stock ground motion graph (`ground.xml`, `Skitching/skitchpush.xml`,
`skitchbrake.xml`) uses EnterSkitchingBehaviour / SkitchingBehaviour / SkitchShimmyingBehaviour and the conditions
IsSkitchingWithAbsorb / SkitchingPosition / IsSkitchShimmying. Before this step our graph host returned an error on the
behaviours (graph execution stopped once the skater skitched) and did not parse the conditions. The behaviours bind
`anim_skitching/default`: an absorb curve over the closing rate and six floats in the schema's fixed layout (+80 0.1,
+84 0.25, +88 0.85, +92 0.05, +96 0.83 `LongSkitchIntoReachTime`, +100 0.121; read with the vault layout tool, b61
corrects b60's hash-order guess). SkitchingBehaviour sets "Crouch" (grab height + 0.121), "PushSpeed" (state+996:
the tow speed plus up to 200 m/s^2 x dt, at most 3.5 per update, toward 8 m/s; `82D47BD8`, b61) and "absorbspeed"
(toward the curve by 0.04 per update); EnterSkitchingBehaviour sets "Crouch". SkitchingPosition's mirror term is
(natural stance regular) == (riding switch), the shimmy flip the mirrored animation bit (b61).

**Change.** skate-game `graph_host::motion_skitching` (settings from the record with the stock values as defaults, the
absorb instance, the condition results); the three conditions parse and evaluate; the behaviours run (instance
`Instance::Skitching`); `GameplayConditions` carries the 104 outputs (280 / 284 / 136 / 140).

EnterSkitchingBehaviour also sets "reachspeed" (1 - n, n from the time to the spline over the current clip's remaining
fraction, 0.1..0.83, rate-limited by 0.1) and SkitchShimmyingBehaviour "ShimmySpeed" (the shimmy rate between 0.25 and
0.85, normalised) (b61).

**Engine choices.** NOT RETAIL YET: the shimmy channels SKCH_2H_SHIMMY_LEFT / RIGHT_CHANNEL are not started (their
contents are not in the stock assets); the push speed's hold bit (1345 bit 0x10) is not ported (always updated).

**Verification.** skate-game `absorb_follows_the_curve_at_most_four_hundredths_per_update`, `shimmy_and_position_results`;
the stock graph loads with the new conditions (`stock_motion_host_loads_graph_settings_and_authored_riding_tree`, run
against the user's install); graph host tests (74).


## Skitching step 6: cars slow down for a skater behind them (2026-10-09)

**Problem.** Traffic never reacted to a skater coming up from behind, and the earlier V3 notes read retail's
"following" rule as a lead-car rule (`follow::retail_follow_accel`), which it is not.

**Retail [code + data] (`.local/research/npc/b63-traffic-skater-scan.md`; main re-read the scan's resets
0x82165A10 / 0x8216DEE0, the state-104 skip, the 0.5 size factor, the quad B builder `82C400A8` (corners at +3520 to
+3568, the back corners pushed `Hash_33466832` metres along their side edges) and the planner's five vault hashes).**
- Skater scan `82C414A8` (per car, after the look-ahead, before the horn): nothing while a skater holds the car;
  else every actor of the skater list that is not skitching, with `|p - car| - 0.5 x size` under 40 m
  (`Hash_33466832D8178EAF`) and inside the REAR zone (quad B: from the front bumper back past the car and 40 m
  further; the far edge is not tested), writes distance and speed; one facing the car's way sets bit 0x10. Last match
  wins.
- Planner `82C3FA08`, limiter kind free only: with bit 0x10 and both speeds above 20 km/h (`D20826F1`) the
  acceleration cap is 0 and the car brakes toward the skater's speed minus 20 km/h (`256A412E`):
  `(max(vs - m, 0)^2 - v^2) / (2 D + 0.001)`, D = 20 m (`3AB7FC7C`) once the release grace is over (FAR), or D = 5 m
  (`F682D359`) within 20 m (`D49FC490`) when the skater's NEAR flag is set (not gated by the grace).
- Release edge `82C34B30`, once per traffic step after every car: a car let go this step gets 2.5 s
  (`4727CF78`), otherwise the grace drops by the step and sits at -1 once negative.
- So a car slows until a skater catching up from behind can reach it: the setup for skitching.

**Change.**
- skate-core `traffic/skater_scan.rs`: `SkaterFollowParams` (retail defaults), `rear_zone_quad`,
  `in_quad_three_edges`, `scan`, `skater_follow`, `grace_step`. `follow::Car` gains `held_last`, `release_grace`,
  `skater`; `FollowParams.skater` replaces `follow_min_speed` / `follow_margin`; `follow::step` applies the rule while
  the limiter kind is free and steps the grace after the car loop. `retail_follow_accel` removed.
- skate-game `vehicles.rs`: the look-ahead builds quad B and runs the scan per car over the observers (the local
  player, skipped while skitching); the spec loader fills the new params.
- Setup: `living_world.py` names the hashes (`skater_follow_margin_kmh`, `skater_far_distance`, `skater_scan_range`,
  `skater_near_range`, `skater_near_distance`, `release_grace`). `3AB7FC7C` was labelled `follow_speed_margin_kmh`; it
  is the FAR braking distance (same value, 20, in every stock spec, so nothing changed in play). The loader reads the
  old label and the raw hashes too, so older exports keep working.

**Engine choices / NOT RETAIL YET.** The skater rule is a min with our lead / junction terms (our planner folds them
into one pass; retail keeps them apart by the limiter kind). The actor facing is the velocity heading (retail: the
actor matrix row +32); the NEAR flag byte (`[[state+52]+55]` bit 0) is not identified, so the NEAR rule never fires;
the scan's list is our observers (whether retail's list holds NPC skaters too is open); `car+36` vt+172 is taken as
the car's full extents (open); quad B uses the speed ratio 0 like our quad A. Mod access: the values live in the
per-car `FollowParams` (`VehicleOverrides.params`); the Lua surface comes with V8.

**Multiplayer.** Per car: `held`, `held_last`, `release_grace`, the scan result; host-authoritative, plain data. The
actor order must be stable (last match wins).

**Verification.** skate-core `skater_scan` tests (rear zone hit / miss / range / held / skitching, last match and the
sticky bit, open far edge, FAR / NEAR / grace / slow / cap formulas, 150-step grace); traffic 40 and skate-game
living_world 71 pass. Not play-tested yet.

## Skitching step 4i: PushSpeed's latch and step (2026-10-09)

**Retail [code] (`.local/research/npc/b67-skitch-reel-in-decode.md`; main decoded `82D47BD8` and checked that the
state-104 update `82D477C0` copies Player+1888 to state+1024).** PushSpeed (state+996, published as ground+308) moves
toward 8 m/s by min(200 x state+1264, 3.5) per update unless state+1345 bit 0x10 is set. state+1264 is the held
record's +240, the held object's inverse mass, not the step time (b30 / b61 read it as dt). The bit latches while the
animation carries the PushContact attribute (Processed2488 bit 29, set by `82BDA0D0`) and clears when it drops. The
same function queues a reel-in force (D x 60 x clamp(T - D.V, 0, 1.5), length 50, plus a yaw torque) and a let-out
force (minus the spring force x inverse mass, first 0.2 s of the state) on the held object; retail drops held-object
forces for cars (kind 1, b50), so they do nothing while skitching.

**Change.** `skitch_state.rs`: the step uses the held record's word 60 (inverse mass); `SkitchState.push_latched`
holds PushSpeed while PushContact is set; reset on enter. The held-object forces are not applied (cars).

**Open.** What a car writes at record+240 in retail (ours: the record default 1.0); whether the skitch push clips
author PushContact. Tests: skate-game skitch / living_world pass. Not play-tested.

## Skitching step 4j: the board's effective transform rows (2026-10-09)

**Problem.** The state-104 code read Processed+128 (the "board forward" of the re-orient test) as the deck's forward
row and fed the target step the deck forward and Processed+592 where retail reads Processed+160 / +176.

**Retail [code] (`.local/research/npc/b68-processed-128-matrix.md`; main checked the reader at the start of
`82D48148`: `lvx [state+16]+128`, dot with the bumper direction state+1136, flip when negative).** Processed+128..+191
is the effective board transform that `82C013F0` (PrepareBoardToolkit, our `physics::board_toolkit`) stores each
frame: row 0 the side axis, row 1 up, row 2 forward, row 3 the deck position (rows 0 and 2 negated by the stance bit).

**Change.** `frame::FrameInput.board_forward` is now `board_side` (= `toolkit.effective[0]`); the target step's
facing / position are `effective[2]` / `effective[3]`. The frame step's own skater position (state+592) is unchanged
(its source is still Processed+592; open whether retail copies it from there).

**Verification.** skate-core skitching 16 and skate-game skitch / living_world / npc 87 pass. Not play-tested.

## Skitching step 7: NPC skaters skitch (opt-in, 2026-10-09)

**Retail [code] (`.local/research/npc/b65-npc-skitch-mode4.md`; main checked `sub_8246FA30` at 0x8246FC78: not
airborne (+928) and avoider mode ([ctrl+8]+6000) == 4 posts the intent at key 0x830BE780 with 1.0).** An AI skater in
avoider mode 4 posts GrabWorld every tick (no timer), which reaches Processed2476 bit 22 like the player's grab, so the
riding skitch query and state 104 run unchanged; while it posts it or skitches, the trick dispatch is skipped. It lets
go when mode 4 ends (the skitch candidate fails: car turned, passed, too slow, airborne). Steering in mode 4 aims at
the skitch entry. Cars count any holder as held (+4402 bit 0x02); only the player's hold skips lights (0x80).

**Change.** skate-core `ai_signals::apply_skitch_mode` (+ `SignalSettings.skitch_grab`, 1.0); skate-game
`npc_sim` applies it to the simulated skater's intents; `npc_avoid` aims mode 4 at the skitch target and mode 3 at the
steer target; `NpcSim::held_car` and `drive_traffic` mark cars held by simulated NPC skaters (`player_held` stays the
local player's).

**NOT RETAIL YET / open.** Needs `SKATE_SKITCH=1` and the simulated tier (`SKATE_NPC_SIM=1`); the controller gates
`ctrl+568 != 4`, `ctrl+800` and the skater's +1904 bit 26 are not modelled; recorded node action 6 (also GrabWorld)
is not handled; the candidate's skater direction uses the sample velocity (retail: the recorded step 3 nodes ahead
when `ctrl+920` is set).

**Verification.** skate-core `mode_4_grabs_and_skips_the_trick_dispatch`; skate-game npc / living_world pass. Not
play-tested.

## Manoeuvres groundwork: lane-change passage, stop spots, gap check (2026-10-09)

**Retail [code + data] (`.local/research/npc/b69-traffic-manoeuvres.md` with the b71 / b72 extensions; main checked
the progress step `82C3C3C0` (speed x +/-1 x 1/60), the road-network values 1.3 / 10 in the exported tables and the
vehicle-characteristics fixed layout).** The manoeuvre decider `82C41CD0` tries an overtake (never with stock data:
`Hash_9366C67755A24D89` is 0), a lane change on a timer and a pull-over. A lane change is a passage: a cubic Hermite
curve from the car's lane point at d0 to the target lane at d1 = d0 + ext x `Hash_328B9F4685A14018` + speed, tangents
= lane direction x (d1 - d0) x 1.3 (`Hash_D2C11CC10C9E0E49`), a 10-chord arc table (`Hash_E35079BDD5BAE286`); the car
advances along it at its speed and faces its tangent. Pulling over reserves a stop spot (`82E151D0`): the middle of
the road when free, else the middle of the first long enough gap ahead within 0.75 of the road (`82E14CF8`). A lane
change needs one second of travel clear of the cars ahead and behind on the target lane (`82E14928`).

**Change.** skate-core `traffic/passage.rs` (`Passage::begin / advance / distance / sample`, `PassageParams`) and
`traffic/spots.rs` (`Reservations`, `find_spot`, `gap_free`, `SpotParams`), pure and tested; then
`traffic/manoeuvre.rs` (`decide`: the held-car cancel, the overtake roll, the lane timer with the go / least-loaded
rolls and the adjacent-lane pick, the pull-over roll and spot reservation; driver fields per b74: go +28
`Hash_52CF2CF3`, least loaded +24 `Hash_7C6B48BD`, lane timer spec+48 `Hash_90AB56A5`). Retail road +64 is the disc
segment's `word_56` (`82E14158` copies the 64-byte record to road+8; b76): bit 0x02 (every segment) allows lane
changes, bit 0x01 (some 1-lane roads, value 3) allows pull-over and adds a kerb lane slot; road +136 is the summed
occupancy length per lane (`82E14FC0` / `82E15110`). `SegmentInput` / `Segment` now carry it as `manoeuvres` (from
`skate-data` `word_56`). DownTown: 22 one-lane roads with 3, 3 one-lane and 21 two-lane roads with 2, so lane changes
happen on two-lane roads and pull-overs on those one-lane roads. Not wired yet: the occupancy length sums, then the
state machine (ChangingLane, PullingOver, StayingParked, PullingOut; edges per b74) and the follower / transform.

**Found on the way (todo `traffic-accel-fields-mislabelled`).** The follower loads `Hash_328B9F46` as its max
acceleration and `Hash_75822921` as its comfortable braking; retail uses them as the lane-change passage factor and
the pull-over approach factor (`82C3CDD0`). Behaviour unchanged for now (their values sit near the recomp's measured
acceleration); the real acceleration chain (spec+60 `Hash_03F46E22E52C0002` 0.2 per tick, cap `+3676`) is next.

**Verification.** skate-core traffic 46 pass.

## Traffic: the horn's driver fields corrected (2026-10-09)

**Problem.** The horn's per-car driver bits read the wrong vault fields: "horn enabled" from `Hash_B5C60C1D43899F74`
and "blocked kind 4 vs 5" from `Hash_7C6B48BD9ADF8E6E`. The offsets came from b41's field order, which was the
exported JSON sorted by hash, not the schema.

**Retail [code + data] (b74 in `.local/research/npc/b69-traffic-manoeuvres.md`; main read the bit roller
`82C42348` at .78.cpp:7866: `[car+4116]` +20 -> bit 0x02, hash C423B7E0 -> 0x08, +12 -> 0x04, +16 -> 0x10, +36 -> 0x01,
then hash 99083122 -> +4402 0x80; schema offsets from `vault_layout.py 02AA3F538D6C188A`).** Horn enabled = +36
`Hash_50E084076390A573` (1.0 in every stock driver); blocked long = +20 `Hash_20E9C6487FDDBDE8` (0.5, normal 0.3,
reckless 0.8). `7C6B48BD` (+24) is the lane-change direction roll, `52CF2CF3` (+28) the lane-change go roll.

**Change.** `vehicles.rs` loads +36 / +20; `HornParams` docs and default (blocked long 0.5) updated; mod doc updated.
In play: taxis now honk like every other car (the old 0.2 gate was not retail), and the blocked horn picks kind 4
or 5 by the driver's 0.3 to 0.8 chance instead of almost always kind 4.

**Verification.** skate-core traffic 47, skate-game vehicles / living_world 71 pass. Not play-tested.

## Traffic: cars change lane (2026-10-09)

**Retail [code + data] (b69 / b71 / b72 / b74 / b76 in `.local/research/npc/b69-traffic-manoeuvres.md`; main read
`IsRequiredToChangeLane` `82C39F78`).** On a lane, the decider rolls a lane change every lane-timer period (spec+48,
10 s; go chance driver +28: 0.1 default, 0.4 taxi, 0 for fast / normal / reckless drivers) on roads whose `word_56`
has bit 0x02. The change starts when pass = ext x the passage factor (spec+40) + speed and h = pass / 2 satisfy:
speed > h, free distance ahead > h, d > ext, d + pass before the road end minus ext, and the gap check on the target
lane passes. The car is registered on the target lane at once, rides the Hermite passage at its speed and faces its
tangent, and takes the target lane when the passage ends.

**Change.** skate-core `follow::step` runs `manoeuvre::decide` for cars on a lane (pull-over passed as off for now),
starts the passage on the retail conditions, advances it instead of the cursor, sets the lane at the end
(`FollowEvent::LaneChange`, then `EnteredLane`); `Car` keeps `decider` and `passage`, `Car::on_place` puts a changing
car on its target lane for the lane lists and the lead scan, `Car::pose` draws it on the curve; `FollowParams` gains
`manoeuvre`, `passage_factor` (2.0 until the loader maps spec+40), `passage`, `spots`. skate-game draws cars from
`Car::pose` and emits `TrafficEvent::LaneChange`. `Passage` is plain `Copy` data (fixed 32-chord table).

**NOT RETAIL YET / open.** ext = half the car length (inferred); the decider's per-car values still use the defaults
(the spec / driver loader for spec+40, spec+48, driver +24 / +28 comes with the pull-over wiring); the follower's
acceleration chain is still the old one (todo `traffic-accel-fields-mislabelled`); the lane load sums our car
lengths (retail: extent + safety distance).

**Verification.** skate-core `a_car_changes_lane_along_its_passage` (lane 0 to 1, sideways on the way) and traffic
51; skate-game vehicles / living_world 71 pass. Not play-tested.

## Traffic: cars pull over, park and pull out (2026-10-09)

**Retail [code + data] (b73 / b74 / b76 / b77 in `.local/research/npc/b69-traffic-manoeuvres.md`; main checked the
three passage builders' callers and the road record copy).** When the lane timer fires and no lane change starts, a car
on the outer lane of a road whose `word_56` has bit 0x01 rolls the pull-over chance (driver `Hash_559BA807`, 0.02, taxi
0.04; 0 while a skater holds it unless it has the "pulls over while held" trait, taxi 0.2) and reserves a stop spot
(the road's middle when free, else the middle of the first long enough gap ahead within 0.75 of the road, at least the
approach length ahead; approach = ext x spec+36). FollowingLane then treats spot - d + 0.1 as a stop target (kind 2,
standoff = approach), so the car crawls to it; below 1 m/s within the approach length it rides the Hermite passage to
the kerb slot (one lane past the outer lane), leaving the outer lane halfway, and parks there with the spot still
reserved. After the parked time (driver `Hash_988BB0F6`, 30 s, taxi 20) it rides a passage of one approach length back
to the outer lane and the spot is freed.

**Change.** skate-core: `manoeuvre::Manoeuvre` (Following / PullingOver / Parked / PullingOut), `pull_over_stop`,
`is_required_to_pull_over`; `follow::step` passes the road's pull-over bit and a spot search over the other cars'
spots (pending, pulling over, parked) to the decider, adds the spot stop target, starts the pull-over passage,
brakes to the curve's end, parks, times the stay and starts the pull-out; `Car::on_place` takes a parked car (and one
past the middle of its pull-over) off the outer lane; `Car::pose` draws a parked car on the kerb slot;
`FollowEvent::PullingOver / PullingOut`. `FollowParams` gains `approach_factor` (spec+36), `pull_over_slack`,
`pull_over_speed`, `parked_time`. skate-game loads spec+40 / +36 / +48, the driver's lane-change, overtake,
pull-over, held-pull-over and parked-time values, rolls the trait at spawn and emits `TrafficEvent::PullingOver /
PullingOut`.

**NOT RETAIL YET / open.** The reservations are derived from the cars each tick (same set as retail's list); before halfway retail only follows a car
ahead that is itself in a passage (ours follows any); retail has no position clamp at the spot (ours keeps the
follower's clamps); the acceleration chain is the old one (todo).

**Verification.** skate-core `a_car_pulls_over_parks_and_pulls_out` (spot 200 m on a 400 m road, kerb slot 4 m
aside, parked at 0 m/s, back on the lane past the spot), `pull_over_tests`, traffic 53; skate-game vehicles /
living_world 71. Not play-tested.

## Traffic: the car alarm on parked cars (2026-10-10)

**Retail [code] (spec `.claude/notes/world-traffic-audio.md` "Car alarm trigger", recomp session 2026-10-04 16:11).**
StayingParked's begin / end (`82C39120` / `82C391F0`) set and clear `+3424` bit 0x80; the collision callback
`sub_82C3C150` only tests a parked car: a contact whose vector `m+48` is longer than the spec's 0.1 sets the alarm
(`+3424` bit 0x10) and zeroes the alarm timer `+3716` and the parked timer `+3712`. StayingParked's update `82C39138`
then holds `+3712` at 0 while the alarm sounds, and `IsRequiredToPullOut` (`82C3A3A8`) is false while bit 0x10 is set.
StopAlarming (`82C3A4D0`) clears it after 8 s; the parked time then runs again from 0.

**Change.** skate-core: `Car::alarming` (host-set each tick); `follow::step` keeps a parked car's time at 0 and skips
the pull-out while it is on. skate-game: `drive_traffic` puts `VehicleParked` on a car entity while its manoeuvre is
Parked (removed when it leaves), reads the alarm back from the audio bridge every tick (`VEHICLE_ALARM` log on a
change); `apply_vehicle_hits` sends a `VehicleImpact` (relative speed at the contact, `ImpactSource::Player`) for every
skeleton contact with a car proxy, so the existing alarm rule (`game_audio/car_alarm.rs`, data `world_tuning.vehicle_alarm`,
mod `sdk.world_audio.alarm_rule`) sets the alarm off and every further contact restarts it. Moddable as before: the
rule's threshold / length / enable, mod `impact` events, the `parked` option.

**NOT RETAIL YET / open.** Only the player's skeleton contacts reach the car (retail's callback sees any collider:
board, peds, props, NPC skaters); whether retail's `m+48` is a speed or an impulse is open (ours: relative speed, the
VEHHIT reading).

**Verification.** skate-core `an_alarming_parked_car_holds_its_parked_time_and_stays` (parked time held at 0 for 10 s,
no pull-out, then the full 2 s parked time after the alarm); skate-game `a_parked_car_is_marked_for_the_car_alarm`,
the car alarm rule tests. Not play-tested (DownTown: wait for a car to pull over, walk into it).

## Car materials: the retail vehicle shaders (2026-10-11)

**Problem.** Car paint and glass were estimates: V3 tinted a copy of the body texture with an inferred channel rule and
a paint gain of 2.0 chosen by eye, and drew the glass with Bevy PBR at 0.55 opacity. The vehicle shaders were not
decoded.

**What retail does.**
- Programs [code, shader microcode of TU3 `shaders_final.big`, read with the offline Xenos decode; constant tables
  parsed from the `.fpo` objects]: material kind `vehicle_chassis` runs `vehicle_defaultPS`, `vehicle_glass` runs
  `vehicle_glass_defaultPS`. Constants: `g_vLightDir` c9, `m_params` c14..c19 (rows 0..5), `i_params` c20,
  `i_colorize_red` c21, `i_colorize_blue` c22; samplers `i_diffuse` 3, `i_normal` 4, `i_environment` 5 (glass: no
  diffuse, `i_normal` 3, `i_environment` 4).
- Paint: `lin = diffuse^2`; a texel with `G^2 < 0.001225` becomes `R^2 x i_colorize_red + B^2 x i_colorize_blue`, any
  other keeps `lin`. The same rule as the ped shaders (doc 26f). Secondary -> red, chassis -> blue (the pair that makes
  the base palette `(1, 0, 0)` / `(0, 0, 1)` the identity [data]). No gain.
- Light (both programs): `albedo x (ambient + max(N.L, 0) x m_params[1].rgb x S) + (pow(sat(V.reflect(L)),
  m_params[2].w) x S x m_params[2].rgb + env^2) x (1 - sat(N.V))^m_params[1].w`, the body's term times `diffuse.a^2`;
  ambient = the shader literal `(0.1, 0.175, 0.3)` x `m_params[5].w`; S = the shadow lookups, zero where `N.L < 0`;
  times `m_params[0].y` and the vertex fog factor, plus the fog colour; then the exposure / tone tail shared with the
  character shaders. Normal map: x in alpha, y in green. Environment lookup at `(R.x, R.y, -R.z)`, R = reflect(-V).
- Glass: albedo `m_params[4].rgb`, output alpha `i_params.x x m_params[4].w`; body output alpha `i_params.x`.
- Rows [data, every car arena]: each part names its vault row with `AttribulatorMaterialName`: body (`Accessory`, and
  `Misc` wheels of reda_car) `character.livingworld_vehicles`, windows (`Equipment`)
  `character.livingworld_vehicles_glass`; `sub_827E4F58` splits the name into class `material_` + prefix and key
  [code]. Both rows inherit `livingworld`; glass row 4 = `(0, 0, 0, 0.75)` (black glass at alpha 0.75), body row 5 w =
  0.3, glass row 5 w = 0.5. The class is `material_character`, not `material_vehicle` (whose rows no car uses).
- Render state [code, technique setters `sub_82CEF720` (body), `sub_82CF0B90` (glass), state tables built in
  `sub_82CF5A10`]: body opaque, depth write on; glass SRC_ALPHA / INV_SRC_ALPHA blend, depth test on, depth write
  off, back-face cull.
- `i_params.x` [code, `sub_827C1188`, partly read]: min(distance fade, fade-in ramp +1/30 per update clamped to 0..1;
  the ramp at model +576 checked in the recomp code). 1 for a car in full view.
- Environment cube [data, code]: every car material samples its own cube, texture `0x2c7f38180017266e` in
  `livingworld.big` (256 x 256 DXT5, 6 faces, a baked city panorama); the vehicle binders `sub_827C1720` /
  `sub_827C39F8` never rebind `i_environment` (name hash `0x41C2B2E0`; the sampler binders `sub_82CEF028` /
  `sub_82CF04C8` take the material's).

**Change.**
- Converter (`living_world_vehicles.py`, VERSION 3): material extras `attrib_material` from each part's
  `AttribulatorMaterialName`; `vehicles.json` `materials` (`m_params` rows per name, vault inheritance applied),
  `material_kinds` (the name each kind used) and `environments` (cube id -> `vehicles/env_<id>.png`, the six faces
  stacked top to bottom in native order like the map cubes; `decode_cube` reads DXT5 cubes, which the vendored
  decoder rejects).
- Engine: `skate-game::living_world::vehicle_material` (`VehicleMaterial`, `vehicle_material.wgsl`) ports both
  programs: the paint rule in the shader (the palette pair is a uniform, no tinted texture copies), the light terms
  above with the character lighting's light and Bevy's directional shadow, the cube, glass alpha and blend. Like the
  character shader it leaves the vertex fog and the tone tail to the frame passes. `present_car_looks` binds it;
  `update_car_light` follows the light when the world changes. `PAINT_GAIN` is 1 (retail), `GLASS_ALPHA` is gone.
- Mod surface: `VehicleOverrides::material_params` (rows per material name), `VehicleOverrides::environments` (cube
  per id, `""` = no reflections), `paint_gain` (scales both colours); a mod GLB names its row with material extras
  `attrib_material`, else gets its kind's retail row; without a row it keeps its own glTF material.

**Verification.**
- Tests: skate-game `living_world_traffic_material_rows_resolve_by_name_kind_and_mod`,
  `living_world_traffic_material_uniform_is_the_retail_constant_set`,
  `living_world_traffic_environment_strip_becomes_a_cube`; converter
  `test_material_rows_and_environments_come_from_the_built_cars`; the paint rule's tests are the ped colorize tests
  (skate-core `peds::colorize`). All living-world tests pass (skate-game 83, skate-core 276, converter 15).
- Export from the disc: all 17 cars resolve to the two rows; the cube decodes to six clean faces (sky on +Y, ground
  on -Y).
- In game (DownTown, `--verify` captures with a camera mod): paint in the palette colours, the red mask (stripes)
  in the secondary colour, dark see-through glass with the cube reflection, no shader errors. Not compared side by side
  with the recomp.

**Open.**
- The distance fade / fade-in of `i_params.x` (near / far keys `0x9FCFDBEA56BA4733` / `0x73B6874C7B46C7C6`, gating
  byte); cars draw at 1.
- The vehicle vertex shader did not disassemble (`xenos_disasm.py` fails on the `.vpo`): world-space interpolants are
  inferred from the pixel shader.
- The stencil tags (128 body, 255 glass) and what reads them.
- NPC peds still draw with Bevy PBR and the baked tint; the same lighting port would apply (follow-up).

## Grinding on cars (2026-10-14)

**Problem.** Living-world cars could not be grinded: no car rails were exported and the rail query had no car provider.

**Retail.** 826C0630 asks the DMO provider and then the vehicle provider (LWVehicleMan, world +2704) with no condition
between; both run 82C4C4E0 (15 m cull, the car matrix, the rail map +1796, the shared 40-record cap; no moving, parked
or speed test). Car setup 82C3CCD8 fills the rail map from the same definition as the skitch grab splines
(82C2A8C8 -> +136: rail count +8, list +24; grab count +12, list +28).

**Evidence.** [code] above. [data] 33 of 39 car part arenas in livingworld.big have one EB0004 section (211 rails,
flags 0, none closed) and EB001B +8 equals its rail count in all 33; `reda_car` has none. Rails sit on roof edges,
hood / trunk / bumper edges and pickup bed rims. Not confirmed in a live run (the user was not sure whether retail
cars are grindable).

**Change.** Pipeline `living_world_vehicles.recipe_grind_rails` (the grab splines' arena, count check) into
`vehicles.json` `models.<record>.grind_splines`. skate-game `VehicleModel::grind_rails`, `car_rails` (pushed each tick
from `push_vehicle_grab_splines` into `GamePhysics::set_car_rails`), and the car pass after the props in
`grind_world::RailView` (`RailRules::cars`, retail on). Owner: tag bit 63 | car serial << 16 | rail index. Same
consumers as the prop rails (doc 26i "Grinding on props").

**NOT RETAIL YET / open.** Whether retail ever takes a car out of the provider (AI-driven, despawn) is not decoded.

**Verification.** Pipeline `test_grind_rails_come_from_the_grab_arena_and_match_its_rail_list` and the disc check
(39 arenas, 211 rails); skate-game `a_car_provides_its_grind_rails_at_its_current_transform`,
`rail_view_puts_static_rails_first_then_props_then_cars_under_one_cap`; skate-game 693 pass (1 known failure).
Install after refresh: 18 models, 107 rails. Play test pending.

## Skitching session logging (2026-10-15)

**Problem.** Skitching wrote no session log lines, so a play session (planned by the user: "Third session will be
trying out skitching") could not show where a grab failed or let go.

**Change.** Three lines, all on change only:
- `SKITCH_LATCH <before> -> <after>` (`physics/ground_runtime/skitch.rs`): a grabbable spline (kind, id; kind 1 =
  traffic car) comes into or leaves latch range, with the time to the spline, the candidate count and the opt-in.
- `SKITCH_ENTER` (`physics/skitch_state.rs`): the grab starts: record kind and id, the car serial, board speed.
- `SKITCH_MODE <from> -> <to>` (same file): sub-mode changes and the hold's release, with the position along the
  car, tow speed, side distance, hands off and the grab input.

No behaviour change.

## Skitching crash on the first grab (2026-10-15)

**Problem.** The first live skitch (user's session 3, Skitching opt-in on) crashed the game the moment the grab
latched: `SKITCH_LATCH None -> Some((1, ...))`, then `PhysicsGround -> Skitching`, then a panic at
`physics/player_state/transition.rs:37`.

**Root cause.** `set` routes Skitching to `skitch_state::enter` (and a no-op exit), but the `PhysicalStateCalls`
`enter`/`exit` assertions in the same file list every supported state except Skitching, so the lifecycle publication
asserted before the state's own Enter ran. The registry test only checked that the transition is allowed, not that
it can be published.

**Change.** Skitching added to both assertion lists. New test `lifecycle_call_tests::calls_accept_every_state_set_routes`
runs every state `set` enters (and Sleeping on exit) through the Calls assertions.

**Verification.** The new test passes; the user's next skitching session is the live check.

## Skitching holds: crash, held car, hands and sub-mode writes (2026-10-16)

**Problem.** Session 3 (2026-10-16) crashed on the first grab ("PreState requires the selected state's actual
PredictFutureOfDeck; state=Skitching"). After that fix, session 4: "skitching sorta works, i had to try like repeatedly to grab the car and when it finally did, my arms were all crossed over eachother and i was immediately flung by the car. I couldn't really control like you can in retail leaning with the turns to keep skitching the car." and "both arms just grab spots on the back of the car, not the shoulders of the car." The log showed one latch, sub-mode 1 for 6.7 s, then the release with both hands off.

**Root causes and changes** (all [code], TU3 recomp read by main):
1. PreState and FillPhysOut had no Skitching entry. Skitching's deck prediction is vtable `0x82327398` slot 6
   (`82D34DA8`); added to `pre_state::predicts_deck` and `publication::has_output_owner`. Test
   `every_supported_state_has_a_deck_prediction_and_an_output_owner`.
2. Step 14 of the gated body (`82585DD8`) re-requests the held car's grab spline every tick; ours published the record
   once, so the hold dropped after 2 ticks. `skitch_state::update` now calls `offboard_grab.request_primary(car)` each
   ready tick (`vehicles::grab_object_at` builds the car's grab object).
3. `82D49580` reads the hand-off flags (1328 / 1332, 1 = off) directly; we passed them inverted.
4. Crossed arms: `82D48148` re-orients the held record copy itself (state+1024): when the board side
   (Processed+128 row 0) points against the direction (+112) it swaps +64 / +80, negates +112 and toggles the reverse bit
   (+200 bit 0x20, read by the spline walk `82D2D550`). Ours flipped only a local copy inside the frame step, so the
   hands (`82D4A378`: grip parameter = posed hand 908 + the hand part's skeleton-local x) and the grab point used the
   un-flipped edge and each hand reached to the mirrored point. Headless: shoulder A +0.13 m across the car, target A
   -0.08 m before; +0.17 m after. `skitch_state::update` re-orients endpoints and direction before the frame step.
5. The per-mode writes of `82D49AB8` were missing (switch on 1324): 0 `904 = clamp(864, +-928)`; 1 both hands grip
   again (`1328 = 1332 = 0`) every tick; 2 the edge point `+-932` latches into 900 once (1344 bit 0x02), a snapped hand
   (0x01) takes it as 864, the push timer 960 counts (868 and 836 same sign, |868| > 50), the outer hand lets go
   (836 > 0: A off); 3 the same latch at `+-(928 + 0.8)` (`Hash_AED1D7B20DA7CBBA` = 0x3F4CCCCD, checked in the export),
   both hands off. Without the mode 1 write one transient "off" stayed for the whole tow and the tail let go.
   New `skitching::mode_writes` (+ `SkitchSubModeSettings::step_off`), run after the shimmy chain as in retail's call
   order (`82D48148`, `82D49580`, `82D48C98`, `82D49AB8`, `82D4A378`, `82D4AC38`, `82D4A0C0`, `82D47BD8`, `82D4AFB8`);
   the shimmy now snaps to 900 (it read the hold target 904). The 960 timer moved out of the sub-mode step (retail
   counts it only in mode 2, and `82D49580` reads last tick's value).
6. Leaving sub-mode 0 writes `908 = 864 = 836` and `948 = 0` (`82D49580`); ported (`SkitchSubMode::left_zero`).

Checked and NOT changed: "tows fast" (1345 bit 0x40) is rewritten every tick in `82D48148` (r29 = 0, set to 1 only
when 832 >= 3.0, `Hash_1F85F500908C5E17` = 3.0), as ours does; the research agent's "sticky" reading was wrong.

**Verification.** Headless `a_skater_latches_onto_a_traffic_car_rides_and_lets_go` now asserts that each gripping
hand's target is on its own shoulder's side of the bumper and that sub-mode 1 keeps both hands on for at least a second
(it fails with the re-orient disabled). Towed at the car's speed with a steady 0.47 m gap; the spring stays within
+-6 N where it swung +-500 N before. With the left stick held the hand target shimmies along the bumper (0 to 0.74 of
0.78) and the lean yaw follows (0.2 rad), so the stick reaches the skitch. skate-core `each_sub_mode_writes_its_hands_and_edge_point`. New log line `SKITCH_SUBMODE` (every `82D49580` switch with
stick, turn, spin).

**Open.** The latch needing repeated tries (research b140). Closed 2026-10-29 (research b687): in state 104
`sub_82D4AC38` calls the Ground tilt `sub_82D92440` with no history unless 1345 bit 0x20, so the truck tilt reads the
stick, as ours does (`skitch_state.rs`, `calculate_tilt(.., input.steering, None, None)`). The grab point is still the chord, not the polyline walk with the reverse bit.

## Skitching: the latch's own limits and the hand grab sound (2026-10-16)

**Problem.** Session 4: "i had to try like repeatedly to grab the car". Session 5: "Skitching works better but seems to push you off sometimes.", "Additionally skitching sounds incorrect when your riding behind the vehicle", "one of the vehicles forced me off even though it was barely moving in a straight line."

**Latch limits** [code + image, research b140, main checked]. `82D39D98` re-runs CanGrabSpline (`82E08DB8`) with its
own limits: margin `0x82165A10` = 0.0, angles `0x8209918C` = 60 and `0x82099190` = 40 degrees (r17 / r16, times the
degree constant `0x8206D110`). Ours reused the query's 0.25 / 60 / 30, rejecting cars near the spline ends.
New `SkitchQuerySettings::latch_margin` / `latch_angle_a_degrees` / `latch_angle_b_degrees` and
`ground_runtime::skitch::latch_limits`; test `the_latch_rechecks_with_its_own_wider_limits`. The latch window itself
(bumper within reach 1.0 + 0.1 s of closing ahead of the root) matched retail already. Open from b140: the spline point
(retail: the line's crossing of the box's x = 0 plane, ours the closest chord point), +2668 height, the query's box vs
sphere test.

**Hand grab sound** [code + data, research b141, main checked]. The packer `sub_827A1B78` sets record `+152` bit
`0x100` while the state is 104 (`0x827A29EC`); the bridge `sub_824B0DA8` copies it to audio state `+744`; its only
reader `sub_824EC488` (HandGrabs, local player only) starts the `aud_splicer` `handgrab` pair (Skate_Collisions
0x3C5, Skate_Metal 0x1D0, checked in the vault dump) on the eEQChain bus of `CCDFABBAEEA33EAE` (= 2 in
`aud_randomeqtuning` default) at the rising edge, and `HandGrabs.in0` = 32767 while latched (the MixMap fades the
SkateBoard object by it). Rolling itself has no skitch case in retail. Ours had none of this. New
`skate_audio::player::hand_grabs` (`AudioState::skitching`, `PlayerTuning::hand_grab`, exporter `HANDGRAB` /
`HANDGRAB_EQ`); tests `a_skitch_starts_the_hand_grab_pair_once_and_holds_the_input`,
`other_skaters_play_no_hand_grabs`, `test_hand_grab_pair_and_bus_come_from_the_splicer_and_eq_records`. The input is
written at the next MixMap write after the latch (one write later than retail's same-update write).

**Pushed off.** Not reproduced headless: a 0.6 m/s car straight ahead held for 300 ticks. The session's early
releases were all sub-mode 1 with both hands off, which only the hands step can set (lean yaw outside -60..35 /
-35..60 degrees, or a shoulder more than 1.6 m from its grip point). The release log line now carries `lean_yaw_deg`
and `reach` so the next session shows which. Open.

## Skitching: the stick frees a hand held at the edge (2026-10-16)

**Problem.** Session 6: "There were times that I was stuck on the side edge of the car skitching and couldn't move away from it and recenter myself by shifting to the center of the car, this also happened while going straight and not just in turns or curves." The log shows the along position at -1.0 to -1.3 of the half length for seconds, sub-mode 2 -> 1 with the stick at 1.00 and the position unchanged.

**Root cause** [code, main read `82D48C98`]. At `82D48FA8..82D49034` (r22 = 1, r25 = 0) r28 = active (|924| > 0.01 or the
event 1000 != 0); with r28 set the chain clears 1344 bits 0x02 (edge latched) and 0x01 (snapped), only an idle hand keeps
them (r27, read at `82D492D4`, `82D493D0`, `82D49478`). Ours cleared them only while idle and unlatched, so once a hand
latched the edge in sub-mode 2 it stayed snapped there (`target = 900` every tick) and the stick could not move it.

**Change.** `ShimmyState::step` clears `latched` / `snapped` when active and uses the kept latch as `hold`. Test
`the_stick_frees_a_hand_snapped_to_the_edge`. The session also showed long tows (9 s, 14 s) ending on the user's
release, and no hand-off releases in sub-mode 1 after the per-mode writes.

## Cars collide with their retail chassis mesh (2026-10-16)

**Problem.** Session 7: "I did however hear a noise like the collision for the player and the car were colliding or something as i was skitching the whole time. It started when I grabbed the car and stopped only when I let go or was flung off." The log: 54 body-poster arm hits (regions 2 / 3, material 100) and 46 BodySlide posts in a 14 s tow; headless, the upper arms (parts 5 / 9) touched the car every tick.

**Root cause** [code + data, research b143, main checked the install]. The exporter builds each car's chassis mesh
(`recipes/<recipe>/glb/collision`) but the model entry the game loads copied only the grab and grind splines: the
installed `vehicles.json` had the mesh on 17 of 18 recipes and on 0 of 18 models, so every car collided as its bounds
box. The box's rear face stands vertical behind the bumper, between a skitcher's hands and shoulders. Retail has no
held-car exception (group pair 5 / 8 allowed in `82765EF0`, state 104 changes only the 82D91298 parts, the `82BD80D8`
held object argument is unused by `82BD4A30`), so the mesh is the fix, not a filter.

**Change.** `living_world_vehicles.py` copies `collision` onto the model; `vehicles::parse_vehicle_data` falls back to
the recipe's mesh for installs exported before this (no setup refresh needed). Tests
`a_model_without_its_own_collision_takes_the_recipe_mesh`; the headless skitch test now tows behind the taxi's retail
mesh and asserts no arm contact while towed in the open (ticks 10..70; with the box: contact every tick).

**Open.** A session confirming the arm hits are gone. Every traffic collision (car knock-downs, NPC skaters) now
uses the mesh too; `a_traffic_car_knocks_the_skater_down_above_the_contact_limit` passes with it.

## Skitching hitches: audio files read on the game thread (2026-10-16)

**Problem.** Session 8: "considerable hitching that made it difficult to hold onto the cars as I skitched." The hitch
rate was the same in sessions 7 and 8 (0.9 per second); the large ones (150 to 870 ms) are the `audio` stage of the
`FRAME_HITCH` line ([doc 25](25-perf-diagnostics.md)), while towed fast, not at the latch.

**Root cause** [log + code]. A sound's first play reads its file from disk on the game thread, and the install sits on
an HDD. In `game-20261016-142030`, 6 of the 8 audio-stage hitches over 100 ms after loading start in the frame a sound
first plays: a living-world speech line 3 times (257, 181, 723 ms) and a location set one-shot 3 times
(`delivery_truck_in_2` 153 ms, `Tire_Squeals` 867 ms, `Siren_city_4` 260 ms). The speech take is read and decoded in
`world_speech::Voices::ensure` while the native runtime lock is held, so the audio render waited too
(`audio_lock_wait` 713 ms at 20:23:09, an audible gap). The set one-shot is read in `Library::clip` from
`random_sets::update`. The other 2 (264, 395 ms) have no log event. Note: the hitch line's `audio` stage runs from the
end of Animation to the end of `CueSet`, so other Update systems can land in it.

**Change (step 1).**
- Location sets read a bank's files when the scheduler picks the entry, on an `audio-warm` worker thread
  (`Library::warm_bank`); retail's EmitterSystem also loads the bank at the pick and plays it seconds later. `clip`
  builds the clip from the read-ahead bytes (the same bytes), and reads on the game thread as before when they are not
  there yet. Bytes of an entry that unloads unplayed, or of a rebuilt set, are dropped (`cool_bank`).
- `FRAME_HITCH` gains `audio_parts=[...]`: always-on wall time of the audio frame systems, the game-thread bank loads
  (`bank_load`) and first-play file reads (`file_read`), those over 1 ms, largest first (`game_audio/hitch_parts.rs`).
- Test `read_ahead_gives_the_same_clip_bytes`.

**Change (step 2, speech).** Research `speech-stream-start` [code, recomp reference, main checked our side]: retail's
start (`sub_82C5DC80`) takes the stream in the same update (active, speaking byte, interrupt rules, queued requests
wait, stop works) and leaves the data to the library's stream player; the game code has no ready check for normal
lines (only scripted dialogue holds a stream, `sub_824A4EE8` / `sub_82C5E7C8`). So the port keeps every logical step
at the request and only the voice waits for its data:
- `SpeechVoices::prepare` (skate-audio): Ready / Loading / Failed. A Loading line keeps its stream with no voice; the
  cut and speaker checks run as for a playing line; the voice opens once the take is in (one `Started`). Failed ends
  the line as a failed open did. The default is Ready, so other hosts are unchanged.
- The game's speech host reads and decodes takes on a `speech-takes` worker (`world_speech::TakeLoader`, keyed by
  path, 16 uncollected takes kept). A take the worker could not read answers Ready, so the old read runs and reports
  the missing decode once, as before. Headless tests keep the read at the start (`cfg(test)`).
- The read no longer happens under the native runtime lock in play, so the audio render no longer waits on it.
- Tests `a_loading_take_holds_its_stream_and_opens_when_it_is_in`, `the_take_loader_decodes_like_the_game_thread`.

**Difference to retail, kept on purpose.** A line becomes audible when its take is read (a warm read is a few ms, a
cold HDD read up to the stalls above); retail's audible start depends on its own stream buffering, which the
research could not decode. The logical timing (stream taken, speaking, queue) is unchanged and deterministic.

**Open.** Retail delays a line's start by the event's speech tuning `+0` and blocks a stream for tuning `+4` after a
line ends (setup exports both as `unknown_0` / `unknown_4`, `world_audio.py:374`; the engine reads neither): a parity
follow-up.

**Verification.** Session 10 (2026-10-16, this change, 241 s, 5 tows, 16 speech lines started): no hitch over 100 ms
(session 9 on the build before: 20, 15 of them at a first play); the worst two (68, 52 ms) were render frames with
no audio system over 1 ms. User: "there we some minor hitches but nothing as bad as before." and "no notice of ped
lines but they really didn't talk much while i was playing".

## Skitching: both bumpers keep the one pressed first (2026-10-16)

**Problem** [code]. `PlayerControls.bumper_state_502` / `bumper_state_104` were never set, so the controller's
bumper arbitration (`82598FE0`, ported in `skate-core/src/input/controller.rs`) always dropped bit 28 when both bumpers
were held, also while skitching.

**Root cause / change** [code, main read the recomp]. Retail's caller `825999F0` reads the skater's current state
(`*(*(x+28)+16)`) and passes `state == 502` (off-board pushing) and `state == 104` (skitching) to `825992D8`; in those
states both bumpers held keep the one pressed first. `PlayerControls::update_for_physics` now sets both flags from
`physical.state.state_16` before the controller update. Test `both_bumpers_keep_the_first_only_in_states_502_and_104`.

## Skitching: the grab point follows the bumper spline (2026-10-16)

**Problem** [code]. The grab point and the hands' grip points were taken on the straight chord between the record's
ends (`skitch_state.rs`, NOT RETAIL YET note), so on a curved bumper they missed the curve sideways and in height.

**Retail** [code, research b148 (Ghidra + recomp asm), main checked the offsets in the pseudocode]. `82D2D550` looks a
point up by a signed position `t` along the record from the middle of its ends: at or past +-half (`+180`) the ends
(`+64` / `+80`); otherwise u = t + half (reverse bit `+200` 0x20: half - t); the geometry's points (`+196` -> `+48`,
count `+68`) are walked first to last, each segment counting its length projected onto the first -> last direction
(floored at 1e-4, `0x8209BE90`), and the segment that takes the total past u holds the point, to world by the record
frame. Callers: the frame step `82D48148` (`t` = state+836) and the hand targets `82D4A378`; the latch does not use it
(its point stays a separate leftover, b140).

**Change.** `grab_scene::at_along` (skate-core); the published records' geometry is kept beside the 72-word copy
(`SkaterRuntime::grab_geometry`, set where `82762AB0` copies the record); the skitch step builds its point lookup from
the re-oriented held copy (ends swapped, +112 negated, reverse bit toggled), the chord only without geometry. Test
`along_lookup_follows_the_spline_and_its_reverse_bit`.

**Verification.** Session 11 (2026-10-16, hails-additions 678aa1c1, with the latch point, bumpers and the hold fixes):
"skitching felt so much better and the hands were in the right spots both times I grabbed vehicles."

**Open.** Whether the geometry's points are the raw control points or a smoothed list (b148: medium, control points).

## Skitching: the latch point is the edge's centreline crossing (2026-10-16)

**Problem** [code]. The latch (`82D39D98`) timed each candidate edge from the point of its chord closest to the skater's
centre of mass (`ground_runtime/skitch.rs`).

**Retail** [code, research b149, main checked the formula in the pseudocode]. Both edge ends (swapped so the second has
the larger box-local x) go into the query box's frame; the point is where the edge crosses the box's local x = 0 plane,
the skater's centreline: t = l1.x / (l1.x - l2.x), unclamped, no fallback (a parallel edge gives NaN, and the time's
fsel turns it into 0). The query's segment test (`82ADD910`) is against a sphere of radius |extents| at the box
centre (`82ADD6A8`), which ours already does.

**Change.** `latch` takes that crossing (`bounds.frame`: right, up, forward, centre). Tests: the skitch latch tests and
the headless tow `a_skater_latches_onto_a_traffic_car_rides_and_lets_go`.

`+2668` [code, research b150, main checked the store in the recomp]: P.y minus Processed+176 .y, the effective board
transform's row 3 (the deck position, written only by `82C013F0`; ours `BoardToolkit::effective[3]`), set for every
gated candidate before the time gate. `query` now takes the deck and writes it (`LatchDecision::height`). Test
`the_latch_height_is_the_crossing_above_the_deck`.

**Open.** `+2736` and `82D91298` on latch (documented above); who reads the published height (block +32 +280).

## Cars: retail's per-car acceleration, speed style and hit-brake latch (2026-10-16)
- **Problem (research b162 / b163, the b75 / b78 port never applied):** the loader put the spec's lane-change passage
  factor (`328B9F46`) into the max acceleration and the pull-over approach factor (`75822921`) into the planning
  deceleration; every car of a spec pulled away alike; the cap was `lane x (1 + 0.2 held)`; the hit brake faded as
  the car slowed (`accel = -speed` each step).
- **Retail [code + data] (main re-read `82C3FF38` and `82C3FA08`):** at spawn (`82C3CDD0`) each car draws its max
  acceleration uniformly in [spec+52, spec+56] in 1 % steps (2.0..5.0 default, 2.5..6.0 sports / taxi / truck), a
  speed style from the driver's normal / slow / fast chances (default 0.8 / 0.1 / 0.1: slow `1 - spec+24`, fast
  `1 + spec+32`) and the held bonus spec+28 behind the driver's `D0BA2E02` roll (1.0 in every stock driver). The
  integrator's cap is `(1 + bias + held bonus while held) x the lane speed`; it stands a car at `accel <= 0.001 &&
  v <= 0.01`. The hit brake sets `accel = -v`; past `-DD49E3C2` (5 m/s^2) it latches (`+0xD60` bit 0x08: the planner
  is skipped) until the car stands.
- **Change:** `follow::spawn_draws` (one seeded stream per car, `derive(seed, "SPDC")`), `Car::accel_cap` /
  `speed_bias` / `held_bonus` / `brake_latched`, the cap formula, the latch and the standstill thresholds; the loader
  reads the spec and driver fields above and no longer writes `accel_max` / `plan_decel` from the wrong fields.
  `cap_scale` still multiplies the lane speed (the mod hook, 1.0 = retail); every new value is a `FollowParams` field
  a mod's traffic parameters can set.
- **NOT RETAIL YET:** the ramp (retail adds spec+60 per planner call; its cadence against the recomp trace is open,
  b75 O1) and the clamp before the step (retail zeroes the acceleration only after a step past the cap); the lead
  gap and the junction stop window (b162 M1, b163 item 3: `gap = d - 0.5 (s_me + s_lead)`, standoff `0.5 x s x
  73A1` / `s x 2ACE`, brake `-v^2 / (2 (gap - standoff))` inside `gap <= v + standoff`, a dead stop inside the
  standoff) are ported in the next section; which driver record a car gets (the default one here).
- **Verification:** `a_fast_hit_brake_latches_and_stops_in_about_a_second`, `spawn_draws_follow_the_spec_and_driver_values`
  (core living_world 327 pass); skate-game 715 pass; data-gated skitching and car-knock-down tests pass. Not
  play-tested yet: expect quicker, more varied pull-aways and some cars driving above or below the lane speed.

## Cars: retail's stop standoffs at lines and behind the car ahead (2026-10-16)
- **Problem (research b162 M1 / M3, b163 sections 2-3):** our follower braked by engine values: a comfortable
  deceleration and a hard-brake "dilemma zone" that let a car run an amber light, a 0.5 m stop margin, a 2 m minimum
  gap matched to the lead's speed, a lead search `look_ahead + speed + 2 + 10` m and the query window from
  `v^2 / (2 x 3.0)`.
- **Retail [code + data] (main re-read in the recomp asm: `82C376E8` window `vt+100 + vt+92 >= d` around the query
  `82E11E90`, `cmpwi r3,0` = any answer but Go stops; `82C41120` lead window `gap <= v + standoff` and bit 0x04;
  b163 for the spawn stores):** at spawn `+3660 = s x spec Hash_2ACE3FEE16D6F7A2` (0.5 default, 0.25 family / sports /
  taxi / truck, 0.35 minivan) and `+3664 = s x Hash_73A1989BB90C93BA` (0.5, family 0.6), s = the car's size along z.
  Junction: `d = lane end - position - 0.5 s`; the query runs inside `d <= v + +3660`; any answer but Go (Approach
  included) makes the line a stop target with standoff `+3660`. Lead: `gap = bumper to bumper`, standoff `0.5 x
  +3664`, inside `gap <= v + standoff`. The planner (`82C3FA08`, b75) brakes `-v^2 / (2 (d - standoff) + 0.001)`
  and stops the car dead inside the standoff; no lead-speed matching, no hard-brake clamp. `+4402` bit 0x04 is cleared
  when something is inside the standoff and only resets the horn timer `+3704`: there is no amber go-through.
- **Change:** `FollowParams::stop_standoff_factor` / `lead_standoff_factor` (loaded from the spec, mod-settable like
  every `FollowParams` field), `Car::junction_standoff` / `lead_standoff`; the junction block and the lead block use
  the retail windows and the stop formula; `look_ahead()` is the junction standoff (the query window). Removed:
  `plan_decel`, `hard_brake`, `stop_margin`, `committed`, `follow_accel`, `reach_accel`.
- **NOT RETAIL YET / open:** s = our model length (full length per b163 section 1, medium; one hook on `82E0BE68`
  would settle it, and the pull-over approach uses half the length, so one of the two is off by 2x); the query's own
  Approach test reads `+3660` (`entry_speed < speed && vt+100 <= remaining`, confirmed 2026-10-29, research
  `.local/research/b696-traffic-exit-room.md`); an engine safeguard keeps a car out of its lead's bumper when a fixed
  step overshoots.
- **Ported 2026-10-29 (b696, asm read):** the query's connector room check compares the full `+3664` (length x
  `Hash_73A1989B`, strict `>`, `82E11E90`), not the engine `min_gap` 2.0; the lane-change gate reads `+3752` = the
  gap to the lead alone (`IsRequiredToChangeLane` `82C39F78`), so an obstacle inside h no longer refuses a lane
  change (it did in ours). Open: what `car+4396 == 1` means, the zone lookup `82E147C8`.
- **Checked 2026-10-29 (research `.local/research/b688-car-following-gaps.md`, asm read):** the lead standoff is
  always half of `+3664` (`82C38390` -> `82C41120`), as ours; the lane occupancy adds `s + +3664` per car
  (`82C34430`), which supports `s` = the full length. `82E14F50` has no range: the lead is the previous node of the
  lane's list sorted by distance to the lane end, and only the `82C41120` window limits it, the same result as ours
  on one place (our search of the next connector / exit lane is an engine extension; retail sees those cars as
  look-ahead obstacles). The obstacle standoff is `+3668` = s x `Hash_30313EA6F282C874` (0.5, `82C3CDD0`,
  `stfs 3668`), read by `82C412D8` and the look-ahead quad: ported (`FollowParams::obstacle_standoff_factor` from
  the spec, `Car::obstacle_standoff`), replacing `min_gap` in both.
- **Verification:** core traffic 56 pass, incl. `a_car_brakes_by_the_standoff_rule_and_a_mod_factor_moves_the_gap`
  (window, brake value, dead stop, family factor 0.6 queues at 1.35 m) and `stops_at_red_and_goes_on_green` (holds at
  the standoff); data-gated DownTown 2 min: 113 junction entries, 0 on red, 0 dead ends; skate-game vehicles 14 pass
  (the spec loader reads both factors). Not play-tested yet: expect cars to queue closer (about a quarter of a car
  length) and to brake later and harder at lights, stopping dead at the line instead of running an amber.


## Skitching: the car is drawn on the ticks the skater met it (2026-10-18)

- **Problem:** laptop session 2026-10-18 22:01, the user, skitching: "it does jitter a tiny bit and struggle to stay
  with the car at high speeds".
- **Root cause [code, research `.local/research/b285-skitch-view-jitter.md`, main-checked]:** traffic moves after
  Physics (`drive_traffic` in the chain after `step_population`, `living_world/mod.rs:867`, `vehicles.rs:1239`), and
  the skater's solve reads the car proxies and grab splines from `CarMotion::curr` before Physics, so the skater of
  tick N met the car of tick N-1. The skater and camera are drawn between their snapshots N-1 and N, the car between
  `prev` (N-1) and `curr` (N): the car was drawn one tick ahead of the skater holding it, car speed / 60 (0.33 m at
  20 m/s, 0.5 m at 30 m/s), wobbling with the car's speed changes.
- **Change (drawing only):** `CarMotion` keeps one more pose (`drawn_prev`, `wheel_drawn_prev`); `present_car_pose`
  draws between `drawn_prev` and `prev` (`vehicles::drawn_pose`). Simulation, proxies, rails and grab splines are
  unchanged (still `curr`), so the result stays deterministic for multiplayer. Every car is drawn 16.7 ms later, the
  delay the skater already has. Moving `drive_traffic` before Physics was not done: it changes the simulation, and
  retail's update order is not documented in our notes.
- **Verification:** test `the_car_is_drawn_on_the_ticks_the_skater_met_it`. Not play-tested.
- **Open:** NPC skaters that skitch probably had the same gap (unverified). The tow itself may fall behind at high
  car speed (force cap 500, `skate-core/src/riding/skitching.rs`); a session after this change separates the two.

## Code reference: living_world/vehicles.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/vehicles.rs`; each comment there links its section here.

### Traffic cars engine side

Traffic cars (doc 26, milestone V3): turns the population's vehicle spawn / despawn records into visible cars that drive their lanes.

- **Entity** per spawn record: [`TrafficCar`] (stable `LivingWorldId`, entity / model keys, palette ids and colours, engine record) + `Transform` + #32's [`TrafficAudio`](crate::world_audio::TrafficAudio). [`TrafficState`] keeps the follower cars (`skate_core::living_world::traffic::follow`) and the id -> entity map.
- **Motion** (`FixedUpdate`, after the population step): one follower step per 60 Hz world tick, the signal clock ticked once per world tick (retail ticks its 4 controllers by 1/60 s per world tick, `sub_826B2C18`); a car spawned at tick `s` steps from tick `s + 1`. Lane state goes back to the population (`LivingWorld::update_lane`, `update_position`) so the census cull and the placement test see the real cars. A car at a dead end leaves (despawn, reason External).
- **Look** (`Update`): the car's GLB (`private/living_world/vehicles/<recipe>.glb`, or a mod's path from [`VehicleOverrides`]) as a scene, drawn with the glTF materials on render layers 0 and 28 like mod graphics; the `vehicle_chassis` and `vehicle_glass` materials run the retail car programs ([`super::vehicle_material`]: paint rule below, lighting, glass) with the material rows of `vehicles.json` `materials`; the six wheel bones spin with the distance driven over the model's wheel radius (`wheel_hint`, `Hash_FD7A66142F16B9CC`, equal to the wheel bone height [data, V0]). Drivers are part of the body mesh [data, V0]; no vehicle lights exist [data].
- **Collision**: one kinematic box per car from the GLB bounds (`mesh_bounds`), infinite mass, the car's velocity, joined to the skater solve through `physics::network::Proxies` like the NPC skaters. Solid contact only; bails and roof behaviour are V5.
- **Audio**: `TrafficAudio { engine: <spec engine_audio record>, speed: +3412, load: +3408 }` and `AudioVelocity`, so #32's traffic engine host (nearest 4 within 40 m, Doppler) plays it.
- **Events** ([`TrafficEvent`]): spawned, despawned, junction answer changed, entered a junction, entered a lane; the planned `sdk.living_world` events read these.

Paint rule (retail `vehicle_defaultPS`, the program of the `vehicle_chassis` material, TU3 `shaders_final.big`) [code, shader microcode]: the same rule as the ped shaders (`skate_core::living_world::peds::colorize`): `lin = diffuse^2`; a texel with `G^2 < 0.001225` becomes `R^2 x i_colorize_red + B^2 x i_colorize_blue` (constant table: `i_colorize_red` c21, `i_colorize_blue` c22, `i_diffuse` sampler 3), any other keeps `lin`; the shader writes `sqrt` at the end. The body atlases paint the car body pure blue `(0, 0, b)` and the base palettes are chassis `(0, 0, 1)` / secondary `(1, 0, 0)` [data, V0], so chassis -> `i_colorize_blue` and secondary -> `i_colorize_red` (the pair that makes the base palette the identity, as for peds). No gain in retail; [`VehicleOverrides::paint_gain`] scales both colours for mods (default [`PAINT_GAIN`] = 1, retail).

Multiplayer (no networking): a car's motion is a function of its spawn record, the world tick, the signal clock's tick count (= world ticks since the world loaded) and the other cars (which are themselves spawn records); the connector choice reads the occupancy, so a client must run the whole set of cars, not one. See doc 26 V3.

Moddability: [`VehicleOverrides`] (model per entity, GLB per model, colour per palette id, follower numbers per entity, connector choice); restoring `VehicleOverrides::default()` undoes a mod; the spec / palette defaults come from the export (`tables.json`, `vehicles.json`).

### Vehicle hits

The skater's body hit a car (`sub_82C3C150`, actor branch): a contact ahead of the car (ours: along its velocity; retail's axis is the car's vtable +24, inferred forward) latches the hit brake on the follower car. Logs `VEHICLE_HIT`. Every contact also goes to the car alarm rule as a [`VehicleImpact`](crate::world_audio::VehicleImpact) (relative speed at the contact; the rule sets a parked car's alarm off).

### Car look-ahead

V4 look-ahead (`skate_core::living_world::traffic::obstacles`): the obstacle lists (`sub_826B2EE0`: skaters radius 0 and peds, soft; movable props (DMOs) radius half their smallest extent, hard; traffic cars add nothing, `b42`) against each car's look-ahead quad from last frame's pose; the nearest free distance goes to the follower car. Ours: the ped radius is the fallback ped radius (retail 0.5 x the body extents' z, the per-ped value is collision data), props carry no corner points, a bailing skater's four extra points are not added, and the turn widening is off (the turn side `+3748` is open; threshold 0.025).

### Car grab splines

The cars' authored grab splines into the grab scene each tick (retail: the vehicle provider `82C35B98`, scene slot `+4088`, answering the riding skitch query's mode 255; b49). The car's world transform is the record frame, its velocity the record vector. NOT RETAIL YET: the assembly is a stand-in with the car's id (retail reads it from the car's `+172` interface, object open).

## Code reference: skate-core traffic/follow.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/follow.rs`; each comment there links its section here.

### Lane follower

The simple lane follower of milestone V3: census cars drive their lanes, take connectors by retail's least-loaded rule, stop at the stop line when the V1 junction query says so and keep a gap to the car ahead. The full retail driver (look-ahead `sub_82C412D8`, speed planner `sub_82C3FA08` with its lead / obstacle cases, horn and manoeuvre deciders, the state graph) is milestone V4; this module is the part of it V3 needs to put moving, light-obeying traffic on screen, with every simplification named below.

Retail [code, TU3; addresses are evidence only, behaviour re-implemented, not copied]:
- integrator `sub_82C3FF38`: `speed (+3412) += accel (+3408) x dt`; the accel is forced to 0 while the speed is above the cap `+3688` (= (1.0 + `+3680` (+ `+3684` while held)) x f2, b57); speed and accel are zeroed when both are tiny. [`integrate`] is that step.
- the cap: the lane's speed limit (14.167 / 13.889 m/s [data]); the recomp shows `+3688` = 14.167 on a 51 km/h road and 17.0 while skitched (`+3684` = 0.2) [trace, `npc-livingworld-re` §7d]. V3 uses the segment's limit on a lane and the exit segment's limit (at most the junction speed) on a connector.
- stop line (`sub_82C3FA08`): `accel = -v^2 / (2 (d - f2) + 0.001)` with `d` the distance to the line and `f2` the stop distance [code]. [`stop_accel`].
- a skater coming up behind (`sub_82C3FA08`, `+4403` bit 0x10 from the skater scan, limiter kind free): the car brakes toward the skater's speed minus 20 km/h and never accelerates ([`super::skater_scan`], b63; this was misread as a lead-following rule before).
- the junction query (`sub_82E11E90`, V1 [`junction_entry`]) runs once the stop line is within `speed + standoff` (`+3660` = length x spec `2ACE3FEE`, `82C376E8`); Go enters, every other answer (Approach included) is a stop target at the line with that standoff, so Approach slows the car until the query answers Go [code, b163].
- the car ahead (`82C38390` -> `82C41120`): a stop target at the bumper-to-bumper gap with half of `+3664` (length x `73A1989B`) as the standoff, inside `speed + standoff` [code, b162 M1].
- connector choice: least loaded exit lane per metre on every car (`sub_82C376E8`, V1 [`choose_connector`]) [code]; the load is the number of cars on the exit lane (the retail per-lane vec4 is not read yet, V1 open item).
- acceleration ramps 0.2 m/s^2 per ~0.25 s up to 2.3-3.4 m/s^2 from a stop [trace 47CAEDA0, proof1]; hard stops reach about -7.3 m/s^2 [trace 47CAEDA0, 164620].

Not retail yet: a car at a dead end stops there (the engine despawns it); the obstacle look-ahead and the query's exit-room check use the engine `min_gap`; two engine-side safeguards: the step never moves a car into its lead's bumper, and [`lead`] treats a car inside the junction on another connector into the same exit lane, nearer the exit, as the car ahead (two lanes of one approach merging into one; retail's junction query does not cover that case, its look-ahead does).

Multiplayer seams: [`step`] is a pure function of the road network, the signal clock, the cars (sorted by key) and `dt`; cars update in key order and see the cars before them already moved (deterministic). A client that runs the same spawn records from the same tick with the same signal tick count gets the same motion; see doc 26 V3.

Moddability: every number is a field of [`FollowParams`] (defaults from the vehicle spec records or the trace values named on each field; a mod overrides per car); the connector choice is the V1 [`ConnectorChoice`].

### Merge safeguard

Merging: a car inside the junction on another connector into the same exit lane that is closer to the exit point goes first. ENGINE SAFEGUARD, NOT RETAIL: retail's junction query never scans the car's own approach (`sub_82E11E90` passes only the ends from_end + 1, + 2 and + 3 (r28 / [r1+80] / r16) to the merge scan `sub_82E11C78` and the crossing scan `sub_82E11980`; its sibling check covers connectors of the same lane only) [code], so two lanes of one approach merging into one exit lane are spaced by the look-ahead / planner (`sub_82C412D8`, `sub_82C3FA08`), which is V4. Replace this with the ported look-ahead then.

### Parked pull-out

Parked (`82C39138`): standing, the parked time runs; `IsRequiredToPullOut` (`82C3A3A8`) after the parked time when the end of the approach is still on this road: the passage back to the outer lane (`82C3AFB8`: d1 = d + approach, from the kerb slot to lane n - 1; the spot is released). A sounding alarm holds the parked time at 0 and blocks the pull-out (`82C39138`, `82C3A3A8`).

### Junction stop target

FollowingLane stores the answer in the same field as the limiter kind (`+4392`: 1 signal, 2 approach, 3 yield, 4 blocked, 5 a yield to a flagged car); a car behind one waiting at a light (1) or a flagged yield (5) counts as waiting itself and does not get the blocked horn. Any answer but Go (`cmpwi r3,0`) is a stop target at the line: best = d, standoff = `+3660`; the planner (`82C3FA08`) brakes `-v^2 / (2 (d - standoff) + 0.001)` and stops the car dead inside the standoff. Approach slows the car this way until it is at the entry speed and the query answers Go. There is no go-through for a car that can no longer stop (no dilemma zone in `82C376E8` / `82C41120`; `+4402` bit 0x04 only feeds the horn timer `+3704`) [code].

### Car ahead

The car ahead (`82C38390` -> `82C41120`): gap = bumper to bumper (`0.5 x (s_me + s_lead)` off the centres), standoff = half of `+3664`; inside `speed + standoff` the lead is the stop target (kind 1 / 3), braking `-v^2 / (2 (gap - standoff))`, dead stop inside the standoff. No lead-speed matching [code]. Ours: the lead search reaches the window only (retail's lead finder `82E14F50` range is not read).

## Code reference: living_world/traffic/signals.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/signals.rs`; each comment there links its section here.

### Traffic signals

Traffic signals: retail's 4 signal controllers, each with a car light programme and a walk light programme, ticked by a fixed step.

Retail [code, TU3; addresses are evidence only]:
- `sub_826B1540` (living-world init) builds 4 controllers (loop to 4 at `0x826B1860`), each a car phase list at `+4` and a walk phase list at `+148` (16-byte phases: light, length, remaining, word), filled by `sub_82E156D8`. Even controllers: all-red, red (= green + amber), all-red, green, amber; odd controllers: all-red, green, amber, all-red, red. So controllers 0 / 2 and 1 / 3 alternate. The walk list mirrors the car list, except that a green phase splits into walk green `(1 - split) x green` and walk amber `split x green` (split = `+300`, the `trafficlights` record's `Hash_5E41C959D17527CC`, 0.4) and every other phase is walk red. Jump targets: `+312` / `+316` = the index of the all-red before green, `+304` / `+308` = the all-red before red.
- `sub_82E158D8` ticks one controller by a fixed `1/60` s (`0x820849C8`): the current phase's remaining time drops; below 0 the next phase starts and inherits the overshoot, except when the car list wraps to phase 0 (then the overshoot is dropped and the walk list is forced back to its phase 0 too).
- `sub_826B2C18` (from the living-world update `sub_826BDB50`, world tick `sub_82859E70`) ticks all 4 every world tick unless the lights are frozen (`+13382`). The world tick is a fixed 60 Hz step [trace: 60 ticks per second in the recomp while it renders at ~345 fps], so the cycle is 17 s of game time at any frame rate.
- Which controller an approach obeys: the connector's `from_end` (connector `+84`) indexes the controllers (road network vtable `+36`, `sub_82E11E90`). Every signalled junction in a district shares the 4 controllers: approaches from node ends 0 and 2 run together, 1 and 3 together, and the whole city switches in step.
- A priority vehicle (`sub_8269B328` sets it, `sub_8269B338` clears it and unfreezes; caller not found) gets a green wave: within 50 m (`0x8220E13C`) of a signalled junction, or inside one, the manager calls `sub_826B3C88(end)`: the controllers with the end's parity jump to their all-red-before-green, the others to their all-red-before-red, and once the end's controller shows green the lights freeze until the priority is cleared.

Durations are data (`livingworld` record `trafficlights`: `signal_green` 7, `signal_amber` 1, `signal_all_red` 0.5, split 0.4 [data]); [`SignalTimings`] takes them from the export or a mod. The phase arithmetic stays in f32 like retail, so the phase changes land on the same ticks.

## Code reference: living_world/traffic/spawn.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/spawn.rs`; each comment there links its section here.

### Car placement

Placing a census car on the road (milestone V2): what the retail vehicle factory does with the census spawn point before a car exists.

Retail [code] (TU3, addresses are evidence only; behaviour re-implemented, not copied):
- the census vehicle pass `sub_826B9B90` hands the factory a spawn record whose position is the random ring point (no heading: the basis rows are the identity, `0x82139A10..`) and flag byte `+84` = 0 ("place on the road"); the census vtable `0x8230C638` slot +8 (`sub_826B83C8`) refuses when the live vehicle count is not below the vehicle limit (census `+148`, 15 from the constructor `sub_826B6D58`; the initial populate uses a literal 15);
- the factory create `sub_82C36300` (vtable `0x8232260C` slot +20), road branch: 1. the road network (`0x830854C0`, vtable `0x8230C6A8` slot +60 = `sub_826B3B18`) finds the road surface under the point: a grid query of 10 m (`0x821963E4`) for lane pieces, then a point-in-triangle test on each piece's two road triangles (`sub_82E16410`); the first hit wins, no hit = no car; 2. the distance along the segment is the start of that piece (lane run start + the piece's cumulative length minus its length); 3. the point must be at least 15 m (`0x820BD16C`) from both ends of the segment; 4. every lane of the segment is tested with `sub_82E14928` (margin 15 m, extra 0 = `0x82165A10`): the next car ahead on that lane must start (its distance minus half its length, 0.5 = `0x8209975C`) at least `margin + extra` past the point, and the car behind must end (distance + its length + its speed x 1 s) at least `margin / 2` before it; 5. none fits = no car; else the factory takes a car from its pool, then one fitting lane by `trunc(u32 x 100 / 2^32) mod n` from the world RNG (`sub_82970628`, `0x822F94B0`), sets the mover onto (segment, lane, distance) (mover slot +44), and 6. queries the world for objects within 20 m (`0x820996EC`) and gives the car back when its extent overlaps one (the car's extents from vehicle `+36` slots +160 / +172). The car drives in its segment's direction (segments are directed, one per travel direction). Its length is the z of the model's size vector (vehicle `+36` slot +172 returns the z of `[+76]` slot +16) [code]; the engine takes it from the model record (`size_hint`, `vehicles.json`) [data, hypothesis that it is the same vector].

## Code reference: living_world/vehicle_material.rs

Detail moved out of the code comments of `crates/skate-game/src/living_world/vehicle_material.rs`; each comment there links its section here.

### Car materials

Retail car materials: the `vehicle_chassis` and `vehicle_glass` programs of TU3 `shaders_final.big` (`vehicle_defaultPS`, `vehicle_glass_defaultPS`) as one Bevy material.

Retail [code, shader microcode; constant tables read from the `.fpo` objects]:
- Constants: `g_vLightDir` c9, `m_params` c14..c19 (rows 0..5), `i_params` c20, `i_colorize_red` c21, `i_colorize_blue` c22; samplers `i_diffuse` 3, `i_normal` 4, `i_environment` 5 (glass: no diffuse, `i_normal` 3, `i_environment` 4).
- Chassis: `lin = diffuse^2`; a texel with `G^2 < 0.001225` takes `R^2 x i_colorize_red + B^2 x i_colorize_blue` (the ped shaders' rule, `skate_core::living_world::peds::colorize`). Light: `lin x (ambient + max(N.L, 0) x m_params[1].rgb x shadow) + (pow(sat(V.reflect(L)), m_params[2].w) x shadow x m_params[2].rgb + env^2) x (1 - sat(N.V))^m_params[1].w x diffuse.a^2`, ambient = the literal `(0.1, 0.175, 0.3)` x `m_params[5].w`, the shadow zero where `N.L < 0`; times `m_params[0].y` and the fog factor, plus the fog colour; the exposure / tone tail; output alpha `i_params.x`.
- Glass: the same with albedo `m_params[4].rgb` and no diffuse alpha mask; output alpha `i_params.x x m_params[4].w`. Render state [code, `sub_82CF0B90`]: alpha blend (SRC_ALPHA / INV_SRC_ALPHA), depth test on, depth write off.
- The rows come from the vault row named by each part's `AttribulatorMaterialName` (`character.livingworld_vehicles` / `character.livingworld_vehicles_glass` on every car) [data]; the converter writes them to `vehicles.json` `materials`.
- `i_params.x` = min(distance fade, fade-in ramp) (`sub_827C1188`) [code, partly read]; the engine draws 1 (no fade yet, open).

Engine: lit with the character lighting's light (`retail_character::Lighting`), Bevy's directional shadow and, like the character shader, without the vertex fog and the tone tail. The environment cube is bound when the export carries one (open: not converted yet).

Moddability: the rows are data (`vehicles.json`) and a mod replaces them per material name (`VehicleOverrides::material_params`); the palette colours and their gain come from `VehicleOverrides` as before; a mod GLB names its row with material extras `{"attrib_material": "..."}`, else gets the row its `kind` used in retail.

## Code reference: living_world/traffic/junction.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/junction.rs`; each comment there links its section here.

### Junction entry

Junction entry: may a car on its approach lane enter its chosen connector now?

A port of retail's car junction query `sub_82E11E90` (callers: FollowingLane `sub_82C376E8` and `sub_82C3B500`) with its two conflict scans `sub_82E11C78` (merging into the same exit lane) and `sub_82E11980` (crossing flows), plus the lane leader lookup `sub_82E14EE0` [code]. The occupancy lists are retail's: each lane of a segment (`+104`, 16-byte list heads per lane) and each connector (`+8` of its 248-byte record) holds its cars newest first.

Results ([`Entry`], retail return 0-4, stored by FollowingLane as the junction state `+4392` together with the stop distance):
- 0 [`Entry::Go`];
- 1 [`Entry::Signal`]: the approach's light is red or amber, or green without time to clear the line; a right turn (`to_end == from_end - 1`) ignores the light (turn on red);
- 2 [`Entry::Approach`]: faster than the connector's entry speed and still beyond the car's look-ahead; at an unsignalled junction (with the light check on) the entry speed is 0.1 m/s (`0x820641A8`), so cars come to a stop first (a stop sign);
- 3 [`Entry::Yield`]: another connector from the same lane is in use, or a conflicting flow holds the junction (merge into the same exit lane, crossing, oncoming for left turns); at an unsignalled junction cars still on their approach yield by distance to the line (closer first, equal distance: the lower id);
- 4 [`Entry::Blocked`]: no room on the exit lane, the car ahead on the connector is closer than the minimum gap, or the approach / exit segment is missing. Validation [trace, proof1 + proof2 VEHJUNC, 89 changes]: straight on red / amber always 1, right turns never 1, unsignalled right turns 2, the light skipped when the flag is off.

## Code reference: physics/skitch_state.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/skitch_state.rs`; each comment there links its section here.

### Skitching state 104

PhysicsSkitching 104 (TU3 vtable `0x82327398`; doc 26h "Skitching step 4e"). Enter `82D47278` + reset `82D47318`, Exit `82B61BB8` (empty), Update `82D477C0`, publication `82D4C078`. The maths lives in `skate_core::riding::skitching` (frame, sub-mode, hold, shimmy, target, spring); `physics/skitch_state.rs` composes it in retail order (`.local/research/npc/b50-skitch-prestep-update.md` section 1, b51 sections 1-3). Always on in the game (`ground_runtime::skitch::latch_enabled`, set by `LivingWorldPlugin`). The grab point and the hands' grip points are the record's spline at an along position (`82D2D550`, research b148), the chord only while no geometry is published. NOT RETAIL YET: the sub-mode's hard event comes from the shimmy state.

### Skitch board compose

`82D4AC38` (b51 section 2, arguments b56): capture, spring (tag 6), slide friction (tag 1, heading time = the sub-mode time 948), tilt (no push / damped-turn history; 0 while the board is off the ground), speed wobble (skipped off the ground), anti-flip, truck targets, heading (when balance and spin are both set), manual (powersliding off), the manual and anti-flip displacements, the yaw correction (`82C07328`) while towing fast. Sub-mode 4: tilt, wobble and truck targets only.

## Code reference: roads.rs

Detail moved out of the code comments of `crates/skate-data/src/roads.rs`; each comment there links its section here.

### Road graph

The living-world road graph, `roads.bin` v2 (setup group `livingworld`, written by `tools/asset_pipeline/living_world_roads.py` `write_graph`; format notes in `docs/hails-additions/living-world/vehicles-data.md`). Bytes in, graph out; no file I/O.

The graph is the retail road network (RW object `0x00EB0013`) of every district merged across its tiles: directed segments (retail 64-bit ids; `to_node` is the destination, `from_node` the origin), their lane geometry as ~4 m pieces (cubic Hermite centre line, road edges, arc-length tables), junctions (one per road node: inner / outer quads, 8 end records = the approaches and exits of node ends 0-3 with the connectors each lane may take) and the turn connectors (cubic Hermite curves from an approach lane to an exit lane). Unknown retail fields keep raw names (`flag_04`, `f32_50`, `word_56`). Every element keeps its retail id or index so a mod or a network peer can name it; [`RoadGraph::to_bytes`] writes the same format (synthetic tests, per-map roads of mods).

### District traffic input

The traffic input of one district only (its segments and junctions; a junction end whose segment belongs to another district becomes a map edge). The districts are separate worlds that overlap in x / z [data: University roads lie 16-18 m above the DownTown walk mesh, Industrial roads 47-59 m below it], so a world must only see its own roads; with the whole graph a car placed or routed onto another district's road floats in the air or drives under the ground. `None` when the graph has no district of that name.

## Code reference: living_world/traffic/mod.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/mod.rs`; each comment there links its section here.

### Traffic core

Living-world traffic, milestone V1: the road graph, the lane cursor, the traffic signals and the junction entry query (doc `docs/hails-additions/26h-living-world-traffic.md`, design `docs/hails-additions/living-world/vehicles-design.md`).

Pure and engine-independent like the population core: no I/O, no ECS, no rendering. The data layer turns the user's exported `roads.bin` into a [`RoadInput`] (`skate-data::roads::RoadGraph::traffic_input`); a mod can build one for its own map.

Retail reference (TU3, addresses are evidence only; behaviour re-implemented, not copied):
- road network object (global `0x830854C0`, vtable `0x8230C6A8`): `+8` vehicle by id, `+16` junction by node, `+24` lane run by id, `+32` segment by id, `+36` / `+40` signal controller by index (`sub_8269B2F0`: `this + 24 + 320 i`);
- FollowingLane `sub_82C376E8`: connector choice, junction query, lane leader;
- junction query `sub_82E11E90` (+ `sub_82E11C78`, `sub_82E11980`), leader `sub_82E14EE0`;
- signals `sub_826B1540` (build), `sub_82E156D8` (phases), `sub_82E158D8` (tick), `sub_826B2C18` (manager), `sub_826B3C88` (green wave).

Multiplayer seams (no networking): everything is a pure function of the roads, the signal clock's tick count and the cars' snapshots; ids are retail ids; iteration is sorted; the only randomness (the unused random connector branch) takes an explicit [`Rng`](super::rng::Rng). A client can rebuild the lights from the host's tick count ([`SignalClock::ticks`]).

Moddability: durations are data ([`SignalTimings`] from the `trafficlights` record), the connector choice is a parameter ([`ConnectorChoice`] plus the load function), the lights can be held for a car ([`SignalClock::request_green`], retail's priority vehicle), and the graph is plain input records a mod can write ([`RoadInput`]).

## Code reference: riding/skitching.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching.rs`; each comment there links its section here.

### Skitch spring

Skitching (physical state 104, held to a car's grab spline): the tow / ride-height spring
(`sub_82D4B500`, board force tag 6). Retail (TU3, evidence only; re-implemented;
`.local/research/npc/b27-skitch-state.md`, main checked the vault keys and the constants):
a velocity-target spring along the hold axis `n` (state+784), not a position pin:
```text
cd    = clamp(n . state+624, 0.5, 1.0)
pred  = state+848 + state+852 + dt * (2 * state+832 - p+2612)
z     = body height (negated when p+2476 bit 2 is set)
err   = pred - (p+2820 - z + offset)        offset: 0.1 (sub-mode 2: a second key, 0.1)
v     = curve(err)                          vault PointGraph, -20 .. +4 m/s
dv    = (v - damping * (-state+856)) / cd   damping 0.2
F     = n * dv * mass / dt, length-clamped to 500
```
The state fields (+624, +832, +848, +852, +856) come from the skitch frame step `sub_82D48148`
([`frame`]); this module keeps the spring formula with named inputs.

### Skitch sub-mode writes

`82D49AB8` before its hold step (`82D49D70`), after the shimmy chain (TU3 call order in the state update: `82D49580`, `82D48C98`, `82D49AB8`). Retail (evidence only; re-implemented; main read the switch on 1324):
- 0: `904 = clamp(864, -928, 928)`;
- 1: both hands grip again (`1328 = 1332 = 0`) every tick;
- 2: the edge point `e = +-932` (sign of 836, + only for 836 > 0) latches into 900 once (1344 bit 0x02), a snapped hand (bit 0x01) takes it as 864; 960 counts while the car's along acceleration 868 pushes outward past 50 (both 868 and 836 non-zero, same sign), else 0; the hand on the outer side lets go (836 > 0: hand A off, B on; else A on, B off);
- 3: the same latch with `e = +-(928 + 0.8)`, both hands off.

## Code reference: living_world/traffic/graph.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/graph.rs`; each comment there links its section here.

### Traffic road graph

The road graph the traffic runs on: directed segments with ~4 m lane pieces, junctions with approach / exit ends and turn connectors (retail RW object `0x00EB0013`, decoded in milestone V0, `docs/hails-additions/living-world/vehicles-data.md`).

Built from plain input records ([`RoadInput`]) that the data layer fills from the user's export (`skate-data::roads::RoadGraph::traffic_input`) or a mod fills from its own map; no I/O here. Every element keeps its retail identity ([`SegmentId`], [`JunctionId`], [`ConnectorId`], [`LaneId`]) and the network stores them sorted by that identity, so the dense indices are the same on every machine that loads the same roads (multiplayer and replay safe).

Lane geometry [data, all 138 shipped connectors]: a piece's centre curve is the middle of the road; lane `k` of `n` sits at `(k + 0.5) / n` of the way from the left edge to the right edge (lanes 4 m apart: -2 / +2 m on the two-lane roads, 0 on one-lane roads), and every connector starts exactly on its approach lane at the last piece's end and ends exactly on its exit lane at the first piece's start. Lane 0 is the left lane: right turns leave from the right lane.

## Code reference: living_world/traffic/cursor.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/cursor.rs`; each comment there links its section here.

### Lane cursor

The lane cursor: where a car is on the road graph (a lane of a segment or a turn connector) and how far along, advanced by distance across pieces, connectors and segments.

Retail [code]: the mover interface (vehicle `+144`, vtable `0x82322240`) keeps the segment id (`+72`, `ld +3992`), the distance along it (`+68`, `+3496`), the lane (`+84`), the chosen connector index (`+76`, stored at vehicle `+4388`), the distance along the connector (`+60`, `+3492`) and whether the car is on a lane (1) or inside the junction (0) (`+52`). FollowingLane (`sub_82C376E8`) picks the next connector when it has none or the segment changed (mover `+76 < 0` or `+72` != the lane's segment) and stores it at `+4388` before it looks at the junction; the choice:
- with vehicle flag `+4401` bit 0x02 (set by the constructor `sub_82C3B7C8`, 0xCE, on every car and never cleared): the connector of the lane's list whose exit lane has the smallest `load / exit segment length` (exit segment `+136`, one vec4 per lane, x used; strict `<` so the first wins a tie) [code];
- without it (unused in retail): list entry `trunc(u32 x 100 / 2^32) mod n` from the world RNG (`sub_82970628`, `0x822F94B0` = 100 x 2^-32) [code]. What the per-lane load holds is a runtime value not read yet (open); the engine passes the number of cars on the exit lane by default, a mod or the V4 driver may pass another measure.

## Code reference: graph_host/motion_skitching.rs

Detail moved out of the code comments of `crates/skate-game/src/graph_host/motion_skitching.rs`; each comment there links its section here.

### Skitch graph nodes

The skitch motion graph nodes (doc 26h "Skitching step 4h"). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b60-skitch-graph-nodes.md`, b58 part 2; main read the `anim_skitching/default` record):
- the behaviours bind `anim_skitching/default` (`8289D550`): +0 a PointNegGraphData8 (the absorb curve over the closing rate, ground+284), then six floats at +80..+100 in the schema's fixed layout (read with `tools/claude-skills/aems-port/tools/vault_layout.py 07D39B41AACEE50F`): +80 0.1, +84 0.25, +88 0.85, +92 0.05, +96 0.83 (`LongSkitchIntoReachTime`), +100 0.121;
- SkitchingBehaviour (`82BBC390`) each update: "Crouch" = ground+280 + rec+100, "PushSpeed" = ground+308, "absorbspeed" toward graph(ground+284) by at most 0.04;
- EnterSkitchingBehaviour (`82BBC538`): "Crouch" the same way (its "reachspeed" is not ported yet);
- IsSkitchingWithAbsorb (`82BBBD00`): state 104 and graph(ground+284) > 0;
- IsSkitchShimmying (`82BBC1A0`, attribute `direction` left 1 / right 2): anim+136 (negated by the flip) > 0.25 is 1, < -0.25 is 2.
- EnterSkitchingBehaviour's "reachspeed" (b61): `rem = 1 - t / len` of the current tree, `X = rem > 0 ? ground+276 / rem : len`, `n = clamp01((X - rec+80) / (rec+96 - rec+80))`, rate-limited by 0.1 once started (the instance starts at -1), "reachspeed" = 1 - n;
- SkitchShimmyingBehaviour (`82BBC858`): "ShimmySpeed" = `(clamp(|anim+136|, rec+84, rec+88) - rec+84) / (rec+88 - rec+84)`, 0 at the floor. SkitchingPosition's mirror term is (natural stance regular) == (riding switch) and the shimmy flip the mirrored animation bit (b61). NOT RETAIL YET: the shimmy channels SKCH_2H_SHIMMY_LEFT / RIGHT_CHANNEL are not started (their contents are not in the stock assets).

## Code reference: living_world/traffic/horn.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/horn.rs`; each comment there links its section here.

### Traffic horn

The traffic horn (V4, doc 26h "Traffic: the horn"). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b36-traffic-v4-driver.md` sections 2, 4, 5 and 8, `b41-v4-obstacles-corridor.md` section 3, `b44-horn-honker.md`; main re-read the honk receiver `sub_82E3C3D0`):
- every driving-state update sets the limiter kind `+4392` (0 free, 1 behind a lead that waits itself, 2 stop point, 3 behind any other lead, 4 obstacle from the look-ahead, 5 junction wait); the nearest limit wins. The junction answer is stored in the same field (1 signal, 2 approach, 3 yield, 4 blocked, 5 flagged yield), so a car queued behind one waiting at a red light is "behind a waiting lead" and never gets the blocked horn;
- blocked timer `+3704` (`sub_82C41120`): no lead or not inside the standoff behind it -> 0; inside it with kind 3 and slower than `honk_approach_speed_kmh` -> `+= dt`;
- obstacle timer `+3708` (`sub_82C412D8`): no obstacle -> 0; kind 4, not inside a lead's standoff and slow -> `+= dt`; otherwise it holds;
- the horn decider `sub_82C40660` writes the horn state `+3420` every frame, first match wins: horn disabled (driver bit 0x01) -> 0; kind 5 -> 3; blocked > `honk_blocked_time` -> 4 (driver bit 0x02) or 5; an obstacle with record flag 0 (skater / ped): obstacle timer > `honk_obstacle_time` -> 2 and the honked-at notify to the record's handle, else under 2 s to it (`2 v > d`) and faster than `honk_approach_speed_kmh` -> 1;
- driver bits (`sub_82C42348`): each a percent roll `rand() % 100 + 1 <= chance x 100` on a driver field. The horn sounds for as long as the decider returns a kind (no hold time in the AI); the sound per kind is the horn AEMS program's (`SFXObj_TrafficHorn`). The parked car alarm (state 6) is the car alarm's, not this.

## Code reference: riding/skitching/frame.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/frame.rs`; each comment there links its section here.

### Skitch frame step

The skitch frame step (`sub_82D48148`, state 104). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b32-skitch-frame.md` section 1, `b47-skitch-frame-transforms.md` section 1; main checked the constants in b32 and the query side of b47):
- re-orient: when the board points against the spline direction the endpoints swap and the direction flips;
- frame history 64 <- 128 <- 192 <- new (row0 spline direction, row1 up, row2 `normalize(d x n)`, row3 the endpoints' midpoint; all three = new on the first frame after a reset);
- the skater's along-axis coordinate `836` in the previous frame (128) and its rate `840`; the axis point `A = prev.row3 + 836 prev.row0` (b47 corrects b32: the previous frame directly);
- the grab point (`82D2D550`, current world pose) mapped by `320 = inv(current) * previous` into the previous pose, `672 = g' - A` (zero outside the half range `928`);
- the car velocity at the grab location `752` (finite difference of frames 128 and 64), the horizontal direction and distance to the axis point (`656`, `852`, rate `856`), the signed sideways offset `848` / `688`, the tow speed `832`, the side target (`624`, `860`, origin pushed 0.5 m along row2) and the "tows fast" gate (state+1345 bit 0x40, `832 >= 1F85F500908C5E17`). Frames are rigid, row-vector style: `p = row3 + x row0 + y row1 + z row2`. The relative transforms 256 / 448 (car motion over the last frame) are kept for the hand targets (`82D4A378`, not ported); 384 is identity in retail and left out.

## Code reference: living_world/traffic/skater_scan.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/skater_scan.rs`; each comment there links its section here.

### Traffic skater scan

A traffic car slowing for a skater coming up BEHIND it, so they can catch it (and skitch). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b63-traffic-skater-scan.md`, main checked the scan's constants, the quad B builder and the planner's vault hashes):
- the per-car tick (`sub_82C3D918`) runs the skater scan `sub_82C414A8` after the look-ahead: nothing while a skater holds the car (`+4402` bit 0x02); else every actor of the skater list (index order) that is not skitching (state 104), whose distance `|p - car| - 0.5 x size` is under the range (`Hash_33466832D8178EAF`, 40 m) and that lies in the rear zone (quad B, `+3520`) writes the distance (`+3760`) and its speed (`+3756`); one facing the car's way also sets `+4403` bit 0x10 (and bit 0x08 from a skater byte). Last match wins; 0x10 is never cleared inside the loop;
- the speed planner `sub_82C3FA08`, only while the limiter kind is free: with 0x10 set and both speeds above `Hash_D20826F15FB15A2E` (20 km/h) the acceleration cap becomes 0, and the car brakes toward the skater's speed minus `Hash_256A412E350A2659` (20 km/h): `(max(vs - m, 0)^2 - v^2) / (2 D + 0.001)`, D = `Hash_3AB7FC7CF7A17C81` (20 m) when the release grace is over and the skater is in range (FAR), D = `Hash_F682D359CDBC4D12` (5 m) when bit 0x08 is set and the skater is within `Hash_D49FC49019181EE5` (20 m) (NEAR, overwrites FAR, not gated by the grace);
- the release edge `sub_82C34B30`, once per traffic step after every car: a car let go this step gets the grace `Hash_4727CF785EF735C8` (2.5 s), otherwise the grace drops by the step and sits at -1 once negative.

## Code reference: living_world/traffic/manoeuvre.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/manoeuvre.rs`; each comment there links its section here.

### Traffic manoeuvres

The traffic manoeuvre decider (retail TU3, evidence only; re-implemented; `.local/research/npc/b69-traffic-manoeuvres.md` with the b71 to b74 extensions; main checked the overtake hash before the first roll, the driver block at `[car+4116]` (schema offsets) and the bit roller):
- `sub_82C3D830` first: a pending pull-over is cancelled while a skater holds the car (unless the car has the "pulls over while held" trait), outside junction state 2: the spot is released, the timer reset;
- `sub_82C41CD0`, only while no manoeuvre is pending: (1) the overtake roll (`Hash_9366C67755A24D89`, 0 in every stock record, one draw every call); (2) the lane timer (`+3632`, + dt per tick) past the spec's period (`[spec+48]` = `Hash_90AB56A5DDCF2A3A`, 10 s, sports 5, truck 8) resets and rolls "go" (driver +28 `Hash_52CF2CF3...`: 0.1, taxi 0.4, others 0) and "least loaded" (driver +24 `Hash_7C6B48BD...`); on a road that allows lane changes the adjacent lane (random, one draw, or the least loaded) must pass the gap check (`82E14928`); (3) when the timer fired and no lane change started: the pull-over roll (`Hash_559BA807...`, 0.02, taxi 0.04; 0 while held without the trait) on a road that allows it, on the outer lane, with a spot at least the approach length ahead (`82E14CF8`), which is reserved.

The roll is retail's percent test (`8269A588`: `r % 100 + 1 <= chance x 100`, [`super::horn::percent_roll`]). Multiplayer: [`DeciderState`] is plain per-car data; the host rolls with its seeded RNG.

## Code reference: physics/ground_runtime/skitch.rs

Detail moved out of the code comments of `crates/skate-game/src/physics/ground_runtime/skitch.rs`; each comment there links its section here.

### Skitch query

The riding skater's skitch query (`sub_82D39BB8` / `sub_82D39D98`, doc 26h "Skitching step 3"; `.local/research/npc/b33-skitch-wiring.md` section 2, main read the call site, the box flag and the limits):
- the ground update runs it instead of `82D749D0` (the grab owner's invalidate) while Processed `+2476` bit 22 is set (`82D37C88` branches between the two);
- the box: `82D2E250(frame +192, offset, extents, out, true)` ([`skitch_bounds`]);
- when the owner's validated results are ready (`+12836` bit 0x40) the latch walks them: endpoints oriented by the box's right axis, CanGrabSpline (`82E08DB8`), the re-grab cooldown, the time to the spline `max(0, (dot(P - pos, fwd) - reach) / max(dot(vSkater - vCar, fwd), 0.5))`, a bind (`82585DD8` + vcall +44) per candidate, and a latch on the first one under `latch_time` (state +2729 / +2560 / +2564);
- always: submit the mode-255 query (`82D74200`: margin 0.25, angles 60 / 30 degrees, cap 5), which reaches the cars' provider (`+4088`). NOT RETAIL YET: a type-2 candidate (world object) is not latched (its box `82D2CF68` is not decoded); `P` is the closest point of the spline's endpoint chord (retail's point choice is not decoded); `+2668` (`skitch_spline_height`) stays 0 and `82D91298(state+28, 2)` is not called.

## Code reference: riding/skitching/shimmy.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/shimmy.rs`; each comment there links its section here.

### Skitch shimmy

The along-the-bumper offset chain (`sub_82D48C98`): where on the grab edge the skater holds and how the hand shimmies. Retail (TU3, evidence only; re-implemented; `.local/research/npc/b30-skitch-pull-release.md` section 4, `b31-skitch-submode-hold.md` section 2):
- the car's along acceleration is low-passed (`868 = 0.9 * 868 + 0.1 a`), a hard event (`1000`) is its sign past 40, and the excess over a dead-band curve (`880`) pushes the hand;
- the stick pushes the hand at 10 (`876`); with neither a velocity servo damps it to rest (`884`);
- the velocity `892` integrates the total (`888`), capped at 1 (or the stick gain while an event opposes the stick) and unbounded in the event's direction; the hand target `864` moves by it, clamped to the edge range (+-932), and snaps onto the latched edge point 900 when it crosses it. The acceleration `a` is the caller's (its derivation from the frame history is open after b47's frame layout).

## Code reference: riding/skitching/hold.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/hold.rs`; each comment there links its section here.

### Skitch hold step

The skitch hold step and release (`sub_82D49D70`, the release bits of `82D49580`, the tail `82D4AFB8`, reset `82D47318`). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b53-skitch-hold-step.md`, b51 section 2, b52 section 3; main checked the 1344 masks in `82D49580`):
- the hand's along target 904 ratchets with the stick while 1344 bit 0x08 is set (`max` / `min` with the clamped position for a stick past +-0.5, else follows it); the bit clears once the skater rests near the posed hand (|rate 840| < 0.02, |836 - 908| < 0.1) and sets again when the hand is 0.2 m off or after 2 s;
- the posed hand 908 chases 904: `912 = clamp(0.25 (904 - 908), 912 +- 0.03)`, `908 += 912` (per 1/60 tick);
- release bits (1344): sub-mode 0 clears 0x60, 1 sets 0x40 (pull in), 2 / 3 set 0x20 (push off); the 2 -> 4 switch clears both, 3 -> 4 keeps 0x20; 0x08 is set on every switch but 3 -> 4;
- the tail: without the grab input (2476 bit 22) the sub-mode becomes 4; in sub-modes 1 / 2 with both hands off (1328 = 1332 = 1) and the hold timer 1020 run out it becomes 4 and sets 0x10; in 4, 1320 counts frames, the first one queues the release impulse (0x40 pull in, else 0x20 push off), past 3 frames 0x10 is set (the state ends), and the re-grab block 1016 is refilled to 0.5 s when empty. The step sizes 0.25 / 0.03 are per 1/60 tick (console cadence).

## Code reference: riding/skitching/target.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/target.rs`; each comment there links its section here.

### Skitch target and impulses

The skitch hold target (`sub_82D4B8C0` / `82D4BAC0` / `82D4BC58` / `82D4BCF8`) and the release impulses (`82D4B1D0` / `82D4B378`). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b52-skitch-target-impulses.md` sections 1-2; main checked the impulse vault keys):
- the hold target point `736 = 576 + 864 * 528.row0` (the side-shifted previous frame at the hand's along coordinate 864); `784` = the horizontal unit direction skater -> 736; `952` = the yaw from the direction to the current grab point (836) to 784;
- `at` = yaw from the skater's facing to 784, `as` = yaw from the facing to the side direction 624; `940 = tows fast ? wrap(at - as) : -as`;
- the yaw correction `V = up' * sin(clamp(at, +-180 deg/s * dt)) * max(g1(992), g3 * g2(832))` (`up'` = up flipped to y >= 0), applied by `82C07328` only while towing fast and not releasing;
- release impulses (tag 6, one frame): pull in `624 * min(max(-832, 856 - 2.0), 0) * m / dt` or push off `624 * (856 + 2.5) * m / dt`. The signed angle `8286CD88` is read as the angle a -> b about the axis (internals not decoded [inferred]).

## Code reference: living_world/traffic/passage.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/passage.rs`; each comment there links its section here.

### Lane change passage

A traffic car's lane-change passage (retail TU3, evidence only; re-implemented; `.local/research/npc/b69-traffic-manoeuvres.md` "b71 / b72 extensions"; main checked the progress step 82C3C3C0 and the road-network values 1.3 / 10 in the exported tables):
- enter `sub_82C3A9E0`: from distance d0 on the source lane to d1 = d0 + ext x the spec's passage factor (`Hash_328B9F4685A14018`, `+3692`) + speed (one second of travel) on the target lane;
- `sub_82C3F540`: a cubic Hermite curve between the two lane points (`82E14860`), tangents = the lane direction x (d1 - d0) x the road network's tangent scale (`Hash_D2C11CC10C9E0E49`, 1.3);
- `sub_82E10C88`: an arc-length table of `Hash_E35079BDD5BAE286` (10) chords; its total is the passage length;
- every tick (`sub_82C3C3C0`): progress += speed x (+1, -1 reversing) x 1/60; the distance along the road is lerp(d0, d1, progress / length); the car sits on the curve at the arc-length parameter, facing its tangent (`sub_82C3F0C8`); done when progress >= length (`sub_82C3A5A8`), the car's lane becomes the target.

Multiplayer: the host owns (lanes, d0, d1, progress); the curve and table rebuild from them and the road.

## Code reference: living_world/traffic/obstacles.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/obstacles.rs`; each comment there links its section here.

### Traffic obstacles ahead

What a traffic car sees ahead (V4, doc 26h "Traffic: obstacles ahead"). Retail (TU3, evidence only; re-implemented; `.local/research/npc/b36-traffic-v4-driver.md`, `b41-v4-obstacles-corridor.md`, main checked the characteristics / driver values and the 0.68 s constant):
- the traffic manager refills two obstacle lists every frame (`sub_826B2EE0`): skaters (radius 0, "soft") and world actors (peds radius 0.5 x their size, soft; cars radius 0.5 x their smallest extent, hard);
- each car builds a look-ahead quad (`sub_82C400A8`): from the car centre (half width x (1 + `E88C`, 0.0)) to the front bumper, widened on the turning side by half width x (1 + `02A6` (2.0) x speed ratio), then both front corners pushed `speed + standoff` further along their side edges;
- a record is in the way when its circle touches the quad's side / front edges or its centre is inside (`sub_82C41BD0`, `sub_82C41A90`); its free distance is `|pos - car| - radius - 0.68 x speed` (`0x82099764`); below 20 km/h soft records count only for drivers that opt in (`+4401` bit 0x08, every stock driver does);
- the nearest one becomes the planner's obstacle (`sub_82C412D8`): braking `-v^2 / (2 (d - standoff) + 0.001)` when `standoff < d <= v + standoff`, `-v` inside the standoff (`sub_82C3FA08`).

## On foot on a car roof (2026-10-27)

- **Problem:** the user: "for some reason I autobail when jumping ontop of a car with no skateboard, which is incorrect." Log game-20261027-073605 13:59:48: BipedAir -> WipeoutGround, `WIPEOUT_REASONS [(32, 0.0)] vehicle_contact=true vehicle_force=0.9`.
- **Retail [code, research b648 `.local/research/b648-onfoot-car-bail.md`]:** wipeout 32 is retail's rule: `82BEBEA8` sets the extra errors (parts 24 / 25 minus their targets), `82BD9FB0` copies them in partial ragdoll, `82D2F748` requests a wipeout when either error's vertical part or residual passes 0.2 (`0x82099280`), with no vehicle filter. The on-foot launch / requery arc (`82D6CA58`, `82D6E798`) goes through `82E099A0` over the pools of `82764CF8` mask 7: the static map and the two dynamic island pools (world +0x5580 / +0x5584), where physics islands are added (`82768878`, `82768918`). That cars are among those islands is inferred (medium).
- **Cause in ours [code]:** `GamePhysics::biped_world` was the map plus the prop world only, so the on-foot arc planned the landing on the street under the roof; parts 24 / 25 then met the car (groups 20 and 8 collide) and the error passed 0.2.
- **Change:** `living_world::vehicles::publish_on_foot_solids` (after `push_vehicle_proxies`) puts the car proxies (contact group 8) in `GamePhysics::on_foot_vehicles`; `BipedWorld::vehicles` and `StaticScene` answer them with the Ground pool (lines, the trajectory, nearby triangles), after the map's own solids. Only the on-foot queries see them (the camera, board air and foot IK keep their geometry). Mod key `living_world.on_foot_vehicle_ground` (default on).
- **Verification:** `biped_lines_land_on_a_car_roof_when_cars_are_on_foot_ground`. Not play-tested. Open: the reason-32 bails at 14:00:03 / 14:00:09 without a car (other geometry); whether retail carries a moving car's velocity into the on-foot landing.

## Code reference: living_world/traffic/spots.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/spots.rs`; each comment there links its section here.

### Stop spots

Road-side stop spots for pulling over and the lane-change gap check (retail TU3, evidence only; re-implemented; `.local/research/npc/b69-traffic-manoeuvres.md` "b71 extension", main checked the passage step 82C3C3C0):
- `sub_82E151D0` / `sub_82E15368` keep one (car, stop distance) pair per pulling-over car on a road, sorted by stop distance, far end first; only other cars' spot searches read them;
- `sub_82E14CF8` searches a spot up to 0.75 x the road length: the middle of the road when nothing is reserved, else the middle of the first gap (walking from the far end) longer than the car plus 5 m and beyond d + 1.5 x the car's length + speed; the gap behind the last reservation is never tried;
- `sub_82E14928` checks a lane-change gap: not near either road end, and one second of travel plus the lengths clear of the cars ahead and behind on the target lane.

Multiplayer: [`Reservations`] is plain per-road data the host owns.

## Code reference: riding/skitching/hands.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/hands.rs`; each comment there links its section here.

### Skitch hands

The skitch hands (`sub_82D4A378`): where each hand grips the grab edge, when a hand lets go, and the hand IK weights. Retail (TU3, evidence only; re-implemented; `.local/research/npc/b59-skitch-hand-targets.md`; main checked the IK write path against ours):
- sub-mode 0 clears the hand-off flags (1328 / 1332); a lean yaw (940) outside -60..35 / -35..60 degrees sets them;
- 988 follows the world grab height: `clamp(988 + 0.1 ((c M).y - S(908).y), 0, 0.8)`, `c = (0, grab y, grab z)`;
- each hand's spline parameter is the posed hand (908) plus its part's skeleton-local x minus the anchor's x, clamped to the range (x / z mirrored by Processed 2476 bit 2); a shoulder part farther than 0.9 + 0.7 m (through the skeleton and the car's last-frame motion) from its spline point lets that hand go;
- on a flag change the hand's weight target becomes 0 (off) or 1 (on); the weight moves toward it by +0.03 / -0.2;
- a hand with weight > 0 gets an IK target: the spline point plus the car's velocity over one tick, clamped to 0.9 m from the shoulder; the bitmask 984 says which hands are on.

## Code reference: riding/skitching/lean.rs

Detail moved out of the code comments of `crates/skate-core/src/riding/skitching/lean.rs`; each comment there links its section here.

### Skitch lean

The skitch lean (`sub_82D4A0C0`): the skater leans toward the hold target. Retail (TU3, evidence only; re-implemented; `.local/research/npc/b58-skitch-lean-outputs.md` part 1, b51 section 2):
- `T = sign(940) * graph(|940| in degrees) in radians`, 0 while Processed 2488 bit 0x00800000 is set;
- `944 += clamp((T - 944) * 0.25, -2 deg, +2 deg)` per update;
- above 2 degrees the board offset's orientation becomes a rotation of 944 about Y for 15 updates (Skeleton+15696, +16389 = 1, +16392 = 15; the height channel is not touched).

## Code reference: living_world/traffic/tests.rs

Detail moved out of the code comments of `crates/skate-core/src/living_world/traffic/tests.rs`; each comment there links its section here.

### Traced signal phase gaps

proof1.tsv TRAFLIGHT2, controller 0: the first line (one tick in, 0.483 left) then 29 ticks to phase 1, 480 to phase 2, 30 to the green, 252 to the walk amber, 168 to the car amber, 60 to phase 0, 30 to phase 1, 480 ... [trace]. Controller 1 (f32 carries make its walk light lead the car light by one tick): 30 to the green, 253 to the walk amber, 167 to the car amber, 1 to the walk red, 59 to the all-red, 1, 29 to the red, 1, 479 [trace].
