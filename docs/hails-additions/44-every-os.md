# 44. Every OS: Windows, Linux and macOS

Status: in progress on `world/challenges` (PR #68), started 2026-11-10.

## Goal

The engine, its setup, its tools and the couch launcher run on every OS we can support: Windows, Linux and macOS
first. Every new change is built that way (ours, not retail: the game was a console title).

## Imported work (2026-11-10)

Two existing ports were merged on branch `platform/linux-macos`, credited to their authors:

- **Upstream PR #7 by dylan-gluck** (`feat/add-mac-support`, 9 commits): Apple Silicon macOS (wgpu through MoltenVK,
  non-bindless materials), a Nix development shell for macOS and Linux (`flake.nix`), setup tools found on `PATH`
  outside Windows, windowless setup (`tools/setup.py --source <iso|default.xex>`), a static link build, and checkout
  instructions in the README. Its author verified macOS build, setup and rendering on an M3 Pro.
- **stev0760's `linux-support`** (1 commit): `BUILD.sh`, `SETUP.sh` and `PLAY.sh` beside the Windows `.bat` files,
  the pinned Linux builds of extract-xiso and vgmstream, `librefpack.so` for the fast refpack decoder, the window
  icon only on Windows. Its author reports a full setup from a retail ISO and all 10 maps on Arch Linux (NVIDIA,
  Vulkan, Wayland).

Where the two met (2026-11-10, merge `4583fe96`):

- `tools/asset_pipeline/install.py` `dependency()`: on Windows the pinned download; elsewhere a tool on `PATH` wins
  (Nix shell, distro package), else the pinned build for the platform, else an error naming the tool.
- `tools/setup.py`: both added the windowless `headless()`; #7's is kept (it refreshes when an install exists).
- `tools/asset_pipeline/test_dependency.py`: #7's "tool on PATH" test only worked off Windows (Windows needs an
  extension to find a program); it now runs on every OS. 3 tests pass on Windows.

## Our changes made OS-independent (2026-11-10)

- UI fonts (`crates/skate-game/src/ui_fonts.rs`): the music caption uses a system UI font on every OS (Segoe UI;
  San Francisco or Helvetica; Noto Sans or DejaVu Sans) instead of Bevy's monospace, and titles in other scripts load
  the system's script fonts: the Windows list as before, on Linux and macOS the Noto / DejaVu / Apple script faces
  under the system and user font folders (160 MB cap).
- Custom Music (`mods/custom-music`): `ADD-MY-MUSIC.bat` (Windows) and `add-my-music.sh` (Linux, macOS; folder
  picker through zenity, kdialog or the macOS dialog, else a typed path); README steps for each OS.
- `.gitattributes`: every `.sh` is stored with LF, every `.bat` with CRLF.

## First-time setup on Linux and macOS (2026-11-10)

`tools/steam-launcher/setup.sh` is the twin of `setup.ps1`: the same check lines (id, name, ok, detail, required)
for Git, the compiler / CMake / patchelf, the Linux build libraries, Python 3 with Tk, Rust, the GitHub CLI and
Vulkan (MoltenVK on macOS); `--install` installs everything missing in one step through apt, dnf, pacman, zypper or
Homebrew (one password prompt through sudo), then Rust for the user through rustup (never as root). On SteamOS it
explains that the system is read-only and points to the prebuilt versions or Distrobox. Python's own packages
(numpy, Pillow) come from setup's venv bootstrap, which now finds the venv's Python on every OS. The Rust launcher
will show it the same way as the Windows setup page. All of the launcher's features are ported (scope of the
rewrite below); the doc 44a parity checklist decides when it replaces the old one.

## Fork builds and badges (2026-11-10)

