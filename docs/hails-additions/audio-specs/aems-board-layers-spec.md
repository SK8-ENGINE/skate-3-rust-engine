# SFXObj_SkateBoard rolling layers: Class_rolling, Rolling_Rattle_Class, c_board_slide (spec, 2026-10-02)

Native port: `crates/skate-audio/src/player/rolling.rs`, tests
`tests/rolling_banks.rs` (data-gated). Read from the TU3 (title update 3) recompilation (reference only; every function below was dumped
to a local text file per address). Companion specs: `grain-player-spec.md` §1.4/§2.7/§3.3 (routing, push
envelope), `aems-evaluator-spec.md` §1.10 (a post reaches every bank bound to the class).

Owner offsets are `SFXObj_SkateBoard` (`r31`), state offsets the audio state (`[owner+36]`).

## 0. Order inside the owner

Process `sub_824C6A78` (before the MixMap tick): slope `sub_824CA738` → **routing `sub_824C5CA8`** → push / **rattle**
/ brake / turn `sub_824C6198` → chain values `sub_824C9058` → skid `7438` → squeaks `7738` → **layer 5 `sub_824C9F68`**
→ seam envelope `CA448` → `CAEC0` / `CB180` (send / wobble) → **board slide `sub_824CB3C8`** → `CB828`, `CBAC0` (not
examined: no class posts).

Update `sub_824C6BD8` (after the tick): per-truck loop (grain records, or **the Class_rolling patch**), then skid
`7A20`, squeaks `7DD0`, **rattle `80C0`**, **held layers `9948`**, **layer 5 `CA038`**, seam draw, push envelopes,
**board slide `CB4C0`**.

Create (`sub_824C59C8`, owner reset): truck surfaces `+768/+772` = 14, kinds `+1320/+1324` = 1 (grain), key `+760` =
`default`; for the local player `sub_824C9830` posts the **held layers 0 and 3**.

## 1. Surface routing `sub_824C5CA8` (V: read in the asm)

Per frame: SkateBoard input 0 := 0. Pass 0 = primary truck `p` (`+1500`), pass 1 (local player only) = `1 − p`
(recomputed after pass 0). For truck `t`, other `o = 1 − t`:

1. `new` = `sub_824C82A8(t)`: local player and manual latch (`sub_824CA688`: `+1504` set while balancing `+340`, cleared
   once wheel count `+200` is 0 or 4) and the truck's wheel not landed (`+464` wheel 0 for the primary truck, `+467`
   wheel 3 for the other) → 14; grinding (`+341`) → 14; material `+620` (primary) / `+632` (other) ≥ 143 → 3; else
   AudioSurfaceMap word 1 of `min(material, 94)` (`sub_82494CD8`).
2. `new == stored[t]` → next pass.
3. `stored[t] != 14`: input 0 := 32767 (this frame). If **local and the other truck has no live sound** (`+1496 + o`
   == 0): **hand over**: `p := 1 − p`, swap `t` ↔ `o` (no stop). Else stop `t`: kind 0 → release its patch
   (`+1312 + 4t`); kind 1 with grains running (`+1328 + t`) → stop both players, reset both chains; live := 0.
4. `stored[t] := new`; 14 → next pass (kind keeps its value).
5. `+760` := member key of `new` (`sub_824C8370`, soft by `sub_824B23C8` = state `+684` for the local player); the grain
   collection `+184 + 16t` := lookup(grain class `7AB23C11B6ADA2DE`, key).
6. kind = 0 (Class_rolling) for surfaces 7, 8, 10, 11, 12, 13, else 1 (the `bdzf` switch at `0x824C5EB8`; surface 11
   has no tag in the vault map).
