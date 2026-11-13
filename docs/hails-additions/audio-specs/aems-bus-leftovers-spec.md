# Bus leftovers: FlangeSub returns, sense_of_speed FXWET0, Splice Send A / Collision SubMix, reverb zones

Written 2026-10-02 (headless reading of the code). Companion to `aems-env-bus-spec.md` (§7 FlangeSub, §6.2 zones) and
`aems-eqchain-buses-spec.md`, which it completes. Behaviour in our own words from the TU3 (title update 3) lifted code (reference
only); addresses are TU3 3.0.3.0. Tags: [R] our reading of the code, [IMG] image constant, [VLT] vault value,
[TR] checked against a clean recomp trace, UNCERTAIN = not settled. Local analysis scripts
(`sendb_bank.py`, `senda_bank_fixed.py`, `senda_windows.py`, `voice_windows.py`, `submix_sends.py`). Crate
probes: `examples/effect_routes.rs` (routing-record census), `examples/post_probe.rs` (what a class's words do to
its voices), `aems_render` with `AEMS_FLANGE=…`, `voice_load_probe … fx`.

## 0. Answers
1. **FlangeSub returns** are two mono flangers: `Sub0 → Del0 → PI20 → Sen0 (env) → Gai0 → Pn21 → Sen0 (SFX
   Master)`. Routing 4096 / 8192 → return A, 16384 → B. Users on the disc: GRINDS (4096), WHEEL_SKID_BANK (8192),
   Sk8_Air_Flip_Tricks and hom_slo_mo (16384). The Send B level comes from the component's words (grind w15 =
   Rail out6 = −26 dB, skid w16 = SkateBoard out20 = −25 dB) and already matched retail; only the return was
   missing. Implemented (`bus/flange.rs`, mixer Send B).
2. **sense_of_speed FXWET0 = 0 is retail.** The updater `sub_824E7CB0` writes w6 = 0 every frame and w6 is
   property 5. The "retail median 0.0150" came from `envbus/senda_bank.py`, which attributes every trace line
   to the owner's *last* PLAY of the session. With time-correct attribution, 282 of 293 sense_of_speed Send A
   lines are 0; the 11 others are other graphs reusing the owner address (send/gain 754). No port bug.
3. **Splice voices have no Send A** (retail graph `Rsp0 → Gai0 → … → Sen0`, trace `PITCH GAIN SEND6`). The
   "0.0146" was the same attribution artefact. The collision voices' wet path is the per-voice **Collision
   SubMix** (`sub_824D25E0`): mono, env send = Collision output (category) / 32767 posted once at build (≈ 0.1
   near the camera, retail trace cluster 0.100), then into SFX Master's centre. Implemented (`Route::mono`,
   `CollisionManager::submix`).
4. **Reverb zones** (`.ems` attribute type 5): the full `SFXObj_Reverb` state machine with zone fade,
   zone-to-zone and the rotation of the reverb outputs toward the zone. Implemented as
   `EnvNetwork::update` (`bus/zones.rs`); the host has to supply the zone nodes.
5. Extra: **manager +104** (global env-send scale) is the Reverb owner's out4 every frame. The EnvSub sends are
   re-posted per frame as *last applied preset* × scale (`sub_8248DB78`) [TR: 0.2497 / 0.4994 / 0.9989 =
   preset × 0.99887]. Implemented as `EnvNetwork::scale_frame`. The hosts write Reverb.in5 = 32767 (from PR #4's
   capture stats). That holds F213 at −400 mB, so out4 = 20603 and the scale would be 0.63. The retail trace
   rules this out: in those sessions in5 = 0.

## 1. FlangeSub effect returns
### 1.1 Graph `sub_82490270` [R]
- Two graphs, both order 150, 7 modules each. Module list (params, FourCC, channels):
  - Sub0 (name 0x8224DC5C), 1 ch;
  - Del0, 1 ch, ctor max delay [0x820D0190] = 0.015 s;
  - PI20, 1 ch;
  - Sen0, 1 ch;
  - Gai0, 1 ch;
  - Pn21, 6 ch;
  - Sen0, 6 ch.
- Return A: graph at mgr+108, modules mgr+116. Return B: mgr+112 / mgr+120.
- Sen0 #3 links (0x7FF7FFF4) to `[[mgr+52]]`, the env input. Sen0 #6 links to `[[0x830CFDBC]+44][0]`, SFX
  Master.
