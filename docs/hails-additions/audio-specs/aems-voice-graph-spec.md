# Skate 3 voice graph and output mix: behavioral spec for the native port

Written 2026-10-02. This describes behavior, formulas and constants in our own words, as the basis for a native Rust
re-implementation (Phase B of the native-port plan (doc 11, "Native audio port")). Nothing here is code from PR #4 or the recomp; both have
no license and were read for behavior only. Companion notes: `aems-reference.md` (banks, evaluator, property ids),
`rwaudio-prior-work.md` (RenderWare Audio prior art: b5-decomp, BP-Decomp, nfsmw), `ems-emitters-re.md` (ambience).

## 0. Sources and confidence tags

- **[V]**: PR #4 marks the function verified (shadow-compared against the running game) and/or it replays recorded
  vectors with 0 disagreements.
- **[T]**: transcribed by PR #4 from the lifted code, never compared.
- **[R]**: our own reading of the recomp's generated C++ (`skate3recomp/generated/skate3_recomp.*.cpp`).
- **[IMG]**: a constant or table read from a dump of the TU3 image
  (base 0x82000000).
- **[I]**: inference.
- **[PA]**: corroborated by the RenderWare Audio prior art (`rwaudio-prior-work.md`; b5-decomp, DWARF names).
- **UNCERTAIN** marks anything a port must not treat as settled.

Addresses are TU3 (title update 3.0.3.0). Offsets are object-relative bytes, big-endian guest layout.

## 1. Clocks, blocks and latency

- **Mixer**: 48000 Hz, blocks of **256 frames** (5.333 ms) [V]. Every module processes exactly one 256-frame output
  block per call. Per-block time step = 256/48000 as f32 (0x3BAEC33E).
- **Evaluator tick** (AEMS patch programs, incl. the voice op 27 that pushes voice parameters): accumulate block
  deltas until the next one would reach 1/30 s. That gives **6 blocks = 32.0 ms** per tick, and the tick-scale global =
  32.0 ms [T][PA]. Boot image holds rate 41.6; game init overwrites it with 30.0. Parameter changes therefore reach a
  voice only every 6th block. The rest of the time the modules hold their last values; Gain/Send/Pan ramp for 64
  samples after a change.
- **Game frame**: 60 Hz. Per frame the audio-state bridge writes the MixMap inputs, the MixMap evaluates once, and then
  every SFX component re-reads outputs and re-delivers its held packets. Inputs written during a component's update
  are seen by the next evaluation, one frame later [T].
- **Latency chain (game event → sound)** [I]:
  1. ≤ 1 frame (16.7 ms) until the component posts or redelivers;
  2. ≤ 32 ms until the next evaluator tick runs the program;
  3. the command ring, drained at the start of the next block;
  4. plus PR #4's stand-in 15 ms master delay, if adopted (see §6.3; not retail);
  5. plus device buffering.

  A play command can also carry an absolute start time (SndPlayer1 record +0, §4.1), which delays the start to the
  sample.
- **Graph pass order** [T]: every graph ("player") has an order byte (+73). Players run in ascending order each block.
  - voices: 0
  - board chain graph 1: 2
  - graph 3: 3
  - owner one-shot bus: 4
  - graph 2: 5
  - effect returns: 150
  - material buses: 253
  - PR #4's master stand-in: 255

  A contributor must run before the bus it feeds. Sends accumulate into a bus that flushes when its own graph runs
  in the same block, so there is no extra block of delay per bus hop [I].

## 2. Graph mechanics (common to all modules)

- **Signal convention** [V]:
  - The pass object holds two plane descriptors: +28 is the current signal, +32 is scratch.
  - A descriptor is {+4 base of channel 0, +14 u16 floats between channels}.
  - A processing module reads current, writes scratch, then swaps the pair. A module that bypasses does not swap, so
    the next module reads the untouched input.
  - Pass +60 holds the current channel count; pass +48 the frame count of the current round; pass +52 the round's
    sample rate.
- **Pull negotiation** [T][PA]: each block, the graph asks its sources, in reverse order, how many input frames they
  can take for the remaining output (prepare). It then runs them forward (process). Only SndPlayer1, Rechannel and
  Resample have a prepare:
  - Resample answers how many source frames it needs for the block (§4.3).
  - SndPlayer1 stores that as its block size (+460).
  - Rechannel passes the request through.

  If a round produces fewer than 256 frames, the graph repeats rounds and concatenates, then zero-pads the tail. A
  source that declines falls back to a clear-and-advance.
- **Parameter delivery** [V]:
  - A module's parameters are 8-byte slots from +48: tag word 0x7FF7FFF1, then an f32. So parameter i is at
    +52 + 8i.
  - The game thread writes them through the command ring (handler 0x82B463A8). The value passes f32 → f64 → f32
    and is **not clamped** there.
  - Commands are drained once per block before the graph pass, so a new value applies from the next block boundary.
- **Per-voice CPU cost** [T]: each module adds a constant to the player's +40 cost (Iir2 450, PeakingIir2 1500,
  Resample 6). The Dac voice-retirement budget uses these costs (§6.5).
- **Release tail** [PA]: rwaudio voices keep running for the filters' decay before being expelled. UNCERTAIN in
  Skate.

## 3. Voice open and parameter mapping

### 3.1 The voice graph
Opened by the device (`sub_824A3140`) when the patch's voice op starts a sample [T]. Shape (the `msgs1` trace's
119-build shape):

`SndPlayer1 → Rechannel → Resample → HighPassIir2 → LowPassIir2 → [Send A] → Gain → [Send B] → Pan2D1 (6 out) → Send (6 ch)`

- Every module up to Gain runs at the sample's channel count (1, 2, 4 or 6).
- **Send A** (pre-Gain, environment/reverb bus): present only when the bus manager's env bus `[[0x830CFDEC]+52]` is
  non-zero. Its level is posted 0 at open.
- **Send B** (effect send): present only when the voice's routing records ask for an effect bus (§3.3).
- **Final Send**: 6 channels, to the voice's output bus, or to the default bus when none is set.

The other graph shapes seen:

