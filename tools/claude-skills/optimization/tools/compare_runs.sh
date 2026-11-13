#!/usr/bin/env bash
# Compare two e2e_bench.sh runs: output hashes (must be identical) and the timing summaries.
#   .claude/skills/optimisation/tools/compare_runs.sh BASE NEW
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/.local/audio-re/opt/runs"
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
py -3.13 "$ROOT/.claude/skills/optimisation/tools/timing_summary.py" "$R/$1" "$R/$2"
exit $status
