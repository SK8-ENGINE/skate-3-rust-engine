# Board grain bus chain (`sub_824C8878`): spec for the native port

Scope: the four per-player bus chains of `SFXObj_SkateBoard` (2 trucks × players A/B) and everything the owner posts
to them per frame: FrequencyShiftSsb, graph 3 (clip / wobble / shelf), the graph-1 level ramp, the gain wobbles, the
graph-2 sends. Extends `grain-player-spec.md` §3.1–3.2 (read that first). Written 2026-10-02 from the TU3 (title update 3) lifted asm
(`tools/recomp-code-search/fn.sh`), image constants (`img.py` (local tool)) and vault fields (`find_field.py`).
Port: `crates/skate-audio/src/dsp/fss.rs`, `dsp/shelf.rs`, `grain/chain.rs`, `grain/bed.rs` (work copy until merged).

Tags: [R] read in the asm, [IMG] image constant, [V] vault value, [TR] matched in a retail trace, [U] UNCERTAIN.

## 1. Chain topology (`sub_824C8878`) [R]

Plug-in ids looked up by FourCC: `Sub0`, `HI20`, `LI20`, `FSS0`, `Pn21`, `Sen0`, `Gai0`, `HS20`, `DCl0`. Each module
entry is {config ptr, plug-in, channel byte}.

| graph | order | modules (index) | notes |
|---|---|---|---|
| 1 | 2 | 0 Sub0 · 1 HI20 · 2 LI20 · 3 FSS0 · 4 Sen0 (→ graph 3) · 5 Gai0 · 6 Sen0 (→ graph 2) | mono; FSS config {0x7FF7FFF1, **0.0**} |
| 2 | 5 | 0 Sub0 · 1 Sen0 (→ `[[mgr+116]]`) · 2 Sen0 (→ `[[mgr+52]]` env) · 3 Pn21 (6 out) · 4 Sen0 (→ bus) | 1, 1, 1, 6, 6 ch |
| 3 | 3 | 0 Sub0 · 1 DCl0 · 2 Gai0 · 3 HS20 · 4 Sen0 (→ graph 2) | local player only (`[owner+16]+72`) |

- Owner slots per chain (stride 24 from `+1192`): +0 graph-1 modules, +4 graph 1, +8 graph-2 modules, +12 graph 2,
  +16 graph-3 modules, +20 graph 3. Player B of a truck = +24, truck 1 = +48.
- `mgr` = `[0x830CFDEC]` (bus manager). `+52` = EnvSendSub input (env/reverb network, `aems-env-bus-spec.md`);
  `+116` = **FlangeSub return A** input (`aems-env-bus-spec.md` §7), ported 2026-10-29 (`bus::flange`).
- Graph 2's last Sen0 target = `sub_82491108(mgr, enum of field A5D3ADA63608617F, [owner+28]+72)` = eEQChain 8 =
  the default bus (`aems-eqchain-buses-spec.md` §1).
- Posted at build: graph-2 Sen0 #1 level **0.0**; graph-1 Sen0 #4 (→ graph 3) level = `+1556` (current send);
  DCl0 p0 = `+1528` (0.09), HS20 p0 = `+1548` (5000), p1 = `+1552` (0.65). Never posted (Sen0 / Gai0 defaults 1.0):
  graph-1 Sen0 #6, graph-3 Sen0, graph-2 Sen0 #4, and both Gai0 until the wobble posts (§4).
- Built on every bind (two call sites); a rebuilt chain starts from module defaults: FSS phase/histories 0, Gai0 1.0.
- Pass order 2 < 3 < 5 within one block: graph 1 adds into graph 2's Sub0 first, then graph 3. The env send is
  mono and **before** the panner (graph 2 is mono until Pn21).

## 2. FrequencyShiftSsb (`FSS0`) [R][IMG]

Functions: process `sub_82B22898`, constructor `sub_82B22770`, size `sub_82B22738`, descriptor `0x82FCE2A4`
("FrequencyShiftSsb"); param 0 = shift Hz (`+52`, range ±96000 = doubles at `0x82FCE268`).

