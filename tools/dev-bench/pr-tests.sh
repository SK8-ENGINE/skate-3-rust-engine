#!/usr/bin/env bash
# The test suites PR 52's tests table reports, one after another, with one summary at the end
# (docs/hails-additions/36-known-test-failures.md#skipped-without-private-inputs).
# usage: pr-tests.sh [out-dir]      Git Bash on Windows, from any folder; logs go to out-dir
#   INSTALL        the installation folder (default: the one data/installation.json names)
#   JOBS           cargo -j (default: half the logical cores)
#   PY             Python with numpy (default: .local/venv-setup's python if present, else py -3.13)
#   SKIP_IGNORED=1 only the plain suites (no converted assets needed)
#   CMAKE          cmake.exe for sdl3-sys (default: cmake on PATH, else the copy scripts/Ensure-CMake.ps1 finds)
#   Data-gated inputs you have (SKATE_AUDIO_STATE_LOGS, SKATE_RECOMP_SESSIONS, ...) are passed through; tests whose
#   private inputs are missing print TEST_SKIPPED and count as skipped.
set -u
R="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/../.." && pwd)"
OUT="${1:-$R/.local/dev-bench/pr-tests-$(date +%Y%m%d-%H%M)}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
cd "$R" || exit 3
export SKATE_REPORT_CHILD=1 SKATE_TEST_SKIPS="$(cygpath -w "$OUT/skipped.txt")"
rm -f "$OUT/skipped.txt" "$OUT/summary.txt"

if [ -z "${CMAKE:-}" ] && ! command -v cmake > /dev/null; then
  c="$(powershell -NoProfile -ExecutionPolicy Bypass -Command \
    ". '$(cygpath -w "$R/scripts/Ensure-CMake.ps1")'; \$env:CMAKE" 2> /dev/null | tr -d '\r')"
  [ -n "$c" ] || { echo "cmake not found: install it or set CMAKE to cmake.exe"; exit 3; }
  export CMAKE="$c"
fi
JOBS="${JOBS:-$(( $(nproc) / 2 ))}"; [ "$JOBS" -ge 1 ] || JOBS=1
if [ -z "${PY:-}" ]; then
  if [ -x "$R/.local/venv-setup/Scripts/python.exe" ]; then PY="$R/.local/venv-setup/Scripts/python.exe"; else PY="py -3.13"; fi
fi
if [ -z "${INSTALL:-}" ]; then
  d="$(sed -n 's/.*"directory": *"\([^"]*\)".*/\1/p' "$R/data/installation.json" 2> /dev/null)"
  [ -n "$d" ] && INSTALL="$R/data/$d"
fi

run() { # run <name> <command...>
  local n="$1"; shift
  local t0=$SECONDS
  "$@" > "$OUT/$n.log" 2>&1
  echo "$n: exit $? in $(( SECONDS - t0 )) s" >> "$OUT/summary.txt"
}
run core cargo test --locked -j "$JOBS" -p skate-core --lib
run game cargo test --locked -j "$JOBS" -p skate-game --bin skate3rust
run mods cargo test --locked -j "$JOBS" -p skate-mods --lib
if [ -n "${SKIP_IGNORED:-}" ]; then
  echo "ignored: not run (SKIP_IGNORED)" >> "$OUT/summary.txt"
elif [ -z "${INSTALL:-}" ] || [ ! -d "$INSTALL/maps" ]; then
  echo "ignored: not run (no installation; run setup or set INSTALL)" >> "$OUT/summary.txt"
else
  I="$INSTALL"
  SKATE3_ASSET_ROOT="${SKATE3_ASSET_ROOT:-$I/assets}" SKATE3_MAP="${SKATE3_MAP:-$I/maps/DownTown.skate}" \
  SKATE_BUDGET_MAP="${SKATE_BUDGET_MAP:-$I/maps/DownTown.skate}" SKATE_MAP_TEST_PATH="${SKATE_MAP_TEST_PATH:-$I/maps/DownTown.skate}" \
  SKATE3_GRIND_MAP="${SKATE3_GRIND_MAP:-$I/maps/University.skate}" SKATE3_WATER_MAP="${SKATE3_WATER_MAP:-$I/maps/Industrial.skate}" \
  SKATE_TRANSITION_TEST_ASSETS="${SKATE_TRANSITION_TEST_ASSETS:-$I/assets}" \
    run ignored cargo test --locked -j "$JOBS" -p skate-game --bin skate3rust -- --ignored
fi
run tools $PY -m unittest discover -s tools -p "test_*.py"
mods="$(ls tools/asset_pipeline/test_*.py | sed 's#/#.#g; s#\.py$##')"
run pipeline $PY -m unittest $mods

{
  echo
  for n in core game mods ignored; do
    [ -f "$OUT/$n.log" ] && echo "$n: $(grep '^test result' "$OUT/$n.log" | tail -1)"
  done
  for n in tools pipeline; do
    echo "$n: $(grep '^Ran ' "$OUT/$n.log" | tail -1), $(grep -E '^(OK|FAILED)' "$OUT/$n.log" | tail -1)"
  done
  echo "skipped (private inputs missing): $(cat "$OUT/skipped.txt" 2> /dev/null | wc -l)"
  echo "failed tests:"
  grep -h -E '^test .* FAILED$' "$OUT"/*.log | sed 's/^test /  /; s/ \.\.\. FAILED$//'
  grep -h -E '^(FAIL|ERROR):' "$OUT"/tools.log "$OUT"/pipeline.log | sed 's/^/  /'
} >> "$OUT/summary.txt"
cat "$OUT/summary.txt"
echo "logs: $OUT"
