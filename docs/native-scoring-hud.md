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
shape atlases and font glyphs through a separate camera with inherited
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
`59c7c876d289c58e9661f669274fa4faa4e50a7bf51085461bcf33f2fc1bd809`.
The launcher now selects `skate3rust-hud-animation.exe`.

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

## Output resolution and authored blue glow

The HUD target now uses the window's physical pixel dimensions and resizes with
the window. Its orthographic projection retains the authored 1280x720 coordinate
space. Previously the adapter rasterized at 720p and enlarged the finished image,
adding blur on larger windows. This change improves rasterization without
inventing higher-resolution source artwork.

A data-only inventory of `fedata.big`, `fedynamic.big`, `fetexture.big` and
`miscboot.big` found byte-identical duplicate trickdisplay APT/CONST/GEO/RX2
assets and 512x512 Futura font atlases. No higher-resolution variant was found
in those banks. The inventory and hashes are retained in ignored
`logs/scoring/hud-resolution-inventory.json`.

`GetGeneralInfo` had clean and sketchy reversed. The original
`UpdateTrickScoring` action stream reads slot2 as sketchy (0199D2) and slot3
as clean (019A30), matching backend bytes152/153 returned by `825C2D90`.
Correcting those slots selects the authored clean color animation. The native
FontPS/Font8PS programs contain texture/color modulation; the glow is supplied
by the authored layers, not a replacement procedural effect.

Additional collector fixes follow `82DA8A70`/`82DA93D8`: published Air452
suspends continuous air metrics and grab accrual while preserving the carrier.
Distance collectors also start their first active frame at zero elapsed
time/distance, following `82DB0588`.

Validation: the 1,800-frame supplied-data HUD audit asserts that clean input
selects mLastClean rather than mLastSketchy; the scoring-flow audit verifies
Air452 suspension in addition to landing lifetime, expiry and cancellation.
Both pass, and the static-CRT release build succeeds. The game was not launched;
pixel appearance and GPU resize behavior still require interactive verification.

The first window-resolution build introduced a render-target invalidation
regression: Bevy 0.18.1 `Assets::get_mut` queues a Modified event even for a
read-only inspection. The size check therefore recreated the GPU image each
frame while the UI material retained its earlier texture binding. The check
now uses immutable access; a real resize refreshes the compositor material,
including a following-frame refresh to account for independent image/material
preparation order. An asset-only regression test checks that an unchanged
target emits no Modified event and a resize invalidates both image and material.
The test uses no window, renderer or gameplay systems.

## Multiplier timeline playback

The host previously called `UpdateLineDisplay` every fixed tick. The shipped
action at 019C39 computes `floor(GetLineTimeRemaining() * 60) - 30`, clamps
it, and seeks `multiTimer_mc` with `gotoAndPlay` at 019CF4. Since the native
binding returns whole seconds, repeated calls pin the 501-frame timer to the
same frame for a second, then jump it forward. The ActionScript dispatches
this method for event11 (0196CC) and ScreenShow, rather than onEnterFrame.
The adapter now dispatches when the exposed timer sample or line score changes,
allowing the authored timeline to advance between samples. This is an adapter
event policy; the entire native HUD event producer is not yet ported.

Original x3 effect clips are retained: character59 changes mBlueGlow alpha
with `80 + Math.random() * 20` every four frames; character70 changes mFlicker
with `90 + Math.random() * 10`. Original multiplier labels select the glow
and background timelines (characters66 and77); no replacement particles,
colors or universal x3 effect were added to lower multiplier levels.

The supplied-data movie audit now visits x1.5, x2 and x3 over 1,800 frames,
asserts consecutive timer frames between events, checks both flicker ranges
and changing alpha values, and reaches 40 authored draw batches. The scoring
flow audit still passes landing persistence, line expiry and cancellation.
No game or GPU capture was launched.

## Remaining native parity gaps

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
- The trick name is now composed as retail composes it (`sub_825E51A0`): one `#`,
  then space-separated localisation ids and a bare degree count, resolved token by
  token by `apt_text::localize`. The five metric slots are *not* a label plus four
  modifier flags -- `sub_825C27C0` returns `[name, switch, fakie, clear, new-trick]`
  -- and the modifiers are tokens inside the name string. Spin degrees and the
  front/back flip suffix are supplied; still missing are the Cab/Half-Cab tokens,
  the nollie record swap through the metadata link column, and the Miracle Whip
  case. The FS/BS parity needs a skater stance (`M+187`, the negation of
  `PlayerStance_IsRegular`), which this engine does not publish, so it assumes
  regular.
