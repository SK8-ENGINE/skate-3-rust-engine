#!/usr/bin/env bash
# Prepares the game assets in data/ from your Skate 3 Xbox 360 ISO, or from
# default.xex in an extracted game folder, without the setup window. Run again
# after updates: only changed asset groups are prepared again.
set -euo pipefail
if [ $# -ne 1 ]; then
    echo "Usage: ./SETUP.sh /path/to/skate3.iso | /path/to/default.xex" >&2
    exit 1
fi
source=$(realpath "$1")
cd "$(dirname "$0")"
[ -x bin/skate3rust ] || { echo "Game is not built. Run ./BUILD.sh first." >&2; exit 1; }
python3 -c 'import numpy, PIL' 2>/dev/null || {
    echo "Setup needs numpy and Pillow (pacman -S python-numpy python-pillow)" >&2; exit 1; }
exec python3 tools/setup.py --base "$PWD/data" --game-exe "$PWD/bin/skate3rust" --source "$source"