7. `new == stored[o]` → no start. Else kind 0: selector `+1488 + 4t` = {7: 1, 8: 2, 10: 10, 11: 12, 12: 11, 13: 9},
   post `Class_rolling(speed 0, selector, new)` → `+1312 + 4t`; kind 1: bind both grain players (`sub_824C8878`,
   members by the key's slots), `+1328 + t` := 1. Either way live `+1496 + t` := 1.
8. kind → `+1320 + 4t`.

After the passes: input 6 := 32767 iff `sub_824C82A8(0) == 9` (truck **index 0**, whichever wheel it reads).

Consequences (new, not in the grain spec):
- The primary truck always reads wheel 0, the other wheel 3; a hand-over moves the "primary" label, not the sound.
- **The local player's last sounding truck is never stopped**: when both trucks go to 14 together (grind, manual with
  both latches clear) pass 0 hands the sound to the silent truck (which stores 14) and pass 1 hands it back. A manual
  (front wheel up) hands the label to truck 1 and the sounding truck keeps playing on wheel 3. The MixMap mutes it
  (SkateBoard level(1) = 0 without wheel contact, see §5). Retail evidence: GREC (session 180430, 310 k frames) has
  neither truck running on only 621 frames (menus/start), 39 k (1,0)↔(0,1) label swaps
  (`grec_running.py` (local script)). Grain spec §2.10 "Grind: both players stop" is wrong for the local
  player; it holds for other players (no hand-over).
- Surface 0 (tag 90): kind 1 with the `default` key; `sub_824C8370` leaves the caller's slot words at 0/1, so the bind
  plays **asphalt_rough_hard** with the `default` collection's tuning (resolves grain spec §1.4 "0 UNCERTAIN").

## 2. Class_rolling (12 words; constructor `sub_824C4C18`)

Post: w0 0, w1 0, w2 4096, w3 clamp(speed, 0, 10000), w4 clamp(selector, 0, 15), w5 0, w6 clamp(surface, 0, 13), w7 0,
w8 0, w9 25000, w10 0, w11 32767.

| use | post | update |
|---|---|---|
| per-truck patch `+1312 + 4t` (routing) | speed 0, selector by surface, w6 surface | in `sub_824C6BD8` while live and kind 0: w0 32767, w11 level(1), w1 raw(0) ≤ 65536, w2 pitch(3), w3 = S(selector, push-scaled), w5 0, w8 = selector 1 ? 0 : level(13), w9 filter(11), w10 filter(12), w7 = `+340` ∥ `+339` |
| layer 0 `+1304` / layer 3 `+1308` (create, local) | speed L(0)/L(3), selector 0 / 3, surface 3 | `sub_824C9948`: w0 32767, w11 level(7) / level(9), w1 raw(0) ≤ 65536, w2 pitch(8), w3 L(layer), w5 0, w6 = surface rule, w7 manual flag, w8 level(19), w9 filter(17), w10 filter(18) |
| layer 5 `+1332` (spidercrack) | `sub_824C9F68`: pattern = `+636` (wheel 0), or `+648` (wheel 3) while the manual latch holds and `+464` = 0; none held and pattern 1 → post S(5, push-scaled), selector 5, surface 3; held and pattern ≠ 1 → release | `sub_824CA038`: as the held layers with w11 level(10), w3 S(5, push-scaled) |

- S(n, scaled) = `sub_824C6B30`: trunc(clamp01((v·k / kmh[n])·3.6)·10000), v = state `+208` (signed), k = push
  speed-scale envelope value `+1028` while running (`+1032` = 0), kmh[n] = vault array (§6).
- L(n) = the same without k (`sub_824C9830` / `sub_824C9948`).
- Surface rule (w6): primary truck's `sub_824C82A8`; if 14 the other's; if both 14: 13 while grinding, else unchanged.
- clamp01 = retail's `fsel` pair (NaN → 1).
- Held layers and patches are released only by the routing / layer-5 rule (and owner destruction).

Retail POST callers (sessions 163809 / 164620 / 180430): held layers `824C98C4` / `824C991C` once per session at board
create; patches `824C5F88` 2 / 14 / 14; layer 5 `824C9FE8` 1 / 0 / 108.

### Which bank answers (census `class_rolling_selector_census`, evaluator only)

