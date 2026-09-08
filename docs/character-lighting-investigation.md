# Character lighting investigation

Status: native character-lighting reconstruction started; no runtime lighting change yet.

Observed in the local reference renderer (`skate3_native_scene.cpp`, CaptureCharLighting,
and `scene_char.hlsli`): defaultcharacter/CAC shading consumes a directional key,
nine RGB spherical-harmonic rows, ambient multiplier, exposure, rim colour/direction,
key/rim specular colours and powers, material multiplier, and shadow visibility.
The character material path differs from the baked world-lightmap path. Hair uses
its own wrapped diffuse/ambient/rim response. Skin needs its dedicated specular
mask; opaque diffuse alpha is not a substitute. The player currently uses generic
StandardMaterial, with imported-world directional lighting disabled.

Observed in TU3 generated PPC source:
- 0x827E7280 reads nine Vector3 elements from field F45DFCAB6EC195CD, copies them
  into 16-byte rows at manager +16, and retains a default-probe mode byte.
- Collections with class hash 6A2B5946514B8490 have default/dlc/cas/freeskate rows
  containing this complete nine-element array. Class 1FFDC8E3ACA07C1F also has it.
- 0x827E7348 computes squared position differences against records at +160 and
  copies nine lighting vectors from a selected record. This supports a spatial
  lighting lookup, but does not yet establish the complete sampling/blending,
  registration, or fallback rule.
- `light_probes` (class 0E902F3F038A483C) contains global directional-light structs,
  ambient, and arrays of cSimpleLightData. Do not assume these are the same as
  the spatial baked SH samples just because of the class name.

Implementation work still required:
1. Recover the spatial irradiance resource/registration and sampling path and
   resolve default probe selection from world/render state.
2. Extract character frame inputs and per-material parameters through native
   references, retaining texture/normal/spec-mask semantics.
3. Add a skinned character material adapter that evaluates native SH, key, rim,
   and highlights, updated from character position. Keep world materials and
   gameplay/physics unchanged.
4. Supply character-only shadow visibility separately from the world's disabled
   additive sun/shadow rendering; establish the native source before choosing
   its renderer implementation.
5. Validate in exposed and shaded locations and with skin/clothing/hair. No
   game launches are authorized by this investigation; prepare user launchers.

Reference caveat: community symbol labels are hypotheses. In particular the
82E3B548 label must not be treated as a verified prototype. No copied renderer
capture's lighting constants should become a universal outdoor preset.
