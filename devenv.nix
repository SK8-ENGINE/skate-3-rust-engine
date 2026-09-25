{
  pkgs,
  lib,
  ...
}:

let
  # Libraries Bevy/winit/wgpu dlopen at runtime on Linux.
  linuxRuntimeLibs = with pkgs; [
    alsa-lib
    udev
    vulkan-loader
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
{
  # Toolchain: edition 2024 + Bevy 0.18 need a current stable rustc.
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      "rust-src"
    ];
  };

  packages = [
    pkgs.pkg-config
    # Asset pipeline (tools/setup.py, tools/asset_pipeline); see tools/requirements-setup.txt.
    (pkgs.python313.withPackages (ps: [
      ps.numpy
      ps.pillow
      ps.tkinter
    ]))
    # ISO extraction; the pipeline downloads a Win64 build of this on Windows.
    pkgs.extract-xiso
  ]
  ++ lib.optionals pkgs.stdenv.isLinux linuxRuntimeLibs
  ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.vulkan-loader
    pkgs.moltenvk
  ];

  env =
    lib.optionalAttrs pkgs.stdenv.isLinux {
      LD_LIBRARY_PATH = lib.makeLibraryPath linuxRuntimeLibs;
    }
    # The renderer is Vulkan-only (app.rs); on macOS wgpu reaches Metal through
    # the Vulkan loader + MoltenVK driver, which wgpu dlopens at runtime.
    // lib.optionalAttrs pkgs.stdenv.isDarwin {
      DYLD_FALLBACK_LIBRARY_PATH = "${
        lib.makeLibraryPath [ pkgs.vulkan-loader ]
      }:/usr/local/lib:/usr/lib";
      VK_DRIVER_FILES = "${pkgs.moltenvk}/share/vulkan/icd.d/MoltenVK_icd.json";
    };

  # dev-dynamic builds load libstd via @rpath with no rpath set, and libbevy_dylib
  # via the absolute path it was linked at (stale once the checkout moves). Exposing
  # the toolchain's target libdir and target/debug/deps lets tools/setup.py run
  # target/debug/skate3rust directly (cargo run sets these itself).
  enterShell =
    let
      var = if pkgs.stdenv.isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
    in
    ''
      export ${var}="$(rustc --print target-libdir):$DEVENV_ROOT/target/debug/deps:${"$" + var}"
    '';

  # `skate-setup <game.iso|default.xex>`: preflight, build, then headless asset setup.
  scripts.skate-setup.exec = ''
    set -euo pipefail
    if [ $# -ne 1 ]; then echo "usage: skate-setup <Skate 3 Xbox 360 ISO | default.xex>" >&2; exit 64; fi
    source="$(realpath "$1")"
    cd "$DEVENV_ROOT"
    echo "==> Preflight: $source"
    case "$source" in
      *.iso|*.ISO)
        # Lists only the directory tables, so rejecting a PS3/non-XDVDFS image is fast.
        listing="$(extract-xiso -l "$source" 2>&1)" || listing=""
        grep -qi 'default\.xex' <<<"$listing" \
          || { echo "Not an Xbox 360 Skate 3 ISO (no XDVDFS default.xex)" >&2; exit 1; } ;;
      */default.xex) ;;
      *) echo "Expected an .iso or default.xex" >&2; exit 1 ;;
    esac
    echo "==> Building workspace"
    cargo build --workspace --locked
    refresh=()
    if [ -f data/installation.json ]; then refresh=(--refresh); fi
    echo "==> Asset setup"
    python tools/setup.py --base data --game-exe target/debug/skate3rust --source "$source" "''${refresh[@]}"
    echo "==> Installed: $(skate-assets)"
  '';

  # Prints the published asset directory from data/installation.json.
  scripts.skate-assets.exec = ''
    set -euo pipefail
    cd "$DEVENV_ROOT"
    [ -f data/installation.json ] || { echo "No installation; run skate-setup <iso> first" >&2; exit 1; }
    python -c 'import json;print("data/"+json.load(open("data/installation.json"))["directory"]+"/assets")'
  '';

  # `skate-run [game args...]`, e.g. `skate-run --map path/to/map.skate`.
  scripts.skate-run.exec = ''
    set -euo pipefail
    cd "$DEVENV_ROOT"
    assets="$(skate-assets)"
    exec cargo run --bin skate3rust -- --assets "$assets" "$@"
  '';

  # Pre-commit: installed into .git/hooks when the shell starts. rustfmt is not
  # enforced yet: most existing sources predate rustfmt and would be rewritten.
  git-hooks.hooks = {
    cargo-check = {
      enable = true;
      args = [
        "--workspace"
        "--locked"
      ];
      files = "\\.rs$|Cargo\\.(toml|lock)$";
    };
    nixfmt-rfc-style.enable = true;
  };
}