- Module pointers kept for the level posts:
  - A: mgr+148 = Pn21, +152 = Sen0 env, +156 = Gai0;
  - B: +184 / +188 / +192.
- Boot `sub_826DD3A8` zeroes these six pointers before building.

### 1.2 Voice side `sub_824A3140` [R]
- A routing record (id ≥ 9) with value 4096 / 8192 / 16384, followed by an enable record = 1, sets the effect:
  - voice +72 = code, +76 = 1;
  - +80 = next-next value / 32767;
  - +88 = `[[mgr + 4·(29 + (code == 16384))]]` (Sub0 of A or B).
- The voice graph gets a Sen0 **after Gain and before Pn21**, at the voice's channel count. N → 1 into the
  mono Sub0 uses the image route tables: L, C, R, Ls, Rs at unity, LFE dropped.
- Level posted 0.0 at open. Property 11 then sets Send B = value / 32767 (`sub_824A29A8`, only when +88 ≠ 0).
  It is not multiplied by VOL: the signal is already post-Gain.
- The +80 record level is stored but never posted.

### 1.3 Census `examples/effect_routes.rs` (disc banks) [R]
The template words are 0 and programs fill them; `post_probe` shows the source.

| bank / class | record 9 | record 10 (enable) ← | record 11 (level) ← | record 12 |
|---|---|---|---|---|
| GRINDS / Class_grind | 4096 → A | w14 (local) | w15 = Rail out6 | w16 = eEQChain 5 |
| WHEEL_SKID_BANK / Class_wheels_skid | 8192 → A | w15 (local) | w16 = SkateBoard out20 | n/a |
| Sk8_Air_Flip_Tricks / Class_Flips | 16384 → B | (not ported) | | |
| hom_slo_mo / hall_of_meat_slo_mo | 16384 → B | (not ported) | | |

Totals over all routing records: 0: 211, 256: 3, 512: 160, 2048: 347, 4096: 6, 8192: 4, 16384: 19.

### 1.4 Presets `sub_824DDF58` → `sub_8248FE68` [R][VLT]
- Vault class `4CDD7CDC1A955D5C`, two collections (first → type 12288 → A, second → 16384 → B):
  - A = `C6290FFCB9E84CD9`, which drives the graph that sweeps 1200–1800 Hz in the trace;
  - B = `DFDFFAD67CCBA322` (constant 250 Hz).
- Nine floats by record offset:

| off | field | A | B | used as |
|---|---|---|---|---|
| 0 | DE9BF5C10AB2C866 | 20.0 | 1.7 | stored at mgr+216+96i; no reader found (PI20 Q?) |
| 4 | 113F78E17495296F | 0.3 | 0.3 | PI20 LFO rate: period = 1 / v (fdivs, no lower bound) |
| 8 | 3C0376079AD9A56C | 0.2 | 0.0 | PI20 LFO amount |
| 12 | 7D3294EC1C443CBD | 0.1 | 1.0 | stored only (PI20 gain?) |
| 16 | CD44B25FAD9E48D9 | 1500 | 250 | PI20 LFO centre = depth (Hz) |
| 20 | EDEE41D561FA8CE5 | 0.002 | 0.0005 | Del0 LFO centre = depth (s) |
| 24 | 9C890EC62D3BC5C6 | 0.5 | 0.25 | Del0 LFO rate (period 1 / v) |
| 28 | D10D7F661896AC30 | 0.9 | 0.6 | Del0 LFO amount |
| 32 | 9024FC56E2314F9F | 0.03 | 0.11 | stored only (feedback?) |

- LFO records 11 / 13 (Del0 p0) and 12 / 14 (PI20 p0): {period, phase 0, amount, centre, depth = centre}.
  - The "LFO Task" (`aems-env-bus-spec.md` §5) posts them every block: d(t) = d₀ (1 + amount · sin 2πt / period).
  - A: delay 0.2…3.8 ms at 0.5 Hz (10…182 samples). B: 0.2…0.8 ms at 0.25 Hz.
- **Nothing posts PI20's gain or Q, nor Del0's feedback.** The trace's PEAK lines stay at gain 1.0 / Q 3.0
  (class defaults, PI20 bypassed). Del0's descriptor default feedback is 0.0 (attribute table 0x82FCE068:
  delay default 0 / max 20; feedback default 0, range ±0.99).

### 1.5 Levels per frame `sub_824DF220` (SFXObj_Reverb vtable 0x822FCDE0 +48, after the MixMap tick) [R][TR]
- vfunc60 of the Reverb owner (`0x40050000`), each / 32767:

