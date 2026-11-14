# MixMap (`MixMapSK8.mxb`): behavioral specification for a native Rust port

Written 2026-10-02 in our own words (no code transliterated from PR #4/#1, the recomp or b5-decomp; constants,
offsets, addresses and EA's record names are facts). Companion notes: `aems-reference.md`,
`aems-voice-graph-spec.md`, `ems-emitters-re.md`, `rwaudio-prior-work.md`.

**Status: the evaluation model below is proven against PR #4's port.** Our own reference evaluator
(`mxb_tool.py`, a local tool written from this spec; not published because it carries a name table
taken from the game) gives the same owner reads as
PR #4's MixMap port (run through the PoC) on 2 scripted scenarios: 900 evaluations × 41 watched outputs each,
73,800 cells, **0 mismatches**. The scripts cover fades, envelopes, pause, menu, reverb and HOM ducks, distance
and angle lookups, Doppler, a disabled controller, and dt changes. PR #4 in turn reports 99.9991 % word-exact
agreement with a retail capture (84 M output words). So: spec ≡ PR #4 ≈ retail. What is *not* proven here is
the game side: which game values feed which inputs (§7) and what reads each output (§6).

Terminology: EA's own record names come from the NFS ProStreet PDB, as reproduced in b5-decomp's
`NFSMix/NFSMixRecords.hpp` (same lineage: 564-byte host, 96-byte state instance, same field order).
Our letters: **A** = MixCtl (input product), **B** = 3DMixCtl (distance/angle), **F** = EvtMixCtl (envelope),
**C** = SubMixCh (clamped sum), **E** = MasterMixCh (output sum), **G** = Preset (output conversion list).
"Slot" = EA "state" = one SFX slot of the game (14). All file data is big-endian.

---------------------------------------------------------------------------------------------------------
## 1. Where it sits in the frame

- **Load** once at audio-system init (`sub_82484FE8` → manager init `sub_8294B918`, file load `sub_8298ED88`,
  build `sub_8294BA18`). The host object is 564 bytes (vtable `0x82316B78`). The build needs, per slot, the
  number of instances the game created (§4.1).
- **Per game frame**, in `sub_82485190`:
  1. Inputs are written. The audio-state bridge `sub_824B0DA8` ends in `sub_824B19C8`, which writes the
     PlayerPhysics inputs. Then every SFX object's process slot (+36) writes its own controller's inputs.
  2. The MixMap tick runs (`sub_8294BAE8` → host slot 2 `sub_8294F5E8(host, dt)`).
  3. Every SFX object's update slot (+40) reads **this** tick's outputs and posts or updates its AEMS
     packets and voice graphs.
  - An input written during step 3 (e.g. SkateBoard input 4 in `sub_824C6198`) is seen one tick later.
- **dt / rate** (PR #4's reading of `sub_82485190`, threshold `0x822F8DE8` = 0.02 s):
  - If the call's dt ≤ 0.02 and its r5 flag is 0, the two halves (1 and 2+3) alternate between calls,
    each using the sum of the last two dts.
  - Otherwise both halves run on every call.
  - In the recomp capture, calls came every ≈7.4 ms, so the MixMap ran every ≈14.7 ms.
  - **Port rule:** run inputs → tick → reads once per 60 Hz frame with dt = 1/60. That is what retail does
    for any dt > 0.02.
  - Only envelopes use dt. The Doppler slew is per evaluation, so its speed depends on the frame rate (§5.4).
- **Mode word** (EA `m_CurCamState`, host +128; previous +124): taken from the manager's +4 at every tick. It
  selects the B "camera state" variant. In retail it stays −1, so variant 0 is always used. Every B record
  in this file has exactly one variant, so the mode has **no effect** for `MixMapSK8.mxb`.

---------------------------------------------------------------------------------------------------------
## 2. File format

### 2.1 Header (EA `stMixMapHeader`, 16 bytes)
| off | type | value in SK8 | meaning |
|---|---|---|---|
| 0x00 | u32 | 0 | MixMapID. Also mixed into the build's sharing key (no visible effect). |
| 0x04 | u32 | 14 | NumStates = number of slots |
| 0x08 | u32 | 0x10 | offset of the slot table |
| 0x0C | i32 | −1 | DynamicMapOffset (unused) |
| 0x10 | i32[14] | 0x44, 0x2A48, 0x42D8, 0x4418, 0x461C, 0x4B1C, 0x534C, 0x54E0, 0x5640, 0x5850, 0x5BA8, 0x5CE4, 0x5D04, 0x5E10 | section offset per slot (from file start; −1 = none) |

Quirk: slot 0's section starts at 0x44, so its first word overlaps slot 13's table entry. That word
(StateIndex) is never read; in the other sections it is `0x001F0000 | slot`.

### 2.2 Section header (EA `stMixMapStateHdr`, 32 bytes at section offset S)
`+0` StateIndex (unread). Then offsets from S, each −1 when absent:
`+4` A, `+8` B, `+12` C, `+16` E, `+20` G, `+24` F, `+28` reserved (−1).

### 2.3 Tables. Each starts with a 16-byte header; records follow at +16.

**A: MixCtl (input products).** Header `{count, count again (NumNewMixDataProcs), 0, 0}`.
Record = (2 + n) words:
- `w0` = input key (§3). Bits 24–27 are the curve kind applied to the input.
- `w1` = bits 16–20: n, the number of scale references; low 16 bits: the **swing** (depth).
- then n reference keys.

**B: 3DMixCtl (distance/azimuth lookups).** Header `{count (low byte), 0, 0, 0}`. Record = 4 + 24·v bytes:
- `w0` = controller selector: bits 24–27 = v (variants, always 1 here); slot/object bits name the SFXCTL
  controller whose input block is read (§3).
- v × 24-byte variants (EA `st3DStateParams`):
  - `+0`: bits 24–27 camera-state id; bits 16–23 slot; bits 12–15 distance select (0 → input 1,
    1 → input 0, ≥ 2 → −1.0); bits 8–11 azimuth select (0 → input 3, 1 → input 2, ≥ 2 → 0);
    low byte = record index.
  - `+4`: curve kind per quadrant: q0 bits 28–31, q2 bits 24–27, q3 bits 20–23, q1 bits 16–19. The low 16
    bits are the Doppler speed constant c (0 = no Doppler).
  - `+8, +12, +16, +20`: one range per direction d0..d3. Bits 0–14 = min distance; bits 16–30 = max
    distance (integers, meters).

**C: SubMixCh (clamped sums).** Header `{count, …}`. Record = (2 + n) words:
- `w0` = `0xD0` top byte (type C), bits 16–23 = n, bits 8–15 = slot, low byte = index.
- `w1` = bits 16–30: upper clamp; low 16 bits (signed): lower clamp.
- then n reference keys.

**E: MasterMixCh (output sums).** Header `{count, number of distinct destination controllers
(NumUniqueSFXOBJs), …}`. Record = (3 + n) words:
- `w0` = top byte `0xC0 | type` (0 volume, 1 pitch, 2 filter); bits 16–23 = n; bits 8–15 = slot; low byte
  = index.
- `w1` = high 16 bits (signed): base value in mB/cents. The low half is always 0xD8F0 (−10000) and unread.
- `w2` = destination controller key (§3). Records with the same destination are consecutive in this file.
- then n reference keys. B references ("specials") always come first.

**G: Preset (output conversion list).** One entry per E record, in order. Each entry is a head word plus
`count` output words:
- head: bits 24–27 = type (same as E), 16–23 = slot, 8–15 = E index, 0–4 = count.
- output word:
  - bit 31: raw-azimuth flag;
  - bits 26–30: output id (0..29);
  - bits 21–25: special index (which B reference; ≥ the number of specials = none);
  - low 16 bits (signed): offset added to the sum.

**F: EvtMixCtl (envelopes).** Header `{count, …}`. Record = (6 + n) words (EA `stMixEvtParams`):
- `w0`:
  - bits 24–27: kind (0 = AR, 1 = AHR, 3 = ADSR; other kinds are inert);
  - bits 16–23: slot;
  - bit 10: retrigger;
  - bit 9: "linear", where the level is used directly as gain (no record in SK8 sets it);
  - bit 8: gate: hold while the trigger is on;
  - low byte: index.
- `w1` = bits 16–19: n scale references; low 16 bits: the swing.
- `w2` = trigger key (§3). A non-zero value means "on".
- `w3` = attack: bits 0–11 frames, bits 12–15 curve kind.
- `w4` = decay frames in bits 16–27; low half: bits 0–11 hold frames, bits 12–15 decay curve kind.
- `w5` = sustain level in bits 16–31 (signed Q15); release frames in bits 0–11; release curve kind in
  bits 12–15.
- then n reference keys.
- Times are 60 Hz frames, converted to ms with the image float at `0x822F8F04` (16.6667). Attack and release
  of kinds 0/1/2, and hold of kind 1, are raised to at least 1 frame at build time.

### 2.4 Counts in `MixMapSK8.mxb` (records per slot; instances from §4.1)
| slot | name | inst | A | B | F | C | E | output ids written |
|---|---|---|---|---|---|---|---|---|
| 0 | Global | 1 | 180 | 1 | 245 | 12 | 65 | Announcer, Music, Master, CameraMan, Reverb, NIS, Bloom |
| 1 | Player | 2 | 122 | 14 | 37 | 18 | 92 | SkateBoard 0–27, Contacts 0–22, Wheels 0–7, Rail 0–6, Cracks 0–6, Tricks 0–8, Clothing 0–6, Treatments 0–6, SenseOfSpeed 0–5, OffBoard 0–16, HandGrabs 0–4, Takedown 0–5, DropIn 0–5 |
| 2 | Ambience | 1 | 3 | 0 | 0 | 1 | 4 | Ambience 0–3 |
| 3 | Collision | 10 | 2 | 2 | 1 | 1 | 5 | Collision 0–22 |
| 4 | Traffic | 4 | 12 | 14 | 0 | 4 | 19 | Engine/Skids/Horn/Woosh |
| 5 | Pedestrian | 15 | 14 | 24 | 0 | 7 | 29 | Speech/SFX/BodyFall/Tazer |
| 6 | Emitter | 5 | 0 | 1 | 0 | 2 | 8 | Emitter 0–8 |
| 7 | Crowd | 5 | 2 | 3 | 0 | 2 | 3 | |
| 8 | Dynamic | 5 | 1 | 1 | 0 | 2 | 9 | Dynamic, Moving |
| 9 | Speaker | 3 | 12 | 12 | 0 | 1 | 12 | |
| 10 | Whoosh | 3 | 2 | 2 | 0 | 1 | 2 | |
| 11 | ObjectInstance | 0 | n/a | n/a | n/a | n/a | n/a | (empty section) |
| 12 | NISCharacter | 5 | 0 | 1 | 0 | 2 | 5 | |
| 13 | PlayerSpeech | 7 | 13 | 14 | 3 | 2 | 9 | |

Instantiated: 547 input entries, 853 A, 635 B, 350 F, 240 C, 1044 E; 154 output blocks; **247 controllers**,
the same count as the retail capture. Envelope kinds: 281 AHR, 4 ADSR, 1 kind-4 (inert); 24 retrigger,
11 not gated. Full record dump: `catalog.txt` (and `.json`); every output with its type
and base: `outputs.txt`.

---------------------------------------------------------------------------------------------------------
## 3. Keys (32-bit identifiers everywhere)

| bits | meaning |
|---|---|
| 29–31 | type: 000 A, 001 C/E (bit 28: 1 = C, 0 = E), 010 SFXObj controller, 011 SFXCTL controller, 100 B, 101 F |
| 24–27 | input keys: curve kind (0–9). In records: other per-type fields. |
| 16–23 | slot |
| 11–15 | instance ("group"); always 0 in the file, filled in at build |
| 4–10 | controller keys: object id within the slot |
| 0–3 | controller keys: input id (0–15) |
| 0–7 | node keys (A/B/C/E/F): record index in that slot's table |

- Controller identity = key & `0xE0FFFFF0` (type + slot + group + object).
- SFXObj keys look like `0x4SSG GOO0`, SFXCTL keys `0x6…`. For example `0x40020000` = Ambience,
  `0x40060000` + g·0x800 = Emitter instance g, `0x60010000` = player 0's PlayerPhysics (audio state),
  `0x60010010` / `0x60010020` = the player's two 3DObjPos, `0x60060000` = emitter 3-D input.
- Object names per (slot, id) are the game's factory table (`0x8302CF60..0x8302D3A0`), listed in `mxb_tool.py`.

---------------------------------------------------------------------------------------------------------
## 4. Build (load-time)

### 4.1 Instances
- Every slot with a section is instantiated once per game object of that slot: the manager's slot-8
  callback returns `[sys[164+slot]+20]`.
- Retail free-skate counts: `[1, 2, 1, 10, 4, 15, 5, 5, 5, 3, 3, 0, 5, 7]`. PR #4 derived these from the
  capture's 247 keys; we cross-checked that slot 6 = 5 matches the CSTATEMGR_Emitter pool of 5.
- Each instance has its own copy of every A, B, F, C and E record of its slot, with its group number g in the
  keys.

### 4.2 Reference expansion
A reference to node or controller X in slot s, made from a record of slot o in instance g:
- s == o → X in the same instance g (exactly one reference);
- s ≠ o → **one reference per instance of slot s**, all of them. They are all summed (C/E) or multiplied
  (scale lists).
- For A and F records, o is taken from the record's own key byte. In this file that always equals the
  section slot.
- Example: Ambience's sum contains `A[Player.5]`, which expands to player 0's and player 1's copies.

### 4.3 Input entries
- One entry per distinct input key (curve bits and group included): `{key, source, mB = 0, lin = 32767}`.
- Entries are stored in runs by curve kind (all kind-0 entries, then kind 1, …, kind 9). Within a run they
  keep their first-use order.
- This order matters (§5.1).

### 4.4 Controllers and blocks
- A controller is 16 bytes `{vtable 0x82316B54, key, inputs*, outputs*}`.
- The **input block** is 16 × i32, zeroed. It is created for any controller named by an input, reference,
  trigger or B record.
- The **output block** is 16 × u32, zeroed. Word 15 = enable, set to 1 at build. It is created for every E
  destination.
- Output id n lives in word n>>1: even n in the low 16 bits, odd n in the high 16 bits.
- Retail hands each new controller to the game (manager slot 4 = `sub_82485850`). The game stores it at the
  owning SFX object's `+12`, which is what the owner readers dereference.
- A port can simply look controllers up by key.

---------------------------------------------------------------------------------------------------------
## 5. Evaluation (one tick, in this exact order)

All math is i32 with arithmetic `>>` unless marked f32. "trunc" = round toward zero (fctiwz). Values are
Q15 linear (0..32767), mB (1/100 dB, 602 per halving), or cents.

### 5.0 Shared helpers and tables
- **POW[r]**, r = 0..601: `int(32768·10^(−(r+1)/2000))`, except POW[601] = 16384. Exact against the image
  (read backwards from `0x82FDBFAC`).
- **mB → lin(m)**, used only for m ≤ 0:
  - k = trunc(m / −602); rem = −m − 602·k;
  - k outside 0..15 → 0; otherwise POW[rem] >> k.
  - So 0 mB → **32730**, not 32767. −10000 mB → 0.
- **LOG[i]**, i = 0..511 (`0x82FDBFB0`): max(0, 601 − #{r : POW[r] ≥ 16384 + 32·i}). Exact.
- **lin → mB(x)**:
  - x ≤ 0 or x > 32767 → −10000.
  - Otherwise c = 14 − floor(log2 x) (0..14).
  - idx = (x − 2^(14−c)) >> (5−c) for c ≤ 5; else (x << (c−5)) − 512 + 2^(c−5) − 1.
  - Result: LOG[idx] − 602·(c+1). So 32767 → −1, 16384 → −602, 1 → about −9030.
- **CURVE[i]**, i = 0..511 (`0x82FDC7B0`): floor(32767·cos(π/2·i/511)). Exact. Entry 512 reads as 0.
- **shape(x, kind)**, x a Q15 input:
  - kind 0: i = x>>6. If i < 0 or i ≥ 511 → 0. Otherwise
    t + (((CURVE[i+1] − t) · frac) >> 15), with t = CURVE[i] and frac = 1023 + 1024·((x>>1) & 15).
    Retail quirk: frac only uses x bits 1–4, so it interpolates at most halfway between entries.
  - kind 1: shape(32767−x, 0)
  - kind 2: s0² >> 15
  - kind 3: kind 2 of (32767−x)
  - kind 4: a = 32767 − CURVE[511 − (x>>6)], b = 32767 − CURVE[512 − (x>>6)], result a + ((b−a)·frac >> 15)
  - kind 5: kind 4 of (32767−x)
  - kind 6: s4² >> 15
  - kind 7: kind 6 of (32767−x)
  - kind 8: 32767 − x
  - kind 9: x
  - kind ≥ 10: 0
  - In words, over u = x/32767: 0 cos↓, 1 sin↑, 2 cos²↓, 3 sin²↑, 4 1−sin↓, 5 1−cos↑, 6 (1−sin)²↓,
    7 (1−cos)²↑, 8 1−u↓, 9 u↑.
- **curve01(kind, f)**: shape(trunc(f·32767), kind) · (1/32767), in f32.
- **cents → ratio(c)** = 2^(c/1200), built from whole octaves (×2 or ÷2) × SEMI[(|c| mod 1200) div 100] ×
  FINE[|c| mod 100]. For negative c the factors are divided out.
  - SEMI = f32(2^(i/12)) (`0x82FDCFB4`).
  - FINE = f32(2^(i/1200)) (`0x82FDCFE8`). Entries 15, 36, 56 and 65 are 1 ulp off naive f32 rounding, so
    read them from the image or patch them.
- **ratio → cents(r)**: r > 1 → trunc(f32(lin→mB(trunc(32767/r)) × K⁻)); otherwise
  trunc(f32(lin→mB(trunc(32767·r)) × K⁺)). K⁻ = −1.99316 (`0x822F8AE0`), K⁺ = +1.99316 (`0x822F8AE4`)
  (cents per mB).
- `mxb_tool.py tables` checks the generated tables against the image: all identical except those 4 FINE ulps.

### 5.1 Input stage
For each input entry, in run order (§4.3):
- raw = the source value in **linear flavor**:
  - controller → the raw i32 input word;
  - A key → the referenced product's *shaped input* (its entry's lin, not the product value);
  - B → its Q15 roll-off;
  - F → its level (Q15);
  - C/E → their value.
- lin = shape(raw, kind); mB = lin→mB(lin).

Entries that alias another entry (A-type input keys, e.g. Ambience A1 reads A2's input) see that entry's
value from this tick only if it sits in an earlier run. Otherwise they see last tick's value (one tick of
lag, or the initial 32767 on the first tick).

### 5.2 A: MixCtl products (per instance, in build order: slot, instance, record)
- From the swing w (low 16 bits of w1):
  - w bit 15 clear ("boost"): offset = w; ratio = 32767 − (mB→lin(−w)).
  - w bit 15 set ("cut", w is negative mB): offset = 0; ratio = 32767 − (mB→lin(w)).
- x = 32767 − (((32767 − lin)·ratio) >> 15); dB = offset + lin→mB(x).
- Result: a cut gives ≈ w mB at lin 0 and ≈ 0 at lin 32767; a boost gives ≈ 0 → ≈ +w.
- Scale list: acc = 32767; for each reference, acc = (v·acc) >> 15, with v in **linear flavor**.
- **value = (acc·dB) >> 15.** The scales shrink the dB amount, which acts as a gate or depth control.

### 5.3 B: 3DMixCtl lookups (per instance)
Read the instance's SFXCTL input block `in` (§7.3).
- If in[15] bit 0 is clear (inactive): dB = −10000, lin = 0, azimuth = 0, Doppler = 0. Done.
- d = f32 distance per the variant's distance select. az = the azimuth input, a u32 (0..65535).
- q = (az >> 14) & 3; f = az − 16384·q. The range pair (a, b) for q = 0..3 is (d0,d1), (d1,d2), (d2,d3),
  (d3,d0). kind = the quadrant's curve.
- If d > max_a **and** d > max_b: dB = −10000, lin = 0, Doppler = 0 (the azimuth is kept). Done.
- ra = trunc(f32((clamp(d, min_a, max_a) − min_a) / (max_a − min_a)) × 32767); rb likewise on b.
  - Note: both use the same distance.
- ca = shape(ra, kind); cb = (f ≠ 0) ? shape(rb, kind) : 32767; w = 2f.
- **lin = ((32767 − w)·ca >> 15) + (w·cb >> 15); dB = lin→mB(lin).** The roll-off is interpolated by angle
  between the two directional ranges.
- **Doppler**, only when c ≠ 0:
  - s = in[13] if distance select = 1, else in[14] (f32 signed relative speed, negative = approaching);
    flag = bit 31 resp. bit 30 of in[15].
  - If the flag is set: clear it **in the input block** and use target = 0.
  - Otherwise span = s + c, using c instead when span ≤ 0, and target = ratio→cents(c/span).
  - Then Doppler += trunc(0.2·(target − Doppler)). Written exactly: D −= trunc(f32((target − D)·(−0.2))),
    with −0.2 = `0x8208EA7C`.
  - This slews per evaluation, regardless of dt.
- Also tracked, but unused: previous distance and |Δdistance|.
- File facts:
  - Distances span 0–100 m; c ∈ {275, 340, 380, 520, 554, 866, 1557, 3740}.
  - Only the select pairs (0,0), (0,1) and (1,0) occur.
  - No range has min = max (that would divide by zero).

### 5.4 F: envelopes (per instance)
State: stage (0 off, 1 attack, 2 decay, 3 sustain/hold, 4 release); el = elapsed ms (f32); t0 = stage start
offset (ms); start = level at stage start; level (Q15); out (mB).
- **Idle:** stage 0 and trigger 0 → reset everything; out = 0 (or −10000 if linear). Skip.
- Otherwise el += dt·1000 (f32), then step the stage machine (loop until a stage stops):
  - Progress p = (el − t0)/(span − t0) when span − t0 > 0, otherwise el − t0.
  - **Attack** (stage 0 counts as 1):
    - If gated and the trigger is off (kinds 1/3 only): carry into release.
    - If el < A: level = start + trunc(curve01(attack curve, p) · (32767 − start)). A zero-length attack
      (kind 3: A ≤ 16.666 ms) gives level 32767.
    - Otherwise enter the next stage at level 32767: AR → release, AHR → hold, ADSR → decay.
  - **Decay** (ADSR):
    - Gated and trigger off: release, carried proportionally from the decay.
    - el ≤ D: level = start + trunc((1 − curve01(decay curve, 1 − p))·(S − start)).
    - Otherwise sustain at S.
  - **Hold / sustain** (AHR stage 3 at 32767, ADSR stage 3 at S):
    - If gated: stay while the trigger is on (el is pinned to 0); release when it goes off.
    - If not gated: release once el > H.
  - **Release:**
    - el ≥ R → reset to off (level 0).
    - Retrigger flag and trigger on → carry back into attack.
    - Otherwise:
      - AR: level = start − trunc(curve01(release curve, 1 − p)·start).
      - AHR/ADSR: level = start − trunc((1 − curve01(release curve, 1 − p))·start).
  - **Carry** from a stage of length L into stage X of length M: when L < 16.666 ms (`0x822F8ADC`), start X
    at its end (t0 = el = M). Otherwise t0 = el = ((L − el)/L)·M. In both cases start = the current level.
  - **Enter** = el = 0, t0 = 0, start = the given level.
- **Output:** with ratio/offset from the swing as in A:
  - cut (swing ≤ 0 as signed): out = lin→mB(32767 − ((level·ratio) >> 15)). Full level → ≈ swing mB
    (a duck).
  - boost: out = offset + lin→mB(((level·ratio) >> 15) − ratio + 32767).
  - linear: out = lin→mB(level).
  - A scale list multiplies the mB amount (or the linear gain, in linear mode) as in A.
- Kinds other than 0/1/3 never leave stage 0 (level 0, so out = 0 mB). F184 is kind 4.

### 5.5 C: sums (per instance, build order)
- value = Σ references in **mB flavor**:
  - A → its value;
  - B → its dB;
  - F → its out;
  - C/E → their value;
  - controller → the raw input word.
- Clamp to [lower, upper].
- Records read whatever is currently stored. An earlier record has this tick's value; a later index or a later
  stage has last tick's value (e.g. Global C3 reads C4 from the previous tick).

### 5.6 E: output sums
- Output block disabled (word 15 bit 0 = 0) → value = −10000.
- Otherwise value = base + Σ non-B references (mB flavor). B references are not summed; G uses them.

### 5.7 Output write (per E record, per G output word)
v = value + offset; b = the special B reference, if any.

| type | no special | with special B |
|---|---|---|
| 0 volume | mB→lin(clamp(v, −10000, 0)) → 0..32730 | mB→lin(clamp(b.dB + v, −10000, 0)) |
| 1 pitch (cents) | clamp(v, −4800, 2400) | x = b.Doppler + v; x > 2400 → 2400; **x < −4800 → 0** (quirk) |
| 2 filter (Hz) | trunc(f32(cents→ratio(clamp(v, −10000, 0)) · 25000)) → 77..25000 Hz | clamp(v, −10000, 0), not converted |
| 3 / ≥5 raw | clamp(v, 0, 25000) | the sum unchanged |
| any + raw-azimuth bit | n/a | b.azimuth & 0xFFFF |

- Type 4 behaves as type 0. Only types 0–2 occur in this file.
- Disabled block: only the record's first output id is written: −10000 for volume (owners read it as
  0x58F0 = 22768!), 0 for pitch, 25000 for filter.
- Typical offsets: filter −1 or −2 cents → 24985 / 24971 Hz (LPF open); −19931 → clamped to −10000 → 77 Hz
  (HPF "off").

---------------------------------------------------------------------------------------------------------
## 6. Outputs → audio objects

### 6.1 Readers
Owner virtuals 52/56/60/64 read `[[owner+12]+12]` = that object's output block:

| reader | function | returns | used for |
|---|---|---|---|
| vfunc52 | `sub_824C2870` | half & 0xFFFF | pan angle (0..65535 = 0..360°; graphs scale by 360/65535) |
| vfunc56 | `sub_824C5910` | trunc(cents→ratio(half as signed) · 4096) | pitch, 4096 = 1.0 (256..16384) |
| vfunc60 | `sub_824AF240` | half & 0x7FFF | gains and sends, /32767 |
| vfunc64 | `sub_824AF240` | half & 0x7FFF | filter cutoffs in Hz |

Controller slot +16 (`sub_8294BCC0`, type 0–4) does the same conversions.

### 6.2 Ambience (`0x40020000`, 1 instance). Input 0 is written by the bed state machine `sub_824D3C28`.
The fade: 0 = bed fully up, 32767 = silent.

| out | type | formula (mB/cents before conversion) | read by |
|---|---|---|---|
| 0 | volume | −1100 + C0 + Σplayers A[Player.5] + F35 + F73 + A0 | bed gain = out0/32767 × zone volume (`sub_824D42C8`) |
| 1 | volume | −1400 + C0 + A1 + Σplayers A[Player.5] + F73 | crossfade packet w0 = w8 = clamp(out1 × level) (`sub_824D0F38`) |
| 2 | pitch | 0 | bed pitch = out2 via vfunc56 / 4096 (always 1.0) |
| 3 | filter | −2 + F36 + F40 + F211 (all Global) | bed LI20 cutoff (24971 Hz open) |

- **A0** = in0 through cos, cut −10000: in0 = 0 → 0 dB; in0 = 32767 → ≈ −90 dB.
- **A1** = A2's shaped input, where A2 = in0 through 1−u, then cos, cut −10000. This is the complementary
  equal-power crossfade. It reads A2's value one tick late (§5.1).
- **C0** (clamped −10000..0) = Global ducks:
  - A1 (Master.in2 = 0 → −100 dB);
  - A41 (Master.in4, gated by NIS.in2 / Master.in0);
  - A54 (VU.in0 −7 dB);
  - F1 (Master.in6), F4 (NIS.in1), F13 (Pause.in0, × A15/A16), F41 (Reverb.in1 −2 dB);
  - F65 / F66 / F91 / F121 / F139 (Menu.in0 / in1 / in3 / in2 / in8);
  - F122 (Music.in5 −2 dB × Master.in1);
  - F148 / F149 / F150 / F201 (Challenge.in8 / in7 / in9 / in1);
  - F186 / F210 / F233 (NIS.in8 / in6 / in9).
- **A[Player.5]** = PlayerPhysics.in1 (speed, 30 km/h full scale) through 1−sin, cut −300, scaled by A9
  (= 32767 − PlayerPhysics.in9). **Correction 2026-10-02:** in9 is 0 for the local player (PoC writer
  `[[state+16]+72]` set → 0; confirmed by the board's high-pass: 77 Hz = retail modal only with in9 = 0,
  1109 Hz with 32767), so for the local player this speed duck (up to −300 mB) is active; it is 0 for
  the second player.
- **F35** = Reverb.in2, −200. **F73** = HOM.in0 (× A98). **F36 / F40 / F211** = Reverb.in2 / in1 / in6 low-pass
  ducks of −4374 / −3406 / −3671 cents (to ≈ 2.0 / 3.5 / 3.0 kHz).

### 6.3 Emitter (`0x40060000` + g·0x800, g = 0..4; one per CSTATEMGR_Emitter state)
B0 reads SFXCTL `0x60060000` + g·0x800:
- distance = input 1, azimuth = input 3;
- all quadrants 4..70 m with curve 1−sin;
- Doppler c = 3740 (unused: no pitch output uses B0).

| out | type | formula | c_emitter slot (`sub_824DCF08`) |
|---|---|---|---|
| 0 | raw | azimuth of B0 | +16 (w3 pan), positional branch |
| 1 | pitch | 0 | +20 (w4 pitch), non-positional |
| 2 | volume | −988 + C0 + F21 (Reverb.in1 −100 dB) | +8 (w1 dry) × level, non-positional; forced 0 when `[*(0x820CFDBC)]+104` ≠ 0 |
| 3 | filter | −2 + F40 + F36 + F211 | +24 (w5 LPF), non-positional |
| 4 | volume | −600 + C0 (**no distance roll-off**) | +8 (w1 dry) × level, positional |
| 5 | pitch | 0 | +20 (w4), positional |
| 6 | filter | −2 (open) | +24 (w5), positional |
| 7 | volume | −2100 + C1 (= C0) | +12 (w2 send) × level, non-positional |
| 8 | volume | −2600 + C1 **+ B0 dB** | +12 (w2 send) × level, positional |

- C0 = Global ducks: A13 (Master.in4), A1 (Master.in2); F1, F4, F10, F11 (Pause.in0), F65, F66, F71, F90,
  F91, F118 (Menu/HOM), F139, F168, F180, F186, F201, F210.
- Mapping the slot offsets to payload words assumes the 4-byte packet header (the `ems-emitters-re.md`
  payload: w1 dry, w2 send, w3 pan, w4 pitch, w5 LPF).
- Consequence: a positional emitter's dry level falls off only by the emitter's own shape level ((1−d)² etc.).
  The MixMap rolls off the **send** with camera distance (4 → 70 m) and supplies the pan angle.
- Our ems note's "ch0/2/3/4/5/6/7/8" are exactly these output ids.

### 6.4 Player components (slot 1, per player instance g)
Common pattern for each SFX object:
- out0 = raw azimuth of B[Player.2] (or B0 for body-centered objects such as Clothing, OffBoard, HandGrabs,
  Takedown, DropIn).
- Volume outputs = base + B dB (via B2/B0/B4) + Player C sums + Global ducks + per-object A/F terms.
- Pitch outputs = B1 Doppler (c 275) + C1 / C3 / C7 / C9… (each clamped ±5000).
- Filter outputs: −1/−2 (LPF open) or −19931 (HPF 77 Hz), some with A/F sweeps.

Per-packet word mapping (which vfunc id lands in which AEMS word) is in PR #4's
`docs/player-audio-retail-drivers.md` §3–§4; for example rolling w1 = vfunc52(0), w2 = vfunc56(8),
w11 = vfunc60(7), and grain gain vfunc60(1)/(2). Not repeated here.

Player B lookups:
- B0: 3DObjPos `0x60010010`, 4–30 m, 1−sin.
- B1: 3DObjPos, 4–60 m, cos, the Doppler source (c 275).
- B2: 3DObjPos#2 `0x60010020`, directional 4–50 / 4–40 / 1–30 / 4–40 m, (1−sin)².
- B4: #2, 4–60 / 2–50 / 1–40 / 2–50 m.
- Others are listed in `catalog.txt`.

### 6.5 Ducking catalog (Global section; all AHR envelopes, gated on a controller input)
- **Master.in1–in4** are category gains:
  - via A4/A0 → Music;
  - A5/A1 → nearly every SFX sum;
  - A3/A2 → Announcer, NIS, CameraMan speech;
  - Master.in4 via A14→A13 → emitters, and via A42→A41 → ambience.
  - Retail keeps all four at 32767 (likely the option sliders; unverified). **A port must write 32767, or
    everything is −100 dB.**
- **Pause.in0:** F11, F13 and others ramp the player, ambience and emitter levels to silence. In retail
  capture: down over ≈175 ms, back over ≈88 ms.
- **Menu.in0–8, NIS.in0–13, HOM.in0–4, Challenge.in1–12, Speech.in0–4, Announcer.in0, CameraMan.in0,
  Reverb.in0–6, Music.in3/5/8/9, VU.in0:** each drives one or more AHR ducks with its own A/H/R times
  (e.g. HOM.in0 drives 15 envelopes). Full list in `catalog.txt` (`trig=`).
- **Music.in3/in6** are the combo-multiplier emphasis. They raise SkateBoard 21/22, Wheels 4, Rail 6 and
  Tricks 6–8 (see PR #4 `MusicEmphasis`).

---------------------------------------------------------------------------------------------------------
## 7. Inputs: who writes what (game side; from PR #4 docs + our notes, not re-verified here)

### 7.1 PlayerPhysics `0x60010000` + g (`sub_824B19C8`, v = ground speed m/s)
| in | value |
|---|---|
| 0 | clamp(v × 23592.24) (5 km/h full) |
| 1 | v × 3932.04 (30 km/h) |
| 7 | v × 2359.22 (50 km/h) |
| 8 | v × 1685.16 (70 km/h) |
| 14 | |COM vel| × 1685.16 |
| 2 | 32767 when no wheel touches |
| 10 | wheels in contact × 8191.75 |
| 4 / 5 / 6 | brake / manual-brake / trick flags (0/32767) |
| 9 | 0 if `[state+16]+72` (the local player) else 32767 |
| 11 | bail-related global flag (writer unknown) |
| 13 | slewed |state+96 − [G+0x2F078]+32| (100 /s, cap 35, /35): `G+0x2F078` is the audio-state record array (player 0 first), so both are the local player's own COM velocity → **0 for the local player** (corrected 2026-10-02; PR #4 read it as a listener distance) |
| 3 | listener-facing factor |
| 12 | soft/hard surface selector |

### 7.2 Per-object inputs
- SkateBoard 0/2/3/4/6; Contacts 1/2/6/7; Rail 0/1; OffBoard 0; HandGrabs 0.
- Collision: input 15 bit 0 is the bus switch (PR #4 defect note).
- See PR #4 `mixmap/inputs.rs` docs and `player-audio-retail-drivers.md` §1–§2.

### 7.3 3DObjPos SFXCTL (`sub_824AEC70` / `sub_824AEB28` / `sub_824AEE60`), the block B reads
- in0 = f32 |followed point (skater) − emitter|;
- in1 = f32 |camera point (pulled back 0.25 m along the view) − emitter|;
- in2 / in3 = azimuth (u16 scale) in the skater frame / camera frame;
- in10 = facing difference; in11 = flag;
- in13 / in14 = f32 signed relative speed vs skater / camera (negative when closing);
- in15: bit 0 active; bits 31/30 = speed sign flipped (resets the Doppler slew).
- Inactive: in0 = in1 = −1.0, in2 = in3 = 0, bit 0 cleared.

### 7.4 Globals
Free-skate values (PR #4 capture statistics):
- Master in1–in4 = 32767; Music in1, in2, in5 = 32767; Reverb in5 = 32767;
- Pause in0 = 32767 while the pause menu is up;
- Music in3/in6 from the combo multiplier; Jitter in0–4 random every frame; VU in0 = output meter level.
- Everything else is 0 in free skate.

### 7.5 Ambience in0 = the bed fade; emitter SFXCTL `0x60060000`: writer not traced (see §10).

---------------------------------------------------------------------------------------------------------
## 8. Port checklist (behavior that is easy to get wrong)

1. 0 mB → 32730, not 32767. Volume outputs never exceed 32730.
2. The kind-0 curve interpolates with a 4-bit fraction of at most ≈0.5 (§5.0).
3. Evaluation order and one-tick lags: input runs by curve kind (§5.1); C and E read earlier records of this
   tick and later ones of the previous tick.
4. First tick: input entries start at lin 32767; nodes start at 0; E starts at −10000. Outputs on tick 0
   differ, e.g. emitter levels read 0 on the first tick.
5. Cross-slot references expand to all instances of the other slot.
6. Pitch with Doppler: below −4800 the result is **0**, not −4800.
7. Doppler slews per evaluation, not per second. The flag bits are cleared inside the input block.
8. A disabled output block writes only its first id, with −10000 (read back as 22768 through & 0x7FFF).
9. Envelope time constants are 60 Hz frames × 16.6667 ms. The "too short" threshold is 16.666 ms
   (`0x822F8ADC`), a different float.
10. f32 rounding matters (B ratios, curve01, cents tables). Use f32 for every float step.
11. Master.in1–in4 must be written as 32767.

---------------------------------------------------------------------------------------------------------
## 9. Validation plan

### 9.1 Golden vectors (done; rerun after any port change)
- Scripts (local): `golden1.txt` and `golden2.txt` (golden2 adds dt changes at evaluations
  300/320/520).
- Format: `dt`, `dtat <eval> <dt>`, `frames`, `watch <key> <id> level|raw|pitch`,
  `set <eval> <key> <id> <int|0xhex|f:float>`.
- PoC side (local only, never committed): `crates/skate-data/examples/mixmap_golden.rs` in the PoC worktree
  - Build: `cargo build --release --locked -p skate-data --example mixmap_golden`
  - Run: `target/release/examples/mixmap_golden.exe <poc>/assets golden1.txt golden1.poc.csv`
- Our reference: `python mxb_tool.py eval <mxb> <tu3 image> golden1.txt golden1.py.csv` (local tool)
- Diff: `mxb_diff.py a.csv b.csv` → today 0 / 36,900 cells for each script.
- The Rust port should emit the same CSV from the same script (an example or test), diffed against
  `golden*.poc.csv`.
- Add scripts for: Collision (10 instances; input 15 bit 0), Pedestrian/Traffic B lookups with Doppler sign
  flips, ADSR records (Global F133/F137/F138), Jitter-driven sums.

### 9.2 Retail capture through recomp hooks (passive; the existing `skate3_audio_trace_hooks.cpp` REX_FUNC-override style, TU 3.0.3.0 addresses, all present in our generated code)
- **MXTICK** at `sub_8294F5E8` entry (r3 = host, f1 = dt):
  - log the eval counter, dt, and host+128 (mode);
  - walk controllers: array `[host+160]`, count `[host+180]`; each controller has +4 key, +8 input block,
    +12 output block;
  - **MXIN**: each controller's 16 input words, change-only per controller.
- **MXOUT** after the original returns: each output block's 16 words, change-only.
- These two lines are a complete golden vector. Convert to a `set`/`dtat`/`watch` script, replay through
  `mxb_tool.py eval` and the port, and diff every output word.
- **MXSET** at `sub_8294BC50` (r3 = controller, r4 = id, r5 = value; key = [r3+4]) plus caller LR,
  change-only per (key, id): who writes which input. Use it for the open writers (emitter `0x6006…`,
  PlayerPhysics 11, Reverb 1/2/6).
- **MXREAD** at `sub_824C2870` / `sub_824C5910` / `sub_824AF240` (r3 = owner, r4 = id, return r3) plus LR,
  logged once per distinct (LR, owner vtable, id): the output → packet-word map for every family.
- Ambience: `sub_824D42C8` (out0..3 applied), `sub_824D0F38` (r4 crossfade group, obj+116 level).
  Emitter: after `sub_824DCF08`, the c_emitter slot words +8..+24.
- Cadence: `sub_82485190` entry (f1 dt, r5 flag) confirms the half alternation.
- Size: about 250 controllers × change-only. Gzip the log. Use short scripted recomp runs, never
  user-performed sessions.

---------------------------------------------------------------------------------------------------------
## 10. Open questions
- Who writes the emitter SFXCTL `0x60060000+g` (distance/azimuth for B0) and when it is active.
  - Candidates: 3DObjPos constructors `sub_824AE098` called from `sub_824AE008`, `sub_824B07A8`,
    `sub_824B0890`, `sub_824B25A0`, `sub_824B2688`, `sub_824B2EE0`, `sub_824B3570`…`sub_824B3A58`,
    `sub_824B6098`…`sub_824B6B40`.
  - Hook MXSET to settle it.
- Meaning of Master.in1–in4 (option volume sliders?) and of Reverb.in1/in2/in6 (indoor/underwater/zone?).
  These are the inputs behind the ambience/emitter low-pass and Reverb.in1 dry mute.
- Instance counts come from the capture, not from code; other modes (career, online) may differ.
- The camera-state mode word (manager+4) is never seen changing. That is harmless for this file (1 variant).
- The 3DObjPos "followed point vs camera" frames and the 0.25 m pull-back are PR #4's reading; they were not
  re-derived here.
- PlayerPhysics in11 (bail global G+16) writer unknown (PR #4).
- Whether retail on real hardware evaluates at ≈68 Hz (recomp cadence) or 30/60 Hz. This matters only for
  the per-evaluation Doppler slew.

---------------------------------------------------------------------------------------------------------
## 11. Sources & credits
| source | author | link | license | what it contributed |
|---|---|---|---|---|
| Upstream PR #4 (`skate-audio-core/src/mixmap/*`, docs `player-audio-retail-drivers.md`, `wheel-audio-handoff-2026-09-17.md`, `engine-defects.md`) | andrewnakas | https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/4 (commit 368a433) | none (read-only reference) | The retail function map, stage order, controller/reader functions, input writers, instance counts, capture match rate. Its port is our golden oracle via the PoC. Described in our words only. |
| Upstream PR #1 (`mx/audio-engine-vehicles`) | andrewnakas | https://github.com/SK8-ENGINE/skate-3-rust-engine/pull/1 | none | Same MixMap core (PR #4 is a superset); first "PathFinder 5.03 MixMap" identification |
| skate3recomp generated C++ (TU 3.0.3.0) | mchughalex (+ rexglue SDK) | https://github.com/mchughalex/skate3recomp | none | Spot checks: the curve function's out-of-range / zero branches (`sub_8294B668`); that every hook address exists. Measurements only. |
| b5-decomp `NFSMix/*.hpp` (NFSMixMap, NFSMixRecords from the ProStreet PDB) | BurnoutDecomp | https://github.com/BurnoutDecomp/b5-decomp | none | EA's record/field names (MixCtl, 3DMixCtl, EvtMixCtl, SubMixCh, MasterMixCh, Preset, camera state, envelope stages). Confirms the same lineage (564/96-byte objects). |
| BP-Decomp_Workflow (DWARF/PDB pointers) | BurnoutDecomp | https://github.com/BurnoutDecomp/BP-Decomp_Workflow | MIT (content EA's) | Background only, via `rwaudio-prior-work.md` |
| dbalatoni13/nfsmw | dbalatoni13 | https://github.com/dbalatoni13/nfsmw | CC0 | Not needed for the MixMap (AEMS only); listed for completeness of the prior-art search |
| Skate 3 retail files (`MixMapSK8.mxb`, TU3 image tables at `0x82FDBFB0`/`0x82FDC7B0`/`0x82FDCFB4`/`0x82FDCFE8`) | EA Black Box | user's own disc / TU | proprietary | Measured data. Never committed; the tables are regenerated by formula (§5.0) or read from the user's image. |
| Our notes `ems-emitters-re.md`, `aems-*.md`, `rwaudio-prior-work.md` | this project | n/a | n/a | Ambience/emitter consumer side, payload words |

## 12. Files
- Spec: this file.
- Tools (local, not published):
  - `mxb_tool.py`: `catalog` / `tables` / `eval`: decoder + reference evaluator, our own code;
  - `mxb_diff.py`.
- Data (local): `catalog.txt/.json`, `outputs.txt`, `tables-check.txt`,
  `golden{1,2}.txt`, `golden{1,2}.{poc,py}.csv`.
- PoC (local, uncommitted): `crates/skate-data/examples/mixmap_golden.rs`.
