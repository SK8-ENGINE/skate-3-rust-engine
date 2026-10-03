# 8. Setup performance: duplicate stream copies and customiser overlap (PR G)

Measured on a full setup (Windows 11, i7-14700KF, 28 threads, 3 map workers):
466 s total before this change — stock data 20 s, maps 183 s (DownTown alone
182 s), character customiser 226 s (run only after all maps), publish 24 s —
with total CPU use around 10%.

## 1. Skip decoding identical copies of streamed assets

### Problem

City districts are stored as a grid of streaming cells (`cPres_*`, `cTex_*`,
`cSim_*` `.xsf` files). The same asset (texture, model) is copied into every
cell that uses it. `skate3_streams.load_district_stream` called `read_sfil` on
every cell file, which **fully decoded every copy** (RefPack), and only then
compared it with the first copy and discarded it.

| District | Copies / unique (Pres) | Copies / unique (Tex) | RefPack data decoded / unique |
|---|---|---|---|
| DownTown | 13,991 / 3,627 | 10,510 / 985 | 1,548 MB / 347 MB |
| University | 7,736 / 2,665 | 6,427 / 689 | 1,332 MB / 285 MB |
| Industrial | 6,915 / 2,163 | 5,088 / 578 | 806 MB / 188 MB |

In source checkouts the native RefPack DLL is not built (only the release
script builds `target/native/refpack.dll`), so decoding runs in pure Python.

### Change

`tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py`:
`read_sfil` takes an optional `known_copies` (asset id → BLAKE2b digest of the
first stored copy: asset header + stored payload). In a district load, a later
copy whose stored bytes have the same digest is not decoded and not returned.
A copy whose stored bytes differ is decoded and returned as before, so the
existing conflict check (`conflicting SFIL copies`) still sees it. Record
lookup, in-file duplicate detection and stride validation still run for every
copy. Without `known_copies` (all other callers) behaviour is unchanged.

Why identical: identical stored bytes (same section table, sizes and
compressed payload) decode to identical data, and the loader already kept the
first copy; the returned list is still built in ATOC order from the first
copies.

### Verification

- All 10 districts × Pres/Sim/Tex loaded with the original and the new module:
  every asset's record fields, source path, source offset, stored size and
  decoded bytes are identical.

  | District | Stream load before | After |
  |---|---|---|
  | DownTown | 107.8 s | 24.8 s |
  | University | 83.4 s | 19.1 s |
  | Industrial | 49.3 s | 12.9 s |
  | all 10 | 247.9 s | 64.1 s |

- Unit tests (`test_streams.py`): identical copies from another cell are
  skipped, differing copies are still decoded and returned, in-file duplicates
  are still rejected. (The fixture uses the uncompressed storage path; the
  compressed path is covered by the full-district comparison above.)

## 2. Run the character customiser beside the map stage

### Problem

