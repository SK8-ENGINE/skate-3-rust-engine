# Tricks component (Class_Flips, cloth_trick) and Class_Treatment: spec (2026-10-02)

Our reading of the TU3 (title update 3) recompilation (`tools/recomp-code-search/fn.sh ADDR`, reference only); image constants by
`img.py` (local tool); vault values from the user's `skater-collections.json`. Port: `crates/skate-audio`
`player/tricks.rs`, `player/treatment.rs`, `player/globals.rs`.
Tags: [R] read in the asm, [V] vault, [IMG] image, [T] retail trace, [U] UNCERTAIN.

## 1. Tricks component (vtable 0x822FC7B8, owner Tricks controller 0x40010050 = `keys::tricks(0)`)

Object fields [R] (`sub_824CBD98`): +16/+28 owner refs (+72 local byte), +32 audio state, +36 Class_Flips object,
+40 cloth A, +44 cloth B, +48 last frame's +348 (−1), +52 posted flip id (−1), +56 cloth A id (−1), +60 latched
+352 (−1), +64 running trick time (0.0), +68 cloth B time (0.0), +72 w12 slew (0), +76..+100 trick-event latches,
+84 event timer.

**Process `sub_824CBEB0(dt)`**, only when `[this+28]+72` (local):
1. `+60 := state+352` while `state+348 ≠ −1`.
2. Flip poster `sub_824CBFB8(id = +348)`:
   - off = !air(+332) && local && +310; go = air ? +343 : off. Not go, or +36 held → return.
   - flip id +52 := off ? 34 : id. Ids −1, 35, 36 → return (no flip for grabs 35, coffin 36).
   - Spin words from state +480/+484/+488 (deck ω on its Ri/Up/At rows): x = trunc(|v| / div × 1000)
     (`0x82256FE8` = 1000.0), min 1000, then 0 if x < threshold (subf/xoris/addc/subfe select).
   - Posts the 116-byte object `sub_824AFAD8` (28 words, below). Class handle 0x8302EE48 = object 4 `Class_Flips`.
3. Cloth A `sub_824CC590(id)`: id = −1 → nothing. Not held → post cloth_trick(id), +56 := id. Held with another
   id → release (re-posted the next frame).
4. Cloth B `sub_824CC680(id, dt)`: id ≠ −1 → +64 += dt; else if +48 ≠ −1 (the trick just ended) → +68 := +64,
   +64 := 0, ended. If +68 > 0: when ended and +60 ≠ −1 → release any cloth B, post cloth_trick(+60). Else
   (+68 ≤ 0) release cloth B.
5. +48 := id; +68 := +68 > 0 ? +68 − dt : 0.
6. Slew `sub_824CD170(dt)` (dt ≤ 0 → +72 := 0): target = `sub_824898C8()` ? T₁₃ : first set bit of G+96
   (0x8000 → T₁₅, 0x4000 → T₁₄, 0x2000 → T₁₃) else 0; step down trunc(R↓·dt), up trunc(R↑·dt) [V below].
   `sub_824898C8` = G+96 bit 0x2000 || (X+932 && copied X+476 byte) || X+1060 == 8 (X = *(0x830CFDC4)).
7. `sub_824CD390`: game events 24685–24697 through `sub_824AA858` (trick-event system, no AEMS): not ported.

**Update `sub_824CBF78`** = `sub_824CC7D8`, `sub_824CCE48`, `sub_824CCFE8`:
- Flip: id = off ? 34 : +348; if !(air || off) or id ≠ +52 → release. Else rewrite: w9/w8/w7 spins (Ri/Up/At),
  w10 = trunc(+220 × 500) 0..1000, w4 = pitch(2) 0..8192, **w0 = level(1)**, w3 = raw(0) 0..65536, w1 = 32767,
  w2 = 0, w5 = level(3) 0..25000, w6 = 0, w16 = (+224 == 0), w26 = local ? level(6) : 0, w20 = level(8),
  w22 = level(7), w12 = +72 0..1000; redeliver.
- Cloth A: bail (+676) or (+348 == −1 and !air) → release; else w0 = 32767, w4 = 25000, w5 = w6 = 0,
  w7 = level(4), w1 = raw(0) 0..65535, w2 = pitch(5) 0..8192; redeliver. Cloth B: bail → release; else same.

