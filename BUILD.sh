#!/bin/sh
# Linux build: the game, the Steam relay helper and the ISO extractor.
# Windows DLL staging in scripts/Build.ps1 is Windows-only and intentionally
# not mirrored here.
#
# Steam-less build (e.g. musl, where the steamworks SDK has no prebuilt):
#   cargo build --release --no-default-features -p skate-game
# Direct UDP multiplayer still works; Steam lobby browsing is disabled.
#
# musl note: rustup's musl target defaults to static-pie, but Alpine does not
# ship static wayland/alsa libraries. Detect musl and link dynamically instead.
set -e
cd "$(dirname "$0")"
if [ "$(ldd --version 2>&1 | grep -ci musl)" -gt 0 ]; then
    export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-feature=-crt-static"
fi
cargo build --release -p skate-game -p skate-steam-relay -p skate-xiso
