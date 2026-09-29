# Agent guide: physics/graphics mods (API 2)

Use this when asked to make a mod. Deliver a validated package under `mods/` (folder or `.zip`).
Do **not** invent vehicle/wheel/suspension host APIs — assemble those in Lua from primitives.

## Read first

1. `docs/physics-graphics-sdk.md` — layer philosophy and deferred work
2. `sdk/skate.lua` — annotations
3. `sdk/examples/physics-sandbox/` — reference mod
4. Host: `crates/skate-game/src/modding/`, runtime `crates/skate-mods/`, dynamics `crates/skate-dynamics/`

## Workflow

```powershell
cargo run --locked -p skate-mods --example check_mod -- sdk/examples/your-mod
# copy/symlink into mods/
```

Manifest must use `"api": 2`. Return a callback table from the entry Lua file.

## Building blocks

Physics bodies (box/sphere/capsule/convex/`mesh` from named GLB objects), extra
`add_collider` hulls, forces/joints(+motors)/sensors, graphics meshes/overlays,
player attach/detach, camera follow/set, `sdk.assets.objects`, `sdk.input.pad`,
settings/log/timers. No `sdk.vehicle`.