| graph | shape |
|---|---|
| ambience bed | `SnP1 → Rch0 → Rsp0 → Gai0 → LI20 → Sen0` (no panner: the 5.1 bed plays as authored; `ems-emitters-re.md`) |
| grain voice | `SnP1 → Rsp0 → GaF0 → Sen0` (mono), sending into the board chain (§6.2) |

### 3.2 Voice properties
The voice op pushes 12-byte records {u8 id, applied, wanted}. A record is pushed only when it changed, and only on
an evaluator tick [T][PA].

The setter (`sub_82B1BE30`) clamps:
- ids 0, 6, 7 → 0..65535;
- ids 2, 5, 8 → 0..32767;
- 1, 4 and 9..136 pass through;
- 3 goes to the "alternate" setter.

The voice's own setter (voice vtable 0x822FBCA8 +12) then maps them [T, matches the DWARF/b5 player input names PA]:

| id | name [PA] | effect |
|---|---|---|
| 0 | PITCHMULT | Resample parameter 0 = value / 4096 (4096 = unity pitch) |
| 2 | VOL (master) | m = value/32767 (f32), stored at voice +40 |
| 8 | DRYLEVEL | d = value/32767, at voice +44; **Gain parameter 0 = m·d** |
| 5 | FXWET0 | w = value/32767, at voice +48; **Send A level = m·w** (only if Send A exists) |
| 6 | LOWPASS | LowPassIir2 parameter 0 = value, **in Hz, no scaling** (25000 = open, §4.4) |
| 7 | HIGHPASS | HighPassIir2 parameter 0 = value in Hz (0 = off) |
| 11 | (user) | Send B level = value/32767, only if an effect bus exists |
| 3 | AZIMUTH | Pan2D1 parameter 0 = value × 360/65536 degrees (mono, voice mode 1). Before the first such post, if voice byte +69 is set, parameter 7 = 30.0 and parameter 8 = 110.0 are posted and +69 is cleared [R, `sub_824A2C58`] |
| 1, 4 | TIMEMULT, ELEVATION | ignored |

- Any change of 2, 5 or 8 re-posts both products. Each setter stores its f32 first, then multiplies the stored
  singles.
- MixMap levels are 15-bit (0..32767), so full scale = 1.0.
- 1/32767 is the constant at 0x822F8898 (0.000030518509).

### 3.3 Routing records (at open) [T]
Only records with id ≥ 9 are read. Their value selects the bus:

| value | bus |
|---|---|
| 0–7 | material bus N (`sub_82491108`, not created) |
| 10–17 | material bus N−10, created on first use |
| 512 / 2048 | `[[[0x830CFDC4]+688]+48]` / `+40`; also sets voice mode +96 = 0; purpose UNCERTAIN |
| 4096 or 8192 | effect return `[[manager+116]]`, only if the next record's value is 1; level = next-next value / 32767, at voice +80 |
| 16384 | effect return `[[manager+120]]`, same rule as 4096/8192 |
| 256, 1024 | ignored |

### 3.4 Other open-time values
- **Descriptor byte 2** × 0.01 is published to the player graph's +56 (constructor default 100.0).
  - PR #4 calls it "gain %". No DSP kernel we know reads player +56: Resample multiplies the pass's +56 accumulator
    by its ratio, which looks like a pitch/time accumulator, and rwaudio has mTotalPitch.
  - **UNCERTAIN**: it may be a priority or a time scale, not an audible gain. Do not apply it as gain until proven.
- **Panner angles** come from descriptor bytes 3..8 (each <<8, × 360/65536 → degrees):
  - mono, mode 1: parameter 0;
  - multichannel: parameter 1 = 0 (distance 0), parameter 7 = angle; with more than 2 channels also parameter 8.
  - Which descriptor word feeds which parameter depends on the channel count: frame words +100/+104/+112 hold word
    indices (1 ch → 0; 2 ch → 0, 1; 4 ch → 0, 1, 3; 6 ch → 1, 2, 4) [T].
- **SndPlayer1 play** (configure id 5) enqueues a play command with:
  - the sample (EAAC stream header);
  - a running **play counter** float (wraps above 4194304.0) at command +48. **Our trace's PLAY "level" column is this
    counter, not a gain.**
  - the byte at +46 = round(block +60), which the open sets to 1.0 (our PLAY "flags" = 1). Meaning UNCERTAIN.
  - the start time (double).

## 4. Modules

### 4.1 SndPlayer1 (source): process `sub_82B34278` [T, gate-1], play `sub_82B32DC8` [T]

**State.**
- A ring of up to 20 48-byte play records (table offset +464; cursor +469; write index +467; ring size +470).
- 20 decoder-feed slots of 16 bytes (consumer +474, producer +473; slot +113 = 0 empty / 1 ready / 2 done; slot +112
  the stream id).
- +42 current channel count; +456 current source rate; +460 block size (set by the prepare = frames Resample asked
  for); +432 position in the current record; +462 offset of a per-channel "last sample" table; +471 "has rendered";
  +472 pending fade frames.

**Record (48 B).**

| offset | field |
|---|---|
| +0 | f64 start time (0.0 = immediately) |
| +8 | stream |
| +12 | play counter |
| +16 | f32 source rate |
| +20 | total frames to play (end point, or loop end) |
| +24 | s32 loop start (negative = no loop) |
| +28 | pre-roll frames to decode and discard |
| +32, +36 | start offset (summed into the position on the first block) |
| +44 | decode scratch bytes |
| +46 | state: 0/4 idle, 1 queued, 2/3 playing |
| +47 | channels |

**Per block** (output = up to +460 frames at the **source rate**; Resample converts to 48 kHz).
1. If a fade is pending and something was rendered: run the fade block (step 6) and stop.
2. Skip records whose total is 0, marking them done. A non-playing record renders nothing.
3. **Format change**: if the record's rate or channel count differs from the current one, publish the new format with
   0 frames this block. Resample re-caches its rate and passes the block through unprocessed. So the first block
   after a format change is silent [T][PA].
4. **Scheduled start**: if the start time ≠ 0 and lies ahead of the pass clock:
   - delay frames = trunc(rate_block × (start − now) × frames/sec), clamped to one block;
   - write that many zeros and return. If the delay is beyond a limit cell (0x822F8A48), write nothing this block.
   - Once reached, the start time is cleared. This gives **sample-accurate scheduled starts**.
