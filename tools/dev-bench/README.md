# Dev bench and test-run tools

The scripts behind the measurements and checks in `docs/hails-additions/` (frame time, start-up, map switches, setup
speed, NPC runs, captures). The doc sections name the script behind their numbers. Windows only;
the shell scripts run in Git Bash, the `.ps1` scripts in Windows PowerShell 5.1, the Python ones with `py -3.13`.

## Before a run

- A checkout with a converted installation (`data/installation.json`, from `tools/setup.py` or the launcher) and a
  release build in `bin/` (`scripts/Build.ps1`).
- One game at a time: every game-running script stops when a `skate3rust` (or recomp / RPCS3) process runs, or while
  the Steam launcher marks a player session (`.local/steam-launcher/PLAYING.txt`).
- Runs are muted, use their own empty mods folder (`.local/dev-bench/empty-mods`) unless a script says otherwise, and
  set `SKATE_REPORT_CHILD=1` so a timeout kill opens no crash-report window (`frame-time-run.ps1` loads the checkout's
  mods unless `-NoMods`). The benches that change settings (`traceall-gpu-bench.ps1`, `water-look-bench.ps1`) use
  their own settings folder; the others run the game with the working folder's settings.
- Shell scripts share `common.sh`: `REPO` (default: this checkout), `EXE` (default `bin/skate3rust.exe`), `DEPS`
  (the exe's DLL folder when the exe is a saved copy elsewhere), `RUN_DIR` (the game's working folder).
- Compare like with like: the same build for before / after (most switches have an env var, named in the doc), runs
  interleaved (A B A B), and nothing else heavy on the machine. Drop runs another process disturbed.

## Tests

| Script | What it does |
|---|---|
| `pr-tests.sh [out-dir]` | Every suite of PR 52's tests table in order (skate-core, skate-game, skate-mods, the data-gated run with the installation's maps, setup and tools), then one summary: result lines, skipped tests (private inputs missing) and every failure. Finds cmake; `INSTALL`, `JOBS`, `PY`, `SKIP_IGNORED=1` in its header. |

## Frame time and play

| Script | What it does |
|---|---|
| `lw_bench.sh <exe> <out> [s]` | Muted living-world bench at the University library: NPC Skater Physics on, the player off the board, the frame log and the timing readouts, then a summary (percentiles, LIVING_TIMING / FIXED_TIMING per frame). `PLAIN=1` keeps only the frame log. |
| `frame-time-run.ps1 -Checkout <dir> -Map <.skate>` | Minimised trace-all run of a checkout's build: median / p90 / p99 frame time of frames 100+, error and hitch lines. |
| `map-switch-run.ps1 -Checkout <dir>` | Starts on one map, switches to another after N seconds (`SKATE_MAP_SWITCH`), prints the hitches after the switch. |
| `traceall-gpu-bench.ps1` | Frame-time cost of trace-all's GPU queries, one run per mode. |
| `water-look-bench.ps1` | Frame-time cost of the Better Water look and each of its pieces, two rounds in opposite order. |
| `tick_gaps.py <trace.json> [steps]` | What fills each fixed step in a Chrome trace (`profiling.rs` capture), and the uncovered time between spans. |
| `npcsim.sh <exe> <out> [s]` | Muted NPC Skater Physics run with the player idle, then counts of replays, bails, path lines, sim loads, long frames and panics. |
| `npc_matrix.sh <out-dir> <s> "<ENV_A> <ENV_B>" "<MAP:TELEPORT> ..."` | `npcsim.sh` for every on/off combination of the given switches at several spots, one row per run. |
| `npc_grinds.py <stderr.log> [window_s]` | Per recorded grind in a session log: did the NPC catch it. |
| `park-catches.py <audio_state.tsv> ...` | Where the skater catches on the ground (speed loss, wheel contacts, bails) in a trace. |
| `park-seam-edges.py <layout.json> x y z` | Edges shared by more than two triangles near a point of a park layout (seams between pieces). |

## Start-up

| Script | What it does |
|---|---|
| `startup_bench.sh <exe> <out> [s]` | Muted start: the `STARTUP_STEP` timeline (ms since `main`), the first frames, the map fingerprint. `COLD=1` empties the file cache first, `TIMING=1` adds the Startup systems' costs. |
| `evict.ps1 <file or folder>...` | Drops files from the Windows file cache without admin rights (opens each unbuffered); `EVICT_SKIP=speech` skips folders by name. Nearly all of its time is the per-file opens (about 7 ms each on a hard disk). |

## Setup

| Script | What it does |
|---|---|
| `setup_bench.py --checkout ... --base ... --game-exe ... --source ... --label ...` | Timed full setup into a scratch base plus a sha256 manifest of the output; compare two manifests to prove a setup change keeps the converted files the same. |
| `refresh_bench.py TREE BENCH_BASE GAME_EXE GROUP LABEL` | Times a one-group Refresh Game Files on a bench copy of an installation and hashes the result. |
| `rehash_check.py TREE BENCH_BASE` | Checks the refresh's size + time shortcut on a bench installation (changes one file, restores it). |
| `with_affinity.py <mask> <command>...` | Runs a command pinned to a CPU mask, e.g. to bench setup as a smaller machine. |
| `recipe-precedence.py <checkout> <disc> <dlc> <work>` | Lists recipe pieces whose rows differ between the disc and the DLC packs. |

## Captures

| Script | What it does |
|---|---|
| `fountain-capture.sh <map> <teleport> <out.png>` | One muted `--verify` capture with `mods/fountain-cam` (Better Water on, a fixed camera; edit its `SPOT`). |
| `falls-capture.sh <out.png> <on\|off> "<player>" "<eye>" "<look>"` | The same for any spot, writing its camera mod per run. |
| `crop-stats.py <png> x0 y0 x1 y1` | Colour statistics of a rectangle in a capture. |

The audio benches (render cost, byte-identity of the native audio) are in `tools/audio-bench`, the map / spawn /
customiser checks in `tools/regression-checks`, and the per-topic helpers in `tools/claude-skills/<skill>/tools`.
