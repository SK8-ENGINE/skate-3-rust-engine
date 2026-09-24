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
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-src" ];
  };

  packages =
    [
      pkgs.pkg-config
      # Asset pipeline (tools/setup.py, tools/asset_pipeline); see tools/requirements-setup.txt.
      (pkgs.python313.withPackages (ps: [ ps.numpy ps.pillow ps.tkinter ]))
      # ISO extraction; the pipeline downloads a Win64 build of this on Windows.
      pkgs.extract-xiso
    ]
    ++ lib.optionals pkgs.stdenv.isLinux linuxRuntimeLibs;

  env = lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath linuxRuntimeLibs;
  };
}
