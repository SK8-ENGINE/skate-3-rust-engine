---
name: build-and-run
description: Build, test, and launch the skate3rust game on Windows. Use when asked to compile, run tests, play/launch the game, smoke-test a change in the real game, or check that a change builds.
---

# Build and run skate3rust

Prerequisites: Rust (MSVC toolchain), LLVM (default `C:\Program Files\LLVM`), CMake for the static SDL3 build
(`scripts/Ensure-CMake.ps1` finds Visual Studio's bundled copy). Keep machine-specific install facts in your own
notes file in your checkout.

## Build
- Release: `BUILD.bat` -> `scripts/Build.ps1` -> `cargo build -p skate-game --bin skate3rust --release`, then stages
  exe + DLLs + `manifest.json` into `bin/`.
- Dev (debug profile, still opt-level 3): `BUILD_DEV.bat` (`Build.ps1 -Dev`).
- From an agent, prefer Bash so cargo's stderr isn't treated as an error:
  ```bash
  cd "<repo>"
  export PATH="$HOME/.cargo/bin:$PATH" CMAKE="<path to cmake.exe, e.g. Visual Studio's CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe>"
  cargo build -p skate-game --bin skate3rust --release
  ```
  Then stage with `powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\Build.ps1` (don't hand-copy the
  exe; `bin\manifest.json` holds hashes).
  - Staging FAILS ("being used by another process") while a game runs from `bin\`: check `Get-Process skate3rust`
    first and never kill someone's running game. The cargo build itself still succeeds.
  - `Build.ps1` output redirected with `*>` is UTF-16; decode before grepping (`iconv -f UTF-16LE`) or check
    `$LASTEXITCODE` and `(Get-Item bin\skate3rust.exe).LastWriteTime`.
  - `target\release\skate3rust.exe` cannot run on its own (needs `bevy_dylib-*.dll` / `std-*.dll` beside it): run
    from `bin\`.
- Release-package link configuration (static CRT + static Bevy, what CI ships). Verify native dependency changes
  with it:
  ```bash
  CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static' \
    cargo build --release --locked --target x86_64-pc-windows-msvc -p skate-game --bin skate3rust --no-default-features
  ```
  Slow (full static Bevy rebuild, output in `target\x86_64-pc-windows-msvc\`); run it in the background. Required
  whenever a native (C/C++) dependency or its build flags change: dev builds use the DLL CRT, so CRT mismatches only
  show up here (`__imp_*` unresolved externals; see skill `vendor-patch`).
- `--locked` fails after editing `[patch.crates-io]` or dependencies: run `cargo update -p <crate> --offline` once.

## Test
- Rust: `cargo test -p <crate> --release` (scope it; skate-core and skate-game are large). `skate-data`: use
  `--lib --tests` (some of its examples may not compile upstream). Filter within skate-game:
  `cargo test -p skate-game --release --bin skate3rust -- input::`.
- Whole workspace: `cargo test --workspace --lib --bins --tests --no-fail-fast` (leave the examples out: some do
  not compile). Data-gated tests are `#[ignore]` with the reason in the attribute; run them with `-- --include-ignored`
  and the paths they name (for example `SKATE3_ASSET_ROOT=<repo>\assets`, `SKATE3_MAP=<installation>\maps\DownTown.skate`).
- Never put a short `timeout` on a test or build run: a compile killed mid-codegen leaves a broken incremental cache,
  and the next link fails with hundreds of unresolved `.llvm.*` symbols (`LNK1120`). Fix: delete only
  `<target>\debug\incremental\<crate>-*` and rebuild.
- Python tools: `py -3.13 -m unittest tools.test_setup_assets tools.test_setup_refresh` (setup-related), or
  `discover -s tools -p "test_*.py"`.
- Mods: `cargo run --locked -p skate-mods --example check_mod -- <mod-folder>`.
- Asset validation without a window: `bin\skate3rust.exe --assets assets --test-world --check-assets` -> prints
  `SKATE_ASSETS_READY`, exit 0.
- Many maps at once (on branches that have it): `bin\skate3rust.exe --assets assets --validate-maps`, then one
  `.skate` path (or `TEST_WORLD`) per stdin line -> one `SKATE_MAP_CHECK {json}` stdout line each (errors, warnings,
  collision_triangles, support_drop, grounded_ticks, visible_floor_gap, timings). Easiest via `check_maps.py`
  (skill `regression-check`).

## Run
- `PLAY.bat` (saved map, University default), `PLAY.bat path\to\map.skate`, or `PLAY.bat -trace [map] [file]`. Logs
  go to `logs\game-*.log`.
- Needs `bin/skate3rust.exe`, converted assets behind the `assets\` junction (skill `asset-setup`), and a controller
  for gameplay.
- **Screenshots without a player** (render/effect checks): `bin\skate3rust.exe --assets assets --map
  <installation>\maps\<Map>.skate --teleport <destination id from assets\private\teleports.json> --verify <out>.png`;
  `SKATE_VERIFY_AT=<seconds>` (default 4) sets when it captures, then it exits. With `SKATE3_MODS=<folder>` holding a
  tiny mod that calls `sdk.camera.set(pos, look_at)` or `sdk.player.teleport(...)` in `on_update`, you can frame a
  spot or trigger an event (for example a water drop) and capture several times. Keep such test mods in a gitignored
  folder (for example `.local/test-mods/`), not in `mods/`. A connected controller can open the pause menu during a
  capture: retry.
- Smoke test from an agent (opens a window briefly): start `bin\skate3rust.exe --assets "<repo>\assets"` with
  `SKATE3_MODS=<repo>\mods`, redirect stdout/stderr into `.local\`, stop it after about 15-25 s, then grep the log for
  `Controller`, `SDL`, `panic`, `ERROR`.
- Audio (upstream main since PR #32): add `--mute` to automated smoke runs (silences game + mod audio); master volume defaults to 25 %
  (`settings\audio.json`). Launch long play sessions from Explorer/cmd rather than inside Windows Terminal (a
  Terminal crash kills the attached game). Audio iteration: skill `audio-tuning`.
- Audio diagnostics switches: `SKATE_AUDIO_TRACE`, `SKATE_AUDIO_TIMING`, `SKATE_AUDIO_STATE_LOG`,
  `SKATE_AUDIO_SET`, `SKATE_AEMS_BANKS`, `SKATE_AUDIO_FMA`, plus world toggles `SKATE_AEMS_WORLD`,
  `SKATE_AEMS_WORLD_PREFETCH`, `SKATE_AEMS_NPC_SKATERS`.
- Input backend: SDL3 by default; `SKATE3_INPUT=xinput` for the old XInput path. SDL hints can be set as env vars
  (for example `SDL_JOYSTICK_HIDAPI_XBOX_ONE=1`).
- Run only one game instance at a time (engine or recomp); two GPU-heavy instances can make a machine unstable.
- Hitches (living-world branch): every frame over the hitch rule logs a `FRAME_HITCH` line with the slowest stage, the physics phases, the
  living-world systems and the audio systems (`audio_parts`), so a reported stutter can be traced from the log alone.

## Getting to a spot: teleport there
Use coordinate teleports to reach an issue spot instead of menu teleports or driving there:
- Retail (the recomp research build): `SKATE3_TP_OVERRIDE="x y z yaw[; ...]"` (skill `recomp-scripted-runs`): each
  Challenge Map menu teleport lands at the next pose. Add about 4 m to an estimated ground height and read the settled
  height from the trace; points outside the playable area put the game into its out-of-bounds re-place loop.
- The engine: `sdk.player.teleport(...)` / `sdk.camera.set(pos, look_at)` from a tiny test mod with `--verify`
  captures (above), or `--teleport <id>`.
- Use the same pose in both for matched retail and engine shots, and keep the poses next to the evidence.

After running, record any new environment findings in your notes file.
