# Skate 3 environment (reverb) bus: behavioral spec for the native port

Written 2026-10-02 (headless reverse engineering of TU3 (title update 3) + clean recomp traces). Companion to `aems-voice-graph-spec.md`
(§3 Send A, §4.8 Send, §6 buses), `mixmap-spec.md` §6.3 (emitter send word out8), `ems-emitters-re.md` (reverb
zones, region layers). Behavior, formulas and constants in our own words. The recomp's generated code, PR #4 / the
PoC and b5-decomp were read for behavior only. Addresses are TU3 3.0.3.0. Tags as in the voice-graph spec:
**[R]** our reading of the recomp asm, **[IMG]** image constant, **[TR]** confirmed in a clean recomp trace
(`dsp_20261002_185826`, `all_20261002_163809`), **[I]** inference, **UNCERTAIN** = do not treat as settled.

Throwaway local scripts (`owners.py` groups trace modules per graph, `senda_bank.py` joins voice
Send A levels to banks, `presets.py` dumps the 24 reverb presets, `strs.py` reads image strings). Asm dumps kept locally too.

---------------------------------------------------------------------------------------------------------
## 0. Answer in one paragraph

`[[0x830CFDEC]+52]` is the input Sub0 of a **mono "EnvSendSub" bus** built by the bus manager constructor
`sub_8248EEE8`. It is never zero after audio init, so **every standard voice has Send A** (confirmed in traces:
108–1920 voice graphs per session carry `… LPF SEND1 GAIN …`). The env bus feeds a fixed 15-graph network: two
parallel "sides" (A/B, for crossfading presets), each = EQ stage → one **ReverbModel1** (Schroeder: 6 combs + allpass)
with a pre-delay, panned to the left and right surrounds-ish (270° / 90°), plus two **feedback echo taps**
(0.04–0.3 s, LFO-wobbled delay and pan): all summed into the 6-ch "SFX Master" bus. Parameters come from 24 vault
presets (`aud_reverb/reverbNN`, class `204CAC1FD77088B8`), chosen by the world-painter `audio_reverb` region layer
(default `reverb01`) or a type-5 reverb-zone emitter, crossfaded by moving the env send between side A and B. A
voice's env send level = VOL × FXWET0 (property 5). For emitters FXWET0 is the MixMap send word (out8 = −2600 mB +
distance roll-off B0), so a nearby emitter carries a wet tail/echo and a far one loses it. The user-facing symptom
("ambience/cars don't sound distant") is consistent with the port having no env bus at all.

---------------------------------------------------------------------------------------------------------
## 1. Construction (who builds what) [R]

Audio boot `sub_826DD3A8` allocates the bus manager (1156 bytes) into global **0x830CFDEC**, sets
manager+104 = **1.0** (global env-send scale), +388/+392 = 0.0, and calls in order:
1. `sub_8248EEE8`: the **environment network** (this spec);
2. `sub_82490270`: the two **"FlangeSub" effect returns** (§7);
3. `sub_824907D8`: fills 14 **LFO records** and registers the per-block **"LFO Task"** `sub_82490B60` (§5);
4. `sub_82490CA0`: the material buses (not part of this spec).

Graph creation is `sub_82B48C48(system, order, module_count, module_list)`; a module-list entry is
{params, descriptor, channel_count}. Module descriptors are looked up by FourCC: Sub0, Sen0, Gai0, PI20 (PeakingIir2),
Del0 (Delay), RM10 (ReverbModel1), HI20/LI20 (HighPass/LowPassIir2), Pn21 (Pan2D1). Bus names are image strings at
0x8224DC24.. (`EnvSendSub`, `EnvSub`, `RvrbSub`, `rvrbfiltsub`, `rvrbdelaysub`, `FlangeSub`).

| graph (name) | order | ch | modules (index: module) | manager slot (graph / module array) | count |
|---|---|---|---|---|---|
| **EnvSendSub** | 199 | 1 | 0 Sub0 · 1 Sen0 · 2 Sen0 | +0 / **+52** | 1 |
| **EnvSub** (side A, B) | 200 | 1 | 0 Sub0 · 1 Gai0 · 2 PI20 · 3 Sen0 · 4 Sen0 · 5 Sen0 | +4/+56 (A), +8/+60 (B) | 2 |
| **RvrbSub** (A, B) | 201 | 1 | 0 Sub0 · 1 Del0 (max 0.16 s) · 2 RM10 · 3 Gai0 · 4 Sen0 · 5 Sen0 | +12/+64, +16/+68 | 2 |
| **rvrbfiltsub** ×4 | 202 | 1 → 6 | 0 Sub0 · 1 HI20 · 2 LI20 · 3 Gai0 · 4 Pn21 (6 out) · 5 Sen0 (6 ch) | +20/+72, +24/+76 (A), +28/+80, +32/+84 (B) | 4 |
| **rvrbdelaysub** ×4 (echo taps) | 201 | 1 → 6 | 0 Sub0 · 1 Del0 (max 0.46 s) · 2 HI20 · 3 LI20 · 4 Gai0 · 5 Pn21 (6 out) · 6 Sen0 (6 ch) | +36/+88, +40/+92 (A), +44/+96, +48/+100 (B) | 4 |

