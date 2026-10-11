# Web port

Goal: the game playable in a desktop browser from a `wasm32-unknown-unknown` build on Bevy's WebGPU
backend, served by a small Rust web server that also hosts multiplayer lobbies and relays. The native
Windows build stays exactly as it is: every web change sits behind `cfg(target_arch = "wasm32")`, a
target-specific dependency or a Cargo feature.

Ground rules (from the porting brief):

- **No Electronic Arts data is bundled, committed or served publicly.** Players bring their own
  converted installation. The server distributes engine code and original project content only
  (`maps/format-demo.skate` is procedural). The one exception, an operator's own asset mount for
  personal use, is opt-in, binds to localhost and needs a token.
- **The served `.wasm` is a GPL-3.0-only binary distribution.** The loader page links to the
  corresponding source for the exact commit (`SKATE_RELEASE_REVISION`, embedded by
  `crates/skate-game/build.rs`).
- **Retail parity is untouched.** Every `f32::mul_add` stays (no `a * b + c`, no relaxed-SIMD
  `madd`). On wasm it is a software `fmaf`: the same bits, measured cost. See
  [determinism.md](determinism.md).

## Milestones

The brief's milestone list did not come through, so this plan is the working proposal, with the
graphics upgrades the owner added interleaved (G rows; they serve the native and browser builds
alike). Each milestone is finished, verified, committed and reported before the next starts.

| # | Milestone | Status |
|---|---|---|
| 1 | Engine-independent crates build for wasm32; prove bit parity with native and measure `mul_add` | **Done** ([determinism.md](determinism.md)) |
| G1 | Optional graphics quality: supersampling, edge-adaptive upscaling (FSR 1), CAS sharpening, FXAA/SMAA, anisotropic filtering; defaults unchanged | **Done** ([../graphics/quality-settings.md](../graphics/quality-settings.md)) |
| 2 | `skate-game` compiles for wasm32 (WebGPU backend, canvas window, browser logging; Windows and process-level modules, SDL3 and file asset sources gated) | **Done** ([browser-build.md](browser-build.md)) |
| G2 | HD texture pack loader: replacement textures by name from a folder the player fills (nothing EA-derived shipped) | Next |
| 3 | Browser installation: the player chooses their own converted installation, read from their disk as the engine needs it; startup, maps and the asset server run from it | **Done** ([installation.md](installation.md)) |
| 4 | `skate-web-server`: serves the wasm build and loader page (source link, MIME, caching), opt-in localhost operator asset mount with token | Proposed |
| 5 | Multiplayer through the server: lobby directory and a WebSocket relay for `skate-net` snapshots | Proposed |
| 6 | Browser input and audio (Gamepad API in the XInput shape the game expects, Web Audio output for `skate-audio`), performance pass | **Done** ([input-audio.md](input-audio.md)) |

## Codebase map

Surveyed from the tree and checked while porting; corrections to the brief's survey are in bold.

### Workspace (`Cargo.toml`: edition 2024, resolver 3, Bevy pinned at `=0.18.1`)