| target | output | free skate | retail |
|---|---|---|---|
| A Gai0 | out0 | 32692 (E46, 0 mB + Global ducks) | 0.9977 [TR] |
| A Sen0 env | out1 | 2313 (E48, −2300 mB) | 0.0706 [TR] |
| B Gai0 | out2 | as out0 | |
| B Sen0 env | out3 | as out1 | |
| mgr+104 | out4 | 32730 (E47) | |

- The values go through `sub_8248FFF8`.
- mgr+124 / +160 (the Pn21 value) and +136 … +144 / +172 … +180 (parameter ids) are never written.
  - Zeroed allocation assumed: Pn21 p0 (azimuth) = 0, D = 1 (class default). The return plays **in the
    centre**. UNCERTAIN (allocator flags 257 | 0x1000000, not traced).
- Retail processes return A only in bursts (its PEAK lines appear while grinds / skids play). We render a return
  only in blocks where some voice sends into it; its LFO advances every block.

### 1.6 Port
- `bus/flange.rs` (`FlangeReturns`, `FlangePreset`, `FlangeReturn::render`). Disabled until `set_presets`.
- Mixer Send B (`Voice::render`): mono sum of the post-gain channels × property 11 (our user group volume too),
  Sen0 64 / 65 ramp. Allocation-free per block.
- **Identity:** returns disabled → GRINDS and WHEEL_SKID `aems_render` WAVs byte-identical to the baseline; the
  mixer test renders an effect-routed voice identically to a plain one.
- **Effect:** GRINDS render with the returns: the difference signal is −26.1 dB re the dry (a −26 dB centre
  copy, 0.2–3.8 ms, sweeping). Comb ripple ≈ ±0.4 dB.
- **Cost:** `voice_load_probe 96 3000` p50 per block, three runs each:
  - baseline 516 / 515 / 516 µs;
  - ours, off: 521 / 522 / 521 (+5 µs, the larger Voice and the extra branch);
  - all 96 voices routed into the returns: 528 / 530 / 529 (+8 µs).

## 2. sense_of_speed FXWET0 [R][TR]
- `post_probe sense_of_speed SenseOfSpeed_rattle`: property 5 ← w6, 7 ← w5, 8 ← w7, 2 ← w0 × curve.
- The retail updater `sub_824E7CB0` writes w0, w1 = raw(0), w2 = pitch(2), **w6 = 0** (stw r27), w7 =
  level(1) and w3. The wind is the same with outputs 3 / 4.
- So retail Send A = VOL × 0 = 0, as ours.
- Trace (all_163809, `senda_windows.py`): every sense_of_speed voice window starts `SEND1 0.0000 / GAIN … /
  SEND6 1.0` and its Send A never changes.

**`envbus/senda_bank.py` bug:**
- `cur[o]` is read after the file loop, so every SEND line is attributed to the owner's last PLAY in the
  session.
- `senda_bank_fixed.py` attributes by voice window and identifies Send A as the SEND below the GAIN address
  (all_163809):

| bank | Send A lines (non-zero) | non-zero median | Send A / gain | Send B median |
|---|---|---|---|---|
| GRINDS | 15 (15) | 0.0102 | 0.165 | 0.0499 |
| WHEEL_SKID_BANK | 107 (88) | 0.0146 | 0.635 | 0.0561 |
| fstep_skateshoe1_sm | 3318 (2527) | 0.0147 | 0.255 | n/a |
| Seams_Bank | 730 (623) | 0.0127 | 0.205 | n/a |
| Rolling_Rattles | 326 (326) | 0.0241 | 0.190 | n/a |
| sense_of_speed | 293 (11) | artefacts | 754 | n/a |
| Skate_Collisions | 153 of 6754 voices | 0.0703 (= Collision SubMix env sends on reused owners) | 1.44 | n/a |

- The doc-11 table's GRINDS / WHEEL_SKID / FOOT_DRAG Send A figures should be re-read with the fixed
  script.
- Hook caveat: the SEND / GAIN hooks key their change filter by module address across voices. A window sees a
  module's first post and changes > 0.01 relative to the last logged value at that address.

## 3. Splice Send A and the Collision SubMix
### 3.1 No Splice Send A [TR]
- Skate_Collisions voice windows: 271 × `PITCH GAIN`, 111 × `PITCH GAIN SEND6`, 13 × `PITCH`, 2 × with stray
  SEND1s (reused owners). There is no Send A and no filters.