- The HUD manager's stance transitions are incomplete: `sub_825C2700` returns
  `[switch, fakie, idle latch (M+168), is-nollie-variant && display active]`, and
  the port's fourth slot is a placeholder.
- The compact VM supports the exercised movie paths, not arbitrary APT programs.
  Superclass/native constructor behavior is limited, and Math.random uses a
  local presentation RNG rather than the original engine RNG stream.

Do not describe this build as fully finished or an exact native recreation.

## Why every score was far below retail (fixed 2026-09-21)

`scoring_runtime` computed all five air metrics every frame and then discarded
them at the landing that was supposed to bank them. The landing resolved each
metric with `ScoringData::by_id`, which only returns scorables the vault holds a
`Hash_6918469984A8C596` record for. Of the executable's 332 scorables exactly 300
are authored, and the 32 that are not include **129..=133** (`air_horizontal_distance`,
`air_height_to_peak`, `air_total_height_gain`, `air_player_spin`, `air_player_flip`),
**237** (`air_metric`) and **253** (`generic_metric`).

Retail never needs a record for these: `82DA6260` indexes the fixed metadata table
at `820862A8` by bare id, and `82DA8550` credits them the same way (`li r4,129` /
`bl 0x82da6260`). Their reward is a curve value, not authored points, which is
precisely why they carry no authored record. `catalog::metadata` now resolves them
from that table. A 10 m, 3 m-high 360 air banks **508** where it banked **100**;
a stationary kickflip still banks exactly its authored 100, because the curves are
zero at zero input.

### Authored magnitudes, for judging what is still missing

| curve | input | peak |
|---|---|---|
| `0x3C0` | horizontal distance | 500 at 34.45 m |
| `0x410` | height to peak | 500 at 12 m |
| `0x320` | total height gain | 500 at +12 m, **and 500 at -12 m** |
| `0x370` | spin | exactly 1 point per degree, to 1260 |
| `0x640` | body flip | 300, flat |
| `0x4B0` | gap run above 6 m | 1800 at 20 m |
| `0x500`/`0x550`/`0x5A0` | the other three gap runs | 900 each |
| `0x190` | off-board peak height | 700 at 10 m |

### Still missing, with the evidence already gathered

- **The gap/context collector** (`82DA89A0`, run accumulators `82DA7B50`, sum
  `82DA88E8`). Four runs of horizontal distance, gated on a surface byte, a second
  surface byte, height > `0x680` = 3.0 m and height > `0x67C` = 6.0 m; banked as
  scorable 237. Blocked on the ground query `82DA7A38`, whose reference point is
  the unidentified **player+1632**.
- **The landing-context multiplier and four flat bonuses** in `82DA8550`
  (`0x644`=1.5, `0x654`=1.15, `0x64C`=1.5, `0x65C`=1.15 multiplying all five
  metrics; `0x648`=500, `0x658`=200, `0x650`=300, `0x660`=100 paid as id 237),
  gated on four flags in a 10x68-byte context block filled by `82DA14A0`.
- **The class-3 one-shot** in `82DA93D8`: `carrier.reward += 0x600 (=4.0) * points *
  factor`, one-shot per air, gated on `82DAC780`'s geometric test -- which also
  needs player+1632.
- **Off-board height**: `82DAB7D0` tracks a peak height and banks `curve(0x190)` as
  id 253 at exit. The port's off-board collector instead accrues `curve(0x140)`,
  whose authored y array is **all zeros**, so it earns nothing. Same missing input.
- **The spin accumulator**: `82DA8BE0` sums **two** rotation accumulators (`this+816`
  over `[[owner+20]+368]`, `this+896` over the root transform `[owner+0]`), each a
  `82DAC880` measuring a signed angle about the object's *own* up axis and wrapping
  to (-pi, pi]. The port accumulates a single world-yaw heading delta, which
  degenerates when the board pitches toward vertical. The threshold arithmetic
  itself, `floor((|deg| + 0x63C=80) / 180)` signed by the rotation, is retail-exact.
