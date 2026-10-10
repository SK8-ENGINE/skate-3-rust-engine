# Living world: pedestrian data and the setup group (milestone M0, 2026-10-04)

Branch `world/living-world`. Design: `27-peds-design.md` section 2 / M0.
Tags: [code] read from the TU3 code (addresses are evidence only), [data] read from the disc, [trace] recomp only.

## What was built
| Item | Status | Where |
|---|---|---|
| Setup group `livingworld` (own fingerprint, optional) | done | `versions.py` GROUPS + SOURCES, `install.py`, `group_receipts.py` ROOTS, `asset_exports.livingworld` |
| Tables `tables.json` (29 vault classes) | done | `tools/asset_pipeline/living_world.py` `tables()` |
| Census grids per district | done (4 m grid) | `<District>.census.bin` + `census.json` |
| Road network `roads.json` | partial: header + segment table decoded; nodes, lanes, crossings kept raw in `roads.bin` | `parse_roads()` |
| Waypoints `waypoints.json` | done | `parse_waypoints()` |
| NavPower | inventory only (M3) | `navmesh.json` |
| Binary `.recipe` reader | done (all 77 recipes) | `parse_recipe()` |
| Ped + hand-prop GLBs (both LODs, parts, textures) | done: 77 / 77 (51 peds, 26 `zprop_*`) | `tools/asset_pipeline/living_world_models.py` |
| Validation | done in Python (`living_world.validate`, runs in the export) | see "Open" for `--check-assets` |
| Tests | 16 new Python tests | `tools/asset_pipeline/test_living_world.py` |

The group runs `living_world.export` (peds) and then `living_world_skaters.export` (NPC skaters) with one vault conversion, and writes `living_world/export.json` (both reports + sha256 of every file). On
any content error it removes `living_world/`, writes `living_world-availability.json` (status `unavailable`, shown by
`validation_report.summary`) and setup continues: the world runs empty. Fingerprint inputs: `living_world.py`,
`living_world_models.py`, `living_world_skaters.py`, `vlt.py`, `names.txt`, `audio_formats.py` (region layers),
`optional_content.py`, `character_glb.py`, `retail_character.py`, `extract_default_skater.py`, `vendor/utt/**/*.py`,
`skate3_streams.py`, plus the `livingworld` function of `asset_exports.py`.
A `core` change rebuilds it too (existing rule). The group does its own vault conversion (with the summary-report record
names, like the customiser) instead of core's `converted`, so record keys match the skater half.

Run on the user's disc (verification output `<stage>/assets/private/living_world/`,
log `m1-out/peds/export-run.log`): 40 s for both halves, 53 MB, validation clean, one warning (below).

## Formats
### tables.json
`{"version": 1, "classes": {class: {record: {"parent", "fields": {name: value}}}}, "field_names": {class: {readable: Hash_}}}`.
Inheritance applied (parent fields first, child overrides). Values: floats as the shortest decimal that reads back as
the same f32 (3.167, not 3.1670001); RefSpecs `{"class","key"}` or `null`; enums as ints; text, text arrays; structs
decoded by `STRUCTS` with readable members where known (`tLWCensusEntry {group, max_population}`, `tLWGroupEntry
{category, weight}`, `tLWCensusCircle {spawn_inner, spawn_outer, cull, forward_offset, speed_kmh}`,
`tRecoveryCollisionIntervals {lying_down_until, crouched_until}`, `tAnimTakedown {anim, anim_b, ..., reach, angle_min,
angle_max}`, `tAnimAttributes {anim, ..., window_0..2: [start, end, tag]}` …) and offset names (`f32_24`) otherwise,
so a key stays stable once its meaning is found. Classes: `livingworld`, `_census`, `_census_ranges`,
`_categorygroups`, `_entitycategories`, `_entities` (+ chase, perceptions, navigation, locomotion, moodreactions,
moodresults, knowledge, protect, patrolzone), `_moodeventcategories`, `_models`, `_entity_animation` (+ postadjust),
`_entity_takedown`, `_entity_headtracking`, `_handprops`, `_handprops_usagecharacteristics`, `_conversations`,
`_conversation_categories`, `_conversation_category_groups`, `_load_groups`, `_props`, `physics_ai`.