5. **Render**:
   - discard the pre-roll in 256-frame chunks;
   - render min(block, remaining) frames;
   - remember each channel's last sample;
   - advance the position (pre-roll included in the decoded count);
   - publish frames and rate.
6. **End**: when the position equals the record's total:
   - loop start ≥ 0: jump the position to the loop start (seamless, the decoder loops as well);
   - otherwise: mark the record done, move the cursor to the next record and continue with it in later blocks.
     Records queue back to back on one voice.
   - A natural end has **no fade**: the sample's own tail is the end.
7. **Stop / flush** (`sub_82B32328`) [R]: retires every queued record, resets the ring indices and sets **fade = 16**.
   On the next render, each channel's remembered last sample ramps linearly to zero over min(16, block) frames
   (value − k·value/16). The voice output becomes that ramp, not the signal. This is a de-click "hold-and-decay".
   The bus applies its own 16-tap fade on Send release as well (§4.8).

**Grain voices** start at a seek frame: start frame = trunc(rate × seconds). Every grain stream is one block, so a
seek is "restart at 0, decode a 384-frame pre-roll, skip target − 384" [T].

**Native-port notes.**
- Decode with vgmstream at setup (our decision).
- Model the record queue, loop points (start, end), scheduled start, format-change silent block and the 16-frame stop
  fade.
- UNCERTAIN:
  - exact pre-roll/skip values per bank sample (they come from the EAAC header via `sub_82B335A8`);
  - the admission check `rate × span ≤ queued` (`play.rs`);
  - the spans +32/+36.

### 4.2 Rechannel: `sub_82B2C8F0` [V]
- If the incoming count (pass +60) differs from its declared count (+42), the module converts through the route
  matrix (§6.1). The first route into an output overwrites, later routes add, and unrouted owned outputs are zeroed.
- For non-standard counts it copies min(n, m) channels at unity and zero-pads.
- Then it swaps and records the new count.
- **No-op when the counts are equal**, which is always the case in the voice graph unless a stream's channel count
  differs from the descriptor's.

### 4.3 Resample: prepare `sub_82B2DAC8` [V], process `sub_82B2DBA8` [T], kernel `sub_82B43FB8` [V]

**Parameters.**
- +52 = pitch scale (property 0 / 4096);
- +64 = requested rate, i.e. the sample's native rate (measured: 48000, 44100, 36000, 22050);
- +56 = ratio in use; +60 = last computed ratio; +68 = u32 16.16 step; +72 = 16.16 fraction (low 16 bits used);
- +78 = output frames this block; +80 = u8 carried tail frames; +81 = u8 extra look-ahead frames;
- +64 is also reused as the rate cache in the process.

**Prepare, every block.**
- ratio = f32(f32(requested / mixer_rate) × scale), two single roundings. mixer_rate = the pass format's rate,
  48000.
- Recompute only when the ratio differs from +60:
  - step = trunc(ratio × 65536 ± 0.5), rounded half away from zero;
  - if step > 2^18, step = 2^18 and the stored ratio = the ceiling cell 0x82257308 (4.0 = MAX_RESAMPLE_RATIO [PA];
    the cell value is UNCERTAIN, unit tests assume 4.0).
- Every block:
  - the pass's +56 accumulator is multiplied by the ratio (a pitch/time accumulator; not applied to samples);
  - the source frames requested = ((step × 256 + fraction) >> 16) − carried(+80) + lookahead(+81), clamped at 0.
- Measured in our MOD trace: requested 44100, scale 0.9749 → ratio 0.8956 = 44100/48000 × 0.9749 ✓.

**Process.**
- If the stream rate changed, re-cache it and pass the block through unchanged once [T][PA].
- Otherwise, per channel:
  - build a run = this channel's carried tail (up to 6 floats per channel kept at +76's table) + the new input;
  - output n = min(256, ((usable + 1)·65536 − fraction − 1) / step), where usable = carried + input − lookahead
    (8192 if step = 0);
  - each output sample is **linear interpolation**: out = a + (b − a)·(frac × 1.5258e-5), with a = run[i],
    b = run[i+1], frac = the low 16 bits of the phase.
    - The weight constant is the f32 at 0x377FFC9C, **not exactly 1/65536** (it differs by about one part in 2^21).
    - The last step is one fused multiply-add (difference·weight + a).
  - The phase advances by step; the index advances by the phase's high half plus carries.
  - Unconsumed input frames become the new tail; the fraction is kept for the next block.
- **No anti-alias filtering**: pitching up aliases, as retail does.
- Bit-exactness detail [V]: the 8-sample unrolled loop forms its 8 phases from the phase at the top of the trip (fine
  for any step ≤ 2^18).

### 4.4 HighPassIir2 `sub_82B26568` / LowPassIir2 `sub_82B27E20` + builder `sub_82B43CC0` [V: 576 + 576 replayed]

**Parameter.** +52 = cutoff in **Hz** (f32). Class defaults [IMG]: HPF 0.0, LPF 96000.0 (range 0..96000), so both
bypass by default.

**State.**
- history per channel {x1, x2, y1, y2} at +56 + 16·ch (up to 8 channels);
- coefficients {a1, a2, b0, b1, b2} at +184, already normalized by a0;
- cached ω at +204. The constructor stores the raw Hz there, so the first filtering block always rebuilds.

**ω.** ω = f32(f32(fc / fs) × 2π_f32), with 2π_f32 = 6.2831855 and fs = the block's rate (48000 after Resample).

**Bounds** [IMG]: FLOOR = 0.0031415930 (π/1000, about 24 Hz at 48 kHz); CEIL = 3.1384511 (0.999π, about 23976 Hz).

**LPF.**
- ω ≥ CEIL, or NaN → **bypass**:
  - no swap (bit-exact pass-through);
  - history cleared only if the previous block filtered;
  - cache = ω.
- So **raw 25000 = open** because 25 kHz is past 0.999·Nyquist, not because of a special case. Raw 23975 filters;
  23976 bypasses.
- Otherwise ω is floored at FLOOR; rebuild coefficients if ω ≠ cache (exact compare); filter all channels; swap.

**HPF** (mirrored).
- ω ≤ FLOOR, or NaN → bypass (raw 0..24 Hz is off; raw 25 filters).
- Otherwise ω is capped at CEIL.

