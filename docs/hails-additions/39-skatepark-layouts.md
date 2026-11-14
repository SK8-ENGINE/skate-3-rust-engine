# 39. skate.Park layouts: the Full / Basic worlds place their pieces

Part of PR #52 (living world, item 9 of its list; asked by the user 2026-10-20: our Skate.Park entries do not seem to
load the maps properly; for instance Stadium - Full has many placed objects that build it).

## Problem

Skate 3's skate.Park worlds share one district stream per park (MegaPark, StartPark, IndustrialSkatePark,
DownTownSkatePark and the DLC lots) and differ by a layout of placed pieces: "Stadium - Full" builds the stadium course
from 450 pieces, "PCU Training - Basic" from 251. Our maps were only the streams' shells: no world loaded its pieces,
and the Challenge Map could not reach the Full / Empty rows at all (setup kept only locations with their own artwork).

## What retail does

Research b424, b425, b429, b430, b431, b434 (addresses TU3, title update 3):

- A world row names its layout in field `Hash_79B248E8469DC5DA` (`skatepark\MegaPark_Fullpark.park`, disc
  `miscload.big`; the DLC packs carry their own) [data]. The stream's own row names the Empty layout (0 pieces).
- The `.park` file: version, four block lengths, origin world hash, then static pieces (36-byte records in the
  tutorials, 48-byte with an RGB tint elsewhere; blocks are allocated larger, so a reader skips by length), movable
  pieces (40-byte records), and two id lists [data, b425]. Piece ids are `<mod>` rows of the static recipe
  (`recipe/static/static_db_*.xml`) whose first `<lod arenaid>` is the piece's RX2 [data].
- The static reader (82868318) skips id 0 and builds the matrix from the quaternion x y z w as R transposed for the
  row-vector engine, position as row 3; the piece ctor (82865108) copies it unchanged, no template matrix on top
  [code, b434; all 533 disc templates have identity matrices anyway, data].
- Every piece is its own object: collision is a per-piece registration in the physics world's static set, rebuilt on
  each layout apply (b431 section 2, [code] for the add / clear / build calls); grinds go through the static piece
  manager, a pluggable provider of the shared rail query, posing the template's splines by the piece matrix
  (82868860 -> 82C1F098, b431 section 1, [code]). The tint is a per-draw constant that multiplies the linear albedo
  (b433, [code] in the shaders).

## Change