Del0 max-delay constructor values: 0.16 s at 0x82098E40, 0.46 s at 0x8208ECE0 [IMG]. RM10 gets no constructor
parameter, so its max space size is the descriptor default 80 m [IMG 0x82FD2060 = 80.0 double].

**Links** (Sen0 configure id 0 = "send to bus"):

```
EnvSendSub.1 → EnvSub A          EnvSendSub.2 → EnvSub B
EnvSub A.3 → RvrbSub A           EnvSub A.4 → rvrbdelaysub A1 (+88)   EnvSub A.5 → rvrbdelaysub A2 (+92)
EnvSub B.3 → RvrbSub B           EnvSub B.4 → rvrbdelaysub B1 (+96)   EnvSub B.5 → rvrbdelaysub B2 (+100)
RvrbSub A.4 → rvrbfiltsub A1 (+72)   RvrbSub A.5 → rvrbfiltsub A2 (+76)
RvrbSub B.4 → rvrbfiltsub B1 (+80)   RvrbSub B.5 → rvrbfiltsub B2 (+84)
every rvrbfiltsub.5 and rvrbdelaysub.6 (6 ch) → default bus [[0x830CFDBC]+44] = "SFX Master"
```

Pass order: 199 < 200 < 201 < 202 < 254 (SFX Master), so the whole chain resolves inside one 256-frame block
(voices order 0 → … → SFX Master 254 → mastering 255). No extra block latency per hop [I, voice-graph spec §1].

**Other contributors to the env bus** [R] (all send their Sen0 to `[[manager+52]]`): every standard voice (Send A,
§4); both FlangeSub returns (§7); the board grain chain graph 2 (`sub_824C8878`, level vfunc60(13)/32767);
"Splicer Wet SubM" (`sub_82488DD0`, order 4, 6 ch: Sub0 → Sen0 env → Sen0); "FootStep SubMix" (`sub_82494188`);
"Collision SubMix" (`sub_824D25E0`); `sub_82498248` (order 1, 2 ch, env level initially 0.0); `sub_824CE108`.
Their runtime send levels are **UNCERTAIN** (not traced per graph here; the generic SEND hook sees them).

**Trace confirmation** [TR, `dsp_20261002_185826`]: graph 0x40C28020 = EnvSendSub (`SEND1 SEND1`, levels sweeping
0..1); 0x40C18420 / 0x40C18820 = EnvSub A/B (`GAIN PEAK SEND1 SEND1 SEND1`); 0x40C30020 / 0x40C30820 = RvrbSub
(`GAIN SEND1 SEND1`, gain 1.0–2.0); 0x40C31020 / 0x40C31820 / 0x40C38020 / 0x40C386E0 … = rvrbfiltsub /
rvrbdelaysub (`HPF LPF GAIN SEND6`); 0x40C40020 / 0x40C40770 = FlangeSub (`PEAK SEND1 GAIN SEND6`). Del0, RM10 and
Pn21 are not hooked.

---------------------------------------------------------------------------------------------------------
## 2. Signal flow of one side (what to implement)

```
voice Send A (N ch → 1 ch: 1→1 copy, 2→1 L+R, 4→1 sum, 6→1 L+C+R+Ls+Rs; all unity, LFE dropped)
 + other env contributors
      │  EnvSendSub (mono)
      ├─ Sen0 level = wA (side A blend weight) ─┐
      └─ Sen0 level = wB ───────────────────────┤ (the other side is identical)
                                                ▼
 EnvSub: Gai0 g_in → PI20 (f_eq, gain_eq, Q_eq; f_eq LFO-swept)
      ├─ Sen0 × (s_rev · 1.0)  → RvrbSub: Del0 (predelay, fb) → RM10 (T60, size, brightness) → Gai0 g_rev
      │                                   ├─ Sen0 1.0 → rvrbfiltsub L: HPF → LPF → Gai0 → Pn21 az 270°, D 1, LFE 0.5 → SFX Master
      │                                   └─ Sen0 1.0 → rvrbfiltsub R: HPF → LPF → Gai0 → Pn21 az 90°,  D 1, LFE 0.5 → SFX Master
      ├─ Sen0 × (s_tap1 · 1.0) → rvrbdelaysub 1: Del0 (d1, fb1; d1 LFO) → HPF → LPF → Gai0 → Pn21 (az LFO about 270°) → SFX Master
      └─ Sen0 × (s_tap2 · 1.0) → rvrbdelaysub 2: Del0 (d2, fb2; d2 LFO) → HPF → LPF → Gai0 → Pn21 (az LFO about 90°)  → SFX Master
```

