#!/usr/bin/env bash
# Native-audio optimization bench (skill `optimization`): renders the e2e scenarios and real-play
# replays headless, records per-frame / per-block timing, and hashes every output.
#
#   .claude/skills/optimization/tools/e2e_bench.sh LABEL [MODE...]
#
# MODE: row (one host call per 60 Hz row, the default e2e), fpsN (the game's host at N fps:
# E2E_FPS=N, console MixMap cadence; e.g. fps30, fps300, fps1000). Default: row fps300.
# Inputs: .local/audio-re/opt/scen/*.tsv (the 13 e2e scenarios) and .local/audio-re/opt/real/*.tsv
# (whole play sessions via `scenarios.py --from-log LOG --cut r0-999999 NAME`).
# Output: .local/audio-re/opt/runs/LABEL/<set>-<mode>/ with timing.txt and hashes.txt
# (sha256 of every .ours.f32 and .ours.voices.tsv). KEEP=1 keeps the .f32 renders (else deleted
# after hashing, except for LABEL=base). Compare two labels with compare_runs.sh A B.
# Uses its own target dir (.local/opt-target) so it never waits on, or invalidates, the main build.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.local/opt-target}"
LABEL="$1"; shift
MODES=("$@"); [ ${#MODES[@]} -eq 0 ] && MODES=(row fps300)
for set in scen real; do
  for mode in "${MODES[@]}"; do
    out="$ROOT/.local/audio-re/opt/runs/$LABEL/$set-$mode"
    rm -rf "$out"; mkdir -p "$out"
    cp "$ROOT/.local/audio-re/opt/$set/"*.tsv "$out/"
    envs=(E2E_DIR="$out" E2E_TIMING=1)
    case "$mode" in fps*) envs+=(E2E_FPS="${mode#fps}");; esac
    (cd "$ROOT" && env "${envs[@]}" cargo test -q -p skate-game --release --bin skate3rust --locked -- --ignored --exact game_audio::e2e::e2e_render --nocapture) > "$out/timing.txt" 2>&1
    (cd "$out" && sha256sum *.ours.f32 *.ours.voices.tsv > hashes.txt)
    if [ "$LABEL" != base ] && [ "${KEEP:-0}" != 1 ]; then rm -f "$out"/*.ours.f32; fi
    echo "$LABEL $set-$mode: $(wc -l < "$out/hashes.txt") outputs hashed"
  done
done
