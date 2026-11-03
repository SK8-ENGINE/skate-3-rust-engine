# Steam launcher (couch testing)

Start any version of the engine (the fork's `hails-additions`, upstream `main`, your branches, open PRs, work in
progress), the Skate 3 recomp with a tracing option, or retail Skate 3 in RPCS3, from Steam or Steam Link with a
controller, and see after each session whether it went fine. Built for testing from the couch: compare two versions
back to back, try a PR exactly as it was pushed, record a recomp trace session, without touching a keyboard.

What it does:
- **A real window:** `SkateLauncher.exe` is a full-screen menu for the controller (XInput), the keyboard and Steam's
  desktop layout (arrows, Enter, Esc / Space). It hides while a game runs, so the game gets the front and Steam's
  gamepad layout, and comes back with the session's result when the game closes. The main screen is a hero card
  with the selected version and Play, then tiles (D-pad in all four directions); the splash's skateboard is the mark.
  The engine's play screen (Play, Play + trace all, ...) uses the same tiles, two wide and as many rows as needed.
  `SkateLauncher.exe --snapshot <out.png> [main|versions|tools|play|job|build|failed]` draws one screen at 1920x1080
  into a PNG and exits (checking the look without opening the window).
- **Builds:** while a version builds, the window shows cargo's own step count as a progress bar, the crate being
  built, the warning count and, if it fails, the first error in a fixed-width block. The whole output goes to
  `<version>\logs\launcher-build-<id>-<time>.log` (the newest 10 per version are kept), named on the result screen.
  `launcher.ps1` switches cargo's progress bar on only for the window (`SKATE_LAUNCHER_UI=2`); a terminal run keeps
  cargo's normal output.
