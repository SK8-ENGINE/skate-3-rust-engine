# Living world: vehicle data and the road network v2 (milestone V0, 2026-10-05)

Branch `world/living-world`. Design: [`vehicles-design.md`](vehicles-design.md) section 3.1 / V0.
Tags: [code] read from the TU3 code (addresses are evidence only), [data] read from the disc (checked on every
shipped record / object), [trace] recomp only. Nothing here is copied into the repo: the setup reads it all from the
user's disc.

## What was built
| Item | Status | Where |
|---|---|---|
| Vehicle tables (`livingworld_vehicle_characteristics`, `livingworld_vehicle_drivers`) in `tables.json` | done | `living_world.py` `TABLE_CLASSES`, `FIELD_NAMES` |
| 18 car GLBs (`living_world/vehicles/<recipe>.glb`) | done: 18 / 18 | `tools/asset_pipeline/living_world_vehicles.py` |
| `vehicles.json` (models, palettes with stable ids, entities, census, recipes) | done | `living_world_vehicles.vehicle_doc` |
| Road network v2: junctions, approach / exit records, turn connectors, lane pieces | done (all 82 objects) | `tools/asset_pipeline/living_world_roads.py` |
| `roads.bin` v2 graph + `roads.json` v2 + `roads_raw.bin` (old verbatim pack) | done | `living_world.export` |
| `skate-data::roads` (bytes in, graph out, `to_bytes` for synthetic / mod roads) | done | `crates/skate-data/src/roads.rs` |
| Validation: every painted vehicle census record resolves to entities with a built car, palette, spec and driver | done (Python, at export) | `living_world_vehicles.validate` |
| Tests | 13 Python, 4 Rust unit, 3 Rust data-gated | `test_living_world_vehicles.py`, `roads.rs`, `tests/roads_data.rs` |

The vehicle half runs inside `living_world.export` (same group, same vault conversion). Fingerprint inputs gained
`living_world_roads.py` and `living_world_vehicles.py` (the other groups' fingerprints are unchanged). A vehicle
census record that does not resolve makes the whole group unavailable like any other content error.

## Formats
### tables.json additions
Two classes with inheritance applied (same rules as milestone 1). Readable names where a code site proves the
meaning, `Hash_*` otherwise (`field_names` maps both ways):
- `livingworld_entities`: `driver` (`Hash_023EF929823A3C50`, RefSpec drivers), `spec` (`Hash_92A043B4A11F1A2A`, RefSpec
  characteristics), `scoring` (`Hash_E356ED00ABF1D7F0`, RefSpec `scoring_entities`, vehicles: `car`) [data].
- `livingworld_vehicle_characteristics`: `engine_audio` (RefSpec `aud_traffic_engine`) [data]; `alarm_impulse` 0.1
  (`sub_82C3C150`), `alarm_duration` 8 (`sub_82C3A4D0`), `follow_min_speed_kmh` 20, `skater_follow_margin_kmh` 20,
  `skater_far_distance` 20, `skater_scan_range` 40, `skater_near_range` 20, `skater_near_distance` 5, `release_grace`
  2.5 (`sub_82C3FA08` / `sub_82C414A8` / `sub_82C34B30`, b63; exports before 2026-10-09 call `skater_far_distance`
  `follow_speed_margin_kmh`) [code]. Records: default, family01, minivan01, sports01, taxi01, truck01.
- `livingworld_vehicle_drivers`: `honk_obstacle_time` 2, `honk_blocked_time` 4 (taxi 1) (`sub_82C40660`),
  `honk_approach_speed_kmh` 5 (taxi 10) (`sub_82C34190`), `pull_over_chance` 0.02 (taxi 0.04) (`sub_82C41CD0`),
  `parked_time` 30 (taxi 20) (`sub_82C3A3A8`) [code]. Records: default, driver_normal, driver_fast, driver_reckless,
  driver_taxi.
- `livingworld_models`: `chassis_colours` (`Hash_12026E2EED18CC8D`) and `secondary_colours` (`Hash_DF76D7D773857EDB`),
  renamed from milestone 1's `tints_b` / `tints_a`.
- `livingworld` record `trafficlights`: `signal_green` 7.0, `signal_amber` 1.0, `signal_all_red` 0.5 [code
  `sub_826B1540` + data]; `Hash_5E41C959D17527CC` 0.4 is stored at controller `+300`, use open.

### vehicles/<recipe>.glb
One glTF per vehicle recipe (`recipe/vehicle/*.recipe`). Rig: the arena's 8 bones (`Vehicle_Root`, `Chassis`,
`LeftFront_wheel`, `RightFront_wheel`, `LeftRear_wheel_1/2`, `RightRear_wheel_1/2`; no stored parents, so all hang off
the rig root at their bind pose; `sedan_4door_02` windows add `lod_high` / one more), body vertices rigidly skinned to
the wheel and chassis bones. Primitives per part and material instance with `extras.role` (`body` = Accessory,
`windows` = Equipment, `wheels` = Misc on `reda_car`) and `extras.kind` (`vehicle_chassis` / `vehicle_glass`, from the
recipe XML). Materials: diffuse RGBA as stored, converted normal map, environment cube id in `extras` (the cube is
not converted). LODs: one per car (z_pipeline_lw_vih, a pipeline test car, has two). About 3,200-4,400 vertices and
3,200-3,900 triangles per ambient car; `reda_car` (marquee prop) 15,440 / 14,172.
- Positions are 4 x half float (Xenos `0x001A2360`), which the shared rx2 reader does not decode; the vehicle module
  reads them itself from the same vertex buffer (no change to the vendored reader, so no other group refreshes).
