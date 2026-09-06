# Imported skater project

Double-click **PLAY.bat** to launch. Use an XInput-compatible controller;
**Esc** opens the pause/graphics menu. Startup selects the supplied **Easy** profile.

The menu supports window resolution (720p through 4K), internal render scale
(25–100%), supported MSAA levels (Off/2×/4×/8×), an FPS cap (30–240 or
Unlimited), and GPU occlusion culling (On/Off). Click a row to cycle it, or use arrow keys and Enter. Changes apply
immediately and save to `settings/graphics.json`. Escape or Resume returns to
gameplay; Quit game exits. The menu stays at the window's native resolution.
Occlusion culling defaults to On and skips meshes hidden behind other geometry.
It needs no changes to `.skate` files. Its benefit depends on the view; compare
On/Off where you see dips. It does not reduce visible mesh detail or collision.

The standing/rolling **360 flip** now uses the supplied gesture files and
ActionGraph/MotionGraph route, including right-stick crouch preparation.
See [360-flip integration and visual checks](docs/360-FLIP.md).

Double-click **PLAY-MAP.bat** to choose a `.skate` map, or drag a map onto
either launcher. `maps/private/` contains local copies of the transition park
and parity grid. `maps/format-demo.skate` is a small original textured floor.
No-map PLAY.bat still loads the supplied test environment.

New San Vanelona now loads its embedded `RWCM` collision archive, including
native edge flags and cluster query filtering. The staged build was checked
with the actual map and rendered the skater on its streets. No separate
collision sidecar is needed. See [map support notes](docs/SKATE-MAPS.md) for
verification and remaining sky/grind limitations.

This folder is a **local Git repository**, on `lol/skate-map-support`, with no
GitHub remote. Private assets, private maps, binaries, logs and build outputs
are excluded. See [the map support notes](docs/SKATE-MAPS.md) for supported
versions and the explicit runtime limitations.

This folder is separate from the earlier clone. It contains the contents of
`crates (1).zip` and `assets.zip`, a reconstructed Cargo workspace, and launch
scripts. The supplied stock animation banks, decoded frames, graphs, skater
mesh and textures are used together. No previous-clone animation code was
needed for setup.

The supplied scene contains a small test environment. Gameplay quality and
Skate 3 parity remain for the user to assess; compiling and starting the
project do not establish those claims.

## Observed runtime issue

The later bump crash (tick 4159, behaviors 3623/3624) is repaired: the host now
supports the stock `B_BUMP` blend space and `SetBumpCoefficients`. See
[the fix and verification notes](docs/BUMP-CRASH-FIX.md). Visual confirmation
remains with the user; the separate runout issue below is still open.

The build launches and reports `GAME_CHARACTER_READY bones=35`. All 35 skin
bones match the supplied stock rig; the decoded frame data contains 487 clips.
However, the imported motion-graph host exits when it encounters
`AddRunoutAttribs` (behavior 86). Both startup sessions recorded that exact
unsupported-behavior error. This is an implementation gap, not a missing
animation-file error. It has not been bypassed or replaced with guessed behavior.
See `logs/game-20260906-033940.stderr.log` for the character-ready message
and subsequent failure. No automated gameplay or screenshot test was run.

## Files

- `crates/`: imported Rust source. The two startup profile selections were
  changed from `normal` to `easy`.
- `assets/private/`: supplied assets; keep these private.
- `bin/`: staged executable and its exact Rust/Bevy runtime DLL dependencies.
- `logs/`: per-launch output and error logs.
- `Cargo.lock`: resolved dependency versions for reproducible rebuilds.
- `SETUP-PROVENANCE.json`: original ZIP hashes and setup changes.

**BUILD.bat** rebuilds and stages the executable. It requires Cargo/Rust and
LLVM `llvm-readobj` at the installed Windows location. The initial build
reused the earlier project's compilation cache; PLAY.bat uses only this
folder's staged binary, libraries and assets. Subsequent default builds use
this folder's own `target/` directory.

If startup fails, the launcher keeps the console open and shows the error
log path. Do not substitute animation files to silence a missing-data error;
record the exact error first.
