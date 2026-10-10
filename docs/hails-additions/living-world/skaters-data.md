# Living world: NPC skater data (milestone M1)

2026-10-04. Branch `world/living-world`. Code and disc data only, no game
launched. Labels: [code] retail code (TU3 recomp disassembly as reference), [data] the user's disc, [trace] recomp runs.
Format reference: DumbadsSkate3ModdingTools by Ethanw05 (field names of tAIPath / tAIPathNode / tAIPathNodeExtData from
Skate 2 symbols; credits there to SunJay, Dumbad, RenderWareGavin, Tuukkas). No code copied; every field re-checked on
the disc. Credit it in the doc and PR.

## 1. What was built
| file | what |
|---|---|
| `crates/skate-data/src/aipath.rs` (new, `pub mod aipath` in lib.rs) | AIPATHDATA parser (`parse(&[u8]) -> Vec<AiPath>`), pack container (`parse_pack`, `write_pack`, `PackTile::grid`), retail-style dedupe by id (`district_paths`), `decode_orientation`, flag constants. Bytes in, no I/O, no deps, no unsafe. 5 unit tests on synthetic blobs. |
| `crates/skate-data/tests/aipath_data.rs` (new) | 2 data-gated tests; skip with a note unless `SKATE3_ASSET_ROOT` (exported packs) or `SKATE3_AIPATH_BLOBS` (raw research blobs) is set. |
| `tools/asset_pipeline/living_world_skaters.py` (new) | `export(ctx) -> dict`: packs + index + `skater_profiles.json`. |
| `tools/asset_pipeline/test_living_world_skaters.py` (new) | 13 unittest cases (synthetic) + 1 disc test gated on `SKATE3_DISC_ROOT`. |
| `tools/asset_pipeline/native_roster.py` (extended) | `npc_pool(rows)` (free-roam pool with the look source per character), `bind_teammates(pool, library, bindings)` (runtime teammate hook). Existing `roster()` / `prepare()` unchanged. |

## 2. AIPATHDATA layout (all [data], verified over all 280 tile objects, 0 inconsistencies)
Big-endian; every `m_p*` is relative to the struct that holds it.
- Header: u32 path count, u32 path array offset (0x10), 8 B 0xDE pad.
- tAIPath 96 B: bbox min/max vec4 (w 1); +0x20 16 B `m_ID` (byte 0 = 0 on all 3,891, bytes 1..5 tag `dwtn`/`univ`/`indu`,
  byte 5 = 0, bytes 6..7 one of 3 values, last 8 bytes look like a recording timestamp, e.g. 0x4A3AFB4B = June 2009);
  +0x30 nodes, +0x34 node count, +0x38 ext block, +0x3C branch groups, +0x40 group count, +0x44 flags, +0x48 u64
  allowed skaters, +0x50 i32 skill level (0 on every path), +0x54 3 x i32 (0).
- Node 44 B: pos, direction (displacement since the previous node), board / skater orientation 4 B each, +0x20 ext
  offset (node-relative; the "rare u32 flags" of the earlier notes were these offsets), +0x24 frames since previous
  node, width left, width right (255 unbounded), event (0/1/2/4 only), flags (bits 0..3; values 0..13, 17, 21, 24
  seen, so bit 4 also occurs a few times), 3 B zero.
- Orientation: 4 x u8, `(b - 128) / 127`, norm 0.997 median (p1 0.989, p99 1.011): a biased quaternion. Component
  order (likely x, y, z, w) not confirmed from code.
- Ext 40 B: trajectory start pos, start vel, offset (3 x vec3), i16 trick index, i8 180 spin count, u8 flags. Ext
  records are contiguous, the first at the header's +0x38 [data].
- Branch group 12 B: u32 offset (group-relative) of its branches, u32 count, u32 node index on this path. Branch 24 B:
  16 B target id, u32 target node, f32 in 0..1 (p50 0.49; meaning open: position along the target or a weight).
  Every branch target id exists and every target node is in range. NOTE: Dumbads' builder writes 0 groups ("engine
  recomputes at load"); the shipped data has them (1,008 of 1,691 paths, 4,442 branches), so we keep them.

