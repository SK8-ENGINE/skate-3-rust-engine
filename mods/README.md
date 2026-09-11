# Mods

Drop API-2 packages here as **folders** or `.zip` files. Each package needs `mod.json`
with `"api": 2` and a Lua entry that returns a callback table.

This is the physics/graphics interaction layer — not a vehicle SDK. See
[`docs/physics-graphics-sdk.md`](../docs/physics-graphics-sdk.md) and
[`sdk/examples/physics-sandbox/`](../sdk/examples/physics-sandbox/).

Override the scan path with `SKATE3_MODS`. Settings persist under `settings/mods/`
(or `SKATE3_MOD_SETTINGS`).
