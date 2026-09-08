# Windows installation

`Build-Release.ps1` produces `target/skate3rust-windows-x64.zip`. Keep its
`support` directory beside `skate3rust.exe`. The game links Bevy and the MSVC
runtime statically; the setup helper bundles Python, NumPy, Pillow and Tcl/Tk.

On first launch, setup asks for a local Xbox 360 Skate 3 ISO. It downloads
hash-pinned portable Blender and extract-xiso distributions from their
publishers. No game files are downloaded or included in the package.

The conversion pipeline:

1. Extracts the disc into a new installation workspace.
2. Preserves native animation banks and state graphs from `miscload.big`.
3. Reads the big-endian 64-bit AttribSys schema and collections from `db.big`.
   Settings are decoded from the disc, with numeric hashes retained for names
   absent from debug strings. The JSON export serves the fields consumed by
   this engine; it is not a general VLT editor or portable array serialization.
4. Builds the default modular skater from the CAC archive, including textures,
   morph settings, the native rig and its four board-relative IK targets.
   Gameplay animations are evaluated from ABIN by the Rust runtime.
5. Converts each `worldDIST_*.big` into a `.skate` map. The supplied map exporter
   runs in v14 storage mode, with native collision archives and grind splines
   embedded. University uses the supplied exporter's starting location. Other
   districts select a broad upward collision face near the district origin.
6. Runs the game's asset and map loaders before publishing `installation.json`.
   This validates loading, not gameplay. Conversion intermediates are removed
   after their output succeeds. Failed workspaces retain logs for diagnosis.

Data lives under `%LOCALAPPDATA%/Skate3RustEngine`. `installation.json` points
to the completed installation. Conversion tools are cached separately, so
retrying does not download them again. `setup-error.log` reports setup errors;
each installation also has `setup.log` with converter output.

The Escape menu discovers installed `.skate` files. Loading a map starts a new
game process and closes the current session so physics and render resources
are recreated. A fresh launch defaults to University. Development checkouts
with `assets/private/game.json` retain their existing local assets and default
test world; `--assets DIRECTORY` also selects a prepared asset set explicitly.
