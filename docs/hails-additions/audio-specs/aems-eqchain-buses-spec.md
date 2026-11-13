# eEQChain (material) buses, EQ jitter, LFO task and owner send buses: behavioural spec

Written 2026-10-02 from the TU3 (title update 3) recomp's generated code (reference only, read for behaviour), the image dump and the
user's vault. Companion to `aems-voice-graph-spec.md` (§3.3 routing records, §4.10 bus modules, §6.2 buses), which
this supersedes for the material buses. Tags as there: [R] our reading of the lifted code, [IMG] image constant,
[VLT] vault value, [TR] matched against a retail trace, UNCERTAIN = not proven. Throwaway local scripts
(`validate.py`, `walk.py`, `walk_sim.py`, `hashes_before.py`, `callers.sh`, asm dumps).

Bus manager = `[0x830CFDEC]` (`mgr`). Mixer/runtime = `[0x830CFDBC]`; default bus input = `[[0x830CFDBC]+44][0]`.
Init order (`sub_826DD3A8`): `sub_8248EEE8` (env/reverb graphs, mgr+52..+100) → `sub_82490270` (effect returns,
mgr+108..+120) → `sub_824907D8` (LFO task) → `sub_82490CA0` (8 material buses).

## 1. The 8 material ("eEQChain") buses