`customiser_setup.install` passes the customiser to `install._install` as
`finalize`, which ran after the whole map stage. It reads only stock data
(core group), default-skater textures (character group) and the disc — all
prepared before maps — and writes only `assets/private/customisation`. It
launches no processes (so it cannot race `spawn()`'s process-wide DLL setting).

### Change

`tools/asset_pipeline/install.py`: when the map stage runs and memory allows
(`overlap_customiser`: available memory ≥ 2 GiB desktop reserve + 3 GiB per
map worker + one more 3 GiB slot), the customiser starts on a background thread
(`Background`) just before the map jobs and is joined where it used to run;
its exception is re-raised there, so failures behave as before. Otherwise it
runs at the end exactly as before. `map_workers` and the new gate share
`available_memory()`.

### Verification

- Unit tests: `Background.join` waits and re-raises; the memory gate needs one
  slot more than the map workers.
- Full setup: see below.

## 3. Faster clothing library and pro roster

### Problem

After section 2 the customiser was the critical path. Its two big stages were
single-threaded loops: the clothing library (126.5 s alone) decodes ~2,900
textures one at a time, and the pro roster (89.6 s) prepares 41 characters
one at a time. In a dev checkout both also used the pure-Python RefPack
decoder, because only `Build-Release.ps1` built `target/native/refpack.dll`.

### Change

- `scripts/Build.ps1`: after the cargo build, compiles
  `tools/asset_pipeline/refpack_native.rs` into `target/native/refpack.dll`
  with the same rustc flags as the release script, when the DLL is missing or
  older than its source. `fast_refpack.py` already prefers it.
- New `tools/asset_pipeline/customisation_workers.py`: worker entry
  (`--warm-textures`, `--roster`) plus `run_parallel`, which runs workers through
  `install.run(task(...))` so frozen builds re-enter correctly. The worker count
  is `customiser_workers()`: a quarter of the logical CPUs, from 1 to 8. If a
  worker fails, `run_parallel` prints the tail of that worker's log.
- Library (`customisation_library.py`): `requested_textures(catalog)` lists
  exactly the textures `prepare()` will decode, using the same skip rules. Before
  the serial loop, `warm_up()` decodes them into the `decoded/` cache, spread
  across worker processes in disjoint chunks. The loop itself and its
  first-writer-wins ordering are unchanged; it now finds the decoded files
  already present. Decoded files that the loop doesn't use, and source files
  the warm-up created for them, are deleted before `prune_unavailable`. That
  leaves the cache exactly as a serial run would.
- Roster (`native_roster.py`): `prepare_parallel()` splits the characters
  round-robin over worker processes (`prepare(only=..., report_path=...)`).
  Each character writes to its own outputs (`library/entries/<id>`,
  `work/<key>`). The reports are merged back into roster order and
  `work/report.json` is written as before. It falls back to a serial
  `prepare()` with one worker, or if any worker fails or a report is missing.
- Shared roster caches (`work/source`, `work/decoded`) are published by
  `_publish()`: write a pid-unique temporary file, then rename it into place
  without overwriting. On Windows, renaming over a file that another process
  is opening fails on one side: WinError 5 for the renamer, or Errno 13 for the
  reader's `open()`. The first version used `os.replace`, which overwrites. In
  the first real run, 4 of 7 workers died this way and the stage quietly fell
  back to serial. Now `os.rename` is used (it doesn't overwrite on Windows). An
  existing destination, or `FileExistsError`, means another worker already
  published identical bytes, so that file is kept. A `PermissionError` is
  retried briefly.

### Verification

- RefPack: the native and Python decoders give identical bytes for every
  compressed entry in `createacharacter.big` (4,229 entries; 34.0 s → 1.4 s)
  and `marquee.big` (11.2 s → 0.5 s), and for every district stream
  (DownTown 23.9 s → 1.8 s).
- Library warm-up, 7 workers: all 2,921 requested textures decoded in 25.6 s,
  byte-identical to the installed `decoded/` cache.
- Roster, 7 workers, no fallback: 18.5 s. All 41 characters are ready and each
  entry's `character.glb`, `recipe.json`, `manifest.json` and `preview.png` is
  byte-identical to the installed set. The report equals the installed one and
  no temporary files are left behind.
- Unit tests (`test_customisation_workers.py`):
  - merge order;
  - fallback to serial on a failed worker;
  - workers never exceed the number of characters;
  - `_publish` is atomic and keeps an existing file;
  - rename contention is tolerated and retried;
  - `requested_textures` follows the skip rules.
- Refresh setup (customiser only, rebuilt once):
  - library 126.5 → **51.2 s**, roster 89.6 → **15.9 s**;
  - customiser stage total 73 s, with no worker failures;
  - `check_customiser.py`: IDENTICAL, all 8,288 receipted files;
  - `check_maps.py` and `check_spawns.py`: all OK, unchanged.

### Cost

The customiser fingerprint covers `customis*.py` and `native_roster.py`, so an
existing install rebuilds its customiser once after this change (about 75 s on
this machine). The outputs are identical, so nothing the user can see changes.
There is no equivalence mechanism for the customiser fingerprint, and the
rebuild is cheap, so none was added.

## Fingerprints

The stream change touches a file in the shared parser set, changing the
`character`, `environment` and `maps` fingerprints without changing outputs
(proven above); old→new pairs are in `pipeline-equivalence.json`. The
customiser fingerprint and stage versions are unchanged. An installation built
from the committed code rebuilds nothing.

## Full setup result

Full fresh setup, same machine and source, before → after:

| Stage | Before (466 s total) | After (331 s total) |
|---|---|---|
| Stock data, HUD, skater, environment | 0–20 s | 0–19 s |
| Map stage (10 maps, 3 workers, validated) | 21–204 s (183 s) | 19–138 s (**119 s**) |
| Character customiser | 216–442 s (226 s, after maps) | 19–311 s (292 s, beside maps) |
| Publish | 442–466 s | 311–331 s |

**Total: 466 s → 331 s (−29%), 0 warnings.**

- Identical outputs: all 8,288 receipted customiser files match a snapshot of
  the previous (sequential) installation, with only the random set id
  differing in 3 JSON paths; all maps pass validation with unchanged
  collision counts and no warnings; all baked spawns unchanged.
- Contention: overlapped, the customiser's library stage took 193.9 s instead
  of 126.5 s alone (three map workers compete for disk and memory bandwidth,
  boost clocks drop, and the customiser thread shares the GIL with the setup
  main thread). It is now the critical path; the roster (89.6 s) and library
  are single-threaded loops and the next candidates.

## Files

- `tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py`
- `tools/asset_pipeline/install.py` (`available_memory`, `overlap_customiser`,
  `Background`, map stage), `pipeline-equivalence.json`
- `tools/asset_pipeline/test_streams.py`, `test_map_validator.py`
- Section 3: `scripts/Build.ps1`, `tools/asset_pipeline/customisation_workers.py` (new),
  `customisation_library.py`, `native_roster.py`, `customiser_setup.py`,
  `test_customisation_workers.py` (new), `test_customiser_setup.py`
