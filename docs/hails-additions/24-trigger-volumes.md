# 24: Named trigger volumes, exported, tracked, moddable

Branch: `feature/trigger-volumes` (from `main` 4488651). Related: PR #25 / doc 05 (drops the same boxes from solid
collision). This change does not touch collision, so it neither needs nor conflicts with #25.

## Problem

Retail collision archives hold clustered meshes with no surface on any triangle: 12-triangle boxes. PR #25 found
they were invisible walls and drops them from solid collision, but that only throws them away. They are the
collision shapes of the maps' **named trigger volumes**: SkateSchool's tutorial areas and map-wide DMO (dynamic prop) reset box,
the MegaPark and Maloof teleport prompts, DownTown's session spots. The engine had no notion of trigger volumes at
all, so nothing (engine or mod) could react to the player entering one.

## What retail does

Reference: the TU3 build through skate3recomp (by @mchughalex, built on the rexglue SDK), the converted disc data
read in place, and guarded hook runs of the recomp. Addresses are TU3; no game code or data is copied.

**Data.** Each volume is an item of a `0x00EB0019` volume-set record in an RW4 (RenderWare 4) simulation arena (ATOC processor
`0xAB329A6A`) of the district's `cSim_*.xsf` streams (fixup `82962538`, stream-in `82C9AFD8`). Item (240 bytes):
matrix +0, bounds +64/+80, **link GUID** +176, **instance id** +184 (the id challenge scripts address,
`0x2C701706xxxxxxxx`), **group word** +216, +220 the index of a `0x00EB000A` link record, +224 the name
(`<short>_0x…:0x…::[…]_HighLOD`). The link record names an RW collision volume of **type 4 (box)**: 3×3 rotation
rows (the box's axes in world space), centre, half extents, fatness. That box is the real shape; the bounds only
enclose it (SkateSchool's `inthehub_vol_02` and `wrongway_vol_01` are 45° squares). A second link entry names the
clustered mesh, the surfaceless box PR #25 drops.

**Runtime** (`cTriggerVolumeMessageGroup`). The trigger manager (`82DD7AE0`) owns three groups: **Challenge**,
**Stairs**, **Camera**, and adds each streamed-in item to one of them by its group word (`82DD7C58`: 1 Stairs,
2 Camera, anything else Challenge). Only Challenge tracks entities: the group descriptors (`0x82FCA108`) build
Stairs and Camera as the smaller group class with no entity slots and no entity list, so those two never post
enter/exit (Camera is looked up by volume name around a position, `82DF90C0`). Each sim tick (`82DD70E8`), for
every tracked entity (up to 9 slots):

1. Its query shape is rebuilt (`82DD80B8` → `82ADA4E0`): a **cylinder** (RW volume type 5) of **radius 0.34 m**
   from three points the skater entity reports (vtable +12 / +16 / +20): its **feet** (the character's ground
   point), its **head** and its **hips**. The axis runs `feet - hips` (down), the half-height is
   **`0.5·|head - feet| + 0.05`** and the centre lies **`half-height - 0.02`** above the feet, so the cylinder runs
   from 2 cm below the feet to 8 cm above the head (about 1.72 m for a standing skater).
2. An AABB tree of the volumes' bounds gives candidates (`82DD90E0` → `82476158`), and each candidate is tested
   against the volume's **box** (`82DD8BC8` → `82DD8978` → `82DD8498` → RW volume-volume overlap `82AD3CD8`,
   tolerance 0.0). The bounds contain the box, so the narrow test decides.
3. The new set is diffed with last tick's (`82557600`): **entered volumes are posted first, then exited ones**, as
   `cMsgTriggerEnterCollision` (0x5EAD1B62) / `cMsgTriggerExitCollision` (0xE961529A), each **twice**: once with
   the link GUID, once with the instance id, plus the entity id.

Removing a volume (`82DD7018`) drops it from every entity's set **without** an exit message; removing an entity
(`82DD6D20`) posts exits for every volume it was in.

**Which entities.** The tracked entities are **skaters**: the player and the NPC skaters around them, all of one
class (entity interface at skater +8, vtable `0x823009B8`). Peds and boards are never tracked.

**Where they live.** Only 11 volumes are in the world streams (= PR #25's boxes): SkateSchool `coach_frank_sksc`,
`ws_sksc_coachfrank_instance_01`, `tut_sksc_inthehub_vol_01`, `tut_sksc_inthehub_vol_02`, `tut_sksc_reset_vol01`
(the whole map, Y −73…79), `tut_sksc_wrongway_vol_01`; MegaPark `tele_stadium_to_world_volume_a`; Maloof
`tele_mega_ramp_up_volume_01`, `tele_mmcp_to_dwtn_volume_01`; DownTown `dwtn_sessionspot_01_kubetower_volume_01`,
`dwtn_sessionspot_02_spillway_volume_02`. The other 2,549 are in 337 challenge packages (`missions.big`), which
stream with their challenges (see "Answers" below); this change exports the world-stream ones only.

## Answers to the open questions (recomp, 2026-10-04)

Evidence: a guarded hook on the query builder `82DD80B8` (head and hips from its frame, its output centre and axis;
the feet point is rebuilt from the output because the builder reuses that stack slot), item +216 on every AddVolume,
the dynamic hull manager's set diff, and one muted background scripted run (Mega-Park spawn, PCU Library,
skate.School; 0 malformed lines), plus the world hooks in three of the user's earlier sessions and static reading of
the disc's challenge database (`db.big` challengebanks, read through the repo's VLT reader).

