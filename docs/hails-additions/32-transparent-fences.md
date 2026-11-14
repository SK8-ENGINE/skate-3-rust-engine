# See-through fences and grates: retail `environment.transparent` shading

Branch `fix/transparent-materials` (off `main` `b3c96793`). First written on
`fix/fence-alpha` (off `hails-additions`); this branch is the port to `main` for
its own upstream PR (bundle O in [PULL-REQUESTS.md](PULL-REQUESTS.md)).

## Problem

Chain-link fences, wire mesh and grilles, for example the row of fence panels
behind the bus stop at the Industrial Ghetto Spot (locator `Z_Ind_GhettoSpot`,
about (-516.9, 3.6, -294.5)), draw as solid black panels. Retail draws the wire
mesh with the background visible through it.

## Root cause

- [data] The fence panels use the retail world shader `environment.transparent`
  (Industrial materials 884 `0x15EA09769DB7638D_12`, 1179 `0xC50D7FD400B4BECD_17`,
  1293 `0x3A36F543A04A4DA9_26` within 30 m of the spot; the exported map stores
  `alpha_mode 2`, so the draw is already blended). Game logs list 429
  `environment.transparent` materials in one district and 90 / 15 in others.
- [code] The converter had no family for this shader
  (`tools/asset_pipeline/retail_material.py` `_retail_shader_family` fell
  through to `return 0`, line 46 on this branch), so the renderer counted it as
  unsupported and drew it as family 1
  (`crates/skate-game/src/retail_render.rs:803`, "unsupported family renders as
  plain diffuse plus lightmap", logged at `:821`).
- [code] Family 1 is `environment.default`, an opaque program: the shader leaves
  the output alpha at 1.0 (`crates/skate-game/src/retail_world.wgsl:119`,
  `var alpha = 1.0`; only families 7 and up take the diffuse alpha, `:307`).
  The blended draw is therefore fully opaque, and the wire texture's empty
  texels (alpha 0, near-black color) fill the panel.

## Evidence (retail program)

`transparentenvironment_defaultPS.fpo` from `data/big/shaders_final.big`, read
with a Xenos shader microcode disassembler (ALU slot numbers):

- 23..26, 28, 31, 33, 34: four lightmap taps averaged (x 0.25); 36..38:
  `min(lm^2, shadow + (0.05, 0.09, 0.13))`, the same shadowed lightmap as the
  other world families.
- 9, 10, 14: diffuse squared; 39: `diffuse^2 * lightmap`.
- 30..42: specular from the pseudo light (-0.14, 0.5, 0.9) reflected about the
  interpolated normal, `(2.1, 1.8, 1.5)`, masked by `lightmap.g` and the
  specular texture; no normal-map fetch, no `kd` term.
- 43: `lit = diffuse^2 * lightmap * diffuse.a + specular`.
- 42, 44: fog with `m_params.y` (c10.y, last swizzle component rule) as the
  multiplier; 45..53: exposure c9.x and the shared tone curve.
- 53: `oC0.w = diffuse.a * diffuse.a`.

These are the terms our family 13 (`environment.reflective_trans`) already
implements, without its normal map and reflection cube.

## Change

- `tools/asset_pipeline/retail_material.py:16`: `environment.transparent` is
  family 16.
- `crates/skate-game/src/retail_render.rs:56`:
  `TRANSPARENT_ENVIRONMENT_FAMILY = 16`; `Definition::parse` (`:478`) upgrades
  map packages that stored 0 for this shader, so existing installs need no
  re-export; `supported` accepts it (`:545`).
- `crates/skate-game/src/retail_world.wgsl`: family 16 takes the shadowed
  lightmap (`:122`), `lin = lightmap * diffuse^2 * alpha` (`:291`),
  `alpha = diffuse.a^2` (`:309`) and the `m_params` multiplier (`:314`), like
  family 13. It reads no normal map (`:83`), no macro overlay (`:265`, families
  below 13 only) and no reflection cube (`:299`).

## Files

- `crates/skate-game/src/retail_render.rs` (family constant, parse upgrade,
  `supported`, two tests)
- `crates/skate-game/src/retail_world.wgsl` (family 16 terms)
- `tools/asset_pipeline/retail_material.py` (family mapping)
- `tools/asset_pipeline/test_environment.py`
  (`TransparentEnvironmentMaterialTests`)
- `docs/hails-additions/32-transparent-fences.md`, `README.md` index row,
  `PULL-REQUESTS.md` row O

## Moddability

The family follows the material's shader name, which is map data: the
converter maps shader names to families in `retail_material.py`
(`_retail_shader_family`), and the renderer reads the family and the material
parameters (`m_params`, bindings) from each map package's definition records.
A custom map or mod material that names `environment.transparent` gets the same
retail shading, with its own diffuse alpha, lightmap and specular textures.
Nothing here is a per-object rule.

Hard-coded values that could become data (open, not changed here): the
specular pseudo light `(-0.14, 0.5, 0.9)` and color `(2.1, 1.8, 1.5)` and the
shadow floor are shader constants shared with the other world families
(`retail_world.wgsl:293`, `:297`), and the material multiplier
(`m_params[0].y`) is still the constant 1 for every world family, the same open
item as in the world radiance notes.

## Retail parity and tests

The shading is a term-by-term port of the retail pixel program
`transparentenvironment_defaultPS` (see Evidence); no value was tuned by eye.
Tests:

- `retail_render::tests::transparent_environment_takes_its_own_family`: a
  definition record naming `environment.transparent` gets family 16 whether the
  package stored 0 (old exports) or 16, and is supported; other shaders keep
  their stored family (`environment.reflective_trans` 13,
  `incandescent.transparent` 0).
- `retail_render::tests::transparent_environment_shader_matches_retail`: the
  WGSL holds the retail terms (ALU 39/43 alpha-scaled light, 53 alpha squared,
  shadowed lightmap, `m_params` fog multiplier) and no normal-map read for
  family 16.
- `test_environment.TransparentEnvironmentMaterialTests`: the converter maps
  `environment.transparent` to 16 with the blended render flag, and keeps
  `environment.reflective_trans` at 13.
- The existing naga validation tests of the retail world shader cover the
  changed WGSL.

## Verification

Not run yet on this branch: the build was stopped before the tests ran (2026-10-08). The first version (on `fix/fence-alpha`, off `hails-additions`) passed 2/2 `transparent_environment` tests and the 46 retail shader validation tests; the Python converter tests pass here (6/6). The PR stays a draft until the tests above pass on this branch and the fences are checked in game.

## Open questions

- In-game check at the Ghetto Spot still to do.
- Retail render state for these draws (blend factors, depth write, cull mode) is
  not read yet; we use the engine's existing blended class
  (`AlphaMode::Blend`, `retail_render.rs:263`; back-face culling unless the
  material is two-sided, `:412`).
- The specular mask channel and power use the same convention as the other
  ported world families; the texture fetch destination swizzles were not decoded.
- `incandescent.transparent`, `advertisement.default`, `animated.flag` and the
  traffic light shaders also still fall back to family 1.
