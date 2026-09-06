# .skate maps

Run `PLAY-MAP.bat` and enter the map number. Alternatively drag a `.skate`
file onto `PLAY.bat` or `PLAY-MAP.bat`. Command-line equivalent:

```powershell
.\Launch.ps1 -Map 'C:\path with spaces\park.skate'
```

The original test environment remains available through PLAY.bat without a map.
Maps are parsed before initializing graphics; invalid files produce a readable
error and do not silently load the default level instead.

## Reader and adapter are different capabilities

| Data | Current support |
| --- | --- |
| SKATE01–14 records | Reader supports the documented layouts and raw, zlib and Zstandard blocks. |
| SKATE15+ | Rejected with a version error. V15's additional storage transforms are not implemented. |
| Visual geometry | Authored positions, triangle winding, normals, UV0/UV1 and material groups. Packed v12+ tangent frames are reconstructed. |
| Embedded materials | Albedo, normal, ORM, emissive, alpha mask/blend and indirect lightmaps. Lightmap RGB is decoded as `encoded² * 4` into half-float textures. |
| Collision | Separate authored collision mesh, reference 1 mm vertex welding, adjacent-edge and coplanar-vertex metadata. No collision inferred from visible triangles. |
| Surface types | Exact audio/physics/pattern packing from `EncodeRwSurfaceId`. Authored friction is retained; the original imported static-world contact combine remains in charge. |
| Spawn | Package position interpreted as the wheel-ground anchor, with the supplied wheel/body offset. Heading rotates about Y; coordinates stay Y-up metres. |
| Point/spot lights | Position, direction, color, intensity, range and cone settings are passed to Bevy. Renderer brightness parity is not established. |
| Sky/day-night | Metadata retained. Currently uses the package horizon as the clear color and existing game directional/ambient lighting. No full sky/celestial adapter yet. |
| Grind rails | Authored polylines and native spline words retained. Supplied game's `NoGrindEdges` path does not support grind engagement. A startup warning reports this. |
| Doors | Full records and geometry parsed. Launch rejected if present: no hinged rigid-body controller exists in the imported game. |
| NPC routes / area lights | Parsed and retained, but no runtime adapter. Startup warnings report them. |
| Retail shader definitions | Retained as original definition records. Portable material fields render; original shader families/decal channels are not recreated. |
| Native collision edge codes | Retained. Launch rejected rather than replacing these with generated edges; the retail packed-edge decoder still needs connecting. |
| Extensions | Decoded and retained. WMET/WCFG/BMAT currently report their limitations. Other extensions reject launch because they may contain required world geometry/controllers. |
| Empty texture placeholders | Parsed, but launch requires embedded textures. |

This is a map-loading integration, not a claim of full SK8 Engine renderer or
Skate 3 gameplay parity. In particular, maps can be readable without all their
runtime features being supported. Nothing bypasses the imported game's existing
`AddRunoutAttribs` failure, documented in the main README.

## Maps supplied locally

- `format-demo.skate`: original 60 m square checker floor with an embedded
  albedo and indirect lightmap; reproducible with `python tools/make_skate_demo.py`.
- `private/transition_grind_park.skate`: existing v8 transition map, copied from
  the user's research folder. Contains 510 visual / 398 collision triangles.
- `private/skate_parity_grid.skate`: existing v8 textured floor, copied from
  the same folder. Contains 2 visual / 2 collision triangles.

The user's v14 `blender_bake_showcase.skate` also parses (15,072 visual / 1,028
collision triangles), but contains a hinged door and is not a supported playable
map yet. It has not been modified to remove that feature.

## Source evidence

Layout, versions and compression were checked against the user's local:

- `Skate3Research/Skate3CustomEngineLayer/tools/blender_owned_map/SKATE_FORMAT.md`
- `Skate3Research/Skate3CustomEngineLayer/owned/world/src/owned_map_package.cpp`
- `SK8R15/Source/tools/blender_owned_map/SKATE_FORMAT.md`
- `SK8R15/Source/owned/world/src/owned_map_package.cpp`

Collision surface packing, welding and adjacency reference:
`owned/world/include/skate/world/rw_collision_mesh.h` and
`owned/world/src/rw_collision_mesh.cpp`. Spawn ground-anchor reference:
`src/skate3_native_collision.cpp`, native collision placement block.
All reference files remain outside this repository and were read only.

## Nonvisual checks

```text
cargo test -p skate-data --test skate_map --locked
cargo test -p skate-game skate_world::tests --locked
cargo run -p skate-data --example inspect_skate -- path/to/map.skate
```

Reader tests cover versions 1–14 and all three supported compression methods,
every truncated prefix of the v8 fixture, invalid references, non-finite input,
oversized counts, wrong byte order and unknown versions. Adapter tests check
separate collision ownership, packed surfaces, flat seam handling and embedded
lightmap decoding without creating a window. Gameplay and visual checks belong
to the user.
