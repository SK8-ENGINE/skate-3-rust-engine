# Character lighting investigation

Status: position-dependent character lighting implemented; user-run visual validation pending.

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

Initial investigation checklist (superseded by the implementation status below):
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

## Implementation, 2026-09-08

`tools.asset_pipeline.irradiance` extracts RX2 type `0xEB0024` from the district's
other presentation assets. Its section starts with count and relative array
offset; each 160-byte record contains nine float4 SH rows, XYZ at byte 144 and a
runtime link word at 156. The `SHP1` sidecar preserves groups, values and original
world coordinates, converting only endianness. University contains 17,493 samples
in 64 resources. Future map conversions write `<map>.irradiance` beside the map.

TU3 `0x827E6E28` registers these resources into a 41-by-41 spatial grid;
`0x827E6AC8` selects the nearest sample in three dimensions from the XZ
neighborhood and falls back to the default coefficients. `0x827E7348` checks a
six-entry MRU cache with a strict squared-distance threshold of 6.25. Raw binary
constant `0x820BD16C` is 15.0 (query margin); `0x822F8F38` is 6.25. The evidence
is the generated PPC instructions corroborated with resource contents and the
TU3 dump with SHA-256
`F4AA113EB541BFBA03DBC108CF5AB43F58C965B20FA3B82F9C40938A0AD841C4`.
Addresses use the dump's `0x82000000` base.

`retail_irradiance.rs` retains the native nearest-sample/cache behavior. Its
static-world adaptation searches all loaded groups using expanded bounds and an
XZ 15-metre neighborhood. It does not reproduce the original 12-resource
streaming limit or grid-cell boundary quantization. The freeskate collection
supplies the nine fallback rows; live changes of the manager's default mode are
not implemented. This is not trilinear probe interpolation.

`tools.asset_pipeline.character_lighting` resolves each selected model's
AttribulatorMaterialName through VLT inheritance and exports its nine complete
material parameter rows. Skin/face specular textures preserve the original red
channel separately; the GLB's grayscale inverse-roughness conversion is not used
as the native specular mask. Source assets remain private.

`retail_character.rs` replaces the ten named retail GLB materials while retaining
Bevy's skinned vertex path. It updates the SH rows from PlayerRoot's position.
The fragment shader evaluates the native SH basis, authored directional key,
ambient multiplier, rim response, key/rim highlights, normal maps and material
multiplier. Diffuse sampling accounts for the GLB's sRGB texture format before
the retail gamma-two decode. Output remains linear HDR for the existing shared
tone pass. The separate depth shader retains alpha rejection.

A zero-lux directional shadow source provides world/self occlusion to the
character's key and key specular terms. The baked world shader does not sample
this shadow map; remaining StandardMaterials receive zero additive sunlight.
This uses Bevy cascades/filtering/biases, not the console's exact cascade layout
and 9-tap shadow filter. It does not add a skater-shadow receiver to baked world
surfaces. No gameplay or physics code changes are involved.

Hair now uses the native wrapped diffuse/ambient/fresnel response, with the
authored material colours and the existing GLB dye/coverage composition. The
live CAC colour-register derivation and separate second-UV blended strand pass
are not reconstructed; hair keeps the installed cutout mode. This is an explicit
adapter limitation, not a claim of exact hair parity.

Scene exposure remains the existing 2.5 day-capture value shared with the world;
the regional exposure controller, water and fog are still unfinished. Missing
lighting sidecars leave the existing GLB material path active and log the reason.

Validation: 13 synthetic Python extraction/inheritance/GUID tests, two spatial
selection/cache tests, and the windowless world-render adapter test pass. Character
colour, shadow and normal-depth shader variants pass offline Naga composition
against the vendored Bevy 0.18 shader modules. Release compilation passes. The
game/recomp was not launched. User visual checks should cover exposed terrain,
sheltered spots, skin/clothing highlights, hair coverage and body/board shadows.
