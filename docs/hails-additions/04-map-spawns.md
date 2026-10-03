# 4. Authored map spawns

Each converted `.skate` map stores one spawn (position + heading). The game
uses it as the map's **startup position** (the saved default map) and as the
menu's **default landing zone**; `teleports.json` destinations are only used
when the player picks one. So the baked spawn must be right for every map.

## Problems observed in play

- **MegaPark:** the skater fell forever. Log: `PhysicsGround->PhysicsAir`
  9 ticks after spawning, never landing; the respawn (`Teleporting`) returned
  to the same point, so it looped.
- **Industrial Skate Park, StartPark:** spawned on the roof; only "a small
  part" of the park was visible.
- **Industrial, DownTown:** spawn "not where the original game puts you".

## Root cause

`tools/asset_pipeline/map_writer.py` (`SpawnSelector`) chose, for every
district except University, the **upward-wound collision triangle nearest the
world origin** and spawned 1 m above its centre. Two properties of the retail
data defeat that:

1. Skate-park origins sit at a corner of the park, not its middle.
2. Retail collision winding is inconsistent: some park floors face down
   (StartPark's and Industrial Skate Park's ground floors at y = 0), so they
   were never candidates.

Measured with the collision extracted from each park:

| Map | Old spawn | What was there |
|---|---|---|
| MegaPark | (21.7, 21.6, −19.7) | 6.3 m² double-sided panel at the park edge; **nothing below it** |
| IndustrialSkatePark | (−1.6, 40.3, 2.0) | roof (interior: floor 0 m, level 15 m, ceiling 39.6 m, roof 43 m) |
| StartPark | (1.5, 33.2, −9.8) | roof |

The retail game does not use geometry at all: it has authored start
locators (below). The converter never read them.

## Intermediate attempt (kept as a fallback)

A geometric `ground_spawn()` was added first: ignore winding, search outward
from the area-weighted centre of flat surfaces, and accept a point that is
the lowest surface at its XZ, has ≥ 2.5 m headroom, and has no flat surface
within 25 m (by triangle extent) more than 1 m lower.

**Regression found in play:** applied to city maps, the search starts from the
area-weighted centre of the whole district. For Industrial (a port) that point
is out in the harbour: the spawn became (−864.7, −3.0, 414.1), on a 1,604 m²
collision-only floor at y −4.0 with nothing above it (likely the harbour bed)
and no visible geometry nearby, so the world looked invisible while collision
held the player up. (An earlier version of this note said "under the streets";
collision analysis at that point showed otherwise.) The streaming map validator
(doc 6) now flags this case as "no visible surface … (invisible collision)".
Fix: only districts whose collision spans ≤ 500 m
(`GROUND_MAX_EXTENT`, i.e. the parks) use `ground_spawn`; larger districts keep
the original rule unchanged. A unit test pins this. Lesson recorded: a spawn
change moves every map's startup position, so all maps must be checked.

`ground_spawn` now only applies if a district has no authored start.

## Final change: authored starts from retail data

New `tools/asset_pipeline/map_starts.py` resolves each district's start from
the game's own data, in priority order:

1. **`world` row start locator** — field `Hash_3735C5C12E8E7AE1` in
   `skatercollections` names the start locator of each skate park
   (`dist_megapark` → `Z_MegaPark_SkatePark`, etc.).
2. **Chosen `fe_locations` row** for districts with several authored
   locations and no single start (`DEFAULT_LOCATIONS`, a project choice —
   see below).
3. **The district's only `fe_locations` row** (BlackBoxPark, SkateSchool).

The locator's matrix comes from the district's own
`data/content/global_locators/**/<District>/*.rx2` EB0009 records (parsed with
the existing `teleports.location_records`; row 3 = position, row 2 = forward).
Missing or ambiguous locators fall back to the geometric rule.