**Coefficients.** RBJ cookbook (bilinear transform) at **fixed Q = 1**, all single precision, with s = f32(sin ω) and
c = f32(cos ω):
- α = 0.5·s; a0 = 1 + α; inv = 1/a0; a1 = (−2c)·inv; a2 = (1 − α)·inv;
- LPF: n = 1 − c; b0 = b2 = n / (2·a0) (true division); b1 = n·inv. So b1 is not bit-equal to 2·b0.
- HPF: n = 1 + c; b0 = b2 = n / (2·a0); b1 = −(n·inv).
- Response: 0 dB at fc, a peak of about +1.25 dB near 0.72·fc (LPF) or 1.41·fc (HPF).
- sin/cos are the image's double-precision CRT routines, rounded to f32. libm + f32 rounding matches except for rare
  1-ulp cases.

**Updates.**
- Coefficients change only at block boundaries when ω changes. No smoothing or interpolation (zipper possible,
  retail-faithful).
- History is kept across a change.

**Worked coefficients** (fs 48000, f32) [our model from the formulas, 1-ulp caveat]:

| case | ω | a1 | a2 | b0 = b2 | b1 |
|---|---|---|---|---|---|
| LPF 5000 Hz | 0.65449846 | −1.2164445 | 0.53329474 | 0.079212569 | 0.15842514 |
| LPF 1000 Hz | 0.1308997 | −1.8614084 | 0.87747043 | 0.0040154932 | 0.0080309864 |
| HPF 77 Hz | 0.010079277 | −1.9898703 | 0.98997140 | 0.99496043 | −1.9899209 |

Measured retail values (session 20261001_211347 MOD lines):
- HPF raw 77 (most voices) or 0;
- LPF 24971 or 25000 (open); some voices 0.7–9 kHz.

### 4.5 Biquad kernel `sub_82B43AF8` [V: 1000 replayed]
- Direct Form I, processed 8 samples at a time.
- Feed-forward: b0·x + b1·x1 + b2·x2 + **1e-18** (a denormal guard, cell 0x822F87B0), using fused single multiply-adds.
  The association order differs between sample 0, sample 1 and samples 2..7 of each group of 8 (matters only for
  bit-exactness).
- Feedback: y = (t − a1·y1) − a2·y2, as two fused negative multiply-subtracts.
- History is written back after the block.
- The recomp runs with flush-to-zero; Xenon hardware may not. This is mostly moot because of the bias. UNCERTAIN.

### 4.6 Gain (Gai0): `sub_82B23B50` [V: 185 replayed]
- Parameter 0 = target gain (+52, linear amplitude, class default 1.0). +56 = applied gain.
- Per block:
  - **restart flag** (the pass's "module not yet reached in this graph's progress" flag, i.e. first/restarted block;
    UNCERTAIN meaning): applied := target first, so no ramp.
  - step = f32(f32(target − applied) × 1/64) (two roundings, never fused).
  - For every channel: out[k] = in[k] × (applied + k·step) for k = 0..63, then × (applied + 64·step) for k = 64..255.
    - All channels start from the same applied value.
    - The kernel's lane arithmetic rounds twice (no FMA), with group multipliers 1..8.
    - applied + 64·step may differ from target by an ulp.
  - Afterwards applied := target; swap.
- So a **64-sample linear de-click on every gain change** (1.33 ms), then flat [PA confirms GAIN_DECLICK 64].
- The retail GAIN trace hook logs +52/+56 (change-only).

### 4.7 GainFader (GaF0): `sub_82B238A8` [T] (grain voices, not the standard voice graph)
- A fade request carries a start time (f64, sample-accurate), a duration, a target and a curve:
  - 0 linear; 1 square-root (rsqrte + one Newton step); else sine (four-lane sine kernel).
- Length = max(1, trunc(duration × rate)) samples. It may span many blocks.
- The envelope (256 values per block) multiplies every channel; the last envelope value becomes the current gain.
  With no ramp running and gain ≠ 1.0, a flat envelope is applied.
- Grain player usage: fader to 0 immediately, then attack to 1 over the attack time; release to 0 over the release
  time.

### 4.8 Send (Sen0): process `sub_82B31838` [V: 933,816 calls], configure `sub_82B31370` [T]

**Fields.**
- +52 target gain (parameter 0); +112 current gain (constructor 1.0); +116 dirty;
- +41 incoming channel count; +64 the bus accumulator; +76 offset of the bus's contributor counter; +78 bus channel
  count;
- **+80..+108 = per channel: last sample of this block × target.** This is the "last contributed value" for the
  release de-click.
- **Our recomp SEND hook prints +80.. as "channel gains"; they are NOT gains** (they read 0 or sample-like ±0.27).
  Fix the hook or ignore that column.

**A Send is a tap.** It does **not** swap: the signal continues unchanged to the next module.

**Linking.** Configure id 0 enqueues a "send to bus" command. It pushes the Send onto the target Sub0's contributor
list and copies the accumulator pointer, counter offset and channel count. Ids 1 (by name) and 2 are not ported.

**Per block.**
1. If the restart flag or +116 is set: current := target.
2. If the bus has 0 channels: set dirty and return.
3. Bump the bus's contributor counter.
4. Mix according to the owner player's mode byte (+72):
   - mode 1: ramp current → 0;
   - mode 3: ramp 0 → target;
   - otherwise: ramp current → target if they differ, else flat.
   - **Flat**: for each route byte (§6.1) of the (source count → bus count) pair, bus[d] += g_route × target × src[s].
   - **Ramp** (`sub_82B46810`): per route, start = g·current and step = g·(target − current)·**(1/65)** (cell
     0x822F8A00 = 0.015384615). Samples 0..63 use start + k·step; samples 64..255 hold start + 64·step.
     - So the ramp **lands at 64/65 of the way** and the next block jumps the last 1/65 (retail arithmetic, keep it).
     - Mode 1 therefore holds current/65 for the rest of the fade-out block.
5. current := target; store the +80 last values.

**Accumulate**: always adds into the bus accumulator.

**Release de-click** (`sub_82B31480`, `sub_82B3C668`) [V]: the +80 last values are mapped through the same route
matrix into the bus layout and added to the bus's delta array. At the bus's next flush they are faded into its first
16 samples with taps 16/17, 15/17, …, 1/17 [IMG 0x822F8A04..]. That smooths the step a removed voice leaves.

