# Retail renderer

The world material adapter follows Alex's native renderer at
[`f6e0ae8`](https://github.com/mchughalex/skate3recomp/tree/f6e0ae87fdfecbadb5c1e36c55d66a744187a3cd/src/native/shaders).
The reference files are `scene.hlsl`, `scene_common.hlsli`,
`scene_frame_cb.hlsli`, and `hdr.hlsl`. This is a Bevy implementation of
those shading equations, not a port of the game hooks or the whole renderer.

Implemented:

- World families 1–13: diffuse/lightmap squaring, the family lighting terms,
  decal art, macro overlay, normal/detail lighting and specular masks.
- Reflective-family cube sampling with all six faces and a complete mip chain.
- The shared HDR tone curve, including environmentdiffuse's reduced curve.
  The final output accounts for Bevy's sRGB conversion instead of applying
  gamma twice. The HUD is drawn after this pass.
- Alpha-tested world materials use the reference's 30/255 threshold in both
  colour and depth passes. Foliage is two-sided.
- Lightmaps remain bilinear, clamped and fixed at mip zero. Repeating world
  textures have mipmaps.
- All ten installed worlds resolve sky geometry/texture paths through their
  authored `world` collection inheritance. The parks inherit the default
  panorama; Downtown and Industrial override it. Sky X/Z follow the camera;
  Y comes from the selected `render_locations` chain, not camera height.
- The sky uses its material's diffuse and specular GUID bindings. The latter
  is the 512x16 radial sun gradient: sine-angle lookup, squared RGB divided
  by saturated alpha + 0.01, then the material multiplier and scene exposure.
  `material_sky/default.m_params[0]` supplies scale 0.75 and multiplier 0.35;
  these are decoded inputs, not constants copied from one frame. This follows
  `scene.hlsl`'s sky branch and `skate3_native_scene.cpp`'s sky-bank layout.
- The selected render location supplies the normalized sun vector used by
  the sky and existing world tangent-sign terms. This adds no directional
  light energy or shadow map to imported worlds.

The converter reads scalar material channels at record offset 0x10.
Earlier exports incorrectly read their string pointer and saved empty strings.
Reconvert maps to obtain those constants and complete reflection cubes.
Old packages still load; missing constants disable the relevant layer and
single-face environment textures are not treated as cubes.

The VLT converter now adds `array` to array fields, retaining the existing
`type` and `data` unchanged. `array.items` contains complete big-endian element
hex strings without padding; `capacity`, `element_size` and `alignment`
describe the source layout. Text arrays additionally retain decoded
`text_items` before the binary bank is discarded. The layout follows
VaultLib's `VLTArrayType`: four big-endian u16 header lanes followed by
individually aligned elements. Alignment in the schema is a base-two exponent.

Skies can be refreshed independently of map/character conversion:

```text
python -m tools.asset_pipeline.sky --game-root OWNED_EXTRACTED_GAME --assets PRIVATE_ASSET_ROOT
```

Use a private staging asset directory for experiments. This reads only the
database pair and sky resources; it does not require an ISO or Blender.
Legacy sky metadata retains its previous multiplier and has no sun gradient.

## Treeline investigation

The missing forest in the supplied University / Super Ultra Mega Park
comparison is not resolved by this change. The camera-relative
`WorldPresEntityOptimesh` anchor from `sub_82792900` is consistent with the sky
placement above, but the decoded University panorama has mountains and city
scenery, not a dense forest layer. Its RX2 contains one 380-triangle dome.
The separate `DIST_MegaPark` archive contains 102 mesh parts and no tree mesh;
it is not a source for additional trees at University's park.

The main University export retains 479 model assets, 8,546 mesh parts and 86
named TreeWall parts. Those named walls are outside the immediate park area;
their absence at that location does not prove geometry is missing. Nearby
proxy geometry demonstrably overlaps the main trees: in cell 350/-750,
451 of 456 proxy long-tree triangles match triangles in the 912-triangle
main tree after quantization to one millimetre, independent of winding.
The five nonmatching triangles are not evidence for an extra forest.
The main tree's diffuse and transparent channels resolve the same DXT5
texture, with nonconstant alpha, and its baked lightmap is retained. The
renderer already uses the retail cutoff in colour/depth and two-sided foliage.

No proxy geometry has been added. `skate3_draw_distance.cpp` documents the
ProxyWorld/full-detail exchange; rendering both whole sets is not a valid
fix. Identifying the specific missing silhouette now requires matched-camera
reference evidence connecting it to a submitted mesh/material or a streaming
collection. The two supplied images have different viewpoints. Native mip
selection and regional/streaming state also remain unverified.

## Remaining differences

This is not a claim of pixel parity. Native frame constants are supplied by
game hooks in the reference; this engine does not yet recover that controller.
Scene exposure 2.5, world material multiplier 1, and tree/proxy values still use
documented day-capture defaults. Imported maps have no dynamic directional sun or realtime
sun shadows: their world lighting comes from the baked lightmaps. Normal-map
sign terms use the initial world's authored direction when new sky metadata
is available; old packages retain the previous direction. Regional changes
and the runtime environment controller are not implemented. Fog selection
and its authored near/far/colour/power/max are retained in sky metadata, but
the CPU transformation to native frame rows is not yet proven, so shader fog
remains disabled. The selected `material_fog/default` must not be confused
with the different `fog_default` collection.

Native water, character shaders, SSAO, SSR, bloom and volumetric lighting are
not yet ported. The water arrays are recoverable now, but flowing-water
normal/tangent unpacking, ocean PCA input bindings and the separate horizon
water resource still need their own adapters. Character parity needs the
native CAC-composed texture/mask inputs plus verified key/rim/specular and
nine SH rows; portable GLB materials do not retain that contract. Reference
post effects need their depth/normal/reflection inputs and native pass ordering;
volumetric sun visibility also conflicts with the currently disabled world
sun-shadow source. These gaps are not enabled using guessed settings.
The character and
unsupported world families still use Bevy materials beneath the shared tone
curve. Reflection normals use the reference's analytic world-up construction;
there has been no matched-camera pixel comparison against the recomp.

The shaders are embedded in the executable. Sky and map payloads are extracted
from the user's game and must never be committed or bundled with releases.

### Treeline investigation and diagnostic (2026-09-08)

The user's mega-park screenshots refer to the bowl/cliffs in University, not
DIST_MegaPark (the separate create-a-park venue). A read-only decode of the installed
University SKATE14 package found 8,546 materials, 1,645,617 render triangles,
1,180 tree-family mesh materials and all 86 TreeWall meshes from the source cache.
TreeWall diffuse texture 0x2c70170a00053a88 retains its RGBA cutout, and the tree
materials retain masked alpha and two-sided flags. A TOC audit of all 479 full
models found no extra vertex or face buffers omitted by the parser.

Proxy comparisons must resolve mesh material GUIDs with
`_bind_material_groups_by_guid`; `mdl_parser.Mesh.name` is in material-table order
and can label proxy terrain as trees. With GUID bindings, the 14 proxy foliage
meshes intersecting x=200..550, z=-850..-600 contain 3,878 distinct triangles;
3,813 match full geometry after unordered-vertex quantization to 1 mm. The small
remainder does not establish absent trees (different triangulation/rounding also
changes these keys). DMO_University and DMO_Global contain movable props, not a
second background forest. These checks do not prove visual parity or a fix.

For a user-run discriminating capture, `SKATE_DEBUG_FOLIAGE=1` renders tree-wall
cards cyan and other tree-family surfaces magenta, opaque and unlit. Geometry,
camera, depth testing and occlusion remain authored. This bypasses texture alpha
in both colour and depth passes without adding proxy geometry. Return to the
reported viewpoint: solid foliage appearing in the missing area implicates the
normal shading/alpha path; an empty area requires checking placement, camera or
source selection. Unset the variable to restore normal rendering. This diagnostic
has been prepared without launching the game.

### Missing global tree-wall backdrop resolved (2026-09-08)

The user-run opaque-foliage diagnostic left the same empty background. The
missing surfaces are in `world/models/DIST_Water_University.rx2`, not in
`DIST_University` or its proxy stream. Despite its name, this global model has
both an ocean plane and a separate tree material: mesh 1 contains 1,572 vertices
and 786 tree-wall triangles. World fields `951898F6C0FA6856` and
`CA5A157A65E75934` select this model and its texture resource. The global
Industrial model likewise contains 716 foliage triangles.

`tools.asset_pipeline.backdrop` follows those world references and exports only
the tree-family presentation meshes into `private/native-backdrops/<map>.skate`.
Mesh-to-material and material-to-texture selection use binary GUIDs: texture
name suffixes differ from the global texture resource GUIDs. Original positions,
indices, UVs, masked alpha, two-sided rendering and baked lightmaps are retained.
No water surfaces or synthetic forest placements are added.

The renderer loads this supplemental package after the district and before sky
setup, through the existing retail mesh/material path. `parse_render_only` is
an explicit data-reader entry point for presentation packages; ordinary playable
map loading still requires collision. Presentation loading rejects collision,
rails, doors, lights, routes and extensions other than WMET. This does not alter
the playable map's collision or gameplay state. Future installation runs prepare
the supplement automatically; existing assets can be updated with:

```powershell
python -m tools.asset_pipeline.backdrop --game-root <owned-game> --assets <assets-root>
```

Static verification: the University supplement contains exactly the source's
786 triangles and matching vertex positions/indices/UVs. Zero of these triangles
match the 86 existing district TreeWall meshes after unordered-vertex 1 mm
quantization. Both University and Industrial supplements pass the runtime data
reader's offline inspector. Synthetic GUID tests and map-reader regression tests
pass. The executable was compiled but not launched; visual confirmation remains
for the user. The normal launcher clears SKATE_DEBUG_FOLIAGE.

For integration with staged map switching, prepare this supplement alongside the
district, before applying sky lighting. Track its entities and assets in the
same map lifetime. The current adapter is in `retail_backdrop.rs`.
