{
  description = "Skate 3 Rust Engine development shell for macOS and Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forSystems = systems: f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      forAllSystems = forSystems systems;
      # Loaded by the game at runtime (winit, wgpu, SDL, audio).
      linuxRuntimeLibs =
        pkgs: with pkgs; [
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
      # SDL requires these headers when X11 is enabled.
      linuxBuildLibs =
        pkgs:
        linuxRuntimeLibs pkgs
        ++ (with pkgs; [
          libxcb
          libxext
          libxfixes
          libxscrnsaver
          libxtst
        ]);
      # Prints the prepared asset directory; run from the checkout root.
      resolveAssets = pkgs: ''
        assets="''${SKATE3_ASSETS:-assets}"
        if [ -z "''${SKATE3_ASSETS:-}" ] && [ ! -f "$assets/private/game.json" ]; then
          if [ ! -f data/installation.json ]; then
            echo "No installation; run skate-setup <iso|default.xex> first" >&2
            exit 1
          fi
          assets="data/$(${lib.getExe pkgs.jq} -er .directory data/installation.json)/assets"
        fi
        if [ ! -f "$assets/private/game.json" ]; then
          echo "Prepared assets not found at: $assets" >&2
          exit 1
        fi
        realpath "$assets"
      '';
    in
    {
      # Release build that runs outside `nix develop`. Its runtime closure holds only
      # the game and its runtime libraries; `skate-install` roots it against GC.
      packages = forSystems [ "aarch64-linux" "x86_64-linux" ] (
        pkgs:
        let
          skate3rust = pkgs.rustPlatform.buildRustPackage {
            pname = "skate3rust";
            version = (lib.importTOML ./Cargo.toml).workspace.package.version;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./crates
                ./vendor
                ./maps/format-demo.skate
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            # Drops dev-dynamic: Bevy is linked statically.
            buildNoDefaultFeatures = true;
            cargoBuildFlags = [
              "-p"
              "skate-game"
              "--bin"
              "skate3rust"
            ];
            # Fixup strips debug info anyway.
            env.CARGO_PROFILE_RELEASE_DEBUG = "0";
            doCheck = false;
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.cmake
            ];
            # CMake only builds SDL from sdl3-sys's build script.
            dontUseCmakeConfigure = true;
            buildInputs = linuxBuildLibs pkgs;
            # winit, wgpu and SDL dlopen these; fixup would shrink unused rpath entries away.
            postFixup = ''
              patchelf --add-rpath ${lib.makeLibraryPath (linuxRuntimeLibs pkgs)} $out/bin/skate3rust
            '';
            meta.mainProgram = "skate3rust";
          };
        in
        {
          inherit skate3rust;
          default = pkgs.writeShellApplication {
            name = "skate-play";
            runtimeInputs = [ pkgs.coreutils ];
            text = ''
              assets="$(${resolveAssets pkgs})"
              # The store copy of the executable is read-only; keep mods beside settings.
              export SKATE3_MODS="''${SKATE3_MODS:-$(dirname "$assets")/mods}"
              mkdir -p "$SKATE3_MODS"
              exec ${lib.getExe skate3rust} --assets "$assets" "$@"
            '';
          };
        }
      );

      devShells = forAllSystems (
        pkgs:
        let
          inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux;
          runtimeLibraryPath =
            if isDarwin then
              "${lib.makeLibraryPath [ pkgs.vulkan-loader ]}:/usr/local/lib:/usr/lib"
            else
              lib.makeLibraryPath (linuxRuntimeLibs pkgs);
          libraryPathVariable = if isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
          commands = {
            skate-build = ''
              cd "$SKATE_ROOT"
              exec cargo build --locked -p skate-game --bin skate3rust "$@"
            '';
            skate-assets = ''
              cd "$SKATE_ROOT"
              ${resolveAssets pkgs}
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
          }
          // lib.optionalAttrs isLinux {
            # The out-link is a GC root; its name avoids nh clean's result-link expiry.
            # A git flake ref copies only tracked files, never data/.
            skate-install = ''
              cd "$SKATE_ROOT"
              exec nix build --print-build-logs "$SKATE_ROOT#default" --out-link skate-play "$@"
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
              lib.optionals isDarwin [ pkgs.apple-sdk ] ++ lib.optionals isLinux (linuxBuildLibs pkgs);

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
                echo "Skate dev shell: skate-build | skate-setup <iso|default.xex> | skate-run [args] | skate-assets${lib.optionalString isLinux " | skate-install"}"
              fi
            '';
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