**Mode values.**
- PR #4's host sets suspend = 3 and resume = 1. That looks inverted against the kernels (1 = fade out, 3 = fade in);
  retail `sub_82B1E8D8` / `sub_824A2E98` are unverified. **UNCERTAIN**: check before relying on pause behavior.
- **Bad channel counts** 3, 5 and 7 hit empty route ranges and execute a garbage route once. Never send those.

### 4.9 Pan2D1 (Pn21): process `sub_82B29BE0` [T; its parts V]

**Instance.**
- +748 source count (= the voice's channels); +752 destination count (6 for voices);
- 8×8 gain matrix at +444 (row = source, 8 floats);
- parameter cache at +704..+740; law gain at +744;
- speaker tables at +128 (built by `sub_82B44FE8`).

**Constructor arguments.**
- front 30°, side 110°, rear 150°;
- law: class default 2; the voice open passes **0**.
- **Two outputs force front = 90°.** Burnout used 45°/135° [PA]; Skate uses 30°/110°.
- Law gain: 0 → 1.0; 1 → 1/n (n < 6) or 1/(n−1); 2 → 1/√n or 1/√(n−1). **Voices use law 0 → 1.0.**

**Parameters** (no clamping on write) [IMG defaults]:

| id | meaning | default |
|---|---|---|
| 0 | angle A, **degrees, positive = to the right** | 0 |
| 1 | distance D (1 = on the speaker circle) | 1.0 |
| 2 | entry radius about the center (multichannel layout) | 1.0 |
| 3 | turn: degrees added for multichannel entries | 0 |
| 4 | focus: center-extraction weight, 0..1 | 1.0 |
| 5 | level: output weight, 0..1 | 1.0 |
| 6 | **LFE send level**, written raw (not × level or law) | 0.0 |
| 7 | spread 1 (front pair angle) | 30 |
| 8 | spread 2 (side pair angle) | 110 |
| 9 | spread 3 (7.1 only) | 150 |

The names are ours; DWARF lists 7 attributes: azimuth, radius, width, spread, focus, level, center level [PA].

**Internal channel order** (6 outputs): **0 L (+30° internal), 1 C (0°), 2 R (−30°), 3 Ls (+110°), 4 Rs (−110°),
5 LFE**.
- The internal angle is θ = −A·π/180, so A = −30 is L and A = +30 is R.
- The voice azimuth word maps as 65536 = 360°, × 360/65536 degrees (cell 0x822F88E8).
- So **word 0 = front center, increasing words turn clockwise** (to the right). This is consistent with the route
  tables and the interleave [I, PA "positive degrees pans right"].
- **UNCERTAIN**: no impulse test yet confirms that +A is right on a real console.

**Source placement (mono).**
- Position p = D·(cos θ, sin θ).
- Unit-disc clamp on r² = |p|²:
  - r² > 1 → normalize, r² = 1;
  - 0.999 < r² < 1 → r² := 1, position kept;
  - otherwise unchanged.
- D ≤ 0 reflects the stored angle by π.

**Gains** (5 speaker slots + LFE).
1. **Distance spread** (only if r² < 1):
   - w_k = 1 − 0.5·|s_k − p| for each speaker unit vector s_k.
   - front share F = 0.5(x + 1), back share B = 1 − F; each snaps to 0 below 5e-4.
   - Front group: g = √(F / (wL² + wR² + (wC·focus)²)) × {wL, wR, wC·focus}.
   - Back group: g = √(B / (wLs² + wRs²)) × {wLs, wRs}.
   - Every gain × √(1 − (r²)²).
2. **Angular term** (added):
   - Wrap the angle into one of four sectors (6 outputs):
     - front [−30°, +30°) between L and R;
     - [30°, 110°) between L and Ls;
     - [110°, 250°) between Ls and Rs across the back;
     - the remainder between Rs and R.
   - Pairwise VBAP: for speakers at α and β, g_b = sin(θ−α)/sin(β−α) and g_a = sin(β−θ)/sin(β−α), via a
     precomputed inverse 2×2 matrix per pair.
   - **Front sector center extraction**: share = min(gL, gR)·focus; subtract it from both; gC = 2cos(30°)·share =
     √3·share.
   - Scale the pair (or the L/R/C triple) by r² / √(Σ g²) (constant power, amplitude r²).
3. **Final**: s = level × law gain. If r² < 1, also divide by √(Σ g² over the 5 slots). Multiply all by s.

On the circle (D = 1): pure constant-power pairwise panning with center extraction. Inside: a renormalized blend. At
D = 0: L/C/R 0.408 each, Ls/Rs 0.5 each.

**Multichannel sources** (`sub_82B45C50` [T]):
- The layout center is D·(cos, sin)(−A·π/180). The constant used here is 1 ulp different from the mono path's.
- Each entry is placed at center + radius·(cos, sin)(−(A + turn)·π/180 ± spread·π/180), then clamped; its angle =
  atan2.
  - 2 ch: ±spread 1. Spread 1 exactly 90 uses a mirrored pair.
  - 4 ch: ±s1, ±s2.
  - 6 ch: entry 1 at the center angle, 0/2 at ±s1, 3/4 at ±s2; the 6th input is LFE.
  - 3, 5, 7 ch: nothing is placed.
- With the open's defaults (distance 0, radius 1, spreads = speaker angles), source channel k lands on output k.

**LFE.**
- 6 or 8 inputs: the LFE input goes only to the LFE output, at parameter 6.
- Otherwise every source row's LFE column = parameter 6. **Voices default 0, so the LFE output is normally silent.**

**Stereo** (2 outputs): R = (y + 1)/2, L = 1 − R, normalized to unit power × level × law.

**Matrix application, per block.**
- Compare the 10 parameters with the cache (NaN counts as a change).
- Unchanged: apply the matrix flat. Source 0 overwrites each output; other sources accumulate.
- Changed: save the old matrix, rebuild, then apply with a **64-sample ramp per cell**: delta = (new − old)/64, flat
  from sample 64. With the restart flag set, apply flat.
- Then swap.

**Worked gains** (mono, focus = level = 1, LFE 0; columns L, C, R, Ls, Rs) [our f64 model, not bit-exact]:

| A, D | L | C | R | Ls | Rs |
|---|---|---|---|---|---|
| 0, 1 | 0 | 1 | 0 | 0 | 0 |
| −30, 1 | 1 | 0 | 0 | 0 | 0 |
| +30, 1 | 0 | 0 | 1 | 0 | 0 |
| −15, 1 | 0.7071 | 0.7071 | 0 | 0 | 0 |
| −15, 1, focus 0 | 0.9391 | 0 | 0.3437 | 0 | 0 |
| −90, 1 | 0.3673 | 0 | 0 | 0.9301 | 0 |
| +90, 1 | 0 | 0 | 0.3673 | 0 | 0.9301 |
| 180, 1 | 0 | 0 | 0 | 0.7071 | 0.7071 |
| −150, 1 | 0 | 0 | 0 | 0.8374 | 0.5466 |
| 0, 0.5 | 0.4196 | 0.6791 | 0.4196 | 0.3055 | 0.3055 |
| −90, 0.5 | 0.4927 | 0.3226 | 0.2477 | 0.7437 | 0.1970 |
| 180, 0.5 | 0.2411 | 0.2211 | 0.2411 | 0.6461 | 0.6461 |
| any, 0 | 0.4082 | 0.4082 | 0.4082 | 0.5 | 0.5 |

**Verification status.**
- Replayed [V]: clamp, place, distance, angular, final scale, matrix flat/ramped, sin, cos, atan2, floor.
- Unit-tested only: layout of multichannel sources, matrix fill, the process itself.
- Unverified: constructor and speaker tables.

### 4.10 Bus-only modules (not per voice)

| module | behavior |
|---|---|
| **HighShelfIir2 (HS20)** `sub_82B26740` [V: 2,996 replayed] | +52 corner Hz, +60 gain (linear plateau amplitude, A = √gain). Filters only if ω < CEIL **and** gain ≠ 1.0; otherwise bypass (history cleared once). RBJ high shelf, slope S = 1, α = sin ω × 0.70710653 (not exactly 1/√2). Used in the board chain graph 3 (5000 Hz, 0.65) and the stream bus. |
| **PeakingIir2 (PI20)** `sub_82B2C658` [unit-tested only] | +52 Hz, +60 linear gain, +68 Q (clamped 0.2..20 for the math). ω is clamped to [FLOOR, CEIL]. Bypass only when gain == 1.0 exactly. RBJ peaking: A = √gain, α = sin ω/(2Q). Used on material buses and effect returns. Our MOD trace saw 36 PEAK modules around 930–1393 Hz, gain 0.1, Q 20, slowly drifting (probably music/radio processing; UNCERTAIN). |
| **DCl0** hard clip `sub_82B22678` [V] | clamps to ±level; bypass when level ≥ 100 or NaN. Board chain graph 3 uses 0.09. |
| **Del0** delay [T] | fixed delay (0.015 s on the effect-return graphs, cell 0x820D0190). |
| **Sub0** submix `sub_82B34E08` [V] | see §6.2. |
| One-pole stage `sub_82B399D0` [V] | belongs to ReverbModel1 (six instances), not to voices. |
| FrequencyShiftSsb | board chain only (grain/chain notes). |

## 5. MixMap's role per voice

The MixMap (`MixMapSK8.mxb`, EA PathFinder 5.03) runs once per 60 Hz game frame. It turns each SFX object's
controller inputs into 16-bit outputs. Each owner reads them:
- vfunc52 = azimuth word 0..65535;
- vfunc56 = cents → 2^(c/1200) × 4096;
- vfunc60 = 15-bit level & 0x7FFF;
- vfunc64 = filter word 0..25000.

The owner writes them into its held packet words, which the patch program wires to voice properties. So for a
player voice:

| MixMap output | voice property |
|---|---|
| level outputs (/32767) | properties 2/8 (master × dry) → **Gain**; env/effect sends → 5 / 11 |
| pitch output (/4096, unity 4096) | property 0 → **Resample** scale |
| filter outputs (Hz, 25000 = open) | properties 6/7 → **LPF / HPF** |
| azimuth | property 3 → **Pan2D1** parameter 0 |

- The MixMap's own envelopes (attack/hold/release in ms) are the main **level smoothing**. Voice-side smoothing is
  only the 64-sample de-click.
- Exact per-object wiring lives in each bank's patch program, so the evaluator port (Phase B, `aems-evaluator-spec.md`)
  reproduces it by running the program.
- Grain voices bypass properties: owner gain → Sen0 parameter 0, pitch → Rsp0 parameter 0, written each scheduler
  tick.
- Ambience bed: level = MixMap out0/32767 × zone volume; pitch out2/4096; LP out3; send set once.

PR #4's MixMap evaluator matches the capture on about 99.999 % of words; use it as the oracle.

## 6. Buses, master and output

**Superseded in part (2026-10-02):** the env/reverb bus, the master path and the output channel count are recovered in
`aems-env-bus-spec.md`; the material (eEQChain) buses, their EQ re-roll / jitter and the owner one-shot buses in
`aems-eqchain-buses-spec.md`. Both are ported (`skate_audio::bus`).

### 6.1 Channel layouts and route tables [IMG, V]
- **Internal 6-ch order**: L, C, R, Ls, Rs, LFE.
- **Internal 8-ch order**: L, C, R, Ls, Rs, Lx, Rx, LFE.
- Route byte: bits 0–2 destination, bits 3–5 source, bits 6–7 gain index into {1.0, **0.707** (exactly 0.7070000172,
  not 1/√2), 0.5}.
- Ranges per (src, dst) at 0x820ED700, routes at 0x820ED780.

Conversion matrices (unlisted gains are 1):

| src → dst | matrix |
|---|---|
| 1→2, 1→4 | 0.707 to both fronts |
| **1→6/8** | **center only** |
| 2→6/8 | L→L, R→R |
| 4→6 | FL, FR, SL, SR |
| **6→2** | **L = L + 0.707·C + Ls; R = R + 0.707·C + Rs; LFE dropped** |
| 6→1 | L+C+R+Ls+Rs |
| 6→4 | L + 0.707C, R + 0.707C, Ls, Rs |
| 6→8 | identity on 0–4, LFE → 7 |
| 8→6 | Lx folded into Ls, Rx into Rs, LFE → 5 |

### 6.2 Buses [T unless noted]
**Bus = a graph starting with Sub0** [V]. Sub0 owns an accumulator (channels × 256 floats) and a contributor counter.
When its graph runs:
- no contributor and no pending delta → the block is cleared;
- otherwise it copies the accumulator out, folds the release deltas (16-tap fade), and zeroes the accumulator.

The rest of the graph processes the sum like a voice.

| bus | shape and notes |
|---|---|
| **Material buses ×8** (`sub_82490CA0`, manager 0x830CFDEC) | `Sub0 → DCl0 → PI20 → PI20 → Sen0`, 6 ch, order 253, → default bus. Clip level and two peaking EQs come from authored data interpolated in 11 steps (`sub_82491DF0`); **the values are not recovered** (UNCERTAIN). |
| **Effect returns ×2** (`sub_82490270`, order 150; targets of sends 4096/8192/16384) | `Sub0 (1 ch) → Del0 15 ms → PI20 → Sen0 (→ env bus) → Gai0 → Pn21 (1→6) → Sen0 (→ default bus)`. A mono slap-back/early reflection [I]. |
| **Env/reverb bus** `[[manager+52]]` | **not recovered** (ReverbModel1 exists; the world reverb zones come from SFXObj_Reverb, `ems-emitters-re.md`). PR #4 leaves the env sends disabled. |
| **Board grain chain** (`sub_824C8878`) | graph 1: `Sub0 → HI20 → LI20 → FSS → Sen0 (→ graph 3) → Gai0 → Sen0 (→ graph 2)`.<br>graph 3: `Sub0 → DCl0 ±0.09 → Gai0 → HS20 5 kHz/0.65 → Sen0 (→ graph 2)`.<br>graph 2: `Sub0 → Sen0 (→ manager+116) → Sen0 (env, vfunc60(13)/32767) → Pn21 → Sen0 (→ default bus)`. |
| **Default/master bus** (`[0x830CFDBC]+44` / `[0x830775EC]`) | **not recovered** (UNCERTAIN). The output stage reads 8 planes and the 6→8 route maps LFE to plane 7, so retail's master is most likely an 8-channel submix [I]. **PR #4's master (Sub0 → Del0 15 ms, 6 ch, order 255) is its own invention**: it reuses the effect-return delay. Do not copy the 15 ms delay without evidence. |

### 6.3 Output stage (Dac): `sub_82B21F58` → `sub_82B21D98` [V: 800 replayed]
1. **Route planes.** The 8 master planes go to N outputs, where N = global byte 0x8306705D (0 in the static image;
   PR #4 sets 6; the retail runtime value is UNCERTAIN). Output tables at 0x820ED6CC/0x820ED6DC:

   | N | outputs |
   |---|---|
   | 6 | L, C, R identity; Ls+Lx; Rs+Rx; LFE |
   | **2** | **Lo = 0.707·L + 0.5·C + 0.5·Ls + 0.5·Lx; Ro mirrored; LFE dropped** |
   | 4 | L + 0.707C, R + 0.707C, Ls+Lx, Rs+Rx |
   | 1 | 0.707·(L+C+R) + 0.5·(surrounds) |

2. **Interleave** to 256 frames × 6 = 6144 bytes, with plane → slot 0→0, 1→2, 2→1, 3→4, 4→5, 5→3. Device order:
   **FL, FR, C, LFE, SL, SR** (XAudio default).
3. **One-shot fade-in**: if flag byte 0x83067057 is set, frame k < 128 is multiplied by k/128 (cell 0x820300D4 =
   0.0078125), then the flag is cleared. A 2.7 ms fade after an output (re)start [I]; what sets it is UNCERTAIN.
4. **Clamp** every sample to [−1, +1]. Only out-of-range values are stored; NaN passes.
5. **No master gain** in this stage.

Retail's own native mix exceeds 0 dBFS before the clamp on landings: PR #4 measured landing peaks at +0.4 dBFS in the
native 6 ch.

### 6.4 Downstream of the game: XAudio and the host
- The 6144-byte interleaved block is handed to the console audio API. The recomp's host driver (rex
  `sdl_audio_driver.cpp`) receives **planar** 6 × 256 big-endian frames from the guest's XAudio render path.
- **UNCERTAIN**: whether a guest-side XAudio2 mastering stage between the Dac block and the render frame applies
  volume.
- **Recomp capture** (`SKATE3_AUDIO_CAPTURE`, stereo f32 LE 48 kHz) uses the host conversion, **not game code**:
  Lc = 0.4·(FL + SL + 0.5·C), Rc = 0.4·(FR + SR + 0.5·C), LFE dropped. The same conversion feeds stereo playback in
  the recomp.
  - A front-center voice at amplitude a reaches the capture at 0.2a (−14 dB).
  - A hard-left voice reaches it at 0.4a (−8 dB).
  - **Any level comparison against the capture must pass our 6-ch output through this exact conversion.**

### 6.5 Downmix choices for our engine
- **Retail-native candidates** (both are the title's own matrices):
  - (a) device 6-ch, with OS/driver downmix;
  - (b) the output stage's N=2 table (0.707/0.5/0.5, LFE dropped) when the device is stereo. Recommended for stereo
    output: it is the game's own stereo fold.
- **PR #4's host fold is its own choice, matching neither retail table**: L = clamp(0.6·(FL + 0.707·C + 0.5·SL)), LFE
  dropped, OUTPUT_GAIN 0.6 as headroom.
- **For validation against the capture**, always use the rex conversion above.

### 6.6 Voice stealing (Dac budget) [T]
- Each player's cost = a running average of its node times.
- If the Dac load (+224) < 100 and 64 × (smoothed Dac time + Σ player costs) exceeds load × 170666.67 (ticks per
  block), voices are retired (`sub_82B49100`, reason 2) until the budget fits.
- The selector `sub_82B3C440` (which voice) is unknown. rwaudio has ExpelReason "cpu limit" and PRIORITY_PERMANENT
  [PA].
- A native port can ignore CPU-based stealing but needs a voice cap. Per-bank instance capacity (live < capacity) is
  in the evaluator spec.

## 7. Validation plan

**Principles.**
- Measure internally (no A/B comparison pages).
- Use existing captures before asking for new sessions (no repeated play sessions). New retail data comes only from
  short scripted recomp runs.
- Every comparison is per block (256 frames) at 48 kHz.

### 7.1 Per-module golden vectors from the PoC
PoC = a local worktree running upstream PR #4, never committed. Add a local example in its
`crates/skate-audio-core` (keep a local copy of the source).

The example loads the TU3 image (`assets/private/stock/audio-runtime-image/g_8200.bin`), builds module objects with
PR #4's constructors, drives them with synthetic inputs, and writes one binary file per case (input block, parameters
per block, output block). Our crate's tests read those files (gitignored, regenerated locally).

| module | cases | tolerance |
|---|---|---|
| Resample | ratios 0.5, 0.8956, 1.0, 1.1478, 3.99, 4.0+ (clamp); a ratio change mid-stream; 1 and 6 ch; sine 1 kHz and white noise; 64 blocks | **bit-exact** f32 (kernel [V]); fallback max abs error 1e-7 |
| LPF / HPF | fc ∈ {0, 24, 25, 77, 1000, 5000, 23975, 23976, 25000}; a cutoff step mid-stream; bypass ↔ filter transitions (history clear) | coefficients bit-exact with image trig, ≤ 1 ulp with libm; output max abs 1e-6 relative to peak |
| Gain | target steps 1→0.5→0, restart flag, multichannel | bit-exact |
| Send | flat, ramp (64/65 landing), modes 1/3, 1→6 / 6→6 / 6→2 routes, release de-click (16 taps) | bit-exact |
| Pan2D1 | A ∈ {−180..180 step 15}, D ∈ {0, 0.5, 0.9995, 1, 2}, focus {0, 1}, LFE 0/0.5, 1/2/4/6-ch sources, parameter-change ramp | gains abs 1e-6 (f64 trig) or bit-exact (image trig) |
| SndPlayer1 | one-shot end, loop wrap, two queued records, scheduled start mid-block, stop fade (16 frames), format change (silent block) | sample-exact positions; values bit-exact |
| Output stage | 8→6 and 8→2 routes, interleave order, clamp, NaN pass-through | bit-exact |

### 7.2 Whole-voice renders (PoC vs ours)
- Use the PoC's authored runtime (`AuthoredRuntime::post`/`pump_once`) to open real bank voices: one-shots (pop
  0x449, landing 0x447), a looping bed, a pitched grind, a panned emitter.
