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
- The three major districts use their actual sky geometry and diffuse texture
  from `miscload.big`. Texture selection follows material GUIDs through the
  RX2 reference table. Sky X/Z follow the camera; Y uses the reference's
  documented 165-metre default.

The converter reads scalar material channels at record offset 0x10.
Earlier exports incorrectly read their string pointer and saved empty strings.
Reconvert maps to obtain those constants and complete reflection cubes.
Old packages still load; missing constants disable the relevant layer and
single-face environment textures are not treated as cubes.

## Remaining differences

This is not a claim of pixel parity. Native frame constants are supplied by
game hooks in the reference; this engine does not yet recover that controller.
Exposure 2.5, material multiplier 1, and tree/proxy values use documented
day-capture defaults. Imported maps have no dynamic directional sun or realtime
sun shadows: their world lighting comes from the baked lightmaps. Normal-map
sign terms retain the adapter's fixed direction until native frame data is
available; this adds no light energy or shadow map. The fog inputs
are wired into the shader but remain disabled without authored frame rows.

Sky sun-gradient shading, park sky selection, native water, character shaders,
SSAO, SSR, bloom and volumetric lighting are not yet ported. The character and
unsupported world families still use Bevy materials beneath the shared tone
curve. Reflection normals use the reference's analytic world-up construction;
there has been no matched-camera pixel comparison against the recomp.

The shaders are embedded in the executable. Sky and map payloads are extracted
from the user's game and must never be committed or bundled with releases.