A post reaches PatchBank_Rolling_Surfaces, _SpiderCracks, _Objects and _RocksBounce. Slots opened (8 s):
- always: Rolling_Surfaces 11, 12, SpiderCracks 0 / 11 (selectors 0–5) or 17 (6–15), Objects 9 / 12 / 15 (by speed):
  silent at the gains below (none shows in the rendered table);
- surface word 3: Rolling_Surfaces 15 (the held layers' texture: retail's most played stream, 50 of 133);
- selector 1: Rolling_Surfaces 0, 2 (4 at low speed); selectors 2–15 (except 9): 1, 3 (5 at low speed);
- selector 9: Rolling_Surfaces 13, 14; selector 5: SpiderCracks 0–16; selectors 10, 11: Objects 0–2, 12–14 (5–7, 15–17
  fast; 8–11 slow);
- RocksBounce never opens for these packets (other owners post it).

## 3. Rolling_Rattle_Class (12 words; holder `+1300`)

In `sub_824C6198` inside the push-plant block (`+335`), after the push envelopes: release + free the held rattle; then
if `+1320 + 4·p` == 1 (primary truck kind grain) post `sub_824B0248(speed, code, eq)`:
- speed = trunc(clamp01(((v − 1)·3.6) / F)·10000), v = `+208`, F = field `12275AA8AC4A63FB` of the primary truck's
  grain collection (30 in `default`; no member overrides it);
- code from `+760`: asphalt_smooth_hard 1, concrete_rough_hard 2, concrete_smooth_hard 3, wood_ramp_hard 4,
  concrete_aggregate_hard 5, else 0 (soft members, asphalt_rough, metal, `default`);
- words: w2 4096, w3 clamp(speed, 0, 10000), w4 clamp(code, 0, 8), w6 1, w8 25000, w10 32767, w11 clamp(eq, 0, 32767),
  rest 0; eq = eEQChain class `42AFE160E647167C` `default` field `C04832978CDED925` = 8.

Update `sub_824C80C0` (while held): w0 32767, w10 level(6), w1 raw(0) ≤ 65535, w2 pitch(3) ≤ 8192, w7 level(16), w8
filter(14), w9 filter(15). Never released except by the next push.

Bank: sample = 3·code + (0, 1, 2) by speed word (thresholds between 4000–7000 and at 10000); code 5 reuses 6..8.

## 4. c_board_slide (12 words; holder `+1884`, bank board_scrapes)

`sub_824CB3C8`: none held and state `+780` ≠ 0 → post `sub_824B0670(eq, mode = (+780 == 2))`: w2 4096, w4 25000, w8
clamp(eq), w10 clamp(mode, 0, 3), rest 0; eq field `F2B44F93BD91662E` = 7. Held and `+780` = 0 → release.
`sub_824CB4C0`: w0 32767, w1 raw(0) ≤ 65535, w2 pitch(23), w3 = trunc(clamp01(((v − 0.5) / 15)·3.6)·10000) ≤ 10000
(0.5 = image `0x8209975C`, 15 = `C1831BDB6CB1B1EA`/`621090620F4F936A` field `9635B780C7472A6E`), w4 filter(25), w5
filter(26), w6 level(27), w7 level(24), w11 = mode 2 ? `AB87C3D1EDDDDCBC` : `662CEE73D2E3FE2F` (both 15000).

`+780` ("loose board"), bridge `sub_824B0DA8` from audio record `+156` bits 5–6, written by the conditioner
`sub_827A1B78` (`0x827A2A60..2B14`, cleared each frame): set when (record `+148` bit 5 = State`+59` (bail) **or**
`+152` bit 30 = state 500 (on foot)) and `+148` bit 8 (= `[rec+24]+3475`, a deck-contact byte) and `+476` (=
`[rec+24]+12` − 1, a material) < 94: dot(`[rec+0]+80`, `[rec+32]+80`) < −0.9 → 1, −0.1 < dot < 0.1 → 2.
Retail plays it: caller `824CB470` 22 events (163809, peak 0.256), 28 (164620, peak 0.346). Port:
`rolling::loose_board(bail ∥ on_foot, deck_contact, deck_material, up_dot)`. UNCERTAIN: which two vectors make the dot
(taken as deck up · contact/world up) and what `[rec+24]+3475` is exactly.

