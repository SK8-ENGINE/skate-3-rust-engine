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
  Session publication at `82DA37B0` captures the multiplier before crediting
  this reward to the combo timer. Settlement at `82DA3B38` preserves repetition
  on an empty line timer while a collector is active.
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
  The data-only audit initializes the original class, builds the authored
  initial child hierarchy and checks the constructor's four trick-name
  visibility writes. Native method calls still use a dummy host, so this does
  not validate a functioning display.
* `apt_display`: retained depth-list placement flags and transforms. The
  data-only audit validates all controls across 38 original movie timelines.
  Unsupported clip actions and filters fail explicitly.

## Extractor provenance

`tools/vendor/skate3_ui` reuses the Python extractor from the local Custom
Engine Layer preview.18 source distribution. Its accompanying project MIT
license is preserved. `actions.py` is the new compact instruction decoder,
based on TU3 `_parseStream` at `82E67868` and dispatch table `82FC9BF0`.
Extracted game assets are not covered by that source license and must stay in
the ignored private output directory.

Example preparation (data extraction only):

```powershell
python tools/prepare_hud.py --game <owned-game-directory> --output assets/private/hud --collections <owned-assets>/private/stock/skater-collections.json
```

The original movie is authored at 1280 by 720. Its score, line, stance,
multiplier and trick-name timelines must control presentation; replacing them
with a new overlay would not meet this port's requirements.

## Production integration and validation

The production fixed tick now supplies animation descriptors, conditioned category,
grind IDs, landing output and physical motion to `scoring_runtime`. The runtime
loads native VLT points, repetition, announcement and collector curves, maintains
carriers and continuous distance rewards, and publishes sequence/line accounting.
It is a usable integration for testing, **not a finished native-parity port**.

The HUD executes the owned trickdisplay ActionScript and timelines, including
score, multiplier, line meter, stance and trick-name updates. It renders original
shape atlases and font glyphs through a separate 1280x720 camera with inherited
multiplicative/additive colors. Movie objects are collected and mesh/material
slots reused. Fixed updates pause with gameplay; map-generation changes recreate
the movie even while paused. Missing assets or unsupported actions are logged.

Font resolution follows FontManager initialization `82808AE8`, APT-name lookup
`82809208` and loading `82809308`. VLT class `FECFBCAF356518C4` maps AptName to
FileName; `FuturaOuterGlow` resolves to `futurashadow`. Glyph advance and placement
use native scale/ascent metrics. English text resolves through the owned language
asset. No replacement interface or bundled game assets are introduced.

Validation on 2026-09-08:

- All nine scoring unit tests pass, covering clock wrap, early completion,
  conversion, cancellation, repetition, timer thresholds and one-time banking.
- The data-only scoring-flow example credits the authored 100-point stationary
  kickflip once and verifies idle stability and teleport cancellation.
- The original HUD data-only audit passes 1,800 frames, with 172 VM slots,
  116 display instances, at most 33 draw batches and finite geometry.
- The release target builds successfully with static MSVC CRT, without default
  features. Compiler warnings remain in the existing game and audit-only code.
- No game, recomp, controller harness, gameplay automation, `--check-assets`
  or screenshot capture was run. GPU rendering and interactive behavior remain
  unverified until the user launches the build.

The copied build and launcher are in ignored `logs/scoring/build`. The launcher
uses this worktree's HUD cache and the existing owned asset installation and
starts paused. It checks required paths before running and retains failures on
screen. The executable SHA256 is
`9b3458045e7078dcd6daca3e5bea89d16298315b751b4cac4f82f8a6531cd7c0`.

The missing-HUD startup defect is fixed: HUD setup now depends on presentation
setup, so Bevy applies the deferred camera spawn before the HUD queries it.
Previously the two PostStartup systems were unordered and a missing camera
caused HUD initialization to return permanently. The rebuilt launcher also
saves runtime output to `logs/scoring/build/scoring-test.log`. This fix is
release-compiled; interactive rendering has not been launched for verification.

## Landing lifetime and native font passes

The landing hide bug came from sending `CloseTrickDisplay` at every score
publication. Native `82775328` calls `82774E88` only on scoring output byte
14630; `82DA4238` copies that from module byte128, selected by `82DA4010`
for its reset/bail category. `82774E88` sets backend byte184, which
`825E4F40` turns into event3, resolved by `825D3B38` to
`_global.TrickDisplay.CloseTrickDisplay`. Normal banking no longer emits it.
Line expiry clears the score and lets the authored Clear/outro actions run.

The timer unit is now traced end-to-end: `82DA4238` writes line points divided
by drain rate to output+40; `82775328` copies it to backend+32;
`825C2910` and `825C2D90` truncate that value to seconds for ActionScript.
The previous raw-point binding was wrong. Changes in that seconds field now
also refresh `UpdateTrickScoring`.

Observed font code: `825D6B68` compares the APT name with `Futura Shadow`
(string at `8220BD44`) and attaches `futuraheavy` (`8220BD54`) as a secondary
font, setting X adjustment 1 and the other adjustment 0. `82CA1FD8` first
sets the primary RGB multiplier to black while retaining alpha, draws it,
then restores the color and draws the foreground with the X adjustment.
The scene renderer now follows those two passes. FuturaOuterGlow remains
its separately authored tinted atlas pass. No outline radius, new glow
color or replacement glyphs were invented.

The offscreen adapter also needed premultiplied-alpha compositing. The
mesh target already stores covered RGB; Bevy's ordinary ImageNode uses
straight-alpha blending, which multiplied coverage again and weakened
soft atlas edges. The dedicated HUD composite now uses premultiplied
blending to preserve the original layers' coverage.

Validation: supplied scoring data exercises landing, retained HUD visibility,
seconds conversion, natural line expiry and teleport cancellation. The
1,800-frame original movie audit checks a black shadow/foreground pair and
finite geometry (33 maximum batches). Release compilation passes. User
screenshots guided the investigation but did not supply rendering constants;
no game, reference executable or GPU capture was launched for validation.

## Native parity gaps

These are implementation gaps, not merely missing gameplay validation:

- Collector activation gates and exact publication timing need further porting;
  the current runtime uses conditioned categories and an idle countdown.
- Air spin uses accumulated board heading rather than the complete native
  transform accumulator. Body-flip direction and some landing modifiers remain
  incomplete.
- Gap/context collectors and their native ground-query inputs, contextual
  bonuses and off-board height rewards are not wired.
- Revert recognition depends on an unpublished physical state flag in the
  current host. Full native revert scoring is not yet available.
- Trick-name spin/direction modifiers and the HUD manager's stance transitions
  are incomplete. The bindings expose five metric slots but currently only
  supply the base trick label and default modifier flags.
- The compact VM supports the exercised movie paths, not arbitrary APT programs.
  Superclass/native constructor behavior is limited, and Math.random uses a
  local presentation RNG rather than the original engine RNG stream.

Do not describe this build as fully finished or an exact native recreation.
