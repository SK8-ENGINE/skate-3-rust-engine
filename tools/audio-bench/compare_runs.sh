#!/usr/bin/env bash
# Compare two e2e_bench.sh runs: output hashes (must be identical) and the timing summaries.
#   tools/audio-bench/compare_runs.sh BASE NEW      (runs under BENCH_DIR/runs, default .local/audio-bench/runs)
# Exit 1 if any output differs.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
R="${BENCH_DIR:-$ROOT/.local/audio-bench}/runs"
[ $# -eq 2 ] || { echo "usage: compare_runs.sh BASE NEW"; exit 2; }
status=0
for d in "$R/$2"/*/; do
  s=$(basename "$d")
  if [ ! -f "$R/$1/$s/hashes.txt" ]; then echo "$s: no baseline"; continue; fi
  if diff -q "$R/$1/$s/hashes.txt" "$d/hashes.txt" > /dev/null; then
    echo "$s: IDENTICAL ($(wc -l < "$d/hashes.txt") files)"
  else
    echo "$s: DIFFERENT"; diff "$R/$1/$s/hashes.txt" "$d/hashes.txt" | head -20; status=1
  fi
done
"${PYTHON:-python}" "$ROOT/tools/audio-bench/timing_summary.py" "$R/$1" "$R/$2"
exit $status
