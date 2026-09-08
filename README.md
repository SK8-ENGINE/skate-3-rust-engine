<p align="center">
  <img src="docs/images/skating-crab.png" alt="Rust crab riding a skateboard" width="480">
</p>

# Skate 3 Rust Engine

A Rust and Bevy skating project built from Skate 3 reverse-engineering research.
Includes skating, tricks, grinds, offboard movement, difficulty settings and
`.skate` map support. Gameplay parity is still a work in progress.

## Mods and agent authoring

Drop mod ZIPs into top-level `mods/` and use `PLAY-MARIO-KART.bat` for the integrated
mod build in this checkout. **Point your coding agent at [sdk/AGENTS.md](sdk/AGENTS.md)**
to make a mod. See [package structure](docs/mod-packages.md), [Lua API](docs/lua-modding.md),
[vehicle API](docs/vehicle-sdk.md), [multiplayer mod SDK](docs/multiplayer-mods.md) and [Mixamo workflow](docs/mixamo-vehicle-workflow.md).
Editable examples live in `sdk/examples/`; `tools/package_mod.py` validates and packages them.

## Play

Extract the Windows release ZIP and run `skate3rust.exe`. Select your Skate 3
Xbox 360 ISO, or select `default.xex` in an extracted game folder. Keep its
`data` folder alongside it. Setup prepares the skater, animations and all disc maps, then
starts University. No Blender, Python or Rust installation is needed.
ISO extraction needs internet access. The first conversion can take a while.

Use an XInput controller to play. Escape opens graphics, difficulty and map
settings. Loading another map restarts the game session.

**Skate 3 assets are not included.** Your converted files stay in
`%LOCALAPPDATA%/Skate3RustEngine`.

## Build

Requires Windows, Rust with the MSVC toolchain, and LLVM installed in its default
location. Run `BUILD.bat` to build, then `PLAY.bat` to launch the test world.
Use `PLAY-MAP.bat` to select a map. An XInput controller is required for gameplay;
Escape opens difficulty and graphics settings.

Development builds use a prepared asset set in `assets/private/` or the
installed asset directory. `Build-Release.ps1` builds the portable Windows
package and requires Python 3.13. Published GitHub releases build and attach
the package automatically.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.