### <District>.census.bin (little-endian, documented in `write_census_grid`)
`LWCENSUS`, u32 version 1, f32 cell (4 m), f32 origin x/z, u32 width/height, u32 layer count, u32 name count, per layer
32-byte name + u32 offset, names (u16 length + ASCII), then per layer width x height u16 cells (row-major z then x;
0 = unpainted: for peds and vehicles no record and cap 0, `sub_826B8A28`, see doc 26 milestone 2; k = name k-1). Cell value = the record at the cell centre, from the exact
quadtree (`audio_formats.region_key`). Sizes: DownTown 384 x 448, Industrial 640 x 288, University 384 x 448 cells.
Layer keys are the vault hash of the `livingworld_census` record name [data] (all keys resolved).
Records painted: DownTown aletown, business_center, mall, memorial, residential (+ vehicles dwntwn); Industrial
loadingdocks, reclaimed, downtown, pedestrians (+ indust); University campus, observatory (+ univ) [data].
Warning: DownTown's `pedestrians` key covers about 0 ha (a sliver, `census_layers.txt` 0.00 ha) and falls in no 4 m
cell; a finer grid or the raw quadtrees would keep it.

### roads.json / roads.bin (`0x00EB0013`) [data, 82 objects]
Object header: bbox min/max (vec4, y up), u32 intersection count, u32 segment count (twice; one University tile
differs), u32 intersection table offset (stale pointer when the count is 0), u32 node table offset, u32 segment table
offset. Segment 0x40: u64 id, u64 node A, u32 end A, pad, u64 node B, u32 end B, f32 length, f32 width A, f32 width B,
f32 speed limit (14.17 m/s = 51 km/h; some 13.89), u32 word_52, u32 word_56 (1-3; probably lane counts,
unverified). Per district: DownTown 46 unique segments / 19 nodes / 19 intersections in 51 objects; Industrial 26 / 10
/ 10 in 19; University 5 / 6 / 4 in 12. (V0, [`vehicles-data.md`](vehicles-data.md): University has 4 segments; the
fifth was a phantom from reading the lane-run count as the segment count. `roads.bin` is now the v2 road graph and
the verbatim pack moved to `roads_raw.bin`.) `roads.bin` = every object verbatim (`LWROADS\0` pack, same layout as the
skater path packs) so M3 decodes nodes / lane samples (4 m, 0x19 per block) / crossing quads without a new setup.

### waypoints.json (`0x00EB001A`) [data, 59 objects, 74 groups, 394 waypoints, layout checked on all]
Group 0x60: vec4 centre / min / max, u64 name id, u64 GUID, u64 class id, u64 type id (vault hash of
`waypoint_vendingmachine` etc.), u32 count, u32 first waypoint offset, u32 strings, u32 class-name offset. Waypoint
0x30: vec4 position, vec4 facing, u32 word (0/1), u32 group offset (back-pointer, checked), u32 locator name, u32
class name. Types present in streams: only `waypoint_vendingmachine` (54 groups) and `waypoint_usetrashbin`
(University 20). Sit / ATM / fountain / newspaper / conversation anchors are not in the waypoint streams: they come
from the DMOs ("DMO Waypoint Group (From Hotpoint)", `npc-livingworld-re` section 2) and need the DMO export (M6).

### models.json + models/<recipe>.glb
Binary `.recipe` [data, all 77 parse]: u32 version 7, string name, u32 x3 (1, 15, 0), u32 part count; part: string
slot, u32, u64 id, u32 LOD count; LOD: u64 id, u8, u64 model arena id, u32, u32 material count (0/1), if 1: u64
material id, u32 texture count, (string channel, u64 id) each; trailer: zero padding to 4, zero or one u32 0, u32 = its
own offset. Strings are u32 length + bytes. Every arena and texture exists in `livingworld.big` (0 missing).
GLB: skeleton = the arena's own bind skeleton (peds 39 named bones, root `Hips`; Eyelids / HeadEnd / Eyebrow / Mouth /
thumbs have no stored parent and hang off the rig root), parts as primitives, two meshes `LOD0` / `LOD1` sharing the
skin, diffuse (+ alpha, MASK) and converted normal map; arena space (metres, Y up). Visual check: `male_jock_1`,
`female_adult_1` thumbnails render correctly textured (`m1-out/peds/female_adult_1.png`). `zprop_saftybarrier` LOD1
is empty in the data (no mesh, no material) and is written with LOD0 only. 76 recipes have both LODs.

## Values and findings (new or confirmed)
- [data] census ranges pedestrians: slow 50 / 60 / 70 m, offset 0, 45 km/h; fast 50 / 80 / 90 m, offset 20, 80 km/h
  (confirms `npc-livingworld-re` section 1). Census aletown max 15, extra fields 6.5 / 5.0.
- [data] knock-down thresholds 3.0 / 3.0, flagged 6.0 / 6.0; recovery intervals 2.0 / 3.167, left 1.667 / 2.334
  (match `peds-re` section 5, read by `sub_82E38FB8` / `sub_8269E460` [code]).
