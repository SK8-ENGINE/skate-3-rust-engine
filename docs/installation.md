# Windows installation

`Build-Release.ps1` produces `target/skate3rust-windows-x64.zip`. Keep its
`support` directory beside `skate3rust.exe`. The game links Bevy and the MSVC
runtime statically; the setup helper bundles Python, NumPy, Pillow and Tcl/Tk.

On first launch, setup asks for a local Xbox 360 Skate 3 ISO. It downloads
the hash-pinned extract-xiso utility from XboxDev. Asset conversion runs in
the bundled setup helper. Blender is neither required nor downloaded.
No game files are downloaded or included in the package.

The conversion pipeline:

1. Extracts the disc into a new installation workspace.
2. Preserves native animation banks and state graphs from `miscload.big`,
   and the controller mapping from `miscboot.big`.
3. Reads the big-endian 64-bit AttribSys schema and collections from `db.big`.
   Settings are decoded from the disc, with numeric hashes retained for names
   absent from debug strings. The JSON export serves the fields consumed by
   this engine; it is not a general VLT editor or portable array serialization.
4. Builds the default modular skater from the CAC archive, including textures,
   morph settings, the native rig and its four board-relative IK targets.
   Writes GLB directly with retail skin weights, inverse-bind matrices and
   the bone-local basis used by the runtime. Gameplay animations are evaluated
   from ABIN by the Rust runtime, without offline animation baking.
5. Converts each `worldDIST_*.big` directly into a v14 `.skate` map. Native
   collision archives and grind splines are embedded; collision triangles are
   decoded from that archive by the game rather than duplicated in the file.
   University uses the existing starting location. Other
   districts select a broad upward collision face near the district origin.
6. Runs the game's asset and map loaders before publishing `installation.json`.
   This validates loading, not gameplay. Conversion intermediates are removed
   after their output succeeds. Failed workspaces retain logs for diagnosis.

Data lives under `%LOCALAPPDATA%/Skate3RustEngine`. `installation.json` points
to the completed installation. Conversion tools are cached separately, so
retrying does not download them again. `setup-error.log` reports setup errors;
each installation also has `setup.log` with converter output.

For an already extracted disc, developers can use `tools/prepare_assets.py`
with `--game-root`, `--output` and `--game-exe`. The release helper can run the
same entry point with `--task tools/prepare_assets.py`.

The Escape menu discovers installed `.skate` files. Loading a map starts a new
game process and closes the current session so physics and render resources
are recreated. A fresh launch defaults to University. Development checkouts
with `assets/private/game.json` retain their existing local assets and default
test world; `--assets DIRECTORY` also selects a prepared asset set explicitly.
