# Broken Bones

Enable **Broken Bones** in Mods on the matching generalized-API build. A hard
limb impact releases that joint's angular limits and the limb's animation and
board-holding drives. The limb stays attached at its linear joint anchor.

Use **Skater mods > Broken bones > Heal all injuries** to restore this mod's
overrides. Turning off Enable injuries also heals. Map changes/unload restore
controls. Heal on teleport is enabled by default. Normal bail recovery alone
does not heal injuries.

Settings expose impulse threshold (default 80 N s), minimum closing speed
(3 m/s), cooldown, bail-only detection and HUD visibility. Thresholds are gameplay
settings, not medical fracture estimates. Resting support force alone cannot
cause an injury. Joint names and indices come from the loaded native rig.

All injury logic is in `main.lua`. The host only exposes body/contact observations,
joint and drive controls, menu actions, networking and command results. A failed
override is reported instead of falsely marking a joint injured. Owned overrides
are restored without clearing another mod's direct joint settings.

With the mod enabled on peers, the HUD displays remote injury counts; native
physical poses use the existing multiplayer pose stream. There are no detached
limb meshes or blood effects. Headless Lua/native integration tests cover the
rules and constraint controls; injury feel still needs gameplay evaluation.