### 3.2 `sub_824D25E0(collision, i)`, called by `sub_824D1F68` per material voice before it starts [R]
- Graph order 4, 3 modules, all **1 channel**: Sub0 → Sen0 → Sen0. Stored at this + 32i + 52 / 56; module
  indices 1 / 2 at +64 / +68.
- Sen0 #1 → env input; level = vfunc60(`sub_824D21D0`(material i)) / 32767, posted once at build.
  - Category → output: 0 → 3, 1 → 4, 2 → 5, 3 → 6, 4 → 7, 5 → 8, 6 → 2, 7 → 9, 9 → 11, else (8, material
    ≥ 143) → 10. Jump table 0x824D2214.
  - MixMap E2: outputs 2–10 = −2000 / −2000 / −1622 / −2000 / −2000 / −1755 / −2000 / −2000 / −2000 mB +
    B[Collision.1] (4…50 m) + C0 + A0, and out11 = −1600.
- Sen0 #2 → `sub_82491108(mgr, sub_82497BF8(mat i, tier i, mat 1−i), create = message byte +41)`, level never
  posted (1.0).
- The Splice voice is started with this Sub0 as its output (`sub_82975700` r6).
  - Its 6-ch final Send sums 6 → 1 (L … Rs, LFE dropped).
  - The submix's Sen0 into the 6-ch bus is 1 → 6 = **centre**.
  - So retail collisions are mono in the centre, and a voice panned between two speakers sums its pair (up to
    +3 dB at the sides).
- `sub_82497BF8`: AudioSurfaceMap entry (0..93 by material, else entry 94).
  - +48 (word 12) for tier 0, +64 (16) for tier 2.
  - Tier 1: +52 / +56 / +60 by `sub_82497910`(other) = class 0 / 1 / 2 (+52 when other = 143).
  - **Every shipped entry holds 8** (= SFX Master), so the EQ leg is SFX Master.
- [TR] SEND lines on non-voice owners within 1.5 ms of a collision PLAY: SEND1 values cluster at 0.100 (290
  lines, = −2000 mB near the camera) and 1.0 (281 = Sen0 #2 default); `submix_sends.py`: 315 owners
  `SEND1 SEND1`.

### 3.3 Port
- `Route::mono` (mixer `SIX_TO_MONO_CENTRE`: the final Send adds L…Rs into the bus centre; the existing
  owner-env tap gives the env send).
- `CollisionTuning::{surface_eq, eq_bus, env_output}`.
- `CollisionManager::submix` (default **off** = SFX Master as before). With it on, `start` sets each voice's
  route from the outputs after this frame's tick.
- Test: with the MixMap warmed up, board (category 6 → out2) env ≈ 0.1. On the very first tick of a fresh
  MixMap every output is 0.

## 4. Reverb-zone emitters (type 5) [R]
### 4.1 SFXObj_Reverb update `sub_824DE548` (per frame)
State:
- +400 current key, +408 target key;
- +416 current side, +420 target side;
- +424 zone side;
- +428 / +432 zone nodes;
- per side (+28 + 184·side): +48 weight, +128 mode, +204 timer, +208 length 1.0.

`sub_82488278(mgr, skip)` returns the first type-5 node of the emitter manager's active list (audio+488, list
+128) that is not `skip`, provided vfunc92(attribute) passes (else 0; it does not look further).

- **No transition** (+400 = +408):
  - no zone → clear the nodes; if the current key ≠ the region key (`*(0x83083C38)+0x2F0B0+16`, 0 → reverb01)
    → start(region, 0);
  - zone z1 and none held → hold z1; start(z1 preset, 1); zone side = target side;
  - z1 held and a second zone z2 = query(z1):
    - same preset and current = k1 → target side = current side;
    - k1 ≠ k2 and current = k1 → start(k2, 2), node 2 = z2;
    - otherwise → start(k1, 2), node 2 = z1;
  - z1 held, no z2: if held node d > 0 and zone side = current side → start(region, 1), zone side = current
    side (leaving a committed zone).
- **Transition** → the target side's mode:
  - 0 = timed fade `sub_824DE850` (timer += dt; weights x / 1 − x; commit at 1; pans 270 / 90);
  - 1 = zone fade `sub_824DE970`;
  - 2 = zone-to-zone `sub_824DEAE8`;
  - ≥ 3: target side = current side.
- start `sub_824DE468`: the target key; target side = 1 − current; the preset on that side at weight 0;
  mode; timer 0; length 1.0.