Per 256-frame block (mono, channel 0 only):
1. **Hilbert pair**: four biquads through the shared kernel `sub_82B43AF8` (the HPF/LPF/HS20 kernel; coefficient
   layout {a1, a2, b0, b1, b2}, history {x1, x2, y1, y2} at `+56/+72/+88/+104`):
   I = AP(`0x82FCE2B0`) → AP(`0x82FCE2C4`); Q = AP(`0x82FCE2D8`) → AP(`0x82FCE2EC`), both from the input.
   Fixed image constants (allpass: b0 = a2, b1 = a1, b2 = 1):

   | section | a1 | a2 |
   |---|---|---|
   | I1 | −1.9228750 (0xBFF620C5) | 0.92365128 (0x3F6C7469) |
   | I2 | −0.40179494 (0xBECDB811) | −0.21314885 (0xBE5A43B1) |
   | Q1 | −1.9677674 (0xBFFBDFCD) | 0.96786267 (0x3F77C5D9) |
   | Q2 | −1.2627572 (0xBFA1A207) | 0.34741214 (0x3EB1E001) |

   Design (f64 response): I leads Q by 90° ± 0.6° from 100 Hz to 1 kHz, 6.1° at 8 kHz, 10° at 15 kHz, falls apart
   above 18 kHz / below 50 Hz; image rejection −47 / −74 / −25 dB at 100 Hz / 1 kHz / 8 kHz.
2. **Oscillator**: Δ = f32(f32(shift / rate) × 2π) (`fdivs`, `fmuls`, 2π = `0x820B411C`); lanes {φ, φ+Δ (fadds),
   fma(Δ,2,φ), fma(Δ,3,φ)}, each group of 4 adds 4Δ (`vaddfp`). φ = `+260`.
3. **out = I·cos φ − Q·sin φ** (products rounded, then `vsubfp`). Sign checked: a tone at f moves to f + shift.
4. Trig: `sub_824531C8` = XMVectorSin, `sub_82473930` = XMVectorCos: V = fma(−2π, round_even(x·1/2π), x)
   (`vrfin`, constants `0x822F9850`: π, 2π, 1/π, 1/2π), then 11 Taylor terms (sin V³…V²³ `0x822F97C4..EC`, cos
   V²…V²² `0x822F97F4..981C`) with `vmaddfp`; power pairing as in `fss.rs` docs. Max error vs libm 6e-7.
5. Block end: e = fma(Δ, 256, φ); φ' = fma(−trunc(e·1/2π), 2π, e) (`fctiwz`, `fnmsubs`; sign kept, |φ| < 2π).
- No smoothing (the shift applies from the next block), **no bypass**: at 0 Hz out = I exactly (allpass copy).
- Construction param 1.0 (the default) adds a 64-tap band-limit FIR before the pair (`sub_82B42510` designs a
  windowed band-pass [max(0, −ωs), min(π, π − ωs)], `sub_82B41D58` runs it; size 296 + ch·256): **off** in this chain
  (config 0.0) → not ported.

## 3. Graph 3 modules

- **DCl0** (`sub_82B22678`): clamp ±0.09; bypass ≥ 100 / NaN (already `dsp::peaking::clip`).
- **HS20** (`sub_82B26740`, coefficients `sub_82B43D78`) [R]: ω = f32(fc/rate)·2π; bypass if ω ≥ CEIL (3.1384511)
  or gain == 1.0 (histories cleared once, the pair (ω, gain) cached); else ω raised to FLOOR, coefficients rebuilt
  when (ω, gain) ≠ cache. A = √gain, α = sin ω × 0.70710653 (`0x822F8E50`), RBJ high shelf S = 1 in this order:
  a0 = fma(√A·α, 2, (A+1) − (A−1)cos); b0 = A·((2√Aα + (A−1)cos) + (A+1))/a0; b1 = −2·A·((A+1)cos + (A−1))/a0
  (−2.0 = `0x82094178`); b2 = A·(((A−1)cos + (A+1)) − 2√Aα)/a0; a1 = 2·((A−1) − (A+1)cos)/a0;
  a2 = (((A+1) − (A−1)cos) − 2√Aα)/a0 (fused steps as written in `shelf.rs`). Plateau above 5 kHz = gain 0.65.
- **Gai0** (graph 3) = the wobble without the level ramp (§4).

## 4. Owner side, per frame (`sub_824C6A78` process, before the MixMap evaluation; dt = frame time, s) [R]

Order: `sub_824CA738` slope → `sub_824C6198` push envelopes (trigger, then advance both by dt) → **`sub_824C9058`**
→ … → `sub_824CA448` seam envelope → **`sub_824CAEC0`** → **`sub_824CB180`** (→ `sub_824CB078`).

