# Mods

PLAY-MARIO-KART.bat loads this project-root folder directly through SKATE3_MODS.
Use this folder to add or edit mods; you do not need to find the executable in .local.
The in-game Open mods folder button opens the same location.

For a standalone game distribution, the default layout is beside the executable:

```
skate3rust.exe
mods/
  native-trainer/
    mod.json
    main.lua
```

Drop each community mod's folder here, then open Escape -> Mods. New mods start
disabled. Use Open mods folder to return here from the game. Added/changed files
are discovered automatically, or choose Rescan packages. Keep personal settings
out of this folder. SKATE3_MODS is an optional explicit directory override.

Native Trainer Showcase is original Lua source, with stock tuning defaults.
Read its README for controls and examples of supported SDK capabilities.
Do not distribute private Skate assets with mods.

Current loader: unpacked mod folders containing mod.json and main.lua, plus assets.
ZIP files are not loaded directly yet. For sharing, ZIP one package with mod.json
at the archive root, then extract it into mods/<mod-name>/ before use.
A future ZIP installer can validate/extract packages while keeping folder support
for mod development. See docs/vehicle-sdk.md for asset types and package limits.
