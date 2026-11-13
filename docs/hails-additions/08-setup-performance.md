# 8. Setup performance: duplicate stream copies and customizer overlap (PR G)

Measured on a full setup (Windows 11, i7-14700KF, 28 threads, 3 map workers):
466 s total before this change: stock data 20 s, maps 183 s (DownTown alone
182 s), character customizer 226 s (run only after all maps), publish 24 s;
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
copy. Without `known_copies` (all other callers) behavior is unchanged.

Why identical: identical stored bytes (same section table, sizes and
compressed payload) decode to identical data, and the loader already kept the
first copy; the returned list is still built in ATOC order (the stream's index table) from the first
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

## 2. Run the character customizer beside the map stage

### Problem

`customiser_setup.install` passes the customizer to `install._install` as
`finalize`, which ran after the whole map stage. It reads only stock data
(core group), default-skater textures (character group) and the disc, all
prepared before maps, and writes only `assets/private/customisation`. It
launched no processes then (it has since gained worker processes and, from the third pass of
"Export speed pass", runs as its own process; `spawn()` now holds a lock around the DLL setting).

### Change

`tools/asset_pipeline/install.py`: when the map stage runs and memory allows
(`overlap_customiser`: available memory ≥ 2 GiB desktop reserve + 3 GiB per
map worker + one more 3 GiB slot), the customizer starts on a background thread
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

After section 2 the customizer was the critical path. Its two big stages were
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
- Refresh setup (customizer only, rebuilt once):
  - library 126.5 → **51.2 s**, roster 89.6 → **15.9 s**;
  - customizer stage total 73 s, with no worker failures;
  - `check_customiser.py`: IDENTICAL, all 8,288 receipted files;
  - `check_maps.py` and `check_spawns.py`: all OK, unchanged.

### Cost

The customizer fingerprint covers `customis*.py` and `native_roster.py`, so an
existing install rebuilds its customizer once after this change (about 75 s on
this machine). The outputs are identical, so nothing the user can see changes.
There is no equivalence mechanism for the customizer fingerprint, and the
rebuild is cheap, so none was added.

## Export speed pass (2026-10-24, USER-ASKS 305)

The user asked for an optimization pass over the asset extraction scripts: faster, without
introducing any new issues into the export process.

**Measured first.** A full fresh setup of the disc (no DLC) into a scratch base on K:, with the native RefPack DLL and
the dev game exe the launcher's refresh uses, every progress line timestamped (`tools/dev-bench/setup_bench.py`: also
a sha256 manifest of every installed file). Baseline 228.3 s. The stages before the maps ran one after another: the
HUD and menu movies 31 s, audio 59 s (of it the bank decode loop 36 s, the ambience beds 10.5 s, the region read
11 s), the living world 28 s; then maps beside the customizer 61 s, park layouts 17 s, the final validation 18 s.
Inside the menu movie export (cProfile): DXT decoding in pure Python 13 s, a pure-Python RefPack 9 s (the vendored UI
tools' copy, not the setup's native one), PNG zlib 5 s.

**Changes:**
- Audio: the sound banks and the ambience beds decode on `setup_budget.decode_workers()` threads (vgmstream processes;
  `SKATE_SETUP_DECODE_WORKERS`, default cpu/2, 2..6); each writes only its own files and the manifest is filled in the
  original order (`audio_export._in_order`). The beds' NumPy downmix stays one at a time (`_NUMPY` lock): with several
  threads in NumPy's BLAS at once Python crashed (access violation) in a first attempt.
- Menu movies: the vendored RefPack uses the setup's native decoder when it is built (`vendor/skate3_ui/refpack.py`,
  as `owned_game/refpack.py` already did); the DXT decoder works on all blocks at once in NumPy with the same integer
  arithmetic (`xenos._decode_dxt`; the old one is `_decode_dxt_reference`, test `test_menu_texture_decode`: 480 random
  cases over the three formats, odd sizes, three-color and six-step-alpha blocks, byte-identical).
- The HUD / menu movie stage (its own process; reads only the disc and the stock collections, writes only its folders)
  runs beside character, environment, audio and the living world, joined before validation (`install.py` `hud_job`).
  In-process stages stay one at a time (the audio overlapping the living world is what crashed in NumPy).
- `install.spawn` holds a lock: the frozen build's `SetDllDirectoryW` switch around a child start is process-wide.

**Result:** 228.3 s to 189.5 s (-17%); everything before the maps 131 s to 75 s. The maps and the customizer
(beside them) are now the critical path.

**Identity:** the new export writes the same 18,868 files. Every file that differs from the baseline also differs
between two runs of the unchanged pipeline (a second baseline): 40 files that are not reproducible run to run
(map `.skate` files, park layouts and props, the customizer set id in paths, a few HUD / menu manifests that hash
their temporary sources). The other 18,828 are byte-identical, including all audio and every menu movie texture.

**Second pass (2026-10-24, USER-ASKS 308 / 309):**
- **One core budget** (the user asked for 75% of the cores, leaving headroom for the user): `setup_budget.cores()` = 75% of
  the CPUs the process may run on (`os.process_cpu_count`), `SKATE_SETUP_CORES` overrides. Every pool derives from it:
  map jobs up to 3 (half the budget, memory permitting), the customizer's worker processes a quarter (1..8), the
  threads inside a map job what is left over 3 jobs, audio decodes a quarter (they run beside the maps), archive
  extraction and receipt hashing up to 8 / 16. On the 28-CPU desktop: budget 21, customizer 5, 5 threads per map job.