### 1.1 Graph [R] `sub_82490CA0`
Per bus i = 0..7: graph order **253**, 5 modules, all 6 channels: `Sub0 → DCl0 → PI20 (#1) → PI20 (#2) → Sen0`.
Sen0 is linked with configure id 0x7FF7FFF4 ("send to bus") to the default bus input `[[0x830CFDBC]+44][0]`.
Storage: `mgr+1116+4i` = graph, `mgr+1084+4i` = module array (graph+80; [0]=Sub0, [4]=DCl0, [8]=PI20#1,
[12]=PI20#2, [16]=Sen0), byte `mgr+1148+i` = "created" flag (0 at build). Module class defaults until first post:
PI20 = 96000 Hz / gain 1.0 / Q 3.0 (= bypass, gain exactly 1) [TR], DCl0 bypass (level ≥ 100).
Retail graph addresses [TR]: bus i = `0x40C18C20 + 0x400·i` (proven for 0–2 by value grids, 5–7 by jitter values).

### 1.2 Resolver `sub_82491108(mgr, idx, create)` [R]
- idx 8 → the default bus input (no EQ).
- else, if `create` (u8) ≠ 0 → `sub_824916E8(mgr, idx)` (below), then return `[[mgr + 4·(idx+271)]]` = bus i's Sub0.
- Voice routing records (`sub_824A3140`): value 0–7 → create = 0; 10–17 → idx = value−10, create = 1.
- Player components call it with create = **owner byte `[[component+28]+72]`** (pops `sub_824B9CC8`, touchdowns
  `sub_824B8D48`, manual landing `sub_824BB330`, landing `sub_824BA630` (inlined), `sub_824EC670/958`, …). Meaning of
  owner+72 UNCERTAIN (probably "local/human player"). Wheels `sub_824CE108` pass literal 0; `sub_824F0AB8` literal 1.

### 1.3 Preset data [VLT, IMG]
Vault class `AA801D9FC0ADBBBF`. Collection key per bus from the image table **0x8224DC78** (8 × u64):

| bus | collection | enable `C6DA68C12A3822D2` | PI20#1 freq / gain / Q | PI20#2 freq / gain / Q | DCl0 clip `2086A0CE99C39A86` |
|---|---|---|---|---|---|
| 0 | 42DDFA3F5011BAB4 | 1 | 150..450 / 0.75..1.5 / 0.25..3.0 | 1500..5000 / 0.9..1.25 / 0.25..2.0 | 100 (default) → bypass |
| 1 | DA24A51DDC51EDF3 | 1 | 150..500 / 0.75..1.25 / 0.25..1.5 | 1000..7500 / 0.25..2.5 / 0.25..1.5 | 100 → bypass |
| 2 | 201C88714085E3C3 | 1 | 150..350 / 0.25..1.5 / 0.2..1.5 | 1500..6000 / 0.1..2.0 / 0.2..1.5 | 100 → bypass |
| 3 | 8AF23914D89E4AF0 | 1 | 100..2000 / **1.0..1.0** / 1.0..3.0 | 1500..8000 / **1.0..1.0** / 1.5..5.0 | **0.1** |
| 4 | F7CFA0A8D2F98BDE | 1 | 100..700 / 0.75..1.2 / 2.0..5.0 | 2000..5000 / 0.5..1.2 / 1.0..3.0 | **0.07** |
| 5 | 8AA93567C056736B | 0 (parent 5D76D8135CFA617B) | jitter (§2) | jitter (§2) | 100 (clamped 0..1000) |
| 6 | 87D5F21D9A71736A | 0 | jitter | jitter | 100 |
| 7 | BFA2D9D8C138F925 | 0 | jitter | jitter | 100 |

Field pairs (first-read "a", second-read "b"; value range written b..a above):
PI20#1 freq a `5B48CBEB3EB440D2` b `2B77D4F58AF71CCF`; gain a `7317CC02098BDFF6` b `90A94DDDA5F804CB`;
Q a `F30B7500D67E206B` b `B4190B5C42432FA2`. PI20#2 freq a `3264F689300A522A` b `1DA611DA4DE5A67A`;
gain a `54868EDE952EF581` b `25853859640A3796`; Q a `594D91B30B95483A` b `CDF599CDEC9C7C64`.
Records 1–4 inherit from `55578C11340FFDF1` (enable 1), 5–7 from `5D76D8135CFA617B` (enable 0); `default` record
values fill the gaps (clip 100, freq 10000/5000, …). Bus 3 has gain fixed at 1.0 → its two PI20 stay bypassed; it is
a hard clipper at ±0.1. The per-record RefSpec fields (`23E3…`, `3875…`, `53AC…`, `7570…`, `B713…`, `FBE8…`) are not
referenced by any code we found (the jitter keys are hard-coded, §2); UNCERTAIN whether used at all.

### 1.4 Re-roll on creation: `sub_824916E8(mgr, i)` [R, TR]
Skip if i = 8 or created[i] ≠ 0. Set created[i] = 1. Look up record (class `AA801D9FC0ADBBBF`, key table[i]);
if its enable byte is 0 → done (no posts). Otherwise, under the mixer lock, post in this order:
1. PI20#1 param 0 = pick(freq), param 1 = pick(gain), param 2 = pick(Q);
2. PI20#2 param 0/1/2 = pick(freq/gain/Q);
3. DCl0 param 0 = clip field (no clamp).

`pick(a, b)` = `sub_82491DF0`: if a == b → b (no RNG draw). Else `k = r mod 11` (r = u32 from the title-wide
add-with-carry generator `sub_82A8AF10`, state 0x82FD7D74; same generator as the grain picker), and
**value = b + (k·(a − b))·0.1** in f32, product k·(a−b) rounded, then one fused multiply-add with 0.1f
(0x820641A8 = 0x3DCCCCCD). So every value sits on an 11-point grid between the two fields. Up to 6 draws per
creation, in the order above.

### 1.5 Clearing and the jitter post: `sub_82491180(mgr)` [R]
Called from the audio manager update `sub_82485190` (vtable 0x822FBEA0 +36) on every second game frame (frame
counter at +500 alternates two halves; when frame dt > 0.02 s (double 0x822F8DE8) both halves run every frame).
For each bus i = 0..7:
- if the record's enable byte is **0** (buses 5–7, or a missing record): post jitter values (§2) to both PI20
  with clamps, and DCl0 = clamp(clip field, 0, 1000) (0x82165A10 = 0.0, 0x82256FE8 = 1000.0);
  - freq = clamp(v, 0, 96000) (0x822F8920); gain = clamp(v, 0.1, 20) (0x820641A8, 0x820996EC);
    Q = clamp(v, 0.2, 20) (0x82099280). The clamps are `fsel` max/min (NaN → upper/lower bound behaviour as fsel).
- **always**: created[i] = 0.

Net behaviour: buses 0–4 get a **fresh random preset at the first use after each clear**, i.e. at most once per
2 game frames per bus, and only when a voice/component resolves the bus with create ≠ 0. Buses 5–7 all receive the
**same** jitter values every second frame, whether or not anything is routed there.

## 2. EQ jitter table (class `0AB9F005A2C8FBC7`, "Sk8::Audio::eJitterParams")

### 2.1 Table [R] (`sub_824EEFB0` ctor, vtable 0x822FC248; load `sub_824EF0B8` = slot +28; update `sub_824EF378` = slot +36)
The loader stores itself at `[0x830CFDC4]+724`, loads every record of class `0AB9F005A2C8FBC7` (max 30, count at
+1232). Entry E = this + 32 + 40·n: +0 u64 key, +8 value, +12 velocity (0), +16 a2, +20 a3, +24 a0, +28 a1,
+32 u8 post flag (`8F956FBAD301AE26`), +36 i32 post id (`E7D491E2EB228F54`), from params `B66AAD957873A8B3` =
{a0, a1, a2, a3}. Initial value = a0. Lookup `sub_824EF458(table, key)` returns the value (0.0 if absent).

### 2.2 Step `sub_824EF4C8` [R]
```
n     = (r mod 2001) − 1000          # r from sub_82A8AF10 (title generator), u32
s     = (a2 − a3) · (n · 0.001)       # 0.001 = 0x82063A48
acc   = s ≥ 0 ? a3 + s : s − a3       # |acc| ∈ [a3, a2], sign of n (n = 0 → +a3)
v     = clamp(v + acc, −a2, a2)
x     = value + v ; hi = a0 + a1 ; lo = a0 − a1
if x > hi: v = v · −1 (0x8216DEE0 = −1.0); x = hi − (x − hi)
elif x < lo: v = −v; x = lo + (lo − x)
value = clamp(x, lo, hi)
```
A bounded random walk with random acceleration, reflecting at the range ends. The update (slot +36) steps every
entry, then for entries with post flag set posts trunc(clamp(value, 0, 32767)) with the entry's id to the object at
table+12 (vfunc 8): the six flag-1 records (`FB7567C0…`, `5FBCB3B8…`, `02D9546B…`, `C20C6630…`, `6AEEEEB9…`,
`B5E641CC…`, ids 0–5) feed some other consumer, UNCERTAIN which. Caller/cadence of slot +36 not found (virtual):
UNCERTAIN; the retail trace fits "one step per game frame" (§4.3).

### 2.3 Keys used by the material buses (hard-coded in `sub_82491180`) [R, VLT]

| target | key | a0, a1, a2, a3 | range lo..hi | max step |
|---|---|---|---|---|
| PI20#1 freq | E17029CE4388E1E8 | 400, 300, 299, 199 | 100..700 Hz | 299 |
| PI20#1 gain | C27373E7FD2DCE47 | 1.25, 0.75, 0.75, 0.25 | 0.5..2.0 | 0.75 |
| PI20#1 Q | EA2ED27D25247927 | 3.0, 2.5, 2.4, 1.0 | 0.5..5.5 | 2.4 |
| PI20#2 freq | 816A58ECCCD4112D | 2000, 1500, 500, 175 | 500..3500 Hz | 500 |
| PI20#2 gain | 8802BC5475904597 | 1.75, 1.25, 1.0, 0.25 | 0.5..3.0 | 1.0 |
| PI20#2 Q | D4ED2F0BA77ACAB5 | 3.0, 2.75, 2.5, 0.5 | 0.25..5.75 | 2.5 |

## 3. eEQChain enum → bus, and who uses which [VLT, R]
Enum value = bus index 0–7; 8 = default bus (no EQ). Routing records add 10 to request creation (10–17).
Holder class `42AFE160E647167C` (default record), with the component that reads the field where confirmed:

| bus | fields (value) | confirmed users |
|---|---|---|
| 0 | E34B48082B5BF185 (pops, `sub_824B9CC8`), 9326AEAC6E053997 (touchdowns `sub_824B8D48`, manual landing `sub_824BB330`, via `sub_824B7AF0`), 4B6E2D79A8452D9B (cloth), 22D0D4A5A14FFF7D, 985A1A2890A77F85 (`sub_824EFA60`), AEA5BA7E64515945 (`sub_824DBBB8` ×2), B2C4F86A53963BA2, D1A87641CCB98787 (`sub_824C3FC8`, `sub_824C4138`) | retail bus 0 re-rolls 228× in all_163809 |
| 1 | 8B0E030799CBDD00 (landing `sub_824BA630`), 0D73DB849EC8E2B2 (shoe scuffs `sub_824B97A8`, `sub_824C0DA8`), F61F2797B24868D9 (foot taps `sub_824B9268`, `sub_824C07D8`), 2F880F059AB9B151 (`sub_824F0718`), 428FDFC2E99FFA65 (`sub_824F05D0`), 55DA01533B378EFA (`sub_824C01E8`), 660885AD6F8F9AF4 (`sub_824EF8C8`), 748CBC9727A5347F (`sub_824BB8D8`, `sub_824BBA00`) | 207 re-rolls |
| 2 | C014A21D0FF6EDBA (footsteps, routing word +10 → created), ED52262DABB5DE4C (`sub_824B8310`, `sub_824B8448`), A9023782094771B5, CCDFABBAEEA33EAE | 2013 re-rolls |
| 3 | none (no field has 3) | never posted in either session |
| 4 | none | never posted |
| 5 | D489344CEDEE5036 (grind), F52450E504250254 (skid), E633C8F009CAEFFC | jitter |
| 6 | 55E6488906A2E330 (wheels; `sub_824CE108` create = 0), D9BE1F2F1A72FEE8 (flips) | jitter |
| 7 | 4D30A6CA8C9E1ABC (rattle), C1DC8556BA66CDCD (wind), 2DBD9ED0AD824844, 4A022FEF9905D8F4, C58CE169C13320CA, F2B44F93BD91662E | jitter |
| 8 | 46321AD5724BCC48, 5A837C613E3F41DC, A5D3ADA63608617F (board grain chain `sub_824C8878`), C04832978CDED925 | default bus |

Other `sub_82491108` callers take the enum from elsewhere: `sub_824D25E0`, `sub_824EC670`, `sub_824EC958` from the
material-pair lookup `sub_82497BF8` (collision voices; UNCERTAIN mapping); `sub_824EC488`/`sub_824EC670`/`sub_824EC958`
via `sub_824B7A98`; `sub_824F0AB8` via `sub_824B7B48` (fields 552899F3BF9927CC, A3E8A9381F4222E1,
C0FFD1E535F218E2 of another holder; UNCERTAIN class).

## 4. Validation against retail [TR] (`validate.py`, `walk.py`, `walk_sim.py`)
### 4.1 Random buses 0–2 (every non-default PEAK line on the 11-point grid of its fields)
| session | bus 0 P1 / P2 | bus 1 P1 / P2 | bus 2 P1 / P2 |
|---|---|---|---|
| dsp_20261002_185826 | 21/21, 21/21 | 31/31, 31/31 | 15/15, 15/15 |
| all_20261002_163809 | 228/228, 228/228 | 207/207, 207/207 | 2013/2013, 2016/2016 |

e.g. bus 0 PI20#1 freq ∈ {150, 180, …, 450} (step 30), gain ∈ {0.75, 0.825, …, 1.5}, Q ∈ {0.25, 0.525, …, 3.0};
PI20#2 freq ∈ {1500, 1850, …, 5000}. All 11 grid points occur in all_163809. Both PI20 change on the same line
timestamp (one creation = one re-roll of all six).
### 4.2 Jitter buses 5–7
- Inside the §2.3 bounds: dsp 904/904, 553/553, 980/980 lines; all 4050/4050, 8637/8637, 12666/12666.
  Observed extremes reach the bounds (e.g. 100.0009..699.9996 Hz, 0.5..2.0, Q 0.25..5.75).
- Buses 6 and 7 carry **identical** values at the same timestamps (checked 20.5 s window): one shared source.
- Before the first jitter step bus 6 showed exactly the centres 400 / 1.25 / 3.0 and 2000 / 1.75 / 3.0 (6159 ms, dsp).
### 4.3 Update cadence
- Bus 5–7 changes: median 9.5 ms apart in the recomp (its game frame rate, not 60 Hz; UNCERTAIN host pacing).
- |Δvalue|/a2 per logged change, all_163809 bus 7 PI20#1 freq: quantiles (10/25/50/75/90 %) 0.107 / 0.292 / 0.670 /
  1.011 / 1.330. Simulated walk: 1 step per post 0.06 / 0.17 / 0.48 / 0.92 / 1.00; 2 steps 0.15 / 0.37 / 0.72 / 1.11 /
  1.39; mixture 20–30 % one-step ≈ 0.12 / 0.31 / 0.68 / 1.02 / 1.30: matches. Consistent with "jitter steps every
  frame, bus post every second frame, both every frame on long (> 20 ms) frames". UNCERTAIN until the slot +36
  caller is found.

## 5. Owner send bus `sub_82488DD0(graph_out, modules_out, target, f1 level)` [R]
Graph order **4**, 3 modules, all 6 ch: `Sub0 → Sen0 (env) → Sen0 (eEQChain bus)`.
- Sen0 #1: linked (0x7FF7FFF4) to `[[mgr+52]]` (env/reverb bus input); param 0 = `level` posted once at build.
- Sen0 #2: linked to `target` = `sub_82491108(mgr, eEQChain, owner+72)`; its level is never posted (Sen0 default,
  UNCERTAIN = 1.0, constructor current gain 1.0).