### Key finding: tile copies
The 3,891 "paths" of the census are **copies per tile**: **1,691 unique ids** [data]. Copies with the same id are
byte-identical (nodes, header, groups; only offsets differ), up to 12 tiles per path. Retail keys the path manager
by `m_ID` (Skate 2 `PathManager` hash map per the Dumbads notes), so the engine must dedupe the same way.
Unique figures (replace the census ones that counted copies): DownTown 760 paths / 47,988 nodes (100 tiles),
University 508 / 47,469 (99), Industrial 423 / 47,738 (81); **143,195 nodes, 208.7 km**, length p10/50/90 =
38 / 96 / 244 m. Flags: 7 (1,276), 4 (210), 0x47 (124), 1 (48), 0x44 (23), 0x41 (4), 0x64 (3), 5 (2), 0x45 (1).
Allowed mask: all 62 bits on 1,259; bit 51 (teammates, IP, community) on 1,378. Node and length medians in the
earlier notes (8.5 m/s, 60 Hz) are unaffected (copies are identical).

## 3. Export formats (setup group `livingworld`, NPC skater half)
`export(ctx)`; ctx is a dict or object: `game_root` (extracted disc root), `private` (stage `assets/private`) or
`stage`, `work`, optional `report(text)`, optional `collections` (pre-converted vault rows, skips the db.big
conversion). Writes under `<private>/living_world/`, returns `{version, districts{tiles, path_copies, paths, nodes},
path_copies, paths, nodes, characters, profiles, free_roam_pool, outputs, sha256, warnings}`. Inputs: `worldDIST_*.big`
(`*_Sim.xst` + `cSim_*.xsf`, region processor 0xAB329A6A arenas, object type 0x00EB0014; asset copies read once like the
audio region export) and `data/big/db.big` (skaterschema / skatercollections + both summary reports for names).
- `skater_paths/<District>.bin`: our pack: `LWSKPTH\0`, u32 version 1, u32 tile count, 48 B tile entries (u64 asset id,
  u32 offset, u32 length, 32 B tile name), then the retail blobs verbatim, 16-byte aligned. Choice: raw blobs + index
  rather than a re-encoded format, because (a) one decoder (`skate-data::aipath`) serves disc and export, so the
  data-gated test proves the shipped file; (b) no lossy conversion or own field choices (1:1 retail); (c) the tile split
  is the retail streaming unit (paths per tile, dedupe by id), which M3 streaming can mirror; (d) mods can author lines
  in the retail layout or patch by id. Cost: 19.4 MB for the three cities (vs ~8 MB unique).
- `skater_paths/index.json`: per district tiles (name, asset id, copies) and every unique path by its 32-hex id
  (tiles, nodes, flags, allowed mask hex, skill, start node, bbox, length, branch count): the key a content overlay
  patches.
- `skater_profiles.json`: `ai_skater` (1 record, 38 tunables, still hash-named), `ai_skater_profiles` (193, inheritance
  applied, `ProfileTrickEntry` arrays as `{trick, weight}`, RefSpecs resolved to `{class, key}`, enums as ints),
  `characters` (87 `characters_marquee` rows: recipe, name id, voice, aiprofile key, pro_index from the profile's
  `Hash_A390BE5FC0DDA71C`, `layout {+8, +9, +16, +17}`, community, teammate, teammate_index, linked, `free_roam_pool`,
  `needs` = `recruited_save_slot` / `offline`, plus all resolved fields), `free_roam_pool` (42 keys). Keys are the
  vault record names: stable string ids a mod overrides by key.
- Verification export: `<stage>/assets/private/living_world/` (+ `export-report.json`,
  `run_export.py`, `probe_aipath.py`). 17-20 s; two runs give identical sha256 for all 5 files; 0 warnings.