**Class_Flips words** (`sub_824AFAD8`, payload 28 = bank's): w0 0, w1 32767, w2 0, w3 0, w4 4096, w5 25000,
w6 0, w7 At spin 0..1000, w8 Up spin, w9 Ri spin, w10 time-scale word 0..1000, w11 trick id 0..40,
w12–w15 0, w16 (+224 == 0), w17 27646, w18 14848, w19 6656, w20 0, w21 10000, w22 0, w23 bool class
`0x11A631878B239355` (not modeled: 0), w24 local && +64 == 0, w25 local, w26 local ? level(6) : 0, w27 eEQ 6.
**cloth_trick** (`sub_824B71C0`, 11 words, handle 0x8302EED8 = object 22): w2 4096, w4 25000, w8 1,
w9 id 0..40, w10 eEQ 0, rest 0.

Banks: `Sk8_Air_Flip_Tricks` (Class_Flips, capacity 3), `Foley_Cloth` (cloth_trick capacity 3; its
`c_foley_utility` is posted once at boot in retail: POST object 20 once per session [T]).

## 2. Class_Treatment (vtable 0x822FCDA0, owner Treatments 0x40010070 = `keys::treatments(0)`)

**Process `sub_824DD408`** (local only): while +36 empty → `sub_824B0080` posts the 92-byte object (handle
0x8302EE70 = object 9), never released. Companion `hall_of_meat_slo_mo` (handle 0x8302F058 = object 70, 64-byte
`sub_824AF368`) held while (G+96 & 0x04000000 && !`sub_8279E180(G)` && +220 < 1.0) || X+1060 ≠ 7, else released.
Words (15): w2 4096, w3 25000, w6 32767, w9 1, w10 = level(3), w11 8, w14 = clamp(X+1060 − 7, 0, 3).
**Constructor words** (22): w1 32767, w4 4096, w5 25000, w10 500, w15 7000, w16 28000, w17 32767, w18 bool class
via `sub_82484460` (0), w19 local && +64 == 0, w20 local, w21 8, rest 0.
**Update `sub_824DD6F0`**: w0 = level(2), w3 = raw(0) 0..65535, w4 = pitch(1) 0..8192, w10 = trunc(+220×500),
w14 = (+224 == 0), w7 = trunc(+236 × 1000) 0..10000, w8 = trunc(+240 × 1000) 0..10000, w9 = trunc(clamp(+260 ×
166.66667 (`0x822F9408`), 0, 1000)), w11 = w12 = 0; B+16 → w11 = trunc(B+24 × 1000) 0..1000; B+164 → w12 = 1,
w13 = trunc(B+168 × 10000) 0..10000 (else w13 keeps its value). Companion: w0 32767, w7/w8/w13/w10 = level 4/5/6/3,
w12 = trunc(clamp(|COM v| (+212) / 40, 0, 1) × 10000).
B = `*(*(0x83083C38)+0x2FCB4)`, sub-object B+16 reset by `sub_827AB6E0` (from `sub_8279E800`); B+16 is the byte
PlayerPhysics.in11 reads (`sub_824B19C8`). Writer not located [U].

## 3. Audio-state fields read (bridge `sub_824B0DA8`, record writer `sub_827A1B78`)
| offset | meaning | source | our engine |
|---|---|---|---|
| +332 | in the air | rec+148 & 0x80000000 | `airborne` ✓ |
| +343 | trick active | rec+148 & 0x01000000 | `trick_active` (= scorable present) ✓ |
| +348 | audio trick | rec+180 = scorable record +164 (`8C3025DB4D1761AF`) | `audio_trick` (host-resolved) ✓ |
| +352 | 2nd audio trick | rec+184 = scorable record +172 (`A2C5C22C5BE725F8`) | **new** `audio_trick_2`; 28 flips, 35 grabs, −1 ollie/nollie/footplants |
| +344 | rec+148 & 0x00800000 = scorable record +176 bool (`7B9298C7883E2FA2`) | n/a | not read here |
| +310 | off-board hold expired: +309 (rec+164 bit 0 = [r30+72]+309) held > 0.4 × (1 − min(0.12·v₂₀₈, 1)) s (`0x8208EA70` 0.12, `0x82181B90` 0.4) | bridge | **new** `offboard_310`, not published: false |
| +480/+484/+488 | deck ω · rows Ri/Up/At (B40+0, `sub_82772748`) | | +488 = `deck_spin`; **new** `deck_spin_xy` (host: ω·basis col 0 / col 1) |
| +220 / +224 | G+0 time scale (1.0) / G+4 byte (c_tazer input) | G = *(0x83083C38)+0x2F070 | **new** `time_scale` (1.0), `global_224` (false) |
| +236 / +240 / +260 | air time / time until predicted landing / jump height | Air+176 / +184 / +200 | `air_time` ✓; **new** `air_until_landing`, `jump_height` (skate-core `KnownAirOutput::time_until_collision_184`, `jump_height_200`) |
| +676 | bail | | `bail` ✓ |
| +212 | |COM v| | | `com_speed()` ✓ |

## 4. Globals (`player::globals::Globals`)
G+96 (writer not located): free skate 0 [T: no slow-mo post in 4 sessions]. X+1060 game-flow mode: free skate 7
[T: object 70 never posted]. `sub_8279E180(G)` taken as false.

## 5. Vault (holder `*(0x830CFDA4)`: +72 = `C1831BDB6CB1B1EA`/`tricks`, +68 = `C1831BDB6CB1B1EA`/`EE7B8A8A893A4E30`,
+140 = eEQChain `42AFE160E647167C`/default) [V]
- tricks: Ri thr `5C73CF6A0D50C8D8` 297 / div `1494BB20854C155C` 3.0; Up `EE157886DE5D3C97` 703 /
  `02D39586635FB1A3` 4.4; At `9A0316625B63CD99` 603 / `8EDBACCBA6FE46AD` 10.2; w17 `99E6FF024834E4C7` 27646,
  w18 `D2D0EBAC43842F6D` 14848, w19 `13C155A181A55BA4` 6656, w21 `B565D4D763128252` 10000.