### 4.2 Zone fade `sub_824DE970`
- Re-query z1 / z2. If z1 is missing, or the held node is neither z1 (or the same attribute) nor z2 → snap
  `sub_824DEDD8`:
  - if zone side = current → weights current 0 / target 1, commit;
  - else current 1 / target 0, cancel;
  - nodes cleared, pans 270 / 90.
- Timer[zone side] = held node d (node +12, 0 inside the inner core). d ≠ 0 → blend; d = 0 → commit (zone side
  = target) or cancel, weights current 1 / other 0, pans 270 / 90.

### 4.3 Blend `sub_824DEEF0(side, zone)`
- Inputs: camera = `*(0x83083C38)+0x2F078` +48 position / +64 forward (x, z); zone position = node record +8 /
  +16. No camera or no node record → vfunc 28 (UNCERTAIN).
- Unit vectors f̂ (forward) and d̂ (zone − camera).
- c = clamp(d̂·f̂, −1, 1); cross = d̂ₓ f̂_z − d̂_z f̂ₓ.
- θ = (1/π)·(acos(c)·180) ∈ [0, 180] (vector acos `sub_82453298`).
- left = θ − 270, or θ + 90 when cross ≤ 0 and θ < 90; right = 90 − θ.
- L = 270 + d·left (fused); R = −(d·right − 90) (fused). Both wrapped into [0, 360).
- Weights: side 1 − d, other side d. Post side with (L, R), other side with (270, 90).

### 4.4 Zone-to-zone `sub_824DEAE8`
- No z1 → vfunc 28 (UNCERTAIN; we snap).
- z1 only:
  - held node 1 ≠ z1 → node 1 = node 2;
  - current key = node 1's preset → current 1 / target 0, else target 1 / current 0;
  - node 2 cleared; target side = current side; zone side = current side.
  - Retail quirk, kept: the 1-weighted side need not become current.
- Both zones: the held nodes must both be z1 or z2 (else vfunc 28).
  - Same attribute → current 1 / target 0, cancel.
  - Else d of node 2 = 0 → current 0 / target 1, commit, node 1 = node 2.
  - Else timer[target] = d, blend(target, node 2).

### 4.5 Port and host contract
- `bus/zones.rs`:
  - `EnvNetwork::update(dt, region_key, &[Zone], Option<&Camera>)` replaces `request` + `frame`.
  - With no zones it is the same timed fade in retail's order: the starting frame does not advance it, and the
    network starts with reverb01 on both sides (`sub_824DDE50`), so the first region preset fades in over
    1 s.
  - `Side::apply` now resets the reverb outputs' pans to 270 / 90 (sub_8248DD18). `scale_frame` follows the
    selector's sides.
- (2026-10-02 correction) The disc has 1,040 type-5 records in 24 attributes, not one: 989 in the district
  `reverb_*.ems` files (loaded with music_ / sfx_ / speakers_ / crowds_, map database field `65FA976EF23A314E`), 51 in the
  parks' `sfx_` files. Wired: `emitters::reverb_zones`.
- `Zone`, per frame, the active `.ems` nodes of attribute type 5 in the emitter list order:
  - `id`: node identity;
  - `attribute`: the record's sound id (node +24);
  - `preset`: the attribute's RefSpec `99FD793BC30CF0FA` collection key (class `204CAC1FD77088B8`; the only
    type-5 record on the disc, `1F94F2F815C00368`, 37 nodes in the sfx files, points at
    `BEEFC8E3DE04FBAE`; its parent `ECADC64E974CF9A1` holds reverb01);
  - `d`: normalised distance after the inner core (`ems-emitters-re.md`: sphere / ellipsoid, active while
    d < 1);
  - `position` [x, z];
  - `enabled` = vfunc92 `sub_824A2438`: the attribute's `99FD793BC30CF0FA` key is in the image table `0x8302E298` (the 24
    reverb preset keys = the exported `aud_reverb` set); missing field → key 0 → fails. All 1,040 disc zones pass.
- Tests: the timed fade from reverb01; zone entry → blend 1 − d / d with the pans turned toward the zone (θ = 0:
  L 337.5, R 22.5 at d = 0.75) → commit at d = 0 → leaving (region preset on the other side) → snap outside;
  the rotation unwraps on both sides (θ = 45 / 90 → both pans on θ at d = 1).

