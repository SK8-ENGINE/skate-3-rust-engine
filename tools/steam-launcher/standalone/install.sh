#!/bin/sh
# Standalone Skate launcher, Linux and macOS (`sh install.sh`, or double-click INSTALL.command on macOS): downloads the
# Skate launcher for this computer from the fork's rolling release, checks it against the release's SHA-256, runs the
# first-time setup (setup.sh: what this computer needs, prebuilt versions only) and starts the launcher. Run it again
# to update or repair. Doc: README.md "Launcher-only download (standalone)".
set -e
here=$(cd "$(dirname "$0")" && pwd)
base=https://github.com/Hailey-Ross/rusty-trucks/releases/download/hails-additions
case "$(uname -s)/$(uname -m)" in
    Linux/x86_64) asset=skate-launcher-linux-x64 ;;
    Darwin/arm64) asset=skate-launcher-macos-arm64 ;;
    *) echo "There is no Skate launcher build for $(uname -s) $(uname -m) yet."; exit 1 ;;
esac
part="$here/$asset.part"
fail() { rm -f "$part"; echo "$1"; exit 1; }
echo "Downloading the Skate launcher ($asset)..."
curl -fsSL --max-time 900 -o "$part" "$base/$asset" || fail "The download failed ($base/$asset). Check the internet connection and run install.sh again."
want=$(curl -fsSL --max-time 60 "$base/$asset.sha256" | cut -d' ' -f1) || fail "The checksum could not be downloaded. Run install.sh again."
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$part" | cut -d' ' -f1); else got=$(shasum -a 256 "$part" | cut -d' ' -f1); fi
[ -n "$want" ] && [ "$want" = "$got" ] || fail "The download is damaged (its checksum does not match). Run install.sh again."
mv -f "$part" "$here/skate-launcher"
chmod +x "$here/skate-launcher"
# macOS marks downloads as quarantined; the launcher was checked above.
command -v xattr >/dev/null 2>&1 && xattr -d com.apple.quarantine "$here/skate-launcher" 2>/dev/null || true
printf 'rust\n' > "$here/launcher-flavour.txt"
echo "Checking what this computer needs..."
if ! SKATE_LAUNCHER_RELEASES=1 sh "$here/setup.sh" >/dev/null; then
    echo "Installing what is missing (it may ask for your password once)..."
    SKATE_LAUNCHER_RELEASES=1 sh "$here/setup.sh" --install
fi
echo "Starting the Skate launcher."
exec "$here/skate-launcher"