### 4.1 `sub_824C9058` (per truck with +1328+t running and +1320+4t == 1)
- HI20 = vfunc64(12), LI20 = vfunc64(11) (both players' graph 1).
- Manual latch updated inline (+1504; set by state +340, cleared at wheel count 0 or 4); **the trick latch
  (`sub_824CA6E0`) is only evaluated when the manual latch is clear**.
- FSS A = (special ? `special_shift` [truck's collection, F62BC5EBD8E5DDE8] : 0) + (push-shift env not done ?
  `+1152` : 0), then if D (`+1508`) > 0: fma(primary-collection +80 = A shift/slope, D, ·).
- FSS B = truck collection +88 (B base shift) + (push shift if running), then if D > 0: fma(D, primary B shift/slope
  [7FFF3A8AD44809EF], ·).
- Pn21 (both) = raw(0) × 0.0054932479 (`0x822F8C64` = 360/65535). Env Sen0 (both) = level(13) × 1/32767.
- Local player: graph-2 Sen0 #1 = level(21)/32767 (A), level(22)/32767 (B).

### 4.2 Push shift envelope (`+1036`, value `+1152`, done `+1156`)
Retail's segment envelope (`sub_8248D368` reset → value 0, done; `sub_8248D3C0` add(start, end, ms): duration =
ms × 0.001 (≤ 0 → 0.01), first segment sets value = start; `sub_8248D498` append(end, ms): start = previous end;
`sub_8248D510` advance(dt): t += dt; if t > dur: value = end, next segment with t −= dur (or done); else linear
value = fma(end − start, t/dur, start); kinds 1/2/5 = quadratic / ease-out / table, unused here). Push: 0 → peak
(lerp(low, high, t) of the primary collection) over attack, hold, → 0 over return.

### 4.3 `sub_824CAEC0` (local player): send and level
kmh = state +208 × 3.6 (`0x822F8628`).
- send = kmh ≥ 70 ? 3.0 : kmh ≥ 46 ? ((kmh − 46)/(70 − 46)) × 3.0 : 0 → `+1556`; posted to graph-1 Sen0 #4 of all
  four chains only when it changes (and at every build). [TR exact]
- level `+1560` = kmh ≥ 74 ? 0.45 : kmh ≥ 52 ? fma(−(kmh − 52)/22, 1 − 0.45, 1) : unchanged (holds below 52).

### 4.4 `sub_824CB078` / `sub_824CB180`: the wobbles (local player)
Two records (A's chains `+1572`, B's `+1728`, stride 156): +0 ms_low, +4 ms_high (int), +8 low, +12 high, +16
previous target e, +20 graph-1 gain, +24 last posted, +28 graph-3 gain, +32 envelope (value +148, done +152).
Init (`sub_824C59C8`): e 0, gains 1, envelope reset (done → the first frame draws).
- Per record: if done → ms = ms_low + r % (ms_high − ms_low); range = trunc((high − low) × 100); target =
  fma(r % range, 0.01, low) (two title-generator draws, `sub_82A8AF10` = `0x82FD7D74`), × −1 if the previous e ≥ 0;
  reset; add(e, target, ms); e = target (no advance that frame). Else advance(dt).
- ramp = kmh ≥ 70 ? 1 : kmh ≥ 42 ? (kmh − 42)/28 : 0. g3 = fma(value, ramp, 1); g1 = g3 × level.
- Post loop: trucks 0, 1 × players A, B: if g3 ≠ last posted → last = g3, then (if the chain exists) graph-1 Gai0
  = g1, graph-3 Gai0 = g3. The compare value is per player, so **truck 1's chains never get a wobble** [TR], and a
  rebuilt chain keeps Gai0 1.0 until the next change (below 42 km/h: never; the held level is lost on a rebind).
- Non-local owners: `+1592`, `+1748`, `+1560` = 1.0, nothing posted.

### 4.5 Vault (class `6E878344774A7999`, `default`, read by `sub_824CA938`) [V]

| owner | hash | value | role |
|---|---|---|---|
| +1520 | 0D665393E2EDC605 | 46.0 | send start km/h |
| +1524 | 28E708782445747F | 70.0 | send end km/h |
| +1528 | E64C04ED542DABC8 | 0.09 | DCl0 level |
| +1532 | D900C07BE7C5450F | 3.0 | send max |
| +1536 | 88AA96B08FD16914 | 52.0 | level ramp start (exported `g1_level_start_kmh`) |
| +1540 | 3FFB5107C82BA3E0 | 74.0 | level ramp end (`g1_level_end_kmh`) |
| +1544 | D3E8894CA25A4F71 | 0.45 | level floor (`g1_level_floor`) |
| +1548 | 55BEB30353F244A9 | 5000.0 | HS20 corner |
| +1552 | 45516395725ED16B | 0.65 | HS20 gain |
| +1564 | 281A501B22B6CCDF | 42.0 | wobble ramp start |
| +1568 | 54CDE019E31FC04E | 70.0 | wobble ramp end |
| +1572 / +1576 | 36AE41817640FE04 / 71EE27313BD30F21 | 15 / 30 (int) | A segment ms |
| +1580 / +1584 | 437D128B53669C34 / 02885338DD5D7DCA | 0.0 / 0.30 | A |e| range |
| +1728 / +1732 | 2055BBF39C152FA9 / F5240AFADA3B3FFC | 5 / 15 (int) | B segment ms |
| +1736 / +1740 | F916E153393C5F24 / 0A36F90732016D85 | 0.10 / 0.25 | B |e| range |

## 5. Port (crate `skate-audio`)
- `dsp::fss::FrequencyShift` (+ `sin`/`cos` polynomials, `SECTIONS`), `dsp::shelf::HighShelfIir2`.
- `grain::bed`: `ChainValues` gains `fss_hz`, `graph3_send`, `graph3_gain`, `env_send`, `flange_send`;
  `GrainBed::chain_extras` (default true; false = the older chain, bit-identical), `chain_tuning`;
  `render_block` / `add_to` / `env_send` (runtime: bed first, its env mono added to `buses.env_in` after the voices,
  `Mixer::render_with_env`). A truck's full chain runs from its first bind (retail builds it there).
- `grain::chain`: `Envelope`, `PushShift`, `fss_shifts`, `ChainTuning` (vault defaults), `ChainState`
  (`frame`, `rebuilt`, `values`, `gains`), `ChainFrame`, `DEGREES_PER_RAW`, `PER_LEVEL`.

## 6. Validation
- Unit tests: allpass magnitudes / quadrature / tone shift (−60 dB image at 1 kHz) / 0 Hz = I path / phase wrap;
  trig vs libm; HS20 vs f64 RBJ and plateau; envelope segments; send/level/hold; wobble bounds, alternation,
  truck-1 quirk, rebuild; bed: +150 Hz shift of a tone, graph 3 silent at send 0, env tap = pre-pan mono × level,
  extras off ignores the new values.
- Identity: `examples/grain_chain_probe.rs --mode old` = the pre-change copy (built with `--cfg baseline`),
  byte-identical 12 s stereo (hash 7F07793579AFCFD5). Cost per block p50: 10.5 µs (old) → 37 µs (full chain, one
  bound truck; f32 `mul_add` is a libm call without `+fma`).
- Retail (`chain_check.py` (local script), session `dsp_20261002_185826`, owner 40C33020):
  graph-3 send = model exactly (median error 0.0000 over 189 posts, 4 chains); graph-1 Gai0 / graph-3 Gai0 = 1
  below 52 km/h and = the level ramp above 53 km/h (|Δ| ≤ 1e-4); max |g3 − 1| / ramp = 0.290 (A, cap 0.30) and
  0.240 (B, cap 0.25); graph 3 `0x40C118A0` ↔ graph 1 `0x40C79BF0` is A, `0x40C11C20` ↔ `0x40C7A180` is B;
  only truck 0's two graph-3 Gai0 ever change (the other graph 3s log only their default).
- Expected manual / trick effect: A × 0.65 (−3.7 dB, records) and A +150 Hz (FSS), B silent (I = 0): probe
  30 km/h manual −38.21 dBFS (= old chain), centroid 1375 → 1502 Hz. High speed (send 3, level 0.45..0.87):
  +5.7 dB from graph 3's clipped copy in the probe (60–85 km/h, concrete_rough_hard).

## 7. UNCERTAIN / not modelled
- FlangeSub return A (`[[mgr+116]]`, graph-2 Sen0 #1): rendered since 2026-10-29 (doc 11 "Rolling leftovers").
- Env send level vs retail: graph 2's mono Sen0s never appear in the SEND hook (`sub_82B31838`) in the dsp session,
  so level(13) at the chain is untraced; our MixMap gives 0.079 at 30 km/h.
- Two SEND lines at 1.0 (t 27720 / 28690) where the model says 2.0 / 0: probably a rebuilt chain's first block at
  the Sen0 default before its build-time post (game thread vs audio thread race); not modelled.
- XMVectorSin/Cos to the ulp: VMX flushes denormals and the recomp's `vmaddfp` fusing is assumed; no FSS oracle
  vectors (PoC `dsp_vectors` has none).
- Release fold of a torn-down chain's env send (Sub0 deltas into the env bus) is not modelled; the dry fold is.
- The high-speed level jump (+5.7 dB) is the mechanism's result; not compared with a retail capture (the capture
  also holds wind, rattle and rocket at those speeds).
- Trick latch only evaluated while the manual latch is clear (§4.1); the existing `board::Latches` updates both.

## 8. Sources & credits
Retail behaviour read from the skate3recomp TU3 lifted code (reference only), image constants from the PoC's dumped
image, vault values from the converted collections; trace hooks in skate3recomp `src/research/hooks_audio.cpp`
(GREC / GAIN / SEND / MOD). Prior notes: upstream PR #4 (chain shape), `grain-player-spec.md`.

## 9. Listening reports 2026-10-02 (rolling busy / rough, rolling in the air): evidence
Local tools: `air_check.py`, `surface_vs_retail.py`, `spectrum_compare.py`,
`transients.py`, `seams_rate.py`, `trucks_sounding.py`, `capture_windows.py`, `landing_ratio.py`; renders of the user
logs 200832 / 201324 / 203746 / 204047 in `e2e/` (main-repo `e2e_render`, old chain).
- **Air**: in all 48 air windows (> 0.2 s) of the four logs the bed's records are 0 from 100 ms after takeoff (MixMap
  F0 on PlayerPhysics.in2 → C[Player] −10000 mB); retail (dsp session) A 0.06 → 0 in ≈ 80 ms. What keeps sounding in
  the air is the rocket `x_jet_rolling` (≈ 0.06–0.10) and the wheel spin: retail does the same; jet grains keep
  starting inside air windows with send 0.10–0.14 (0.09–0.12 before takeoff), SenseOfSpeed level(5) is not
  wheel-gated (E74 has no F0 term). No change.