- `.github/workflows/rusty-trucks-linux.yml` (Ubuntu 24.04, x64) and `rusty-trucks-macos.yml` (macOS, latest runner, Apple
  Silicon) run on the fork (`Hailey-Ross/rusty-trucks`) for every push to `hails-additions`, its releases and manual
  runs; both call `rusty-trucks-unix-build.yml`: the Linux build libraries (flake.nix's list as Ubuntu packages), the
  setup dependency tests, the game build without dynamic linking, the skate-core / skate-audio / skate-data and
  skate-mods library tests, the native RefPack library, and the binaries as a 7-day run artifact. They check the
  repository name, so they do nothing upstream. Release packages for Linux and macOS are still open.
- The README shows a badge per OS: the existing Windows workflow (`rusty-trucks-win.yml`) and the two new ones.

## Couch launcher on every OS (USER-ASKS 561)

Decision (2026-11-10): the launcher (`tools/steam-launcher`: the WinForms window `SkateLauncher.cs` and the
PowerShell backend `launcher.ps1`) is rewritten as one Rust app (egui window with the camcorder look of doc 41, the
controller through SDL3, the backend actions in Rust). It reads the same state folder and `config.json`, so existing
setups carry over. The current launcher keeps working until the new one matches it.

Steps: (1) the spec below; (2) the skeleton (window, controller, state folder); (3) the backend (versions, builds with
progress, play, session checks, DLC import, Refresh Game Files, clear builds, size cap; tool installs through winget,
apt / dnf / pacman, Homebrew); (4) the camcorder look with the snapshot and bench modes; (5) a parity checklist on
Windows, then Linux and macOS runs, then the Steam shortcuts move over.

### Progress

- 2026-11-10, step 2 started: crate `tools/skate-launcher` (workspace member; `jiff =0.2.42` for local times on every
  OS). It finds the launcher folder and state folder as `launcher.ps1` does, reads `config.json` / `versions.json`
  with the built-in versions, and answers `versions` and `version-set`. Check: on the desktop its `versions` output
  is byte-identical to `launcher.ps1 versions` (three versions: built, built from the working tree, out of date);
  unit tests for the version list (2 pass).
- 2026-11-10, backend ported: every text command and entry of `launcher.ps1` (`versions`, `notes`, `version-set` /
  `-next` / `-prev`, `config-get`, `config-set`, `status`, `entries`, `game-settings`, `game-setting`, `sizes`,
  `setup-check`, `list`, `build`, `share-on` / `-off`, the play entries, the recomp and RPCS3 entries and the tools:
  refresh, build, self-update, send, share, DLC import, recomp copies, first-time setup). Checks on the desktop: the
  output of every read command is byte-identical to `launcher.ps1` (versions, notes, config-get, status, 30 entries,
  21 game settings, setup-check, sizes, list), and version stepping writes the same `version.txt`. Sandbox runs
  (scratch git repositories, a stand-in build script and play script): worktree, fetch, no rebuild when current,
  fast-forward and rebuild on a new commit, PLAYING.txt during play, the session check finding a panic, the result
  line, DLC import by header. Off Windows: builds are one static binary copied into `bin/`, play runs `PLAY.sh`,
  PowerShell helpers run through `pwsh` when installed, the performance trace is greyed out. New dependencies:
  `sha2 =0.10.9`; `zip` and `sysinfo` reuse the locked versions. `config.json` keeps its key order (own ordered JSON
  value, `ojson.rs`). Still open: `pending` and `launcher-exe`, then the window (step 4).
- 2026-11-10, rest of the backend: `pending` (the start check: self-update, assets links, the session that closed
  before its check; sandbox: exit 10 with "(checked at next start)", then 0), `launcher-exe`, `clear-cache` /
  `clear-build`, `room`, `send`, `fixed-executor-single` / `-multi`.
- 2026-11-10, the window, functional pass: `skate-launcher` with no arguments opens it (eframe =0.33.3 on the
  workspace's wgpu 27.0.1 and winit; gamepads through gilrs =0.11.2, keyboard and mouse). Pages as in doc 44a 2: Main,
  Play / Recomp / RPCS3 with the info panel (READY / NOT AVAILABLE, info, records), Tools, Opt-ins and Settings,
  Versions, Label, Clear builds with a confirmation, Share sessions, First-time setup, Set up the launcher (native
  pickers on every OS: Windows Forms, osascript, zenity / kdialog), the job screen (cargo progress, crate, compiled
  and warning counts, the waiting card, two B presses cancel, the first error), the finished page ("Finished, check
  this" on a bad result), the previous session at start; the window minimises while a game runs and comes back with
  the result. `--window <entry> [label] [@version]` is the launch-option mode. Desktop check: the window opens with
  the real state (main menu, the selected version READY, the launcher commit). Next: the camcorder look (doc 41).

### Spec of the current launcher

[Doc 44a](44a-launcher-spec.md) (2026-11-10): every backend command and entry with the lines the window parses, every
screen and how it calls the backend, the inputs and flags, the state folder files and their formats (so the Rust app
stays compatible), a per-OS table, the camcorder look's geometry, colours, fonts and timings, and a module layout
with a parity checklist of about 35 lines.

Hardest per-OS problems found:
- Versions build through `scripts/Build.ps1` and play through a generated `PLAY.bat`; the Rust app runs cargo and the
  game itself on every OS.
- The shaped see-through window and the fold-and-hide around a game are hard on Wayland and in Steam Deck game mode:
  a plain window is the fallback there.
- Self-update and prebuilt versions: only a Windows release zip is published today; Linux and macOS need their own.
- The backend shells out to PowerShell helpers (send, share, recomp share, `setup-dev-assets.ps1`): these move into
  Rust or get a shell twin.
- The Skate 3 recomp is a Windows build: greyed out on Linux and macOS (RPCS3 runs on all three).

## Open

- Fork builds 2026-11-10 at `436c69e7`: Windows, Linux and macOS all pass (game build, engine / mod / setup tests). Still to
  do: a real play run on a Linux and a macOS machine.
- Upstream: #7 and stev0760's work are their authors'; this PR carries them merged so the rest of #68 can build on
  them.
