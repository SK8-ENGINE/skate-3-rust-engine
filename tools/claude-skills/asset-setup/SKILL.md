---
name: asset-setup
description: Convert a Skate 3 Xbox 360 ISO or extracted disc into game assets for a dev checkout, repair missing pro skaters, or re-point the assets junction. Use when assets are missing, setup failed, characters are unavailable, or the game says the assets folder is missing.
---

# Asset setup for a dev checkout

You need your own legally owned copy of Skate 3 (Xbox 360 ISO, or an extracted disc folder with `default.xex`).
Converted assets are never committed. Keep where your ISO, extracted disc and current installation id live in
your own notes file.

## How it fits together
- `PLAY.bat` -> `scripts/Launch.ps1` expects converted assets at repo-root `assets\` (a junction to
  `data\installations\<id>\assets`).
- `tools/setup.py` is the release GUI; the work is
  `tools.asset_pipeline.customiser_setup.install(source, base, game_exe, report, refresh=False)`:
  - `source`: an `.iso`, or `default.xex` inside an extracted disc folder.
  - `base`: `<repo>\data` (gitignored). Output: `data\installations\<32-hex id>\`, published via
    `data\installation.json`.
  - `game_exe`: `<repo>\bin\skate3rust.exe`, which must be built first (skill `build-and-run`); setup runs it with
    `--check-assets` to validate.

## Headless run (no GUI)
```python
import sys; from pathlib import Path
ROOT = Path(r'<repo>'); sys.path.insert(0, str(ROOT))
from tools.asset_pipeline.customiser_setup import install
from tools.asset_pipeline.optional_content import summary
stage = install(Path(r'<extracted disc>\default.xex'), ROOT/'data', ROOT/'bin'/'skate3rust.exe', lambda t: print(t, flush=True))
print(stage, summary(stage))
```
Run with `py -3.13 -u` in the background; it takes several minutes. Use raw strings for Windows paths in Python
(a non-raw `"\a..."` or `"\x86"` silently becomes a control character). Afterwards:
```powershell
New-Item -ItemType Junction -Path assets -Target "<repo>\data\installations\<id>\assets"   # remove the old junction first if the id changed
bin\skate3rust.exe --assets assets --test-world --check-assets                             # expect SKATE_ASSETS_READY
```

## Refresh after a converter change
- `install(<default.xex>, data, bin\skate3rust.exe, report, refresh=True)` rebuilds only groups whose pipeline
  fingerprint changed (`versions.changed_groups`), plus the customiser if its fingerprint changed. Check beforehand:
  `changed_groups(installed(data)[1]['pipelines'], fingerprints())`.
- Close the game first: refresh publishes a NEW `data\installations\<id>` and deletes the old one. Afterwards
  re-point the junction (`rmdir assets`, then `New-Item -ItemType Junction ...` to the new id) and run
  `--check-assets`.
- Then run skill `regression-check` section 1 (all maps' spawns vs baseline, `--check-assets` per map, 0 warnings).

## Diagnosing a bad spawn (void / roof / invisible map)
- Spawn rule: maps spawn at their authored start locators; on older trees University used a fixed XZ, compact
  districts (collision XZ span <= 500 m) used `ground_spawn` and cities the nearest-origin rule. "Invisible map with
  working collision" = spawned under the map.
- Log: `SKATE_MAP_LOADED ... spawn=[x,y,z]`, then `REPORT_TRANSITION physics_tick:9
  requested:PhysicsGround->PhysicsAir` with no landing = falling from spawn.
- Every map's startup position and menu "default landing zone" is the baked spawn; `teleports.json` destinations are
  only used when picked from the menu.
- Inspect the collision with the collision tools (`tools/collision-inspect/`, in the repo since upstream PR #37
  merged). They write to gitignored
  `.local\collision\<District>\`:
  ```bash
  py -3.13 tools/collision-inspect/map_collision.py MegaPark            # extract, print bounds + the rule's spawn
  py -3.13 tools/collision-inspect/analyse_spawn.py MegaPark x y z       # surfaces at that XZ + flat-area centre
  ```
  Nothing below the spawn height = void; a much lower surface under it = roof/platform; spawn below the street =
  under the map.

## Setup behaviour to expect (newer branches)
- The environment group also extracts the water/ocean animation table (`ocean_pca.py` -> game
  `--extract-ocean-pca`, needs the current `bin\skate3rust.exe`) and the particle sprites (`particles.py` ->
  `assets/private/particles/*.png`). A change to either refreshes only `environment`; afterwards re-point the
  `assets` junction to the new installation.
- One `--validate-maps` process validates stock data, each map as it finishes converting, recovered maps and the
  final test world (fallback: `--check-assets` per map). Warnings land in `map-status/<map>-validation.json` and
  `setup-report.json` via `validation_report.summary`.
- The character customiser runs on a background thread beside the map stage when memory allows
  (`install.overlap_customiser`); log line "Preparing the character customiser alongside the maps".
- Group `audio` (upstream main since PR #32): downloads vgmstream (SHA-256 checked) into `data\tools\vgmstream-cli\` and decodes
  the disc's audio into `assets/private/audio/` (hundreds of MB). Optional: on failure it writes
  `audio-availability.json` and the game runs silent. Dev re-export of just this group: skill `audio-tuning`.
- City stream loads skip identical duplicate copies (`skate3_streams.read_sfil(known_copies=...)`); prove a
  stream-loader change with `tools/setup-equivalence/compare_streams.py` (in the repo since PR #37).

## Diagnosing invisible walls / invisible floors
- Symptom: blocked or standing on nothing visible; logs show normal states (no `physics_failed`).
- `tools/collision-inspect/collision_attrs.py <District>` then query the spawn/area: a surface with surface id 0 and
  unit flags `0x21` (no 0x80) on a 12-triangle box from `cSim_Global.xsf` is a zone/trigger volume, not geometry.
- `tools/collision-inspect/scan_surfaceless.py` summarises every district. Only fully surfaceless meshes are
  volumes; "mixed" meshes are real geometry: never filter per triangle.

## Known failures
- "Conversion failed. See ...extract.log" right after "Extracting your ISO": the extract-xiso argument-order bug
  (options must precede the image). Otherwise extract once manually with
  `extract-xiso -x -d <existing-parent>\disc <iso>` and pass `default.xex`.
- Optional components listed as unavailable: `summary(stage)` returns them; details in `setup-report.json` (only
  exists when there are warnings).
- Pro characters failing with `[Errno 2] No such file or directory` on a long path: MAX_PATH. Enable long paths
  (HKLM `...\Control\FileSystem\LongPathsEnabled=1`, admin), then repair in place (next section).

## Repair missing roster characters without a full re-run
In the current set `assets\private\customisation\sets\<set id from current.json>\`:
1. `native_roster.prepare(game, assets, set/'native-roster', set/'database/collections.json', set/'roster-work',
   only=[keys])`, keys from `native-roster/complete.json` `unavailable`.
2. Remove fixed keys from `unavailable`, add them to `characters` in `complete.json`.
3. Re-record the stage receipt exactly like `customiser_cache.stage()`: `receipt(set, all files under
   OUTPUTS['roster'])` into `roster-complete.json`, keeping its `version`/`source`. Otherwise a later refresh sees the
   stage as damaged.
4. Check `customiser_cache.complete(set)` is True, `summary(install)` is empty, and `--check-assets` passes. Back up
   the two JSON files first.

## Refreshing one group (2026-10-25)
`installed`, `changed_groups` and `fingerprints` live in `tools/asset_pipeline/versions.py`. To rebuild a group whose
fingerprint already matches (after an exporter change you want to test), back up `data/installation.json`, set
`pipelines.<group>` to a dummy value and run `install(..., refresh=True)`. A livingworld-only refresh also writes the
ped hand props (they live in `private/living_world`); the log says `Hand props: N of M exported`.
