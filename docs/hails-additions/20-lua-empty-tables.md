# 20. Empty Lua tables in mod command list fields

## Problem

A mod that passes an empty table to a list field is refused, for example `sdk.ui.menu('u', {title='T', items={{id='a', label='A', children={}}}})`, a raycast with `{exclude={}}`, `sdk.player.detach{candidates={}}`, or a raw `graphics_mesh` request with `deform_nodes={}`. The command fails with `invalid type: map, expected a sequence`, and the mod loses the whole call.

## Root cause

An empty Lua table has no array part, so mlua hands it to serde as an empty **map**. Most commands are an internally tagged enum (`vm::Command`), so serde buffers the whole command first, and a plain `Vec<T>` field then rejects the map. Non-empty list tables have an array part and read fine, which is why this only shows up with `{}`.

## Evidence

Found in a muted in-game log check of a mod that sent empty lists: the commands were refused with the serde error above. The unit test below reproduces it for every list field before the change.

## Change

- New `crates/skate-mods/src/lua_list.rs`: `list` and `opt_list` serde helpers. A sequence reads as before. A table whose keys are exactly `1..n` (the empty table included) reads as the list in key order. Any other table (a gap, key 0, a named key, a fractional key) is still an error ("expected a list …"). JSON input keeps working, and fields that are meant to be maps aren't touched.
- Every list field a script can fill uses them:
  - `vm.rs`: `GraphicsMesh.deform_nodes`;
  - `graphics_dynamic.rs`: mesh buffer `positions`, `indices`, `normals`, `colors`, `uvs`;
  - `presentation.rs`: canvas `items`;
  - `extensions.rs`: menu `items`, `children`;
  - `scene.rs`: detach `candidates`;
  - `query.rs`: raycast `exclude`.
- `nil` keeps its old meaning: a missing list is empty and a missing optional list is `None`. `{}` in an optional list is `Some(empty)`, so validation still refuses it where entries are required (for example `uvs={}` on a 3-vertex write).

## Files

`crates/skate-mods/src/lua_list.rs` (new), `lib.rs`, `vm.rs`, `graphics_dynamic.rs`, `presentation.rs`, `extensions.rs`, `scene.rs`, `query.rs`.

## Verification

- `lua_list` unit tests: Lua tables (empty, ordered, out-of-order integer keys, gaps / zero / named / fractional / negative keys refused) and JSON (arrays, `{}`, `{"1":…}`, leading-zero keys refused).
- `vm::empty_table_lists::every_list_field_takes_an_empty_table_and_a_list`: every list field through the real `_submit` path (`Vm` + `api.lua`), with `{}` and a list. Fields that need entries read `{}` and are then refused by validation, never by serde. A table with named keys is still refused.
- `vm::empty_table_lists::map_fields_keep_empty_tables_as_maps`: map / free-value fields still read `{}` as an object.
- `graphics_mesh_buffer_tests`: the `uvs = {}` case now fails validation instead of deserialization (it is refused either way).
- `cargo test --locked -p skate-mods`: all pass except `skyline_every_component_is_real_and_drives_through_ground_contact`, which needs the Skyline GLB (not in a checkout) and fails on `main` too. `cargo build --locked` is clean.

## Open questions

None. The audio moddability work (#36, folded into #32, merged) uses the same helper for its audio commands; the repository keeps one copy, `crates/skate-mods/src/lua_list.rs`.