- "· 1.0" = manager+104, a global scale written 1.0 by the constructor; no other writer found among the manager's
  users (UNCERTAIN whether an option changes it).
- Route rows for the mono send are from the image route tables 0x820ED700 (ranges) / 0x820ED780 (route bytes) [IMG]:
  2→1 = routes {0→0, 1→0} gain 1.0; 6→1 = sources 0..4 → 0 at 1.0 (LFE not routed).
- The Pn21 instances are 1-in/6-out bus panners: angle in degrees, positive = right (voice-graph spec §4.9), so
  **270° = −90° = hard left side (between L and Ls), 90° = right side**. Pan2D1 parameter 1 (distance D) = 1.0 (on the
  speaker circle), parameter 6 (LFE send) = **0.5** (written raw: the reverb/echo outputs feed the LFE at 0.5).
- Sen0 levels on RvrbSub (→ filt) and the final Sen0s are never posted → constructor default 1.0 [TR: SEND 1.0].
- PI20 on EnvSub: bypassed whenever its gain is exactly 1.0 (voice-graph spec §4.10), which is the case for many
  presets (e.g. reverb01 gain_eq = 1.0).

---------------------------------------------------------------------------------------------------------
## 3. Parameter application: `sub_8248DD18(manager, block)` [R]

`block` is a 212-byte record (copied to manager+868, flag manager+1080 = 1). Offsets (bytes), the module/parameter
each goes to (side s = block+0: 0 = A, 1 = B), and the preset-record field it comes from (`sub_824DE140`, record =
the vault preset, §6). Parameter ids are the module's attribute index.

| block | target | from preset record (offset, hash) |
|---|---|---|
| +0 | side s (int) | n/a |
| +4 | EnvSendSub Sen0 (1 + s) level = side blend weight | 0 at preset load, then the crossfade (§6.3) |
| +8 | EnvSub Gai0 p0 (g_in) | +84 `C76E3B36EC05F9E0` |
| +12 | EnvSub PI20 p0 (f_eq, Hz), then LFO-driven (§5) | +80 `2DA3FDBA90F64924` |
| +16 | PI20 p1 gain_eq: posted only if 0.1 ≤ v ≤ 20 [IMG 0x820641A8, 0x820996EC], else **0.1** | +76 `DECBDDB58AC61519` |
| +20 | PI20 p2 Q_eq: posted if 0.2 ≤ v ≤ 20 [IMG 0x82099280], else **0.2** | +64 `2EBE3CBF5E339625` |
| +24 | EQ LFO depth (fraction) | +72 `8249454FFE32EA2B` |
| +28 | EQ LFO rate Hz: period = 1/rate if rate ≥ 0.1, else period **10 s** [IMG 0x821963E4] | +68 `533ED00D3783EF02` |
| +32 | EnvSub Sen0 3 (→ reverb) level × manager+104 | +52 `578AB8D6BF494236` |
| +36 | EnvSub Sen0 4 (→ tap 1) level × manager+104 | +60 `B3BB9F5129BBCFB5` |
| +40 | EnvSub Sen0 5 (→ tap 2) level × manager+104 | +56 `C8A4239F7CBBF072` |
| +44 | RvrbSub Del0 p0 pre-delay s | +12 `66588E403F254F64` |
| +48 | RvrbSub Del0 p1 feedback | +16 `8968075E7CBACC4E` |
| +52 | RM10 p0 reverb time s | +20 `FA81EA055905EB17` |
| +56 | RM10 p1 space size m | +24 `2490987C531FB4E9` |
| +60 | RM10 p2 brightness | +32 `1812369BC788F71F` |
| +64 | RvrbSub Gai0 p0 (g_rev) | +28 `5075CFC3C70ACB30` |
| +68 / +72 / +76 | filt L: LI20 Hz / HI20 Hz / Gai0 | +36 `D548E9…` / +40 `4C50D4…` / +44 `4ABF29…` |
| +80 / +84 / +88 | filt L Pn21 p0 / p1 / p6 | constants **270.0** [IMG 0x82063B20] / 1.0 / 0.5 |
| +92 / +96 / +100 | filt R: LI20 / HI20 / Gai0 | +0 `48376C…` / +4 `4E7CDD…` / +8 `22FB42…` |
| +104 / +108 / +112 | filt R Pn21 p0 / p1 / p6 | **90.0** [IMG 0x82256FD8] / 1.0 / 0.5 |
| +116 / +120 | tap 1 Del0 p0 delay s / p1 feedback | +168 `B388DA…` / +172 `EECADF…` |
| +124 / +128 | tap 1 delay-LFO depth / rate Hz (rate < 0.01 [IMG 0x820D71E8] → period 10 s) | +156 `7B2416…` / +152 `843273…` |
| +132 / +136 / +140 | tap 1 LI20 / HI20 / Gai0 | +148 `7492F9…` / +160 `D6812F…` / +164 `796D83…` |
| +144 / +148 / +152 | tap 1 Pn21 p0 / p1 / p6 | 270 / 1.0 / 0.5 |
| +156 / +160 | tap 1 pan-LFO depth / rate | +92 `30DCBF…` / +88 `27D289…` |
| +164 / +168 | tap 2 Del0 delay / feedback | +128 `433B43…` / +132 `FC5073…` |
| +172 / +176 | tap 2 delay-LFO depth / rate | +116 `024737…` / +112 `EDA354…` |
| +180 / +184 / +188 | tap 2 LI20 / HI20 / Gai0 | +108 `3E80A0…` / +120 `E9E26E…` / +124 `81504B…` |
| +192 / +196 / +200 | tap 2 Pn21 p0 / p1 / p6 | 90 / 1.0 / 0.5 |
| +204 / +208 | tap 2 pan-LFO depth / rate | +92 / +88 (same fields as tap 1) |

