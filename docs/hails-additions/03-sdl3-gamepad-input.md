# 3. SDL3 gamepad input

Implements upstream issue #14 ("Extending Controller support via SDL Gamepad
API"), discussed in PR #9. The Windows XInput transport remains available as a
fallback and for comparison.

## Motivation

- One controller API on every platform instead of XInput on Windows plus a
  separate backend elsewhere (PR #9 uses gilrs on Linux/macOS).
- SDL3 supports far more controllers (PlayStation, Switch, generic HID) and
  exposes features XInput cannot (extra buttons such as back paddles, rumble
  on triggers, sensors, lights) for future use.

## Design

The gameplay path is unchanged: `skate_core::input::xbox::convert` (the TU3
conversion with its own dead zone) still receives an XInput-shaped
`XboxState` with raw values. Only the transport in
`crates/skate-game/src/input/platform.rs` changes.

- **Threading.** SDL must be pumped on the thread that initialised it, while
  Bevy runs systems on a pool. A dedicated `sdl-gamepad` thread owns SDL,
  polls every 1 ms, and publishes a per-slot snapshot behind a mutex; the
  game reads the latest snapshot each frame, exactly as `XInputGetState`
  returns the latest state.
- **Conversion back to XInput shape** (exact for XInput-backed devices):
  - buttons → XInput `wButtons` bits (`BUTTONS` table);
  - stick Y: SDL stores `~y` (positive down), so `xinput_y = !sdl_y` restores
    the original value without overflow at the extremes;
  - triggers: SDL expands XInput's byte as `b*257` over the full axis, then
    rescales to `0..=32767`; rounding to nearest (`(v*255 + 16383) / 32767`)
    returns the original byte for all 256 values.
- **Packet number** advances only when the state changes, mirroring XInput's
  `dwPacketNumber`. Device subtype is reported as `XINPUT_DEVSUBTYPE_GAMEPAD`.
- **Slots.** A new pad takes its SDL player index if that is 0–3 and free
  (SDL reports the XInput user index there, so the ring light matches),
  otherwise the first free slot.
- **Paddles.** Optional `settings\controller.json`,
  e.g. `{"paddles": {"right1": "a", "left1": "x"}}`, ORs a paddle into an
  XInput button bit (names in `platform::BUTTON_NAMES`).
- **Backend selection.** SDL by default. `SKATE3_INPUT=xinput` selects the
  original XInput transport (Windows), which is also the automatic fallback
  if SDL fails to start. The backend starts at `Startup` so the first
  gameplay frame is not stalled.
- **Diagnostics.** Each connection logs name, SDL gamepad type,
  vendor/product, paddle availability and SDL driver path, e.g.
  `Controller 0: SDL gamepad "Xbox One Elite 2 Controller" (XboxOne, vendor 045e product 0b22, paddles not reported, path "XInput#0")`.

## Files

- `crates/skate-game/src/input/platform.rs` — SDL backend, conversions,
  backend selection, paddle masks, unit tests.
- `crates/skate-game/src/input.rs` — `start_controllers` startup system
  (settings + early backend start); log wording.
- `crates/skate-game/src/app.rs` — comment on why Bevy's `GilrsPlugin` stays
  disabled.
- `crates/skate-core/src/input/xbox.rs` — `XboxState` derives
  `Clone, Copy, Debug, Default, PartialEq, Eq` (snapshots cross threads).
- `crates/skate-game/Cargo.toml` — `sdl3 = "=0.20.0"` with
  `build-from-source-static` (SDL 3.4.16, statically linked, no DLL to ship).
- `scripts/Ensure-CMake.ps1` (new) — finds CMake for the SDL build: uses
  `cmake` on PATH, else Visual Studio's bundled copy via `vswhere`, and sets
  `CMAKE`. Dot-sourced by `Build.ps1`, `Build-Release.ps1`,
  `build-multiplayer-test.ps1`. CI runners already have CMake.
- `vendor/sdl3-sys/` + root `Cargo.toml` `[patch.crates-io]` + `Cargo.lock` —
  static C runtime fix, below.

## Static C runtime (release/CI builds)

Release packages, CI and the multiplayer test build with `+crt-static` and
static Bevy. The first release-style build failed to link:

```
libsdl3_sys-*.rlib(SDL_stdlib.obj) : error LNK2019: unresolved external symbol __imp_modff ...
fatal error LNK1120: 16 unresolved externals
```

cmake-rs passes `-MT` for `crt-static`, but SDL's CMake project enables policy
CMP0091, which ignores runtime flags in `CMAKE_C_FLAGS` and applies
`CMAKE_MSVC_RUNTIME_LIBRARY` (default `MultiThreadedDLL`). CMake has no
environment variable for this, and a `CMAKE_TOOLCHAIN_FILE` environment
variable would put cmake-rs into cross-compile mode.

Fix: `sdl3-sys` 0.7.1+SDL-3.4.16 is vendored unmodified except `build.rs`,
which defines `CMAKE_MSVC_RUNTIME_LIBRARY` from `CARGO_CFG_TARGET_FEATURE`
(`MultiThreaded` with `crt-static`, otherwise `MultiThreadedDLL`; never the
debug CRT). Documented in `vendor/README.md`. Dropping the patch is possible
once cmake-rs or sdl3-sys handle CMP0091 themselves.

## Verification

- Unit tests: every trigger byte survives the SDL round trip; stick Y
  inversion is exact at `i16::MIN/MAX`; paddle masks and button names;
  existing controller tests (10 total in `input::platform` /
  `input::controllers`) pass.
- Dev build (`scripts/Build.ps1`) and release-style build
  (`+crt-static --no-default-features --target x86_64-pc-windows-msvc`) both
  link; the static executable imports no `vcruntime140.dll` /
  `api-ms-win-crt-*`. The dev build's SDL CMake cache shows
  `MultiThreadedDLL`.
- In play: Xbox Elite Series 2 over Bluetooth LE detected as
  `Controller 0 ... ready`; normal skating across several sessions.

## Known limitations / open questions

- **Elite paddles are not reported on Windows** with this setup. SDL picks its
  XInput driver for the pad ("XInput#0"); the XInput API has no paddles. The
  hints `SDL_JOYSTICK_GAMEINPUT=1`, `SDL_JOYSTICK_HIDAPI_XBOX(_ONE)=1` and
  `SDL_JOYSTICK_RAWINPUT=0` made no difference; with `SDL_XINPUT_ENABLED=0`
  no backend detected the Bluetooth LE pad. The paddle mapping works wherever
  SDL does report paddles.
- Controller type is detected but only logged; carrying it into the game (for
  button prompts, per-model defaults) is future work. Nintendo layouts keep
  SDL's positional mapping on purpose.
- Linux/macOS: the backend is platform-neutral but was only built and tested
  on Windows here.
- The SDL build adds about 1¾ minutes to a clean build and needs CMake.
