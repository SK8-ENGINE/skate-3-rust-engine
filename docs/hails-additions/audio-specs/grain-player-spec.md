# Skate 3 granular rolling bed (grains.big + GrainPlayer): behavioural spec for the native port

Written 2026-10-02. This describes **behaviour, formulas and constants in our own words** so the rolling bed can be
re-implemented natively in Rust (Phase B of the native-port plan (doc 11, "Native audio port")). It replaces the
current approximation (each `.grain` cut into 6 looped speed bands, `tools/asset_pipeline/audio_export.py`
`GRAIN_BANDS`, `crates/skate-game/src/game_audio/cues.rs::grain_for`).

Nothing here is code from PR #4/PR #1 or the recomp (no licence: read for understanding only). Constants, offsets,
addresses and vault hashes are facts and are quoted. Companion specs: `aems-voice-graph-spec.md` (modules, buses, block
clock), `aems-evaluator-spec.md` (patch programs, global RNG), `aems-reference.md`, `mixmap-spec.md` (MixMap; being
written in parallel; the MixMap output ids used here are listed in §2.9 so the two can be joined).

## 0. Confidence tags

- **[V]** verified by us against data (disc, vault, image) or a retail trace in this session;
- **[R]** read by us in the recomp's generated C++ (`skate3recomp/generated/skate3_recomp.*.cpp`, TU3 addresses);
- **[C]** PR #4 states it was confirmed against its retail memory capture (bit-exact object dumps, timers);
- **[T]** PR #4 transcribed it from the lifted code, not compared; we have not re-read it;
- **[G]** reproduced by our golden-vector run of PR #4's ported functions in the PoC (§5.1);
- **[I]** our inference. **UNCERTAIN** = a port must not treat it as settled.

All guest structures are big-endian; addresses are TU 3.0.3.0.

---

## 1. Data

### 1.1 `grains.big` [V]

EB BIG v3 archive, 3,446,976 bytes, 14 uncompressed `.grain` members (survey: `grain_survey.json`,
script `tools/audio-file-inspect/grain_survey.py`):

| member | H | duration (s) | rate | samples | seek row 0 (bytes, side, samples, key) | side bytes |
|---|---|---|---|---|---|---|
| asphalt_rough_hard | 160 | 19.503 | 44100 | 860080 | 232349, 118, 860080, 1 | 128 |
| asphalt_rough_soft | 176 | 19.919 | 48000 | 956100 | 259515, 131, 956100, 1 | 144 |
| asphalt_smooth_hard | 176 | 21.968 | 44100 | 968807 | 284227, 143, 968807, 1 | 144 |
| asphalt_smooth_soft | 176 | 21.842 | 44100 | 963222 | 280983, 142, 963222, 1 | 144 |
| concrete_aggregate_hard | 176 | 21.117 | 48000 | 1013604 | 277250, 140, 1013604, 1 | 144 |
| concrete_aggregate_soft | 176 | 19.470 | 48000 | 934573 | 263906, 133, 934573, 1 | 144 |
| concrete_rough_hard | 176 | 21.842 | 44100 | 963228 | 259879, 131, 963228, 1 | 144 |
| concrete_rough_soft | 160 | 20.070 | 44100 | 885096 | 250225, 127, 885096, 1 | 128 |
| concrete_smooth_hard | 176 | 21.090 | 44100 | 930067 | 254494, 129, 930067, 1 | 144 |
| concrete_smooth_soft | 160 | 20.324 | 44100 | 896308 | 251178, 127, 896308, 1 | 128 |
| metal_smooth_hard | 176 | 21.870 | 48000 | 1049760 | 277104, 140, 1049760, 1 | 144 |
| wood_ramp_hard | 144 | 19.053 | 44100 | 840219 | 219096, 111, 840219, 1 | 112 |
| wood_ramp_soft | 144 | 17.617 | 44100 | 776894 | 210104, 107, 776894, 1 | 112 |
| x_jet_rolling | 112 | 12.152 | 48000 | 583305 | 123424, 65, 583305, 1 | 80 |

There is no `metal_smooth_soft` and no wood_ply member, although the vault has a `wood_ply_soft.grain` collection
(key `1258126B657708F0`) that no code path selects [V]: a cut asset.

### 1.2 `.grain` layout [V, field roles C/T]

| offset | type | meaning |
|---|---|---|
| +0 | u32 | **H** = header length = offset of the EAAC stream (112..176) |
| +4 | f32 | duration in seconds. The player uses this stored float, not samples/rate: it equals samples/rate rounded to single in 11 members and is 1–3 ulps off in asphalt_rough_soft (1), concrete_aggregate_soft (1), concrete_aggregate_hard (3) |
| +8 .. H | | seek table (§1.3) |
| +H | | one EA Audio Core stream: header `version 0, codec 3 (EA-XMA), mono, 44100 or 48000 Hz, type 0 (RAM), no loop flag`, sample count, then **one block** spanning the rest of the member |

Each recording is one continuous roll from slow to fast. Measured (20 equal bins over each file, vgmstream decode,
`grain_survey.py --rms`) [V]: level rises monotonically by **9–12 dB** from start to end (e.g. concrete_rough_hard
−22.8 → −13.6 dBFS RMS, asphalt_smooth_hard −25.3 → −12.9, x_jet_rolling −34.2 → −18.9) and the spectral centroid rises
(e.g. concrete_smooth_hard 1.08 → 2.0 kHz, wood_ramp_hard 0.42 → 1.48 kHz). The first ~10 % of most files is
near-stationary slow rolling. The "read position" therefore *is* the speed axis.

### 1.3 Seek table (same format as the `.sek` files of `wheels.big`) [V layout, T semantics]

Header, 8 bytes, identical in every grain: `00 10 0180 00000018`:

| byte | meaning |
|---|---|
| 0 | kind: 0 = run-length columns (all grains); 1 = another table kind (unused here); ≥2 = seek fails |
| 1 | high nibble = column layout (1 for all grains; 0 = another layout, unused); low nibble = a 4-bit value copied into the seek result (0 here) |
| 2–3 | u16 **pre-roll** in samples = 384 (decode this much before the target and discard it) |
| 4–7 | u32 offset of per-entry side data from the table start (0x18 = 24), or 0 |

