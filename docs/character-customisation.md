# Character customisation: extraction boundary and native blockers

Status: **not a playable customiser**. The runtime, Escape menu, character
loader, physics defaults, and renderer are unchanged by this work. No dummy
controls or replacement models were added. A faithful implementation is
blocked on the CAC material/compositor contract and the remaining native
assembly/profile application described below.

## Implemented, private-data-only tooling

`tools/asset_pipeline/customisation_catalog.py` reads the user's existing
`createacharacter.big` and `db.big` directly. It indexes the authored XML,
exports CAC BIN/VLT data with complete arrays, and optionally extracts only
explicitly selected models and their texture dependencies. It does not require
an ISO dialog, map conversion, downloads, a running game, or Blender.

```powershell
python -m tools.asset_pipeline.customisation_catalog `
  --game-root <owned-extracted-game> --output <private-task-staging>
```

Outputs are private `catalog.json`, `native.json`, `extraction-report.json`,
the source XML, and five small database inputs. Do not commit, bundle, or
publish these files. The output directory must be separate from the owned
source. Existing staged resources are reused only when their bytes match;
different content is never overwritten. Publication of a resource is atomic.

`--selection <private.json>` accepts an array like the following, with IDs
copied from that user's catalog rather than invented:

```text
[{"slot": <component slot>, "asset_id": <model variant ID>, "lod": 0,
  "material_ids": [<one material ID for each ordered material instance>]}]
