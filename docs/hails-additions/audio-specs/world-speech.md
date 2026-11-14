# World speech (peds): data, mechanism, port state (2026-10-03)

Port:
- `crates/skate-audio/src/world/speech.rs`: the index;
- `speech_manager.rs`: value → event, the gate, the request words;
- `speech_rules.rs`: the `.evt` parser and the line and take choice;
- `peds.rs`: `PedSpeech`.

Setup: `tools/asset_pipeline/world_audio.py`:
- `speech_index`, with `rules` and each clip's `id` / `history`;
- `world_tuning` → `speech_tuning`;
- opt-in `decode_speech`.

Tests: the skate-audio `world::speech_*` unit tests, and `tests/world_speech.rs` on the dev install export.

Trace check: the local tool `speech_takes.py <sessions…>` (the take of each READ, the history rule, the sequence
records).
Dev install staging: `stage_world_audio.py [--decode 501,104,205,101|all]` (local tool).
Harness: `tests/world_sources.rs` `a_bumped_ped_warns_with_a_line_of_its_voice`.

## Data (`data/audio/english/livingworldspeech.big`, EB v3)
- **Clips.** 3,011 `.dat` clips named `<event>_<voice>[_<voice name>]_<line>.dat`. Examples:
  - `501_59_busm1_Warn_n`
  - `1901_53_Shout`
  - `806_47_adtf2_Int_c14_tour`
  - Voice ids run 41–96 (`adtm1`, `adtf1/2/4`, `grn1/2`, `joc2/3`, `busm1–3`, `busw1–3`, `tenm1–3`, `stdf1`,
    `tenf2/3`, `secg1/2`, `torm1–3`, `torf1`, `bum1–3`, `sktm1/4/5`, `sktf1–3`). Clips without a name in the file name use
    a generic voice (53, 91, …).
- **Archives inside** (EB v3, read at boot):
  - `livingworldhdr.big`: one `<clip>.hdr` per clip. Layout: +2 u16 take count, +14 u16 offsets of each take's `.sth` row.
  - `livingworldsth.big`: one `<clip>.sth` per clip. 12-byte rows = u32 byte offset of the take in the `.dat` + the
    take's 8-byte EA SNR header.
- **Takes.** All are codec 3 (EA-XMA2), mono, 36 kHz. A clip holds 1–48 takes, 20,096 in all and 14.3 h. Free-roam events come to
  about 9.3 h, about 2.4 GB as PCM16; events 101/104/205/501 alone are 518 MB.
- **Decoding.** vgmstream decodes `<take>.snr` (the 8 header bytes) plus `<take>.sns` (the `.dat` slice) pairs.
- **Recomp reads.** READ lines point at a take's offset inside the clip, not only the clip start (fixed in
  `speech_nearfar.py`). `speech_takes.py` maps each READ to its clip and take.
