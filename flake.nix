{
  description = "Skate 3 Rust Engine development shell for macOS and Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (
        pkgs:
        let
          inherit (pkgs) lib;
          inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux;
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
          runtimeLibraryPath =
            if isDarwin then
              "${lib.makeLibraryPath [ pkgs.vulkan-loader ]}:/usr/local/lib:/usr/lib"
            else
              lib.makeLibraryPath linuxRuntimeLibs;
          libraryPathVariable = if isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
          commands = {
            skate-build = ''
              cd "$SKATE_ROOT"
              exec cargo build --locked -p skate-game --bin skate3rust "$@"
            '';
            skate-assets = ''
              cd "$SKATE_ROOT"
              python - <<'PY'
              import json
              import os
              from pathlib import Path

              assets = Path(os.environ.get("SKATE3_ASSETS", "assets"))
              if "SKATE3_ASSETS" not in os.environ and not (assets / "private/game.json").is_file():
                  marker = Path("data/installation.json")
                  if not marker.is_file():
                      raise SystemExit("No installation; run skate-setup <iso|default.xex> first")
                  assets = Path("data") / json.loads(marker.read_text())["directory"] / "assets"
              if not (assets / "private/game.json").is_file():
                  raise SystemExit(f"Prepared assets not found at: {assets}")
              print(assets.resolve())
              PY
            '';
            skate-run = ''
              cd "$SKATE_ROOT"
              assets="$(skate-assets)"
              exec cargo run --locked -p skate-game --bin skate3rust -- --assets "$assets" "$@"
            '';
            skate-setup = ''
              if [ "$#" -ne 1 ]; then
                echo "usage: skate-setup <Skate 3 Xbox 360 ISO | default.xex>" >&2
                exit 64
              fi
              source="$(realpath "$1")"
              cd "$SKATE_ROOT"
              skate-build
              exec python tools/setup.py --base data --game-exe target/debug/skate3rust \
                --source "$source" --refresh
            '';
          };
          commandPackages = lib.mapAttrsToList (
            name: script:
            pkgs.writeShellScriptBin name ''
              set -euo pipefail
              : "''${SKATE_ROOT:?Run this command inside nix develop}"
              ${script}
            ''
          ) commands;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
              pkgs.rust-analyzer
              pkgs.pkg-config
              pkgs.cmake
              pkgs.git
              # Keep in sync with tools/requirements-setup.txt.
              (pkgs.python313.withPackages (ps: [
                ps.numpy
                ps.pillow
                ps.tkinter
              ]))
              pkgs.extract-xiso
              pkgs.vgmstream
            ]
            ++ commandPackages;

            buildInputs =
              lib.optionals isDarwin [ pkgs.apple-sdk ]
              ++ lib.optionals isLinux (
                linuxRuntimeLibs
                # SDL requires these headers when X11 is enabled.
                ++ (with pkgs; [
                  libxext
                  libxfixes
                  libxscrnsaver
                  libxtst
                ])
              );

            env = {
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
              "CARGO_TARGET_${
                lib.toUpper (builtins.replaceStrings [ "-" ] [ "_" ] pkgs.stdenv.hostPlatform.rust.rustcTarget)
              }_LINKER" =
                "${pkgs.stdenv.cc}/bin/cc";
            }
            // lib.optionalAttrs isDarwin {
              VK_DRIVER_FILES = "${pkgs.moltenvk}/share/vulkan/icd.d/MoltenVK_icd.json";
              # Fast-math makes morphed depth/main-pass vertex positions disagree.
              MVK_CONFIG_FAST_MATH_ENABLED = "0";
            };

            # Direct launches (including setup) need dev-dynamic's Rust and Bevy libraries.
            shellHook = ''
              export SKATE_ROOT="$(git rev-parse --show-toplevel)"
              export ${libraryPathVariable}="$(rustc --print target-libdir):$SKATE_ROOT/target/debug/deps:${runtimeLibraryPath}''${${libraryPathVariable}:+:''$${libraryPathVariable}}"
              if [[ $- == *i* ]]; then
                echo "Skate dev shell: skate-build | skate-setup <iso|default.xex> | skate-run [args] | skate-assets"
              fi
            '';
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
