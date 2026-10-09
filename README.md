<p align="center">
  <img src="docs/images/skating-crab.png" alt="Rust crab riding a skateboard" width="480">
</p>

# Skate 3 Rust Engine

A Rust and Bevy skating project built from Skate 3 reverse-engineering research.
Includes skating, tricks, grinds, offboard movement, difficulty settings and
`.skate` map support. Gameplay parity is still a work in progress.

## History

Before this rewrite existed, **dumbad** spent more than two years reverse
engineering Skate 3 and building the tools needed to understand and work with
it. That meant countless hours digging through undocumented file formats,
animation data, and game interaction systems, then testing those discoveries
in the original game. Much of that work is collected in
[DumbadsSkate3ModdingTools](https://github.com/Ethanw05/DumbadsSkate3ModdingTools),
including tools for custom maps, meshes, collision, challenges, and DLC.

That research laid the groundwork for this project. Chasm later worked on a
recompilation and a custom renderer based on dumbad's earlier renderer work,
before moving into the Rust/Bevy rewrite. The rewrite's development time tells
only part of the story: the knowledge and tools it relies on took years of
work to establish.

## AI usage

AI coding tools were used to develop this rewrite, but none of it would have
been possible without dumbad's extraordinary effort to reverse engineer the
original game. The AI had years of hard-earned research and working tools to
build on. Describing the project as simply “AI rewriting Skate 3” leaves out
the work that made it possible in the first place.

AI helped turn that knowledge into a new implementation; it does not replace
credit for discovering how the game works. This is still a work in progress,
and using original assets or showing working tricks does not mean every
system behaves exactly like the original.

## Play

[Download Experimental](https://github.com/SK8-ENGINE/skate-3-rust-engine/releases/tag/experimental).
Successful `main` builds replace this prerelease. Choose **Latest** in Updates
for experimental updates; **Stable** is the default.

Extract the Windows release ZIP and run `skate3rust.exe`. Select your Skate 3
Xbox 360 ISO, or select `default.xex` in an extracted game folder. Keep its
`data` folder alongside it. Setup prepares the skater, animations and all disc maps, then
starts University. The original scoring and session-marker HUD assets are also
exported automatically during setup. No Blender, Python or Rust installation is needed.
ISO extraction needs internet access. The first conversion can take a while.

Use a compatible gamepad to play. SDL3 supports Xbox/XInput, PlayStation,
Switch and generic HID controllers; XInput remains available as a Windows
fallback. Escape opens graphics, difficulty and map settings. Maps can be
switched without restarting the game.

**Skate 3 assets are not included.** Your converted files stay in
the `data` folder beside your executable. Each freshly unpacked copy runs its
own setup; it does not adopt another installation. In-place updates refresh
only changed asset groups.

## Build

On Windows, install Rust with the MSVC toolchain and LLVM in its default
location. Run `BUILD.bat` to build, then `PLAY.bat` to launch the test world.
`PLAY.bat` opens your saved map (University by default); use the in-game menu to switch maps, or drag a `.skate` file onto `PLAY.bat`. An SDL3-compatible gamepad is required for gameplay;
Escape opens difficulty and graphics settings.

On Apple Silicon macOS and on x86_64 or aarch64 Linux, `nix develop` (Nix with
flakes enabled) provides Rust, the C/C++ compiler and linker, CMake for SDL3,
Python with the setup packages, extract-xiso, vgmstream, and the runtime libraries.
`flake.lock` pins these dependencies. On macOS it also supplies the Apple SDK:
engine builds use Nix's Clang and Mach-O linker, not the system Command Line
Tools or whichever SDK `xcrun` happens to select. Xcode-only profiling tools
remain optional system tools; run them outside this shell.

```sh
nix develop
skate-build
skate-setup path/to/Skate3.iso  # Once; also accepts an extracted default.xex
skate-run                     # Builds if needed, then launches installed assets
```

If assets are already installed, skip setup. `skate-setup` builds the engine
before conversion and refreshes an existing installation. `skate-assets` prints
the prepared asset directory: `SKATE3_ASSETS` takes precedence, then checkout
`assets/`, then the installation recorded in `data/installation.json`.
Relative overrides are resolved from the checkout root.
`skate-run` forwards game arguments, for example `skate-run --test-world`.
These commands also work from checkout subdirectories, or without entering an
interactive shell: `nix develop -c skate-run`.

Build and run inside the shell: the game needs its Vulkan libraries and dynamic
Rust/Bevy libraries. Linux still requires a working graphical session and GPU
driver on the host. On macOS the renderer runs Vulkan through MoltenVK with
non-bindless materials; the shell disables MoltenVK fast-math, which otherwise
corrupts customiser skaters. No devenv, direnv, or system SDK override is needed.

When switching from a different compiler, an existing SDL CMake cache may reject
the change. Run `cargo clean -p sdl3-sys` once inside the shell, then `skate-build`;
the rest of the Rust build cache can stay.

Development builds use a prepared asset set in `assets/private/` or the
installed asset directory. `scripts/Build-Release.ps1` builds the portable Windows
package and requires Python 3.13. GitHub Actions builds `main` automatically;
numbered releases are published separately.

Custom animations and climbing support remain available, but no custom clips
are shipped. The included format-demo map is original procedural content.

Implementation notes are in [`docs/`](docs/). Patched Bevy dependencies and
their licenses are in [`vendor/`](vendor/). This is an unofficial project,
not affiliated with EA.

## Advanced diagnostics

Windows builds support opt-in [performance timeline capture](docs/performance-tracing.md)
through the `--trace` CLI option, including optional GPU pass diagnostics.

## License

Copyright (c) 2026 dumbad and the Skate 3 Rust Engine contributors.
Unless otherwise noted, this project's original code is licensed under the
[GNU General Public License version 3 only](LICENSE) (`GPL-3.0-only`).
You may use, modify, and distribute it, including commercially. If you distribute
a modified version or a binary of the covered software, you must also make its
corresponding source available under GPLv3 and preserve the required notices.

Third-party code retains its existing licenses and copyright notices, including
the vendored Bevy crates and tooling under `tools/vendor/`. This license does
not grant rights to Electronic Arts' game code, data, assets, or trademarks,
or to content supplied by other map and mod authors.