Heading is now written into the `.skate` header (the `f32` after the spawn,
previously always 0): `heading = atan2(forward.x, forward.z)`, matching the
game's `Mat3::from_rotation_y(heading)` (forward = (sin h, 0, cos h)). The
convention was checked against a logged teleport (Aletown: locator forward
(0.1536, 0, 0.9881) = the game's `requested_at`).

`_install` computes the starts once before map conversion and writes
`<work>/map-starts.json`; each `convert_map` job reads its district's entry.

### Resulting defaults

| Map | Locator | Source | Position | Heading |
|---|---|---|---|---|
| BlackBoxPark | `A_BlackBoxPark` | fe_locations `dist_blackboxpark` | (10.55, 0.08, 6.05) | 90° |
| DownTown | `Z_DT_SkatePark` (Rosalita Skate Park) | **choice** → fe `dist_downtown_skatepark` | (−254.05, 40.54, 32.39) | −85.5° |
| DownTownSkatePark | `Z_SP_DT_Start` | world `dist_downtownpark` | (66.05, 0.0, −64.02) | 0° |
| Industrial | `Z_Ind_ReclaimedHastings` (Haystings Park) | **choice** → fe `dist_industrial_reclaimed` | (−89.16, 1.01, −138.35) | −45.4° |
| IndustrialSkatePark | `Z_SP_Ind_SP_Start` | world `dist_industrialpark` | (−64.01, 0.75, 0.0) | 0° |
| MaloofMoneyCup | `A_Maloof_Street` | **choice** → fe `dist_maloof_street` | (0.56, −0.06, −43.42) | 180° |
| MegaPark | `Z_MegaPark_SkatePark` | world `dist_megapark` | (230.63, 0.48, −73.08) | −90° |
| SkateSchool | `tele_world_to_school_dest_locator_01` | fe `dist_skateschool` | (−31.28, 0.11, −31.5) | 0° |
| StartPark | `Z_SP_SkateParkStart2` | world `dist_startpark` | (−64.01, 0.0, 0.0) | 0° |
| University | `ftl_univ_02_challengelocator_01` (career start) | **choice** → fe `gamestart` | (294.64, 74.12, −432.6) | 114.5° |

### Project choices (maintainers may prefer others)

Retail has no single start for the cities (you always arrive at a location
picked in the map menu) or for Maloof (two locations). Chosen:

- **University:** the retail career start (`gamestart`).
- **Industrial:** Haystings Park, the district's skate park.
- **DownTown:** Rosalita Skate Park; retail data also links the DownTown
  skate-park world to this spot.
- **MaloofMoneyCup:** the street course.

Previous University spawn was a fixed point next to Super-Ultra Mega-Park,
(330, 133.005, −710).

## Other findings

- Locator files for the parks live under `global_locators/PARK/`, not `BAM/`.
- `teleports.json` marks MegaPark's only FE destination ("Stadium – Monster
  Park", locator `vert_skp5_01_challengelocator_01`) unavailable: that locator
  does not exist in the shipped content. Unrelated to spawns; left as is.
- The map-conversion work runs in subprocesses (`map_job.py`) capped at 3
  workers; see performance notes elsewhere.

## Files

- `tools/asset_pipeline/map_starts.py` (new)
- `tools/asset_pipeline/install.py` — computes starts before conversion;
  `convert_map` uses them.
- `tools/asset_pipeline/map_writer.py` — `ground_spawn`, `GROUND_*`
  constants, `SpawnSelector` changes, `spawn_point` no longer prunes by
  distance for non-University districts, `write(..., prepared_heading=)`.
- `tools/asset_pipeline/test_map_writer.py`, `tools/asset_pipeline/test_map_starts.py`.

Pipeline fingerprints change for the `maps` and `environment` groups, so an
existing install rebuilds those groups on its next asset refresh.

## Verification

- Unit tests (28 across the setup/map suites) pass, including: roof over a
  down-facing floor, floating panel over the void, city-sized districts
  keeping the original rule, heading convention.
- Collision analysis per park (extract with the collision consumer, then
  query all surfaces at an XZ) confirmed the old failures and the
  ground_spawn results.
- In play (before authored starts): Industrial with the restored city rule
  spawned visible and grounded.
- Authored starts, after an asset refresh (maps + environment groups, 0
  warnings): every map passes `skate3rust --check-assets`, and each baked
  spawn and heading read back from the `.skate` header matches the table
  above exactly.
- In-game confirmation: all 10 authored starts tested in play and correct.
  SkateSchool's start is inside an invisible collision volume that blocks
  movement; that is a separate, pre-existing collision issue (the old spawn
  stood on top of the same volume) and is documented separately.
