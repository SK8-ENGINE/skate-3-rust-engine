# Retail world emitters (.ems): how TU3 evaluates them

> **2026-10-02 19:00: the recomp sessions behind the measured sections below were deleted (pre-fix trace writer, a few malformed lines). Treat those findings as "re-verify on clean data" (only all_20261002_163809 / _164620 / _180430 and world_hooks are clean).**


From reading the recompiled TU3 (title update 3) code (reference only; 2026-10-02). Addresses are evidence; never copy
code. Record offsets are decimal.

## File and records
- Layout: BE u32 count, then 72-byte records: index, flags, pos[3], extent[3], scalars[4], u64 sound id,
  gains[4]. Our reader: `tools/asset_pipeline/audio_formats.py` `ems_emitters`.
- The sound id is lookup8 of a name, written as named or lower-cased (254 vs 144 on the disc). Our
  `name_id`. 28 ids name banks that are not on the disc (e.g. restaurant_amb_2, metal_groans_a).
- Loader `sub_824A24F8`, run from the audio tick `sub_82485190`, owns the EmitterSystem at audio+488. It
  takes up to 5 file names from the map database entry and loads `data\audio\<name>` asynchronously.
  Records are registered (`sub_828EA7F0`) in a spatial tree as cubes `pos ± max(extent)`. Unload:
  `sub_824A2808`.

## Routing is per record
The sound id keys the **attribute database** (`sub_8248CDC8`). Its type field (hash 0x6D18B8674D7E5337,
read by `sub_824A1608`) decides:
- 1 = looping emitter, with its bank loaded on demand;
- 5 = reverb zone (`SFXObj_Reverb` `sub_824DE548` via `sub_82488278`);
- 4 = single-winner **music zone** (`sub_828EB410`, `sub_824A2198`, `sub_824A2330`): attribute RefSpec
  `442A158E27F2B5AD` → class `E93ABF17955A5264` playlists (ePlayListMode, eChannelMapPreset, IPodSongInfo /
  DJSongInfo); gate = playlist `+4` == audio+1608; the winner (smallest d) is stored at emitter system +208..+256.
  Licensed-music player: not ported (2026-10-02).
- 6 / 7 (speakers_ / crowds_ files, Maloof / MegaPark sfx): no bank, always flagged; no caller of the type getter
  checks 6 or 7 (only 1, 4, 5). Event PA / crowds: not ported.

Other attribute fields: volume (+4), patch index (+12), variant list 0xC48AB73A227B3B00 (`sub_824A1D38`
picks one at random), curve type 0xF209C093F40A4CCC.

## Per frame (`sub_828EA120` → query `sub_828EA918`, gate `sub_828EAA30`)
- **Query:** the tree is searched with a box of listener ±0.68 m (0x82099764). The listener comes through
  pointer 0x820CFDD4.
- **Flags:** `flags != 0 && (flags & system+24) == 0` skips the record. The mask = audio+1032, or 4 when that is 0 or
  0x80000 (`sub_8248B258`, message 0). The free-skate value of audio+1032 is not measured.
- **Sphere** when ex == ey == ez: `d = |L−P| / ex`.
- **Ellipsoid** otherwise:
  - forward `F = (s1, s2, s3)`; `side = norm((0,1,0) × F)`; `up = norm(F × side)`;
  - `d = sqrt((Δ·F/ex)² + (Δ·up/ey)² + (Δ·side/ez)²)`.
  - Active while d < 1.
- **Inner core s0:** d < s0 gives 0 (full level); otherwise `d = (d−s0)/(1−s0)`, or 0 when s0 ≥ 1.
- **Level** = attribute volume × curve(d): type 0 → (1−d)², 1 → 1−d, otherwise 1. Computed at start
  (`sub_824A1E48`) and each frame (`sub_824A2030`).
- **gains[4] (+56..68) are never read**, as far as traced.

## Posting
- **Start:** a 48-byte tEmitterInfo (patch index, level, position) goes to `CSTATEMGR_Emitter`. Its pool
  has **5 states** (`sub_824F6FE0`). When the pool is full, start retries every frame.
  - No priority: emitters take states in discovery order.