- The voices of that owner send into this Sub0 instead of a material bus directly; the owner bus is a mixing point
  that taps the sum to reverb and forwards it to the EQ bus.

Builders (level = `vfunc60(id) × 1/32767`, 0x822F8898):

| caller | owner | level id | eEQChain field | storage |
|---|---|---|---|---|
| `sub_824B9CC8` | pop (enter air with a trick) | Contacts 14 | E34B48082B5BF185 (0) | +72 / +76 |
| `sub_824B8D48` | touchdown voice slots (4 × 24 bytes at +140) | Contacts 15 | 9326AEAC6E053997 (0) | slot +156 / +160 |
| `sub_824B8D48` | extra (second-variant) touchdown voice | Contacts 15 | 9326AEAC6E053997 (0) | +252 / +256 |
| `sub_824BB330` | manual landing (kind 4) | Contacts 15 | 9326AEAC6E053997 (0) | +44 / +48 |

No other caller of `sub_82488DD0` exists. Whether the level is refreshed after build: UNCERTAIN (not seen here).

## 6. 0x40C18420 / 0x40C18820 and the LFO task
### 6.1 The two graphs = env pre-stages mgr+56 / mgr+60 [R, TR]
`sub_8248EEE8` builds: mgr+52 env bus input (order 199, `Sub0 → Sen0 → Sen0`, mono), then **two** order-200 mono
graphs `Sub0 → Gai0 → PI20 → Sen0 → Sen0 → Sen0` at mgr+56 / mgr+60, then ReverbModel1 (`RM10`) graphs mgr+64/+68
(order 201), four order-202 graphs mgr+72..+84 (HI20/LI20/Pn21 present), four order-201 7-module graphs mgr+88..+100.
The trace's 18420/18820 have exactly GAIN, PEAK, 3 SENDs in that module order → mgr+56 = 0x40C18420,
mgr+60 = 0x40C18820 (the LFO centres below confirm). The effect returns (mgr+108/+112, `Sub0 → Del0 → PI20 → Sen0 →
Gai0 → Pn21 → Sen0`) are the trace's 0x40C40770 / 0x40C40020 (PEAK, 1-ch SEND, GAIN, 6-ch SEND).

