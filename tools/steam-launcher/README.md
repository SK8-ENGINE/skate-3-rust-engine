# Steam launcher (couch testing)

Start any version of the engine (the fork's `hails-additions`, upstream `main`, your branches, open PRs, work in
progress), the Skate 3 recomp with a tracing option, or retail Skate 3 in RPCS3, from Steam or Steam Link with a
controller, and see after each session whether it went fine. Built for testing from the couch: compare two versions
back to back, try a PR exactly as it was pushed, record a recomp trace session, without touching a keyboard.

What it does:
- **A real window, shaped like a camcorder's LCD panel:** `SkateLauncher.exe` is the flip-out screen of a mid-2000s
  HD camcorder on your desktop (a borderless see-through window, drag it by its bezel) with a menu for the controller
  (XInput), the keyboard, Steam's desktop layout (arrows, Enter, Esc / Space) and the mouse: the bezel's keys work too
  (HOME = back, the T / W rocker moves, SET selects, START/STOP plays, POWER closes). It swings open from its hinge when
  it starts and shut when it closes, and wears fresh skate-session scrawl and scuffs every start
  (`SkateLauncher.exe --wear-test <out.txt> <out.png>` checks one start's wear stays off the UI). It hides while a game runs, so the game gets the front and Steam's gamepad layout, and comes
  back with the session's result when the game closes. Every screen is the camera's own LCD menu: a list with the
  selected option's info beside it (its full name, READY or why it is not available, what it does and, for the play
  options, what it records: FILES / GAME LOG rows from launcher.ps1 `entries` columns 8 and 9). A worn skate sticker,
  picked once per user, sits on the camera (`camcorder-sticker.txt`; PNGs in `stickers\` in the state folder replace
  the built-in set). Details: [doc 41](../../docs/hails-additions/41-launcher-camcorder-viewfinder.md#change).
  `SkateLauncher.exe --snapshot <out.png> [main|versions|tools|setup|play|play-traceall|waiting|fetching|checking|job|build|failed][@<0..1>][#<seed>] [<w>x<h>]` draws one screen at 1920x1080
  into a PNG and exits (checking the look without opening the window; `@0.5` = halfway through the power-on, `#7` = the
  sticker from seed 7). `SkateLauncher.exe --bench <out.txt> [<w>x<h>]` writes the time per frame through the power-on.
  The window opens centred on the main monitor. `SkateLauncher.exe --same-monitor` (or Tools > "Open on", saved as
  `open-on-launching-monitor.txt` in the state folder) opens it on the monitor of the window it was started from
  (Steam, Explorer), or under the mouse when there is none.
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
- **Import DLC (Tools):** pick one or more of your Skate 3 DLC package files, or a folder (every package inside, any
  depth). Each Skate 3 package (checked by its header, not its name; the title update and other files are skipped) is
  copied to `dlc\<pack name>\` in the state folder (`dlc_dir` in `config.json` moves it). Every game, setup and refresh
  the launcher starts gets that folder as `SKATE3_DLC_DIR`; run Refresh Game Files afterwards to convert what the packs
  add (the Create pack's replay editor, DLC maps as they are supported).
- **First-time setup:** the first start opens **Set up the launcher** (USER-ASKS 461): the game folder (versions and
  their builds: `storage_root`, then `target_root` = `<folder>\builds`, `build_cap_dir` = `<folder>` and, standalone,
  `versions_root` = `<folder>\versions`), the space builds may use (`build_cap_gb`), the Skate 3 ISO or `default.xex`
  (`disc`: Play converts the game files from it when a version has none), the Skate 3 Recompiled folder
  (`recomp.working_dir`, with `recomp.exe` found inside it) and RPCS3 (`rpcs3.exe`, `rpcs3.game`). It saves only what
  changed and sets `first_setup_done`; **Tools > Set up the launcher** opens it again. On a fresh PC a page after it
  checks Git, Rust, the C++ build tools, LLVM, Python 3.13 and Vulkan, and installs what is missing with winget (one
  UAC prompt for the machine-wide tools).
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

## Linux and macOS: get your computer ready

The launcher window itself is Windows-only for now (a version for every OS is being built). Until then, this gets a
Linux or macOS computer ready to build and play:

1. Open a terminal in this folder (Linux: right-click an empty spot, **Open in Terminal**; macOS: right-click the
   folder in Finder, **New Terminal at Folder**).
2. Type `sh setup.sh` and press Enter. It lists what is installed and what is missing.
3. Type `sh setup.sh --install` and press Enter. Type your password when it asks (you will not see it as you type;
   that is normal). It installs everything missing, which can take a few minutes.
4. When it says **All required tools are installed**, go to the main folder and run `./SETUP.sh <your Skate 3 ISO>`,
   then `./BUILD.sh` and `./PLAY.sh`.

On macOS without Homebrew, the setup tells you how to get it first. On a Steam Deck in game mode, use the prebuilt
versions instead.

## Launcher-only download (standalone)

Each fork release also has `skate-launcher-windows.zip`: the launcher on its own, without a game build and without a
copy of the repository. It downloads only the versions you use:

1. Extract the zip to a folder of its own (for example `C:\Games\SkateLauncher`). The `STANDALONE.txt` inside marks it
   as standalone: its config, state and versions all stay in this folder.
2. Double-click `build.bat`: it downloads the release's prebuilt `SkateLauncher.exe` (when it was built from this
   `SkateLauncher.cs`; otherwise it compiles it) and starts it. **Set up the launcher** asks for the game folder, the
   space for builds, your Skate 3 disc and, optionally, the recomp and RPCS3; then **Install what is missing** installs
   what the selected version needs (one UAC prompt): Git only for a prebuilt version, plus Rust and the build tools for
   one built from source. Add `SkateLauncher.exe` to Steam as a non-Steam game for the couch.
3. Choose your versions (optional): by default the launcher lists hails-additions and upstream main. To keep only
   one, put `{"builtin_versions": ["hails-additions"]}` in `config.json`; add your own branches in `versions.json`
   (each with `"url"` and `"branch"`).
4. Play. **Prebuilt versions** (USER-ASKS 463: a version with `"release"`, a release tag of its repository; the
   built-in hails-additions has its rolling release): the first play downloads that release's
   `skate3rust-windows-x64.zip` (checksum checked) into `versions\<id>` in the game folder; later starts read its
   `release.json` and download again only when the revision moved, over the old build (the converted game files, logs
   and settings stay). `config.json` `"use_releases"` (default on for this download, off in a checkout) switches it.
   **Other versions** are cloned (only that branch) and built; later starts fetch them and rebuild only when they
   moved. A version you never start is never downloaded.
5. Game files: the first version needs them converted from your Skate 3 disc once; with the disc picked in **Set up
   the launcher**, Play does it (a prebuilt version's own setup gets it as `SKATE3_SETUP_DISC` and starts without
   asking; a source version runs `tools\setup-dev-assets.ps1 -Disc ...`). Every other source version then shares them.

## Updating the launcher itself

The launcher keeps itself current with the newest pushed `hails-additions` on the fork (no `git pull`, no shell):
at every start (not while a game runs), before **Tools > Update and build the selected version**, and from **Tools >
Update the launcher**. It fetches that branch into its own small clone (`self.git` in the state folder), copies
`tools\steam-launcher` (into the launcher's folder) and `tools\second-pc` from it, rebuilds `SkateLauncher.exe`
when its source changed (the release's prebuilt `SkateLauncher.exe` when its `SkateLauncher.exe.source-sha256`
names that source, else compiled) and restarts into the new window (the old one waits hidden, so Steam keeps tracking it).
Nothing else of the checkout is touched and nothing is committed. A local file that differs (for example one copied
in by `get-patches.ps1`) is backed up to `self-update-backup\<time>\` first. The main screen shows the launcher's
commit next to the wordmark, and the fork's newer one when it did not update. Log: `self-update.log`; on a failure
the current launcher stays. `config.json`: `"self_update": false` turns the automatic runs off, `"launcher_branch"`
picks another branch (default `hails-additions`, from `fork_url`).

## Files

| File | What it is |
|---|---|
| `SkateLauncher.exe` | The window Steam starts: menu, controller and keyboard input, result screens. Build it with `build.bat`. |
| `SkateLauncher.cs` | Its source (C# 5, built with the compiler that ships with Windows). |
| `launcher.ps1` | Does the work: versions, builds, launching, session checks, tools. The window runs it; it runs from a shell too. |
| `setup.ps1` | The first-time setup: `setup.ps1` checks, `setup.ps1 -Install` installs what is missing. |
| `launcher.bat` | `launcher.bat <entry> [label] [-Version <id>]` from a keyboard; no argument opens the window. |
| `build.bat` | Gets `SkateLauncher.exe` (the release's prebuilt one, or compiled) and starts it; afterwards the launcher updates itself the same way. |
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
| `tool-build` | update the launcher, then fetch and build the selected version now |
| `tool-self-update` | update the launcher to the newest `hails-additions` on the fork (see Updating the launcher itself) |
| `tool-send` | the `after_session` send now |
| `tool-share` | pack the new logs and open a GitHub issue (Share sessions on; `share-on` / `share-off` toggle it, see docs/hails-additions/second-pc-testing.md#share-sessions-via-github) |
| `tool-recomp-update`, `tool-recomp-getsave` | `play-recomp.ps1 -Update` / `-GetSave` |
| `tool-setup` | `setup.ps1 -Install` |
| `game-settings` | Tools > Opt-ins and settings: one row per setting (key, group, label, shown value, hint) from the game's own `settings\*.json` beside the converted game files, the files its Guide / F1 overlay writes |
| `game-setting <key>` | Steps one of them to its next value (refused while the game runs: change it in the overlay then) |
| `fixed-executor-single`, `fixed-executor-multi` | Physics Step Threads (also on the Opt-ins page): saved as `physics_threads_multi` in the game's `settings\graphics.json`, passed as `SKATE_FIXED_EXECUTOR` too for older game versions |
| `clear-cache <id\|all>`, `clear-build <id\|all>` | delete a version's build cache (its cargo target folder) or its whole build (that plus `bin\`); Tools > Clear builds runs these |
| `recomp-*`, `rpcs3-*` | the modes from `config.json` |

Commands (`launcher.ps1 <command>`): `entries` (id, name, hint, ask, group, on, why off), `versions`, `status`,
`version-next`, `version-prev`, `version-set <id>`, `notes [id]`, `build <id>`, `pending` (checks a session that was
closed with Steam's **Exit game** before its check, and sends its logs; the window runs it at every start), `send`,
`setup-check`, `sizes` (each version's build cache and `bin\` in bytes), `room <id>` (apply the size cap as before a
build of `<id>`), `list`.

## Notes

- The window never waits on a `launcher.ps1` call: the menu's lists (entries, versions, notes, status) load behind the
  start splash ("Loading the menu") and, after a job or a version switch, in the background before the menu is rebuilt
  (Play waits for a switch to finish); the game window checks run 4 times a second, not every frame. Waiting on them in
  the window froze it at the end of a job and at start, long enough for Windows to offer to end it (USER-ASKS 436).
- One launcher per folder: starting it again (a second Steam shortcut, a double click) brings the open window to the front
  instead of opening a second one. The build log closes when the build ends, and the after-session send skips a file
  another program still writes (it goes with the next send) and keeps the share's error next to the fallback's.
- A build that has to wait for another build of the same folder (a shell build, a second window, a dev `cargo`) shows
  **WAITING** with who holds it (`BUILDING-<id>.txt` in the state folder, or the cargo process) and for how long; **B**
  twice (Esc on a keyboard) cancels it. The build log gets `[launcher ...] waiting / waited / cancelled` lines.
- Closing a game with Steam's **Exit game** also closes the launcher, so its after-session check is skipped; the next
  start of any shortcut catches it up.
- A version's worktree is a normal git worktree: remove it with `git worktree remove <dir>` (its `assets` folder is a
  junction to your prepared assets; remove the junction first, never delete through it).
- Steam gives the launcher window its desktop layout (keys), and the game its gamepad layout once the game's window
  is in front; the launcher reads both, and drops a key that repeats a pad press within 150 ms.