- **`SFXObj_Emitter` activation (`sub_824DCE60`):** creates an AEMS `c_emitter` object (`sub_824D0E48`) with
  the patch index clamped to 0..500. The position is set once (emitters are static).
- **Every frame (`sub_824DCF08`, `sub_828E2D18`)**, parameters in `c_emitter` object slots:
  - +8 = MixMap ch4 × level, +12 = MixMap ch8 × level (0..32767)
  - +16 = MixMap ch0 (0..65535)
  - +20 = MixMap ch5 via `sub_824C5910` (0..8192, 4096 default: pitch?)
  - +24 = MixMap ch6 (0..25000: low-pass Hz?)
  - How these channels depend on position is not verified.
- **Stop (`sub_828EB0F0`):** a node that wasn't hit this frame is released at once, with no fade here. Its
  bank unloads when no node uses it. Timed one-shots (`sub_824A1A20`) expire after 10 s by default.

## The attribute database = our skater collections (verified 2026-10-02)
`assets/private/stock/skater-collections.json` (`skatercollections.vlt`), class **Hash_F0CEF367088EFFF8**,
360 records. Key = `Hash_<sound id>` (the `.ems` id exactly; no name needed). Inheritance runs through
`parent`: e.g. water_fountain → E11BD62E74030160 → 07AF07ECC07F3828 → default. Fields:

| Field | Meaning | Example (water_fountain) |
|---|---|---|
| `volume` | attribute volume (level = volume × curve(d)) | 0.5 |
| Hash_9908F2D75D7381BD | float, default 10.0; probably the timed one-shot duration (`sub_824A1A20`, 10 s default; unverified) | 15.0 |
| Hash_BE88128A30BE926E | bank file (text) | `water_fountain.abk` |
| Hash_C493ED34D1D32521 | patch index (clamped 0..500 for `c_emitter`) | 81 |
| Hash_6D18B8674D7E5337 | `Sk8::Audio::eVolumeType`: 1 = looping emitter, 5 = reverb zone, 4 = single winner; 0/2/3/7 also exist | 1 (inherited) |
| Hash_F209C093F40A4CCC | `eVolumeFalloffType`: 0 = (1−d)², 1 = 1−d, other = flat | 0 |
| Hash_C48AB73A227B3B00 | variant list (RefSpec array) | n/a |

Examples (volume / f9908 / patch):
- water_lapping 1.0 / 15 / 82; water_lapping_pond 0.8 / 15 / 83; trees_rustle 0.25 / 38 / 76
- pub_amb 1.0 / 36 / 33; clock_bell 0.35 / 40 / 38
- unresolved-by-name `086CD0AEE99DB825` = `fountains_waterlaps_left.abk`, patch 347, vol 0.5 (the University
  channel, where the user heard bad water)
- `1F94F2F815C00368` (37× in the sfx files) = type 5 reverb zone (RefSpec 99FD793BC30CF0FA → a reverb preset)

## What the banks' programs do (PoC evaluator run, 2026-10-02; probe `emitter_probe.rs` in the PoC)
- **Evaluator gotcha:** the tick delta must be rounded to f32 (`(256f32/48000f32) as f64`). With an f64
  delta the period is recounted every block and nothing runs.
