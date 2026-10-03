# Collision inspection

Extract a district's collision from your own disc and look at it, without touching the installation.
Used to place map spawns and to find the invisible trigger volumes (see
`docs/hails-additions/04-map-spawns.md` and `05-collision-volumes.md`).

| Script | What it does |
|---|---|
| `map_collision.py` | Extracts `world<District>.big` into `<work>/<District>/raw` (once), runs the map converter's collision pass and saves every triangle to `collision.npy` (N x 3 x 3 float64, Y-up metres). Prints the triangle count, bounds and the spawn the current spawn rule would choose. |
| `analyse_spawn.py` | Lists every collision surface at a point's X/Z (height, normal-up sign, area), plus the area-weighted centre of the flat surfaces. Nothing below a spawn = void; a much lower surface under it = roof or platform. Needs `map_collision.py` first. |
| `collision_attrs.py` | Saves the triangles with their surface id, unit flags (0x80 = has a surface id), mesh flags and mesh index to `collision_attrs.npy`. Needs the extracted district. |
| `scan_surfaceless.py` | For each district: counts collision meshes that are surfaced, mixed or surfaceless (no surface ids at all) and lists the non-surfaced ones with their size and bounds. |
| `unsigned_collision.py` | Imported by the others: makes the vendored collision decoder read 16-bit vertex deltas unsigned, as the game does. Without it, clusters spanning more than 32.8 m wrap by 65.536 m. |

## Inputs

- Your extracted disc: `--disc DIR` (the folder holding `data/`), default `$SKATE3_DISC` or `.local/skate3-disc`.
- District names without the `DIST_` prefix: `MegaPark`, `StartPark`, `BlackBoxPark`, `SkateSchool`,
  `IndustrialSkatePark`, `DownTownSkatePark`, `MaloofMoneyCup`, `University`, `Industrial`, `DownTown`.
- Work folder: `--work DIR`, default `.local/collision` (gitignored).

## Usage

```
py -3.13 tools/collision-inspect/map_collision.py MegaPark
py -3.13 tools/collision-inspect/analyse_spawn.py MegaPark 21.7 21.6 -19.7
py -3.13 tools/collision-inspect/collision_attrs.py SkateSchool
py -3.13 tools/collision-inspect/scan_surfaceless.py SkateSchool StartPark
```

Parks take about a second; the city districts take minutes and a lot of memory.

## Example output

(made-up values)

```
triangles 12000
bounds min [-120.0 -5.2 -130.0] max [140.0 40.1 125.0]
spawn (current rule) [10.5 0.1 6.0]

MapA surfaces at (21.7, -19.7) : [(0.0, 1.0, 12.5), (6.3, 1.0, 3.1)]
```

## Requirements

Python 3.13 with `numpy`; the repository's setup modules (`tools/asset_pipeline`) and the vendored
map extraction tools (`tools/vendor/university`, `tools/vendor/utt`).