- **The map jobs start right after the core stage** (with the movable-object catalog and the map starts): they read
  only the disc, the starts and the catalog and write only their own files, so they run beside the HUD, environment,
  audio and living world; each map is validated once the validator runs, as before. A failure in a later stage cancels
  the queued jobs and waits for the running ones. The hand props (they need the living world's tables) follow the
  living world as before; a hand prop failure no longer deletes the catalog under the running map jobs.
- **The living world runs in its own process** (`stage_job.py`, the same stage function) beside the audio; its
  progress lines reach the setup window, its log lines the setup log.
- **Archive extraction** (`BigArchive.extract_entries`) checks every entry first, in order, then reads, decompresses
  (native RefPack) and writes on threads; **receipt hashing** (`group_receipts.record`, 19 GB) runs on threads
  (20.7 s of the old run's end).
- **Result:** 189.5 s to 135.3 s (from the first baseline: 228.3 s, -41%). The maps now finish at 72 s; the customizer
  (library 57 s, roster 20 s; it starts once the validator runs, at 50 s) is the critical path, ending at 131 s.
- **Identity:** the same 18,868 files; the differing ones are the run-to-run set (maps, park layouts, props, the
  customizer set id, HUD / menu manifests) plus the customizer's `*-complete.json` records, whose `version` is the
  customizer's pipeline fingerprint (it covers `customisation_workers.py`, changed here) and `timings.json`.

**Third pass (2026-10-24, USER-ASKS 312):**
- **The customizer starts right after the character stage**, as its own process (`stage_job.py customiser`, the same
  `customiser_setup.prepare`), so its NumPy work never shares a process with the in-process environment and audio
  stages. It needs one more memory slot than beside the maps alone (`overlap_customiser(workers+1)`); without it the
  customizer starts beside the maps or after them, as before. A failure in a later stage waits for the customizer
  process before the stage is cleaned up. `customiser_setup.py` itself is unchanged (every `customis*.py` is in the
  customizer's fingerprint, so editing it would make every install redo the customizer): `install.py` recognizes its
  finalize by module.
- **Result:** 125.2 s / 125.4 s (two runs of the unchanged pipeline, today's machine state) to 94.3 s (-25%). The
  customizer now runs 6.5 to about 88 s (library 51.4 s, roster 19.6 s, as before) and ends before the final validation;
  the stages before the maps take 4 s longer beside it (46.3 to 50.7 s).
- **Identity:** the same 18,868 files; the 41 files that differ from the first baseline are exactly the 41 that differ
  between the two baselines.

**Fourth pass (2026-10-24, USER-ASKS 314):**
- **The post-map exports run beside the map wait** (`install.py` `post_maps`): map names, teleporters, visual
  indicators and park layouts read only the disc, the collections and the packs (whose map BIGs `dlc.load` copied
  whole before the map jobs started; `extract_bigs` keeps a file whose size matches) and write only their own files.
  They run on a thread while the main thread waits on the map jobs and the validator (no NumPy there), joined before
  the map catalog takes their names and before the kept DLC maps are carried over. Only when the customizer runs as
  its own process: park layouts use NumPy, so beside the in-process customizer fallback they stay after the maps.
- **Result:** two runs of the unchanged pipeline 102.6 s / 107.4 s, the change 97.9 s / 96.1 s (about -8 s, -7.6%).
  The post-map exports now run 64 to 80 s, inside the map phase; the maps end later beside them (77 to 84 s) and the
  customizer's roster (it ends at 92 to 93 s) is the critical path again.
- **Identity:** the same 18,868 files; the 41 files that differ from the first baseline are exactly the 41 that differ
  between the two baselines. No DLC on the bench machine, so the pack paths were checked by reading only.

**Open:** the run-to-run differences are pre-existing (not caused here); made reproducible 2026-10-29 (below). The customizer is the critical path (library 53 to 58 s, roster 20 to 24 s with 5
worker processes beside the maps); when the in-process customizer fallback runs (low memory) it shares the process with
the post-map exports' NumPy as before this pass. `customisation_workers.customiser_workers`'s docstring still says
"a quarter" (the budget is in `setup_budget.py`); it is fixed with the next change that touches the customizer, since
editing any `customis*.py` changes the customizer's fingerprint.

**Customizer worker count (2026-10-24, USER-ASKS 315, not landed):** the library stage is a texture warm-up on worker
processes, then one serial loop converting the 480 models; the roster splits over the workers. With
`SKATE_SETUP_CUSTOMISER_WORKERS` on the unchanged pipeline (28-CPU desktop, default 5): 5 workers 84.7 / 86.3 s, 8
workers 77.6 s, 10 workers 79.4 s. A default of 40% of the budget (8 here) then measured 86.9 / 81.1 s against 83.7 /
89.3 s for the unchanged pipeline: within the noise, so it was not landed (output identical in both runs: the 41
differing files are the run-to-run set). The laptop result is below ("Customizer worker share on two machines"); the library's serial model loop went to
the workers next (it changes the customizer fingerprint, so every install redoes the customizer once).

**Customizer library over the workers (2026-10-24, USER-ASKS 322):**
- **Where the time went** (timing probes in a bench copy only): the GLB conversion of the 480 models was about 12 s
  serial, but the bigger serial part after the texture warm-up was the material loop's derived textures (normal maps,
  roughness maps, diffuses with the alpha mask multiplied in): 10.8 s. The texture decode on the workers (2,921
  textures) is about 20 s, and about 10 s comes before the library loop.
- **Change** (`customisation_library.py`, `customisation_workers.py`): the model conversion is one `ModelConverter`
  used by `prepare()` and by a new `--warm-models` worker job, which shares the warm-up pool with `--warm-textures`
  (models first: 5 workers convert all 480 in 2.5 s). Then `--derive-textures` builds the derived textures the material
  loop will ask for (`derived_jobs` follows that loop's rules: first use of a material wins, Hair keeps a separate
  alpha, models without a GLB are skipped). `prepare()` still converts or derives whatever a worker left or failed
  (and records the error as before), and deletes any derived file its loop did not ask for, so a failure in a worker
  never changes the output. Workers publish through a temporary file and a rename. With one worker (low memory) nothing
  changes.
- **Result** (28-CPU desktop, 5 customizer workers): library 50.8 / 49.4 / 53.1 s -> 36.5 / 37.4 s (-27%); the roster
  now ends at 66 to 68 s instead of 77 to 82 s; whole setup 80.9 / 86.1 s (111.3 s cold first run) -> 73.4 / 76.8 s.
- **Identity:** the same 18,868 files in all five runs; against each baseline the only differing files are the 41 of
  the run-to-run set. Tests: `test_customisation_workers.py` (the warm-up split, the derived-texture plan).
- **Cost:** the customizer fingerprint changes, so every existing install redoes the customizer once on its next
  refresh.
- **Open:** the worker count stays a share of the machine (`setup_budget.customiser_workers`, a quarter of the core
  budget, 1..8; memory scale-with-machine); see the worker share below. The docstring "a quarter" in
  `customisation_workers.py` matches `setup_budget.py` (the note above about it was stale).

**Customizer worker share on two machines (2026-10-24, USER-ASKS 316, 324):**
- **Laptop** (Ryzen 9 8945HS, 8 cores / 16 threads, 31 GB, NVMe; budget 12, 3 map workers, customizer default 3;
  pipeline 5b16a77b, before ddf2231a; laptop run setup-bench-20261024-1325): 30f19a4e -> 5b16a77b
  178.4 / 180.5 s -> 136.2 / 136.9 s (-24%). Customizer workers 3: 136.2, 136.9, 134.8, 136.3 s; 6: 117.4, 124.7 s
  (-11%): the maps end at about 103 s and the customizer (roster ends about 131 s) runs on alone with 3 workers
  while 9 cores of the budget are idle. Same 18,868 files; only the 41 run-to-run files differ.
- **Desktop** (28 threads, budget 21, customizer default 5; pipeline ddf2231a): 5 workers 77.3 / 76.8 s (library
  36.96 / 36.91 s, customizer ends 68.7 / 68.2 s, maps 72.5 / 71.9 s); 8 workers 78.9 / 79.3 s (library 29.81 /
  29.77 s, customizer ends 54.8 / 55.4 s, maps 74.0 / 74.1 s). The customizer and the maps already end together at
  5, so more customizer workers only take threads from the map jobs (`job_threads` is the budget left after the
  customizer, over 3 jobs). Same files (0 outside the 41-file set).
- **Conclusion:** no fixed share fits both (a quarter is right here, half is right on the laptop). What fits both is
  giving the customizer the cores the maps release once they finish (below).

**Machine-aware budget: CPU load at start, customizer takes the maps' cores (2026-10-24, USER-ASKS 324, 325):**
- **CPU load** (`setup_budget.measure_load`, called from `install._install`, the path both entry points take):
  setup samples the machine's CPU time for 0.5 s (Windows `GetSystemTimes`, no new package) and fixes the budget for
  the whole setup: 75% of the CPUs minus the ones other programs keep busy, never under a quarter
  (`load_budget`; e.g. 28 CPUs idle: 21, 25% busy: 14). Child processes inherit it as
  `SKATE_SETUP_MEASURED_CORES`; `SKATE_SETUP_CORES` still overrides. The setup log names it: "CPU budget: 20 of 28
  logical CPUs (other programs used 5%)".
- **Shared customizer pool** (`customisation_workers.run_shared`): the library warm-up (models in batches of 8,
  textures in 32), the derived textures (32) and the roster (one character per batch) are lists of batches that
  worker processes claim one by one (a file created exclusively per batch), so a worker started later joins the same
  work. It starts `customiser_workers()` (a quarter of the budget); once the map jobs end (`install.py` creates the
  `maps-done` marker named in `SKATE_SETUP_MAPS_DONE`, or at once when no maps convert)
  `setup_budget.customiser_limit` allows up to the budget less one, at most 16, and only as many as free memory
  allows (2 GiB per extra worker above a 2 GiB reserve). `SKATE_SETUP_CUSTOMISER_WORKERS` fixes the count (no growth).
  Each worker parses the catalog and opens the disc archive once (a first version re-parsed `catalog.json` per model
  batch: models took 25.8 s of worker time instead of 12.3 s).
- **Desktop** (where the maps already end last, so no gain is expected): 78.1 / 77.2 s before, 78.2 s after (library
  37.0 / 37.1 s before, 37.2 s after). Same 18,868 files; outside the run-to-run set only `sets/<id>/roster-complete.json`
  differs (it records the customizer fingerprint, which this change alters). No asset group fingerprint changed
  (`versions.fingerprints` before and after); the customizer's did (one customizer redo per install).
- **Laptop:** the gain should show there (maps end about 103 s, the customizer ran on alone with 3 workers to about
  131 s); measured with second-pc-testing.md "Machine-aware budget test".
- `setup_bench.py` (`.local/perf-bench`, published copy `tools/claude-skills/setup-bench/tools`) now reruns itself in
  a setup venv (`.local/venv-setup` of the checkout or a parent, or `.local/versions/hails-additions`) when the
  Python it was started with has no numpy / Pillow (the laptop's system Python 3.13 failed in extract_default_skater).
  An install folder was about 4.9 GB on the laptop (desktop bench installs include more). A snapshot outside a
  checkout finds none of those (laptop, 2026-10-24 16:00), so it also takes `--python <python with numpy>`.

### Laptop result and a laptop-shaped desktop run (2026-10-24 16:00 to 16:50)

- **Laptop** (Ryzen 9 8945HS, 8C/16T; second-pc-testing.md "Machine-aware budget test"): 964fb5bc 150.0 / 156.3 s,
  current 145.2 / 141.9 s (-6%); "CPU budget: 11 of 16 (8%)" and "12 of 16 (1%)"; roster 51 s -> 26 / 42 s once it
  takes the maps' cores. Same 18,868 files; before1 vs after1 41 differences, before1 vs before2 40, the extra one
  `sets/<id>/roster-complete.json` (the customizer fingerprint).
- The laptop's 964fb5bc (153 s) looked slower than its morning 5b16a77b (137 s), as if the library on the workers
  (ddf2231a) slowed its maps. Not so: the steps before the maps, which run none of that code, took 7.7 to 8.2 s in
  the afternoon against 5.8 s in the morning, so the laptop itself ran about 35% slower. A same-session test of all
  three versions (below) settled it.
- **Desktop pinned to 16 CPUs** (8 P-cores with hyperthreading, `tools/dev-bench/with_affinity.py 0xFFFF`) and
  `SKATE_SETUP_CORES=12` (the laptop's budget), two runs each:

| version | total s | 10th map | library | roster |
|---|---|---|---|---|
| 5b16a77b | 99.5 / 98.6 | 70.4 / 69.6 | 58.6 / 58.2 | 28.5 / 27.8 |
| 964fb5bc (library on the workers) | 89.1 / 89.9 | 71.3 / 71.5 | 47.1 / 47.4 | 29.0 / 29.4 |
| daf26d34 (load budget, customizer grows) | 83.2 / 82.9 | 72.2 / 72.0 | 47.1 / 47.0 | 23.3 / 22.9 |

  -16% in all on a laptop-sized budget; the maps end about 2 s later than on 5b16a77b.
- **Fix:** Python's `os.process_cpu_count()` ignores the CPU affinity on Windows (it said 28 in the pinned process),
  so the budget ignored it too ("21 of 28"). `setup_budget._cpu_count` now counts the affinity mask
  (`GetProcessAffinityMask`); pinned to 16 it reports "CPU budget: 12 of 16".
- **Laptop, same sitting** (2026-10-24 17:05 to 17:25, plugged in, power mode unchanged; second-pc-testing.md
  "Same-session version test"), runs alternating:

| version | totals s | mean | "Converting 10 maps" s | library | roster |
|---|---|---|---|---|---|
| 5b16a77b | 167.6 / 185.0 | 176.8 | 14.7 / 28.2 | 91.5 / 94.4 | 49.6 / 49.2 |
| 964fb5bc | 150.1 / 161.9 | 156.7 | 14.1 / 20.7 | 73.5 / 76.2 | 50.6 / 51.1 |
| a6ef8401 | 150.5 / 156.2 | 154.1 | 17.8 / 19.6 | 95.1 / 95.4 | 23.3 / 29.0 |

  Same order as the desktop. The machine was not steady: the maps started at 14 to 28 s (9.4 to 10.5 s in the
  morning), and the laptop's ASUS screensaver (rendering on its second screen while it sat idle) used about as much
  CPU as VS Code. a6ef8401 measured that load ("CPU budget: 11 of 16 (11%)", "10 of 16 (16%)"), so its customizer
  ran 2 workers instead of 3 beside the maps (library 95 s) and then grew once they ended (roster 23 / 29 s instead
  of 50 s): it gave way to the other program and still finished first. 18,868 files; no worker failures.

## Fingerprints

The stream change touches a file in the shared parser set, changing the
`character`, `environment` and `maps` fingerprints without changing outputs
(proven above); old→new pairs are in `pipeline-equivalence.json`. For this
change the customizer fingerprint and stage versions are unchanged, and an installation built
from the committed code rebuilds nothing (later passes did change the customizer fingerprint).

## Full setup result

Full fresh setup, same machine and source, before → after:

| Stage | Before (466 s total) | After (331 s total) |
|---|---|---|
| Stock data, HUD, skater, environment | 0–20 s | 0–19 s |
| Map stage (10 maps, 3 workers, validated) | 21–204 s (183 s) | 19–138 s (**119 s**) |
| Character customizer | 216–442 s (226 s, after maps) | 19–311 s (292 s, beside maps) |
| Publish | 442–466 s | 311–331 s |

**Total: 466 s → 331 s (−29%), 0 warnings.**

- Identical outputs: all 8,288 receipted customizer files match a snapshot of
  the previous (sequential) installation, with only the random set id
  differing in 3 JSON paths; all maps pass validation with unchanged
  collision counts and no warnings; all baked spawns unchanged.
- Contention: overlapped, the customizer's library stage took 193.9 s instead
  of 126.5 s alone (three map workers compete for disk and memory bandwidth,
  boost clocks drop, and the customizer thread shares the GIL with the setup
  main thread). It is now the critical path; the roster (89.6 s) and library
  were single-threaded loops then (both run on worker processes now: "Export speed pass" above).

## Files

- `tools/vendor/university/tools/vanilla_map_extraction/tools/skate3_streams.py`
- `tools/asset_pipeline/install.py` (`available_memory`, `overlap_customiser`,
  `Background`, map stage), `pipeline-equivalence.json`
- `tools/asset_pipeline/test_streams.py`, `test_map_validator.py`
- Section 3: `scripts/Build.ps1`, `tools/asset_pipeline/customisation_workers.py` (new),
  `customisation_library.py`, `native_roster.py`, `customiser_setup.py`,
  `test_customisation_workers.py` (new), `test_customiser_setup.py`

## Refresh Game Files: setup window opened only after a full file check (2026-10-15)

**Problem.** After Extras > Refresh Game Files the setup window took a while to open (reported by the user,
2026-10-15).

**Root cause.** `tools/setup.py` ran `damaged()` (`asset_pipeline/group_receipts.py`) before creating its window, only
to list the groups in its label. `damaged()` re-reads and SHA-256 hashes every installed output file of every group
(audio, maps, props, stock data), minutes on a hard drive. `install()` then ran the same check again on the worker
thread, so the files were hashed twice. (Since 2026-10-23 the check compares size and time first and hashes only a
file whose time moved; see "Setup speed pass: speech decode and refresh check" below for its cost now.)

**Change.** setup.py keeps only the version check before the window (0.75 s measured on the user's installation);
the file check runs once, inside `install()` on the worker thread, and `damaged(report=...)` shows
"Checking your installed game files: <group> (n of N)..." in the window while the progress bar runs (user, 2026-10-15:
"make it show a progress screen while it rechecks every game file it had installed"). The groups that get prepared
are unchanged; the label now says setup checks the installed files first.

**Files.** `tools/setup.py`, `tools/asset_pipeline/install.py`, `tools/asset_pipeline/group_receipts.py`,
`tools/test_setup_assets.py` (the progress message per group).

**Verification.** Setup unit tests (37) pass. The window itself has not been opened yet (the user was playing).

**Fix (2026-11-13): a refresh with the window crashed, then hung.** The window's progress bar was named `progress`, the same name as the progress writer (`ProgressWriter`, the launcher's progress file) that the worker hands to the conversion as its report since the per-group progress above. With a progress file the worker passed the Tk bar instead: an ISO refresh failed at once ("'Progressbar' object is not callable", `iso_refresh.py`), and the error handler failed on the same bar (`progress.write`) before it told the window, so the window stayed in its running state (it cannot be closed while running) and the refresh hung until it was killed (laptop, 30 min). Now the bar is `bar`, and the handler posts the error to the window first, then writes the log and the progress line inside its own try, so a failing report never leaves the window stuck. Test `test_the_window_bar_never_hides_the_progress_writer` (it finds the old name in the code before the fix); setup tests 13 / 13 (`test_setup_refresh`, `test_setup_progress`, `test_setup_recovery`).

## Code reference: asset_check.rs

Detail moved out of the code comments of `crates/skate-game/src/asset_check.rs`; each comment there links its section here.

### Asset staleness check

Launch-time asset staleness check. It reads the converted files themselves on every start, not a stamp: every output setup recorded must still be there at its recorded size, and each group's files must hold what this build reads (format versions and content). A failing group is logged (`ASSET_CHECK <group>: <reason>`) and marked stale (`setup::mark_stale`), so the PC overlay offers Extras > Refresh Game Files, which makes setup rebuild exactly those groups. Runs on a background thread once the startup loading screen is gone, so its thousands of file reads do not compete with the start's own loading on an HDD.

A file a later build added is not in an older install's receipts, so it needs its own check. Laptop session game-20261101-084423: an Oct 25 install had no `private/hud/scene/MarqueeStage.skate`, FE 60 drew the preview skater in the live world with no stage, and both `--check-assets` and Refresh Game Files said the files were current. The check now reports `menu stage missing` when `TrickGuide.skate` exists without it (older hud export), and that finding forces the refresh like out-of-date menus.

Development and launcher copies have no `release.json`, so nothing compared their converters with the install's (`installation.json` `pipelines`). `--update-game-files`, an explicit request, now runs the checkout's `tools/asset_pipeline/versions.py` and reports every group that differs, ignoring setup's `+dlc:` suffix (a changed pack is the DLC check's finding) and honouring `pipeline-equivalence.json`. A failure to run Python is logged and falls back to the file checks. Normal starts stay file-only: a Python start on every launch would slow them. Tests: `asset_check::tests::hud_check_reads_the_files_not_a_stamp`, `setup::tests::an_explicit_refresh_reports_changed_converters`. Open: the forced refresh on the real laptop install is the end-to-end check.

## Code reference: splash.rs

Detail moved out of the code comments of `crates/skate-game/src/splash.rs`; each comment there links its section here.

### Startup splash

Startup splash (Windows): a small window of its own, up from the first moment of the process, with a skateboard (no skater) rolling across it and exactly what is loading written under it, the last finished steps dimmed below (like RPCS3's progress lines). It runs on its own thread with its own message loop, so it keeps animating while the game thread is busy (config, skater, physics, world, menus, the audio runtime). The game window is created hidden and shown when the start is done (`startup_loading`), and then the splash closes.

Steps are reported from any thread with [`stage`] (a step: "Loading physics") and [`detail`] (the item being read: "sound bank grind_concrete"). Not retail: retail boots into its own movies; this only covers our engine's start. `SKATE_NO_SPLASH=1` turns it off (scripted runs that want no extra window).

### Asset refresh on the splash (2026-11-12, PR 68 ask 611)

**Problem.** Refresh Game Files restarts the game with `--refresh-assets`; the splash came up and sat on "Reading the settings and the map (N s)" for the whole refresh while setup's own window showed the progress beside it (design b172, `.local/research/b172-splash-asset-refresh.md`).

**Decisions (user, 2026-10-17).** The splash only (setup's window appears only when setup needs the player or fails); a Cancel button that stops setup and starts on the current game files; after a refresh the game starts as before; no launcher entry for now.

**Change.**
- Setup (`tools/setup.py`): `--progress-file <path>` writes JSON lines (`tools/asset_pipeline/setup_progress.py`, version 1: `start` with the groups, `step`, `item` with done / total, `needs_window`, `error` with the log, `done`; readers ignore unknown kinds), `--no-window` runs a refresh with a reusable source without the tk window; without a reusable source it writes `needs_window` and opens the window as before. The map loop reports its count (`report.progress`) when the reporter has it; plain lambdas get the text as before. The edit sits in `_install_stages`, which the pipeline fingerprints do not hash, so no group rebuilds for it.
- Game (`crates/skate-game/src/setup.rs` `run_setup`, both the release and the development refresh): spawns setup with the progress file (and `--no-window` when the splash is up), follows it every 100 ms onto the splash (`refresh_mode`: title "UPDATING GAME FILES", the groups under it; steps and items as the step line; a thin progress bar while the count is known; `fail` in the warning color), and logs every line as `SETUP: ...`. Cancel kills setup's process tree (`taskkill /T`), leaves the `--assets` link on the current installation (setup publishes a new installation only at the end) and starts the game; the stale groups stay marked, so the overlay still offers the refresh. A failed setup leaves its message up for 5 s before the error path.
- Splash input: the window still drags anywhere except over the Cancel button (client area there, `WM_NCHITTEST`).

**Verification.** Python `test_setup_progress` (format, plain-lambda fallback, the windowless path's start / done and error with `setup-error.log`); Rust `setup_progress_lines_become_splash_changes`, `the_progress_file_is_read_line_by_line_without_half_lines`; the ignored `splash::tests::refresh_mode_preview` puts the refresh splash up for 8 s (screenshot checked: title, groups line, bar, step line, error text, Cancel button). Not run yet: a real Refresh Game Files (it rewrites the installation; needs the user's go).

**Setup windows in the project's look (ask 611).** The setup window (`tools/setup.py`) and the updater (`tools/updater.py`) use one tkinter theme, `tools/ui_theme.py`: the splash's palette (near-black background, the gray caps title, bright body text, the maple ply gold for the action button and the progress bar, amber for warnings), Segoe UI, the `clam` ttk base so the colors apply on Windows. Titles read like the splash ("UPDATING GAME FILES", "SET UP SKATE 3 RUST ENGINE", "UPDATES"); the updater's two em dashes in its title and lead went. The updater imports the theme from beside itself first (a script run and PyInstaller's analysis of `tools/updater.py`, whose package has no `--paths` root) and falls back to `tools.ui_theme`. Checked: screenshot of the first-install window; Python tests `test_update*` (22), `test_setup*` (24), `test_versions` (9) pass.

**Open.** Partial staging cleanup after a cancelled refresh (`remove_stale_installations` on the next run, b172 unverified); a packaged `support/skate3setup.exe` older than this change does not know the two flags (it fails at argument parsing), so a development copy with a stale packaged helper needs it rebuilt.

## Code reference: startup_loading.rs

Detail moved out of the code comments of `crates/skate-game/src/startup_loading.rs`; each comment there links its section here.

### Startup loading phase

The start's loading phase behind the splash (`splash.rs`): the game window is created hidden and stays hidden until every hold is released, gameplay waits meanwhile (virtual time paused, as the menu pauses it, so the game starts in the same order as before), then the window is shown and brought forward and the splash closes.

The game is silent while the splash is up (`AudioSettings::master` is 0 until `splash::close`, 2026-10-27, ask 384): the world's beds and random one-shots start ticking behind the splash and were heard for over a minute before the game showed (session game-20261027-073320: splash 95 s, ambience from 18 s).

The slowest hold is the native audio runtime's first start (~40 s on an HDD with a cold file cache; `game_audio::native::begin_launch` builds it on its own thread). Engine systems and mods hold the phase with [`StartupLoading::hold`] / [`StartupLoading::release`] (a label each, shown as the splash's step) and report the item being read with [`detail`].

### The setup window (2026-10-27, ask 385)

**Problem.** The setup / update window showed a long paragraph (asset version list, reuse notes, where to keep the game data) and its button even when nothing had to be picked; the user asked for a less cluttered, better-looking screen.

**Change** (`tools/setup.py`, `tools/asset_pipeline/install.py`; ours, setup is not a retail screen). Title "Updating game files" / "Set up Skate 3 Rust Engine", one lead line (what is converted, or what to pick), one gray note, one status line over a thin progress bar; the file button shows only when a file must be picked, and again after an error; window 700 x 320. The conversion itself is unchanged.

**Verification.** Both layouts (update and first setup) rendered and checked; setup tests unchanged. Not seen by the user in a real refresh yet.

## Reproducible setup output (2026-10-29)

**Problem.** Two setups of the same pipeline differed in 41 files (map `.skate` files, park layouts, native props, the
Industrial proxy backdrop, HUD / menu manifests, the session marker, the customization set), so a bench could only
compare a change against a second run's noise.

**Cause.** Absolute paths written into the output: the `.skate` files' `WMET` manifest kept the setup's random temp
folders (`skate-proxy-*`, `skate-park-*`) and the install folder (`installations/<id>`) as texture and stream source
paths; the HUD and menu font definitions kept their `sk8hud-*` temp source path; the session marker hashed a source
manifest that names the temp folder. Nothing reads those paths back (the engine reads `WMET` only for grind source
identities).

**Change.** `tools/asset_pipeline/map_writer.py` `portable`: absolute paths in the stored `WMET` manifest become their
file names; `tools/vendor/skate3_ui/scene_graph.py`: a font definition keeps its source's file name;
`tools/extract_session_marker.py`: the source manifest is hashed without the temp folder.

**Verification.** Setup bench, two runs of the unchanged pipeline: 41 differing files; two runs of the fixed one: 8,
all by design (the customization set's fresh id in its 7 files, and `maps.json` timings); the file lists are equal.
`test_map_writer` and the HUD / menu tests pass. A Refresh Game Files rewrites the maps once (the map writer's
fingerprint changed).

## Setup speed pass: speech decode and refresh check (2026-11-02, USER-ASKS 464)

**Problem.** A first setup from the Xbox 360 ISO took 500 s on the desktop (world/replay 02c67017, ISO on the F: hard
disk, install on K:), and a Refresh Game Files with nothing to do took 18.5 s. Earlier passes had been measured from an
extracted `default.xex` and before Ped Speech was on by default.

**Root cause.** (1) Speech: `world_audio.decode_speech` decoded its clips one after another, one vgmstream process per
clip (3011 world, 1283 main-cast and 405 announcer clips, about 33k takes), while the sound banks already decoded on
`setup_budget.decode_workers()` threads. World speech took about 250 s and main-cast about 107 s of the audio stage, the
critical path of the whole setup. (2) Refresh: `group_receipts.valid_group_receipt` resolved the installation root once
per file and ran two stat calls per file: 149k `_getfinalpathname` calls (6.9 s) and 76k stats (2.5 s) for about 37k
files (cProfile of a no-change refresh).

**Change.** `decode_speech` runs the clips through `audio_export._in_order` (the bank decoders' pool, sized from the
machine); clips sharing a folder stay together in index order, so the result is the same as one after another.
`valid_group_receipt` resolves the root once and makes one stat per file (a regular-file check instead of `is_file()`);
the per-file resolve and the inside-the-installation check stay. `pipeline-equivalence.json` gets the proven pair for
the audio fingerprint, so existing installs do not redo their audio. `group_receipts.py` is in no fingerprint.

**Verification** (bench `tools/dev-bench/setup_bench.py`, skill setup-bench; `--keep` added for refresh runs):

| Run | Before | After |
| --- | --- | --- |
| First setup from the ISO | 500 s | 188 s |
| World / main-cast speech | 250 / 107 s | 67 / 27 s |
| Refresh, nothing to do (2 runs each) | 18.3 / 18.7 s | 14.9 / 14.5 s |
| Refresh, audio check | 7.5 s | 4.0 s |
| First setup from the ISO, every change of this pass (fresh, final audit) | 500 s | 176.7 s |
| Refresh from the ISO, nothing to do, every change | 18.5 s | 9.2 s |

- Identity: same 40727 paths; every differing file is in the known noise set, plus
  `assets/private/session-marker/hud.json` on ISO runs: its `source_manifest_sha256` hashes the UI archive records,
  which hold the archive's absolute path inside the run's random `character-source-*` extraction folder (pre-existing,
  differs between any two ISO runs).
- Damage audit: one deleted speech take and a same-size one-byte change in `stock/data/config/input.cfg` were detected
  by the changed code; the refresh rebuilt the affected groups and every file hashed as before (only the two noise
  files differ).
- Unit tests: test_living_world, test_optional_content, test_versions, test_setup_assets, test_setup_shared_links (47).

### ISO refresh without extracting the disc

**Change.** `asset_pipeline/xiso.py` reads named files straight out of the Xbox 360 image (XDVDFS: the volume
descriptor 32 sectors into the game partition, binary-tree directories). On a refresh from an ISO, `tools/setup.py`
calls `asset_pipeline/iso_refresh.py`: it takes only the five files the checks read (`default.xex` and the customizer's
source archives, 637 MB instead of the whole disc's 6.1 GB) into a folder marked `.partial-source`. When
`install._install_stages` then finds a group to rebuild, or the customizer set is not current for this code and disc,
it raises `NeedsFullSource` before writing anything and the refresh runs again with the whole disc, as before. The
customizer's `prepare()` never runs on a partial source (its optional-content handling could drop the set pointer on a
missing file); when the set is current the partial path does what its success path does. Neither file is in a
fingerprint; the install.py guard is a local import, so no group fingerprint moves.

**Verification.** The five files read this way are byte-identical to extract-xiso's (the customizer source fingerprint
and the `default.xex` hash both match the ISO install's records). No-change refresh from the ISO: 14.7 s -> 11.6 s
warm (the extraction step is 0.8 s instead of 3 s warm and about 33 s cold). Fallbacks, each repaired to the baseline
hashes: a deleted main-cast take (the guard asks for the whole disc for audio after 4 s), an incomplete customizer set
(the finish step asks for it). `test_xiso.py`: both partition layouts, letter case, missing paths, a folder asked for
as a file, a file that is not a disc image.

### Customizer check cost

**Change.** The customizer stage receipts (`*-complete.json`) record each file's time, and `customiser_cache.stage_valid`
uses `group_receipts.valid_group_receipt` (a file is hashed only when its size or time moved), so a refresh no longer
hashes the whole 1.9 GB set. Also fixed: a reused stage rewrote its JSON files in text mode, which turned their line
ends into CRLF on Windows; they are copied as bytes now (only the set path prefix replaced). `customiser_cache.py` is
part of the customizer fingerprint, so every install redoes its customizer once (27 to 34 s on the desktop, the
unchanged stages reused); there is no equivalence mechanism for the customizer.

**Verification.** No-change refresh 11.6 s -> 9.2 s. The rebuilt set matches the baseline set file by file except the
known set-id noise, the three other stage receipts (they now hold file times, so they differ between runs like
`installation.json`) and two JSON files whose CRLF came from the reuse before the fix (equal to the baseline with line
ends normalized; the fixed reuse copies them byte for byte). A one-byte change to a set texture (same size) was found
and only its stage (the library) was rebuilt, back to the original hash. A full build also leaves the roster stage's
scratch folder `roster-work/` in the set; a reused stage does not recreate it (nothing reads it).

### Maps refresh

**Change.** A refresh that rebuilds the maps hard-links the previous `maps/` (10 maps: `.skate`, `.irradiance`,
`.triggers`, 859 MB) instead of copying it, and records the links in `shared`, so `shared_changed()` refuses to publish
if one was written in place. `map_writer.write` and `irradiance.write` unlink their output before writing (the
`.triggers` writer already replaces); the failed-map fallback skips the copy when the target is still the linked
previous file. Equivalence pairs for hud, environment and maps (`map_writer.py` / `irradiance.py` are in their
fingerprints), after the identity proof below.

**Verification.** A refresh rebuilding hud, environment and maps with every previous map file also hard-linked into a
witness folder first: all 30 witness files byte-identical afterwards, the published install equal to the baseline.
Gain: on the desktop's K: drive with a warm cache the copy was only 0.2 s (links 0.005 s); the saving is on hard disks
(859 MB read and written) and the disk space of a second copy while both installations exist.
`test_setup_shared_links.MapWriters`: writing a linked `.irradiance` leaves the previous file and passes the shared
check. Unit tests run: 120 (setup, customizer, maps, versions, living world).

**Final audit.** A fresh full setup from the ISO with every change of this pass against the pristine baseline: the same
40727 paths; the only differing files are the known noise set, `session-marker/hud.json` (ISO-run noise, above) and the
three customizer stage receipts that now hold file times (`catalog-complete.json`, `menu-complete.json`,
`roster-complete.json`).

**Open.** Measured on the desktop only (laptop numbers to come); the cold-cache ISO extraction is not measured
separately. Pre-existing, seen during this pass: old customization sets are never removed (each about 1.9 GB, partly
hard-linked); the maps receipt records only `.skate`, so a damaged `.irradiance` or `.triggers` is not detected; the
session-marker `hud.json` hashes absolute archive paths (differs between ISO runs).

## Start-up pass (2026-11-03, USER-ASKS 471)

**Problem.** The game took about 6.5 s from `main` to the game window on a warm start and 33 to 47 s on a cold one (desktop, install on a hard disk, University). The logged `STARTUP_LOADING done in` only counted from the app build, not the 2 s before it.

**Measuring.** `STARTUP_STEP <ms since main> <step>` lines (`startup_loading::mark`): every start-up stage, the first frame, each loading hold's release, the audio start's phases, and "game shown"; they stop once the game shows. Bench: `tools/dev-bench/startup_bench.sh <exe> <out> [s]` (muted University start, the timeline; `TIMING=1` adds the first SYSTEM_TIMING line = the Startup systems; `COLD=1` first drops the install and the exe's folder from the Windows file cache with `evict.ps1`, which opens each file unbuffered: no admin needed).

**Where the time went.**
- Warm (6.5 s): config + map 1.1 s (the map's fingerprint hash about 0.3 s of it), physics 0.7 s, skater 0.2 s, renderer 0.3 s, the Startup systems 3.1 s (`world::spawn` 1.2 to 1.9 s, `retail_menu_movies::load` 1.3 s, `scoring_hud::setup` 0.4 s), the first frames 1 s.
- Cold (33 to 47 s): every step reads scattered small files. The Startup systems 15 s (menu movies, the session marker HUD and the menus 9 to 11 s each, side by side), then the audio runtime's first start, which only began after the first frame: 11 to 25 s, most of it the player banks. Three of those banks are 1,642 WAV files (`Skate_Collisions` 1,101, `Skate_Metal` 430, `sk8_foley` 111; 39 MB), read one after another: about 12 s of disk seeks.

### Audio start begins in main

The native audio runtime's first start (`native::begin_early`, called from `main` right after the config and map) is built on its own thread from the top of the start instead of after the first frame. `begin_launch` takes that build when its inputs (asset root, world instance counts) match what it would use; otherwise, and when mods add audio overlays (merged first), it is dropped and the start runs as before. The build is the same function on the same inputs (`build_first`: `Library::load` + `Native::start_from`), so the runtime is the same; `AudioSettings::more_audible_world_at_start` reads the one setting it needs without `AudioSettings::load`'s side effects. `SKATE_EARLY_AUDIO=0` turns it off.

### Menu movies read begins in main

`RetailMenuMovies::read` (a pure read of the exported menu movies) runs on its own thread from `main`; `retail_menu_movies::load` takes the result at its old place when the folder matches, else reads as before. `SKATE_EARLY_MENU_MOVIES=0` turns it off.

### Bank files read in parallel

`decode_wavs` reads one bank's sample files on a few scoped threads (half the cores, 2 to 8; `SKATE_AUDIO_READ_THREADS`, 1 = one after another). Each slot keeps its own file's result, so the bank's PCM is the same; several reads in flight let the disk and the OS order them. Used by every bank load (the start, the prefetch worker, map changes).

### Map fingerprint beside the map load

`config::load_map` hashes the map file (`map_fingerprint`, the multiplayer check) on a second thread while the map loads, instead of after it. Same function, same file: the fingerprint is the same (`2479a7e1c4dbf39a` for University before and after).

**Result (desktop, University).**
- Warm, all four on vs off (`SKATE_EARLY_AUDIO=0 SKATE_EARLY_MENU_MOVIES=0 SKATE_AUDIO_READ_THREADS=1`), 3 interleaved pairs before the fingerprint change: game shown 6.62 / 6.70, 5.61 / 6.52, 5.72 / 6.47 s; first frame 0.6 to 0.75 s sooner. With the fingerprint change: 5.35 to 5.56 s (config stage 1.08 -> 0.81 s). About -1 s.
- Cold (hard disk), 2 interleaved pairs: 46.3 / 47.7 s and 36.5 / 46.7 s. Cold runs vary by 10 s and more (Windows' launch prefetch, the disk); one "on" run had the renderer's start stall for 15 s while the audio thread read (the exe's own pages competing for the disk). The hard-disk floor is the file count: the 1,642 small WAV files alone cost about 12 s of seeks whatever the order.
- Laptop (NVMe SSD, University, 2026-11-03 05:46 note): warm, 3 interleaved pairs, game shown ON 9.92 s vs OFF 10.58 s (-0.66 s), first frame 8.07 vs 8.74 s; cold (`COLD=1`), 2 pairs, 10.17 vs 12.72 s (-2.55 s), the audio start done before the skater loads instead of after the first frame. Fingerprint `177dff29d1413d84` in every run; the tests as on the desktop (175 pass, 9 need private trace data).

**Open.** A cold start on a hard disk needs fewer files: done next, [one file per sound bank](#one-file-per-sound-bank-2026-11-03-user-asks-472). `world::spawn`'s prepare (1.2 to 1.9 s warm) could run on a thread like a map change does, but its asset handles would then interleave with the other Startup systems' assets (a different asset id order), so it stays where it is until that is shown not to change rendering.

## One file per sound bank (2026-11-03, USER-ASKS 472)

**Problem.** A cold start on a hard disk spends about 12 s seeking between the 1,642 small WAV files of the three player banks (5,867 bank WAVs in all, 0.8 GB); see [where the time went](#start-up-pass-2026-11-03-user-asks-471).

**Change.**
- Setup (`audio_export.wav_pack`) also writes each bank's WAVs back to back into `banks/<stem>.pack`: `SKWAVPK1`, the count (u32), a zero u32, then per WAV its offset and size (u64 each, little-endian), then the WAVs' bytes unchanged. The manifest lists the packs (`bank_packs`: bank -> pack) and gives each bank sample its range (`pack: [offset, size]`); the sample keeps its `file` name.
- The loose WAVs stay: every engine version on one install (the launcher's main, older builds) reads them. The new engine reads only the packs (0.8 GB more on disk).
- The game (`game_audio/library.rs`, `WavSource`): a bank's load reads its pack with one read and decodes each slice; one sample (a location set's pick, the read-ahead) reads its range. A missing or damaged pack, or a range outside it, falls back to the loose file, so an install from before this change plays as before. `SKATE_AUDIO_PACKS=0` reads the loose files (for comparisons).
- Mods: a mod's replaced sample or bank is a new manifest entry without a range, so it is read from the mod's file as before; the other samples of that bank still come from the pack. Kept DLC banks carry their pack over a refresh (`retained.py`).
- The audio group's converter changed, so the next Refresh Game Files redoes the audio group once.

**Verification.** `packed_banks_decode_the_same_as_loose_wavs` (the fixture's banks packed, the loose files moved away: bit-identical PCM and bank; a damaged pack falls back; a mod's slot stays the mod's file); `install_packs_hold_the_loose_wavs` (data-gated: every packed sample's bytes equal its loose WAV, every bank decodes bit-identically from the pack); `test_audio_formats.WavPack`, `test_retained` (a kept bank keeps its pack).

**Result.**
- Desktop (install on a hard disk, University, cold with `COLD=1`, 2 interleaved pairs): game shown 21.3 / 26.4 s with packs vs 27.4 / 30.4 s loose (about -5 s); the player banks done at 5.4 / 14.8 s vs 16.2 / 16.2 s. Cold runs still vary by several seconds (one pack run's banks waited on other start-up reads). Map fingerprint `2479a7e1c4dbf39a` in every run.
- Laptop (NVMe SSD, 2026-11-03 08:37 note): Refresh Game Files 514 s, 299 packs, banks folder 786 MB -> 1,573 MB; tests 177 pass + the 9 private-trace-data failures, `install_packs_hold_the_loose_wavs` passes (299 banks, 5,867 samples identical). No gain on an SSD: cold 10.30 vs 10.06 s (one pack run after the refresh left out), warm 9.40 vs 9.47 s, player banks about 960 ms either way. Same map fingerprint with and without packs.
- That refresh also rebuilt the living world group, because the pack code first sat in `audio_formats.py`, which the living world group fingerprints too (its region parser). The pack code now lives in `audio_export.py`, which only the audio group fingerprints, so a refresh rebuilds the audio group alone.

## Hard drive note and memory headroom (2026-11-11, USER-ASKS 582, 588)

**Problem.** A refresh on the desktop install folder (a hard disk) spent its first ten minutes copying the previous install's speech files at about 13 MB/s read and write with about 8 % CPU: on a hard disk, many small files are bound by the drive, not the processor. Users get no hint why setup is slow there. Separately, a refresh on 2026-11-10 ran the machine out of commit (a small page file) while free RAM still looked fine.

**Change.**
- `tools/asset_pipeline/drive_kind.py` `on_hard_drive(path)`: True on a spinning drive, False on an SSD, None when unknown. Windows asks the volume for its seek penalty (`IOCTL_STORAGE_QUERY_PROPERTY`, `StorageDeviceSeekPenaltyProperty`, no admin rights), Linux reads `queue/rotational` of the disk behind the folder, macOS reads `SolidState` from `diskutil info`. Junctions and symlinks are followed first, so a linked install folder reports the drive it really lives on.
- `install.py` `_install` reports one calm line (`HARD_DRIVE_NOTE`) at the start of setup when the install folder is on a hard drive. Nothing in game, never repeated.
- `install.py` `available_memory()` is now the smaller of free physical memory and the commit headroom, so worker counts follow a small page file too.

**Verification.** `tools/asset_pipeline/test_drive_kind.py` (Linux partition-to-disk lookup on a fake `/sys`, unknown platforms, Windows system drive). Desktop: F: and G: report hard drives, C:, D:, E:, I:, K: SSDs.

**Open.** Setup speed on hard disks and lower-RAM machines (USER-ASKS 586, 587): the copy of a rebuilt group's old outputs, single-thread stages, fixed caps.

## Dev build as the game exe (2026-11-11)

**Problem.** `setup.py --game-exe <target>\debug\skate3rust.exe` failed at its TEST_WORLD check with "Conversion failed" and an empty log. A cargo dev build loads `bevy_dylib-<hash>` and `std-<hash>` from `<target>\debug\deps`, which only `cargo run` puts on the search path; started by setup, Windows ended it with 0xC0000135 (DLL not found).

**Change.** `install.py` `_spawn`: when the program has a `deps` folder beside it holding `std-*` and `bevy_dylib-*`, that folder goes first on the library search path of the child (`PATH` on Windows, `LD_LIBRARY_PATH` on Linux, `DYLD_LIBRARY_PATH` on macOS). Every game start in setup (map loads, the map validator, `--check-assets`) goes through `_spawn`. Release builds and the `bin` folder of a checkout have no `deps` folder and start exactly as before. `run` reports a missing-DLL exit (0xC0000135) as such, naming the exe, instead of the generic message.

**Verification.** `tools/asset_pipeline/test_dev_game_exe.py`: the deps folder is found only with both libraries, the spawned dev build gets it first on the path while other children keep the inherited environment, the missing-DLL exit gets its own message.

## Audio decode cache (2026-11-11, USER-ASKS 590)

**Problem.** Every audio rebuild decoded every clip again with vgmstream: hundreds of `vgmstream-cli` processes (one per sound bank, bed, speech group and batch of 256), although most clips are byte-identical between runs. On Windows each process start costs time, and the decode itself is the audio stage's main cost.

**Change.** `tools/asset_pipeline/audio_export.py` `DecodeCache`: `_decode` (the only place setup starts vgmstream; banks, beds, grains, wheels, world / main-cast / announcer speech, presentation speech) keys each input by sha256 over the vgmstream executable, the `.snr` header and its `.sns` body. A hit hard-links the cached WAV into place (a copy where links are not possible) and the clip is left out of the vgmstream call; a folder of hits starts no process. New results are hard-linked into the cache. The cache lives in `<base>/installations/decode-cache` (beside the installs, so links stay on one drive and take no extra space); `convert` drops entries the run did not use, so it holds one set. Nothing after `_decode` writes a decoded WAV in place (it is read, moved or renamed), so a link never changes a cached file. `SKATE_SETUP_DECODE_CACHE=0` turns it off; scratch folders that are not an `installations` folder never use it.
- A refresh that rebuilds the audio group no longer copies the previous install's `private/audio` (about 5 GB of small WAVs) into the new one first: the audio step deleted that copy before writing (`install.py` `ignore_rebuilt_assets`). On the desktop's hard disk that copy was the first ten minutes of the 2026-11-11 refresh. `retained.carry` reads the previous install itself, not the copy.

- Sound banks and speech look the cache up from the clip's bytes in memory (`cached`, `DecodeCache.key_bytes`, the same key as the files): a hit links the cached WAV straight to its place in the install, without writing the `.snr` / `.sns` inputs, reading them back and renaming.

**Verification.** `tools/asset_pipeline/test_decode_cache.py` (key over header, body and tool; byte keys equal file keys; a hit writes no inputs; a second decode starts a process only for the new clip and gives the same bytes; prune; folder rules). Setup bench on the desktop (K:, NVMe), three runs: the cache off, a fresh setup filling it, and a refresh rebuilding the audio from hits; all three give the same 42,208 files (only the known customization id / timestamp noise and maps.json differ). The audio decode stage (ambience to announcer): 117 s with the cache off, 21.5 s from hits (76 s from hits before the byte lookup); the whole setup 265 s vs 127 s. A refresh from hits starts no decode process (the music export's own ranges still run).

**Open.** The music export (`music_export.py`) decodes with its own vgmstream ranges (about 4 to 7 s per set on the desktop bench) and packs FLAC banks with the game executable, which dominates; caching the packed banks would need the executable in the key. Not cached.

## Lower-memory machines (2026-11-11, USER-ASKS 586)

**Problem.** Setup's memory gates (map workers, the customizer beside the maps, its later share) only worked on Windows: `available_memory()` returned None elsewhere and every gate reads None as "no limit". The customizer's starting worker count (a quarter of the cores, up to 8, about 2 GiB each) had no memory gate at all, so a 16-thread machine with 8 GB could start about 8 GiB of customizer workers.

**Change.**
- `install.py` `available_memory()`: Linux reads `MemAvailable` from `/proc/meminfo`; macOS sums the free, inactive and purgeable pages from `vm_stat`. Windows stays the smaller of free RAM and the commit headroom.
- `setup_budget.py` `customiser_workers()`: at most (available memory less the 2 GiB desktop reserve) / 2 GiB workers, at least one. Worker counts do not change the converted files.

**Verification.** `tools/asset_pipeline/test_low_memory.py` (Linux and macOS readings, the worker cap at 6 GiB and 1 GiB); `test_customisation_workers`, `test_setup_budget` pass.