- slew (`47EC76B4F9FC79F6`): bit15 `B601DFAB3AF7DE66` 250, bit14 `08441EA8E8019665` 700, bit13 `36F8D12486A929D1`
  1000, down `6B57BD44C0E0B267` 1000/s, up `57AED5FBC374D8F1` 10000/s.
- eEQChain: Flips `D9BE1F2F1A72FEE8` 6, cloth `4B6E2D79A8452D9B` 0.
- treatment: w15 `32F9111CBF746F34` 7000, w16 `E4BC6FE030A553C4` 28000, w17 `1F11951C2AF58CC7` 32767, companion
  speed `5A49D603680EE3FF` 40.0.
Export: `stage_tricks_treatment.py`, a local tool (dry run prints; `--write`; `--bank` stages Treatments).

## 6. Retail evidence and checks
Timing [T] (`post_timing.py` (local script), session 164620): cloth A posts ~215 ms (13 frames) before
Class_Flips; Class_Flips a median 16.7 ms (1 frame) after the pop's jump-velocity write; cloth B 150–650 ms after
(the trick's end). Treatments streams 13/14 start in the air, 14–326 ms before the touchdown contact
(`treat_timing.py`); scripted hops are 0.61–0.64 s.
Levels through the real banks (`tests/player_tricks.rs`, `--nocapture`), vs `retail_voices.py` gains:
- Flips whooshes slots 0–3: 0.370 vs retail 0.347–0.365 (+0.5 dB); slot 5: 0.133 vs 0.134; ollie slot 12 0.030 vs
  0.035–0.038; shove-it/360 slots 24/25 0.199 vs retail 0.015–0.078 (few retail voices).
- cloth_trick slots 8/9/12: 0.18–0.24 vs retail 0.21–0.25.
- Treatments 13/14: 0.0736 / 0.0184 vs 0.0746 / 0.0186 (−0.1 dB); they start when w8 falls through ~0.37 s.
- First trigger: the first trick's Flips voices equal the second's.
[T] Treatments slots 16/17 (a long-air layer scaled by w17, started after w8 rises from 0 at takeoff) DO play in the
recomp (TREAT session all_20261002_223306: 18 + 18 voices, peak 0.004–0.029, median 0.0149; ours on its inputs 20 + 20,
median 0.0155). Their samples equal sense_of_speed 3/4, which hid them from `retail_voices.py` until the address
disambiguation. The recomp's +240: 0 on the ground, the whole prediction on the first air tick, −1/60 per tick, still
counting on the landing tick, then 0. +236 (Air+176) also counts on the landing tick (ours since 2026-10-02). [U] B+164/B+168 (PR #4: "set, ≈1.0 most of play"): with them set slots 0–12 play, which
retail never does → reset values kept. [U] G+4 byte: w14/w16 = 1 needed for 13/14 to play (retail plays them).
[T] 2026-10-02 overnight: the ramp air's late 16/17 start (538/577 ms, 223306) and the 1113 ms one in 223613 follow
~1 s recomp audio-thread stalls (no PLAY/GAIN/SEND/MOD/XMA lines while TREAT/GREC continue;
`audio_gaps.py` (local script)). Short airs (< 0.5 s) with a 16/17 voice: ours 11/22, the recomp 10/22 (walk
phase + shared RNG). No change.
