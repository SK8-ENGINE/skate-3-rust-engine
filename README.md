# Imported skater project

Double-click **PLAY.bat** to launch. Use an XInput-compatible controller;
**Esc** exits. Startup selects the supplied **Easy** profile.

This folder is separate from the earlier clone. It contains the contents of
`crates (1).zip` and `assets.zip`, a reconstructed Cargo workspace, and launch
scripts. The supplied stock animation banks, decoded frames, graphs, skater
mesh and textures are used together. No previous-clone animation code was
needed for setup.

The supplied scene contains a small test environment. Gameplay quality and
Skate 3 parity remain for the user to assess; compiling and starting the
project do not establish those claims.

## Observed runtime issue

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
