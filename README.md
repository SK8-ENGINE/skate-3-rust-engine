<p align="center">
  <img src="docs/images/skating-crab.png" alt="Rust crab riding a skateboard" width="480">
</p>

# Skate 3 Rust Engine

A Rust and Bevy skating project built from Skate 3 reverse-engineering research.
Includes skating, tricks, grinds, offboard movement, difficulty settings and
`.skate` map support. Gameplay parity is still a work in progress.

## Play

Download the Windows release ZIP, extract it and run `skate3rust.exe`.
Select your own Skate 3 Xbox 360 ISO when prompted. Setup prepares the skater,
animations and disc maps locally, then starts University. The first setup needs
internet access for conversion tools and can take a while.

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
first-run installation above. `Build-Release.ps1` builds the portable Windows
package with its setup helper; it also requires Python 3.13. Published GitHub
releases build and attach this package automatically.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.