From byte 8: four interleaved **run-length columns** read row by row through one shared byte cursor:
column 0 = byte step of the entry in the EAAC stream, column 1 = side-data step, column 2 = samples in the entry,
column 3 = 1 on a key (restartable) entry. A row whose sample count is negative ends the table.

- **Column coding.** Each column alternates *run headers* and *deltas*, all signed varints. A header h ≥ 0 means "the
  next delta applies to each of the next h+1 rows"; h < 0 means "each of the next 1−h rows reads its own delta". A
  column's value is the running sum of its deltas. All four columns of a row are advanced in order 0,1,2,3, so their
  bytes interleave.
- **Signed varint** (first byte b0 decides the length; the lowest bit of the *last* byte is the sign, a set sign means
  value = −1 − magnitude):
  - b0 < 0xC0: 1 byte, magnitude = b0 >> 1;
  - 0xC0 ≤ b0 < 0xF0: 2 bytes, magnitude = (((b0·256 + b1) >> 1) with bits 13–14 cleared) + 96;
  - 0xF0 ≤ b0 < 0xFC: 3 bytes, magnitude = ((((b0·256 + b1) with bits 12–15 cleared)·256 + b2) >> 1) + 6240;
  - 0xFC ≤ b0 < 0xFF: 4 bytes, magnitude = ((((b0 & 3)·2^24 + b1·2^16 + b2·2^8 + (b3 & 0xFE)) >> 1) + 0x60000 + 6240;
  - b0 = 0xFF: 5 bytes, a raw big-endian u32 (no sign step).
- **What it indexes:** sample position → (byte offset of the XMA entry, side-data offset). It does **not** index speed
  or grain segments. In every grain row 0 already covers the whole stream (`samples = num_samples`, key 1) [V], so
  every in-range seek resolves to *"restart the decoder at byte 0/sample 0, decode 384 pre-roll samples… "*. In
  practice: entry start 0, pre-roll used = min(target, 384), skip = target − pre-roll used; entry + pre-roll + skip =
  target. The side data (~1 byte per 2 KiB XMA packet) is for the hardware XMA decoder to locate packets [T].
- **Port:** decode each grain once to PCM at its native rate (vgmstream at setup); a seek to t seconds is simply
  "read from frame ⌊rate × t⌋" (§2.6). The seek table only needs parsing to validate members. Keep the varint/column
  reader (it is the same for `wheels.big` `.sek` tables); our implementation in `grain_survey.py` reads all 14.

### 1.4 Which grain plays [T; surface table V]

Per board the owner (`SFXObj_SkateBoard`, constructor `sub_824C5058`) loads the 13 surface members into 26 slots
(two per member, one per player A/B, both point to the same bytes) and keeps **two trucks × two players**.

1. **Material → surface.** Per truck, the audio state's per-wheel material (state +620 + 4·wheel; wheel 0 for the
   primary truck, wheel 3 for the other) is `collision tag & 0x7F` minus 1, with tag 0 (or anything out of 0..143)
   meaning **143 = none** [T]. Rolling surface = word +4 of element `min(material, 94)` of the vault's
   `Sk8::AudioSurfaceMap` (class `C1831BDB6CB1B1EA`, key `C489459A0C07D154`, field `4CA607558B1CF440`, 95 × 72 B);
   material 143 gives surface 3 [T]. In terms of our engine's 7-bit `audio_surface` tag (= material + 1) [V, vault]:

   | rolling surface | what plays | tags (`tag & 0x7F`) |
   |---|---|---|
   | 1 asphalt_rough | grain | 2 |
   | 2 concrete_rough | grain | 4, 66 |
   | 3 asphalt_smooth | grain | 1, 55, 56, 61, 62, 71–83, 86–88, ≥ 95, and 0 (none) |
   | 4 concrete_smooth | grain | 3, 51–54, 57, 60, 63–65, 92, 93 |
   | 5 wood_ramp | grain | 6, 7, 41–50, 58, 59, 94 |
   | 6 concrete_aggregate | grain | 5 |
   | 9 metal_smooth (hard only) | grain | 9, 11–36, 38–40, 84, 85, 89, 91 |
   | 7, 8, 10, 12, 13 | **no grain**: a per-surface `Class_rolling` patch (selectors 1, 2, 10, 11, 9) | 7: 10, 70 · 8: 8 · 10: 67 · 12: 68, 69 · 13: 37 |
   | 0 | UNCERTAIN (no grain member, not a Class_rolling surface) | 90 |
   | 14 | **no contact**: stop this truck's sound | (forced, see 3.) |

   Our current `grain_for` table differs from this (e.g. tag 6/7 wood, tags 5/8/10 aggregate, tag 90 wood); the port
   must use the vault map.
2. **Soft/hard:** soft member when `sub_824B23C8() == 1` = audio state +684 = (Motion+200 < 0.5) [T]. Surface 9 has
   only `metal_smooth_hard`. What Motion+200 is (wheel durometer from the board setup?) is UNCERTAIN.
3. **Forced 14 (no contact):** while grinding (state +341), or on the local player while the manual latch (§2.7) is set
   and the truck's reference wheel has not landed (state +464 + wheel) [T].
4. Collection key per member (grain-class vault `7AB23C11B6ADA2DE`), owner slot pair A/B [T]:
   asphalt_rough soft `943B1CB05BA9A6BA` 14/15, hard `7EB8015B4C02405E` 0/1; concrete_rough soft `B29FADBBBC2F39C2`
   16/17, hard `03721D0FA99A03C8` 2/3; asphalt_smooth soft `DC1009D50E327F8F` 18/19, hard `7C5912FC2DABF98C` 4/5;
   concrete_smooth soft `607A6BC3D427DA49` 20/21, hard `FFB5E3E62E0B4943` 6/7; wood_ramp soft `382B12636ED9D8DA`
   22/23, hard `7947A259F181FDB4` 8/9; concrete_aggregate soft `863C58AC34BAD599` 24/25, hard `B303AED8241530E2`
   10/11; metal_smooth_hard `1C9C52CC0E1CD4CF` 12/13. Anything else → `default` (`D7EDBD362D7D2152`), nothing bound.