- **`c_emitter`:** 291 banks bind to it. The payload is 9 words:
  - w0 = level (0..32767), w1 = dry (prop 8), w2 = send (prop 5)
  - w3 = pan (prop 3, 65536 = 360°), w4 = pitch (4096 = 1.0)
  - w5 = low-pass (prop 6), w6 = high-pass (prop 7), w7 = unused
  - w8 = selector: a bank plays only when w8 equals its own base. Bases: water_fountain 81,
    water_lapping 82, pond 83, trees 76, pub_amb 33, Siren_city_1 225, DogBarks_medium 186.
  - So the attribute **patch index (Hash_C493ED34D1D32521) is the selector** (water_fountain patch 81 =
    base 81; the first reading's "+1" is the 1-based form).
  - The programs do no distance or position work: the game supplies level, pan and filters.
  - Release (op 4) ends the instance at once. Capacity is 10 instances per bank.
- **water_fountain:** a two-voice relay of one-shots. Each voice draws from its own 10-sample shuffle bag
  (no repeat until the bag empties; samples 1.6–2.5 s). When the playing voice has about 4–206 ms left
  (curve over remaining ms × 0.126), the other voice starts. That gives about 24 starts in 30 s, first
  after 60–90 ms. (Re-run 2026-10-02 with selector 81: **17** starts in 30 s, first at 59 ms;
  see aems-evaluator-spec.md §6.)
  - Level = w0 × (26000 + a 4 s triangle LFO with depth from a 3.5 s sine of amplitude 5000) / 32767,
    which is 0.64–0.95.
  - Pitch = (3600 ± 250, 4 s sine)/4096 = 0.818–0.940.
  - Caveat: the mock reports remaining time at pitch 1.0.
- **water_lapping:** the same relay. Samples 0.96–2.45 s; level 25000 ± ≤10000 (0.47–1.0); pitch
  (4100 ± 250)/4096 (0.94–1.06).
- **trees_rustle:** one voice looping a 38.5 s sample. Level 20000 ± ≤20000 (0.03–1.0, gusts); pitch
  (3900 ± 250)/4096.
- **pub_amb:** a single 36 s sample, started once.
- **`c_emitter_utility`** (posted once at boot) sets `random_pitch_gbl` / `random_vol_gbl`:
  - Every 2.5 s it draws an index 0..30 into curves: level 16384→32700→16384, pitch 3700→4400→3700.
  - Ramps take 1.0 s (level) and 1.5 s (pitch).
  - 43 banks use the level global (AC units, vents); 12 use pitch (ships, propane, liquid flow).
  - It also answers the `*_msg` broadcasts by drawing the next shuffle value into the `*_snd` globals.
- **Sirens, dog barks, distant bangs:** ordinary `c_emitter` banks. One post plays once (siren: a 19.3 s
  sample plus a layer 0.19 s later at 16000/32767, pan sweeping about −15°/s). There are **no timers** in
  any program, so the game schedules them; who posts them, and how often, is open. No `.ems` record names
  them.
- **`c_main_ambience_crossfade`** (DT/Ind/Uni):
  - 28 voices in 7 groups of 4, at quad pans 45°/225°/135°/315°.
  - w9 (1..7) selects the group, by slot switch: instant, not a timed crossfade.
  - Front voices at w0, rear at w8 × 23000/32767; w1 dry, w2 send, w4 pitch, w5 LP, w6 HP.

## Distant random one-shots: sirens, dogs, bangs, alarms (TU3, 2026-10-02)
A location-based random one-shot scheduler inside the same EmitterSystem (audio+488). Not the ambience
manager, and never in the `.ems` files.

**Data: attribute class `aud_wp_emitters`** (Hash_39A2DE0232912CE5, 59 sets). Names come from db.big
`skatercollections_summaryreport.txt`; key = name_id(name). Fields:
- +0 29C68772… = min level (usually 0.2); +8 B6AD5C13… = max level (0.2..1.0)
- +4 B1E6821F… = min interval (2..20 s); +12 A306BD1F… = max interval (5..40 s)
- 5CA06CA0… = RefSpec list of `aud_emitter` (F0CEF367…) sounds; 07DDABF9… = Int32 weights (default 10)
- 79460EEF… = level steps (default 10)

Example sets (interval / level):
- e_dwtn_office_buildings 2–5 s / 0.2–1.0; e_dwtn_lot_5 3–8 s
- e_dwtn_mall 5–15 s / 0.4–0.65; e_dt_main 2–5 s / 0.5–1.0
- e_old_town 2–7 s / 0.5–1.0, with Siren_euro_1/2/4

Where sirens appear:
- City sirens only in the downtown "dwtn" sets (lot_5, city_center, mall, office_buildings, canal_plaza,
  rez_plaza, spillway_core_plaza, spillway_brewery, skatepark).
- Distant sirens: dwtn core_plaza, parkade, mem_plaza, wall, brewery, skatepark, and e_univ_low_plaza.
- Suburb sirens: e_dt_main, e_dt_matrix, e_soho, e_urban_res, e_projects_below, e_univ_suburbs.

Per sound (`aud_emitter` record):
- patch selector (Siren_city_2 = 226); volume (sirens 1.0, suburb 0.75, dogs and bangs 0.8);
- the +8 float (the 10.0-default field) = **maximum play time / timeout**: sirens 19.5 s, dogs 5.0,
  alarms 15.5, mcycle 9.4.
- Dist_Dog_Bark_* has no aud_emitter record.

**Code**
- **Set selection:** getter vtable+0x60 `sub_824A1560` returns the u64 at `*(0x82083C38)+0x2F0B0`, the
  current location's audio record. It returns 0 when system+28 == 0, `*(0x820CFDE4)+1096` == 5, or the
  audio mode `*(0x820CFDC4)+892` is 18/19/20.
- **Set change:** the per-frame update `sub_828EA120` (enabled at +76, +28 == 3) calls `sub_828EA4E0`
  SetRandomSet when the key changes. That rebuilds 32-byte entries (key, weight, state, age, bank slot,
  handle) and draws the first interval.
- **Bank loading:** `sub_828EA290` keeps at most **2** banks loaded, each a weighted pick among unloaded
  entries (`rand() % Σweights`).
- **Timer:** `sub_828EA3D0` adds dt; past the interval it plays the loaded idle entry with the largest age
  (the oldest loaded), then resets and redraws even if nothing played.
- **Interval** (`sub_824A1980`) = min + (max−min) × (rand()%1000) × 0.001, timed trigger to trigger.
- **Post** (`sub_824A1A20`), through the same CSTATEMGR_Emitter pool:
  - patch; level = volume × L, with L = (min×10 + rand() % ((max−min)×10)) / 10;
  - one-shot; pan = rand() & 0xFFFF, a random fixed 360° pan;
  - position 0 (not positional); timeout = the play-time float.
- **Per frame** (`sub_824DCF08`, non-positional branch):
  - +8 = MixMap ch2 × level, forced to 0 when `*(0x820CFDBC)+104` != 0 (a mute?);
  - +12 = ch7 × level; +20 = ch1 (pitch 0..8192); +24 = ch3 (LP 0..25000); +16 = the fixed pan.
- **Stop:** when the program ends or the timeout passes. The bank unloads and a fresh weighted pick loads
  another; nothing is excluded after playing.

**Open**
- Who writes the location record (`*(0x82083C38)+0x2F0B0`): no e_* names or hashes appear in the world,
  missions or livingworld data. Lead: the surfaceless zone boxes (doc 05: they are retail trigger volumes).
- The MixMap ch1/2/3/7 values, the mode codes, and the units of dt.

**Passive recomp trace plan** (no repeat sessions):
- `sub_828EA4E0` entry r4 (set key); `sub_828EA290` loaded entry key;
- `sub_824A1980` return f1; `sub_824A1A20` sound key and info (+0 patch, +8 level, +16 pan, +24 timeout);
- `sub_824DCF08` objects with info+4 == 0: c_emitter +8/+12/+16/+20/+24;
- per frame: the location u64, the mode at `*(0x820CFDC4)+892`, the mute byte.
- A short scripted teleport through downtown plazas would show the location → set mapping.

## Location → set: world-painter region layers (SOLVED 2026-10-02, verified against the recomp)
- **Base address:** `*(0x83083C38)+0x2F0B0` (lis 0x8308; the "0x82083C38" written above is a typo, as was
  0x820CFDxx → 0x830CFDxx).
- **The 152-byte record:** +0 emitter-set key, +8 ambience key, +16 reverb key, +24 another key, +32 flags.
- **Writer:** module tick `sub_827A78A8` → `sub_827A11B0` → `sub_827A2E88` with the focused skater's x, z.
  - If byte 323 at 0x830B7AE8 is 0 and `*(*(0x830CFD94)+292)+6013` is set, it uses the vector at
    that object +5808 instead (camera?).
  - Then `rec+0/+8/+16 = sub_82C0EAC0(R, layer 8/7/10, pos)`, where `R = *(*(0x830CFD94)+232)`.
- **Lookup:**
  - 128 m tiles (`sub_82C0EFA0`), each with 19 layer slots matched by `name_id(layer name)`:
    7 audio_ambience, 8 audio_emitters, 10 audio_reverb, plus districts, fog, sky, bloom, etc.
  - Each slot is a quadtree (`sub_82C0F0B0`): header centre (x, z) +0, half-size +16, node count +32,
    node offset +36.
  - Nodes are 10 bytes: four u16 children and a u16 value. child[0] 0xFFFF = leaf; value 0xFFFF = no key.
  - Children are tested in order −x−z, −x+z, +x−z, +x+z, with inclusive edges.
- **Data:** district `worldDIST_<D>.big` → `cSim_*.xsf` (SFIL, index `*_Sim.xst`), RW4 arenas with ATOC
  processor 0xAB329A6A.
  - Dictionary type 0x00EB000F = layer record {tree object index, key-table object index, layer hash};
    0x00EB0010 = quadtree; 0x00EB0011 = key table.
  - The key table holds {offset, type 2} entries; the u64 keys are stored **low word first**.
- **Our code:**
  - `audio_formats.region_layers` / `region_key`; `audio_export.regions` → manifest `regions`
    {district: {layer: tiles}};
  - engine `library::RegionTile::key`; `random_sets.rs` queries at the board position.
- **Verified against the recomp:**
  - Aletown spawn (−182.9, 430.5) → e_dwtn_spillway_brewery;
  - University start (348.8, −725.9) → e_univ_megapark. Axes: x, z as-is.
- **Coverage:** DownTown 128, University 122, Industrial 100 and SkateSchool 62 emitter tiles. The small
  parks have only ambience and reverb layers, so no random one-shots there, as in retail.
- **Next use:** the audio_ambience layer gives the zone bed key (`aud_wp_ambiences`, e.g. dt_open at
  Aletown), replacing our per-map bed pick; audio_reverb gives `aud_reverb` presets.

## Recomp trace confirmation (session sirens1, Aletown, 2026-10-02)
- Hooks WPPOS / WPSET / WPINT / WPFIRE (skate3recomp `src/skate3_audio_trace_hooks.cpp`).
- e_dwtn_spillway_brewery (4–8 s, level 0.2–1.0) fired 9 sounds in about 75 s: Metal_Bang_1,
  car_horn_city_1, **Siren_distant_2**, Jet_Passby_1, truck_horn_city_2, Delivery_truck_leave_3,
  **Siren_city_8**, bus_by_3, Jet_Passby_1.
- Our engine at the same spot selects the same set and fires the same kinds of sounds.
- Bank programs measured for all 113 set banks (PoC probe) → `game_audio/random_programs.rs`:
  - 56 banks play one layer per post, 53 two, 3 loop;
  - 4 never answer their attribute selector (Siren_city_5/6, car_horn_city_2/3), so retail is silent
    for those too.

## 15-location validation (recomp session locations_listen, 2026-10-02)
- Teleports via Challenge Map > Locations: 5 each in Downtown, Industrial and University, 45 s listening
  at each. Script: `locations_listen.txt`.
- Checked with the local tool `check_sets_vs_trace.py <trace>` (each stop against its
  own district; stale post-teleport samples skipped).
- **Our region lookup = retail's location set on 1859/1859 samples**, 15 sets: dwtn spillway_brewery,
  mem_plaza, office_buildings, spillway_start, skatepark; ind reclaimed, quarry, drydocks, spillway_top,
  shipyard; univ new_campus, clocktower_sq, training_facility, megapark, observatory.
- **Silent banks confirmed:** retail fired Siren_city_5 (Hotel District), but no Siren_city_5 sample
  played; the next new samples were delivery_truck_in_1, the next fire.
- **Real sirens start their own samples:** Siren_city_4 layers at +16 ms and +216 ms (two layers ≈0.2 s
  apart, as probed); Siren_city_10 at +208 ms. Matched by the PLAY lines' 48 sample bytes against the bank
  files.
- Location lists (screenshots): Downtown 14, Industrial 12, University 10, then skate.School, skate.Park (8),
  Maloof Money Cup (2), Black Box Park, Maloof 2010 NYC, Danny Way's Hawaiian Dream, The Sanatorium and
  Art Gallery (DLC todo).
  - Menu path: start, a, rt, rt, down×district, a, down×location, a, a, a.

## Zone ambience beds (TU3, 2026-10-02; local asm dumps)
- **Getter `sub_824D3E40`:** ambience key = `*(0x83083C38)+0x2F0B0+8` (region layer audio_ambience).
  - Overrides: front end (`*(0x830CFDC4)+1204` set and +1196 == 10) uses press_start_screen
    (0x5EADF1818A721AF9); a game-state record of class C1831BDB6CB1B1EA (from `*(0x830CFDA4)+76`, RefSpec
    51BAD6E2DB03B4AE) can replace the key.
  - Returns 0 until the bed's stream entry is ready.
- **Beds:** `sub_824D4630` reads `ambienceresident.big`; index = atoi of the first 2 chars of each `.snr`;
  stream path = `data\audio/ambience.big|NN_name.sns` (`sub_824D3590`). There are 16 more DLC slots
  (`%d_dlc_ambience.snr/.sns`).
  - **Index 19 exists twice:** `19_reclaimed_b_fix` and `19_reclaimed_b`; the first in TOC order wins
    (likely `_fix`).
- **aud_wp_ambiences layout:** +0 volume, +4 bed index, **+8 (Hash_17C3…) = fade-OUT time**,
  **+12 (Hash_2E2A…) = fade-IN time**.
- **State machine** (`sub_824D3C28` update; `sub_824D3FE0` on key change, else `sub_824D41E0`; obj+64
  state, +68/+72 timers):
  - **0 silent:** a non-zero key starts its stream, obj+48 = key, go to state 3.
  - **3 fade-in:** timer += dt until the new zone's fade-in time, then state 1.
  - **1 steady:** a key change goes to state 2.
  - **2 fade-out:** over the OLD zone's fade-out time, then stop and free the stream (`sub_824D3B40`) and go
    to state 0; the new bed starts the next frame.
  - **Reversal:** back to the old key mid fade-out → state 3 with t = (1 − t_out/out) × in.
  - A key change during fade-in waits until the fade-in ends.
  - **One stream only:** beds never overlap.
  - Fade = linear attenuation 0..32767 as MixMap input 0 (state 0 → 32767, state 1 → 0).
- **Level** (`sub_824D42C8`, state ≠ 0) = MixMap out0 / 32767 × zone volume.
  - Pitch = out2 / 4096; LP = out3; the send is set once at start.
  - The graph is SnP1 → Rch0 → Rsp0 → Gai0 → LI20 → Sen0: no panner, the 5 channels play as authored.
  - Ducking (which MixMap inputs feed out0..3) is not settled.
- **Crossfade:**
  - The district bank comes from records of class F4917ACACAFAF913 field 33526BC9… (dist_downtown →
    Main_Ambience_Crossfade_DT, industrial → _Ind, university → _Uni; parks none). It loads into a 205 KiB
    slot (542D…); 153 (8B06…) is unused.
  - The pair list is {from, to, group 69359D…, level 875B…}. At the start of a fade-out (1→2) with
    new ≠ 0, the pair is searched **in either order**; with no match, the default is group 1, level 1.0.
  - `sub_824D0F38` posts `c_main_ambience_crossfade` with w1 32767, w4 4096, w5 25000 and w9 = group
    (1..25). In states 2 and 3, w0 = w8 = clamp(MixMap out1 × level).
  - It plays for the whole transition (fade-out + fade-in) and stops when the fade-in ends
    (`sub_828E2C78`) or a new transition replaces it. Never just near a border.
- **Reverb (+16):** `sub_824DE548` SFXObj_Reverb (default key 0xA2782D75A971CC8C) is merged with the type-5
  reverb emitters (`sub_82488278`); presets via `sub_824DE468` / `sub_824DEAE8` / `sub_824DE970` /
  `sub_824DE850`.
- **Trace hooks to confirm:**
  - `sub_824D3C28` entry (f1 dt, obj+48/+64/+68/+72);
  - obj+12 vtable+8 (r5 fade input);
  - `sub_824D42C8` (MixMap outputs 0..3);
  - `sub_824D0F38` (r4 group, obj+116 level);
  - `sub_824D3590` (r4 bed index, path at r1+256);
  - `sub_824D4D40` (bank load).

## Caveat (2026-10-02, evaluator spec)
The PoC evaluator (PR #4) mishandles op 5 (CallFunction): `*_msg` → `*_snd` sample counters never advance.
So the probe-measured SAMPLE CHOICE of function-driven banks (sirens, dogs, birds, bangs) is not retail;
timing, layers, pans and levels are. Ops 19/20/38 are missing in PR #4. water_fountain re-measured: 17 starts
in 30 s (not 24). Details: `aems-evaluator-spec.md`.

## Long runs: Downtown (2026-10-02; 11 of 14 locations, 5 min each; `long_run_stats.py`)
- **Location sets:** ours = retail on 6,350/6,350 position samples (10 sets).
- **Zone ambience:** retail's AMBST zone = our `audio_ambience` lookup at every stop:
  - Aletown dt_open, All Mart dt_open, Carverton dt_parks, Crystal Towers dt_less_busy;
  - Hotel District dt_main, Kube Tower dt_apt, Rippon dt_rez, Rosalita dt_main, Slappy's dt_rez;
  - Tresoutta dt_main, Uptown dt_apt.
- **Intervals:** all within their set's min..max (2 exceptions are carry-overs from the previous stop).
  `e_dwtn_skatepark` has min 7 > max 5; retail draws in 5..7, like ours.
- **Fires vs intervals:** about 60% (e.g. Aletown 32 fires / 53 intervals), the 2-loaded-banks skip rule.
  Compare with our engine's ratio (simulate the same sets) when validating.
- **Rare sounds not seen in 5 min:**
  - All Mart: Racey_cars_2/4, Tire_Squeals, car_backfire, car_crash;
  - Hotel: car_alarm_city_2, semi_truck_by_1, truck_by_1;
  - others: 1–4 per set.
- **Silent banks:** Siren_city_5 is posted (WPFIRE) but silent. Open: does retail keep its slot "playing"
  until the timeout? Ours frees it at once.
- Data: recomp session `long_downtown_a`, `long_dt_c` (`long_dt_b` re-run pending).
- **Update:** Downtown complete (14/14) and Industrial 8/12 (`long_dt_b`, `long_in_a`, `long_in_b`):
  - sets 5,766/5,766 more (Downtown total 12,116/12,116);
  - zone ambience = ours at every stop (indu_quarry, indu_drydock, indu_new_factory, indu_reclaimed_a; where
    retail logged no AMBST change the zone equalled the previous stop's, and ours agrees);
  - intervals all in range except post-teleport carry-overs;
  - rare: `work_whistles` never fired in about 20 min over four `e_ind_newfactory` stops.

## Zone bed level: −11 dB base VERIFIED (2026-10-02)
`check_bed_level.py` compared retail capture levels (1 s RMS percentiles, long-run stops) with the bed's
5-channel stream × zone volume folded like the capture (host fold 0.4·(front + surround + 0.5·centre)).
- Medians, retail vs predicted with −11 dB: dt_open −42.1 / −42.1; dt_main −39.2 / −38.7;
  dt_rez −44.2 / −43.6; indu_quarry −47.7 / −47.8. Without the base, every prediction is 11 dB too loud.
- The stream channel order L R C Ls Rs fits slightly better than L C R Ls Rs (0.3 dB). Unverified.
- Engine: `ambience.rs` `BED_BASE` × `cues::RETAIL_SCALE`.
- **Update:** Industrial complete (12/12) and University 5/10 (`long_in_c`, `long_un_a`): sets 2,570 + 3,170
  more = **23,622/23,622** over 31 locations and 25 sets. The zone ambience equals ours everywhere
  (indu_spillway, indu_old_factory, indu_shipyard, indu_reclaimed_a, univ_campus, univ_housing). All
  intervals in range. University sets fire sparsely (5–25 s ranges): 21–41 fires per 5 min.
- **FINAL (all 36 locations, 2026-10-02):** `long_un_b` adds 3,195/3,195 → **26,817/26,817** samples over 28 sets
  (Downtown 11, Industrial 7, University 10). The zone ambience equals ours at all 36. Intervals in range.
  Rare in 5 min: hawks, rattle_snake (megapark), cicadas (observatory), car_horn_suburb_1 (low_plaza),
  work_whistles (factories). Recomp sessions `long_*` (large: traces with MOD/BRD lines,
  f32 captures, shots; see todo disk-cleanup).
