#!/bin/sh
# Launch the game from the cargo target directory. Pass a map path to override.
set -e
cd "$(dirname "$0")"
cargo build -p skate-game --bin skate3rust
exec ./target/debug/skate3rust "$@"
