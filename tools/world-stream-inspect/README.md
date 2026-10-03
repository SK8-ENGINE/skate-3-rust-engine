# World stream inspection

Read the world districts' simulation streams (`cSim_*.xsf`, RW4 arenas) straight from your own
disc's `.big` archives, without extracting anything to disk.

| Script | What it does |
|---|---|
| `big_list.py` | Lists the entries of any `.big` archive (path and unpacked size), filtered by a regex, or extracts the matching entries with `--out`. |
| `sim_types.py` | Per stream file of a district: the arena dictionary's entry types and counts. `--find HEX64` searches every arena for a 64-bit value (both word orders) and names the entry that holds it; `--dump TYPE` hex-dumps the entries of one type. |
| `volumes_dump.py` | Dumps every named volume (arena type `0x00EB0019`, trigger / zone / teleport boxes): name, instance id, link id, bounds, base corners, plus each arena's collision meshes and other entry types. Writes JSON too. |
| `arenas.py` | Shared helper: walks the arenas of a district (or any `.big` with `*_Sim.xst` + `cSim_*.xsf`, such as `missions.big`) and parses the arena dictionary. |

The volume layout is documented at the top of `volumes_dump.py`.

## Inputs

- Your extracted disc: `--disc DIR` (the folder holding `data/`), default `$SKATE3_DISC` or `.local/skate3-disc`.
- District ids with the prefix: `DIST_SkateSchool`, `DIST_MegaPark`, … (`volumes_dump.py` defaults to all ten).

## Usage

```
py -3.13 tools/world-stream-inspect/big_list.py .local/skate3-disc/data/big/db.big skaterschema
py -3.13 tools/world-stream-inspect/big_list.py .local/skate3-disc/data/audio/audiofiles.big "\.bnk$" --out .local/bnk
py -3.13 tools/world-stream-inspect/sim_types.py DIST_SkateSchool
py -3.13 tools/world-stream-inspect/sim_types.py DIST_SkateSchool --dump 00eb0019
py -3.13 tools/world-stream-inspect/volumes_dump.py DIST_SkateSchool > volumes.txt
```

## Example output

`sim_types.py` (made-up ids and counts):

```
cSim_0_0_high.xsf 0123456789abcdef 00080001:1 00080006:3 00eb000a:1
TOTAL 00080001:40 00080006:112 00eb000a:40 00eb0019:2
```

## Requirements

Python 3.13 (standard library); the repository's `tools/owned_game` and the vendored map extraction
tools (`tools/vendor/university/.../skate3_streams.py`, `retail_collision_mesh.py`).