## 5. Integration (game host, read-only for this pass)
1. **Flange data** (setup `audio_export.bus_tuning`): add `"flange": [[A 9 floats], [B 9 floats]]` from vault
   class `4CDD7CDC1A955D5C`, collections `C6290FFCB9E84CD9` (A) and `DFDFFAD67CCBA322` (B), fields by offset
   (table §1.4).
   - Host: `rt.mixer.buses.flange.set_presets(FlangePreset(a), FlangePreset(b))` wherever `env.presets` is set
     (native.rs and e2e.rs).
2. **Per MixMap frame after the tick** (`native::mixmap_frame`, e2e loop):
   `let r = Owner { mixmap: m, key: keys::REVERB }; buses.flange.frame([r.level(0), r.level(1), r.level(2), r.level(3)]);`
   - Optional: `buses.env.scale_frame(r.level(4))`. Only with Reverb.in5 = 0: the hosts write 32767 today,
     which gives out4 = 20603, i.e. −4 dB on every env send. The retail trace shows the scale at 0.9989. Who
     writes Reverb.in5 is not known: decide before wiring (`flange::tests::the_reverb_owner_…`).
3. **Collision SubMix:**
   - `library.rs` `CollisionJson::tuning` adds
     `surface_eq: surface_table.iter().map(|r| std::array::from_fn(|k| r.get(12 + k).copied().unwrap_or(8))).collect()`
     (the words are already exported).
   - `PlayerAudio.collision.submix = true`.
4. **Zones:** `native::reverb_frame` calls `env.update(dt, region_key, &zones, Some(&camera))` instead of
   `request` + `frame`.
   - Zones come from `emitters.rs`: the type-5 nodes the per-frame query finds, with their d and position.
     They are currently skipped there.
   - Camera = the listener transform: position and forward (Bevy forward = −Z).
   - New `AudioState` fields: none.
5. **No new Slots, banks or MixMap owner keys** beyond `keys::REVERB` (exists).

## 6. e2e expectations (integrator; `tools/audio-e2e/`, game test `e2e_render`)
- **Defaults** (returns disabled, `submix = false`, `request` / `frame` kept): every scenario bit-identical to
  the baseline.
- **Flange on:**
  - rail / ledge grind scenarios gain a centre-channel −26 dB (re the grind voices' dry level) flanged copy,
    plus its env send (0.0706 × that);
  - power-slide / skid scenarios −25 dB;
  - rolling, ollie, manual, bail: unchanged (no effect records). Folded level change < 0.1 dB RMS, comb ripple
    ±0.4 dB.
- **Submix on:**
  - every collision one-shot (grind-start rail hits, deck impacts, landings on landing-flag surfaces) moves
    to the centre;
  - level +0 … +3 dB depending on its azimuth (the e2e camera sits behind the skater, so most are near the
    centre: small);
  - a new reverb tail at ≈ −20 dB (Collision outputs 2–11 / 32767) into the env network.
- **scale_frame with in5 = 0:** wet path −0.01 dB everywhere. With in5 = 32767: −4 dB (wrong, see §5.2).
- **update instead of request / frame:** with no map (reverb01) identical. With a region preset ≠ reverb01 the
  start fades in over 1 s instead of applying at once, and fades start one frame later.

## 7. UNCERTAIN
1. FlangeSub Pn21 azimuth (mgr+124 / +160 never written; zeroed allocation → 0 = centre).
2. The three unread FlangeSub record fields (+0, +12, +32: Q / gain / feedback by their values; no code posts
   them).
3. When retail processes a return. Evidence: A's modules log only in bursts. Ours: blocks with a sending voice;
   retail may keep a tail.
4. (Closed 2026-10-02) Reverb.in0..6: `sub_824DF468` (first step of `sub_824DE548`) writes all seven 0, then 32767 into
   the input of the target side's preset number (`sub_824DF390`, table 0x824DF3D0: 1–5 → in4, 6–8 → in0,
   9/13/14/17/18 → in1, 10 → in2, 21 → in3, 11/12/15/16/22 → in5, 19/20/23/24 → in6). Applied with `scale_frame`.
5. The Collision SubMix create byte (message +41; irrelevant while every AudioSurfaceMap word 12–16 is 8).
6. vfunc 28 of SFXObj_Reverb (no zone in mode 2 / no camera). (vfunc92 of the zone query: read 2026-10-02, §4.5.)
7. Reverb-zone node record layout (+8 / +16 = x / z assumed from the `.ems` record layout).
8. Flips / hom_slo_mo Send B levels (their components are not ported; retail flips showed level 0 in
   all_163809).
