#!/bin/sh
# Check the Trick Guide demo clips: every trickguide_* name in the collections has a clip file, every clip decodes,
# and every clip starts and ends upright (bails that end on the ground are listed, not failed).
# Usage (from the repo checkout to test): check_trick_clips.sh [clip dir]
#   default clip dir: the checkout's installed clips, assets/private/hud/scene/trickguide
#   builds with CARGO_TARGET_DIR when it is set (keep build output off the repo drive if you like)
REPO="$(git rev-parse --show-toplevel)" || exit 1
cd "$REPO" || exit 1
DIR="${1:-assets/private/hud/scene/trickguide}"

echo "== names without a clip file"
grep -o '"trickguide_[a-z0-9_]*"' assets/private/stock/skater-collections.json | tr -d '"' | sort -u > /tmp/tg_names.$$
ls "$DIR" | sed 's/1\.abin$//' | sort -u > /tmp/tg_files.$$
echo "$(wc -l < /tmp/tg_names.$$) names, $(wc -l < /tmp/tg_files.$$) clip files"
comm -23 /tmp/tg_names.$$ /tmp/tg_files.$$
rm -f /tmp/tg_names.$$ /tmp/tg_files.$$

echo "== decode + posture (ignored tests in skate-data/tests/scene_clip.rs)"
SKATE3_SCENE_CLIPS="$DIR" \
  cargo test --release --locked -j 4 -p skate-data --test scene_clip -- --ignored --nocapture 2>&1 \
  | grep -E "frame [0-9]+:|not upright|^test |test result|panicked"