## 4. Characters and the teammate path
- [data] Free-roam pool (byte +9, `sub_82461550` [code]) = 42 records: 33 marquee looks (pros incl. cuz, seb, deerman),
  `teammate_01..04` (`teammate_index` 1..4, profile `teammate_default`, pro index 51), `community_skater_01..05`
  (online uploads). Roster entries NOT in the pool: coach_frank, dem_bones, dr_pepper, isaak, reda, shingo,
  skate_ambassador, steak. `ambient_skater_01..09` have +9 = 0 (not in the pool, confirmed).
- [data] marquee.big has **no recipe for teammate_NN** (or community): the look exists only in the save. [code] it is
  loaded from save slot table 0x8309A800 + index x 23904 when word +8192 != 0 (`sub_827CE490` via `sub_82461550`).
- So `native_roster.npc_pool` marks each pool member's look source: `marquee` (converted by `prepare()`, same identity
  `sha256('native-marquee-v1:'+key)`), `save` (teammate), `online` (community: no data, never spawned without a mod).
- Teammate hook (designed, not faked): the save slot's equivalent is the player's customiser library. A runtime
  binding file `settings/living_world_teammates.json` = `{ "teammate_01": "<library entry id>" | null, ... }`;
  `bind_teammates()` returns the bound entries whose `library/entries/<id>/character.glb` exists. Unbound = an empty
  save slot = not eligible (retail parity: unrecruited teammates never spawn). Who writes the file is open (a menu
  "recruit" action, a career save importer, or a mod via `sdk.living_world`); milestone M3 decides the runtime
  side. Voice (100..103) and profile come from `skater_profiles.json`.

## 5. Moddability: entry points foreseen (M4)
- Lines: overlay by path id (remove, replace node list, change flags / allowed mask), extra lines as retail-layout
  blobs or as a node list in the same schema; mod "spots" = lists of path ids.
- Characters: add / override `characters` entries by key (recipe or mod model, profile, voice, pool membership);
  bind teammates (`bind_teammates` input); allow community slots for mod characters.
- Profiles: override fields / trick tables per profile key; `ai_skater` tunables by field (names open).
- All three files are keyed JSON / id-keyed packs, so `sdk.living_world` can patch by key and drop the patch on mod
  disable.

## 6. Tests and results
```
cargo test -p skate-data --lib --tests --locked
  -> all pass (aipath: 5 unit; 2 data-gated skipped without env)
SKATE3_AIPATH_BLOBS=<folder of extracted 0x00EB0014 blobs> cargo test -p skate-data --locked --test aipath_data
SKATE3_ASSET_ROOT=<stage>/assets cargo test ... --test aipath_data
  -> 2 passed (3,891 copies, 389,676 node copies, 1,691 unique, tags, bbox, 60 Hz ratio median within 5 %,
     branch network closed, ext indices valid, pack dedupe = 1,691)
py -3.13 -m unittest tools.asset_pipeline.test_living_world_skaters          -> 13 OK, 1 skipped
SKATE3_DISC_ROOT=<extracted disc> py -3.13 -m unittest tools.asset_pipeline.test_living_world_skaters
  -> 14 OK (disc totals 3,891 copies / 1,691 paths / 87 characters / 193 profiles / 42 pool)
```
`rustfmt` applied to the two new Rust files only (the crate's examples are not rustfmt-clean upstream). No em
dashes or control characters in the new / changed files.

## 7. Open items
1. Branch f32 meaning; node flag bit 4 (seen on a few nodes); orientation component order; ext `trajectory_offset`
   use; path flag bit names (inferred order, not read from code). All M5.
2. Bytes 6..15 of `m_ID` (constant per recording session + timestamp?) and how retail hashes the id.
3. `ai_skater` 38 tunables and most profile fields still hash-named (M5); `pro_index` is the only profile field named.
4. Teammate binding writer (UI / save import / mod) and where `settings/living_world_teammates.json` lives at runtime.
5. Wiring: the `livingworld` group (versions.py fingerprint must include `living_world_skaters.py`, `vlt.py`,
   `names.txt`, the stream reader), install.py call, and the engine loader: the setup group is now wired (see `peds-data.md`); the engine loader comes with M3.