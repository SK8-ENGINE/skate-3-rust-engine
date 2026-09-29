#!/usr/bin/env bash
# macOS (Apple Silicon) development build for Skate 3 Rust Engine.
# Usage: ./scripts/build-macos.sh [--release]
#
# The staged binary is built WITHOUT Bevy dynamic linking so bin/skate3rust
# runs standalone (the default dynamic build only runs via cargo because its
# Bevy dylib lives under target/debug/deps). First build takes a while
# (static Bevy); later builds are incremental.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="debug"
if [[ "${1:-}" == "--release" ]]; then MODE="release"; fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "Rust is missing. Run ./scripts/download-macos-deps.sh first." >&2
  exit 1
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "Xcode Command Line Tools missing. Run: xcode-select --install" >&2
  exit 1
fi
export PATH="$HOME/.cargo/bin:$PATH"

# Refresh Cargo.lock for the non-Windows gilrs edge (first run only).
if ! cargo metadata --format-version 1 --locked >/dev/null 2>&1; then
  echo "Refreshing Cargo.lock for macOS dependencies..."
  cargo metadata --format-version 1 >/dev/null
fi

if [[ "$MODE" == "release" ]]; then
  cargo build --release --locked -p skate-game --bin skate3rust --no-default-features
  EXE="target/release/skate3rust"
else
  cargo build --locked -p skate-game --bin skate3rust --no-default-features
  EXE="target/debug/skate3rust"
fi

# Native RefPack decoder (optional accelerator; Python fallback otherwise).
mkdir -p target/native
LIB="target/native/librefpack.dylib"
rustc --edition 2024 --crate-type cdylib -C opt-level=3 \
  tools/asset_pipeline/refpack_native.rs -o "$LIB" || {
  echo "WARNING: native RefPack build failed; Python fallback will be used." >&2
}

mkdir -p bin
cp -f "$EXE" bin/skate3rust
echo "Ready: bin/skate3rust"
echo "Run: ./scripts/launch-macos.sh --assets <ASSETS_DIR> [--map <MAP.skate>]"
echo "Tip: prepare assets with: python3 tools/prepare_assets.py --game-root <EXTRACTED_XBOX_FOLDER> --output <ASSETS_DIR> --game-exe bin/skate3rust"
