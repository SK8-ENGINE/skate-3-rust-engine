<p align="center">
  <img src="docs/images/skating-crab.png" alt="Rust crab riding a skateboard" width="480">
</p>

# Skate 3 Rust Engine

A Rust and Bevy skating project built from Skate 3 reverse-engineering research.
Includes skating, tricks, grinds, offboard movement, difficulty settings and
`.skate` map support. Gameplay parity is still a work in progress.

## Play

ISO setup is under development. Release packaging is paused while the asset
converter is changed to work without Blender. Use a prepared local asset set
for now.

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
installed asset directory. The Windows release workflow is present, but
packaging is disabled until direct ISO conversion is ready.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.