- Log per block: the voice's property pushes (id, value, block index) and the final 6-ch Dac block.
- Feed the same property timeline into our graph.
- **Pass criteria per voice**:
  - per-block RMS within 0.1 dB;
  - per-block peak within 0.1 dB;
  - sample-level max abs error ≤ 1e-5 × full scale (larger only where PR #4 approximates, e.g. decode differences);
  - onset sample within 0 frames.
- Run with the master delay and host downmix off on both sides (compare the native 6 ch).

### 7.3 Against retail captures
**Data.**
- Recomp session `20261001_211347` (music and NPC speech off): `audio.f32` (stereo, rex conversion),
  `trace.tsv` with PLAY, GAIN, SEND, MOD (PITCH/LPF/HPF/SHELF/PEAK), POST, SPLC, CONTACT, CSET, and `CAPTURE <frames>`
  every 24064 frames for alignment.
- Older sessions have music (contaminated).

**Per-voice parameter checks** (no audio needed):
- Run our evaluator on the same POST timeline. Compare per voice: Gain target (GAIN), pitch requested/scale/ratio
  (MOD PITCH), LPF/HPF Hz (MOD).
- Tolerance: exact words at evaluator ticks; timing within one tick (32 ms).
- The SEND hook's cells are last samples × target, not gains (§4.8). Use only its target column.

**Mix-level checks.** Render our 6 ch, apply the rex conversion, and compare windowed levels with `audio.f32`:
- use the same edges as PR #4 §8: p50 of a 12-frame window after takeoff/landing edges versus the p50 of rolling
  stretches;
- PR #4's numbers (**rolling −22.9 dBFS, takeoff +17.0 dB, landing +23.3 dB over the bed**) were measured on the
  **native 6-ch** output pass (hook on `sub_82B21F58`, the peak of the 6-ch block), not on the capture;
- **re-measure them in capture space** before using them as capture targets. Expect absolute levels several dB lower
  (rex 0.4 factor; center at 0.2).
- **The relative numbers (+17 / +23.3 dB) are the primary criterion. Tolerance ±1.5 dB** (random sample choice and
  physics differences dominate).
- Band profile per event class: ±1 dB per octave, 60 Hz–8 kHz.

**Missing hooks** (cheap recomp additions, if needed):
- a 6-ch native capture (the `OUT` hook on `sub_82B21F58`) to remove the rex conversion and recover LFE;
- a Pan2D1 matrix dump (+444) per voice;
- a Send hook fixed to log +52/+112 and the route pair;
- the output-count byte 0x8306705D at runtime (settles 6 vs 2, and the 8-plane master question).

**Helper**: `tools/recomp-trace/send_vectors.py` summarizes SEND lines. It found that +80 is not a
gain: all 6-ch cells read 0, and mono cells look like samples.

## 8. Open questions (priority order)
1. The master/default bus graph and any gain or effect on it; the runtime output channel count (0x8306705D; system
   +252); whether a guest XAudio2 mastering volume sits between the Dac and the capture.
2. The env/reverb bus graph (ReverbModel1 parameters) and the material buses' authored DCl0/PI20 values.
3. The sign of the azimuth on real speakers (positive = right is inferred), and the Pan2D1 restart flag meaning.
4. Descriptor byte 2 (×0.01 → player +56): gain or not.
5. The suspend/resume Send modes (1/3 possibly inverted in PR #4).
6. SndPlayer1 pre-roll/skip and span fields per sample; the admission check; the release decay tail before expel.
7. Resample look-ahead byte (+81) value and tail length (6 floats per channel per PA); the ratio ceiling cell value.
8. The PeakingIir2 port has no replay vectors; verify it against the image before the bus port.
9. The 512/2048 routing buses, and the voice-steal selector.