Preset fields never read by this path: +48 (int index = the NN of reverbNN), +96, +100, +104 (270.0), +136, +140,
+144 (90.0): the authored pan constants are ignored in favor of the code constants (same values).

A lighter per-frame re-post `sub_8248D770` (called through `sub_824DE400`) sends only: the side weight, f_eq,
filt L/R pans (+ rotated, §6.3) and gains, tap 1/2 delays and the fixed tap pans 270/90.

---------------------------------------------------------------------------------------------------------
## 4. Voice side (FXWET0 → Send A) [R][TR]

- Voice open (`sub_824A3140`): if `[[manager+52]] ≠ 0` (always, after init) a Send A module is inserted before Gain
  and linked to the EnvSendSub Sub0; level posted 0 at open; then level = VOL/32767 × FXWET0/32767 on every change of
  property 2 or 5 (voice-graph spec §3.2).
- **Retail evidence** (`all_20261002_163809`, `senda_bank.py`): non-zero Send A on player sounds, footsteps,
  collisions, foley, rolling, wheels, grinds and on world banks (fountains_waterlaps_left, helicopter_1, Jet_Passby_1,
  bus_by_3, truck_horn_city_2). Typical levels: median 0.011–0.027, max ≈ 0.07 (−39 … −23 dBFS re 1.0), i.e. the
  wet path sits ≈ 10–20 dB under the dry (send/gain ratio median 0.17–0.5).
- **Emitters**: `c_emitter` payload w2 → property 5 (PoC probe, `ems-emitters-re.md`); the game writes w2 = MixMap
  out8 × emitter level (positional: −2600 mB + C1 + B0 dB, B0 = camera distance 4 → 70 m, 1−sin) or out7 (−2100 +
  C1, non-positional). So near an emitter the env send is ≈ −26 dB × shape level, falling to silence by 70 m;
  the dry word (out4, −600 mB) has no distance roll-off.
- **Ambience bed** (`SnP1 → Rch0 → Rsp0 → Gai0 → LI20 → Sen0`): no Send A → **the zone bed is never reverberated**.
  `c_main_ambience_crossfade` voices are ordinary AEMS voices (Send A exists); their w2 is not written by
  `sub_824D0F38` (w1, w4, w5, w9, w0/w8 only) → send 0 unless the bank program sets it (UNCERTAIN, check the bank).
- Player components' "first send" levels: grain chain graph 2's env Sen0 (vfunc60(13)) and the owner one-shot bus
  feed this same env bus (§1 contributors). The 4096/8192/16384 routing records feed the FlangeSub returns (§7),
  whose output also re-enters the env bus.

---------------------------------------------------------------------------------------------------------
## 5. The LFO task: `sub_82490B60` [R][TR]

Registered by `sub_824907D8` (`sub_82481BE0`, task list 0, name "LFO Task"); called **once per mixer block with
dt = 256/48000 s** [TR: the EnvSub PI20 frequency moves 0.777 Hz per block near the sweep center for reverb02
(1161 Hz, depth 0.2, rate 0.1 Hz) = 232 Hz × 2π × 0.1 × 5.333 ms]. It walks **14 records** of 32 bytes at
manager+396 (+32 each):

| +0 | +4 | +8 | +12 | +16 | +20 | +24 | +28 |
|---|---|---|---|---|---|---|---|
| period s | phase s | amount | center | depth | last output | module | parameter id |

Per block: `phase = fmod(phase + dt, period)` (truncating), `out = centre + depth × amount × sin(2π · phase / period)`
(2π = 6.2831855 [IMG 0x820B411C]); posted to the module parameter every block, even when unchanged.

