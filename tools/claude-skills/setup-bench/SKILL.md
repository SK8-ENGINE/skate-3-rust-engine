---
name: setup-bench
description: Time a full asset setup (first setup or Refresh Game Files) and prove that a change to the setup or export scripts keeps the converted files the same - a scratch install from a snapshot of the pipeline, per-stage timings and a sha256 manifest, compared against a second run of the unchanged pipeline. Use for setup speed work and before landing any change to tools/asset_pipeline, tools/owned_game or the vendored converters that should not change the output.
---

# Setup bench

Tool: `tools/setup_bench.py` next to this file. Doc: `docs/hails-additions/08-setup-performance.md` ("Export speed
pass": the numbers, the identity proof and the lessons).

## Rules
- Same output is the bar. The setup is NOT byte-reproducible run to run: about 40 files differ between two runs of the
  same pipeline (map `.skate` files, park layouts, native props, the customiser set id in paths, HUD / menu manifests
  that hash their temporary sources, the customiser's `*-complete.json` / `timings.json`). Always compare a change
  against a SECOND run of the unchanged pipeline, and check that the file lists are equal.
- A full install is about 19 GB: put the scratch base on a drive with room, delete it when done.
- One setup at a time. Setup runs the game exe headless for map validation (`--validate-maps`), so do not run the game
  meanwhile. Never start the game exe with a flag just to probe it: an unknown flag exits with an error and opens the crash
  report window.
- In-process setup stages gain nothing from threads (GIL), and two threads in NumPy at once crashed Python (its BLAS):
  overlap stages as separate processes (the map jobs, `stage_job.py`, the HUD export), and lock NumPy work in a pool.

## Steps
1. Snapshot the pipeline so edits made meanwhile do not leak in: `git archive HEAD | tar -x -C <bench>/pristine`, and
   copy `target/native/refpack.dll` to `<bench>/pristine/target/native/` (without it RefPack decodes in pure Python,
   about 10x slower, which is not what users get; `scripts/Build.ps1` builds it). For a change: a second snapshot with
   the changed `tools/` files copied over it.
2. The game exe: copy a current build's `bin/skate3rust.exe` (the checkout being benched, not an old one) with every `bin/bevy_dylib-*.dll` and `bin/std-*.dll` (a dev build needs them
   beside it) into `<bench>`; every run uses that same exe.
3. Run from the snapshot (Git Bash), in the background (a few minutes; `SKATE_REPORT_CHILD=1` keeps a failing headless
   game run from opening a crash report window):
   `export SKATE_REPORT_CHILD=1; cd <bench>/pristine && py -3.13 tools/claude-skills/setup-bench/tools/setup_bench.py --checkout <bench>/pristine --base <bench>/base1 --game-exe <bench>/skate3rust.exe --source <disc>/default.xex --label base1`
   It writes `<label>-timing.log` (seconds, progress line) and `<label>-hashes.tsv` (path, size, sha256). Add
   `-X faulthandler` after `py -3.13` when testing threading changes.
4. A second baseline (`--base base2 --label base2`) for the noise set, then the change (`--label new1`).
5. Compare: normalise the customiser set id (`sed -E 's#customisation/sets/[^/]+/#customisation/sets/ID/#'`), sort,
   `comm -3` per pair; every path that differs new-vs-base must be in the base-vs-base set, and the path lists must be
   equal. Explain any extra file (for example a pipeline fingerprint recorded in a file) before landing.
6. Where the time goes: the timing log; per-map phases in the bench install's `maps.json` (`phase_seconds`); inside a
   stage `py -3.13 -m cProfile -s cumtime <stage script>` on the snapshot; temporary `report('PROBE ...')` lines only in
   the snapshot copy, never in the repo.
7. Document before / after numbers, the identity result and what is open in doc 08.

## Budget knobs (`tools/asset_pipeline/setup_budget.py`)
`SKATE_SETUP_CORES` (default 75% of the CPUs the process may run on; every pool derives from it),
`SKATE_SETUP_MAP_WORKERS`, `SKATE_SETUP_THREADS`, `SKATE_SETUP_DECODE_WORKERS`, `SKATE_SETUP_CUSTOMISER_WORKERS`,
`SKATE_SETUP_EXTRACT_THREADS`, `SKATE_SETUP_PRIORITY`.
