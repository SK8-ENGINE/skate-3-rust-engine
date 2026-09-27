# Linux build

The game builds and runs on Linux (glibc and musl). Vulkan is required;
wgpu uses the system Vulkan driver (Mesa RADV, NVIDIA, etc.).

## System dependencies

- wayland / libxcb / libxkbcommon (winit)
- alsa-lib (audio)
- libudev / eudev (gamepad enumeration via gilrs)
- clang + libclang (bindgen in dependency build scripts)

## Build

```sh
./BUILD.sh
```

Or directly:

```sh
cargo build --release -p skate-game -p skate-steam-relay -p skate-xiso
```

Notes:

- **musl**: rustup's musl target defaults to static-pie, but musl distros
  generally do not ship static wayland/alsa libraries. BUILD.sh detects musl
  via `ldd` and exports `RUSTFLAGS="-C target-feature=-crt-static"` to link
  dynamically against musl.
- **`dev-dynamic` (bevy dynamic linking)** is a default feature aimed at
  Windows dev builds. On Linux, and especially on musl, build with
  `--no-default-features` (Steam lobby support lives behind the default
  `steam` feature and is also dropped; direct UDP multiplayer still works,
  and the steamworks SDK only ships glibc binaries anyway).
- **Setup**: the packaged `skate3setup` helper is a PyInstaller executable
  built for Windows/glibc. On Linux run the pipeline with the system Python
  (`tools/setup.py`, needs numpy/pillow/tkinter). A `support/skate3setup`
  shim next to the game binary can simply exec it.
- **ISO extraction**: `skate-xiso` (workspace crate, xdvdfs-based) extracts
  Xbox 360 ISOs natively; the pipeline prefers it over the extract-xiso
  download. A pre-extracted game folder (`default.xex` + `data/`) also works.

## Gamepads

Non-Windows input uses gilrs with its default filters disabled (raw axes,
matching what the TU3 input converter expects). Controller layouts come from
an embedded copy of SDL_GameControllerDB, and users can override or extend
mappings via the standard `SDL_GAMECONTROLLERCONFIG` environment variable.
