# Sky shader validation test

Branch: `gameplay/water`. Status: **done**; test-only change, no game behaviour changes.

## Problem

`retail_render::shader_tests::sky_shader_validates` failed on untouched upstream
(`60efdef`) and was listed as a known pre-existing failure:

```
error: invalid field accessor `position`
29 │     let world = v.position + vec3<f32>(view.world_position.x, ...
```

## Root cause

`retail_sky.wgsl` takes `bevy_pbr::forward_io::Vertex`, whose `position` field is
declared only under `#ifdef VERTEX_POSITIONS`
(`vendor/bevy_pbr/src/render/forward_io.wgsl`). Bevy's mesh pipeline pushes
`VERTEX_POSITIONS` for every mesh layout that has positions
(`vendor/bevy_pbr/src/render/mesh.rs`, `shader_defs.push("VERTEX_POSITIONS")`), so
the shader compiles in the game. The test's `validate` helper
(`crates/skate-game/src/retail_shader_tests.rs`) composes shaders with its own
def list, which included the UV/tangent/colour/normal defs but not
`VERTEX_POSITIONS`. The world shader never reads `Vertex.position`, so only the
sky test tripped over it.

## Change

Add `VERTEX_POSITIONS` to the shader defs in `validate`, matching what Bevy sets
for real meshes. The shader is unchanged.

## Files

- `crates/skate-game/src/retail_shader_tests.rs`

## Verification

`cargo test --locked -p skate-game --release --bin skate3rust -- shader_tests`:
all six pass (`sky_shader_validates`, world, depth in every shadow configuration,
character, and both vertex-location checks).

## Open questions

None. The PR plan's list of known upstream failures notes this one as fixed.