| rec | module (param) | set by preset (§3) | default (`sub_824907D8`) |
|---|---|---|---|
| 1 / 6 | EnvSub A / B PI20 (p0 f_eq) | period 1/rate_eq, amount depth_eq, center = depth = f_eq, phase 0 | period 1, amount 0.5, center = depth = 1.0 |
| 2 / 7 | tap 1 Del0 A / B (p0 delay) | period 1/rate_d1, amount depth_d1, center = depth = d1 | center = depth = 0.3 |
| 3 / 8 | tap 2 Del0 (p0) | period 1/rate_d2, amount depth_d2, center = depth = d2 | 0.3 |
| 4 / 9 | tap 1 Pn21 (p0 azimuth) | period 1/rate_pan, amount depth_pan, center 270, depth 90, phase 0 | 0.3 / 0.3 (pre-preset) |
| 5 / 10 | tap 2 Pn21 (p0) | period 1/rate_pan, **phase = period/2** (opposite sweep), amount depth_pan, center 90, depth 90 | 0.3 / 0.3 |
| 11, 13 | FlangeSub A / B Del0 (p0) | flange preset (§7) | center = depth = 0.006 s |
| 12, 14 | FlangeSub A / B PI20 (p0) | flange preset | center = depth = 1.0 |

So, effectively:
- f_eq(t) = f_eq · (1 + depth_eq · sin(2π·rate_eq·t));
- d1(t) = d1 · (1 + depth_d1 · sin(2π·rate_d1·t)): the echo taps' delay times wobble in whole-sample steps, each
  step a 128-sample tap crossfade (§8.3), e.g. reverb01 tap 1: 0.28 s ± 50 % at 0.51 Hz;
- tap 1 azimuth = 270 + 90·depth_pan·sin(2π·rate_pan·t), tap 2 = 90 + 90·depth_pan·sin(… + π): the two echoes
  swing around the left/right sides in opposite directions.
- Record values are rewritten whenever a preset is applied to that side (phase reset to 0 except recs 5/10).
- Because the task posts every block, any direct post of those parameters (e.g. +144/+192 pans) is overwritten
  within one block.

---------------------------------------------------------------------------------------------------------
## 6. Presets and zone selection

### 6.1 Presets (vault class `204CAC1FD77088B8`, 24 collections) [R][IMG]
Collection key = lookup8 hash of `reverbNN`; field +48 holds NN (1..24). Lookup `sub_824DDC10`. Full table:
`presets.py`, a local script (data stays local). Key columns (reverb01 = **default**, key
`A2782D75A971CC8C`):

| preset | T60 s | size m | bright | pre-dly s | g_in | s_rev | s_tap1/2 | taps d1/d2 s (fb) | tap gain | filt L/R LPF–HPF | EQ f/gain/Q |
|---|---|---|---|---|---|---|---|---|---|---|---|
| reverb01 | 1.5 | 70 | 0.7 | 0.08 | 0.5 | 0.25 | 1 / 1 | 0.28 (0.25) / 0.30 (0.30) | 0.75 / 0.75 | 4000–500 / 4000–600 | 1161 / 1.0 / 3 |
| reverb02 | 1.04 | 8.3 | 1 | 0.08 | 0.75 | 1 | 1 / 1 | 0.17 / 0.18 (0.2) | 1 / 1 | 10000–0 | 1161 / 0.1 / 20 |
| reverb03 | 2 | 23.9 | 0.5 | 0.1 | 0.5 | 1 | 0.5 / 0.5 | 0.23 / 0.22 | 1.5 | 6000–774 / 6000–700 | 2000 / 0.1 / 3 |
| reverb04 | 1 | 45 | 0.5 | 0.15 | 0.5 | 0.5 | 1 / 1 | 0.15 (0.2) / 0.151 (0.22) | 1.5 | 8000–400 | 1935 / 0.1 / 10 |

(others in the dump; region coverage: reverb01 Downtown 23 / Industrial 15 / University 128 tiles, reverb03
Industrial 165, reverb04 Downtown 74, reverb16 Downtown 69, …; parks have their own.)

### 6.2 Which preset (SFXObj_Reverb update `sub_824DE548`, per game frame, dt in f1) [R]
State: +400 current key, +408 target key, +416 current side, +420 target side, +424 zone side, +428/+432 zone
emitter nodes, side records at +28 + 184·side (record pointer +32, blend weight +48, mode +128, timer +204, fade
length +208).
1. If a transition is in progress (+400 ≠ +408), run its mode handler: 0 → timed fade `sub_824DE850`;
   1 → zone fade `sub_824DE970`; 2 → zone-to-zone `sub_824DEAE8`; ≥ 3 → finish.