| Crate | Role | wasm32 status |
|---|---|---|
| `skate-core` (~93k lines) | Physics, state graphs, input conversion, maths. No I/O. | Builds unchanged. Bit-identical with native ([determinism.md](determinism.md)). 1,417 `mul_add` outside test files. **Four gameplay paths call the platform libm** (`atan2` for `BoardAdjustAngle`, `tan` in the offboard air selector, `sin_cos` in the manual camera, an f64 `powf` in trigger volumes). |
| `skate-data` | Stock loaders, the `.skate` format, XEX reading. `flate2` and `zstd`. | Builds. **The C `zstd` builds for wasm32 through zstd-sys's wasm shim** (needs `clang`), and round-trips bit-exactly. Map timing uses `web-time` on wasm; texture decode runs serially there (no threads). Loaders read through `skate-fs`. |
| `skate-audio` + `skate-audio-fma` | Native port of retail AEMS audio. | Build unchanged; the FMA crate already falls back to its plain copy off x86-64. Bit-identical with native on both native copies. 56 + 26 `mul_add` outside test files. **Filter design and the panner, splice envelopes, zones and heading also call the platform libm.** |
| `skate-dynamics` | Prop and deformation dynamics on `rapier3d =0.35.3`. | Builds; its test suite passes unchanged as wasm. **`model.rs` wraps convex decomposition in `catch_unwind`, which cannot catch anything on wasm (panics abort)**: bad input must be rejected before it once the game runs in the browser (milestone 3). |
| `skate-mods` | Lua 5.4 mod VM (`mlua` vendored, `send`), `zip`, `gltf::import`. | Builds, without Lua: **`lua-src` refuses `wasm32-unknown-unknown`**, so `mlua` is a native-only dependency and `Vm` is an uninhabited type there whose constructor says mods need the native build. `Instant` is `web-time` on wasm. |
| `skate-fs` | **New.** The engine's file API: `std::fs` natively, the player's mounted installation in the browser ([installation.md](installation.md)). | Built for both; its tree is tested natively. |
| `skate-net` | Transport-neutral snapshot protocol: `lobby`, `directory`, `packed`, prediction, interpolation. | Builds. `socket` (the `socket2` buffer tuning) is native-only; `Instant` is `web-time` on wasm. Test suite identical as wasm (real-UDP tests excluded). |
| `skate-steam-relay` | Steamworks helper executable. | Excluded from web builds. |
| `skate-wasm-parity` | **New.** Native-vs-wasm bit parity probe ([determinism.md](determinism.md)). | Built for both. Not part of any Windows package (the build scripts select `-p skate-game` / `-p skate-steam-relay`). |
| `skate-game` (~121k lines) | The Bevy app: `skate3rust`, `skate3-multiplayer`, `skate3-custom-models`. Modules declared in `src/main.rs`. | `skate3rust` builds and starts on WebGPU from the installation the player chooses, with Gamepad API pads and Web Audio ([browser-build.md](browser-build.md), [installation.md](installation.md), [input-audio.md](input-audio.md)). |

Patched crates (`vendor/bevy_pbr`, `vendor/bevy_core_pipeline`, `vendor/sdl3-sys`, `vendor/cpal`) come
in through `[patch.crates-io]` and are explained in `vendor/README.md`. The Bevy patches must compile and
behave the same on WebGPU (milestone 2). The cpal patch changes only its Web Audio host (milestone 6).

### Startup path (`crates/skate-game/src/main.rs`, `app.rs`, `config.rs`, `setup.rs`)

- `main` loads everything synchronously from disk before building the `App`: `Config::from_env`
  (CLI args, env vars, `setup::asset_root()`, the saved default map), then `StockGameplayConfig`,
  `GameAssets` (`private/game.json`), `StockGraphs`, `GamePhysics::load_with_difficulty`,
  `SkaterRuntime` and `PlayerControls`.
- `setup::asset_root()`: `--assets`, `SKATE3_ASSETS`, or `current_exe()/data/installation.json`
  (which can spawn `support/skate3setup.exe`). An installation is `data/installations/<32-hex>/` with
  `assets/` (the asset root), `maps/*.skate` and `settings/*.json`, reached through
  `asset_root.parent()`.
- `app::build` forces `Backends::VULKAN` and `InstanceFlags::empty()`, opens a fixed 1280×800 window,
  and disables `LogPlugin` (`profiling.rs` owns tracing) and `GilrsPlugin`. It registers **three**
  `FileAssetReader` sources: `custom_models::register_source` adds `online-characters` (a cache) and
  the custom model directory, `modding::register_source` adds `mods`. `FileAssetReader` does not exist
  on wasm.
- Windows-only or process-level: `updater.rs`, `crash_report.rs` + `crash_native.rs` (supervisor
  process, `ntdll`), `profiling.rs` (Chrome trace file), `verification.rs` (`--verify` PNG capture),
  `frame_timing/log.rs` (log-writer thread), and the setup tool modes `--extract-ocean-pca` and
  `--extract-menu-tables`.

### Native baseline

Checked on x86-64 Linux at the start of the port (the Windows release build is unchanged by it):

- `cargo test --lib --tests` for `skate-core`, `skate-audio`, `skate-audio-fma`, `skate-dynamics`,
  `skate-net`, `skate-data`: 1,190 passed, 74 ignored, **1 failing before any web change**:
  `physics::skeleton_body::collision_feedback_tests::a_moving_group_8_body_reaches_native_impact_feedback_for_a_stationary_actor`
  (fails identically as wasm).
- **`skate-data`'s `apt_data` example does not compile** on the base commit (it includes
  `skate-game/src/apt_vm.rs` and calls a removed `Vm::construct`), so test runs use `--lib --tests`.

## Tooling

[`tools/web-port/`](../../tools/web-port/README.md): the browser package build and its headless
browser test, the parity probe runner and a Node WASI runner for the stock test suites.
