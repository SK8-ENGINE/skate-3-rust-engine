# Shared settings for the dev-bench shell tools (sourced; Git Bash on Windows). See README.md.
#   REPO      the checkout (default: the one this file is in)
#   EXE       the game exe (default: $REPO/bin/skate3rust.exe, what scripts/Build.ps1 stages)
#   DEPS      folder with the exe's std / bevy DLLs when the exe is a saved copy elsewhere (default: the exe's folder)
#   RUN_DIR   working folder of the game process (default: $REPO)
DEV_BENCH="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
R="${REPO:-$(cd "$DEV_BENCH/../.." && pwd)}"
# The installation setup made (data/installation.json names its folder).
INST="$R/data/$(sed -n 's/.*"directory": *"\([^"]*\)".*/\1/p' "$R/data/installation.json" 2>/dev/null)"
[ -d "$INST/assets" ] || { echo "no installation under $R/data (run setup first, or set REPO)"; exit 3; }
# An empty mods folder, so runs load no mods.
EMPTY_MODS="$R/.local/dev-bench/empty-mods"
mkdir -p "$EMPTY_MODS"
# One game at a time; the Steam launcher writes PLAYING.txt while a player session runs.
bench_guard() {
  tasklist | grep -qi "skate3rust\|skate3recomp\|rexglue\|rpcs3" && { echo "a game is already running"; exit 3; }
  [ -e "$R/.local/steam-launcher/PLAYING.txt" ] && { echo "a player session is running (launcher)"; exit 3; }
  return 0
}
bench_path() {
  export PATH="${DEPS:-$(dirname "$1")}:$HOME/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin:$PATH"
}