2. Otherwise query the reverb-zone emitters (`sub_82488278`: first active `.ems` node whose attribute type = 5):
   - **none**: key = region record `*(0x83083C38)+0x2F0B0+16` (world-painter `audio_reverb` layer at the focused
     skater's x,z, `ems-emitters-re.md`), or `A2782D75A971CC8C` (reverb01) when that is 0; if it differs from the
     current key, start a **timed fade (mode 0)**.
   - **a zone** (first time): its attribute record's preset key → start a **zone fade (mode 1)**.
   - a second overlapping zone with a different preset → mode 2.
3. Starting a transition (`sub_824DE468`): target side = 1 − current side; load the preset onto that side with
   weight 0 (`sub_824DE140` → full `sub_8248DD18` post); timer = 0, fade length = **1.0 s** [IMG 0x8231A844].

### 6.3 Crossfades [R][TR]
- **Timed (region change)**: timer += frame dt; x = min(timer / 1.0, 1); new side weight = x, old side = 1 − x
  (linear), re-posted every frame through `sub_8248D770`. At x = 1 the new side becomes current. [TR: EnvSendSub
  levels step ≈ 0.0167 per 16.7 ms frame, 0.0055 → 0.28 in 268 ms.]
- **Zone (mode 1)**: w = the zone emitter's normalized distance d (node +12, 0 at the inner core, 1 at the edge,
  `ems-emitters-re.md` "Inner core s0"); zone side weight = 1 − w, other side weight = w (linear in d, not the
  emitter's level curve). While blending, the reverb outputs' pans are rotated toward the zone (`sub_824DEEF0`):
  with θ = horizontal angle from the camera forward to the zone center (degrees, 0..360; camera from
  `*(0x83083C38)+0x2F078` +48 pos / +64 forward; exact angle function `sub_82453298` UNCERTAIN), the zone side's
  filt L azimuth = 270 + w·(θ' − 270) and filt R = 90 − w·(90 − θ''), wrapped into 0..360 (θ', θ'' = θ unwrapped
  towards each side). At the zone center (w = 0) the reverb is the normal left/right pair; toward the edge both
  outputs collapse onto the zone's direction. The other side keeps 270/90. When d is exactly 0 the zone side becomes
  current (weights 1/0, pans 270/90); when the zone is no longer found, `sub_824DEDD8` snaps the weights to one
  side (1/0) and clears the zone nodes.
- Mode 2 (two overlapping zones): same blend between the two zone presets using the second zone's d. UNCERTAIN in
  detail (read `sub_824DEAE8` again before porting).
- The first preset is applied ≈ 5.5 s after the env network is built [TR 6119 ms]; before that both sides hold
  constructor defaults with EnvSendSub sends at 1.0 (but no voices play yet).

---------------------------------------------------------------------------------------------------------
## 7. FlangeSub effect returns (`sub_82490270`) [R][TR]

Two graphs, order 150, 1 ch: `Sub0 → Del0 (max 0.015 s) → PI20 → Sen0 (→ env bus) → Gai0 → Pn21 (6 out) → Sen0
(→ SFX Master)`; manager +108/+116 (A) and +112/+120 (B). These are the targets of the voice routing records 4096 /
8192 (A, `[[manager+116]]`) and 16384 (B, `[[manager+120]]`) (voice-graph spec §3.3). The Del0 delay is LFO-swept
around 6 ms and the PI20 frequency is swept (LFO recs 11–14) → a **flanger**, not a slap-back. Its first Sen0 sends
the flanged signal into the env bus. Preset: `sub_824DDF58` reads two vault records (class lookup on key
`4CDD7CDC1A955D5C`; type 12288 → A, 16384 → B) through `sub_8248FE68` (LFO templates + levels at manager+124..192),
posted by `sub_8248FFF8` (Pn21/Sen0/Gai0 of each return). [TR: 0x40C40770 PEAK sweeping 1.03 … 1800 Hz, gain 1.0
(→ bypassed), Q 3; env send 0.07–1.0.] Field meanings beyond this are **UNCERTAIN**.

---------------------------------------------------------------------------------------------------------
## 8. DSP modules new to the port

### 8.1 Delay (Del0): attributes from the image help strings [IMG 0x820F6348..]
- Constructor CONSTRUCTORPARAM_MAXDELAY (s): buffer size; larger requests reallocate.
- p0 ATTRIBUTE_SETDELAYTIME (s), "a delay of 0 turns the plug-in off".
- p1 ATTRIBUTE_SETFEEDBACK, range −0.99 … 0.99.
- "Dynamically changing the delay time and feedback amount by smoothly cross fading from one set of parameters to
  another … over a 128 sample period."
- DSP details: see §8.3 (filled from the asm).

### 8.2 ReverbModel1 (RM10): attributes [IMG 0x82116294..]
- Constructor CONSTRUCTORPARAM_MAXSPACESIZE (default 80 m).
- p0 ATTRIBUTE_SETREVERBTIME (s): T60; **0 = off and the module outputs nothing**; non-zero values below 0.366 s are
  clamped to 0.366.
- p1 ATTRIBUTE_SETSPACESIZE (m): average distance between opposite reflecting surfaces.
- p2 ATTRIBUTE_SETBRIGHTNESS: scales the cut-off of the low-pass in each comb feedback loop; 1 = the model default.
- Structure (help text): mono in, **6 parallel comb filters followed by series all-pass filter(s)**; per output
  channel slightly different all-pass parameters for spatialisation; the LFE is not fed. Object size 1104 bytes
  (`sub_82B2EC00`), constructor `sub_82B2EC08`.

### 8.3 DSP details from the asm [R] (2026-10-02; local asm dumps `rm/`, `dl/`, `del/`;
model script `rm_model.py`)

