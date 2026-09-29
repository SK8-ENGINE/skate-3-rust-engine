#!/usr/bin/env bash
# Download and verify every third-party dependency the macOS build needs.
# The Skate 3 game dump itself is NEVER downloaded: provide your own disc
# source (an Xbox 360 ISO or an extracted folder with default.xex) when
# running asset conversion. See docs/macos.md.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

have() { command -v "$1" >/dev/null 2>&1; }

echo "==> 1/6 Xcode Command Line Tools"
if ! xcode-select -p >/dev/null 2>&1; then
  echo "Installing Xcode Command Line Tools (follow the prompt)..."
  xcode-select --install
else
  echo "present: $(xcode-select -p)"
fi

echo "==> 2/6 Rust toolchain"
if ! have rustc; then
  echo "Installing Rust (stable, minimal profile)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
else
  echo "present: $(rustc --version)"
fi
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> 3/6 Python 3.13 (asset conversion)"
if ! have brew; then
  echo "Installing Homebrew (required for Python 3.13)..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
fi
for brew_bin in /opt/homebrew/bin/brew /usr/local/bin/brew; do
  if [[ -x "$brew_bin" ]]; then eval "$("$brew_bin" shellenv)"; break; fi
done
if ! have python3.13; then
  echo "Installing python@3.13..."
  brew install python@3.13
else
  echo "present: $(python3.13 --version)"
fi
echo "Installing setup-tool requirements..."
python3.13 -m pip install --upgrade pip
python3.13 -m pip install -r tools/requirements-setup.txt

echo "==> 4/6 extract-xiso (macOS, pinned, hash-verified)"
XISO_URL="https://github.com/XboxDev/extract-xiso/releases/download/build-202505152050/extract-xiso_macOS.zip"
XISO_SHA="371e4a800086e875257ddafc037970789fb942b69dbf8ab0ba8301ff7799fef0"
mkdir -p target/tool-deps
XISO_ZIP="target/tool-deps/extract-xiso_macOS.zip"
if [[ ! -f "$XISO_ZIP" ]]; then curl -fSL -o "$XISO_ZIP" "$XISO_URL"; fi
if [[ "$(shasum -a 256 "$XISO_ZIP" | awk '{print $1}')" != "$XISO_SHA" ]]; then
  echo "extract-xiso checksum mismatch; deleting $XISO_ZIP so the next run retries." >&2
  rm -f "$XISO_ZIP"
  exit 1
fi
echo "extract-xiso archive verified (used automatically by setup when given an ISO)."

echo "==> 5/6 FBX2glTF character importer (macOS, pinned, hash-verified)"
./scripts/prepare-character-importer-macos.sh

echo "==> 6/6 native RefPack decoder"
mkdir -p target/native
rustc --edition 2024 --crate-type cdylib -C opt-level=3 \
  tools/asset_pipeline/refpack_native.rs -o target/native/librefpack.dylib
python3.13 -c "import ctypes; ctypes.CDLL('target/native/librefpack.dylib'); print('librefpack.dylib loads OK')"

echo
echo "All downloadable dependencies are ready."
echo "NOT included: the Skate 3 Xbox 360 disc dump (your own dump required)."
echo "Next: ./scripts/build-macos.sh && prepare assets per docs/macos.md"
