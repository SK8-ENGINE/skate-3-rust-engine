#!/usr/bin/env bash
# macOS release packaging. Mirrors the Windows release manifest layout for an
# Apple Silicon target. Produces:
#   target/skate3rust-macos-arm64.zip (+ .sha256) and target/release.json
#
# Env: RELEASE_TAG (default development), GITHUB_RUN_NUMBER (default 0).
# Pass --dev to stage the dev-profile binary instead of rebuilding --release
# (used by CI for speed; artifacts from --dev are not for distribution).
# The PyInstaller setup bundle (skate3setup) has no macOS build yet, so this
# package serves developers and testers via the --assets flow (docs/macos.md);
# first-launch setup windows arrive with the bundle.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

TAG="${RELEASE_TAG:-development}"
BUILD="${GITHUB_RUN_NUMBER:-0}"
STAMP="$(date +%Y%m%d-%H%M%S)"
# Fresh stage dir per run; prune previous ones so repeated runs don't leak disk.
rm -rf "$ROOT/target/release-packages"
STAGE="$ROOT/target/release-packages/$STAMP/skate3rust-macos-arm64"
mkdir -p "$STAGE/support" "$STAGE/mods" "$STAGE/docs/images" "$STAGE/licenses"

echo "==> release binary (static, no Bevy dynamic linking)"
if [[ "${1:-}" == "--dev" ]]; then
  cargo build --locked --target-dir target -p skate-game --bin skate3rust --no-default-features
  cp -f target/debug/skate3rust "$STAGE/skate3rust"
else
  cargo build --release --locked --target-dir target -p skate-game --bin skate3rust --no-default-features
  cp -f target/release/skate3rust "$STAGE/skate3rust"
fi
chmod +x "$STAGE/skate3rust"

echo "==> native RefPack decoder"
mkdir -p target/native
rustc --edition 2024 --crate-type cdylib -C opt-level=3 \
  tools/asset_pipeline/refpack_native.rs -o target/native/librefpack.dylib
cp -f target/native/librefpack.dylib "$STAGE/support/librefpack.dylib"

echo "==> character importer runtime"
# Tolerate Rosetta-less environments (e.g. minimal CI images without a
# passwordless sudo): the package still ships, minus bundled FBX import.
# Conversions can always pass --fbx-tool explicitly (see docs/macos.md).
if ! ./scripts/prepare-character-importer-macos.sh --dest "$STAGE/support"; then
  echo "WARNING: importer runtime skipped; character FBX import requires --fbx-tool." >&2
fi

echo "==> mods, docs, licenses"
cp -f mods/native-trainer.zip mods/mario-kart.zip mods/README.md "$STAGE/mods/"
cp -f README.md docs/THIRD_PARTY_NOTICES.md "$STAGE/"
cp -f docs/images/skating-crab.png "$STAGE/docs/images/skating-crab.png"
for doc in installation.md retail-renderer.md crash-reports.md performance-tracing.md updates.md custom-models.md mixamo-to-skate.md character-customisation.md macos.md; do
  cp -f "docs/$doc" "$STAGE/docs/$doc" 2>/dev/null || echo "WARNING: optional doc missing: docs/$doc" >&2
done
cp -f tools/mixamo_to_skate/licenses/FBX2glTF.txt "$STAGE/licenses/FBX2glTF.txt"
cp -f tools/vendor/utt/LICENSE "$STAGE/licenses/UTT.txt"
cp -f tools/vendor/university/LICENSE-PROJECT.md "$STAGE/licenses/CustomEngineLayer.txt"
cp -f vendor/bevy_pbr/LICENSE-MIT "$STAGE/licenses/Bevy-MIT.txt"
cp -f vendor/bevy_pbr/LICENSE-APACHE "$STAGE/licenses/Bevy-APACHE.txt"

echo "==> release manifest"
REVISION="$(git rev-parse HEAD)"
EXE_SHA="$(shasum -a 256 "$STAGE/skate3rust" | awk '{print $1}')"
ASSET_PIPELINES="$(python3 tools/asset_pipeline/versions.py --tools tools)"
CHARACTER_CUSTOMISER="$(python3 -m tools.asset_pipeline.customiser_setup --fingerprint | tr -d '[:space:]')"
python3 - "$STAGE" "$REVISION" "$BUILD" "$TAG" "$EXE_SHA" "$ASSET_PIPELINES" "$CHARACTER_CUSTOMISER" <<'EOF'
import hashlib, json, sys
from pathlib import Path
stage = Path(sys.argv[1])
revision, build, tag, exe_sha, pipelines, customiser = sys.argv[2:8]
files = {}
for path in sorted(stage.rglob('*')):
    if not path.is_file():
        continue
    name = path.relative_to(stage).as_posix()
    if name == 'release.json' or name.startswith('mods/'):
        continue
    files[name] = hashlib.sha256(path.read_bytes()).hexdigest()
manifest = {
    'schema': 1,
    'repository': 'SK8-ENGINE/skate-3-rust-engine',
    'target': 'macos-arm64',
    'build': int(build),
    'tag': tag,
    'revision': revision,
    'files': files,
    'asset_pipelines': json.loads(pipelines),
    'character_customiser': customiser,
}
Path(stage, 'release.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
EOF
cp -f "$STAGE/release.json" target/release.json

echo "==> archive"
ZIP="$ROOT/target/skate3rust-macos-arm64.zip"
rm -f "$ZIP" "$ZIP.sha256"
# --norsrc keeps AppleDouble (._) resource-fork files out of the archive.
ditto -c -k --keepParent --norsrc "$STAGE" "$ZIP"
shasum -a 256 "$ZIP" | awk '{print $1 "  skate3rust-macos-arm64.zip"}' > "$ZIP.sha256"
echo "Release package: $ZIP"
cat "$ZIP.sha256"