Setup (`tools/asset_pipeline/park_layout.py`, wired into the maps group in `install.py`):
- `parse` (the container, both record formats, the movable block), `recipe` (piece id -> RX2, `cap.*`), `Sources`
  (disc `miscload.big` + `parkassets.big`, then the DLC packs' BIGs; a later archive's row, model or texture replaces
  an earlier one), `world_layouts` (every world row's layout through its parents).
- Each piece template is decoded with the DMO (dynamic prop) readers (`dynamic_props.template_meshes`: mesh, collision, grind
  splines) and its textures from `static/texture/<GUID>.rx2`; the placements are written by
  `dynamic_props.write_placements` (split out of the district prop export, unchanged output for props) as
  `private/park-layouts/<layout>.skate` + `.json` (instances with stable ids `<layout>#s<index>`, template, matrix,
  tint, caps; the movable pieces listed, not yet spawned), plus `worlds.json` {world row: {layout, stream}}.
- `teleports.py`: an fe_locations row whose world names a layout is a destination even without artwork, named by its
  world row's text ("Stadium - Full", "Team HQ - Empty", "PCU Training - Basic"); every destination records its
  world row. A location's locator resolves as retail does (research b436, locator system 82910DC0 [code]): by name
  in one global table, whatever the district (same district first, which keeps every other destination unchanged),
  so the Stadium's layout rows use DownTown park's `Z_SP_DT_Start` (66.1, 0, -64.0) as retail does; a name found
  nowhere (`vert_skp5_01_challengelocator_01`, Monster Park) gets retail's identity at (0, 0.5, -5) (player 1).

Engine (`crates/skate-game/src/park_layout.rs`):
- The Challenge Map plan (`challenge_map.rs`) resolves the destination's world to a layout; another layout on the
  current map is a map load with `map_library::Entry::world` set (the request stays serializable).
- The map loader (`map_transition.rs`) marks the loaded map with the layout (an in-memory `PARK` extension, never in a
  file); `CurrentMap::layout` remembers it.
- Physics (`physics.rs`): `skate_world::collision_world_with` appends the posed piece triangles (retail shapes with
  their native edge codes, else the render mesh, as for props: `skate_world::posed_collision`) after the map's, one
  query range per piece; `StaticProvider::with_pieces` adds one rail asset per piece (`park_layout::pose_rails`,
  near-vertical bit as 82C1E568, owner tag `PIECE_OWNER_TAG` below the moving-rail tags).
- Render (`map_render.rs`): the pieces spawn as `ParkPiece` entities (never `PropInstance`).

Park start (the user's retail video, 2026-10-21: in retail the player spawns at the top of the big ramp):
- The `.park` editor tail's first matrix is the park's player start (b425 section 4 had it as "likely the saved editor
  camera or player start"): Stadium - Full (241.2, 37.1, -91.0) on top of the big ramp, the video's first frame and
  ours match. `park_layout.parse` reads it (`start`); `teleports.py` gives a park row its layout's start instead of
  its location's locator, except a location with its own artwork and locator (`startpark_progression`, ours: not
  traced for those). Test `test_the_editor_tail_holds_the_park_start`.
- Tint: the recomp draws the pieces untinted (the deck beige), our port tinted as Skate 3's code and data give it
  (b445; the deck's record (126, 100, 47)). The user (2026-10-21): our shaders are already better than the
  recomp's, which are clearly not quite right; decision "Keep the tints".

Park teleporters (research b438; the user, 2026-10-20: teleporters are set to the same channel, and when a player
gets close a small dialog on screen asks whether to teleport):
- The teleporter pieces have no model. The disc vault class `Hash_D01B99210BA7323C` gives each one a channel 0..3
  and a role, 0 start / 1 end (field `Hash_77B8CA7AC65C8504`) [data]. Per channel, retail takes the first placed
  start and end piece (8287B918 / 828687C0) and builds an ordinary teleport challenge (8286D6E8: title
  `ID_COMMON_TELEPORT`) with a box entry volume of half extents 2 (8286CFF8) around the start piece's world center,
  arriving at the end piece's world center with its axes [code].
- `park_layout.layout_teleporters` writes those as `private/park-layouts/teleporters.json` (the `teleporters.json`
  record shape, plus `layout`); `teleporters.rs` loads them beside the disc's and arms one only while its layout is
  loaded. They use the ported prompt and confirm (doc 38 "Teleporters"). Zen Plaza - Full: one, channel 0, entry at
  (37.6, 2.0, -21.8), arrival at (15.4, 16.8, -13.0). Tests `test_teleporters_pair_the_first_start_and_end_of_a_channel`,
  `a_park_teleporter_is_armed_only_with_its_layout`. Open: "first placed" follows our record order (retail's is a
  hash map order); whether the arrival snaps to the ground.

Movable pieces (block 2; research b435):
- They are ordinary DMOs [code: 82883828 spawns them with the district DMO descriptor; template = the dynamic recipe
  mod's arena id, 119 / 119 disc mods resolve to a parkassets.big `dynamic/model/dynamic/dmo_*` template; the u32
  flag is never read on load]. Setup writes them per layout as `<layout>.props.skate` / `.props.json` (the district
  prop shapes: types, collision, grab splines, rails; stable ids `<layout>#d<index>`). The engine's prop layer takes a
  loaded map's layout package in place of `native-props/<map>` (`skate_world::prop_files`; no park stream has
  district props today, so nothing is dropped; a stream with both would need them merged), and saved prop poses are
  kept per layout (ours). All 71 disc + DLC movable pieces resolve (Stadium 1, Starter 3, Back Lot 47, Zen Plaza 17);
  the Stadium start loads the barrier with its type data, collision, grab spline and rail.
- Checked 2026-10-29 (research b681): the initial sleep state matches (layout pieces go through the same DMO factory
  `826B8830` as district DMOs; the descriptor bytes +0x50..+0x54 go into the creation message, read by neither DMO
  constructor; ours starts them asleep, `initially_awake 0`). Census: retail's park load (`825111F8` mode 1) spawns
  pieces with no placement record, so they are never culled; ours marked every map body streamable, now layout pieces
  are not (`PropDynamics::set_layout_pieces` from `load_map_prop_layer`). Also retail refuses a DMO spawn past 49 live
  (no shipped layout reaches it; Back Lot has 47). Open: who reads the creation bytes; the 49 cap as data.

Park shading (research b433, b439 to b443):
- The pieces' materials use Skate 3's own programs, families 50 / 51 / 52 for `environmentpark.diffuse` / `.decal` /
  `.alphatest` (`retail_material.py`; `retail_world.wgsl` branch `fam >= 50u && fam <= 52u`, ALU slots cited). Diffuse:
  albedo = diffuse squared times the piece's tint; the tangent normal is not normalized before the frame; the light is
  the tangent-frame pseudo light (kd, as dynamicobject's) times min(lerp(dynamic shadow color, 1, visibility),
  lerp(exponential shadow color, 1, saturate(10 N.L) x visibility), projected lightmap), with no direct N.L,
  counter light or ambient; the specular reflects the world's literal light, unshadowed; fog scale m_params.y. Decal:
  the same, with the decal mixed in by its alpha before the tint and the detail normal. Alphatest: dynamicobject's
  shape with the occlusion in place of its shadow; output alpha = the raw diffuse alpha, tested at 30 / 255
  (`GREATEREQUAL`, blend state 1, b441), two-sided.
- Data: the tint per piece (layout record, white default; b439: the renderer's slots start white and take the
  record's), the shadow colors per park world (the world row's two Vector4 fields when its park bool is set; other
  worlds (0.2, 0.2, 0.2), the retail code constant; written into each layout's sidecar as `shadow_colours`), and the
  park materials' `m_params` rows (vault class `material_environmentpark`, now in `render-parameters.json`; an older
  install falls back to the retail fog scale 1). The engine gives each piece a material with its `ParkDraw` (tint +
  colors), one per distinct value (at most 97 per layout).
- Projected lightmap (b439, b440, b444): each park world row names a lightmap (`Hash_AA9212BDE5A603B4`) and its
  x0, x1, z0, z1 bounds; the textures are 512 x 512 DXT1 RX2s (disc `miscboot.big` `data/content/textures/`: megapark,
  startpark, downtown, industrial; the Create pack: backlot, downtownlightmapnight; the San Van pack: zenplaza).
  Setup binds the layout's lightmap as every piece material's lightmap channel and writes `projected_lightmap`
  (texture, bounds) into the sidecar; the shader samples it once at retail's projection, u = (z - z0) / dz +
  y lz / (ly |dz|), v = (x - x0) / dx - y lx / (ly |dx|) (the `|d|` form kept as retail has it, also for Back Lot's
  reversed bounds), L = the sun direction toward the sun [code for the convention; that L equals our render
  location's sun direction is not traced, b444], used linearly inside the occlusion min. All 11 park layouts get
  theirs.
- Decal UVs: the piece export first wrote only the diffuse UV set, so the decal programs fell back to it. The user
  (2026-10-21, Stadium - Full): "It looked like they are not mapped to it properly, it seemed like the right texture,
  but like it was offset weirdly on the object". Each mesh now carries its decal UV set (`decode_decal_uvs`, as the
  map exporter writes it); after the fix: "that time it looked much better".
- Ours: our one shadow map feeds both of retail's shadow terms; the lightmap sampler addressing (retail's sampler
  state was not found; ours repeats); the alphatest specular's occlusion channel (green taken).

Not retail (ours, the same outcome): a layout change reloads the map instead of re-applying in place; piece rails
join the static pass, so under the 40-record cap they come before the props (retail queries them after the DMO
provider; parks have few props).

Recipe precedence across packs (checked 2026-10-21, `tools/dev-bench/recipe-precedence.py`): the disc's `_000` and the packs'
`_005` / `_010` / `_012` / `_015` / `_017` static recipes share 664 of 682 piece ids and the dynamic `_000` / `_015` /
`_017` share 140 of 141; no shared id has a different name, model (arena id) or cap row, so the load order cannot
change a piece. Setup reads them disc first, then the packs.

NOT RETAIL YET: the lightmap's sun direction L (see "Park shading", b444).

## Moddability and multiplayer

- A layout is data: a mod ships `<layout>.skate` / `.json` and a `worlds.json` row (same shapes as setup's) or
  replaces one; pieces keep stable ids; the world row choice is part of the map request a host would send.
- The pieces are fixed, so every peer builds the same static world from the same files.

## Verification

- Setup tests `test_park_layout` (container, filler, tutorial records, rejects truncation, rotation convention,
  recipe rows; data-gated: the disc layouts resolve every piece with b424 / b425's counts 450 / 328 / 256 / 251 /
  238). A disc export: 12 layouts, 0 missing pieces; MegaPark_Fullpark 56 templates, 45 with collision, 48 with rails.
- Engine tests `park_layout::tests` (a world places its layout only on its own stream, rails posed by the piece
  matrix, the layout mark replaced on re-attach) and the Challenge Map plan (Empty to Full reloads with the world, the
  same layout does not).
- Dev install (2026-10-21, maps refresh plus the park re-export with the DLC packs): 18 layouts, every disc and DLC
  piece resolved except Zen Plaza's two Create-a-Park teleporter pieces (`qa_SkateParkTeleportStart1`,
  `qb_SkateParkTeleportEnd1`: their RX2 has no DMO model, left out and reported). A muted start as "Stadium - Full"
  (`--map MegaPark.skate --teleport megapark_full`): `SKATE_PARK_LAYOUT ... pieces=450 shapes=45 collision_pieces=450
  triangles=197832 railed_pieces=398`, 7029 rail primitives, 450 piece draws; the frame shows the pieces the Empty
  start lacks. The whole skate-game suite passes (867).
- Not play-tested.
- Collision (research b437, data): every piece template carries one ClusteredMesh and no primitive volumes; the
  Stadium's 197832 piece triangles are retail's own (its meshes are subdivided, e.g. a street pad 384 triangles for a
  60-triangle render mesh). The rails and fences (11 of the Stadium's 56 templates) link their volume through the
  other EB000A form, (0, count, 12, volume index..., 0) with no assembly, which `dynamic_props.template_collision`
  now follows: all 661 parkassets.big templates get their retail shape (edge codes, surfaces), none falls back to the
  render mesh. The same reader serves the district DMO props: a DMO template with that link form gains its retail
  shape at the next maps refresh (check the props after it).

## Open questions

- The tint path on the CPU and the `environmentpark` families (b433 change list).
- Movable pieces (b435), the Monster Park challenge locator (2026-10-29, b681: the row belongs to the challenge "Monster Park Jam"; retail's Challenge Map never lists it and our travel menus now leave out destinations placed by the missing-locator fallback, `Destination::fallback`; the real entry comes with challenges), `cap.AllowCollision` (the DLC recipe precedence is moot, see
  "Recipe precedence across packs" above).
- Do-not-fuse pieces (see "IgnoreFuser pieces leave the fused set"): a second source of flag 2 (a template map at
  mgr+0x3B2C, b482) is open; retail's second read of fused pieces for props: settled, none (run 4, see "Props and
  IgnoreFuser pieces"); the ground set's reduction is ported (doc 27 "Ground contact reduction").
- The two dead stops of the laptop session 2026-10-21 21:38 where the user saw nothing (see "Seam audit of every
  layout"): verify captures (2026-10-22) show both pieces drawn (Back Lot `ab_stairs_sm_Downtown` bottom riser,
  DownTown `aa_padStreet_md_Downtown` raised edge). The trace shows head-on hits at 8.2 and 5.7 m/s with the speed
  gone in one frame and no bail; what the user saw is being asked.
- The seam audit's leftovers: 8 floor gaps in Back Lot, 4 in Zen Plaza; edges with more than two triangles and flat
  floor ridges not yet classified.

## The pieces fuse into one collision set: no catching on seams (2026-10-21, research b477 / b478)

- **Problem (laptop session 2026-10-21, the user):** in Stadium full, skating down the big ramp bailed
  almost at once: the cracks between objects do not connect properly, so the skater gets stuck,
  stops, bails or bumps over the crack; on the BackLot DLC map the cracks between surfaces often catch or stop the
  skater. The log: 4 bails in 25 s down the Stadium ramp, 4 on BackLot's flat ground, all on layout pieces.
- **Cause [data / code, b477]:** the seams are flush (0.1 to 1.5 mm), but each piece kept its own retail edge codes:
  its open borders are coded convex (0x60 = cosine -0.234), and nothing paired the edges two pieces share
  (`portable_parts` decoded the native codes over the welded pass). The trace shows the wheel contact mask dropping at
  the seam crossings.
- **Retail [code, b478, medium-high]:** the SkateParkView (`world+0xBC` slot +0x20, vtable `82310560`) fuses all
  static pieces in its "fuserJob" (`827762D0`, build `827765C0` -> `828728B8`): per piece `82872D70` copies only the
  posed corners, piece index and surface of each triangle (the edge-code bytes are skipped), and `82876188` merges
  vertices, removes zero-area triangles (`82AC2010`) and pairs neighbors over the whole set. Open: the exact
  edge-writing function and which builder default (0.01 or 0.1) is the vertex merge.
- **Change:** `skate_world.rs` `portable_parts_fused`: the posed pieces' vertices merge across the whole set within
  `Pieces::weld` (data: the layout sidecar's `fuse_weld`, default 0.01 m), every edge code is rebuilt over the fused
  set (the pieces' own codes are ignored, their surfaces kept), triangles the merge collapses are dropped and the
  piece ranges follow. Disc maps and props keep the exact 1 mm path. Log: `SKATE_PARK_FUSE`.
- **Verification:** `fused_park_pieces_pair_their_flush_seams` (1.5 mm seam paired and flat; the 1 mm path leaves it
  open; a 5 cm gap stays open; deterministic), `fused_pieces_drop_collapsed_triangles_and_keep_their_ranges`; verify
  runs load "Stadium - Full" (450 pieces, 197832 triangles) and "Back Lot Park - Full" (453, 188784). skate-game 882.
  Not play-tested: whether the ramp and BackLot still catch.

## Back-to-back faces between pieces are removed; edges pair with the best neighbor (2026-10-21, USER-ASKS 221)

- **Problem (laptop session 2026-10-21 21:38, the user):** the Skate.Park maps still have many physics and collision
  issues, both at seams between objects and around objects in general. The trace (`tools/dev-bench/park-catches.py`
  on `audio_state.tsv`): on Back Lot the wheels left the ground and the board rose 5 to 15 cm on flat ground exactly on
  the line where two `aa_PlazaPad_sm_Downtown` pads meet (x -151.77 and -139.77, 0.749 m high). Two dead stops (Back
  Lot into the foot of `ab_stairs_sm`, DownTown park into a bench / steep ramp toe) are real collisions, not seams.
- **Cause [data]:** each pad has its own side wall on the shared face, so after fusing the top seam edge is shared by
  four triangles (two tops +Y, two walls +X / -X; `tools/dev-bench/park-seam-edges.py`). Our pairing took the first
  reversed edge it met, so a top could pair with a wall: a 90 degree convex lip under the wheels.
- **Retail [code]:** `82876188` runs, before pairing, `828755B8`: two triangles of DIFFERENT pieces (triangle +0x14)
  sharing an opposite-wound edge whose score is above 2.99 (`822F90E8`) or below -0.99 (`822F8E60`) are faces lying
  back to back; with the same third vertex both are marked 0x80 (removed), otherwise their quads (`82875438`: the same
  piece's triangle across the next edge) are compared and all four removed when they cover the same corners. The score
  `82AC00A0`: d = n1.n2; convex or flat (edge . (n1 x n2) > -1e-6, `822F8ABC`) -> max(d, -1); concave -> min(2 - d, 3)
  (constants `8216DEE0` -1, `82060C50` 2, `82063B08` 3). Pairing `82875B20` / `82875CA8`: per edge the neighbor with
  the highest score wins, a better pair displacing a worse one (unpaired edges score 1.0, `8231A844`).
- **Change:** `skate_world.rs` `fused_edge_features` (the fused park path only; disc maps and props keep their path):
  zero-area drop, the back-to-back removal (pairs and quads), best-score pairing, normals from the merged corners (as
  retail), then the edge codes as before. `portable_parts_fused` drops the removed triangles and keeps the piece ranges.
  Back Lot Park - Full: 188784 triangles, 19650 removed.
- **Tests:** `fused_pads_drop_their_back_to_back_walls_and_pair_the_top_seam` (same and crossed diagonals; the same pads
  inside one piece keep their walls), `fused_park_pieces_pair_their_flush_seams` (cosine within 1e-5 now, merged
  normals), asset-gated `backlot_plaza_pad_seam_is_flat_after_fusing` (all 6 seam top edges pair flat with the other
  pad's top). skate-game 885 passed. Not play-tested.
- **Open:** retail skips static entries with flag bit 2 (`entry+0xB8 & 2`) when it builds the fused mesh (`827765C0`,
  checked; research b480): those keep their own per-piece collision. Which recipe column sets the bit (b480: record
  +0x10, likely `IgnoreFuser`, not proven) is open; our fuse takes every piece. The editor flow (b480,
  research b480) is partial.

## IgnoreFuser pieces leave the fused set and the skater's world (2026-10-22, USER-ASKS 233)

- **Problem:** our fuse took every placed piece. Retail leaves some out (open item of the back-to-back section).
- **Retail [code / data, research b482, b483, b484]:**
  - The fused build `827765C0` skips static entries with flag bit 2 (`entry+0xB8 & 2`).
  - Spawn `828662B8` sets that bit from recipe record +0x10, read as a byte that is neither 0 nor 0xFF (checked by
    main in the recomp).
  - The column is `IgnoreFuser`: the disc recipe rows (parkassets.big static_db, 0x20-byte rows) match the XML
    `cap.*` values for all 542 static and 119 dynamic rows (b482).
  - It is set on hedges, bushes and shrubs only (`hc_Hedge`, `hd_Shrub`, `he_Bush`).
  - Every spawned piece also gets a TerrainPool entry with its template's own collision (`82776238` -> `8276CB18`, b483).
  - The skater's ground query reads only island+0x5590. The island ctor `827632A8` stores it from r7, and `8275DCC8`
    passes the container's GroundPool (+0x2C, the fused object) there for both islands (verified).
  - So IgnoreFuser pieces never touch the skater. Props (DMOs) do: a DmoGround task reads the TerrainPool (b484).
- **Change:**
  - `park_layout.rs` `Pieces::unfused`: per piece, from the sidecar instance's `caps.IgnoreFuser` (setup already writes
    `caps`).
  - `skate_world.rs` `append_pieces` / `split_unfused`: only the other pieces are fused into the skater's world.
  - Log: `SKATE_PARK_FUSE ... unfused_pieces=N`.
  - Moddable: a mod's layout sidecar sets or clears `IgnoreFuser` per instance.
- **Tests:**
  - `ignore_fuser_pieces_stay_out_of_the_fused_set`: the split, an older sidecar with no flags, determinism, only the
    fused set reaches the world.
  - Asset-gated `ignore_fuser_counts_per_layout`: 16 / 8 / 9 / 13 in backlot_full / Downtown_Fullpark / ZenPlaza_Full
    / Nightpark, 0 in MegaPark and Starter.
- **Not play-tested.** Skating through a hedge or bush in these layouts is now retail behavior.
- **Props:** retail props still touch these pieces; ported 2026-10-22, see "Props and IgnoreFuser pieces".

## Props and IgnoreFuser pieces

(2026-10-22, follow-up of USER-ASKS 233; todo park-props-vs-unfused.md)

- **Problem:** after the IgnoreFuser change, the unfused pieces were in no world at all, so props (bins, cones, Move
  Object pieces) fell through hedges and bushes.
- **Retail:** the DmoGround task reads the TerrainPool with no filter, so props get every piece's own mesh, fused
  pieces included (research b485: `8277C5D8` -> `8277CAA0`, bbox test only; `82763EE0` kicks it with r8=0, the
  GroundPool tasks with r8=1, verified). Both DMO jobs write into the world's one contact stream (island +0xC,
  research b492; r4 = r5 = 0 on every `827771E8` kick verified). For an IgnoreFuser piece the TerrainPool read is
  the only one, so props collide with its raw mesh.
- **Change:** `skate_world::props_only_world` builds a world of the IgnoreFuser pieces (native edge codes, one query
  range per piece, layout order); `PropDynamics::set_props_only_world` stores it at load and both prop contact passes
  (`row_corrections`, `contact_corrections`) read it after the step's world. The skater, walker and NPC queries never
  see it. Log: `SKATE_PARK_PROPS_ONLY pieces= triangles=`.
- **Fused pieces for props, settled:** research b508 found the consumer of the contact stream (the rigid body sim's
  "Contact Compiler" `82AE10C8` builds one row per record, "Iterations" `82AE27D0` solves them all, no merge; main
  checked `82DC2840` and the head of `82AE10C8`), and a recomp trace settled which sets a prop gets (2026-10-22,
  research b509 / b512 / b517, hook `DMOROWS`, run 4): the Stadium - Full construction barrier (one rigid body)
  resting on a fused piece got 4 to 6 ground-set rows per step, all against the static world, and no terrain-set row
  on any step. So retail solves fused pieces once for props, as ours does. The ground set's reduction
  `82779EB0` (at most 4 points per coplanar group, research b510) is ported: doc 27 "Ground contact reduction".
- **Moddability:** a piece's set follows its sidecar `caps.IgnoreFuser` (a mod clears or sets it per instance); the
  world is rebuilt with the rest at every map load.
- **Tests:** `props_rest_on_props_only_pieces` (a crate rests on a props-only plate, falls through without it),
  `ignore_fuser_pieces_stay_out_of_the_fused_set` (the props-only world holds exactly the flagged piece), asset-gated
  `ignore_fuser_counts_per_layout` (each layout with IgnoreFuser pieces gets a props-only world, the others none).
  Not play-tested.

## Unpaired fused edges are not convex: no catch at ramp toes (2026-10-22, USER-ASKS 235 / 237)

- **Problem (laptop session 2026-10-22 00:10, MegaPark full, the user):** in the Skate.Park maps the collision between
  placed objects and the ground is not quite right and stops or slows the player when transitioning. The trace
  (`audio_state.tsv`): three catches rolling from the map floor onto a piece's toe (`da_minirampkicker_University`,
  `ac_ramp_md_University`, `cd_pipeflatbank_lrg_University`): speed 8.5 to 4.1 / 6.4 / 6.7 m/s in two frames, all
  wheels off, the board lifted 4 to 6 cm, no bail.
- **Cause [code]:** a toe edge sits on the map floor, which is not in the fused set (retail's too: `827765C0` passes only
  the piece entries to the builder), so it has no partner. Ours left such edges at the default flags: convex, cosine 1.
  Our wheel contact takes edge contacts on a convex edge within about 8 degrees of the face normal; a non-convex edge
  with cosine 1 is rejected (`ground.rs` thresholds, research b487).
- **Retail [code, research b487, writer verified in the recomp]:** after pairing, `82AC2438` writes each edge
  byte with `82AC2390`: the convex bit 0x20 only for a score below 1.0 (`8231A844`), 0x1A above the concave limit,
  0x9A (unmatched) with no neighbor. An unpaired edge keeps score 1.0, so retail writes it unmatched, not convex.
- **Change:** `skate_world.rs` `fused_edge_features`: unpaired edges of the fused set get cosine 1 and no convex bit.
  Fused park path only; disc maps and props keep theirs.
- **Diagnostics (ask 237):** the log now has `SKATE_SPEED_DROP` / `SKATE_SPEED_GAIN actor=player from= to= over_ms= at=
  heading= wheels= surface= state=` for a loss of 2 m/s or more from 3 m/s or more inside 150 ms while riding (or a gain
  of 3 m/s more than 800 ms after a push), at most one every 500 ms (`game_audio/skate_events.rs` `SpeedWatch`). On the
  last two laptop traces it flags exactly the reported catches (MegaPark toes, Back Lot stairs and plaza pad seam,
  DownTown pad edge) and no pushes.
- **Tests:** `fused_park_pieces_pair_their_flush_seams` (a 5 cm gap stays unpaired: not convex, cosine 1),
  `speed_watch_logs_catches_not_pushes_or_slowdowns`. skate-game 887 passed. Not play-tested.
- **Play test (desktop session 2026-10-23 13:39, Skate.Park Stadium full, the user):** "The seams between the floor
  and the ramps are great! no noticeable slowdown when tranisitioning."
- **Open:** the concave rule above 1.8 (retail writes cosine about 1.0, ours keeps the pair's cosine); where retail
  sets vertex-disable bits (`82875200` / `82AC2BE8`, ours: the coplanar rule); which pool holds the map floor. The
  first two are answered in the next section.

## Fused edge bytes and vertex bits

2026-10-22, research b488. Verified every function and
constant below in Ghidra and the recomp asm.

- **Problem:** our fused park path stored each paired edge's full cosine n1 . n2 and set vertex-disable bits only for
  all-coplanar vertices. Retail writes quantized edge bytes and has a second vertex rule. So our wheels took edge
  contacts over about twice the angle retail allows on every park seam, and kept vertices retail disables.
- **Retail [code]:**
  - Angle code `82AC0140(score)`: x = score, or 2 - score above 1.0 (`8231A844` 1.0, `82060C50` 2.0).
    theta = acos(x), at least 6.6e-5 (`822F8EB0`), times 1/pi (`822F8610`). code = trunc(-2 log2(theta / pi))
    (`82094178` -2.0), below 0 -> 0, 27 or more -> 26. Decoded as a disc map's byte, 1 - pi^2 / (8 << code) is about
    cos(theta / 2) at the bin edge: about the half dihedral angle.
  - Edge byte `82AC2390`: the angle code; OR 0x20 (convex) for a score below 1.0; OR 0x1A for a score above
    2 + builder tolerance (-0.2, `8208EA7C`), which decodes to cosine 1 (sharp concave); OR 0x9A with no neighbor.
  - Vertex bits `82875200`: per vertex V with a kept face, T0 = its first kept face. V is disabled (`828749C0` ORs
    0x40 into V's corner byte of every kept face at V) when either test holds:
    - `828746A8`: every later face is coplanar with T0, abs(n . n_ref - 1) <= 0.01 (`820D71E8`).
    - `82874DC8` (with `82874CD0`): u1, u2 = norm(V - a), norm(V - b) for T0's other vertices (`82874B08`: next
      corner first). For each other vertex p of each later face, w = norm(V - p) and s = w . n_ref (T0's normal).
      With abs(s) > 0.05 (`82165A00`): s < 0.15 (`820994B4`) disables V. Otherwise (in plane): if -w lies in the wedge
      ((u1 + u2) . q >= 0 and q . u1, q . u2 >= u1 . u2 - 0.05, q = -w), V is disabled. Else the wedge widens: u2 = w
      when u2 . w > u1 . w and u1 . u2 > u1 . w, else u1 = w when u1 . u2 > u2 . w.
- **Change:** `skate_world.rs` `fused_edge_features` builds each kept triangle's three bytes (`fused_edge_byte`,
  `fused_angle_code`) and vertex bits (`fused_vertex_not_convex` plus the coplanar test), then decodes them with
  `decode_native_edges`, as the disc map path does. Retail's acos / log2 are vector estimates, so a score right on a
  bin edge may land one code apart: same outcome, not bit-identical.
- **Moddability:** `park_layout.rs` `FuseRules` (weld, concave_tolerance, coplanar, in_plane, below_plane,
  wedge_slack) hold retail's values as defaults. A layout sidecar overrides them with `fuse_weld` and a `fuse_rules`
  object. The values are logged in `SKATE_PARK_FUSE`.
- **Multiplayer:** pure functions of the positions and the triangle order, so every peer builds the same bytes.
- **Tests:** `fused_edge_bytes_follow_retail` (open edge 0x9A, 90 degree convex 0x22 with cosine 0.6916, concave 0x1B
  with cosine 1, 14 degrees -> code 7) and `fused_vertex_bits_follow_retail` (flat center and valley floor disabled,
  pyramid apex kept). Regression pass 2026-10-22: skate-game 892 passed; asset-gated `park_layout_seam_audit`,
  `park_layout_render_vs_collision`, `ignore_fuser_counts_per_layout` pass; `backlot_plaza_pad_seam_is_flat_after_fusing`
  now checks only that the seam stays flat (an open edge and a flat pair both decode to cosine 1, so its old "open"
  count no longer tells them apart); it passes (6 seam edges, 0 bent).
- **Open:** the sort order of retail's per-vertex face list (ours: triangle order); whether the welded path (props,
  mixed maps) should use the same bytes. Needs the regression pass and a MegaPark / Stadium session.

## Seam audit of every layout (2026-10-22, USER-ASKS 231 / 232)

Asset-gated report tests in `skate_world.rs` (run with `-- --ignored --nocapture`):

- `park_layout_render_vs_collision`: every placed piece of all 11 layouts (disc and DLC: backlot_full,
  Downtown_Nightpark_Fullpark, ZenPlaza_Full and the disc parks) has its collision inside its drawn mesh bounds (no
  template overhangs by more than 0.25 m). So no layout has collision without a drawn piece; the laptop session's two
  dead stops (Back Lot (-164.22, 0.09, 73.63), DownTown park (47.66, 0.09, -93.84)) are 0.45 m in front of a stair
  piece's 0.37 m riser and a bench / steep ramp toe. The user saw nothing there (2026-10-22: "nothing was visible"),
  so whether those pieces draw in game is the open question (a verify capture at the spot).
- `park_layout_seam_audit` after the back-to-back fix: unjoined floor edges lying beside another floor edge within
  1 cm (gaps the weld did not close, T-junctions): 0 in nine layouts, 8 in backlot_full (around (-225, 0, 26) and
  (-224, 0, 2)), 4 in ZenPlaza_Full (around (97, 2, -43)). Edges still shared by more than two triangles: 161 to
  3465 per layout (not yet classified: overlapping pieces that are not exactly back to back, or a piece's own
  non-manifold edges). Joined convex ridges over 3 degrees between near-flat floors: 16 to 1337 per layout (many can
  be real geometry, such as mellow ramp lips).
- **Edges with 3 or more users [code], research b547 (2026-10-23):** retail has no special case. Every pass works per
  triangle and edge; candidates are the live opposite-wound users; the pairing (82875B20 / 82875CA8) is greedy best
  score in the sorted-table order (a tie keeps the old pair, a displaced triangle is re-paired only by a later one);
  leftovers get 0x9A (cosine 1, not convex: face contacts only, nothing to catch on); only the exact back-to-back cull
  (828755B8) removes faces and nothing welds edges or splits T-junctions. Ours matches (`skate_world.rs` cull, pairing
  and `fused_edge_byte`, verified the thresholds and the 0x9A path). So these edges are not faults in themselves:
  the audit now counts those where a user carries a convex code over 3 degrees, the only ones that can catch. Report of
  2026-10-23 (multi-user edges, of them convex over 3 degrees): Downtown_Fullpark 936 / 156, Downtown_Nightpark 3465 /
  734, Industrial 807 / 147, MegaPark 1078 / 317, Starter 161 / 37, ZenPlaza_Full 1494 / 255, backlot_full 1811 / 515,
  startpark_progression 617 / 86 (stable across runs). Whether retail's runtime gives those the same codes would need
  a recomp trace at one of them (as the toe trace, doc 39 "Ground plane pass").
- Play test 2026-10-21 23:07 (laptop, build db1cd1e5): "stadium full seams are WAY better" (one bail in about 70 s
  against 4 in 25 s on the big ramp before) and "backlot collision is way better".

## Fused vertex order

2026-10-22, #52 item 12 (USER-ASKS 251), research b496 / b497 / b498. Verified 82AC21D8,
82AC4A40 / 82AC52D0 / 82AC5418 / 82AC6660 / 82AC6BD8 (the sort), 82AC0CE0 (merge test) and 8277E650 (unit iterator)
in Ghidra and the recomp asm.

- **Problem:** retail's fuser walks each vertex's triangles in the order of a sorted table; ours walked them in
  triangle order. On edges more than two triangles share (1078 on MegaPark) the pairing, the back-to-back cull and the
  vertex bits' reference face T0 can then pick differently.
- **Retail [code]:**
  - 82872D70 copies each fused piece (layout record order, IgnoreFuser pieces left out) into the builder: per
    cluster all its vertices, then ONE triangle per unit from the unit's first three index bytes. A quad unit gives
    only corners 0-1-2; its 3-2-1 half never enters the fused set. The iterator 8277E650 steps a whole unit.
  - Vertex ids are raw builder indices (piece, cluster, the cluster's vertex order). 82AC10E8 merges vertices whose
    squared distance is strictly below the weld squared; a merged vertex keeps the smallest raw index.
  - 82AC21D8 writes (vertex, triangle) for every builder triangle's three corners, collapsed ones included, and sorts
    the table with EASTL (EA's standard template library) quick_sort on the vertex only (unstable): introsort to 28 entries, depth 2 floor(log2 n),
    median of first / middle / last, two-sided partition, then insertion sort; heap sort when the depth runs out.
- **Change:**
  - Setup (`dynamic_props.py` `decode_with_units`) records per collision triangle its unit `[type, half]` and each
    corner's `[mesh, cluster, index]` (`units`, `vertex_keys` in the template collision), by watching the vendored
    decoder's unit triangulation; the vendored file is not edited.
  - Engine (`skate_world.rs`): `FuseTag` per triangle; `fused_vertex_ranks` gives each merged vertex the rank of its
    smallest (piece, mesh, cluster, index) key; `eastl_quick_sort` sorts the table as retail; quad second halves leave
    the fused set (`FuseRules.quad_first_half_only`, default true, sidecar `fuse_rules.quad_first_half_only`). A sidecar
    without the new keys keeps the old triangle order.
- **Kept as ours:** our merge forms the groups (first-seen within the weld; retail's grid hash differs only on chains
  where A-B and B-C are within the weld but A-C is not); zero-area template triangles are skipped before the fuse
  (retail keeps them in the table, then drops them).
- **Quad check (user 2026-10-22: check first):** `park_layout_quad_second_halves` after the maps refresh: 0 quad
  second halves in all 11 layouts (about 1.02 million fused triangles); every park template unit is type 1 (MegaPark:
  31,586 units, all type 1). The earlier "up to 351 quad-looking pairs" were two triangle units. So the rule changes
  nothing for retail content; it stays for mod content with quads.
- **Tests:** `eastl_quick_sort_orders_by_key_and_keeps_every_entry`, `fused_vertex_ranks_follow_the_smallest_raw_key`,
  `fused_set_leaves_out_a_quads_second_half`, `fused_set_drops_a_zero_area_triangle`; Python
  `test_dynamic_props` unit record.

## Ground plane pass

2026-10-22, laptop session (USER-ASKS 252: "Transitioning from the floor onto the placed ramps is still not right and
you lose speed and/or get bumped up in the air sometimes"), research b502 / b503 / b505, recomp traces.

- **Problem:** at every Stadium - Full floor-to-ramp drop (b502) the lead wheels reach a quarterpipe toe. The toe edge
  pairs with the piece's own underside, a convex knife (code 0, cosine -0.2337), so a wheel up to about 1 m before the
  toe gets a predicted edge contact whose normal leans back toward it and takes several m/s (b503).
- **Retail [trace]:** hook WHLC (recomp `src/research/hooks_wheelc.cpp`) on a scripted roll into the toe at x 223.53
  (recomp trace "toe", 2327 lines, 0 malformed): the same toe edge is NOT convex with cosine 0.9993976
  (code 11) and its edge contacts are rejected; only face contacts are taken. Table research b505.
- **Retail [code, b505]:** the fuser finalize 82876188 runs 82874248 between the pairing and the byte writer. For each
  ground plane (normal, d, optional box; at most 6) and each live triangle facing into it (normal component difference
  at most job +8) with v1 on it (|n . v1 - d| at most job +4), every paired edge with a score below job +12 calls
  82874070: the partner's score becomes the edge score of (plane normal, partner normal, edge) and the triangle adds
  it to its own. 827765C0 gives a park with no planes one: normal (0, 1, 0), d 0, no box.
- **Retail values [trace, hook FUSEPL at 82874248 on Stadium - Full]:** job +4 = 0.05 (distance), +8 = 0.01 (normal),
  +12 = 0.1 (score limit); one plane, normal (0, 1, 0), d 0, no box.
- **Change:** `skate_world.rs` `fused_edge_features` runs the pass after the pairing; `park_layout.rs` `FuseRules`
  carries `ground_planes` (default the y = 0 plane), `plane_distance` / `plane_normal` / `plane_score_limit`
  (0.05 / 0.01 / 0.1). Sidecar `fuse_rules`: `ground_planes` (list of {normal, d, min, max}, at most 6; an empty list
  turns the pass off) and the three tolerances, so a mod layout on a raised deck sets its own plane.
- **Result (asset probe `park_layout_fused_probe` at 223.53 / -88.359):** toe face edge 0 cosine 0.99939764, not
  convex; underside edge 2 cosine 0.6915748, convex: both as retail. Vertex bits match retail on every toe triangle.
  Still different (last bit, b506): the coplanar quad diagonal scores one ulp under 1.0 (code 25, convex, cosine
  0.99999994; retail code 26, not convex): its two faces' vertex positions differ from retail's in the last bits (our
  piece placement arithmetic). Builder normals use retail's formula (`fused_normal`, 82875CA8). A convex edge with
  that cosine accepts only contact normals within about 0.03 deg of the face, so it changes nothing in play.
- **Test:** `a_knife_toe_on_the_ground_plane_is_not_convex`.

## Mesh floor pass

2026-10-22, research b507 (research b507; verified the constant 0.005 at 82099920).

- **Retail [code]:** the fuser finalize 82876188 runs 82873EA0 after the ground plane pass and before the edge bytes:
  the same idea for pieces that stand on another piece's flat top above the ground. Up-facing flat triangles
  (|1 - n.y| at most job +8, 0.01) are the floors. Each down-facing triangle (|-1 - n.y| at most 0.01) whose v0 is off
  the ground (|v0.y| above job +4, 0.05) takes each paired edge with a score below -job +12 (-0.1, a sharp convex
  edge) to 82874070 with normal (0, 1, 0) when a flat triangle of a DIFFERENT piece has its v0 within job +0x10
  (0.005) of the edge start in height and overlaps the edge in xz (82873270, four separating axes, touching counts).
  The grid it builds (16^3 cells) only speeds up the search. It is the only score-changing step of 82876188 we lacked
  (b507 lists the full call order).
- **Change:** `skate_world.rs` `fused_edge_features` runs it after the ground plane pass (`xz_segment_meets_triangle`;
  floors sorted by height and searched in the 0.005 band instead of retail's grid, same result). `FuseRules`:
  `mesh_floor_pass` (on), `mesh_floor_gap` (0.005), `mesh_floor_up` (0, 1, 0); it reuses the plane tolerances as
  retail reads the same job fields. Sidecar `fuse_rules.mesh_floor_pass` / `mesh_floor_gap`.
- **Effect:** b507's estimate for MegaPark_Fullpark: about 206 underside edges on 19 pieces (Mega quarter pipe, lower
  and down ramps, roll-in, University quarter pipes and steep ramps), upper bound before the cull.
- **Test:** `a_knife_toe_on_a_raised_deck_is_not_convex` (and unchanged with a gap over 0.005, on the same piece, or
  with the pass off).

## Code reference: park_layout.rs

- A world row (`dist_megapark_full`) names a `.park` layout. Setup (`tools/asset_pipeline/park_layout.py`) writes each
  layout as a render-only MOBJ package `private/park-layouts/<layout>.skate`, its sidecar `<layout>.json` (instances,
  per-template collision and rails, the movable pieces) and `worlds.json` {world row: {layout, stream}}.
- Retail keeps every static piece as its own object: it collides through a per-piece static set (b431 section 2) and
  grinds on its template's splines posed once by the piece matrix (82868860 -> 82C1F098, b431 section 1); both are
  rebuilt when a layout is applied.
- Ours: the map load carries the chosen layout (an in-memory `EXTENSION_TAG` extension on the loaded map, never written
  to a file); physics appends the posed piece triangles to the static world as one query range per piece and the posed
  rails to the static rail provider as one asset per piece; the renderer spawns the pieces as `ParkPiece` entities.
  A layout change is a map load (retail re-applies a layout in place; the outcome is the same).
- Moddable: a mod ships its own `<layout>.skate` / `.json` and `worlds.json` row; the world row is part of the
  serializable map request (`map_library::Entry::world`).

## Code reference: skate_world.rs

- Backdrop: the district's global presentation model (`private/native-backdrops/<map>.skate`: ocean surfaces, distant
  tree walls, far sea planes) draws through the district's retail material path; every draw carries `Backdrop`, so its
  visibility follows `BackdropSettings` (mod-reachable). No lights, no collision.
- Dynamic props (DMO instances): ported from SK8-ENGINE PR #15 (laaledesiempre, phase 0). Each MOBJ record is one root
  entity with its own transform; its template geometry is merged per (render class, slab) in template space and shared
  between instances.
- `prop_files`: a map with a skate.Park layout that has movable pieces uses `park-layouts/<layout>.props.skate`
  (research b435: ordinary DMOs), else the district's `native-props/<map>.skate`. No park stream has district props,
  so the layout's package stands in for them; a stream with both would need the two merged.
- Fused park set (`append_pieces` / `split_unfused`): retail fuses the static pieces into one mesh and rebuilds its edge
  codes (b478, fuserJob 828728B8 / 82876188). Pieces with flag 2 (IgnoreFuser, b482: hedges, bushes, shrubs) stay out
  of the fuse (827765C0); their own collision lives only in the TerrainPool (b483), which the skater's ground query
  never reads (island +0x5590 = the GroundPool, 827632A8 / 8275DCC8, b484). A mod that wants one solid clears its
  `IgnoreFuser` cap in its layout sidecar. Props collide with them through `props_only_world` (see "Props and IgnoreFuser pieces").
- `welded_edge_features`: 1 mm welding and edge pairing shared by the portable map world and props created mid-game;
  `reconstruct = false` keeps the default features (fully native maps decode theirs per triangle).
- `fused_edge_features` (b478): vertices merge within the weld; triangles the merge collapses or flattens are dropped
  (82AC2010); faces of different pieces lying back to back (abutting side walls under their tops) are removed in
  pairs or as two quads (828755B8 with 82875438); each edge pairs with the neighbor of the highest score, a better pair
  displacing a worse one (82875B20 / 82875CA8); then the edge bytes and vertex bits of the section above.
- `portable_parts_fused`: `fuse` merges vertices across the whole set and rebuilds every edge code, ignoring the native
  ones (surfaces kept), as retail's 82872D70 copies only the corners, piece and surface of each triangle; collapsed
  triangles are dropped and the split ranges follow the kept triangles.
- `add_instance` (mid-game prop, e.g. a released hand prop or a mod's): template-space triangles with scale folded in,
  baked like `rebake`; edge features from the instance's own triangles (load-time instances also pair edges with
  touching neighbors); a retired slot with the same triangle count is reused, so repeated spawns do not grow the
  world.
- DMO template collision (`native-props/<map>.json` `collision`, setup `dynamic_props.template_collision`): the
  template's ClusteredMesh triangles in model space with their RW edge codes and surface ids (research
  dmo-physics-collision.md), keyed by template id so a mod can supply or replace one.
- DMO type data: setup maps each template id to its type's vault record (`types` in `native-props/<map>.json`, from
  the template's EB000D +120); the values come from the installation's stock vault
  (`livingworld_dynamicobject_characteristics`, `prop_dynamics::dmo_type_blocks`). Without a type map (older setup)
  the props keep their authored material until setup group maps runs again.
- `load_dmo_grab_splines`: the authored grab splines per DMO template id (`grab_splines`, doc 26i "Move Object step 1").
- `load_prop_layer`: the prop package is a presentation supplement; missing or invalid files leave props uncollidable
  instead of failing the map, as the render path does.
- Tests: the asset-gated prop type test checks every type in a set-up map resolves to the stock vault's record (default
  record: restitution 0.5, free pair 0.8 / 0.6; `SKATE3_ASSET_ROOT`, `SKATE3_PROP_MAP`, default DownTown). The seam
  audit (`park_layout_seam_audit`) fuses each layout and reports what can still catch a wheel on a floor (normal y >
  0.7): edges more than two live triangles share, unjoined floor edges with another within 1 cm, and joined edges
  between near-flat floors (y > 0.95) with a decoded cosine below 0.99863 (code 9 or less, normals more than about 5.6
  degrees apart; b496). The two-pads test models Back Lot's aa_PlazaPad seams: each pad's side wall lies back to back
  on the other's, 828755B8 removes them, and the tops pair flat.
