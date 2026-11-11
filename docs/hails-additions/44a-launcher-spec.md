# 44a. Skate launcher: spec of the current launcher for the Rust rewrite

Part of [doc 44](44-every-os.md#skate-launcher-on-every-os-user-asks-561). Read from `tools/steam-launcher/` as on
`hails-additions` (launcher commit `c6da2ddc`), 2026-11-10. References are `file:line`; L = `launcher.ps1`,
C = `SkateLauncher.cs`, S = `setup.ps1`, R = `README.md`, D41 =
[doc 41](41-launcher-camcorder-viewfinder.md). Line numbers are approximate (a spot check found drifts of up to about
7 lines); the function and command names are the reliable anchors. "Unverified" marks claims not read line by line.

## 0. Architecture today

- Two processes: the window (`SkateLauncher.exe`, C# 5 WinForms, built with the .NET Framework 4 `csc.exe`,
  build.bat:4-6) and the backend (`launcher.ps1`, Windows PowerShell 5.1). The window runs every backend call as
  `powershell.exe -NoProfile -ExecutionPolicy Bypass -File launcher.ps1 <args>`, no window, stdout+stderr redirected,
  stdin closed, env `SKATE_LAUNCHER_UI=2`, OEM code page decoding (C:405-414).
- Quick calls (`Run`) block for the whole output (C:416-423); the four lists `entries`, `versions`, `notes`, `status`
  are prefetched in background tasks and cached until a job ends or the version changes (C:428-457, RebuildMain
  C:840-860). Jobs (`StartJob`) stream lines asynchronously (C:882-897); `-NoPrompt` is appended to every job.
- The Rust app should fold both into one binary: a backend module called in-process (on worker threads) plus a
  `launcher <command>` CLI that prints the same tab-separated lines, so scripts, agents and `launcher.bat` keep working.

## 1. Backend commands and entries (launcher.ps1)

Parameters: `launcher.ps1 [Entry] [Label] [-NoPrompt] [-Version <id>] [-Path <p>]` (L:18). `-Version` picks the
version (default: `version.txt`, else the first version, L:174-184). `-Path` is used by `tool-dlc` (L:1123).

Start of every run: resolve `$Repo` (two folders up; standalone = `STANDALONE.txt` beside the script makes the
script folder the root, L:21-26); state dir (L:30-31, see section 4); prepend `%USERPROFILE%\.cargo\bin` to PATH
(L:39); load `config.json` (L:45-47); set `SKATE3_DLC_DIR` when the DLC folder exists (L:76-77); build the version
list (built-ins + `versions.json`, L:150-172) and the entries table (L:951-1000).

### 1a. Query commands (window parses the output)

| Command | Output (tab-separated) / effect | Ref |
|---|---|---|
| `entries [-Version id]` | per entry: `id, name, hint, ask(1/0), group(rust/recomp/retail/tools), on(1/0), why_off, info, records` (records joined by `|`; a record starting `#` is a heading row `#Files: ...`) | L:1287-1300, C:624-633, C:2170 |
| `versions` | `id, name, status, selected("*" or "")` | L:1301-1305 |
| `status` | key/value rows: `version, version_status, last` (last_session.txt lines joined " / "), `send` (off/on/why), `share` ("on <repo>", "no repo", "" when absent), `fixed_executor`, `setup_missing`, `first_setup` ("1" = wizard due), `launcher` (8-char commit), `launcher_newer` | L:1306-1323 |
| `notes [id]` | `Version: <name>`, `Branch: <b>   Status: <s>`, blank, `What to test:`, notes | L:1352-1360 |
| `version-next`, `version-prev`, `version-set <id>` | writes `version.txt` (ASCII), then prints `notes` | L:1345-1351 |
| `config-get` | rows `standalone, default_root, storage_root, target_root, build_cap_gb, disc, recomp.exe, recomp.working_dir, rpcs3.exe, rpcs3.game` (placeholder `C:\path\to*` = empty) | L:1327-1338, L:51-55 |
| `config-set <file>` | merges the dotted keys of a JSON object file into config.json (UTF-8, no BOM, trailing newline), deletes the file, prints `saved` | L:1340-1343, L:57-70 |
| `setup-check` | runs setup.ps1: rows `id, name, ok(1/0), detail, required(1/0)`; exit 0 = all required present | L:1344, S:83-85 |
| `sizes` | `id, name, cache_bytes, bin_bytes` | L:1368-1372 |
| `game-settings` | `key, group(Opt-ins/Settings), label, shown value, hint` from the game's `settings\*.json` | L:1404-1413, rows L:858-880 |
| `pending` | start check (below); exit 10 = a result was written to last_session.txt, 0 = nothing | L:1235-1283 |
| `list` | `id  name` aligned (human) | L:1428 |
| `launcher-exe` | gets/builds SkateLauncher.exe; prints ok/failed | L:1325 |

### 1b. Action commands

| Command | Steps | Ref |
|---|---|---|
| `build <id>` | Ensure-Version; prints `Ready: ...` (exit 0) or `Build failed: ... Build log: <path>` (exit 1) | L:1361-1364 |
| `clear-cache <id|all>`, `clear-build <id|all>` | refuses while a game runs (exit 1); deletes Target-Dir (and `bin\` for clear-build); writes last_session.txt with GB freed | L:1375-1397 |
| `room <id>` | Make-Room as before a build | L:1398 |
| `send` | Send-Sessions | L:1399 |
| `fixed-executor-single/-multi` | sets `physics_threads_multi` in game `graphics.json` | L:1400 |
| `game-setting <key>` | refused while PLAYING.txt exists (exit 1, message); steps the row to the next value and writes the JSON | L:1414-1424 |
| `share-on`, `share-off` | `share-session-issue.ps1 -Root <versiondir> -Settings <f> -Consent / -Revoke` | L:1425-1426, L:911-919 |
| no entry | starts SkateLauncher.exe | L:1431-1432 |
| `<entry> [label]` | Launch (below); exit 1 when a tool failed (`ToolFailed`) | L:1429, L:1132-1134 |

### 1c. Entries (Launch, L:1142-1233)

Common: refuse if a game runs (`Not started: <holder> is running.`, L:1145); refuse if Entry-Blocked (L:1046-1076);
tools go to Run-Tool (L:1150). Rust entries: Ensure-Version (L:1154), disc conversion if no `assets\private\stock`
(runs the version's `tools\setup-dev-assets.ps1 -Disc <disc>`, L:1160-1163; release versions get `SKATE3_SETUP_DISC`),
refuse without game files, apply version `env`, set `SKATE_FIXED_EXECUTOR` (L:1169-1172). Then write PLAYING.txt,
run, remove PLAYING.txt in `finally`, Check-Session, Repair-AllAssetLinks, set `SKATE_SEND_LOGS_DIR`, Send-Sessions,
Share-Sessions, Save-Result `"<stamp>  <key> <label> [<ver>]\r\n<summary>"` (L:1174-1232).

| Entry | Group | Runs | Env set | Kind (check) |
|---|---|---|---|---|
| rust-play | rust | `cmd /c ""<ver>\PLAY.bat""`, then wait while any game process lives (Run-Bat L:672-680, Wait-GameTree L:664-670) | - | rust |
| rust-traceall | rust | PLAY.bat; dir `<ver>\logs\trace-all-<yyyyMMdd_HHmmss>` | by Version-Traces (source scan): `SKATE_AUDIO_STATE_LOG=<dir>\audio_state.tsv` + `SKATE_AEMS=1 SKATE_AUDIO_TIMING=1`, `SKATE_AUDIO_TRACE=1`, `SKATE_FRAME_LOG=<dir>\frames.tsv`, `SKATE_LIVING_WORLD_DEBUG=1`, `SKATE_TRACE_ALL=1 SKATE_PERF_REPORT=<dir>\perf.json` | traceall |
| rust-statelog | rust | PLAY.bat | `SKATE_AUDIO_STATE_LOG=.local\audio-state-logs\state_<time>.tsv`, `SKATE_AEMS=1`, `SKATE_AUDIO_TIMING=1` | statelog |
| rust-trace | rust | PLAY.bat | `SKATE_AUDIO_TRACE=1` | rust |
| rust-devmods | rust | PLAY.bat | `SKATE3_MODS_ENABLE=<mods_enable>` | rust |
| rust-perf | rust | `PLAY.bat -trace` | - | rust |
| recomp-* (config) | recomp | `recomp.exe` with `args` (default `--skate3_demo_path --skate3_demo_path_signed_in`), cwd `working_dir`; traced: session dir `<sessions>\<prefix>_[label_]<time>` with stdout/stderr to `game.out/err`; or `bat` + `bat_args` | `SKATE3_AUDIO_TRACE_FILE=<dir>\trace.tsv`, `SKATE3_TRACE=<trace>`, `SKATE3_INPUT_RECORD=<dir>\pad.txt`, `SKATE3_AUDIO_CAPTURE=<dir>\audio.f32` when capture | recomp / none |
| rpcs3-* (config) | retail | `rpcs3.exe <args> "<game>"` (default `--no-gui --fullscreen`), cwd exe folder | - | none |
| tool-refresh | tools | Ensure-Version, then `bin\skate3rust.exe --update-game-files --assets <Resolve-DevAssets>` (release: `skate3rust.exe --update-game-files`); writes REFRESHING.txt; parses `GAME_FILES <text>`; exit 2 = no game files | - | - |
| tool-build | tools | Self-Update (unless `self_update:false`), then Ensure-Version | - | - |
| tool-self-update | tools | Self-Update -Force | - | - |
| tool-send | tools | Send-Sessions | - | - |
| tool-share | tools | Share-Sessions | - | - |
| tool-dlc | tools | Import-Dlc (WinForms Yes/No/Cancel box, then OpenFileDialog multi or FolderBrowserDialog) then Refresh-GameFiles when packages were added | `SKATE3_DLC_DIR` | - |
| tool-recomp-update / -getsave | tools | `play-recomp.ps1 -Update` / `-GetSave` | - | - |
| tool-setup | tools | `setup.ps1 -Install` | - | - |

Recomp/RPCS3 entries come from `config.json` (L:982-994). Grey-out reasons (Entry-Blocked L:1046-1076): missing
setup tools (rust + tool-refresh/build), version source lacks the switch (traceall/statelog/trace via Version-Has
scanning `crates\skate-game\src\*.rs` for `"<ENV>"`, L:976-989), no `mods_enable`, refresh not supported
(`--update-game-files` absent), send not set up, share off, no recomp script, recomp exe / bat missing, rpcs3 exe or
game missing.

### 1d. Key procedures

- **Ensure-Version** (L:543-651): standalone + no `.git` = `git clone -q --single-branch --branch <b> <url> <dir>`;
  else `git -C <repo> worktree add [--detach] <dir> <ref>` (detached for `detached`/`url` versions); Repair-AssetLink;
  if clean and not dev: Fetch-Version (`git -C <d> -c http.lowSpeedLimit=1000 -c http.lowSpeedTime=30 fetch -q <url|remote>
  +refs/heads/<b>:<ref>`, `GIT_TERMINAL_PROMPT=0`, L:198-216) and `merge -q --ff-only`; head = `rev-parse --short HEAD`
  (dev: plus 12 chars of `hash-object` of `git diff --binary HEAD -- crates vendor Cargo.toml Cargo.lock .cargo
  docs/images maps`); up to date when `bin\skate3rust.exe` exists and `bin\built_commit.txt` == head. Else Make-Room,
  build log `<d>\logs\launcher-build-<id>-<yyyyMMdd_HHmmss>.log` (keep 10), wait for holders (BUILDING-<id>.txt with a
  live pid, or a `cargo.exe` whose command line names the target dir via CIM, L:526-541; or the version's exe
  running), write `BUILDING-<id>.txt` = `pid\tname\tISO time\twindow|shell`, set `SKATE_BUILD_STAMP=inputs`, run
  `powershell -File <d>\scripts\Build.ps1 -TargetDirectory <target>`, write built_commit.txt.
  UI cargo env: `CARGO_TERM_COLOR=never, CARGO_TERM_PROGRESS_WHEN=always, CARGO_TERM_PROGRESS_WIDTH=120,
  CARGO_TERM_PROGRESS_TERM_INTEGRATION=false` (L:600-602).
- **Release versions** (L:217-279): `release` + `use_releases` (default on when standalone). Downloads
  `https://github.com/<owner/repo>/releases/download/<tag>/release.json`, `skate3rust-windows-x64.zip` and `.sha256`
  (Invoke-WebRequest, 900 s), checks SHA-256, extracts, `robocopy /E /XF release.json` over the version dir, writes a
  `PLAY.bat` (Write-ReleasePlay L:234-240, contents unverified).
- **Version-Status** strings (L:281-297): `not downloaded yet ...`, `release build <n> (<rev7>)`, `not set up yet ...`,
  `not built yet ...`, `built from the working tree (MM-dd HH:mm)`, `built at <sha>`, `out of date (built X, now Y): next launch rebuilds`.
- **Make-Room** (L:340-365): cap `build_cap_gb` (0 = off), reserve `build_reserve_gb` (30), total = `build_cap_dir`
  size or the sum of every Target-Dir; deletes other versions' target dirs oldest `bin\skate3rust.exe` first.
- **Assets**: Current-Assets reads `data\installation.json` `.directory` (+ `\assets`), standalone also each version's
  `data\` (L:300-321). Repair-AssetLink recreates the `assets` junction with `cmd /c rmdir` + `cmd /c mklink /J`
  when it is a reparse point lacking `private\game.json` (L:367-379).
- **Self-Update** (L:453-524): bare clone `self.git` in the state dir, removes `*.lock` older than 2 min, `fetch -q --depth 1
  <fork_url> +refs/heads/<launcher_branch>:refs/heads/self`, `git archive --format=zip` of tools/steam-launcher and
  tools/second-pc with `core.autocrlf=true`, backs up differing local files to `self-update-backup\<time>\`, writes the
  sha file (`$SelfCommit`, name unverified), log `self-update.log`, rebuilds the exe (prebuilt from the release when
  `SkateLauncher.exe.source-sha256` matches, L:418-434, else csc) and writes `restart.txt`. Markers: `LAUNCHER_SELF\tfetching|fetched`.
- **pending** (L:1235-1283): deletes `SkateLauncher.old.exe`; Self-Update unless `self_update:false`,
  `SKATE_LAUNCHER_NO_SELF=1` or a game runs; rebuild exe when the .cs is newer; Repair-AllAssetLinks; finds the
  newest of statelog / recomp session / trace-all folder / `game-*.log` newer than last_session.txt; removes a stale
  PLAYING.txt; runs Check-Session and the missed send. Stage lines `LAUNCHER_STAGE\t<text>`.
- **Check-Session** (L:718-783): rust = newest `logs\game-*.log` + count of `panicked` in `<name>.stderr.log`;
  statelog = rows, minutes, malformed rows; traceall = per table rows / malformed (`MALFORMED - unusable`), MB;
  recomp = `py -3.13 <trace_check> trace.tsv` last `malformed` line.
- **Send-Sessions** (L:785-812): `powershell -File <after_session> -SendOnly`; needs `send-sessions.txt` beside it or
  at the repo root.
- **Running games**: process names `skate3`, `skate3rust`, `rpcs3` (L:653-654, C:178).

### 1e. Output markers the window parses (job stream)

| Line | Meaning | Ref |
|---|---|---|
| `LAUNCHER_BUILD\tbegin\t<name>\t<commit>\t<logpath>` | build starts; the WINDOW writes the build log file | L:603, C:113-118 |
| `LAUNCHER_BUILD\twait\tbuild\t<who>[\thold]` | holder named; `hold` = launcher itself waits | L:623,630, C:131-136 |
| `LAUNCHER_BUILD\twaitend` / `\tend\t<code>` | wait over / build done (0 = ok) | L:632,642, C:129,138 |
| `LAUNCHER_WAIT\tbegin\tfetch\t<branch>\t<url>` / `\tend\tfetch\t<code>` | fetch before build (WAITING after 5 s) | L:201-205, C:121-127 |
| `LAUNCHER_SELF\tfetching|fetched`, `LAUNCHER_STAGE\t<text>` | start check | L:461-465, L:451, C:553-556 |
| cargo: `Building [..] n/m: crates`, `Compiling x v`, `error[..]:`, `warning: `, `Blocking waiting for file lock on X`, `Updating/Downloading/Locking/Fetching` | progress, crate, first error (40 lines), warnings, lock wait, stage | C:86-91, C:140-160 |
| everything else | plain log (last 400 lines) | C:887-889 |

## 2. Window screens and states

Pages are a stack (`Page`: Title, Items, Sel, Body, Mono, Cols, OnBack; `Item`: Text, Hint, On, Run, Info, Records;
C:69-83). Back pops (or OnBack); empty stack re-pushes Main (C:1072-1076). A text page closes on A (C:1081-1083).

| Screen | Shows / items | Actions | Ref |
|---|---|---|---|
| Start splash (checking) | rolling board, logo, stage text; "B twice to skip" after 3 s of self fetch | runs `pending`, then prefetch lists; min 1.1 s | C:531-588, LcdSplash C:2227 |
| Main "Skate launcher" | Play: <version>; Versions; What to test; Recomp (grey if none); RPCS3 (grey); Tools; Last session; Exit. Two-column grid tiles. Launcher commit next to the wordmark, newer one if any | pushes pages / ShowText / Close | C:605-618, C:1095 |
| Play (`EntryPage` group rust, 2 cols) | rust entries + Share item; detail panel: NOW SELECTED, READY / NOT AVAILABLE pill, info, records; play prompt | Begin(entry) or LabelPage when ask=1 | C:620-645, LcdInfo C:2134 |
| Recomp / RPCS3 | entries of group recomp / retail | same | C:620 |
| Tools | tools entries + Share item + Opt-ins and settings + Open on (monitor) + Set up the launcher + Clear builds | | C:635-640 |
| Opt-ins / Settings | `label: value` rows from `game-settings`; Opt-ins page ends with "Settings" | `game-setting <key>`, error as text page | C:647-662 |
| Share consent | Turn it on / Not now / What is shared | `share-on` / `share-off` | C:664-688 |
| Clear builds | Everything, Every build cache, per version cache + whole build with GB | Confirm page (No, keep it / Yes, delete) then job `clear-build|clear-cache <id|all>` | C:692-716 |
| Label | session, ride, grind, bail, push, emit, carve, water, marker | Begin with label | C:177, C:717-721 |
| Versions "Choose version" | `> ` marks selected; hint = status | `version-set <id>` in background, `switching` blocks Begin until RebuildMain | C:722-731 |
| First-time setup (tools) | body ok / MISSING rows from setup-check | Install what is missing (tool-setup job) / Not now | C:733-746 |
| Set up the launcher (wizard) | Game folder, Space for builds (0,50,100,150,200,300,500 GB cycle), Skate 3 disc, Skate 3 Recompiled, RPCS3, RPCS3 Skate 3 game, Done, Not now; each hint shows "Now: ..." and free space | native folder / file pickers; SaveWizard writes `config-set.json` (changed keys only; storage_root also sets target_root=<root>\builds, build_cap_dir, versions_root when standalone; always first_setup_done:true) | C:748-838 |
| Waiting (another game) | "Already running: X ... B: cancel" | polls 4x/s, starts when free | C:871-881, C:973 |
| Job / build | REC, title, progress bar (done/total), crate, compiled count, warnings pill, elapsed; WAITING card with holder and "B twice to cancel"; fetching card | taskkill /T /F on second B | LcdJob C:2180-2225, C:997-1002 |
| Finished / "Finished, check this" | last_session.txt + build line, or "First error:" mono block | bad = MALFORMED / No new / Not started / panic(s) / failed / Still missing | C:898-921 |
| Previous session / Launcher updated | shown at start when pending exit 10 or `--after-update` | | C:584 |
| Import DLC | tool-dlc job; dialogs come from PowerShell (topmost owner form) | | L:99-128 |

Start order: Main, then SetupPage when `setup_missing`, then Wizard when `first_setup` (C:581-583).
Game hide: when a game process shows a visible main window during a job, the panel folds (Standby) and hides,
the game is brought to the front (C:923-935, C:960); when the job ends the window shows and swings open (C:898-904).
Direct mode (launch options): `entry [label] [@version]`; version-*/notes show text and close in 15 s; others run as
a job and the result closes in 10 s (C:590-603, C:916). Restart after update: `restart.txt` -> start new exe with the
same args (+`--after-update`), hide, close when the child exits (C:507-529). Single instance: named mutex
`Local\SkateLauncher-<folder lowercase, slashes>`; a second start fronts the existing window (C:2307-2318).

## 3. Inputs

| Source | Mapping | Ref |
|---|---|---|
| XInput pads 0-3 (first connected) | A ok, B back, d-pad or left stick (|16000|) up/down/left/right; held up/down repeats after 14 ticks every 4 (16 ms tick) | C:937-951, C:983-993 |
| Keyboard | Up/W, Down/S, Left/A, Right/D, Enter ok, Esc/Backspace/Space back | C:1003-1019 |
| During a waiting build/fetch | B / Esc / Backspace: first press arms, second cancels (taskkill tree) | C:977-981, C:997 |
| During start self-fetch | same two-press skip, kills `pending` and reruns with `SKATE_LAUNCHER_NO_SELF=1` | C:545-573 |
| Pad + key de-dup | same action from the other source within 150 ms dropped (Steam desktop layout sends keys) | C:1067 |
| Mouse | hover selects; left click selects + ok; right click back; click on result with auto-close closes; drag bezel (WM_NCLBUTTONDOWN, HTCAPTION) | C:1046-1064, C:1245-1248 |
| Bezel keys | HOME=back, T/W=up/down, SET=ok, bezel PLAY=ok, STOP=back, REW/FF=left/right, START/STOP=play on main (select item 0 + ok), POWER=close, DISPLAY (drawn only, no action found) | C:1033-1045, D41:16-25 |
| Left/right in lists | same as up/down (linear step, also in 2-col grids) | C:1079-1080 |
| Flags | `--snapshot <png> [screen][@0..1][#seed] [WxH]` (screens: main versions tools setup play play-traceall waiting fetching checking job build failed), `--bench <txt> [WxH]` (default 1243x777), `--wear-test <txt> <png>`, `--standby-test <txt> <png>`, `--same-monitor`, `--after-update` (internal), launch options as direct mode | C:2280-2306, C:355-404, R:13-20 |

Open position: 62 % of the work area height, aspect 1240:760, centred on the primary screen, or with
`--same-monitor` / `open-on-launching-monitor.txt` on the monitor of the foreground window (else under the cursor)
(C:197-204, C:467-471).

## 4. State folder and files

State dir: `<repo>\.local\steam-launcher` when the launcher lives under `<repo>\tools\`, else the launcher folder
(L:30-31, C:458-463). Standalone: `STANDALONE.txt` beside the script (L:24-25).

| File | Format / writer | Ref |
|---|---|---|
| config.json | JSON; keys: remote, fork_url, upstream_url, target_root, build_cap_gb, build_cap_dir, build_reserve_gb, after_session, recomp_script, recomp{exe, working_dir, args, sessions, trace_check}, recomp_entries[{id, name, hint, trace, capture, ask_label, label, folder, bat, bat_args}], rpcs3{exe, game, args}, rpcs3_entries[{id, name, hint, args}], storage_root, versions_root, disc, first_setup_done, builtin_versions, use_releases, self_update, launcher_branch, dlc_dir | L:45-77, L:145, R:154-168 |
| versions.json | array of {id, name, branch, url, dir, detached, notes, mods_enable, env{}, dev, target, release} | L:150-172, R:136-149 |
| version.txt | selected id, ASCII | L:1349 |
| last_session.txt | `<yyyy-MM-dd HH:mm:ss>  <key> <label> [<ver>]` then CRLF summary lines, UTF-8 (PS 5.1 Set-Content UTF8 adds a BOM) | L:1078, L:1232 |
| PLAYING.txt | `entry=<k> label=<l> version=<id> started=<yyyy-MM-dd HH:mm:ss>` ASCII; exists while a game runs | L:1174 |
| REFRESHING.txt | version name while Refresh Game Files runs | L:1090 |
| BUILDING-<id>.txt | `pid\tname\tISO-time\twindow|shell` | L:634 |
| restart.txt | short sha or `local`: new exe waiting | L:1243, C:508 |
| config-set.json | wizard's changed keys, deleted after merge | C:833 |
| open-on-launching-monitor.txt | exists = on (`on\n`) | C:465, C:476 |
| camcorder-sticker.txt | 6 invariant floats separated by spaces: design, zone(0 bezel,1 lens,2 body print), place x, place y, angle (-20..20), wear seed | C:1660-1678 |
| stickers\*.png | replace the built-in sticker set | C:1683 |
| self.git, self-update.log, self-update-backup\<time>\, SkateLauncher.exe.source-sha256, SkateLauncher.old.exe | self update | L:404-524 |
| dlc\<pack name>\<file> | imported DLC (or `dlc_dir`) | L:130-147 |
| versions\<id> (standalone) or .local\versions\<id> | worktrees / clones / release installs | L:157-162 |
| per version: bin\built_commit.txt, logs\launcher-build-*.log, logs\game-*.log(+.stderr.log), logs\trace-all-*, release.json | | L:576-596 |
| repo .local\audio-state-logs\state_*.tsv, recomp sessions dir | | L:38, L:684 |

## 5. Per-OS notes

| Item (Windows today) | Linux | macOS |
|---|---|---|
| powershell.exe backend, csc.exe window, build.bat / launcher.bat | one Rust binary; shell script wrapper | same; .app bundle optional |
| `PLAY.bat` via `cmd /c` | the version's PLAY script for the OS (check what engine PLAY.sh exists; unverified) or `bin/skate3rust` directly | same |
| `scripts\Build.ps1 -TargetDirectory` | needs a POSIX build entry (cargo build --locked + post steps); open: Build.ps1 parity | same |
| process names skate3rust.exe / skate3.exe / rpcs3.exe | `skate3rust`, `rpcs3` (AppImage name varies), recomp: none | `skate3rust`, `rpcs3.app` |
| Recomp (skate3recomp, Windows build) | grey out ("Windows only"); maybe via Wine later | grey out |
| RPCS3 | native Linux build (AppImage); same args | native macOS build |
| winget + UAC installs (Git, VS Build Tools, LLVM, Python 3.13, rustup, gh, Vulkan check) | apt/dnf/pacman via pkexec once; rustup script; Vulkan loader + mesa; `python3.13` not `py -3.13` | brew; Xcode CLT; MoltenVK; no elevation for brew |
| `cmd /c mklink /J` asset junctions | symlinks (std::os::unix::fs::symlink) | symlinks |
| CIM query of cargo.exe command line | /proc/<pid>/cmdline (sysinfo crate) | sysinfo crate |
| taskkill /T /F | kill process group (setsid + killpg) | same |
| robocopy, Invoke-WebRequest, ZipFile, Get-FileHash | Rust: reqwest/ureq, zip, sha2, fs copy | same |
| release asset `skate3rust-windows-x64.zip` | needs `-linux-x64` release assets (not published today: grey out prebuilt) | `-macos-*` same |
| XInput | SDL3 gamepad (already vendored sdl3-sys) | SDL3 |
| Layered per-pixel-alpha window (UpdateLayeredWindow), drag by caption message | transparent undecorated winit window; X11 needs compositor, Wayland ok; drag via `drag_window()` | transparent works |
| Named mutex single instance + bring to front | lock file / abstract socket; raise via IPC message | same |
| WinForms file / folder dialogs (also inside PowerShell for DLC) | rfd (portal / GTK) | rfd |
| Steam non-Steam shortcut, desktop layout (keys) | Steam on Linux same flow; Steam Deck game mode: no compositor transparency, treat as fullscreen | same |
| Foreground handover to the game (AllowSetForegroundWindow, Front) | hide window; the WM focuses the game | same |
| Fonts Segoe UI / Tahoma / Consolas / Segoe MDL2 / Ink Free | bundle open fonts (e.g. Inter/Selawik-like, DejaVu Mono, an open handwriting font); icon glyphs as vectors | same |
| OEM code page decoding of child output | UTF-8 | UTF-8 |
| `%USERPROFILE%\.cargo\bin` prepend | `$HOME/.cargo/bin` | same |
| Paths with `\`, `C:\path\to` placeholder | PathBuf; placeholder check per OS | same |
| self-update copies tools/steam-launcher and rebuilds exe | replace own binary from a release asset per OS, or cargo build; restart-in-place via re-exec | same |
| share / send / recomp share scripts (`.ps1`) | need pwsh or ports; grey out until ported | same |

Hardest: (1) Build.ps1 and PLAY.bat have no POSIX twin in this launcher's flow; (2) transparent shaped window plus
fold/hide around a fullscreen game on Wayland / Steam Deck; (3) self-update of a compiled Rust binary in place;
(4) the PowerShell helper scripts (send, share, recomp share, setup-dev-assets.ps1) the backend shells out to.

## 6. Camcorder look (repaint in egui)

Unit space 1240 x 760, scaled uniformly and centred (Render C:1251-1265). Panel `PanelR` (20,20,1172,720) radius 34,
screen `ScreenR` (172,52,990,557) (C:1161). Colours: Plastic 54,56,62; PlasticHi 100,103,112; PlasticDark 24,25,29;
Print 200,202,208; RecRed 230,40,32 (C:1162-1163); LCD ink 244,246,250, dim 168,178,196, amber hi 255,198,88 / lo
238,124,12 / mid 255,172,44, green 124,220,96, red 255,98,86 (C:1910-1911).

| Element | Detail | C# function |
|---|---|---|
| Panel body | vertical gradient 76,78,86 -> 32,33,38, black 210-alpha outline 3; inner lip ring white 30-alpha at inset 10 r26; model print "SK8-1000  HD" Tahoma 18 bold at (822,700); sensor dot (1144,30,12) | LcdPanel C:1293-1349 |
| Gasket, glass, screen | dark gasket round ScreenR, LCD bitmap drawn into it at `ScreenLevel` brightness | LcdPanel, LcdCanvas C:1916-1953 |
| Bezel keys | HOME (40,70,110,48), SET (40,372,110,48), POWER (48,588,94,36), DISPLAY (716,640,84,32); T/W rocker, START/STOP, tape keys (PLAY/STOP/REW/FF) along the bottom (coords unverified); key face 92,95,104 -> 44,46,52, hover 132,136,146 -> 70,72,80, label Tahoma 19 bold | HardKey C:1278-1283, LcdPanel |
| Hinge barrel | rounded cylinder (1186,150,38,460) r16, gradient Dark/Hi/Plastic/Dark at 0/.28/.62/1, groove lines every 70 from y 200 | Hinge C:1285-1291, Cylinder C:1267 |
| Open / close swing | power-on 1.2 s (`OnTotal`), close 1.3x faster; swing time 0.08+0.6*u(0.85,1.15); BackOut overshoot u(0.2,1.2); screen-on delay with optional flicker (0..2 dips); panel drawn as perspective strips from a cached bitmap while turning | Assembly, Vary, PanelOpen, ScreenLevel C:1192-1216; FlipPanel C:1405, Slice C:1387, PanelShot C:1373; Rust: `swing_panel` in camcorder.rs tessellates the panel's shapes and moves every vertex with the same perspective and darkening (triangles split to 1/32 of the width in place of the 48 strips). Mine, not the classic's (the user asked for it to feel like a camera's flip screen): opening is pulled on a friction hinge (constant acceleration over the first 25 %, steady, braking over the last 30 %, then a 0.8..1.4 % detent click), closing is pushed against the friction, slows near the body and the latch snaps it shut with a 1.8 % rebound (`trapezoid`, `heavy_open`, `heavy_close`); swing 0.08 + 0.8 x u(0.85,1.15) s, power-on 1.65 s; one camera for the turning panel (mine; the classic's eye is on the hinge line with a 0.6 depth factor): the eye in front of the panel's centre, 2.4 panel widths back, so the face recedes towards the middle; the panel is 6 % of its width thick and its hinge-end side faces you on the right of the front face as it turns: the right half of the face outline (both 34-radius corners and the edge) extruded back in the same projection, with front / back shells and a seam, so it meets the top and bottom edges (`hinge_side`); the hinge barrel is drawn again in front of it. The power-on clock starts once the window is on screen and Windows' own open / close animation is off for the window (`window::own_window_shown`, DWMWA_TRANSITIONS_FORCEDISABLED): before, the first part of the swing ran before the window showed. Colour helpers (`rgba`, `mix`, `lerp`) work on unmultiplied channels (Color32 is premultiplied; reading it as unmultiplied applied alpha twice and made the screen sheen and other translucent fills nearly invisible). `SKATE_LAUNCHER_FRAMELOG=<file>` logs every animated frame (clock, open, level). |
| Standby fold / resume | fold with screen lit when a game shows, hide; resume lit and recording | Fold/Resume C:1190-1191, LitFlip C:1367 |
| LCD scene | dimmed dusk skate spot behind menus; skateboard rolling along the bottom with ollies (speed u(120,190), offset, rhythm u(5.5,8)) | Scene C:1955, LcdBoard C:2071, Board C:1128, Rolling C:1124 |
| Status row | STBY / REC + glowing dot while a job runs, HDV 1080i badge, tape counter (runs only while recording), glossy battery | LcdStatus C:2014, Badge C:2044, Pill C:2004 |
| Lists | blue HOME/title tab, glass panels, amber glossy selection bar, small amber dot for the current version, 2-col tiles on Main/Play | LcdPage C:2087-2132, Glass C:1990, Gloss C:1998 |
| Info panel | NOW SELECTED, READY / NOT AVAILABLE pill, info, records (`#Files:` headings in amber), play prompt (pulsing dot + START/STOP badge "to play") | LcdInfo C:2134-2178 |
| Job card | glossy progress bar (red hues when failed), counts, warnings pill, first error mono, waiting card | LcdJob C:2180-2225 |
| Splash | wordmark (Segoe UI Black / Light, h/11), stage text, three pulsing amber dots | LcdSplash C:2227-2252 |
| Footer | A / B / START-STOP badges, date and time stamp | LcdFoot C:2055 |
| Wear | rolled each start: marker tags (skate words), doodles (crown, star, arrow, X, smiley), scratches, edge rub, gaff tape; only on the wear map (bezel minus keep-out zones), avoiding sticker and each other, shrinking to fit; hand font Ink Free > Segoe Print > Comic Sans | RollMarks C:1457, WearMap C:1488, TagMap C:1494, DrawMarks C:1509-1537, FindRoom C:1566, MarkerTag C:1582, Doodle C:1608, GaffTape C:1629, HandFamily C:1449 |
| Sticker | one per user, saved; designs: filmer character die cut (FilmerCut), parodies Blickey, Spitball Wheels, Supremely Mid, Thrashed; faded, scratched, peeled corner, creases over gasket and lip ridges | StickerSpec C:1665, GameSticker C:1680, PlaceSticker C:1691, DrawSticker C:1708, StickerShape C:1818, StickerArt C:1837, Ridges/Creases C:1641-1663 |
| Fonts (window-height scaled) | title h/24, hero h/17, item h/34, small h/52, mono Consolas h/50, body h/42, mark h/30, play h/26, icons Segoe MDL2 | MakeFonts C:220-237 |
| Present | whole frame into one bitmap, `UpdateLayeredWindow` with alpha; 16 ms timer, repaint every 2nd tick when visible | Native.Push C:49, Present C:1226, Poll C:952 |

egui notes: render to an offscreen texture in unit space (painter shapes + gradients via meshes), flip by mesh
strips; keep a deterministic seed path for `--snapshot` / `--wear-test`.

## 7. Suggested Rust layout and parity checklist

Crate `tools/skate-launcher` (workspace member, pinned deps):
`main.rs` (flags, single instance, CLI vs window), `cli.rs` (the commands of section 1a/1b with identical output),
`state.rs` (state dir, config.json / versions.json serde with unknown-field passthrough, all files of section 4),
`versions.rs` (built-ins, git worktree / clone / fetch / ff, status, releases), `build.rs` (cargo run, holders, size
cap, progress parser = BuildView), `play.rs` (entries, env per entry, PLAYING.txt, game wait, recomp, rpcs3),
`session.rs` (Check-Session, pending, send, share), `settings.rs` (game settings rows), `dlc.rs` (header check,
import), `setup/{windows,linux,macos}.rs` (tool checks + installs), `selfupdate.rs`, `platform/{windows,unix}.rs`
(process names, kill tree, links, monitor, foreground), `ui/{app,pages,input,camcorder,lcd,wear,sticker}.rs`.

Parity checklist (one line per behaviour):
- [ ] `entries` prints 9 tab columns per entry in the same order and grey-out texts as L:1287-1300.
- [ ] `versions`, `status`, `notes`, `config-get`, `sizes`, `game-settings`, `setup-check` byte-compatible rows.
- [ ] Built-ins hails-additions / upstream-main, `builtin_versions`, versions.json overrides by id, default `this` version.
- [ ] version-next / prev wrap around; version-set writes version.txt.
- [ ] Ensure-Version: worktree (detached for url/detached), clone when standalone, fetch with timeouts and no prompt, ff-only, local changes kept.
- [ ] Rebuild only when built_commit.txt differs; dev versions hash the diff of the listed inputs.
- [ ] Build log path/name, newest 10 kept, written by the UI side from the stream.
- [ ] BUILDING-<id>.txt holder, cargo-holder detection, exe-running wait, WAITING card, two-press cancel.
- [ ] Size cap: reserve, cap_dir, oldest-built first, never the one being built.
- [ ] Release versions: release.json, zip + sha256 check, copy over, offline fallback to installed.
- [ ] Each rust entry sets exactly the env of section 1c; trace-all only the switches the source has.
- [ ] Recomp entries: session folder naming, trace env, capture, ask_label LabelPage, bat entries.
- [ ] RPCS3 args default and per-entry override.
- [ ] One game at a time; Waiting screen auto-continues; PLAYING.txt written and removed; stale one cleared at start.
- [ ] Check-Session texts (no panics / N panic(s), rows, malformed, MALFORMED - unusable).
- [ ] last_session.txt format; "Finished, check this" triggers.
- [ ] pending: self-update, asset link repair, newest-session detection, missed send, exit 10 semantics.
- [ ] Self-update: skip while a game runs, `self_update:false`, backup of differing files, restart into new binary with --after-update.
- [ ] Refresh Game Files: REFRESHING.txt, GAME_FILES line, exit code 2 text.
- [ ] Import DLC: LIVE/PIRS/CON header, content type 2, title 0x454108E6, name at 0x411, sanitised folder, skip same size, then refresh.
- [ ] Clear builds: sizes, confirmations, refused while playing, GB freed line.
- [ ] Opt-ins / settings stepping; refused while playing.
- [ ] Share consent page and on/off; send blocked reasons.
- [ ] Wizard: fields, cap cycle, recomp exe search, only changed keys saved, storage_root derived keys, first_setup_done.
- [ ] First-time setup page and installs per OS.
- [ ] Inputs: pad, stick threshold, hold repeat, keys, 150 ms pad/key de-dup, mouse hover/click/right-click, drag, bezel keys.
- [ ] Direct mode: `entry [label] [@ver]`, auto-close 10 s / 15 s.
- [ ] Single instance per folder, second start raises the window.
- [ ] Open on main monitor or launching monitor (flag and file).
- [ ] Fold/hide when the game window shows, resume with result; power-on / power-off animations; close plays power-off.
- [ ] Wear and sticker: rolled marks stay on the wear map (`--wear-test` PASS), sticker persisted in camcorder-sticker.txt, stickers\*.png override.
- [ ] `--snapshot` all screens, `@` and `#` suffixes, `--bench` output.
- [ ] Linux / macOS: recomp greyed with reason; installs via the OS package manager; symlinks instead of junctions.

## Open items

- Not read: Write-ReleasePlay contents (L:234-240), Self-Update file list and `$SelfCommit` name (L:470-524),
  Share-Status / Share-Sessions details (L:920-944), TraceAll-Records (L:1022-1041), LcdPanel key coordinates other
  than HOME/SET/POWER/DISPLAY, StandbyTest, Bench output format, the exact Main tile layout maths.
- POSIX play / build per version: the imported Linux work adds `BUILD.sh` / `PLAY.sh` at the checkout root (doc 44); whether versions use them is open.