**Delay (Del0)**: GetSize 0x82B21F80 (192 B), CreateInstance `sub_82B22008`, **Process `sub_82B222D8`**, timer
`sub_82B22228` (same task scheduler as the LFO task), kernels plain `sub_82B3CF58` / crossfade `sub_82B3D0A8`.
- +52 p0 delay s, +60 p1 feedback, +72 max delay (ctor), line at +100 sized round(max·48000); the timer grows it if
  p0 exceeds max.
- D = round-half-away(p0 · 48000): **whole samples, no fractional read**. Feedback: |p1| > 0.99 → ±0.99
  (0x820ED5E8 / 0x822F8E60); NaN passes.
- State +64: 0 = **bypass** (module skipped, signal passes undelayed); on D > 0: reset filter + positions, set fb,
  → state 1. State 1: D ≤ 0 → state 0; buffer filled < D → state 2 (keep old D until filled); D changed → crossfade.
- Plain kernel (wet only): w[n] = x[n] + fb·w[n−D]; y[n] = w[n−D].
- **Crossfade on a D change**: 128 samples, r = 127/128 … 0 (step 1/128, 0x820300D4), finished within the block it
  starts in: y = (1−r)·t_new + r·t_old; w = x + (1−r)·fb_new·t_new + r·fb_old·t_old. A feedback change outside a
  crossfade applies instantly.
- Under the LFO (§5) every block whose rounded D changes starts one 128-sample tap crossfade → stepped integer taps
  (slight phasing/zipper), **not a Doppler pitch glide**.

**ReverbModel1 (RM10)**: GetSize 0x82B2EC00 (1104 B), CreateInstance `sub_82B2EC08`, ctor `sub_82B2E788`,
**Process `sub_82B2F8C0`**, configure `sub_82B2F590`, distances `sub_82B2FE00`, delays `sub_82B2FEA8`, G1
`sub_82B2F2C8`, allpass setup `sub_82B2FF88`, apply `sub_82B2F150`, timer `sub_82B2F038`; comb kernel
`sub_82B399D0` (the "one-pole stage" of the voice-graph spec), allpass kernel `sub_82B389A0`.
- **Distances** (on size change): S = clamp(p1, 2.0, 83.3) (written back); near = 0.8·S, far = 1.5·near; if far >
  100 → far 100, near 66.666664; six distances = near, near + k·0.2·(far − near) for k = 1..4, far.
- **Delays**: x = 48000·d·0.0029002321 (1/344.8 m/s, 0x822F87C8); each delay = the first prime in the table
  0x821147D0 (1652 floats, 2…13999) strictly greater than x, the search continuing from the previous comb (six
  distinct primes). reverb01 (70 m) → 7817, 8581, 9371, 10139, 10937, 11699; 8.3 m → 929 … 1399.
- **g1 (damping)** at 48 kHz: per comb, linear interpolation over distance knots 0, 12.5, …, 100 (0x821161B0) of
  0.08·row25k + 0.92·row50k (rows 0x8211621C {0,.12,.23,.30,.35,.40,.43,.46,.50} and 0x82116264
  {0,.31,.45,.53,.57,.61,.64,.67,.70}). **Brightness quirk**: only when p2 differs from the last applied
  brightness (constructor 1.0): b = max(p2, g1[5] + 0.001); every g1 ×= 1/b; comb states zeroed. With unchanged
  brightness g1 is not divided.
- **g2 (decay)**: T = max(p0, 0.366); g2ᵢ = (1 − g1ᵢ)·(1 − 0.366/T). Loop DC gain = g2/(1 − g1) = 1 − 0.366/T.
- **Comb i** (Moorer, lowpass in the loop): sᵢ[n] = x[n] + g2ᵢ·sᵢ[n−Dᵢ] + g1ᵢ·sᵢ[n−1];
  outᵢ[n] = (sᵢ[n−Dᵢ] − g1ᵢ·sᵢ[n−Dᵢ−1]) / 6. Sum of the six.