1. **The three points.** Feet, head, hips (vtable +12 / +16 / +20). Standing, the recomp's player has the head
   1.61-1.65 m and the hips 0.97-1.00 m above the feet point, and the feet point lies exactly on the ground plane
   (y 74.000 at PCU Library, 0.000 at SkateSchool). The dynamic hull manager queries a 0.58 × 1.74 × 0.58 m box
   standing on the same feet point. In one owner state (300, not identified) the feet point is a 0.68 blend of two
   skeleton points instead.
2. **The 9 Challenge slots.** Skaters only: the player plus NPC skaters (six different NPC skater entities over the
   run, up to four entities at once, each 20-50 m from the nearest ped). No peds, no boards. Remote online skaters
   use the same skater class, so retail is expected to track them too; that could not be checked offline.
3. **The 2,549 challenge-package volumes.** Each challenge record names its package (`tRequiredChallengeHull`, 316
   records), and `cChallengeDynamicHullManager` streams the package in and out with the challenge (by name, through
   the `SetDynamicHullState` handler and the challenge scripts' `LoadAdditionalChallengeHull`); its volumes join the
   Challenge group on load and leave it on unload. In free skate (five traces) only three kinds load:
   **teleports** (`tele_*` / `teleport_*`): every teleport of the current district at district entry, at any
   distance (409-1,175 m), unloaded when the district changes, so University's `tele_world_to_stadium_volume_01`
   is loaded whenever the player is in University; **own-the-spot** spots (`ots_*`) near the spot (loaded with the
   player 0-111 m from the package's nearest volume, unloaded 37-160 m away or on teleport; the exact rule is
   open); and the create-a-park object boundaries (`Mega_skpk_Park_Volumes`). No race, film, photo, Hall of Meat or
   tutorial package loaded in free skate. Consumers: teleport volumes drive the teleport challenge's prompt (volume
   id from `challenge_local_data` `TeleporterTrigger`, confirm, teleport); own-the-spot volumes are the challenge
   boundary and objective triggers of their challenge; the DownTown session-spot volumes are referenced by a
   living-world record together with three `dwtn_sessionspot_01_locator_0N` locators.