- Recipe format fix: the LOD word after the arena id is the **material instance count** (1 everywhere; 2 on
  `reda_car`'s wheels, both the same material). `parse_recipe` now reads every instance; single-instance LODs keep
  the old dict shape, so `models.json` and every ped GLB are byte-identical.

### vehicles.json
`{"version": 1, "models", "entities", "census", "recipes", "specs", "drivers"}`:
- `models.<livingworld_models record>` (vehicle category 3 with a recipe): `recipe`, `glb`, `chassis_colours` /
  `secondary_colours` (RGBA lists), `palette_ids` (`<model>/chassis/<i>`, `<model>/secondary/<i>`: stable ids a mod or
  a network peer names a colour by), `size_hint` (`Hash_F983F2518B335286`), `wheel_hint` (`Hash_FD7A66142F16B9CC`),
  `mesh_bounds` and `wheel_bones` measured from the arena.
- `entities.<entity>`: `model`, `recipe`, `spec`, `driver`, `scoring`, `ai_graph` (group records like `sedan` keep
  the inherited values).
- `census.<record>` for every painted vehicle census record (dwntwn, indust, univ): max population, `vehicle_extra`,
  categories with weights and entity names.
- `recipes.<recipe>`: parts (slot, role, LOD count), material types, the GLB summary.

### roads.bin v2 (little-endian, `living_world_roads.write_graph`, `skate-data::roads`)
Header (36 B): `LWROADS2`, u32 version 2, u32 counts: districts, segments, pieces, junctions, connectors, lane
entries. Then fixed-size records in that order:
- district (32): name[16], first segment, segment count, first junction, junction count;
- segment (72): u64 id, u64 `to_node`, u64 `from_node`, u32 `to_end`, u32 `from_end`, f32 length, width a, width b,
  speed limit (m/s), u32 lanes, `word_56`, first piece, piece count, flags (bit 0: pieces complete), district;
- piece (224): u32 segment, u32 index, f32 distance at the piece end (along the segment), f32 length, centre-line
  Hermite curve (start, end, start tangent, end tangent), road edges (left / right at start and end), 4 edge tangents,
  16-sample arc-length table;
- junction (132 + 8 x 40): u64 node, u32 district, f32 speed, u32 `flag_04`, u32 inner / outer quad tag, inner and
  outer quads (4 corners each), first connector, connector count; then 8 end records (u64 id, u32 kind, side, lane
  count, connectors per lane [4], first lane entry);
- connector (144): u32 junction, u32 retail index, f32 length, f32 `f32_50`, u32 from end, from lane, to end, to lane,
  Hermite curve, arc table;
- lane entries: u32 retail connector indices (the per-lane lists of the end records, in record order).
Segments and junctions are sorted by id per district, so two exports are byte-identical. `roads.json` v2 holds the
same graph without the pieces and arc tables (plus the tile objects); `roads_raw.bin` is milestone 1's `roads.bin`
(every object verbatim, `LWROADS\0` pack), unchanged.

## Road object layout (RW `0x00EB0013`) [data, all 82 objects]
Big-endian. Header: bbox min / max, u32 junction count, **u32 lane-run count, u32 segment count** (milestone 1 read
the run count as the segment count; the one University tile where a segment has two lane runs got a phantom
segment: there are **76** segments, not 77), junction table, lane-run table and segment table offsets.
- **Junction** (0x260, back to back; their connectors, arc tables and lane lists follow after all of them): inner
  quad (the junction box) and outer quad (4 m larger on every side; every connector starts and ends on it), each
  corner with a shared u32 tag; 8 end records of 0x38 (records 0-3 = approaches of node ends 0-3, 4-7 = exits of
  ends 0-3); header at +0x240: f32 speed (the road speed), u32 `flag_04`, u64 node id, connector offset (relative to
  the junction), connector count.
- **End record** (0x38): u64 id, u32 kind (1 road, 2 none, 0 = the node itself when it closes a lane run), u32 side
  (1 approach, 0 exit, 0xFFFFFFFF none; the node end index in a lane-run end), u32 lane count, u32 count[4], u32
  offset[4] (relative to the record) of each lane's connector index list; unused slots hold stale memory.
- **Connector** (0x70): Hermite curve (start, end, two tangents), f32 length, u32 16, arc table offset, f32 `f32_50`
  (equal to the road speed on straight connectors, 1.4-2.4 on turns; meaning open), u32 from end, from lane, to end,
  to lane. All 138 connectors appear in their approach lane list and in their exit lane list.
- **Lane run** (0xA0; one per segment per tile, two where a segment leaves and re-enters a tile): u64 id, start and
  end records, u32 piece count, piece offset, f32 speed limit, u32 lane count (= the segment's `word_52` on all 241
  runs), f32 width, f32 run length, f32 distance at the run end, u32 index of the first piece, u64 segment id.
- **Piece** (0xE0, about 4 m): centre-line Hermite curve, edges and their tangents, cumulative length within the run.
- **Direction**: pieces run from the segment's `node_b` to its `node_a`: the run's end record names the node and end
  of `node_a` / `end_a` on all 76 junction-side runs, and the last piece ends on that junction's outer box. So
  `node_a` is the destination (`to_node`); segments come in pairs, one per travel direction.

Counts: DownTown 46 segments, 19 junctions, 88 connectors; Industrial 26 / 10 / 46; University 4 / 4 / 4; 3,831
pieces; every segment has complete lane geometry. Speeds 51 / 50 / 30 km/h; 1 or 2 lanes per direction.

## Traffic lights [code]
`sub_826B1540` (living-world init) builds **4 signal controllers** (loop `r27` 0..3, `cmplwi r27,4` at `0x826B1860` back to `0x826B16FC`; the recomp's 20
phase timers at load = 4 x 5). Each controller holds two light groups (vectors at `+4` and `+148`, 16-byte entries)
and a phase list (`sub_82E156D8`, state, seconds): all-red 0.5 (state 0), green 7 (state 2), amber 1 (state 1),
all-red 0.5, red 8 = 7 + 1 while the other group runs; odd controllers start the cycle at the other group (the
two orders the recomp trace showed). Cycle 17 s. The group sizes are latched at `+304..+316` at the phase changes.
Which junction approach belongs to which controller and group is not in the road data (the junction `flag_04` is
the only per-junction candidate: set on 14 of 19 DownTown and all 10 Industrial junctions, clear on the 4
University dead ends); the binding is V1 work (`sub_82E14EE0`, `sub_82E11E90`).

## Drivers (the visible people in cars)
The driver is **part of the body mesh**: a low-poly dark figure (head, shoulders, an arm to the wheel) modelled into
each car's `Accessory` part, in the left front seat (+X, left-hand drive), using a dark area of the body atlas. There
is no driver model, recipe part, ped reference or texture of its own (all 73 vehicle textures are referenced by the
recipes), and the window material is a flat 32 x 32 grey (nothing painted on the glass). Rendered with the windows
left out: `.local/research/npc/m1-out/vehicles/taxi_side_nowindows.png`, `sports_car_01_side_nowindows.png`,
`taxi_front_nowindows.png`. So drivers come for free with the car mesh; no driver system is needed (user: "I can
definitely see drivers in the cars").

## Values and findings
- [data] Palettes: the base records (`vehicles`, `vehicle_sedan` ...) hold chassis (0, 0, 1) and secondary (1, 0, 0);
  the body atlas paints the car body pure blue, so the chassis colour most likely replaces the blue mask channel in
  the `vehicle_chassis` shader (hypothesis until the shader is read in V3). Chassis lists hold 2-10 colours (taxi:
  yellow and red), secondary lists 1 (10 for `mongo_patrol01`).
- [data] `wheel_hint` (`FD7A66142F16B9CC`) equals the wheel bone height (wheel radius) to the millimetre on most cars
  (hatchback 0.317, sedan02 0.299, pickup02 0.43); `size_hint` (`F983F2518B335286`) is a per-class value (1.9 x 1.7 x
  4.0 for every sedan and sports car), not the mesh size: meaning open.
- [data] The `vehicle_suv` group record points at a recipe `trafsuva_kit00_lod` that is not on the disc; no entity
  uses that record (suv01 / suv02 have their own recipes).
- [data] No vehicle lights: the recipes use only the chassis and glass materials (confirmed, see the todo).

## Mod entry points foreseen (V8)
- Content overlay patches `tables.json` (vehicle specs, drivers, palettes, census) by `class / record / field`;
  `vehicles.json` keys (model, entity, census record, palette id) are the stable names.
- Cars: a mod GLB with the same bone names (`Vehicle_Root`, `Chassis`, wheels) and `extras.role` / `kind` on its
  primitives replaces `vehicles/<recipe>.glb` or adds a recipe referenced by a new `livingworld_models` record.
- Roads: `RoadGraph::to_bytes` writes the format, so a custom map can ship its own graph (or per-map JSON converted
  at load); retail ids stay the keys, new elements take new ids.
- Cleanup: overlays are separate files; disabling a mod restores the exported defaults.

## Tests
- `py -3.13 -m unittest tools.asset_pipeline.test_living_world_vehicles` (13, synthetic only: road object decode,
  tile merge and direction, incomplete pieces, graph round trip and determinism, the header's segment count,
  bad offsets, recipe material instances, XML material types, half-float positions, vehicles.json palettes / ids /
  census, validation, vehicle table names, fingerprint membership).
- All pipeline tests: every `tools/asset_pipeline/test_*.py` + `tools.test_setup_assets`: 190 OK (2 skipped).
- `cargo test -p skate-data --release --locked --lib roads` (4: round trip, lookups through a junction, curve
  evaluation, rejects bad magic / size / references / duplicate ids).
- Data-gated, `SKATE3_ASSET_ROOT=<export>/assets cargo test -p skate-data --release --locked --test roads_data` (3:
  shipped counts, continuous pieces ending at their junction, connectors in their lane lists and leading onto a
  segment, every lane of a segment into a junction has a connector).

## Open
1. Signal binding: which junction approach uses which of the 4 controllers and which group (V1, code).
2. `flag_04`, `f32_50`, `word_56`, `size_hint`, quad tags, the trafficlights 0.4 value: meanings open.
3. The chassis / secondary tint rule in the `vehicle_chassis` shader (V3: read the shader, check against the user's
   eyes).
4. `--check-assets` in the game still runs only in Python at export time (as milestone 1).

## Credits
TU3 static recompilation (skate3recomp by @mchughalex, rexglue SDK, Xenia) as the code reference; the vendored rx2
reader (skate3_anim) for the arena sections; DumbadsSkate3ModdingTools (Ethanw05) not needed for these formats.
