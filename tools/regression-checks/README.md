# Regression checks

Checks to run after changing the asset pipeline, the converter, input, native dependencies, build
scripts or game code, before calling a change done. Each one compares the current install or build
against a baseline you record yourself on a known-good state.

| Script | What it checks |
|---|---|
| `check_maps.py` | Loads every installed map in one game process (`skate3rust --validate-maps`) and compares each map's loaded collision triangle count with the baseline. Fails on any map error or warning (spawn support, startup, invisible floor). Falls back to one `--check-assets` launch per map on builds without `--validate-maps`. |
| `check_spawns.py` | Reads the spawn baked into every installed `.skate` map and compares it with the baseline (0.01 m tolerance). |
| `check_customiser.py` | Snapshots the character customiser outputs (every stage receipt: path, size, SHA-256 of each file) and compares a later install with it. Ignores differences that are only the random set id embedded in JSON paths. |
| `smoke_maps.sh` | Muted crash smoke test: launches the game on each map for N seconds (windowed, `--mute`) and counts panics, `ERROR` lines, non-finite physics and render-ready lines in the log. |

## Inputs

- An installation set up by `tools/setup.py` (`data/installation.json`, `data/installations/<id>/`).
- `bin/skate3rust.exe` for `check_maps.py` (`--exe` for another build) and `smoke_maps.sh` (`SKATE_EXE`).
- Baselines live in `.local/regression/` (gitignored) unless you pass `--baseline`; the customiser
  snapshot in `.local/customiser-baseline/` (`--snapshot`).

## Usage

From the repository root:

```
# once, on a known-good build / install
py -3.13 tools/regression-checks/check_maps.py --update
py -3.13 tools/regression-checks/check_spawns.py --update
py -3.13 tools/regression-checks/check_customiser.py --save

# after a change
py -3.13 tools/regression-checks/check_maps.py
py -3.13 tools/regression-checks/check_spawns.py
py -3.13 tools/regression-checks/check_customiser.py

# smoke test (Git Bash): 25 s per map, default maps StartPark University DownTown MegaPark
bash tools/regression-checks/smoke_maps.sh 25 StartPark Industrial
```

Every check exits 1 on a difference. The smoke test opens a game window per map; don't run it while
someone is playing on the same machine.

## Example output

`check_maps.py` (made-up numbers):

```
TEST_WORLD           ...
MapA                 12345  ok  (0.41s)
MapB                 67890  CHANGED (baseline 67880, delta 10)
```

`smoke_maps.sh` (from a real run; exit 124 = stopped by the timeout, as intended):

```
StartPark exit=124 panics=0 errors=0 nonfinite=0 render_ready=1 native_lines=7
University exit=124 panics=0 errors=0 nonfinite=0 render_ready=1 native_lines=7
logs: <repo>/.local/smoke/20261003_090413
```

## Requirements

- Windows, Python 3.13 (standard library only).
- `smoke_maps.sh`: Git Bash (`timeout`, `taskkill`).