## 5. MixMap SkateBoard outputs read (headless, real MxB, straight roll, camera 3.5 m behind)

| km/h | l1 | l6 | l7 | l9 | l10 | l13 | l16 | l19 | f11/f12, f14/f15, f17/f18 | l24 | l27 | f25/f26 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 0 | 0 | 0 | 0 | 0 | 2614 | 0 | 0 | 24971 / 77 | 7093 | 3849 | 19456 / 77 |
| 8 | 7575 | 11013 | 12791 | 4834 | 7488 | 2614 | 1658 | 0 | 24971 / 77 | 7093 | 3849 | 19456 / 77 |
| 40 | 8277 | 12034 | 12791 | 4834 | 10231 | 2614 | 1812 | 0 | 24971 / 77 | 7093 | 3849 | 19456 / 77 |

In the air l1/l6/l7/l9/l10/l13/l16 are 0. (`skateboard_outputs_by_speed`.)

## 6. Vault values (setup must export: `audio_export.player_tuning` → `rolling`)

| value | class / collection / field | user's vault |
|---|---|---|
| Class_rolling km/h by selector 0..15 | `C1831BDB6CB1B1EA` / `7B0ED922C779B74C` / `880C82E8EF647EC4` (16 floats, holder `[0x830CFDA4]+56`) | 70, 65, 65, 70, 100, 45, 45, 45, 45, 35, 45, 45, 45, 45, 45, 45 |
| rattle km/h | grain `7AB23C11B6ADA2DE` / `default` / `12275AA8AC4A63FB` (per member, inherited) | 30 |
| rattle eEQChain | `42AFE160E647167C` / `default` / `C04832978CDED925` | 8 |
| slide eEQChain | `42AFE160E647167C` / `default` / `F2B44F93BD91662E` | 7 |
| slide km/h divisor | `C1831BDB6CB1B1EA` / `621090620F4F936A` / `9635B780C7472A6E` | 15 |
| slide level mode 1 / 2 | same / `662CEE73D2E3FE2F` / `AB87C3D1EDDDDCBC` | 15000 / 15000 |

Fallback when a field is missing: retail reads `0x830D0850` (zero block) → divide by 0; the port keeps the vault values
as `RollingTuning::default()`.

## 7. Validation (rendered through the real banks + MixMap, `rolling_layers_play_their_retail_banks`)

Per-start level = peak voice gain (master × dry) in the first 0.4 s (`analyse_session.py` GAIN_WINDOW); retail =
GAIN × SEND per start (report "Levels per bank").

| bank | ours (8 / 20 / 35 km/h) | retail 163809 med / p90 / max | 164620 |
|---|---|---|---|
| Rolling_Rattles (push on asphalt) | 0.336 / 0.354 / 0.366 | 0.007 / 0.301 / 0.366 | 0.032 / 0.362 / 0.370 |
| PatchBank_SpiderCracks (layer 5) | p90 0.226–0.341, max 0.226 / 0.277 / 0.355 | 0.000 / 0.223 / 0.293 | 0.001 / 0.228 / 0.353 |
| PatchBank_Rolling_Surfaces held layers (surface 3, slot 15) | 0.030 / 0.046 / 0.102 | 0.007 / 0.044 / 0.074 | 0.010 / 0.121 / 0.388 |
| Rolling_Surfaces patch surface 8 (slots 1, 3) | 0.024 / 0.120 / 0.115 (max 0.169) | n/a | slot 3 median 0.126, max 0.358 |
| Rolling_Surfaces patch surface 13 (slots 11–14) | 0.106 / 0.111 / 0.115 | n/a | slot 14 max 0.174, 13 max 0.284 |
| PatchBank_Objects (surfaces 10, 12) | 0.101 / 0.166 / 0.203 | 0.001 / 0.030 / 0.030 | 0.003 / 0.219 / 0.250 |
| board_scrapes (loose 1/2, 6–15 km/h, per frame) | 0.057–0.058 median, max 0.060–0.099 | 0.002 / 0.096 / 0.256 | 0.004 / 0.121 / 0.346 |