- **Allpass** (mode 1 = mono, our case): one section g = 0.7, D = round(48000·0.006) = 288 (0x821161AC,
  0x82116288): a[n] = c[n] − 0.7·a[n−288]; **y[n] = 2.0·(0.7·a[n] + a[n−288])** (mix gain 2/ch = 2.0).
  Modes 2/4 use 0.63/320 + 0.7778/259 and copy across channels (not used here).
- **Output** y = 2·AP(Σ combs(x)): wet only; RvrbSub's Gai0 follows.
- **p0 ≤ 0** → output zeroed (state 0). A parameter change sets state 1: the current block still runs the old network
  (or silence if coming from state 0), configure runs at the end of Process, and the new gains/line sizes are applied
  at the next **timer** tick (state 2 → 3 → 4). No crossfade on a reverb change.
- Comb lines start at size 0 (no max-space ctor parameter) and are resized at the first apply to D + 3.

**DSP UNCERTAIN**: comb tap alignment (sᵢ[n−D] vs [n−D−1]); the module timer period (list init arg 74), i.e. how
many blocks until a new preset is audible; whether resizes/resets clear line contents; Del0 first-enable output
before D samples are buffered; bit-exact f32 order of the vectorized comb/allpass kernels.

---------------------------------------------------------------------------------------------------------
## 9. Master path and output [R][TR]

- Voices' final Send (6 ch) and every env/echo output go to **"SFX Master"** = `[[0x830CFDBC]+44]`, built by
  `sub_828DDF78`: order 254, 6 ch, `Sub0 → DCl0 → Gai0 → LI20 → HI20 → Sen0`. [TR graph 0x40C18020: Gain 1.0,
  LPF 25000 (open), HPF 0 (off), unchanged in a 58 s session.] DCl0 level not traced (UNCERTAIN; bypass when ≥ 100).
- SFX Master's Sen0 → the **mastering graph** `[[system+8]+52]`, built by `sub_828DD9F0`: order 255, **8 ch**,
  `Sub0 → Scp0 → Dac0` (Dac0 parameter 2 = 13521.86 [IMG 0x822787CC], a load/budget figure, not a gain). "Music
  Master" and "Speech Master" (`sub_828DDC58` / `sub_828DDDE8`, order 200) are separate 3-module graphs.
  This settles voice-graph spec §6.2's "8-plane master" inference.
- **0x8306705D** (output channel count) is written only by the Dac set-up `sub_82B20EF8`: **always 6** once the
  output device is opened. The same function posts Dac parameter 0 = 3.0 / 5.0 / 1.0 depending on the console
  speaker-config flags (bit 16 / bit 0 / neither): a speaker-mode code, not a gain (UNCERTAIN meaning).
- No master gain was found on this path besides SFX Master's Gai0 (1.0).

---------------------------------------------------------------------------------------------------------
## 10. Implementation plan (native)

1. Build the 15 graphs of §1 with their orders, the Sub0 accumulators and the Sen0 links; the default bus becomes
   SFX Master (6 ch, gain 1, LPF/HPF open) feeding an 8-ch mastering Sub0 → Dac (6 out).
2. Voices: always insert Send A (route N → 1 per §2), level = VOL·FXWET0 with the usual 64/65 ramp.
3. Modules: Delay (§8.1/8.3), ReverbModel1 (§8.2/8.3), PeakingIir2 (exists in spec), Pan2D1 as 1-in/6-out bus panner
   with LFE parameter 0.5.
4. Preset engine: load the 24 presets from the vault at setup (asset pipeline exports the table; no game data in
   git), region `audio_reverb` lookup (already have region tiles) with default reverb01, the 1 s linear side
   crossfade, the zone blend from type-5 `.ems` nodes, and the 14-record per-block LFO.
5. Apply posts at the same cadence: presets on key change, weights per game frame, LFO per block.
6. Validation: per-graph parameter timelines against the trace (EnvSendSub weights, EnvSub GAIN/PEAK, RvrbSub GAIN,
   filt/delay HPF/LPF/GAIN; §1 owners), then an impulse through the whole network at reverb01 compared with a
   recomp capture of a single isolated click (short scripted run, if ever needed).

---------------------------------------------------------------------------------------------------------
## 11. UNCERTAIN / open
1. Runtime send levels of the non-voice env contributors (Splicer Wet, FootStep/Collision SubMix, `sub_82498248`,
   grain graph 2): hook their Sen0 posts.
2. Whether anything other than the constructor writes manager+104 (global env scale).
3. Mode-2 (zone-to-zone) blend details and the exact θ function of the zone pan rotation (`sub_82453298`).
4. FlangeSub preset fields and which banks route 4096/8192/16384.
5. SFX Master DCl0 level; Dac parameter 0 meaning (3/5/1 by speaker config).
6. `c_main_ambience_crossfade` w2 (send): set by the bank program or 0.
7. See §8.3 for the DSP-level uncertainties.
