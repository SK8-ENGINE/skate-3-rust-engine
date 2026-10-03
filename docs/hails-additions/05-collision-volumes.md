# 5. Invisible collision volumes

## Problem

On SkateSchool the player hit an invisible wall in the middle of the spawn
area and could not skate around. Logs showed normal physics states (no
failures), so the obstruction was in the collision data.

## Root cause

Retail collision archives contain a small number of clustered meshes in which
**no unit carries a surface ID** (unit flag `0x80` unset; units read as flags
`0x21` instead of the usual `0xa1`). Every one of them, in every shipped
district, is a 12-triangle axis-aligned box: zone, trigger or level-bounds
volumes. The game's loader (`skate_world::retail_collision_world`, via
`skate_data::retail_collision::visit_clusters`) turned every triangle into
solid collision, so these volumes became invisible walls, floors and ceilings.

SkateSchool has seven, all in its global stream (`cSim_Global.xsf`). Names
are those of the matching retail trigger-volume records (see "What the boxes
are" below):

| Box (X / Y / Z, metres) | Name | Effect in game |
|---|---|---|
| −79.1…16.5 / −1.53…6.65 / −43.3…52.3 | `tut_sksc_inthehub_vol_02` | covers the whole school area; the authored start (y 0.11) is inside it |
| −62.1…−0.5 / −1.53…6.65 / −3.6…12.7 | `tut_sksc_inthehub_vol_01` | the invisible wall in the spawn area |
| −11.0…2.5 / 0…4.17 / −2.6…1.6 (three identical) | `coach_frank_sksc` / `ws_sksc_coachfrank_instance_01` | invisible block near the origin |
| −212…578 / −73…79 / −483…492 | `tut_sksc_reset_vol01` | level bounds: floor under the whole map |
| 104.1…199.7 / −4.1…4.1 / 151.5…247.1 | `tut_sksc_wrongway_vol_01` | — |

Evidence that these are not solid in the original game: every authored
SkateSchool locator (Coach Frank, the hub, all tutorial stations) is at
y 0.0–0.4 on the real floor (surface 258), i.e. inside the big box; and in
DownTown, a surfaceless box (X 259–333, Y 48.9–57.7, Z −152…−71) encloses
the Kube Tower landing zone (`Z_DT_CubeTower`, y 55.9) and four authored
challenge start points.

Before the spawn change in doc 4, SkateSchool's spawn (y 7.63) stood on top
of the big box, which hid the problem.

## Survey of all districts

Classified per mesh by surface-ID presence:

| District | Fully surfaceless meshes | Notes |
|---|---|---|
| SkateSchool | 7 (all 12-triangle boxes) | global stream |
| MaloofMoneyCup | 2 (12-triangle boxes) | global stream |
| DownTown | 2 (12-triangle boxes) | city cell streams |
| MegaPark | 1 (12-triangle box) | global stream |
| others | 0 | |

**Mixed** meshes (some units with surface IDs, some without) are common in
real geometry: University cells (up to 31k triangles), Industrial, DownTown
(138 meshes), BlackBoxPark, and the main geometry of several parks. A
per-triangle filter would therefore remove real floors; the rule must be per
mesh.

## Change

`crates/skate-data/src/retail_collision.rs`:

- `RetailTriangle` gains `has_surface` (unit flag `0x80`).
- `mesh_clusters` decodes and validates a whole mesh (including the triangle
  count check) before visiting its clusters, and skips the mesh when no unit
  has a surface ID. `visit_clusters` returns the number of triangles visited.
  Memory: one decoded mesh at a time instead of one cluster.

The fix is in the game, so existing installations benefit without an asset
refresh.

## Verification

- New test `skips_meshes_without_any_surface_ids` (fixture mesh with its
  surface ID stripped is skipped; the others are visited intact). All
  `skate-data` library and integration tests pass, as do the game's
  collision/world tests.
- Collision triangles loaded per map (`SKATE_RWCM_READY`), before → after:
  SkateSchool 91,790 → 91,706 (−84 = 7 boxes), DownTown −24, MaloofMoneyCup
  −24, MegaPark −12, all other maps unchanged. Every map passes
  `--check-assets`.
- In play: SkateSchool (spawn area free to skate) and DownTown tested
  thoroughly; the Maloof and MegaPark box locations (below) were visited
  afterwards and are also clear.

Further evidence the volumes are triggers: MegaPark's only box
(`tele_stadium_to_world_volume_a`, X 61.4–73.2, Y −4.1…4.1,
Z −140.8…−132.6) surrounds the stadium's arrival
point from the world (`tele_world_to_stadium_dest_locator_01`, 2 m from its
centre), and one of Maloof's two boxes lies 14 m from its arrival point from
DownTown (`tele_dwtn_to_mmcp_dest_locator_01`). The other Maloof box
(2.8 × 3.4 × 2.0 m) is 17 m from a street challenge start.

## Notes for upstream

- `cargo test -p skate-data` currently fails to compile three examples
  (`apt_data`, `hud_data`, `scoring_flow_data`) that reference game-only
  modules; this predates this change. `--lib --tests` runs the test suite.
- The mesh-level rule is empirical (all 12 such meshes in the shipped data are
  boxes, and authored player positions lie inside some). If retail code that
  consumes these volumes (zones/triggers) is ported later, they should be
  routed there instead of dropped.

## What the boxes are (research, 2026-10-02)

Every surfaceless box is the shape of a named **trigger volume**: a volume-set
record (type `0x00EB0019`: matrix, bounding box, link, instance id, name) in the
same stream, whose bounding box matches the box to the millimetre. Retail
registers them as triggers when the stream loads and posts enter/exit events;
they are never solid. All 12 boxes:

| District | Volumes |
|---|---|
| SkateSchool | `coach_frank_sksc`, `ws_sksc_coachfrank_instance_01` (same box, 3 meshes), `tut_sksc_inthehub_vol_01`, `tut_sksc_inthehub_vol_02`, `tut_sksc_reset_vol01`, `tut_sksc_wrongway_vol_01` |
| MegaPark | `tele_stadium_to_world_volume_a` |
| MaloofMoneyCup | `tele_mega_ramp_up_volume_01` (X −9.3…−6.5, Z 54.5…56.5), `tele_mmcp_to_dwtn_volume_01` (X 36.2…42.1, Z 12.8…26.6) |
| DownTown | `dwtn_sessionspot_01_kubetower_volume_01` (X 259–333), `dwtn_sessionspot_02_spillway_volume_02` (X 66.2…105.9, Y 50.2…59.0, Z −244.7…−212.2) |

The `tele_*` volumes are where retail offers a teleport (a yes/no challenge
prompt). Exporting them as data the game can use (a "which volumes contain this
point" query) is planned follow-up work; this change only stops them being
solid. Research source: the TU3 code via the skate3recomp static recompilation
(reference only).

## Correction (2026-10-02)

An earlier version of this document gave the SkateSchool spawn-wall box as
X −66…−62. That came from an analysis script using the vendored Python
collision decoder, which reads the 16-bit compressed vertex deltas as
**signed**; vertices in clusters spanning more than 32.8 m then wrap by
65.536 m. The game's Rust decoder (`retail_collision.rs`) reads them unsigned,
as retail does, so **the game and this change were never affected**: the
triangle counts and play tests above are unchanged. Re-run with unsigned
deltas, every box now matches its retail trigger-volume record exactly. The
real box is X −62.1…−0.5 (`tut_sksc_inthehub_vol_01`).