- **`livingworld_Events.evt`** (114 KB): the speech library's event table, 81 events (`501_warn` = 0x2012, …).
  - **Decoded:** `world_audio.parse_evt` and `speech_rules::EventTable::parse`. All four banks parse: maincast bank 0,
    living world 1, announcer 3, cameraman 4.
  - **Header:**
    - +4: u32 offset of the name table (32-byte rows);
    - +8: bank; +9: sub-bank;
    - +0x10: u16 event count;
    - +0x18: u16 event offsets × 4.
  - **Event:**
    - u16 id, u16 queue timeout (60–180), u16 priority (500–550);
    - u8 record count;
    - u8 external conditions (0 in all banks);
    - u8 flags (high nibble = field count);
    - u8 probability % (100);
    - u8 flags2 (0), then one more byte;
    - u16 record offsets × 4;
    - 3-byte field descriptors `FF <request word> 04`.
  - **Record:**
    - weight code (0x39 = 4 × 25 everywhere in the living world);
    - probability (100);
    - clips << 2 | mode;
    - locals;
    - field count, then 3 pad bytes;
    - one byte per clip (offset × 4);
    - u32 field masks (0 = any);
    - 8-byte clip entries (u16 `.hdr` id; the rest is 0 in the living world).
  - **Fields:**
    - 1 = speaker type bit;
    - 2 = voice variant bit;
    - 3 depends on the event: the near/far flag, zombie (1901, 201, 606, 609), the conversation partner's type (806 / 807),
      or the conversation word (550–557);
    - 4 = zombie for 603.
  - **Type bits** (from the voices of their records):

    | bit | type | voices |
    |---|---|---|
    | 0x1 | adult m | 41 / 42 / 43 |
    | 0x2 | adult f | 46 / 47 / 49 |
    | 0x4 | granny | 51 / 52 |
    | 0x8 | jock | 55 / 56 |
    | 0x10 | teen f | 72 / 73 / 74 |
    | 0x20 | teen m | 69 / 70 / 71 |
    | 0x40 | security | 75 / 76 / 77 |
    | 0x80 | tourist m | 82 / 83 / 84 |
    | 0x100 | tourist f | 85, **53** |
    | 0x200 | skater m | 89, 91, 92, 90 |
    | 0x400 | skater f | 94 / 95 / 96, 93 |
    | 0x1000 | business m | 59–61 |
    | 0x2000 | business w | 64–66 |
    | 0x4000 | bum | 87, 88, 86 |

    Variant bits: 1, 2, 4, 8 and 16.
  - 133 of the 3,348 living-world clip references have no clip (for example 603's tazer lines for some voices). Those
    records never play.
- **`.hdr`:**
  - u16 id;
  - +2 flags (0 in all banks);
  - +3 take count;
  - +4 runtime;
  - +8 history length: the take count, or 0 for 685 clips (conversations 550–557, zombie lines, 497 Silence);
  - +9 / +10 size scale;
  - +16 u16 `.sth` row offsets;
  - then the history ring (cursor + entries, 0xFF = empty).

## Mechanism (TU3)
- **State graphs.** The ped state graphs (`data/state/livingworldentities/pedestrian/aigraph/*.xml`) send
  `SendSpeechEvent speechevent=… speechvalue=N` on state entry. The values are listed in `speech::SPEECH_VALUES`, for example
  10 CollisionNearbyReaction (wanttoobserve), 25 StopCheer (slamreaction), 23 LongCheer (nearbyskatertrick),
  20 Flee, 14 JoinChase, 17 AttemptTakeDown, 19 TakeDownSuccess, 12 ChaseResting, 66 EscapedEndChase.
  - The warn (11, `pedestrian_dowarning.xml`) and takedown success are commented out ("moved to the code").
  - The value lands in the ped audio state +136. Footstep packets read the same field (jump / collision).
- **SFXObj_PedestrianSpeech** process `sub_824D9908`:
  1. When +136 changes, it builds a request to the speech manager `*(0x830CFDDC)`: `sub_824AB6C8` (or `sub_824AC438` with a
     target).
  2. flag = 1 if +148 > +156 else 2.
  3. Values 7 / 8 are remapped to 30 after a 29, else 51.
  4. Value 49 goes to `sub_824D9C70`.
  5. Value 29 (photographer) repeats on a timer while a global flag is set.
  - Ported: `PedSpeech::process` (49 and the 29 timer are not).
- **Speech manager** (`Sk8::Audio::TheSpeechSystem`, global `*(0x830CFDDC)`). **Ported** (`speech_manager.rs`).
  - **Request block** (`sub_824D9908`):
    - w0 = `+96` type bit, w1 = `+88` variant bit, w2 = flag, w3 = `+92`, w4 = `+120`, w5 = `+124`;
    - speaker slot = `+84`.
    - The living-world path needs `+116 != 0`. Otherwise `sub_824AC438` sends main-cast events (pros; not ported).
  - **Value → event** (`sub_824AB6C8`): the table in `event_for_value`.
    - Coin flips use `rand()`; bums use `rand() % 3`; guards get radio lines.
    - Value 6 with `+71` also sends main-cast event 0x8017 (not ported).
  - **The request** (`sub_824ABA18`), in order:
    1. Game-state preconditions.
    2. The vault tuning of the event (class `Hash_9C1F48F5D637E275`, `SPCHType_1_EventID`).
    3. Timers (`sub_824A8C78`): one per speaker slot × 81 events, f32 seconds, starting at 0.
       - The same event needs ≥ `+24` s.
       - Any event needs ≥ `+8` s, when `+8` > 0.
       - Not-follow needs strictly more than its time. Ids 0 / 294 / 8318 / 33245 / 24752 are skipped.
    4. `sub_824A75F0`:
       - probability: `rand() % 1000 × 0.1f` < `+20`, or ≥ 99.9;
       - the player's speed × 3.6 within `+32` / `+36`;
       - the challenge list;
       - the `+40` / `+44` timers;
       - the `+49..51` game flags;
       - zombie mode needs `+60`.
    5. w9 = 2 in zombie mode (virtual `[-620]+212` vfunc 156; the `zomb_Shout` records prove it).
    6. `sub_824A73F0`: interrupt by priority (not ported).
    7. `sub_824ABD90`: the request words of each event.
  - The timers restart when the line starts (`sub_824A90B8` from `sub_824A84B8`).
- **Speech library** (generic, `sub_829717A0` …). **Ported** (`speech_rules.rs`).
  - `sub_82971480`: the event probability (fails when `(draw>>16)·100>>16 > p`), then the queue slot (not ported).
  - `sub_82973CB8`:
    - the weighted record order (`sub_82972980`): weight = 4^(b>>5)·(b&31), scale table at 0x82FDB3D8;
    - per record: a probability draw, the field match (`sub_82973BD8`) and the clips (`sub_82972D70`: at most 12; a
      missing clip fails the record).
  - Candidates per clip (`sub_82972660`): the takes not in the history ring; when every take is in it, the oldest one.
  - Pick (`sub_82974220`):
    - index = `(draw>>16)·n>>16`;
    - redrawn (at most 32 draws) while it is among the last min(n/2, 10) picks of the same header in a global 32-entry
      (index, header) ring;
    - if every draw hits, the one whose match is oldest.
  - `sub_82973408`: writes the history when the line starts.
  - Generator: add-with-carry at 0x82FDB3C0. Retail shares this state with the grain player's title generator.
  - **The queue is not ported** (`sub_82971340` / `sub_82971890` / `sub_82971DA8`):
    - 16 request slots and 8 streams;
    - highest priority wins, then the newest; older requests are dropped;
    - timeout = event +2, in a clock whose unit is not known.
    - The recomp plays several living-world lines at once (`overlap.py` (local script)), so the living-world
      channel is not exclusive. How is not resolved.

## Reaction → event (measured; `speech_reads.py`, sessions 161849 / 163809 / 164620; the clip starts 0.03 s after)
| reaction | events (evidence) |
|---|---|
| bump → warn | 501 Warn_n (M) |
| bump → warn + taze (female) | 501 Warn_f (M); 330 / 331 tazer lines (name) |
| bump → flee | 108 ChsFlee (M) |
| slam nearby | 104 Slam_n (M) |
| collision nearby | 205 SpecCol, 204 Gasp, 202 HImpRct (M) |
| trick nearby | 101 SpecPos_n (M) |
| chase start | 105 SpecChs bystanders (M), 603 chaser (name) |
| greet / returngreet / conversation | 1901 Shout, 806 Int_c<n>, 807 Rct_c<n> (M) |
| ambient (no reaction) | phone 4402 → 805 → 4405 with 497 Silence; bums 206 / 207; guard radio 102; pro lines from maincast (906/101/130) |

`speech::REACTION_CUES` holds this table, for logs and as evidence. The real choice is `SpeechManager::request`.
`speech::choose` (uniform) is only a fallback for hosts without the rules export.

- **`_n` / `_f` lines: solved from the data.** Request flag 1 (ped `+148 > +156`) means far.
  - The flag-1 records name `101_51_GenPos_Grn1_far` / `104_51_grn1_Slam_far`, and the flag-2 records `…_near`.
  - All 366 `_f` / `_n` records of 101 / 104 / 202 / 203 / 501 split the same way (`tests/world_speech.rs`).
  - **Settled 2026-10-03 (`world-audio-hookin-spec.md` §7.3 G2):** `+148` = the ped's distance to the listener,
    `+156` = `aud_characteristics` field `A27215A909135B62` (20 m for peds, 30 for pros), so `_f` lines play beyond 20 m.
  - (Was:) what `+148` / `+156` hold (a distance and a threshold?) is not traced. `speech_nearfar.py`'s distance join is still
    unreliable.
- **Take choice vs the recomp** (`speech_takes.py`, 10 clean sessions; reads inside > 500 ms audio stalls left out):
  - 57 of 57 clips with 2+ plays follow the history rule, including a full cycle. A uniform pick would have repeated a
    take in about 13.6 of them.
  - 15 of 15 multi-clip lines are one record, in order.
  - The first takes of 806 lines are often 0 (6 of 13 in 164620). This may be chance, or a pairing we cannot see. Keep an
    eye on it.
- **Coverage.** 40 voices have 501 and 104 lines, 39 have 205, 36 have 101 (harness print).

## Playback (port)
- `speech::SPEECH_BANK` (`1 << 22`) mixer bank. `SpeechSlots` maps (clip, take) → slot. Takes play as direct voices.
- The harness renders a busm1 warn take (5.49 s, peak −9 dBFS at unity gain).
- **Level / pan: open** (next step).
  - The stream request (`sub_82973408` → `sub_82C5F310`) is 9 words: offset, size, channel, and the context (ped `+64`).
    It goes to a SpeechBank stream. The voice's level is set where those requests are used; that code is not found yet.
  - A lead: in 163809 a speech READ comes with a `GAIN … 0.0 → 1.0` and a `PITCH` ratio of ~0.52–0.70 on one voice
    (0x40C43350).
  - The speech manager reads the PedestrianSpeech owner outputs itself, and which of its 24 outputs is
  not known (E0 out2 −1000 dB via B2 is the main volume candidate; out13/14 filters; out11/12 via B20 = 2–70 m
  camera distance).

## Waiting on the engine
- **The ped system** must provide:
  - speech values on state entry (`PedState.speech_value`);
  - the speaker fields (`Speaker`: slot, type bit, variant bit, partner type);
  - the near/far pair;
  - zombie mode;
  - the player's speed;
  - a manager clock.
- **The game host:** world_sources logs requests (`AUDIO_WORLD speech …`) but plays nothing. To play, it needs:
  1. the index and `rules` from `speech/livingworld.json` → `SpeechIndex::set_ids`, `EventTable`,
     `Library::new(ClipHeader…)`;
  2. `world_tuning.speech_tuning["1"]` → `EventTuning`;
  3. `SpeechManager::request` for each `SpeechRequest`;
  4. `SpeechIndex::picks_to_lines` → `SpeechSlots` with the decoded takes, played in sequence;
  5. the level mapping.

  The JSON → struct mapping is in `tests/world_speech.rs` `load()`.
- Maincast / cameraman / announcer speech (pro skater lines around the player, the cameraman's "own the spot"):
  same format, not indexed yet.

## Next steps
1. Level / pan: find the code that uses the SpeechBank stream requests (the 9-word requests `sub_82C5F338` queues) and the
   speech voice's GAIN / SEND. Or hook it (category `audio`) and join it with the ped distance.
2. What `+148` / `+156` are: find who writes the ped audio state, or add a first-pass hook on `sub_824D9908` that logs
   both.
3. The request queue and stream count for the living world (why lines overlap in the recomp).
4. Main cast / cameraman / announcer: they use the same library and already parse. Their managers differ (other vault
   types, `sub_824AC560`).

## Speech details resolved (2026-10-03, doc 15 "Gaps closed")
- **Block layout** (PedestrianSpeech update `sub_824D9370`, block = obj+44; PlayerSpeech `sub_824DA300`, obj+48):
  +4 speaker voice id (parsed from the clip name by `sub_824A89E8`), +8 main level, +12 the per-voice float
  (S+152), +16 raw azimuth, +20 pitch, +24 HP (ped out13 / skater out8), +28 LP (out14 / out9), +32 second level
  (out15… / out10…), +36/+40/+44 the PEAK curves' values, +48 a delay = min(S+148 × 1.0 [field BF48032DB145C5B4]
  × 1/344, 0.15) s refreshed every 4 frames (field 510BAFA32A76B340), +69 far flag, +72 the stream the speaker
  holds (−1 none), +76 speaking, +80 level 21 (ped) / 13 (skater), +84/+88 filters 22/23 (ped) / 14/15 (skater).
- **PEAK filter = head shadow by azimuth.** raw out0 folded (> 32767 → 65536 − raw), then three 8-point curves of
  record `B29C3B2C13D96482` `default` (holder `*(0x830CFDA4)+44`): center `2C166907CF51DB88` (600 → 4000 at the
  side → 600), gain `EA2C18D9CE5CBA3A` (0.4 → 0.1), Q `CF8679F540B82B2B` (3). Recomp 163809: 6 / 6 PEAK (center,
  gain) pairs lie on the two curves at one azimuth (≤ 1 Hz); ours at the joined geometry 3 / 6 within 10 % (camera
  orientation estimated). Ported: `SpeechVoiceTuning`, export `world_tuning.speech_voice`, mixer stream graph.
- **Two sends.** The stream voice graph (`sub_82C5C318`): Rsp0 → PI20 → Sen0 (Send A, pre-gain, env) → Gai0 →
  HI20 → LI20 → Sen0 (Send B) → Pn21 → Sen0. Recomp modules `+0x570` (A) and `+0x7D0` (B). A = level 21 (ped) /
  13 (skater): 17 of 29 lines within 25 % (out15 there: 11); B = out15: 18 of 27. Send B feeds the stream slot's
  echo submix (`sub_82C5E2D8`: Sub0 → HI20 → Del0 → LI20 → Pn21 → Sen0 → the env bus; delay = block +48; filters
  likely 22 / 23): **not ported** (Send B is computed, not played).
- **Value 49** = the phone: `sub_824D9C70` stops the old ring and starts CellPhone_Rings (Splice index 6) container 5
  (class `C1831BDB6CB1B1EA` `cellphone` field `031EFDF991638985`); `sub_824D9AD8` (end of the update) follows it
  with the main level / pitch / azimuth and, when it ends, requests value **64** (`4402_cell_greet`). Recomp
  164620: SPLC 6/5 at 88.73 → 4402 HelCel 89.73; 234.38 → 235.57; the rings' records last 0.84–1.13 s.
- **Value 29**: while system byte `*(0x830CFDC4)+912` is set, obj+160 accumulates dt; at ≥ 1.0 (image constant) it
  resets and the request repeats (the flag's meaning not traced: `LivingWorldAudio::photo_flag`).
- **Obj:Speech inputs** (SFXObj_Speech process `sub_824E2050`, every frame): in0 / in1 / in4 = 32767 while a playing
  line's speaker (block +4) is 37–38 / 75–77 / 1–29; in2 = speech-system word `+0x1BD28` == 2; in3 = a block with
  +60 == 0 and +93 set (neither traced). F45 (+200 mB on ped out2 / out3) is in0 → not regular peds: the ~+0.6 dB
  of the recomp's near lines stays unexplained (E0's other terms A[Global.2], F[Global.28] / [108], A[Global.112]).
  Ported: in0 / in1 / in4.
- **Which stream**: a new line takes the first free stream (k0 when both are free: 95 of 109 new lines in
  163809 / 164620 / 180430). The interrupt reads record `channel × 2 + block +72` (the speaker's own stream;
  −1 aliases the previous channel's second record).
- **Queue timeout unit**: the library's clock = the speech system's time callback `sub_824A4E90` =
  `[[0x830CFD94]+16]` (installed by `sub_82C5EC20`); its unit was not settled from the code (no writer found).
- **Constructor** `sub_824D90D8`: obj+36 (last value) = 68, obj+40 = 8318, +156 = −1 (ported: `PedSpeech::default`).
- **Main-cast path (pros, S+116 == 0)**: `sub_824AC438`: 29 → event 141 (speaker word 136), 28 → 11 (when
  `sub_82487ED0` is false and system +1196 == 0), 30 / 51 → stop the speaker's stream, then 6 or 115 (`rand() & 1`),
  53 / 54 → 77, else nothing; request `sub_824AC560` (event id → index in a 72-entry table at `0x8224CB80`, vault
  record at mgr + (index + 194) × 8, tuning bank "0", exported) with words [S+100, S+104, flag 2/1 swapped, 0,
  S+108, S+112]. **Not ported:** needs the main-cast speech index + decode (`maincastspeech.big`).

## The echo send and the main cast (2026-10-03, ported; corrects "Two sends" and "Queue timeout unit" above)
- **The stream system** (`sub_82C5CEF0`, every frame per speech stream): gain = block +8 / 32767 × block +12 (the
  per-voice float `S+152`, `aud_characteristics` `2087A3290483BB4F`, 0.8–1.4); `Send(desc+32)` = block +32 (ped
  out15 / skater out10) × the float, the post-filter send straight into the environment bus; `Send(desc+16)` =
  block +80 (ped out21 / skater out13) × the float, the pre-gain send into the slot's echo submix. The echo's
  HI20 = block +84 (ped filter 22 / skater 14), LI20 = block +88 (23 / 15), Del0 = block +48, posted only when it
  changed; Pn21 and Sen0 keep their class defaults. **Correction:** the first decode had Send B (out15) feeding
  the echo; the code says the pre-gain send (out21) does, and out15 goes to the environment bus.
- **Delay refresh** (`sub_824D9370`): a per-speaker countdown; at ≤ 0 the delay is recomputed from the camera
  distance and the count reloads with field `510BAFA32A76B340` (4).
- **Ported:** `skate_audio::bus::speech_echo` (one graph per stream slot, channel × 2 + k; mono, the Pn21 / Sen0
  routing as a unity tap), the mixer's stream path (PEAK → echo send → gain → HPF / LPF → env send), the voice
  float, the delay countdown. Setup `world_tuning.speech_voice` carries `delay_factor` / `delay_frames`.
- **Recomp, re-verified** (39 lines, 163809 / 164620 / 180430, with the voice float): pre-gain send (`+0x570`) =
  out21 within 25 % in 17 of 29 (out15: 10); env send (`+0x7D0`) = out15 in 19 of 27; gain median 0.988
  (p10 0.55, p90 1.36); filters 4 of 5; PEAK on the curves 6 / 6.
- **Queue timeout unit (settled):** `[[0x830CFD94]+16]` is the function behind the `GetVisualGameTick` Lua binding
  (`0x8283C2D8`): visual game ticks, one per rendered frame (the console's ~30 fps), the port's console frames.
- **Main-cast channel** (bank 0, channel 0; `maincastspeech.big`, 1283 clips, 73 events, tuning `speech_tuning["0"]`):
  - who: a model with no living-world type bit and a cast bit / word (`6F2933E977CF40DD` = the pros' bit 1 << (n−1),
    `14FD437D190677C8` = the special cast 30–38, `D6EA428C2B43E23A` = the word another pro's line names it by);
  - words (`sub_824AC898`, block w0 = cast bit, w1 = cast word, w2 = 1 near / 2 far, w4 / w5 = the other skater's
    cast bit / `D6EA` word): events 0 / 1 → [w1, w0, w2]; 247 → [w0, w9, w1]; 251 → [w0, w1, w10];
    254 → [w1, w5, w0, w4]; 268 → [w0, w1, w11]; 271 / 274 / 275 → [w0, w1, w12]; 280 → [w0, w1, w13];
    287 / 288 → [w1, w0, w4, w5]; else [w0, w1];
  - NPC skater speech (`SFXObj_PlayerSpeech` non-local process `sub_824DA1B0`, inputs from the skater entry via
    `sub_824B6E80`: `+102` = entry `+392` bit 4, `+103` = bit 3, `+104` = entry `+396` bit 10, `+131` = entry `+388`
    bit 5). Living-world voices (`sub_824DAAC0`): `+102` / `+103` → 8204 in mode 55, 8239 in modes 19 / 20, else
    8234 | (`rand()` & 1); `+104` → 8202; the chase flag (system `+1100` bit 6) → 8205 when nothing else; a request
    only on change. Pros (`sub_824DA768`): `+102` → 1 (16 in modes 19 / 20 by the player) or 288 by a pro
    (`+112`); `+103` → 1 / 288 by `+116`; `+104` → 0 / 287 by `+108`; requested every frame the byte holds. The
    skater's crash (`sub_824DB688`, `+131` rising, latched in obj `+193`): message 8229 / 125;
  - skater message pairs (`sub_824DAC00`, living-world / main-cast): bail grunt 8206 / 115, crash 8229 / 125,
    impact 8233 / 6 and 8241 / 125 (`sub_824BD358`), hit reaction 8309 / 253 and race punch 8310 / 250
    (`sub_824EFA60`), gesture 8291 / 247 (`sub_824F8778`).
- **Main-cast decode:** the free-roam events (101, 104, 130, 150, 151, 201, 202, 335, 336, 338, 344, 500, 501, 601,
  903, 906, 913, 1014): 6596 takes, 3.0 h, 922 MB of 44.1 kHz PCM16 (opt-in with the living world's).
- **Recomp check:** 82 main-cast lines streamed in 11 sessions (READs of `maincastspeech.big`: 906 `AiSlam` ×45,
  101 `pos`, 150 `pro_pos`, 104 `Slam`, 130 `col`, 201 `grunt`): all reachable through the ported words for their
  speaker (76 of an event the port sends; 130 `_col` has no ported sender).
- **Not ported:** the repeat times of speaker slots 30 / 31 (record `+52` / `+56`); values 30 / 51 stopping the
  speaker's stream first; the cameraman line of the crash; the game modes (free skate passes 0).

## Follow-ups (2026-10-04, branch `audio/world-followups`; corrects some lines above)
- **`Obj:Speech` in0 / in1 / in4 see the main cast's streams only.** `sub_824E2050` walks the stream system's
  records `+1120` / `+1124` = streams 0 / 1 = channel 0 (the main cast; the pro path's stop `sub_82C5E1D8(sys, k)`
  indexes the same records). Main-cast voices include 77 (18 clips) and 37 / 38. A living-world guard (75 / 76)
  raises nothing (the port did raise in1; inaudible in free skate: F42 is scaled by `Master.in7` = 0, F19 feeds
  nothing). **in2** = speech-system word `+0x1BD28` == 2: only the scripted-dialogue path drives it
  (`sub_824A4EE8`, request ids ≥ 5000, categories 31–40; 1 while its stream starts, 0 from `sub_824A54D0`). **in3**
  = a main-cast stream block with `+60` == 0 and `+93` set (`+60` from the stream start `sub_82C5DC80`'s fourth
  argument). Neither in free roam. They duck the music (F29, F46) and F221 / F222 (with Pause.in2).
- **The near lines' ~0.6 dB is gone** since the voice float: 34 lines median 0.988; near (< 10 m, 24) −22 mB, far
  (10) +45 mB. `VU.in0` (`A[Global.112]`, +200 mB on PedestrianSpeech out2 / out3; `SFXObj_VU` process
  `sub_824EDBE8` meters a mixer bus and slews it into in0 / in1) does not explain any residual: ratio vs the
  capture's RMS before each line, correlation −0.20 (a local script, not published).
- **Main-cast repeat times, slots 30 / 31 (ported):** `sub_824AC560` uses record `+52` for speaker slot 31 and
  `+56` for slot 30 instead of `+24` (even 0). Slot = model `S+84`: 31 `skate_coach`, 30 special cast
  `47231014932CE8B3`. Export `repeat_speaker_31` / `repeat_speaker_30`; `EventTuning::speaker_repeat`.
- **Values 30 / 51 stop the speaker's stream first (ported):** speaking byte block `+76` set and stream block `+72`
  not −1 / 2 → `sub_82C5E1D8` before the request (regardless of the gate). `SpeechPlayer::stop_speaker`.
- **Value 29 = two requests (correction):** 141 (`1014_bored`) then 136 (`1002_amb_chat`), not "141 with word 136".
- **The crash's "cameraman line" is the announcer's** `480_slam_pro` (event 24708 = bank 3 event 0x84): after the
  crash message, when the skater's camera distance (record `+96`, `sub_824B6E80`) < speech record field
  `887C1D3324B12C4A` (12.0 m), request via `sub_824AA858` with word 2 = the model's `14B23B4527AF919E`
  (`SPCH3Type_pro_id_ANN`, the pro bit), when non-zero. Needs the announcer channel (`announcerspeech.big`, 63
  events): not ported.

## The announcer channel (2026-10-04, branch `audio/world-followups`, ported)
- **Data.** `announcerspeech.big` (`announcer` prefix; EB v3 like the others): 405 clips, 2,822 takes, 3.4 h,
  36 kHz; `announcer_Events.evt` = bank 3, 63 events (ids 24576–24751). Field 1 of every record = the announcer
  (1 / 2), whose clips use voice 35 / 36. Tuning: `speech_tuning["3"]` (e.g. `480_slam_pro`: repeat 15 s,
  priority 90, probability 100, timers `+40` 8 / `+44` 5).
- **Announcers.** `aud_characteristics` models 35 / 36 (parent `ip`), `SPCH3Type_char_ID_Ann`
  (`6F9C8A27E4CD37DC`) 1 / 2. The pros carry `SPCH3Type_pro_id_ANN` (`14B23B4527AF919E`): bit (model − 1), the
  word `480_slam_pro` names them by.
- **Request** (`sub_824AA858`). Steps in order:
  1. the record `+48` / `sub_8279E180` gate (as the main cast);
  2. word 0 = `char_ID_Ann` of model system `+1036` when 0 and `+1036` < 104;
  3. word 8 = 4 | (1 + (system `+1089` ≠ 0));
  4. timers kept for slot `+1036` (104 → 35);
  5. gate `sub_824A8C78` / `sub_824A75F0`: one `rand()`; timer `+40` against system `+904` − `+900` (timed) or
     `+908`, `+44` against `+900`;
  6. `sub_824A73F0` on channel 3;
  7. library request `sub_824AAC40`, whose words per event come from two jump tables at `0x824AAEF0` / `0x824AB09C`.
- **Who is announcing.** `+1036` is written from the challenge record (`sub_82488330`), and is 104 with no
  challenge. So free skate never matches a record.
- **Senders.** 39 calls. In free skate only the NPC crash (`sub_824DB688`) can ask: camera distance (record
  `+96`) below `887C1D3324B12C4A` (12 m), and `pro_id_ANN` ≠ 0, then 24708 with word 2. The rest are challenge
  and contest code, plus `PlayAnnouncerSpeech`. SFXObj_Announcer's commentary (`sub_824CF350`) runs only for
  announcer 35 / 36.
- **Voice.** The fixed block `mgr+0x1B898` (constructor `sub_824A3C28`: voice float 1.0, PEAK 96 kHz / 1.0 /
  3.0, echo 0). The update (beside `sub_824A7FA0`) reads SFXObj_Announcer at `mgr+0x1B8F8` (copied by
  `sub_824D07B8`):
  - level = int(out2 × `sub_824A8250` multiplier), from the speech record's arrays by language: French
    `CCC18F677B896049` / `CAD1FAC38891DA18`, German `7A9D965C3AE7BB73` / `E0F263477B891647`, other (English)
    `49AE841BE63F9EB7` / `60E0221FD3D6F04B`, the second of each pair with the challenge byte `+1044`;
    index 36 → 1, 31 → 2, else 0;
  - azimuth out0, pitch out1, HP out4, LP out3, env send out5.

  `Announcer.in0` = 32767 while the stream system plays channel 3 (`sub_824CF218`).
- **Recordings.** 38 sessions read only the archive's index (5 reads at boot), never a clip, though they hold
  37 NPC crash lines.
- **Port.** `skate_audio::world::announcer`, `speech_player::announcer_outputs` / `no_cut`, the host's announcer
  channel, `LivingWorldAudio::announcer`, `AnnouncerSpeechEvent`, `sdk.world_audio.announcer` / `announce`;
  setup `speech/announcer.json`, `ANNOUNCER_EVENTS` = (480,) for the opt-in decode.
- **Open.**
  - The challenge clocks (`+900` / `+904` / `+908`).
  - `+1089` / `+1044`.
  - The `+48` gate.
  - The interrupt's fifth argument.
  - The challenge senders.
  - A recorded announcer line to check levels against.