5. **x_jet_rolling** is not a surface grain: it is the "AudBoard Rocket" speed layer of `SFXObj_SenseOfSpeed` (§2.8).

**Surface routing** (`sub_824C5CA8`, every frame, game thread) [T]: for the primary truck, then (local player only) the
other truck, compare the new surface with the stored one. On a change away from a real surface it pulses owner MixMap
input 0 (32767 for that frame, else 0). If the truck had a live sound it is stopped (Class_rolling released, or both
grain players stopped), except on the local player when the other truck has no live sound, in which case the sound
is **handed to the other truck** (the primary-truck index flips) instead of being stopped. Then, for a new surface
≠ 14, a sound starts **only if the other truck is not already on that surface**: grain surface → bind both players of
that truck (§2.5); other surface → post its Class_rolling. So **one sounding truck per distinct surface**: normally
one truck (2 grain players) sounds; both trucks sound only while they straddle different surfaces. Owner input 6 =
32767 while truck 0's surface is 9 (metal). PoC `Routing::route` (skate-game `player_audio/components/board.rs`) is
the oracle for golden vectors of this bookkeeping.

### 1.5 Per-surface tuning (grain-class vault `7AB23C11B6ADA2DE`) [V values; field roles T]

`default` holds everything; members override a few fields. P0..P3 = position-curve control points (column 1 of rows
3, 2, 1, 0 of the 4×4 matrix `A985FBAA9326718D`; column 0, unused by the code, holds 1, ~0.47, ~0.25, 0; probably the
authoring curve's speed axis [I]).

| collection | max km/h | P1 | P2 | turn cap | B slope gain / ramp km/h | A shift/slope | B base shift | notes |
|---|---|---|---|---|---|---|---|---|
| default | 74 | 0.3621 | 0.7069 | 0.8 | 1.5 / 10 | −100 Hz | −10 Hz | |
| asphalt_rough_hard | 65 | 0.2586 | 0.7069 | 0.8 | 1.5 / 10 | −100 | −10 | |
| asphalt_rough_soft | 65 | 0.0690 | 0.7069 | 0.8 | 1.5 / 10 | −100 | −10 | |
| asphalt_smooth_hard | 65 | 0.0517 | 0.8276 | 0.8 | 2.0 / 5 | −150 | −10 | |
| asphalt_smooth_soft | 65 | 0.5517 | 0.6034 | 0.8 | 2.0 / 5 | −150 | −10 | |
| concrete_rough_hard | 60 | 0.0345 | 0.9138 | 0.6 | 2.0 / 10 | −100 | −10 | |
| concrete_rough_soft | 60 | −0.0690 | 1.0690 | 0.6 | 2.0 / 10 | −100 | −10 | curve can leave [0,1] slightly |
| concrete_smooth_hard/soft | 65 | −0.0517 | 0.7069 | 1.0 | 2.0 / 5 | −150 | −10 | |
| concrete_aggregate_hard | 65 | 0.5345 | 0.4828 | 0.3 | 1.5 / 5 | −100 | −10 | B shift/slope −50; rise step 0.1 |
| concrete_aggregate_soft | 65 | 0.2586 | 0.6379 | 0.3 | 1.5 / 5 | −100 | −10 | B shift/slope −50 |
| wood_ramp_hard | 65 | 0.3621 | 0.7069 | 0.8 | 2.0 / 5 | −150 | −25 | |
| wood_ramp_soft | 65 | 0.6207 | 0.7069 | 0.8 | 2.0 / 5 | −150 | −25 | |
| metal_smooth_hard | 55 | 0.0690 | 1.0517 | 0.45 | 2.0 / 10 | −150 | −20 | special gain 0.6, special shift 100 Hz, push shift low −50 |

P0 = 0 and P3 = 1 everywhere. Defaults used by all (unless noted above): **GrainParams** (`D18D1174735E5CDE`, an array
of two 5-float elements, only in `default`): player A = attack 0.1 s, sustain 0.2 s, release 0.1 s, search window 1.6 s,
drift threshold 0.05; player B = 0.2, 0.1, 0.2, 1.5, 0.05. Turn-intensity rise/fall steps 0.06 per frame; special gain
0.65 / special shift +150 Hz; B shift per slope +50 Hz; push envelope: speed ramp 45 km/h, speed-scale peak 1.4 (slow)
→ 1.1 (fast), shift peak −52 Hz → −20 Hz, timing 35/200/600 ms (scale) and 30/200/600 ms (shift); slope divisors
−10 (down) / +10 (up). Field hashes are listed in `grain_survey.py`.

Owner/rocket tuning (class `6E878344774A7999`, `default`) [V]: rocket start 35 km/h, top 60 km/h, gain word 22000,
rocket GrainParams 0.1, 0.4, 0.1, 2.4, 0.1; graph-3 clip 0.09, shelf 5000 Hz / 0.65; graph-3 send ramp 46 → 70 km/h up
to level 3.0; graph-1 level ramp 52 → 74 km/h down to a floor of 0.45; gain wobble ramp 42 → 70 km/h, wobble 0 =
15–30 ms segments of magnitude 0–0.30, wobble 1 = 5–15 ms of 0.10–0.25.

---

## 2. The grain player

### 2.1 Objects and counts

- Per board owner: 2 trucks × 2 **GrainPlayers** (A at owner+1176+8t, B at owner+1180+8t), both of a truck bound to
  the **same** grain file; A and B differ in GrainParams and in their per-frame record. Plus one rocket GrainPlayer in
  SenseOfSpeed (local player only). [T]
- GrainPlayer (372-byte object; layout confirmed against the retail capture [C]): the record {gain, pitch, n/a,
  position}; the 5 GrainParams; a hold flag; send target; bound flag; grain data/duration/stream/seek pointers; **two
  voice slots** {graph, state timer, start seconds, position at pick, state 1 attack / 2 sustain / 3 release}; the
  active-slot index; a scheduler instance; a 16-entry **recent-windows list** {start, end, next} plus used-head,
  last-inserted and free-head indices. Constructor defaults: gain 1, pitch 1, position 0, params 0.01 / 0.5 / 0.01 /
  4.0 / 0.05 (always overwritten on bind).
- **Voices:** at most 2 per player (one fading out while the next fades in). Typical: one sounding truck → 2 players →
  ≤ 4 grain voices; both trucks → ≤ 8; + ≤ 2 rocket voices.

### 2.2 Per-frame record (owner update `sub_824C6BD8`, 60 Hz, after the frame's MixMap evaluation) [T; offset C]

Inputs per truck: ground speed v (m/s, audio state +208), this truck's surface tuning, MixMap outputs (§2.9), the
owner's slewed values (§2.7). Single-precision arithmetic throughout.

1. Effective speed v' = v × s, where s is the **push speed-scale envelope** value while it runs (else v' = v) (§2.7).
2. t = clamp(v' × 3.6 / max_kmh, 0, 1).
3. **Position A** = cubic Bézier in t with control points P0..P3:
   pos = (1−t)³·P0 + 3t(1−t)²·P1 + 3t²(1−t)·P2 + t³·P3. Not clamped (two members can leave [0,1] slightly).
4. **Position B** = max(pos − 0.1, 0): B always reads 0.1 (≈2 s of recording) behind A. Measured on retail: exactly
   −0.100 at every speed [C].
5. **Pitch (A and B)** = MixMap pitch output 3 (cents → ×4096) / 4096. Retail medians: 3209 at rest, ~3500 at 1 m/s,
   ~4080 from ~5 m/s up [C capture medians]: i.e. grains play ~22 % slow at a crawl, ~unity when rolling.
6. **Gain A** = level(1)/32767 × (1 − max(I, Bk)) [× special_gain while "special"] [× seam envelope while active],
   where I = slewed turn intensity (+1164), Bk = slewed brake (+1168) (§2.7).
7. **Gain B** = I × level(2)/32767 [× seam envelope]; plus, while the downhill slope level D (+1508) > 0:
   Gain B += D × ramp × slope_gain(primary truck) × Gain A, clamped to ≤ 1, with ramp = clamp(v' × 3.6 /
   slope_ramp_kmh, 0, 1) (or 1 if that km/h ≤ 0). So **B is the turning (and downhill) layer**: retail B is silent
   ≥ 40 km/h in straight rolling and peaks around 15 km/h; turn input +204 is 0 in 87 % of retail frames [C].
8. The four floats are stored into the player; the audio thread reads them each block (§2.4). No other smoothing
   than the MixMap's own envelopes and the per-frame slews.

The record of a truck is written only while that truck's grains are running (`+1328+t` set and its surface plays grains).

### 2.3 Choosing a grain (pick, `sub_828ECAB0`) [T; G]

Let L = attack + sustain + release (grain length, seconds of source at pitch 1), W = search window, D = stored
duration, pos = record position.

1. Target T = (D − W) × pos; region = [T, T + W]. (So the region never runs off the end of the file for pos ≤ 1.)
2. The recent list holds the windows played since the last reset, sorted by start. Its free gaps (from a sentinel
   before 0 to the first window, between consecutive windows, and from the last window to D) are each clamped to
   the region and **tiled with back-to-back windows of length L** starting at the gap's (clamped) start, while a whole
   window still fits (in single precision; the 4th 0.4-s window of a 1.6-s region does *not* fit because 1.2+0.4
   rounds above 1.6); at most 64 candidates.
3. If there is no candidate, or the list is full (the last inserted entry is index 15): **collapse** the list to just
   the most recent window and retry; if the region holds at most one window (⌊W/L⌋ ≤ 1) return T instead.
4. Otherwise draw r from the **title-wide generator** (`sub_82A8AF10`, 6 words at `0x82FD7D74`, seeded at game init
   with the time base; shared by ~100 call sites) and take candidate ⌊r × 2⁻³¹ × 0.5 × count⌋ (uniform). Insert it into
   the sorted list; return its start.

Behaviour this produces [G, `grain_vectors.tsv`]: a **shuffle bag without replacement** over the 3–4 windows of the
current region (A: 0.4-s windows in a 1.6-s region → 3–4; B: 0.5 in 1.5 → 3; rocket 0.6 in 2.4 → 3–4); when the bag is
empty it refills but never repeats the window just played. The grid is anchored to windows already played, so small
position changes keep reusing aligned windows. A bind resets the list (sentinel only), so the first pick after a bind is
a window starting exactly at T.

Edge (port note): r within 128 of 2³² rounds to 2³² in single precision, making the index = count and reading an
unfilled candidate {−1, −1}; the start is then clamped to 0 (§2.6). Probability ~3·10⁻⁸ per pick; reproduce or clamp.

Pick sequences are **not reproducible** against retail (global RNG); only the distribution and the bag rule are. For
exact validation log the 6 RNG words at each pick (§5.3).

### 2.4 The per-block scheduler (`sub_828EC6F0`, via stub `sub_828ECE98`) [R for the drift test and timers; T rest]

Registered on bind as a plug-in in scheduler bucket 0 of the audio system ("Grain Player"); the block driver
`sub_82B48530` runs bucket 0 in **phase 1 of every 256-frame block** (48 kHz → every 5.333 ms), before the command
drain and the graph pass, with δ = 256/48000 as f32 (`0x3BAEC33E`) [C: attack timer 0.1 → 0x3DAC0831 → 0x3D962FC9 …].
So this is **per audio block, not per 32-ms evaluator tick and not per game frame.** Each block:

1. **Drift test.** d = (position recorded at the active slot's pick) − (current record position). If |d| > drift
   threshold (0.05 ≈ 1 s of recording) and hold is clear and the active slot has a voice:
   - if the other slot has a voice: release it (unless already releasing); nothing new starts this block;
   - else: release the active slot (unless releasing), pick (§2.3), start a voice in the other slot (§2.5) and make it
     active. (A fast speed change therefore cuts the current grain short with a normal release.)
2. For each slot 0, 1 that has a voice:
   - post the record **gain** to the voice's Send (Sen0 parameter 0) and the record **pitch** to its Resample
     (parameter 0), every block, applied this block (phase 1 precedes the drain);
   - timer −= δ in attack/release; **timer −= δ × pitch in sustain** (fused multiply-subtract);
   - if timer < 0:
     - attack → sustain, timer = sustain;
     - sustain → release (timer = release, fade to 0); then, unless hold: stop the other slot's voice if any, pick,
       start a new voice in the other slot, make it active;
     - release → stop the voice.
3. Ordering detail (bit-exactness): a voice started in slot 1 while processing slot 0 is processed (gain/pitch posted,
   timer decremented) in the same block; one started in slot 0 from slot 1 waits for the next block [T]. A voice started
   by the drift branch (step 1) is processed in the same block.

The hold flag (+36) is cleared on bind and never set by any caller we know of (UNCERTAIN; treat as always clear).

**Resulting rhythm at pitch p** (blocks quantise each phase up to the next 5.33 ms):

| player | grain period (start to start) | overlap (release ∥ next attack) | sounding per grain | source used per grain |
|---|---|---|---|---|
| A | 0.1 + 0.2/p s (0.3 s at p = 1) | 0.1 s | 0.4 s | 0.2·p + 0.2 s (= L at p = 1) |
| B | 0.2 + 0.1/p s (0.3 s) | 0.2 s | 0.5 s | 0.4·p + 0.1 s |
| rocket | 0.1 + 0.4/p s (0.5 s) | 0.1 s | 0.6 s | 0.2·p + 0.4 s |

Retail evidence [V, our trace from recomp session `20261001_211347`, `grain_trace_stats.py`]: 8,484 grain voice
starts in 10.5 min. Rocket starts cluster at **475–525 ms** apart (163 of 557), surface-grain starts of one member
never exceed ~350 ms apart while rolling (period 0.3 s + quantisation + pitch < 1), the modal rate is **6 starts/s**
(= 2 players × 1/0.3 s, one sounding truck), with bursts up to 42/s (drift restarts, both trucks, other skaters). This
matches the timing above.

### 2.5 Starting and stopping a voice [T; fade curve T]

**Start (`sub_828EC3F0`)**: build a mono voice graph `SndPlayer1 → Resample → GainFader → Send` (graph order 0),
play the grain's EAAC stream from start = clamp(pick, 0, D) seconds (§2.6), set the fader to 0 immediately, then
**fade in to 1 over `attack` seconds**, point the Send at the player's bus (graph 1 of its chain, §3.1) and stamp the
current record gain on it; remember the start and the record position at this pick.

**Fades (GainFader, `aems-voice-graph-spec.md` §4.7)**: requested with start time "now", the curve code is 1 →
**square-root curve**: fade-in g(n) = √(n/N), fade-out g(n) = √(1 − n/N), N = ⌊seconds × 48000⌋ samples (≥ 1),
sample-accurate inside the block. The overlapping release and attack of consecutive grains therefore form an
**equal-power crossfade** (g_in² + g_out² = 1).

**Release (`sub_828EC2F8`)**: fade to 0 over `release` seconds. **Stop (`sub_828EBCB0`)**: deferred "player stop"
command → the graph is torn down at the next drain (Send release de-click, §4.8 of the voice-graph spec); used after a
finished release (voice already at 0), and abruptly in the cases below.

**Bind (`sub_828EC040`, from surface routing)**: reset the recent list, register the scheduler plug-in, take H,
duration, seek table and stream from the file, copy the GrainParams element (A = 0, B = 1) into the player, **pick and
start a voice in slot 0 at once** (game thread). In retail the truck's bus chains are also torn down and rebuilt on
every bind (`sub_824C4D50` + `sub_824C8878`), so filter, frequency-shifter and pan histories restart at a surface change
(PR #4 keeps its chains instead; a port should reset those states).

**Stop the player (`sub_828EBF90`)**: when bound, stop both voices **without a release fade** and detach the plug-in.
Used on a surface change and when the truck leaves contact (surface 14). The rocket stops its player twice when it
drops to ≤ 35 km/h (second call is a no-op).

### 2.6 Reading the recording [T; V for single-block streams]

The play command converts the start time into a frame at the stream's native rate: frame = ⌊rate × start⌋ (only when
> 0; else 0). With the single-entry seek table the decoder restarts at sample 0, decodes and discards up to 384 pre-roll
samples and skips the rest, so output begins exactly at `frame`. Port: PCM decoded once at native rate; start reading at
`frame`; play forward (no loop flag; a grain never reaches the end because T + W ≤ D at pos ≤ 1 and a grain lasts ≈ L,
but a pitched or drifting grain may run past its window; let it). Resample converts to 48 kHz with ratio = native rate
/ 48000 × pitch, linear interpolation, no anti-alias filter (`aems-voice-graph-spec.md` §4.3).

### 2.7 Owner-side modulators [T unless noted]

Run in the owner's process (`sub_824C6A78`, before the MixMap evaluation) except where noted.

- **Push envelopes** (on every push plant, state +335; `sub_824C6198`): speed-scale envelope from its current value
  (or 1.0) to peak = lerp(1.4, 1.1, t) over 35 ms, hold 200 ms, back to 1.0 over 600 ms; frequency-shift envelope 0 →
  lerp(−52, −20, t) Hz over 30 ms, hold 200, → 0 over 600; t = clamp((v − 1) × 3.6 / 45, 0, 1); tuning from the
  primary truck's collection; linear segments advanced by the frame dt while not idle. Effect: each push moves the
  read position forward (up to 40 % more speed at a standstill) and detunes the bed by tens of Hz.
- **Turn intensity** I (`sub_824C8588`, owner +1160/+1164): raw = clamp(|COM velocity| (state +96) × 0.24, 0, cap) ×
  turn (state +204), clamped to ±cap; forced 0 while the manual latch or the trick latch holds; slewed toward raw by at
  most the rise/fall step (0.06, aggregate_hard rise 0.1) per frame; I = |slewed|. Cap and steps from the primary
  truck's collection.
- **Brake** Bk (+1168): while braking (state +336) against the direction of travel (cosine of velocity delta and
  velocity < 0, or undefined) slew toward 1, else toward 0, by 0.05 per frame.
- **Slope** (`sub_824CA738`): with the primary truck on a grain surface, downhill level D = clamp(slope (state +712;
  PR #4 docs call it the slope, the PoC field is named `pump_absorption_712`; meaning UNCERTAIN) /
  −10, 0, 1), uphill U = clamp(slope / 10, 0, 1); posted as owner inputs 2/3 (×32767).
- **Manual latch** (+1504): set while balancing (state +340); once set, cleared when wheel count (state +200) is 0 or 4.
  **Trick latch** (+1505): set while state +372 (an airborne-trick flag); cleared when both feet are back in the deck
  box (+615 and +616). "Special" = manual latch, else trick latch.
- **Seam-pattern gain envelope** (`sub_824CA448`, local player): when wheel 0's seam pattern (state +636) changes to a
  pattern whose vault wobble (class `7242F32831ED3332`, patterns spidercrack … special_2) is not {1, 1}, **one** linear
  segment from 1 to a random gain in [low, high) (steps of 0.01) over a random duration in [ms_low, ms_high) ms (title
  RNG, `sub_824CA318`), then hold; it multiplies **both** gains A and B while active. **Correction 2026-10-02** (read in
  the asm): not a random walk; the same pattern only advances the segment. In the vault only spidercrack has a wobble
  ≠ {1, 1} (0.8 → 0.6, 30 → 80 ms).

### 2.8 The rocket layer (`x_jet_rolling.grain`, SenseOfSpeed `sub_824E7980` / `sub_824E7CB0`) [T; timing V]

Local player only. When ground speed > 35 km/h and not running: bind to the **default output bus directly** (no chain),
rocket GrainParams (0.1, 0.4, 0.1, 2.4, 0.1), start. At ≤ 35 km/h: stop. While running, record: gain =
level(5)/32767 × 22000/32767 (SenseOfSpeed controller), pitch = pitch(3)/4096 (same controller), position =
clamp((v × 3.6 − 35) / (60 − 35), 0, 1). No hysteresis.

### 2.9 MixMap outputs consumed (for `mixmap-spec.md`) [T]

Board owner controller (owner vfuncs 52/56/60/64 = raw u16 / pitch cents→×4096 / level & 0x7FFF / filter & 0x7FFF):

| output | used as |
|---|---|
| level(1) | gain A scale (retail active-rolling p90 7859 ≈ 0.24) |
| level(2) | gain B scale |
| pitch(3) | grain pitch (both players) |
| level(11), level(12) via vfunc64 | LowPass / HighPass Hz on both chains (retail modal 24971 / 77) |
| raw(0) via vfunc52 | azimuth: Pan2D1 degrees = raw × 360/65535 |
| level(13) | environment send /32767 |
| level(21), level(22) | graph-2 first send A / B /32767 (local player) |

SenseOfSpeed controller: level(5), pitch(3) (rocket). Owner MixMap *inputs* written by this code: 0 surface-change
pulse, 2/3 slope, 4 push foot planted, 5 heading rate, 6 on metal (and 1 skid). Which inputs drive level(1)/(2) (e.g.
wheel-contact inputs that silence the bed in the air) is for the MixMap spec. Chain values (§3.2) are computed in
process (before this frame's evaluation, i.e. one frame old); records in update (this frame's).

### 2.10 Airborne, grinds, manuals, bails

- **Grind**: surface 14 → both players of the truck stop (abrupt); Class_rolling layer surface becomes 13 for the held
  layers.
- **Manual**: the lifted truck goes to 14 (its sound stops or is handed to the other truck); manual/trick latch also
  zeroes I (B silent) and scales A by the special gain with a +150 Hz shift.
- **Airborne**: nothing in the grain code stops the players on take-off; voices keep cycling and their level must come
  from MixMap level(1)/(2) (contact inputs). Whether the wheel material holds its last value in the air or becomes
  "none" (→ surface 3, which would rebind asphalt_smooth on every jump) is **UNCERTAIN**: first thing to measure
  (§5.3, hook on routing). The trick latch only affects I/special.
- **Bail / off board**: the board owner keeps running; the result depends on contact/material (UNCERTAIN).

---

## 3. Voice graph, buses and interplay

### 3.1 Per-player bus chain (`sub_824C8878`; one per player, 4 per board) [T; shape in voice-graph spec §6.2]

```
grain voices ──► graph 1 (order 2, mono): SubMix → HighPass → LowPass → FrequencyShiftSsb → Send(→ graph 3)
                                          → Gain (wobble) → Send(→ graph 2)
graph 3 (order 3, local player only, mono): SubMix → Clip ±0.09 → Gain (wobble) → HighShelf 5 kHz × 0.65 → Send(→ graph 2)
graph 2 (order 5): SubMix → Send(→ manager+116 target, level 0) → Send(→ environment bus, level(13))
                   → Pan2D1 (1 → 6 ch) → Send(→ eEQChain bus 8 = the default bus)
```

- **FrequencyShiftSsb** (`sub_82B22898`): two cascades of two allpass biquads form an I/Q (Hilbert) pair; out = I·cos φ
  − Q·sin φ, φ advancing 2π·shift/rate per sample, wrapped per block. At 0 Hz it is a pure allpass (phase-shifted copy),
  never a bypass. B's base shift (−10 Hz; wood −25, metal −20) decorrelates B from A (same file, 0.1 behind, shifted).
- The four chains run in parallel; graph 3 adds a gently clipped, top-shelved copy at high speed (§3.2).

### 3.2 Values pushed each frame (`sub_824C9058`, per truck whose grains run; plus helpers) [T]

| target | value |
|---|---|
| HighPass, LowPass (A and B) | level(12), level(11) Hz |
| FSS A | (special ? special_shift : 0) + push shift envelope (while running) + D × A_shift_per_slope(primary) |
| FSS B | B_base_shift + push shift envelope + D × B_shift_per_slope(primary) |
| Pan2D1 (A, B) | raw(0) × 360/65535 degrees |
| env send (A, B) | level(13)/32767 |
| graph-2 first send (local) | level(21)/32767 (A), level(22)/32767 (B) |
| graph-1 → graph-3 send (`sub_824CAEC0`, local, all 4 players, on change) | 0 below 46 km/h, linear to 3.0 at 70 km/h |
| graph-1 Gain, graph-3 Gain (`sub_824CB078`/`sub_824CB180`, local) | wobble: gain = 1 + e × ramp, e = a random-walk envelope alternating sign (segments 15–30 ms, |e| ≤ 0.30 for A's chains; 5–15 ms, 0.10–0.25 for B's), ramp = clamp((km/h − 42)/(70 − 42), 0, 1); graph-1 also × level ramp (1 at ≤ 52 km/h → 0.45 at ≥ 74 km/h) |

Quirks to keep [T]: the graph-1 level ramp has no branch below 52 km/h, so it **keeps its last value** when speed
drops (it only returns towards 1 while between 52 and 74 km/h); the graph-1 → graph-3 send is posted only when it
changes. The "last posted" wobble value is shared between the two trucks, so after truck 0 posts, truck 1
always compares equal and its wobble is never posted (retail behaviour).

### 3.3 Interplay with the other board sounds [T]

- **Class_rolling** (`PatchBank_Rolling_*`): per-surface patch for the non-grain surfaces (7, 8, 10, 12, 13) instead of
  grains; held layers 0 and 3 always posted for the local player (sparse random texture, not the bed); layer 5 while
  wheel 0 is on the spidercrack seam pattern. Same speed word shape (clamp(v·3.6/maxKmh) × 10000, push-scaled).
- **Rolling_Rattle_Class**: posted on each push plant while the primary truck is on a grain surface (surface code 1..5
  from the grain key), released on the next.
- **Class_Seams**: per-wheel seam clicks (separate component); the seam-pattern *gain envelope* above modulates the
  grain bed itself.
- **SenseOfSpeed** rattle (≥ 30 km/h) and wind (15–55 km/h) patches + the rocket grain (> 35 km/h).
- **Wheel spins** (`SFXObj_Wheels`, `wheels.big` `.snr` + `.sek`): a separate SndPlayer1 path using the same seek
  mechanism (§1.3), on take-off / manual; spin-down gain × (1 − speed/max).
- Skid, squeaks, board slide: bank patches, independent.
- Levels [C, PR #4 native 6-ch]: rolling bed −22.9 dBFS; takeoff +17.0 dB and landing +23.3 dB over the bed.
  Retail grain transfer (truck 0, both players, by km/h): gain A ≈ 0.06 at 5, 0.08–0.17 from 10 up; position A 0.12
  (10 km/h) → 0.63 (40 km/h); B peaks at 15 km/h (+2.7 dB over A) and is silent ≥ 40 km/h. Compare at matched surface
  and speed with narrow bins and medians, never session averages (three false findings came from that).

---

## 4. Native port outline (behaviour, not code)

1. Setup: decode the 14 members to PCM at native rate (we already decode them; keep whole files, drop the 6 bands);
   validate header/seek table; load the vault tuning (§1.5) and AudioSurfaceMap.
2. Game thread, 60 Hz: surface routing (§1.4), modulators (§2.7), records (§2.2), chain values (§3.2), rocket (§2.8).
3. Audio thread, per 256-frame block at 48 kHz: for each bound player run §2.4 before mixing; voices = PCM reader +
   linear resampler + sqrt fader + send gain (64-sample de-click on change); chains per §3.1. The RNG is our own
   instance of the add-with-carry generator, seeded at startup (retail is unreproducible anyway).
4. Keep it moddable (grain files, GrainParams, curves as data), per the port plan's goals.

---

## 5. Validation plan

### 5.1 Golden vectors from the PoC (done: first set) [G]

`crates/skate-audio-core/examples/grain_vectors.rs` in the local PoC worktree (local only, never committed) drives PR #4's ported functions; output
`grain_vectors.tsv`:

- `POS`: speed 0..80 km/h → position A/B (bit patterns) for concrete_rough_hard and default (e.g. concrete_rough_hard
  30 km/h → A `3EF611A8` = 0.4806, B 0.3806; saturates at max km/h);
- `PICK`: 24 successive picks for A and B params at pos 0 / 0.25 / 0.63 / 1.0, generator from zero and from the image
  state (shows the bag rule: e.g. A, pos 0.63 → windows 12.7524/13.1524/13.5524/13.9524 s);
- `CAND`: candidate tiling (3 windows in [0,1.6] with L 0.4; 4 in [19,20.6]).

**Next vectors** (needs the full runtime): copy PR #4's `crates/skate-data/examples/grain_repro.rs` (in
PR #4's branch; not in the PoC worktree) into the PoC locally and log per block: slot states, timers, starts,
picked positions, posted gain/pitch, the RNG words before each pick, and the 6-ch output RMS, for (a) constant
speeds 2/4/6/8 m/s per surface, (b) a speed ramp crossing the drift threshold, (c) a surface change (stop + bind), (d)
pitch 0.78 vs 1.0. Our port must match timers, state transitions, start seconds and posted values exactly (given the
same RNG words), and the audio to ≤ 1e-5 FS per sample before the chain (decode differences aside).
Also unit vectors for: modulators (push envelopes, turn slew, brake slew, slope, latches), chain values, wobble and
send ramps: all pure functions in the PoC's `grain::board` / `grain::envelope` and `player_audio/components/board.rs`.

### 5.2 Retail evidence already in hand [V]

`grain_trace_stats.py <trace.tsv>` matches PLAY lines (SndPlayer1 play hook) whose sample header equals a grain
stream: per-member counts, starts/s and start-interval histograms (§2.4). Results for session 20261001_211347 in
`trace_stats_20261001_211347.json`. The PLAY hook does not log the start offset.

### 5.3 Passive recomp hooks to add (wrap and call through, like `src/skate3_audio_trace_hooks.cpp`)

| tag | function | log |
|---|---|---|
| GBIND | `sub_828EC040` bind (r3 player, r4 bus, r5 data) | player, data, duration word ([data+4]) → member, params after return (+16..+32) |
| GPICK | `sub_828ECAB0` pick (r3 player; f1 result) | before: position +12, the 6 RNG words at `0x82FD7D74`, recent head/last/free (+364/+366/+368); after: f1 |
| GSTART | `sub_828EC3F0` start voice (r3 player, r4 slot, f1 start) | slot index ((r4 − r3 − 88)/28), start, gain, pitch |
| GTICK | `sub_828EC6F0` per block (r3 player, f1 δ) | on any state/active change only: δ bits, +0 gain, +4 pitch, +12 position, +144 active, per slot +92 graph, +96 timer, +100 start, +104 pick position, +108 state |
| GSTOP | `sub_828EBF90` stop player | player, caller chain |
| GROUTE | after `sub_824C5CA8` (r3 owner) | owner +768/+772 surfaces, +1320/+1324, +1328/+1329, +1496/+1497, +1500, + audio state +620/+632 materials, +332 air, +341 grind, +684 soft |
| GREC | after `sub_824C6BD8` (r3 owner) | the 4 players' records, owner +1028/+1032, +1152/+1156, +1164, +1168, +1456/+1464, +1508, state +204, +208 |
| GCHAIN | after `sub_824C9058` | values posted per truck (read graph-1 FSS +52, HP/LP +52, Pan +52) |
| n/a | `sub_824E7980`/`sub_824E7CB0` | rocket running flag, record |

Use short scripted background recomp runs (never ask the user to replay): on one known surface,
push to ~10, 20, 30, 45 km/h and coast 5 s each; a straight roll vs a carve; a manual; an ollie and a 1-s air; a grind;
a crossing between two materials. Acceptance: start intervals, state timers and pick regions exact given logged RNG
words; record gain/pitch/position per frame within 1 ulp of our model fed with the logged inputs; per-player pick
distribution χ² consistent with uniform; airborne behaviour settled (§2.10).

---

## 6. Open questions (priority order)

1. Airborne: does the wheel material hold, or become none → surface 3 (rebinding asphalt_smooth each jump)? Does
   level(1)/(2) go to 0 in the air, and how fast? (GROUTE/GREC + MixMap spec.)
2. Surface 0 (tag 90): what retail does (no member, not Class_rolling).
3. ~~Soft flag source (Motion+200 < 0.5)~~ **Settled 2026-10-02:** Motion+200 = Processed+2764 = the board
   profile's wheel hardness (`animation_phase_packet.rs` `wheel_hardness`, default 0.7 = hard).
4. Hold flag (+36): any writer?
5. GainFader curve code from the fade request (+28 rounded = 1 → square root); confirm with a GTICK/fader dump.
6. Exact block of the first tick after a game-thread bind vs a scheduler start (the 3-block capture fits both).
7. Chain rebuild on bind: confirm the teardown/rebuild order and that histories reset.
8. PR #4 notes that its capture's "GP ret" value was the window end, not the start; re-check with GPICK.

---

## 7. Sources & credits

| source | author / owner | link | licence | contribution |
|---|---|---|---|---|
| Upstream PR #4 (`skate-audio-core/src/grain/*`, `skate-audio-formats/src/grain.rs`, `skate-data/src/audio/grains.rs`, `skate-game/.../board.rs`, `speed.rs`, `examples/grain_repro.rs`, docs `player-audio-retail-drivers.md` §3/§11, `wheel-audio-handoff-2026-09-17.md`, `engine-defects.md`) | andrewnakas (Andrew Nakas) | github.com/SK8-ENGINE/skate-3-rust-engine/pull/4 (fork github.com/andrewnakas/skate-3-rust-engine, commit 368a433; read from a local clone and a PoC worktree) | none, read only | Discovered that the bed is a granular player; recovered the GrainPlayer object, pick, scheduler, voice graph, bus chains, FSS module, board records, vault mapping, push envelopes, retail capture comparisons and transfer functions. Almost everything tagged [T]/[C] here is their finding, re-described. |
| Upstream PR #1 (`mx/audio-engine-vehicles`) | andrewnakas | github.com/SK8-ENGINE/skate-3-rust-engine/pull/1 | none, read only | Earlier version of the same grain port and docs. |
| skate3recomp (generated C++, audio-trace hooks) | mchughalex (recomp); trace hooks ours (local branch `audio-trace`) | github.com/mchughalex/skate3recomp | none stated, read only | Lifted code we read for §2.4 [R]; the retail trace used in §2.4/§5.2. Built on rexglue-sdk. |
| Skate 3 (EA Black Box), TU3 image, disc `grains.big`, vault `skatercollections.vlt` | Electronic Arts | n/a | proprietary, owner's copy, never committed | The data measured in §1. |
| vgmstream (`data/tools/vgmstream-cli`) | vgmstream contributors | github.com/vgmstream/vgmstream | ISC-style | EA-XMA decode for the RMS/centroid survey. |
| Our notes `aems-voice-graph-spec.md`, `aems-evaluator-spec.md`, `aems-reference.md`, `rwaudio-prior-work.md` | this project | n/a | n/a | Module semantics (Resample, GainFader, Send, buses), block clock, RNG. |
| CC0 prior art (nfsmw Snd9, tw2004) | dbalatoni13, mitsevox | see `rwaudio-prior-work.md` | CC0 | Checked: no GrainPlayer there; nothing used. |

## 8. Files

- Survey script: `tools/audio-file-inspect/grain_survey.py` (`--rms` adds level/centroid profiles).
- Trace stats: `tools/recomp-trace/grain_trace_stats.py`.
- Golden-vector example: `grain_vectors.rs` (local, in the PoC worktree).
- Data (local): `grain_survey.json`/`.txt`, `grain_vectors.tsv`, `trace_stats_*.json` (≈0.1 MB).