4. **Stairs, Camera, SkateSchool.** Every shipped item (11 world, 2,549 package) has group word 0, so Stairs and
   Camera are empty in Skate 3, and they track no entities anyway (above). SkateSchool's `tut_sksc_reset_vol01` is
   not a player reset: it is the **DMO reset area** (`tDMOResetInfo`, mode 2) of every SkateSchool tutorial and of
   `1up_sksc_01`, the region whose movable objects are reset when the challenge (re)starts (76 challenges use mode
   2 with their challenge boundary). `tut_sksc_wrongway_vol_01` and `inthehub_vol_01/02` feed tutorial coach hints
   (tutorial 3's "wrong way" objective, the "in the hub" hints); they do nothing in free skate.

## Root cause

The converter never read the `0x00EB0019` records, and the engine had no trigger system to give them to.

## Change

### Setup (group `maps`, one refresh)
- `tools/asset_pipeline/map_volumes.py`: reads every simulation arena of a district stream once (only processor
  `0xAB329A6A` arenas are decoded), parses each volume set, follows item → link record → box volume, composes the
  item matrix, checks the box against the retail bounds (≤ 0.25 m; retail's rotated bounds are a few cm off) and
  writes **`maps/<Map>.triggers`** (JSON, `format: "skate3rust-trigger-volumes"`, `version: 1`): id (= instance
  id), short and full name, instance id, link GUID, group (from the item's group word: 1 stairs, 2 camera, else
  challenge), oriented box, retail bounds, stream and arena. Maps with no volumes get an empty list (so "none" and
  "not converted" differ). Standalone staging without a conversion:
  `python -m tools.asset_pipeline.map_volumes <worldDIST_*.big…> --output <dir>`.
- `install.convert_map` writes it beside the `.skate` (like `.irradiance`); a failure is an optional-content note
  (`map-status/<Map>-triggers-availability.json`), not a failed map.
- A sidecar, not a new `.skate` section: an older engine refuses unknown `.skate` extensions
  (`validate_runtime`), so an extension would break every other build sharing the install; the sidecar is ignored
  by builds without this change.

### Engine
- `skate-core::triggers` (pure): `OrientedBox`, `Cylinder`, `QueryShape` (retail constants as data:
  `QueryShape::RETAIL` = radius 0.34, length scale 0.5, length pad 0.05, foot pad 0.02, and
  `cylinder(feet, head, hips)`), the narrow test `overlaps` (GJK distance with exact box and flat-capped cylinder
  support, f64 inside), and `Tracker` (per-body inside sets; enters then exits per body in order; removed volumes
  forgotten silently; removed bodies exit).
- `skate-data::trigger_volumes`: the format (serde), validation (unique ids, 64-bit hex ids, orthonormal axes,
  finite values, ≤ 4096 volumes), `TriggerGroup::tracks_bodies` (Challenge only), and `load_for_map`:
  `<Map>.triggers` next to the package wins over an embedded **`TVOL`** extension (our trigger volume tag in the `.skate` file) (schema 1, same JSON) so custom
  maps can ship volumes either way; neither = no volumes. `skate_world::validate_runtime` accepts and validates
  `TVOL`.
- `skate-game::trigger_volumes`: resources `TriggerVolumes` (map volumes in file order = retail registration
  order, mod volumes, mod switches), `TriggerBodies` (engine hook: any system can register a body's query
  cylinder by id), `TriggerState`; messages **`TriggerEntered` / `TriggerExited`** `{body, volume}`; system set
  `TriggerSet` (FixedUpdate, after physics, gameplay only). Only Challenge-group volumes take part in enter/exit
  (retail's Stairs and Camera groups have no tracked entities); all groups stay listed. The player is body
  `"player"`, tracked first like retail slot 0, from retail's three points (`player_points`): the feet (the
  animation root, the character's ground point), the head body and the hips body. A map transition loads the new
  map's volumes on the loader thread and replaces the list at commit; insides are forgotten without events (retail
  unload is silent). Log lines `SKATE_TRIGGERS map=… volumes=… source=…` and
  `SKATE_TRIGGER enter|exit body=… volume=… name=… group=…`.
- `--check-assets` loads the map's trigger data and fails on a broken sidecar / `TVOL`
  (`SKATE_TRIGGERS_READY volumes=N`), so setup catches exporter errors; at runtime a broken sidecar is a warning
  and the map has no volumes.

### Modding (Lua API 2, `sdk.capabilities.triggers` = 1)
- Read: `sdk.triggers.list()` / `get(id_or_name)` / `inside(body)` (snapshot `triggers`), every field including
  retail ids, box, rotation, bounds, enabled, who is inside.
- Events: `on_event {name="trigger_entered"|"trigger_exited", body, volume, volume_name, group, instance_id,
  link_guid}`, delivered before `on_fixed_update` of the same tick (one event per transition; both retail ids are
  in it instead of retail's two messages). Only `challenge` volumes post events, like retail.
- Write: `sdk.triggers.box(key, {center, half_extents, rotation, name, group})` (id `mod:<mod>:<key>`, ≤ 64),
  `remove`; `set_enabled(map_id, false)` switches a map volume off; `track(body_key, {radius, length})` follows a
  mod physics body (≤ 16; the cylinder stands on the body's position and reaches `length` up, like the player's
  feet and head; positions after the dynamics step, one tick late); `configure({radius, length_scale, length_pad,
  foot_pad})` changes the query-shape constants (one mod at a time).
- Cleanup: everything a mod set goes when it stops, fails or the runtime resets; mod volumes / switches / tracked
  bodies are world-scoped (cleared on world change, like `sdk.volumes`).
- SDK docs: `sdk/ENGINE_API.md` (Trigger volumes), `sdk/skate.lua`, `sdk/GENERAL_API.md`.

### SkateSchool's reset box
`tut_sksc_reset_vol01` is exported and fires enter/exit like the others. Retail uses it as the tutorials' DMO
reset area (answer 4), not as a player reset, and the engine has no tutorial challenges or DMO reset yet, so no
behaviour is attached. It is exposed as data so a mod or a later port can act on it.

### Not ported (follow-ups)
- NPC skaters (the living world, doc 26c) and remote multiplayer skaters (expected to be tracked in retail) are not
  registered as trigger bodies yet; a multiplayer system can do it through `TriggerBodies` with each remote's feet, head and hips.
- Challenge packages (`missions.big`) and their streaming: they belong to a challenge system the engine does not
  have. Their volumes can be exported with the same reader when one exists.

## Files
- `tools/asset_pipeline/map_volumes.py`, `tools/asset_pipeline/test_map_volumes.py`, `tools/asset_pipeline/install.py`
- `crates/skate-core/src/triggers.rs`, `crates/skate-core/src/lib.rs`, `crates/skate-core/tests/trigger_volumes.rs`
- `crates/skate-data/src/trigger_volumes.rs`, `crates/skate-data/src/lib.rs`, `crates/skate-data/tests/trigger_volumes.rs`
- `crates/skate-game/src/trigger_volumes.rs`, `crates/skate-game/src/tests/trigger_volumes.rs`,
  `crates/skate-game/src/tests/trigger_points.rs`, `physics.rs`, `main.rs`, `app.rs`, `map_transition.rs`,
  `skate_world.rs`, `modding/triggers.rs`, `modding/mod.rs`
- `crates/skate-mods/src/vm.rs`, `crates/skate-mods/src/lib.rs`, `crates/skate-mods/src/api.lua`
- `sdk/ENGINE_API.md`, `sdk/GENERAL_API.md`, `sdk/skate.lua`

## Verification
- Counts per map (exporter on the owned disc, data-gated tests in Python and Rust): SkateSchool 6, MegaPark 1,
  MaloofMoneyCup 2, DownTown 2, University / Industrial / StartPark / BlackBoxPark / DownTownSkatePark /
  IndustrialSkatePark 0: **11**, names identical to the research and to PR #25's surfaceless boxes; ids, link
  GUIDs and bounds match `volumes.json` from the research tools; every group word is 0 (challenge).
- Tests: Python `test_map_volumes` (synthetic incl. the group word, + data-gated on the disc); skate-core
  `tests/trigger_volumes.rs` (10: retail cylinder from feet / head / hips, a recomp sample's centre and axis
  reproduced, analytic and sampled GJK checks, rotated boxes, tracker ordering and removal rules); skate-data
  `tests/trigger_volumes.rs` (5, incl. data-gated on converted maps via `SKATE3_MAPS_DIR`); skate-game
  `trigger_volumes::tests` (6, incl. only Challenge volumes posting events and the SkateSchool authored start
  entering `inthehub_vol_02` + `reset_vol01`), `trigger_points_tests` (stock skater, ignored without the private
  assets: head / hips / toes over the feet point on and off the board; run: head 1.60 / 1.61 m, hips 0.89 / 0.91 m,
  toes 0.04 m, against the recomp's head 1.61-1.65 m and hips 0.97-1.00 m; the hips point only tilts the axis, the
  length comes from the head), and `modding::triggers::tests` (3);
  skate-mods `trigger_api_crosses_the_lua_serde_boundary`. Full suites (release): only the known pre-existing
  failures (skate-game 2, skate-core 2, skate-mods skyline).
- End to end: the real `install.convert_map` (MegaPark, SkateSchool, scratch stage) writes the sidecars and its
  `--check-assets` step passes; `--check-assets` on all ten maps reports `SKATE_TRIGGERS_READY volumes=` matching
  the counts above.
- Fingerprints: only the `maps` group changes (core / hud / character / environment and the customiser unchanged).
- Collision: untouched; no collision code or data changed; `check_maps` / `--validate-maps` results are the same
  (the sidecar is not collision).

## Open questions
- The exact own-the-spot package streaming rule (distance from what, which radius).
- Owner state 300, in which the feet point becomes a 0.68 blend of two skeleton points.
- The Stairs group's consumer (empty in Skate 3, so no behaviour depends on it).
- PR #25 doc error (signed vertex deltas): fixed in doc 05 on `hails-additions` / the #25 description already
  (2026-10-02). The converter's `map_writer.spawn_point` fallback and `prepare_hawaiian_dream` bounds still use the
  vendored signed decoder: a separate change (it can move fallback spawns), not part of this one.
