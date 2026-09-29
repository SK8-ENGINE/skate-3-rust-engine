#!/usr/bin/env bash
# Fetch the macOS character-importer runtime (FBX2glTF). The game itself does
# not need it; only Custom Models / Mixamo FBX imports do.
#
# Usage: ./scripts/prepare-character-importer-macos.sh [--dest DIR]
# Default destination: target/importer-runtime/  (pass it to conversions
# with --fbx-tool target/importer-runtime/FBX2glTF)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST=""
if [[ "${1:-}" == "--dest" ]]; then DEST="${2:?missing DIR for --dest}"; elif [[ $# -gt 0 ]]; then DEST="$1"; fi
DEST="${DEST:-$ROOT/target/importer-runtime}"

URL="https://github.com/facebookincubator/FBX2glTF/releases/download/v0.9.7/FBX2glTF-darwin-x64"
SHA="f82383ae4185c39f991b479b04ecce104f02e70c12a035ed31fc469e6f74a3fd"

# The darwin asset is an x86_64 Mach-O binary; Apple Silicon runs it via Rosetta 2.
if [[ "$(uname -m)" == "arm64" ]] && ! /usr/bin/arch -x86_64 /usr/bin/true 2>/dev/null; then
  echo "Rosetta 2 is required for the Intel FBX2glTF binary. Installing..." >&2
  sudo softwareupdate --install-rosetta --agree-to-license
fi

CACHE="$ROOT/target/importer-deps"
mkdir -p "$CACHE" "$DEST"
ARCHIVE="$CACHE/FBX2glTF-darwin-x64"
if [[ ! -f "$ARCHIVE" ]]; then
  echo "Downloading FBX2glTF v0.9.7 (darwin)..." >&2
  curl -fSL -o "$ARCHIVE" "$URL"
fi
ACTUAL="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
if [[ "$ACTUAL" != "$SHA" ]]; then
  echo "FBX2glTF checksum mismatch; deleting $ARCHIVE so the next run retries." >&2
  rm -f "$ARCHIVE"
  exit 1
fi
cp -f "$ARCHIVE" "$DEST/FBX2glTF"
chmod +x "$DEST/FBX2glTF"
# Read the full output (no head -q early-close) so a broken binary fails loudly.
VERSION_LINE="$("$DEST/FBX2glTF" --version 2>&1)" || { echo "FBX2glTF smoke check failed." >&2; exit 1; }
echo "$VERSION_LINE" | head -n 2
echo "Importer ready: $DEST/FBX2glTF"