### 6.2 LFO task: `sub_824907D8` builds, `sub_82490B60` runs [R, IMG]
14 entries of 32 bytes at mgr+396+32n: +0 period (s), +4 phase, +8 sine scale, +12 offset, +16 depth, +20 output,
+24 module, +28 param index. Registered with `sub_82481BE0` as task "LFO Task" (string 0x8224DC68), callback
`sub_82490B60`, context mgr+388. Each run (dt in f1): phase = (phase + dt) mod period;
**out = offset + depth · scale · sin(2π · phase / period)** (2π = 0x820B411C, sine `sub_82F4DED0`), posted to the
module's param.
Template: period 1.0 (0x8231A844), phase 0, scale 0.5 (0x8209975C), param 0.

| n | module | offset = depth | output range |
|---|---|---|---|
| 1 | [[mgr+56]+8] PI20 freq (0x40C18420) | 1.0 | 0.5..1.5 |
| 2–5 | [[mgr+88]+4], [[mgr+92]+4], [[mgr+88]+20], [[mgr+92]+20] | 0.3 (0x820D06C0) | 0.15..0.45 |
| 6 | [[mgr+60]+8] PI20 freq (0x40C18820) | 0.3 | 0.15..0.45 |
| 7–10 | [[mgr+96]+4], [[mgr+100]+4], [[mgr+96]+20], [[mgr+100]+20] | 0.3 | 0.15..0.45 |
| 11 | [[mgr+116]+4] effect return A Del0 delay | 0.006 (0x82116288) | 3..9 ms |
| 12 | [[mgr+116]+8] return A PI20 freq | 1.0 | 0.5..1.5 |
| 13 | [[mgr+120]+4] return B Del0 delay | 0.006 | 3..9 ms |
| 14 | [[mgr+120]+8] return B PI20 freq | 1.0 | 0.5..1.5 |