- **Versions:** each version is its own git worktree with its own `bin\`, sharing this checkout's prepared assets.
  Two are always there: **hails-additions** (the fork's newest pushed branch) and **Upstream main** (SK8-ENGINE).
  Launching a version that isn't built yet, or whose branch has new pushed commits, fetches and builds it first
  (fast-forward only; local work is never touched).
- **Greyed out until set up:** every entry is listed; one that isn't set up yet (the recomp, RPCS3, sending logs, the
  recomp share, a trace a version doesn't have) is grey and says what to set up.
- **Tools:** Refresh Game Files (the game's start-up check without starting the game, and the refresh when it finds a
  problem), build a version now, send the logs now, the recomp's newest build and save from the main PC, and the
  first-time setup.
- **First-time setup:** on a fresh PC the window opens on a setup page that checks Git, Rust, the C++ build tools,
  LLVM, Python 3.13 and Vulkan, and installs what is missing with winget (one UAC prompt for the machine-wide tools).
- **Size cap:** optional. The build folders never grow over a set size: before a build the launcher deletes the build
  cache of the version built longest ago.
- **Clear builds:** Tools > Clear builds and build caches frees space by hand: everything, every build cache, or one
  version's build cache (it still plays) or whole build (cache and `bin\`, it builds again at its next launch), each
  with its size and a confirmation.
- **After each session** a short check: the game log's panics, the state log's or trace's malformed lines, then the
  optional log send. A game that restarts itself (Refresh Game Files from the game's menu) is waited for.
- **One game at a time:** it starts nothing while `skate3rust.exe`, `skate3.exe` or `rpcs3.exe` runs. While a game
  runs, `PLAYING.txt` exists in the state folder, so other tools and scripts can wait for it.

Windows only. Needs Windows PowerShell 5.1 (built in) and winget (App Installer, built into Windows 10 1809+ and 11);
the first-time setup installs the rest.

## Launcher-only download (standalone)

Each fork release also has `skate-launcher-windows.zip`: the launcher on its own, without a game build and without a
copy of the repository. It downloads and builds only the versions you use:

1. Extract the zip to a folder of its own (for example `C:\Games\SkateLauncher`). The `STANDALONE.txt` inside marks it
   as standalone: its config, state and versions all stay in this folder.
2. Run `powershell -ExecutionPolicy Bypass -File setup.ps1 -Install` once (git, Rust and the build tools; one UAC
   prompt), then `build.bat` (makes `SkateLauncher.exe`), then add `SkateLauncher.exe` to Steam as a non-Steam game.
3. Choose your versions (optional): by default the launcher lists hails-additions and upstream main. To keep only
   one, put `{"builtin_versions": ["hails-additions"]}` in `config.json`; add your own branches in `versions.json`
   (each with `"url"` and `"branch"`).
4. Play. The first time a version is played or built it is downloaded (only that branch, into `versions\<id>`) and
   built; later starts fetch it and rebuild only when it moved. A version you never start is never downloaded.
5. Game files: the first version needs them converted from your Skate 3 disc once; the launcher says how
   (`tools\setup-dev-assets.ps1 -Disc ...` in that version's folder). Every other version then shares them.

## Files

| File | What it is |
|---|---|
| `SkateLauncher.exe` | The window Steam starts: menu, controller and keyboard input, result screens. Build it with `build.bat`. |
| `SkateLauncher.cs` | Its source (C# 5, built with the compiler that ships with Windows). |
| `launcher.ps1` | Does the work: versions, builds, launching, session checks, tools. The window runs it; it runs from a shell too. |
| `setup.ps1` | The first-time setup: `setup.ps1` checks, `setup.ps1 -Install` installs what is missing. |
| `launcher.bat` | `launcher.bat <entry> [label] [-Version <id>]` from a keyboard; no argument opens the window. |
| `build.bat` | Builds `SkateLauncher.exe`. Run it again after pulling a new launcher. |
| `versions.example.json`, `config.example.json` | Examples for your own versions and settings. |

State and your own settings live in `.local\steam-launcher\` (gitignored): `versions.json`, `config.json`,
`version.txt` (the selected version), `last_session.txt` (the last result, also readable remotely) and `PLAYING.txt`.
If you copy the launcher folder somewhere outside `tools\` (two levels below the repository, e.g. `.local\steam\`), it
keeps those files next to itself instead.

## Setup

1. **Build the exe:** double-click `tools\steam-launcher\build.bat`.
2. **Add it to Steam:** Steam > **Games > Add a Non-Steam Game to My Library > Browse** > pick
   `tools\steam-launcher\SkateLauncher.exe`. Rename it in **Properties**; add more copies with launch options for
   one-click modes (table below).
3. **Start it:** on a fresh PC it opens on **First-time setup**: choose **Install what is missing** and accept the one
   UAC prompt (the C++ build tools take 10 minutes or more). Then convert your game files once from your own disc:
   `tools\setup-dev-assets.ps1 -Disc <your Skate 3 ISO or default.xex>`.
4. **Optional, more versions:** copy `versions.example.json` to `.local\steam-launcher\versions.json` and edit it.

   | Field | Meaning |
   |---|---|
   | `id` | Short name, used in launch options (`@id`). An id of `hails-additions` or `upstream-main` replaces that built-in. |
   | `name` | Shown in the menu. |
   | `branch` | The branch to follow (fetched from `url` when set, else from `remote`, default `origin`). |
   | `url` | Optional: a repository URL to fetch the branch from (the built-ins use the fork's and upstream's). |
   | `dir` | Where its worktree lives, relative to the repository (e.g. `.local/versions/main`); `.` = this checkout. |
   | `detached` | Optional: `true` = a launcher-only copy that follows the branch without checking it out (never conflicts with a worktree where you work on that branch). |
   | `notes` | What to test, shown before the game starts (`\n` for new lines). |
   | `mods_enable` | Optional: the mod id(s) the "dev test mods" entry turns on (`SKATE3_MODS_ENABLE`). |
   | `env` | Optional: environment variables for its sessions, e.g. `{ "SKATE_LIVING_WORLD_DEBUG": "1" }`. |
   | `dev` | Optional: `true` = build from the working tree as it is; rebuilt when its uncommitted changes change. |
   | `target` | Optional: its cargo target folder, relative to the repository. |

5. **Optional, settings:** copy `config.example.json` to `.local\steam-launcher\config.json`:

   | Field | Meaning |
   |---|---|
   | `remote` | The git remote for versions without `url` (default `origin`). |
   | `fork_url`, `upstream_url` | The repositories the two built-in versions follow. |
   | `target_root` | Build folders go to `<target_root>\<version id>` (e.g. a fast or big drive) instead of each worktree's `target\`. |
   | `build_cap_gb` | Size cap for the build folders together (e.g. `200`); `0` or missing = no cap. |
   | `build_cap_dir` | Optional: cap everything under this folder instead (when other builds share the drive folder too); only the launcher's own version build folders are ever deleted. |
   | `build_reserve_gb` | Room a build needs, kept free under the cap (default `30`). |
   | `recomp` | The recomp: `exe` (your build), `working_dir`, `args`, `sessions` (default `.local\recomp\sessions`), `trace_check` (a script printing malformed-line counts). |
   | `recomp_entries` | The recomp modes: `id` (starts with `recomp-`), `name`, `hint`, `trace` (`SKATE3_TRACE` categories), `capture` (record `audio.f32`), `ask_label` / `label`, `folder`, or `bat` + `bat_args` to run your own batch file. |
   | `rpcs3`, `rpcs3_entries` | Retail Skate 3 in RPCS3: `exe`, `game` (disc folder or `EBOOT.BIN`), `args` (default `--no-gui --fullscreen`); entries `id` (starts with `rpcs3-`), `name`, `hint`, `args`. |
   | `after_session` | A send script (path relative to the repository, e.g. `tools\second-pc\play-and-send.ps1`) run with `-SendOnly` after each session. Its settings file `send-sessions.txt` sits next to it or at the repository root. |
   | `recomp_script` | The recomp share script for **Recomp: get the newest build / save** (default `tools\second-pc\play-recomp.ps1` once `send-sessions.txt` exists). |

   The size cap: when the build folders (or `build_cap_dir`) plus `build_reserve_gb` are over `build_cap_gb`, the
   launcher deletes whole version build folders, the one built longest ago first, never the one it is about to build,
   until it fits. Each version keeps its exe in `bin\`, so it still plays; only its next build starts from scratch.
   You build the recomp yourself from your own copy of the game; nothing from the game is included here.

## One-click shortcuts (launch options)

Each starts straight away, shows the result for 10 s at the end and closes by itself.

| Shortcut (suggested name) | Launch options |
|---|---|
| Skate: next version | `version-next` (selects the next version and shows its notes) |
| Skate: play | `rust-play` (the selected version) |
| Skate: play + trace all | `rust-traceall` |
| Skate: hails-additions | `rust-play @hails-additions` |
| Skate: upstream main | `rust-play @upstream-main` |
| Skate: refresh game files | `tool-refresh` |
| Skate: recomp audio trace | `recomp-audio` |

A shortcut without launch options opens the menu.

## Entries and commands

| Entry | What it runs |
|---|---|
| `rust-play` | the version's `PLAY.bat` |
| `rust-traceall` | play with every session trace the version has (`SKATE_TRACE_ALL`), all in the version's `logs\trace-all-<time>\`; the result counts malformed rows in `audio_state.tsv` and `frames.tsv` |
| `rust-statelog` | play + `SKATE_AUDIO_STATE_LOG` (`.local\audio-state-logs\state_*.tsv`) |
| `rust-trace` | play + `SKATE_AUDIO_TRACE=1` (native sound starts in `logs\game-*.log`) |
| `rust-devmods` | play with the version's `mods_enable` mod(s) on |
| `rust-perf` | play + Bevy performance trace (`PLAY.bat -trace`) |
| `tool-refresh` | `skate3rust.exe --update-game-files`: the game's start-up file check without starting the game; out of date = the same refresh as the game's Extras > Refresh Game Files. Versions built before it are greyed out |
| `tool-build` | fetch and build the selected version now |
| `tool-send` | the `after_session` send now |
| `tool-recomp-update`, `tool-recomp-getsave` | `play-recomp.ps1 -Update` / `-GetSave` |
| `tool-setup` | `setup.ps1 -Install` |
| `clear-cache <id\|all>`, `clear-build <id\|all>` | delete a version's build cache (its cargo target folder) or its whole build (that plus `bin\`); Tools > Clear builds runs these |
| `recomp-*`, `rpcs3-*` | the modes from `config.json` |

Commands (`launcher.ps1 <command>`): `entries` (id, name, hint, ask, group, on, why off), `versions`, `status`,
`version-next`, `version-prev`, `version-set <id>`, `notes [id]`, `build <id>`, `pending` (checks a session that was
closed with Steam's **Exit game** before its check, and sends its logs; the window runs it at every start), `send`,
`setup-check`, `sizes` (each version's build cache and `bin\` in bytes), `room <id>` (apply the size cap as before a
build of `<id>`), `list`.

## Notes

- Closing a game with Steam's **Exit game** also closes the launcher, so its after-session check is skipped; the next
  start of any shortcut catches it up.
- A version's worktree is a normal git worktree: remove it with `git worktree remove <dir>` (its `assets` folder is a
  junction to your prepared assets; remove the junction first, never delete through it).
- Steam gives the launcher window its desktop layout (keys), and the game its gamepad layout once the game's window
  is in front; the launcher reads both, and drops a key that repeats a pad press within 150 ms.
