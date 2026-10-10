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
  mod-graphics path); `vehicle_chassis` gets a tinted copy of its base texture, `vehicle_glass` is drawn at 0.55
  opacity (engine value); wheel bones spin by distance / `wheel_hint` [data]. Drivers are in the body mesh, cars have
  no lights [data, V0].
- **Tint rule (shader not decoded, open)**: the atlases paint the body pure blue `(0, 0, b)` and the base palettes are
  chassis `(0, 0, 1)` / secondary `(1, 0, 0)` [data], so the chassis colour replaces the blue channel and the
  secondary the red channel, weighted by channel purity; identity for the base palette. The painted value is scaled by
  a gain of 2.0: an estimate from the data (paint blue sits at 0.5-0.56; only 2x makes palette white and taxi yellow
  read as such), not retail, overridable (`VehicleOverrides::paint_gain`).
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
- [code, shader microcode] `data/big/shaders_final.big`, read with `.claude/skills/living-world/tools/eb_big_extract.py`
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
turn side `+3748` is open), the standoff is the car's `min_gap`, and the lists use last frame's poses.

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
