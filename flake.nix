{
  description = "Skate 3 Rust Engine development shell for macOS and Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      # nixpkgs-unstable no longer supports x86_64-darwin (Intel Macs).
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
          # Linux libraries the game links (ALSA, udev) or dlopens at runtime
          # (winit, wgpu and SDL: Vulkan, Wayland/X11, xkbcommon).
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
          # Runtime-loaded libraries, then dyld's default fallback on macOS.
          runtimeLibraryPath =
            if isDarwin then
              "${lib.makeLibraryPath [ pkgs.vulkan-loader ]}:/usr/local/lib:/usr/lib"
            else
              lib.makeLibraryPath linuxRuntimeLibs;
          libraryPathVariable = if isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
          # Impure on macOS: no Nix C toolchain or SDK, so cc, xcrun, the SDK and
          # any installed Xcode tools (Instruments, Metal tools) come from the system.
          mkShell = if isDarwin then pkgs.mkShellNoCC else pkgs.mkShell;
        in
        {
          default = mkShell {
            packages = [
              pkgs.rustc
              pkgs.cargo
              pkgs.clippy
              pkgs.rustfmt
              pkgs.rust-analyzer
              pkgs.pkg-config
              # sdl3-sys builds SDL from source.
              pkgs.cmake
              # Asset setup (tools/setup.py); see tools/requirements-setup.txt.
              (pkgs.python313.withPackages (ps: [
                ps.numpy
                ps.pillow
                ps.tkinter
              ]))
              # ISO extraction and audio decoding; on Windows setup downloads
              # pinned Win64 builds of these instead.
              pkgs.extract-xiso
              pkgs.vgmstream
            ];

            buildInputs = lib.optionals isLinux (
              linuxRuntimeLibs
              # SDL's CMake requires these X11 extension headers once it finds X11.
              ++ (with pkgs; [
                libxext
                libxfixes
                libxscrnsaver
                libxtst
              ])
            );

            env = {
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            }
            // lib.optionalAttrs isDarwin {
              # The renderer is Vulkan-only; on macOS wgpu reaches Metal through
              # the Vulkan loader and the MoltenVK driver, both dlopened at runtime.
              VK_DRIVER_FILES = "${pkgs.moltenvk}/share/vulkan/icd.d/MoltenVK_icd.json";
              # MoltenVK's default fast-math lets Metal compile the depth-prepass and
              # main-pass vertex shaders to slightly different positions; skinned,
              # morphed customiser skaters then fail the main-pass depth test.
              MVK_CONFIG_FAST_MATH_ENABLED = "0";
            };

            # dev-dynamic builds link libstd and Bevy's dylib without an rpath.
            # `cargo run` exports their directories; the shell does too, so
            # tools/setup.py can launch target/debug/skate3rust directly. Any
            # existing search path is kept after these.
            shellHook = ''
              root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
              export ${libraryPathVariable}="$(rustc --print target-libdir):$root/target/debug/deps:${runtimeLibraryPath}''${${libraryPathVariable}:+:''$${libraryPathVariable}}"
              unset root
            '';
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
