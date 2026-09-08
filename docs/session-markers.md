# Native session markers

`PLAY-SESSION-MARKER.bat` runs this task's private executable and original HUD
overlay, starting paused. Resume, hold **LB**, press **D-pad Down** to place;
hold **LB + D-pad Up** to return. These are the original `input.cfg` bindings
(`SessionMarkerSet`, `SessionMarkerUse`), including pressed versus held behavior.
An unset marker cannot return. Releasing cancels the hold. A completed hold does
not repeatedly teleport until released. Automatic bail recovery retains its own
checkpoint. Successful map changes clear the manual marker; pause and replay
cancel an active return and require releasing LB before accepting another one.

The branch includes map-transition baseline `4a92ad1`, cherry-picked as `1d89d85`.
No game, recomp, controller harness or GPU gameplay validation was launched for
this task. The launcher is for the user's manual validation.

## Original evidence

Addresses are TU3 virtual addresses in the owned `default.xex` image, with the
local disassembly image based at `0x82000000`. Local generated PPC and the
community symbol map corroborate function boundaries. No game binary, extracted
art, original shader bytecode, fonts or private research files are committed.

SHA-256 provenance:

```text
Owned default.xex: 1db39496585c521d17a2137804f42cf73ebed2b32cac166ec42dbf772f4dcf7f
TU3 disassembly image: f4aa113eb541bfba03dbc108cf5ab43f58c965b20fa3b82f9c40938a0ad841c4
Private manual executable: 38be66b784b64d87d2134120b9bf582c74b295d4a01b1d1e9246cf62b4f0775f
```

| Native owner | Behavior represented |
| --- | --- |
| `82898FC8` PlayerUI::UpdateSessionMarker | Actions42/43; set/return ordering; retained timer and release latch |
| `8289B6D8`, `8289B140` | Saved transform, foot-forward and on-board byte; marker initially unset |
| `8289B928`, `8289BAE0` | Ground/biped eligibility; wheel count, deck Up.Y threshold, excluded states |
| `82BFBC18` | Downward10m line from Y+.1; VLT slope/drop/clearance; surface switch |
| `82591E30` | Deck+.2 capture; normalized travel velocity above squared speed.25; cross-product threshold.9 |
| `82592B68`, `82B97388`, `82B970D8` | Foot-forward is natural/relative stance equality, inverted by fakie |
| `82592C08`, `82B97350`, `82B97308`, `82B972A8` | Foot-forward setter and orientation/mirror/relative-stance publication |
| `828977A8` | Flag69, pending wipeout teleport and state702 return gates |
| `825926F8`, `82DB8998` | Actor-reset packet and ordinary board/skeleton/velocity reset; camera cut publication |
| `827A9C60`, `827AAF10`, `827AB790` | `cMsgTeleportEffectAmount` (`FAF37802`) publication |
| `827EDE58` | Progress to `f_NoiseFade` (`F0F1D438`), `(1-p,p,0,0)`; random UV scroll and channel weights |
| `827ED7E8`, `82A8AF10` |64x64 four-channel binary noise texture, native six-word generator |

Return duration is.2s through100m,1s from1000m, and a native fused linear ramp
between them (`3A690453`, `3DE38E39`). The timer adds `3C888889` per UI tick.
Relocation uses a strict greater-than comparison. Within.5m there is no effect
or relocation; the native timer still accumulates. The trigger tick and next two
UI ticks publish full effect. The shader's recovered noise contribution uses
the original10x5.625 and11x6.625 UV scales and channel interpolation, rather than
a generic random-pixel or scanline effect.

The validation collection is `Hash_12B64C0E804B0853/default`, with fields
`Hash_ADD032CACF6A1C15` (slope.75), `Hash_8ABE098D3806D273` (drop1m),
`Hash_C0526C883AF0ECCA` (sweep length.4), and
`Hash_CEB092E418A5B001` (radius.5). Values load from installed VLT data.
The CTR branch chain at `82BFBE30`, including its zero-case fallthrough, rejects
surface categories5,6,9,12,13; category8 is allowed by this geometry function.

## Original HUD extraction and future integration

`tools/extract_session_marker.py` uses the existing private UI toolkit. Example:

```powershell
python tools/extract_session_marker.py --game OWNER_GAME_DIRECTORY --ui-toolkit UI_TOOLKIT_DIRECTORY --output .local/session-marker/overlay
```

The toolkit directory contains the `skate3_ui_extract` Python package. Its
existing APT/GEO/RX2/font readers perform extraction; this adapter compiles only
the marker subtree. Re-running updates its source cache incrementally.

Sources: `hud2/hudphonelist`, imported `controls/button_item2`, the Xbox360
`button_DPad_Up_hud.Texture` and `button_DPad_Down_hud.Texture`, original Futura
Shadow atlas/metrics and English language labels. The compiler emits ten
triangle meshes and five private RGBA textures. The root `hudintro` endpoint14,
quick-menu `maximized` endpoint49 and two-item label endpoint8 supply the actual
placements. No replacement typeface or redrawn panel is used. Source texture
and extraction-manifest hashes remain in the private compiled manifest.

Runtime accepts `SKATE3_SESSION_MARKER_OVERLAY`, falling back to
`ASSET_ROOT/private/session-marker`. This is the narrow interface for a future
shared HUD extraction/setup owner: emit `hud.json` schema1 and its referenced
RGBA files there. This task does not install another whole frontend pipeline.
The launcher uses `.local/session-marker/assets.path`, or `SKATE3_ASSETS`.

Audio integration emits `SessionMarkerAudio(u64)` with original GlobalFEPlaySound
IDs: place `0D6C88A3B91C828F`, rejected place `66B3AFE3B602918C`, return
`7F135F9FD28F7F21`. There is currently no audio consumer in this implementation.

## Verification and limits

Release compilation passed with the existing static-CRT Windows target flags:

```text
cargo build --release --locked --target x86_64-pc-windows-msvc -p skate-game --no-default-features
```

Three standalone hold-state tests passed (cancellation/latching, unset/nearby/
blocked returns and tail, distance ramp). The WGSL passed offline Naga27 parsing
and validation. Original HUD manifest counts, triangle structure and every RGBA
payload size passed data checks. These checks initialize no game or GPU device.

Manual validation remains necessary: place/replace, return from riding and
biped states, switch/fakie stance, interrupted holds, bail recovery, camera cuts,
replay/pause and successful/failed map changes. No gameplay result is claimed.

This is not a claim of complete frontend/render parity. The native region
membership service (`82BFBB48`), challenge/online mode restrictions and disabled
row ActionScript alpha behavior have no corresponding owner here yet. The HUD
uses the recovered timeline endpoints, not a general ActionScript runtime.
The noise stream starts from the original static generator words but is isolated
from unrelated original renderer callers, so its exact pixel sequence differs.
The compositor implements the marker noise contribution; general TV/fisheye and
vignette passes are outside this feature. Native render scheduling/color-space
equivalence and stance/reset timing still need manual observation. Map marker
invalidation is a deliberate host lifecycle rule, not a recovered serialization
claim. These gaps are explicit integration work, not synthetic substitutes.
