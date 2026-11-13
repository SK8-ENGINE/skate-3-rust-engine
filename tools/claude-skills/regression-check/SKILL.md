---
name: regression-check
description: Verify nothing regressed after changing the asset pipeline/converter, input, native dependencies, build scripts or game code, before saying something is fixed or ready to play. Use after any setup refresh, converter edit, dependency change, or when someone reports something "broke".
---

# Regression check

Run the sections that match what changed; when in doubt, run all. Report each result plainly (pass / fail / not
run); don't claim "fixed" from partial checks.

Why this exists: a spawn-rule change for the skate parks once silently moved the **city** maps' startup spawns
underground, making the map look invisible. It passed unit tests and `--check-assets`; only comparing every map's
spawn against a baseline would have caught it.

The check scripts named below (`check_spawns.py`, `check_maps.py`, `check_customiser.py`, `smoke_maps.sh`) are in
`tools/regression-checks/` (in the repo since upstream PR #37, "Tools: published research and regression helpers",
merged). Their baselines are made from YOUR install on the first run (`--update`), so they
never ship game data.

## 1. Converter / asset-pipeline changes (anything under `tools/`)
1. Unit tests: `py -3.13 -m unittest tools.asset_pipeline.test_map_writer tools.asset_pipeline.test_versions
   tools.test_setup_assets tools.test_setup_refresh` (plus `tools.asset_pipeline.test_setup_recovery` where present).
2. Which groups will rebuild: `changed_groups(installed(Path('data'))[1]['pipelines'], fingerprints())` (skill
   `asset-setup`). Make sure that's what you expect.