Maxima and p90 agree; retail's low medians come from the session mix (rattles at a standstill: level(6) = 0 at 0
km/h; quiet samples of other surface codes; landing swells raise level(1) for the higher retail maxima). First
trigger: the first rattle's peak equals the later ones (asserted).

## 8. What this replaces in the game (interim)

- `cues::grain_for` tags 8 / 10 / 70 → concrete_aggregate_hard and 37 / 67–69 → metal_smooth_hard (INTERIM): with the
  native routing these surfaces bind **no grain**; their sound is the Class_rolling patch (selectors 2, 1, 1, 9, 10,
  11). Tag 90 binds asphalt_rough_hard with the `default` tuning (not wood_ramp_hard).
- `cues::BED_CUES` `Rolling_Rattles` and `PatchBank_Rolling_Surfaces` entries: replaced by the rattle (per push) and
  the held layers / patches; the interim bed must not play them when the native layers run.
- The grain bed's own routing (`grain_bed.rs`: majority surface, one truck, stop on grind) should be driven by
  `Rolling::process`'s `GrainEvent`s (two trucks, hand-over, never stopping the local player's last sounding truck;
  the MixMap mutes it on grinds / in the air), and its SkateBoard input 0 / 6 writes taken from `Routed`.

## 9. UNCERTAIN / not modeled

- `sub_824B23C8` for non-local players (a lookup, not `+684`).
- The loose-board vectors and `[rec+24]+3475` (§4); our engine publishes none of it yet.
- `CB828`, `CBAC0` in the owner's process (no class posts; not examined).
- The bus send each layer's voices take (eEQChain tweak words are posted, routing per bus spec).

## 10. Listening report 2026-10-02 ("busy / faster than the speed", "rolling continues while jumping")

Session `state_20261002_203746` (+ stderr log), headless analysis:
- **My layers' interim stand-ins are responsible for part of it.** With `SKATE_AEMS=1` the interim bed still fires
  `BED_CUES` `Rolling_Rattles` (random 0.31/s × `bed_rate`, level 0.44, samples 0.9–1.9 s) and
  `PatchBank_Rolling_Surfaces` (0.14/s, samples 0.3–1.6 s): ~7.7 random rattles + 3.5 surface one-shots in 21 s of
  riding with 4 pushes. Retail posts a rattle only per push (and at a standstill level(6) = 0 → silent) and its
  Class_rolling layers are continuous MixMap-gated textures. The bed gate (`!airborne`) stops new interim fires in the
  air, but a one-shot started up to ~1.2 s before takeoff rings on: P ≈ 0.33–0.43 per jump for a rattle, 0.14–0.19
  for a surface one-shot at the session's takeoff speeds. Fix: skip those two `BED_CUES` when the native player runs
  (and once `rolling.rs` is integrated, they are replaced).
- **The native layers go quiet in the air** (`layers_go_quiet_after_takeoff`: push at 30 km/h, takeoff 10 frames later:
  rattle peak 0.217 / patch 0.173 on the ground, no voice above 0 from 10 frames after takeoff): SkateBoard
  level(1)/(6)/(7)/(9)/(10) are 0 without wheels.
- Retail facts checked for the other owners: retail's state `+208` keeps the riding speed in the air and fluctuates
  ±5 km/h (GREC 180430, owner 40C33020, 96 takeoffs), so SenseOfSpeed's rattle (level(1) not wheel-gated: 832 at
  31 km/h both on the ground and in the air) toggling around 30 km/h in jumps (11 posts in this 38 s session) is retail
  behavior, not a bug. Retail's wheel material is 143 on 96 % of air frames (37938 / 39685), so routing surface 3
  (asphalt_smooth) in the air and the grain bed's "bind asphalt_smooth_hard (surface tag 0)" at landings is retail-like;
  the bed is muted there by level(1) = 0 (the grain bed's part).
