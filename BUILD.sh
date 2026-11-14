#!/usr/bin/env bash
# Linux counterpart of BUILD.bat / scripts/Build.ps1: builds the game, stages it
# in bin/ with the Rust libraries it loads, and builds the native RefPack
# library setup uses. --dev builds the debug profile (still optimised).
set -euo pipefail
cd "$(dirname "$0")"
profile=release
args=(build --locked -p skate-game --bin skate3rust)
if [ "${1:-}" = "--dev" ]; then profile=debug; else args+=(--release); fi
command -v patchelf >/dev/null || { echo "patchelf is required to stage the game (pacman -S patchelf)" >&2; exit 1; }
start=$SECONDS
cargo "${args[@]}"

# Native RefPack for setup (tools/asset_pipeline/fast_refpack.py); without it
# setup decodes RefPack in pure Python, ~10x slower.
source=tools/asset_pipeline/refpack_native.rs
refpack=target/native/librefpack.so
if [ ! -f "$refpack" ] || [ "$source" -nt "$refpack" ]; then
    mkdir -p target/native
    rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort "$source" -o "$refpack"
fi

# Bevy's dynamic_linking (the default dev-dynamic feature) leaves the game
# needing libbevy_dylib and Rust's libstd. Copy every such library into bin/
# and point the loader at $ORIGIN, as Build.ps1 stages DLLs beside the exe.
out=target/$profile
libdir=$(rustc --print target-libdir)
mkdir -p bin
queue=("$out/skate3rust")
declare -A seen=()
while [ ${#queue[@]} -gt 0 ]; do
    file=${queue[0]}; queue=("${queue[@]:1}")
    name=$(basename "$file")
    [ -n "${seen[$name]:-}" ] && continue
    seen[$name]=1
    cp -f "$file" "bin/$name"
    patchelf --set-rpath '$ORIGIN' "bin/$name"
    for needed in $(patchelf --print-needed "$file"); do
        for directory in "$out" "$out/deps" "$libdir"; do
            if [ -f "$directory/$needed" ]; then queue+=("$directory/$needed"); break; fi
        done
    done
done
echo "Ready: bin/skate3rust [$profile] ($((SECONDS - start))s)"
