# Native scoring and HUD port

Work branch: `lol/native-scoring-hud`. This document distinguishes implemented
components from remaining integration; it is not a claim of gameplay parity.

## Evidence

The source executable is the locally owned TU3 image loaded at `0x82000000`,
size `0x011B0000`, SHA-256
`f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4`.
Static analysis uses a disposable IDA database and the existing generated PPC
translation. Original binaries, analysis databases and extracted assets are
not part of this repository. IDA incorrectly decodes some VMX instructions;
PPC instructions and the generated translation must corroborate those paths.

Implemented source components:

* `skate-core::scoring`: ScoreHolder accounting at `82DA6198`, `82DA6260`,
  `82DA6408`, `82DA6468`, `82DA6538`; point timers at `82DA4C28`; carrier
  announcement/completion at `82DA45C8`, `82DA46D0`, `82DA5DE0`, `82DA5F98`.
* `skate-data::scoring`: resolves authored VLT points, labels, delay, repetition,
  announcement curves, combo thresholds and timer settings. Missing data is
  an error. The executable's 332-entry identifier metadata includes unused
  entries; the owned installed database resolves 300 records.
* The scoring display/timer defaults belong to class `349215E2E817703C`.
  They are global tuning, not user difficulty settings. Physical collector
  tuning is a separate class `546C36B656038E04`.
* `tools/prepare_hud.py`: decodes the original `hud2/trickdisplay2` APT,
  geometry, font metadata, textures and compact action stream into a private
  cache. Source asset hashes are retained in the compiled manifest.
* `apt_vm`: partial bounded interpreter for the original compact instructions.
  The data-only audit initializes the original class and runs its constructor
  against a dummy host. This does not validate a functioning display.

## Extractor provenance

`tools/vendor/skate3_ui` reuses the Python extractor from the local Custom
Engine Layer preview.18 source distribution. Its accompanying project MIT
license is preserved. `actions.py` is the new compact instruction decoder,
based on TU3 `_parseStream` at `82E67868` and dispatch table `82FC9BF0`.
Extracted game assets are not covered by that source license and must stay in
the ignored private output directory.

Example preparation (data extraction only):

```powershell
python tools/prepare_hud.py --game <owned-game-directory> --output assets/private/hud
```

The original movie is authored at 1280 by 720. Its score, line, stance,
multiplier and trick-name timelines must control presentation; replacing them
with a new overlay would not meet this port's requirements.

## Remaining implementation

Collectors are not wired into the production tick yet. Recognition still
requires conditioner gates, descriptor conversion, air/ground/grind collector
transitions, spin/flip bonuses, gaps, landing and sequence publication ordering.
The native clock frequency must be recovered before converting authored
announcement delays to ticks. A physics-frame count is not a safe substitute.

The HUD still needs a complete movie object hierarchy, native bindings,
timeline execution and rendering with multiplicative and additive colors.
Function scope/preloads and supported action coverage also remain incomplete.
The audit's dummy host intentionally does not establish native binding parity.

The movie references `FuturaOuterGlow`, absent under that exact name from the
five extracted bitmap-font banks. The old extractor infers an alias to
`Futura Glow` from matching menu metrics; that is not yet evidence of native
font-server resolution. The manifest reports it as unresolved rather than
silently selecting a substitute.

Pause, map-change generation reset, resource reuse and the final task-specific
release build/launcher remain to be integrated and checked. No game/recomp,
controller harness, gameplay automation, `--check-assets`, or screenshot
capture has been run for this work.