```

This validates **extraction membership**, not outfit compatibility. A successful
extraction must not be presented as a renderable or gameplay-ready character.
The report explicitly sets `runtime_ready` to false. The existing installer
still prepares only the fallback skater; this tool is an opt-in preparation
boundary until the runtime dependencies are implemented.

The catalog keeps original shader names, every shader parameter and texture
channel in order, authoring flags including empty values, model names and
IDs, both LODs, and ordered material instances. Unknown scalar/sampler/UV
attributes are retained in `authored`; missing runtime constants are never
filled with guessed values. The original XML bytes are also preserved.

The native export preserves reflected fields and inheritance links, including
complete `MorphPreset`/`ColourPreset` arrays and relocated text-array strings.
It resolves class/collection names using the archive's own summary report;
it does not require an IDA dump or a personal hash-name cache. Its `morphs`
index preserves target order and UI-zone index separately. The two native
gender-branch parameter triples remain labelled A/B pending verification of
the caller's boolean convention; both triples match in the inspected bank.

## Owned-data validation

The inspected archive contains **19 component slots, 481 model variants,
962 LOD records and 3,328 material definitions**. Every referenced model and
texture exists in the same archive. These are authored counts, not counts of
unlocked or compatible menu choices.

The female Rostral has two material instances at each LOD. The extractor
requires both; it does not silently keep the first. The high-LOD mesh has
1,432 vertices and 7,362 indices. How the two native material loops contribute
to that mesh remains part of the compositor investigation.

The ten fallback components resolve to the exact model IDs, arena IDs,
material IDs, texture bindings and resource SHA-256s in
`tools/default_skater_retail_manifest.json`. Forty-four resources including
the catalog XML were staged; a second run reused all forty-four unchanged.
A separate female-head request staged both material-loop dependency sets.

The native export contains 267 CAC-related collections, 19 morph mappings,
and 21 complete morph-preset arrays with 19 elements each (the schema default
plus ten male and ten female presets). This is raw authored data; parent-row
inheritance and preset application still need the native consumer.

Synthetic verification, requiring no game content:

```text
python -m unittest tools.asset_pipeline.test_customisation_catalog tools.asset_pipeline.test_vlt
```

Thirteen tests cover VLT array bounds/alignment/text relocation, material-loop
ordering, broken references, missing resources, staging isolation, no-clobber
reuse, separate UI/target indices and malformed morph ranges. No game,
recomp, screenshot process, or `--check-assets` run was launched.

## Native evidence

Addresses below are virtual addresses in the inspected image whose dump base
is `0x82000000`; subtract that base for a dump-relative offset. Symbol names
from the prior Ghidra export are leads, corroborated with instructions and
authored data. The original IDA database was opened read-only; symbol import
and exploratory disassembly used a private copy.

Source identities:

- Owned `default.xex` SHA-256:
  `1db39496585c521d17a2137804f42cf73ebed2b32cac166ec42dbf772f4dcf7f`.
- TU3 raw image SHA-256:
  `f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4`.
- CAC XML SHA-256:
  `be2f23ab6d2f29b5ac5e143133788e72e1bebd073adedb343effd1fd7e94f4f1`.
- `skatercollections.vlt` SHA-256:
  `3b7dbd062bb1c906a085514355afff35cfa22f486ae70820c5ad1a42a7aab25b`.
- The character archive identity is recorded in the existing fallback manifest.

Observed native behaviors:

| Consumer | Evidence and implication |
| --- | --- |
| `FrontEndState_CaC::ReceiveInput`, `0x825FC7E8` | Cases 47/48 adjust recipe fields `+0x2004/+0x2008` by signed `0.1`, clamped to `[0,1]`. Constants: `0x820641A8=0.1`, `0x82165A10=0`, `0x8231A844=1`, `0x8216DEE0=-1`. These are truck tightness/wheel hardness preferences, distinct from merchandise IDs. |
| `GetIntegerValue`, `0x825FFB18` | Cases 47/48 read those same floats and convert to display ticks with the scale at `0x821963E4=10` and rounding offset `0x8209975C=0.5`. |
| `CASMorphParams` initialization, `0x8253CA60` | Uses the `cac_morph_params` layout's target order at `+4`, target prefix at `+0`, and parameter triples at `+12/+16/+20` or `+24/+28/+32`. Each target's working record is 28 bytes. |
| Morph normalization, `0x8253CD18` | Clamps to that target's minimum/maximum then computes `(value-min)/(max-min)`. The UI's normalized value is not the mesh's direct delta weight. |
| `GetTargetOrder`, `0x8253CF50` | Matches target-name prefixes against 19 entries; clothing `fat_*`/`thin_*` must follow the corresponding native mapping. |
| `CASPlayerInitializer::InitializePlayer`, `0x8253D470` | Copies each working record's default at `+8` into the 19-value customisation container; chooses default parts through `CACPartDB`. |
| `CASAssembler::Correct`, `0x8253F648` | Coordinates hair, rostral, torso, organ, legs/socks, other accessories, removals, colours and stamp information. XML flags are inputs to this sequence, not sufficient proof of a replacement assembly algorithm. |
| `CorrectRemovals`, `0x8253FD40` | Walks the native slots, resolves component names in comma-separated removal strings, and clears the referenced slots. |
| `CorrectRostral`, `0x825406E0`; `CorrectTorso`, `0x82540828` | Further select material/part combinations from native colour and clothing relationships. |
| `GetCACSettings`, `0x82590B50` | Reads saved stance, four gestures and posture-related profile fields. Existing Rust posture consumers already preserve the pending-tree application semantics. |

Observed `cac_morph_params` values in this bank are `min=0`, `max=0.5` for
all 19 targets. Face defaults are `0.25`; fat and thin defaults are zero.
Target order is **fat=0, thin=1**. The existing fallback parser's descriptive
`skinniness/fatness` names must not be used to infer that ordering merely
because both default values happen to be zero. Nose height/length have swapped
target-order and UI-zone indices; the export retains both.

Do not equate low-level constructors with gameplay defaults.
`RecipeData` constructor `0x82DDE700` initializes equipment values to zero,
while the current Rust actor profile uses `0.7`, following its documented
actor settings path. That current behavior is unchanged here. Saved/default
CAC profile initialization must be traced before changing it.

## Remaining implementation gates

1. **CAC material composition.** The current character converter exports
   `StandardMaterial`; its fallback image preparation explicitly skips
   `decal/decal2` because they are shader inputs, not unconditional overlays.
   It handles diffuse, hair alpha and normal reconstruction for that fallback.
   The native catalog includes skin/face stamps, cloth stamps, colour shifts,
   alpha and multi-instance cases. Recover the native compositor's input
   constants, masks, UV transforms, stamp ordering and tint math, then connect
   them to the renderer's character-material adapter. The renderer task
   confirmed this dependency is not implemented. Do not multiply arbitrary
   colours or paste tattoo textures into diffuse images as a substitute.
2. **Assembly and rigs.** Finish the native correction sequence and validate
   both genders against authored component skinning/bind matrices. Preserve
   hair-under-hat variants, arm/leg substitutions, socks, layered inner/outer
   tops, removed necklaces/wrist items and multiple material instances. The
   existing GLB pipeline accepts only the fixed fallback component manifest;
   extend it after the assembly contract is established.
3. **Complete profile/menu semantics.** Apply parent-row inheritance and
   native face presets, colour zones, available-item/unlock filters, tattoos,
   gestures, stance, style and posture. Keep physical board adjustments
   separate. Use actual saved defaults rather than constructor placeholders.
4. **Runtime/UI.** Implement the requested hierarchy, left controls and right
   preview only for working native paths. Add transactional preview/apply/back,
   coherent persistence, controller navigation and in-process character
   replacement. No runtime or menu integration is present in this commit.

The requested hierarchy is recorded verbatim in `HIERARCHY`. It is a scope
contract, not a declaration that those pages work. The map-switch task's
proposed interface leaves a separate customiser resource and character
entities alive, emits `WorldChanged` after `MapTransitionSet`, and reconstructs
the skater using shared immutable animation data. Reapply saved profile values
after that transition without moving selection ownership into transient
physics state. This interface still needs integration against its final commit.
