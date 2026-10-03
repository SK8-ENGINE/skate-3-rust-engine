# Audio bench

Tools for speeding up the native audio without changing what it does: measure first, prove the output
is identical, then compare timings. Built on the headless e2e render (see `tools/audio-e2e`).

| Script | What it does |
|---|---|
| `e2e_bench.sh` | Renders every scenario (and optional real-play sessions) headless with timing on, in one or more host modes (`row`: one call per 60 Hz row; `fpsN`: the game's host at N fps), and writes SHA-256 hashes of every output plus the timing log under a label. Uses its own cargo target folder so it never invalidates your main build. |
| `compare_runs.sh` | Compares two labels: output hashes (must be IDENTICAL) and the timing summaries side by side. Exit 1 on any difference. |
| `timing_summary.py` | Pools the per-frame (game thread) and per-block (render) times of one or more runs: p50 / p90 / p99 / p99.9 / max and total. |
| `audio_timing_summary.py` | Summarises the `AUDIO_TIMING` lines a real play session logs with `SKATE_AUDIO_TIMING=1`: per audio system, the average, the per-second maximum (median / p90 / worst) and calls per second. |
| `emitter_bank_memory.py` | Resident memory of the world emitter banks per `.ems` emitter file (or per group of files, e.g. a map's), from the install's audio manifest. |
| `prefetch_memory_sim.py` | Simulates a distance prefetch of emitter banks over a play session's audio state log and reports loaded and prefetched memory peaks. |

## Inputs

- `BENCH_DIR` (default `.local/audio-bench`): `scen/*.tsv` from `tools/audio-e2e/scenarios.py BENCH_DIR/scen`,
  optional `real/*.tsv` from `scenarios.py --from-log LOG --cut r0-999999 NAME BENCH_DIR/real`.
- An installation with the native audio data (setup group `audio`).
- Game logs (`logs/game-*.stderr.log`) from a session run with `SKATE_AUDIO_TIMING=1`.
- Audio state logs from a session run with `SKATE_AUDIO_STATE_LOG=<path>`.

## Usage

```
bash tools/audio-bench/e2e_bench.sh base               # before the change (keeps the renders)
bash tools/audio-bench/e2e_bench.sh new row fps30 fps300
bash tools/audio-bench/compare_runs.sh base new
py -3.13 tools/audio-bench/audio_timing_summary.py logs/game-20260101-120000.stderr.log
py -3.13 tools/audio-bench/emitter_bank_memory.py --group University=sfx_university,music_university
py -3.13 tools/audio-bench/prefetch_memory_sim.py my_state_log.tsv sfx_university --ahead 60 --evict 90
```

`KEEP=1` keeps the `.f32` renders of a non-base run; `CARGO_TARGET_DIR` overrides the bench target folder.

## Example output

`compare_runs.sh` (made-up timings):

```
scen-row: IDENTICAL (26 files)
scen-fps300: IDENTICAL (26 files)
scen-row block:
              base: n  41000  p50   210.0  p90   260.0  p99   340.0  p99.9   520.0  max   900.0  sum    8610.0 ms
               new: n  41000  p50   130.0  p90   170.0  p99   230.0  p99.9   400.0  max   700.0  sum    5330.0 ms
```

## Requirements

- Git Bash (`sha256sum`, `diff`), the Rust toolchain.
- Python 3.13 (standard library). `compare_runs.sh` calls `python` (set `PYTHON` to change it).
