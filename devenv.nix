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

  # dev-dynamic builds load libstd via @rpath with no rpath set. Exposing the
  # toolchain's target libdir lets tools/setup.py run target/debug/skate3rust
  # directly (cargo run sets this itself).
  enterShell =
    let
      var = if pkgs.stdenv.isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
    in
    ''
      export ${var}="$(rustc --print target-libdir):${"$" + var}"
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