[TR]: at boot 18420 sweeps 0.954..1.241 starting at 1.03, 18820 0.286..0.372 starting at 0.31, both with gain 1 /
Q 3 (bypassed); slope ≈ 1 s period at ~5.3 ms per step. Later an environment preset rewrites them (e.g. 6128 ms both
→ 1936.30 Hz / gain 0.1 / Q 10, then a shared ramp of +1.297 per step; 18820 → 1162 Hz gain 1 at 6168 ms). That
writer (probably the reverb-zone settings reprogramming the LFO entries and the PI20 gain/Q) is **not identified**.
Modules of the mgr+88..+100 graphs at module index 1 and 5 are not resolved (UNCERTAIN; probably modulated delays
of the reverb network).

## 7. Port notes
- Material buses: implement §1.4/§1.5 exactly: per-bus created flag, cleared every second frame; re-roll on first
  create-flagged resolve; draws from the title generator in the fixed order; skip the draw when a == b.
- Jitter: one shared 30-entry table; buses 5–7 share it. Retail is unreproducible sample-for-sample (shared RNG), so
  tests should check grids/bounds/step statistics as in §4, not sequences.
- Do not apply the material-bus EQ to bus 8 (default).

## 8. UNCERTAIN
1. Meaning of owner byte +72 (create flag for player components).
2. Caller and cadence of the jitter update (slot +36 of 0x822FC248); the trace fits per-frame stepping.
3. Real 60 Hz cadence: the recomp's game frame period (~5 ms?) differs; the "every 2nd frame" rule is from code.
4. Consumer of the six post-flagged jitter entries (ids 0–5).
5. Owner send Sen0 #2 level (never posted; assumed class default) and whether the env level is refreshed.
6. The environment-preset writer that changes the env pre-stage / return PI20 gain/Q and LFO entries.
7. Module types at index 1/5 of mgr+88..+100 (LFO entries 2–5, 7–10).
8. Exact rounding of `b + (k·(a−b))·0.1` (fmadds single rounding); all trace values match to 4 decimals.
9. Whether the per-record RefSpec fields of `AA801D9FC0ADBBBF` are read anywhere.
10. Enum sources for `sub_82497BF8` (material pair) and the `sub_824B7B48` holder.