3. After the refresh: re-point `assets\` to the new id in `data\installation.json`.
4. **Every map, not just the one you changed:**
   - `check_spawns.py`: baked spawns vs the spawn baseline. Any `CHANGED` must be explained and intended; then
     `--update`.
   - `check_maps.py`: ONE `skate3rust --validate-maps` process checks TEST_WORLD + every map in seconds: fails on
     errors, on any warning (spawn support, neutral-startup settle, invisible collision floor) and on any change in
     loaded collision triangles vs the collision baseline. Falls back to `--check-assets` per map on an older exe.
     Run it after ANY collision/loader/converter/physics change; a count change on a map you didn't mean to touch
     is a regression. `--update` accepts intended changes.
   - `summary(install)` from `tools.asset_pipeline.optional_content`: expect 0 warnings (all roster characters).
5. If spawns or collision changed: load the affected maps AND one city map in game (startup uses the baked spawn;
   the saved default map is in repo-root `settings\default-map.json`).

## 2. Game code / input changes (`crates/`)
1. `cargo test -p skate-game --release --bin skate3rust -- <module>::` for touched modules (input: `input::`), plus
   `cargo test -p skate-core --release` if core changed. ALWAYS also compile the full test target
   (`cargo test -p skate-game --release --bin skate3rust --no-run`): a normal build passes while tests that construct
   structs literally fail on a new field.
   Know the pre-existing upstream failures (section 3c) so you can tell them from new ones.
2. Build + stage: `scripts\Build.ps1` (skill `build-and-run`).
3. Smoke test (about 20 s window): launch `bin\skate3rust.exe --assets <repo>\assets` with logs redirected; grep for
   `panic`, `ERROR`, `Controller`, `SKATE_RENDER_READY`, `physics_failed:true`.
   - Input: expect `Controller input: SDL <version>`, `Controller 0: SDL gamepad "<name>" ...`, then
     `Controller 0: ready`.
   - Rendering: `SKATE_RENDER_READY draws=... triangles=...` should match the map's `render_triangles`.
4. Close the game with a normal window close (for example PowerShell `CloseMainWindow()`), not a hard kill: after a
   kill, controller rumble can stay on.

### 2a. Frame time before merging game code
Run the staged release exe on a fixed spot and compare with the previous build: `--assets <install>\assets --map
<install>\maps\DownTown.skate --mute` for about 60 s with `SKATE_TRACE_ALL=1` (living world branch and later); the
frame log path is in the stderr `TRACE_ACTIVE` line (`frames.tsv` in the run's trace-all folder; elsewhere use
`SKATE_FRAME_LOG=<file>`). Compare median / p90 / p99 `frame_ms` of frames 100 and later with
`optimisation/tools/frame_log_summary.py`. `SYSTEM_TIMING` (on under trace-all) gives a per-system breakdown.
A Chrome trace: `--trace <file.json> --trace-seconds 30` (capture starts at launch: `--trace-delay` is accepted but
ignored; a 30 s trace is several GB, so write it to a large drive). Tracing slows every frame and hides steady-state
stalls of 30-70 ms: use it for costs, not for finding those stalls.
Before calling a slow play session a regression, check whether a build or other heavy job ran on the machine at the
same time (build output timestamps): one overlapping link turned a 3.6 ms median into 14 ms.

### 2b. Water (physics/water.rs, camera/water.rs, water_splash.rs, board_world water code)
- Headless traces (ignored test, prints per-tick body/board/camera):
  `SKATE3_ASSET_ROOT=<repo>\assets SKATE3_WATER_MAP=<installation>\maps\University.skate
  SKATE3_WATER_DROP=340.3,69.94,-294.3 cargo test --locked -p skate-game --release --bin skate3rust -- --ignored
  --nocapture water_drop`
  Expect: University channel (shallow): lowest body part about 67.996 on a 67.94 surface, calm (settled mean part
  speed about 0.06 m/s), board on the surface. DownTown Aletown canal (`SKATE3_WATER_DROP=-182.3,10.93,465.9`,
  DownTown map, deep): body floats (parts about 8.5-9.0 vs 8.93), board about 8.91, camera about 3 m above. Extra
  env: `SKATE3_WATER_NEAR=x,z,r`, `SKATE3_WATER_SCAN=1`, `SKATE3_WATER_DEPTH_MAP=1`. Build env paths with forward
  slashes in bash.
- Retail reference: shallow water is solid (lie on it); deep water floats; the board floats; water camera high and
  behind with a vignette; splash only in deep water.
- Judge penetration by skeleton parts 1..23: part 0 ("root") does not collide.

### 2c. Audio (game_audio/, skate-audio, audio_*.py)
- `cargo test -p skate-audio --release --locked` and `cargo test -p skate-game --release --bin skate3rust --
  game_audio` (plus `graphics_menu`, `modding::audio` if settings/listener changed).
- `py -3.13 -m unittest tools.asset_pipeline.test_audio_formats tools.asset_pipeline.test_versions
  tools.test_setup_assets`.
- Muted smoke run (`bin\skate3rust.exe --assets assets --mute`): the `Game audio: native ...` start lines, no
  `panic`, `ERROR` or missing-file warnings.
- Fingerprints (section 3d): an audio change must change only the `audio` group; never the customiser.
- Physics must be untouched: audio observation code is observation-only; when in doubt run `check_maps.py`
  (identical results).
- Sound quality itself is judged by people listening in game (skill `audio-tuning`), never by an agent.
- Audio MODDING changes: the automated in-game check (skill `audio-autotest`).

## 3. Native dependency / build-script changes
- Build both link configurations (skill `vendor-patch`): dev `cargo build -p skate-game --bin skate3rust --release`
  and release-style `+crt-static --no-default-features --target x86_64-pc-windows-msvc`.
- `llvm-readobj --coff-imports` on the static exe: no `vcruntime140.dll` / `api-ms-win-crt-*`.
- `--locked` must work (lockfile updated after `[patch]` edits).

## 3b. Performance changes must be behaviour-preserving
Optimisations that silently change behaviour are regressions. For any speed-up (details: skill `optimisation`):
1. Capture a behavioural baseline BEFORE the change, for example full `--validate-maps` results per map (collision
   triangles, support_drop, grounded_ticks, warnings; drop `seconds`/`timings`) to JSON.
2. Keep the old implementation as a test-only reference and compare old vs new on synthetic edge cases AND on the
   real private data (ignored test with `SKATE3_ASSET_ROOT`).
3. After the change, diff the same JSON: it must be identical.
4. Watch for hidden state: caches keyed by path go stale (custom difficulty overlays collections at runtime); reset
   caches on every mutation.

Ready-made behavioural comparisons:
- Maps: `check_maps.py` (validator results + collision baseline) and `check_spawns.py`.
- Character customiser: `check_customiser.py --save` BEFORE the change (snapshots receipts of all output files +
  JSON), then `check_customiser.py` after a fresh setup; set-id-aware, must print IDENTICAL. Needed because a fresh
  setup deletes the previous installation.
- Parallel customiser stages fall back to serial when a worker fails, so the output still matches but the speed-up
  is gone. Check the setup log for `Customiser worker N failed`; there should be none.
- Map conversion: convert all districts with `main`'s tools (`git archive main tools` into a temp folder) and with
  the new tree into the same scratch stage, hash every output (map, irradiance, props) and require IDENTICAL per
  district.
- Stream decoding: `tools/setup-equivalence/compare_streams.py <reference skate3_streams.py>`: every asset
  of every district/stream vs a reference copy of the loader (`git show HEAD:...skate3_streams.py > .local/ref.py`).

## 3d. Fingerprint impact (do this for EVERY tools/ change)
- Compare against the COMMITTED tree, never only your local install (it may already contain your edit):
  `git archive HEAD tools` -> temp dir; compare `versions.fingerprints()`, `customiser_setup.fingerprint(old_tools)`
  and `customiser_cache.versions(old_tools)` with the working tree.
- Asset groups: equivalence pairs `[committed, final]` in `pipeline-equivalence.json` only for groups whose outputs
  are proven identical.
- Customiser: has NO equivalence mechanism; its fingerprint/stage versions must not change unless outputs really
  change. `optional_content.py`, `setup_state.py`, `vlt.py`, `environment.py`, `owned_game/**`, `vendor/utt/**`,
  `customis*.py`, `native_roster.py`, `character_glb.py`, `mixamo_to_skate/*.py` are in it: wrap instead of editing
  where possible.
- Fingerprint-free places: `install.py` bodies except `extract`/`convert_map` (and no new top-level imports),
  `tools/setup.py`, new modules whose names match no glob (not `map*`, `character*`, `customis*`, ...).

## 3c. Is a failing test mine or upstream's?
Run it on untouched upstream in a throwaway worktree (separate target dir), never by editing tests:
```powershell
git worktree add -q .local\upstream-check <upstream main commit>
# bash, from inside it:  cargo test --locked -p <crate> --release --target-dir ../upstream-target -- <names>
git worktree remove --force .local\upstream-check; git worktree prune   # from the MAIN checkout (cwd inside = Permission denied)
```
Keep a list of known pre-existing upstream failures with your work notes.

## 4. Reading a play session's log
Latest log: `logs\game-*.stderr.log` (strip ANSI: `sed 's/\x1b\[[0-9;]*m//g'`).
- Falling from spawn: `requested:PhysicsGround->PhysicsAir` about 9 ticks after `player teleport spawn`, never
  landing; respawn `Teleporting` loops to the same point.
- Under the map: spawn y below the street level of a city (the baseline check catches it); world "invisible",
  collision present.
- Physics blow-ups: `physics_failed:true`, `NONFINITE`, `Non-finite`.

Record new facts and baseline updates in your notes file.


## 5. Getting to a spot: teleport there
Use coordinate teleports in tests and research runs to reach an issue spot, instead of menu teleports, scripted
skating or asking a player to drive there.
- Retail (recomp): `SKATE3_TP_OVERRIDE="x y z yaw[; ...]"` (skill `recomp-scripted-runs`, "Teleport to any
  coordinate"). Add about 4 m to an estimated ground y and read the settled height from the trace; points outside the
  playable area put retail into its out-of-bounds re-place loop (skater near invisible, frozen).
- Engine: `sdk.player.teleport(...)` / `sdk.camera.set(pos, look_at)` from a tiny test mod with `--verify` captures
  (skill `build-and-run`), or `--teleport <id>`.
- Use the same pose in both for matched retail vs engine shots, and save the poses next to the evidence.
- Upstream's original spawns, for reference: DownTown `[0.6737, 45.8373, 0.9177]`, University
  `[330.0, 133.005, -710.0]` (all maps: `docs/hails-additions/04-map-spawns.md`).