- [data] NEW: `livingworld_entities` field `Hash_F89323E420A6AAA3` (`plugin_odds`) = per-entity probabilities of the
  waypoint plugins (entities `waypoint_atm`, `_sit`, `_vendingmachine`, `_conversation`, `_usetrashbin`,
  `_waterfountain`, `_newspaperbox`), e.g. skater_female trash bin 0.65, jock 0.6, worker_male 0.5 and water fountain
  0.0. This answers the open "trash-bin plugin odds" item at the data level; the code that rolls them is not read yet.
  `Hash_46B836EE959C0238` (`handprop_odds`) = hand-prop probabilities (skater_female skateboard 0.7, waterbottle 0.2,
  pop 0.1). `Hash_942AB8AE…` = record name, `Hash_BD8F02BA…` = display name, `Hash_CA4D0AD4…` = AI graph path.
- [data] Entities may point at a GROUP model record without a recipe (`skater_female`, `skater_male`, `worker_male`);
  the concrete looks are the records that inherit from it (`skater_female01..03` → `female_skater_1..3`,
  `worker_male01..06`). The export resolves these (`model_recipes`); which child retail picks (uniform random?) is not
  read from the code yet (census entity choice `sub_826B8B88` / factory).
- [data] Every census record painted on the maps resolves: group → categories → entities → recipe → GLB.

## Mod entry points foreseen (for `sdk.living_world`, M4 / M7)
- Content overlay patches `tables.json` by `class / record / field` (readable or `Hash_` name, `field_names` maps
  both); new records (entities, models, census) by new keys.
- Census: replace / add a district grid (same `LWCENSUS` format) or patch cells; `census.json` lists names.
- Waypoints / roads: add groups by GUID / segments by id in per-map JSON (`<map>.living_world.json`).
- Models: a mod GLB with the ped bone names replaces `models/<recipe>.glb` or adds a recipe key referenced by a
  `livingworld_models` record.
- Cleanup: everything lives under `living_world/`; overlays are separate files, so disabling a mod restores the
  exported defaults.

## Tests
- `py -3.13 -m unittest tools.asset_pipeline.test_living_world` (from the worktree root): 16 tests OK (synthetic data
  only: tables / structs / inheritance / names, census chain incl. group models, census grid round trip and lookup,
  roads, waypoints, pack, recipe incl. material-less LOD and truncation, validation good / missing recipe / missing
  tables, skinned two-LOD GLB, group wiring).
- All pipeline tests: `py -3.13 -m unittest <every tools/asset_pipeline/test_*.py module> tools.test_setup_assets`:
  176 tests OK (2 skipped). `test_setup_assets` exclude tuple gained `livingworld` (new group in `ROOTS`).
- Real-disc export: `py -3.13 -m tools.asset_pipeline.living_world --game <disc> --private <out> --work <tmp>`
  (or `asset_exports.livingworld`), 40 s, validation clean.
- No Rust added, so no Rust tests; skate-game not built (no Rust change).
- Control-character and em dash scan of all new / edited files: clean.

## Open
1. **`--check-assets` in the game**: validation runs in Python at export time (a failure makes the group unavailable).
   The Rust side should load `tables.json` + check census → models when `LivingWorldData` exists (M1); not added now to
   keep skate-game untouched while it does not compile on this branch.
2. Road network nodes, lane sample blocks and crossings (intersection quads, crosswalks): raw in `roads.bin`; decode in
   M3. word_52 / word_56 meaning (lanes?) unverified.
3. NavPower (`0x00EB0027`): inventory only (DownTown 240 objects, 6.0 MB; Industrial 210, 1.5 MB; University 228,
   3.0 MB).
4. Plugin anchors other than vending machines / trash bins come from DMO hotpoints: needs the DMO waypoint export.
5. Census grid is lossy for slivers below 4 m (DownTown `pedestrians`); the raw quadtrees could be kept instead if M1
   needs exact parity at edges.
6. Ped skeleton vs animation rig: GLBs carry the arena's 39 bones; `PedestrianSkeletonPres.abin` has 50 bones (with
   trajectory, face / hand extras). Bind-order check by name in M2 (design risk section 9).
7. Unnamed struct members and most entity / chase / perception fields stay `Hash_*` / offset names until code sites
   are read; group-model child choice; `tLWHandPropUsage` f32_24 / f32_28 (bum 0.2 / 0.1).
8. The skater exporter's report lists every file under `living_world/` (including the ped files) as its outputs:
   cosmetic, its file, left as is.

## Credits
Region-layer quadtree parser: our `audio_formats.region_layers` (audio work). DumbadsSkate3ModdingTools (Ethanw05) was
not needed for these formats (its NavPower constants become relevant in M3); credit it where its layouts are used.
Vault reader `vlt.py` layout reference: NFSTools/VaultLib (MIT).