- **Surface (a)**: our tag = retail material + 1 at the same world position (WPPOS, same frame): 13 of 14 shared 10 m
  cells of 204047 agree (tag 4 = concrete_rough; one boundary cell tag 64), mt_high 3/41 = retail 2/40.
- **Rendering (b)**: at matched speed on concrete_rough (retail all_20261002_204336 vs ours 204047): gain A
  0.22–0.26 vs 0.22–0.24, position 0.70–0.72 at 40 km/h = the Bézier, pitch 0.99 vs 0.995, grain starts 6.7 vs 6/s,
  one sounding truck (retail both trucks running 0.7–1.1 % of frames, ours 0 %), seam clicks 7.7 vs 9.9 /s at
  30–40 km/h and 15.1 vs 15.9 /s at 40–50 km/h at ~0.05 per voice, rattle per push retail 0.32 vs ours 0.12, rocket
  send 0.08–0.12 vs 0.05–0.09; octave spectrum of clean rolling within ≈ 2 dB per band (retail capture also holds
  music/ambience). Retail-only parts left: this chain (A allpass, B −10 Hz FSS, env send level(13) ≈ 0.079,
  graph 3 above 46 km/h). Capture-level ratios (landing over bed) are not usable (capture/trace alignment).

## 10. Listening test 8 (seams "too rough / too often / faster"): evidence
Local tools (`seam_stats.py`, `block_gain.py`, `retail_seams.py`, `retail_by_pattern.py`,
`retail_timeline.py`, `crossings.py`, `dataflow.py`, renders `e2e-before/-after/-off`).
- Cause outside the chain: Class_Seams' sample index = table(w10) + table(w8) + soft + `send_random_0_to_9a/b/c`. Those
  globals are written only by `Common.abk`'s `Start_up_Play_ctl` (a boot utility, on `rnd_call`), which was not loaded.
  Fixed (doc 11 "Listening test 8").
- Retail vs ours at matched pattern / speed: concrete pattern 8, 30–40 km/h: 21 voices/s (retail) vs 28 (ours, 36 km/h).
  Brick, 30 km/h: 27–40 vs 29–31. Per-voice gain on concrete 0.052–0.074 vs 0.045–0.053. The speed word w8 is the
  same as the asm.
- Graph 3 and the spidercrack layer 5 play no part on these concrete windows (≤ 44.7 km/h, patterns 2 / 8 / 11).
