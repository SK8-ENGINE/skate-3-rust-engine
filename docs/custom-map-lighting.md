# Custom map lighting

Custom `.skate` maps use the existing StandardMaterial PBR path: albedo, normal,
ORM, emissive, alpha mask/blend, and UV1 lightmaps remain intact. A scene-owned
shadowed directional light and ambient light now illuminate those surfaces and
PBR characters. Shadow cascades cover 100 metres; local authored point/spot
lights retain their existing settings. Area lights remain unsupported.

The environment header supplies start hour and orbit azimuth; extended headers
also supply sun/moon colour, intensity and day/night ambient. The adapter maps
unit sun intensity to 10,000 lux and unit ambient to 1,000 brightness. These are
portable rendering choices, not recovered retail constants. The start hour is
fixed: there is no new day/night controller or per-frame light update. Existing
baked diffuse on lightmapped custom surfaces is retained. The package horizon
colour remains the background; native sky sidecars are only loaded for retail
scenes. Zero authored light intensity is respected.

## Provenance and ambiguity

Startup and in-process loading share RetailScene::for_map. It checks embedded
retail material definitions, WMET retail-manifest metadata, RWCM collision,
SKYB retail sky, native collision edges and native rails. It never classifies
from a filename, map title or ordinary lightmap presence. The Blender exporter
writes WMET only for its retained SKATE3_RETAIL_MANIFEST; the retail pipeline
also writes WMET/RWCM. This conservatively protects imported Skate 2 scenes
whose material definitions may be absent, and edited retail-derived maps.
WCFG, BMAT and MOBJ are general authoring records, not retail evidence.

A legacy retail export stripped of *all* native evidence is indistinguishable
from custom PBR geometry. A custom rebuild retaining native records is treated
as retail. No unreliable name heuristic or new authoring UI is introduced.
Retain provenance when re-exporting retail maps. This classification protects
the existing rendering path; it does not add support for missing retail shader
families or missing sky/irradiance sidecars.

## Scene lifetime

Directional/local lights, sky and backdrop are MapEntity-owned and retire with
their scene. Ambient/background are replaced on publication. The gameplay
camera switches retail HDR/tone on and off; the GPU exposure meter resets on a
new scene generation. Retail character conversion retains original material
handles and layers, restores them on each transition, and binds fresh authored
lighting only in a retail scene. Dedicated zero-illuminance retail shadow maps
are removed/recreated, preserving the separate player-only baked-world map.
Character conversion is restricted to PlayerRoot descendants; only the gameplay
camera acquires shadow layer 28. Scoring layer 31 and session overlays keep
their existing cameras/materials. Custom character PBR materials respond to the
custom scene lights without a separate character light rig.

## Manual visual checks

- Open an ordinary custom map: compare upward and vertical surfaces, moving
  player shadows, normal maps, transparent textures and emissive surfaces.
- Check a map with local lights and/or baked indirect lighting; confirm those
  remain visible. Check an authored night start and zero-intensity sun.
- Open vanilla Skate 3 and imported Skate 2 maps: verify baked scenery remains
  unchanged and the dedicated player shadow remains visible where supported.
- Switch vanilla -> custom -> vanilla several times without restarting. Check
  sky/background, exposure, character shading and shadows for stale state or
  increasing brightness. Repeat with a custom character and clothing changes.
- Open scoring HUD, session marker and customization overlays on both map
  types; verify no world/player silhouettes or lighting appear in overlays.
- Rename a custom package to a retail-like name: it must still get custom
  lighting. A retail-derived package retaining provenance must stay retail.

No game, recomp, Steam or gameplay automation was launched for this change.

## Automated validation

`cargo check -p skate-game --tests --no-default-features --release --offline`
passed with existing warnings. A static-CRT release test harness was linked
directly into the worktree's ignored private output using the shared Cargo
cache. All six focused CPU tests passed: `map_render::tests` (3),
`retail_character::tests` (2), and `camera::environment_tests` (1). These checks
do not validate GPU appearance; use the visual checklist above.

SKATE15 loading is also covered by ten data-reader tests (including legacy
versions, both compression codecs, all storage transforms, texture references
and malformed input). A user-supplied v15 custom map passed the ignored CPU
`supplied_custom_map_prepares_and_retires` check: decoding, runtime validation,
material/mesh preparation, publication, custom-light classification and scene
retirement. Supply `SKATE_TEST_CUSTOM_MAP` and `SKATE_TEST_CUSTOM_ASSETS` to run
that check on another local custom map; no user map bytes are committed.
