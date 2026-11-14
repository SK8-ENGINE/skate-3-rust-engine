# NPC (AI) skaters' board sounds: spec and port status

2026-10-03, overnight step 4 (headless). Code: `skate_audio::world::skaters` (runner), host
`crates/skate-game/src/game_audio/npc_skaters.rs` (inert until an AI-skater system publishes). Doc 11 "NPC
skaters' board sounds". Plan: the world / NPC audio inventory (row "NPC skaters' boards"; see doc 15).
Source: the TU3 (title update 3) recompilation (reference only, addresses are facts); "the recomp" = its traces, which run
uncapped and stall.

## 1. Which skaters get sound (retail mechanism)

| what | where | fact |
|---|---|---|
| records | `CSTATEMGR_Player` init `sub_824F1F40` | exactly **2** `CSTATE_Player` records (88 bytes, creator `sub_824F8D60`: +64 id −1, +68 skater index, +72 local byte, +76 39, +80 −1, +84 soft) = the MixMap Player slot's 2 instances (`RETAIL_INSTANCES[1]`) |
| manager update | `sub_824F1FB0` (vtable `0x822FD660`) | gated by a game state word == 5 and `sys+560` == 1 (free roam; not decoded further). Finds the local skater (entry +152 bit 31); if its index / id changed since last time, every record is deactivated. Then for every skater entry in list order (`*(*(0x83083C38)+0x2F070)`, count +12, entries of 544 bytes at +240): NPC skaters only when `sub_824F8EF8` holds; an existing active record with the same id (+152 bits 16–19) keeps it (re-created if character +76 / +80 changed); else `create` |
| distance gate | `sub_824F8EF8` | \|entry position − `*(0x830CFDD4)`\| < `0x820D4924` = **30.0 m**. `0x830CFDD4` is the listener point = the camera (GRECX notes) |
| create | `sub_824F2238` | the first record with active byte +52 == 0; when none is free only the **local** skater evicts the first record. NPCs get nothing |
| record update | `sub_824F8E18` | a non-local record whose skater index ≥ count, or whose skater is ≥ 30 m from the listener (or NaN), is deactivated (vfunc 28) |
| record init | `sub_824F8ED0` | copies +64..+80 and sets +84 = 0 |

So: the local player holds instance 0; **instance 1 goes to the first NPC skater (list order) that comes within
30 m of the camera while it is free, and stays with it until it is 30 m or more away.** At most one NPC board
sounds at a time; the other 4–14 AI skaters around the player are silent (their boards; speech is separate).
Ported: `skaters::Slots::assign` (unit tests in the module).

**Recomp check** (`instance1_voices.py SESSION` (local script)): the second Contacts object
(`0x40C710A0`, the CONTACT hook on `sub_824B86E0`) fires in bursts whose impact speeds match a nearby AI board's
SKATEB speed and landing (e.g. 164620: 326.4 s and 329.1–329.5 s = board `468B18F0`, 25 m from the player,
10.4–11.3 m/s, landing 0000000 → 1111000; 394.3 s = `468B1500`, 14 m) while the local player stands still or is
in the air; the earlier note "both contact objects are the player's" was wrong.

## 2. What runs for the NPC instance

Every player SFX object of instance 1 runs; the audio-state bridge `sub_824B0DA8` fills the instance's state from
the skater entry at the record's index (+68), so the components see the NPC's physics. Local tests
(`[record+72]`) select the non-local branches:

| component | non-local behavior | ported here |
|---|---|---|
| PlayerPhysics inputs (`sub_824B19C8`) | in9 = 32767; in12 = `sub_824B23C8` (below); in13 = \|state+96 − `[G+0x2F078]`+32\| (the NPC's COM velocity against the local player's), capped at 35, slewed ±100/s (state +784), × 32767 / 35; vault class `0xC1831BDB6CB1B1EA`: `204DCC9296DC9400` = 35, `0395642CA6FC543A` = 100 | `Physics::write_against` |
| 3DObjPos 1.1 / 1.2 (g) | the NPC's COM / board; distances against the local skater and the camera | `ObjPos` at `obj_pos(g)` / `obj_pos2(g)` |
| Contacts / Rail / OffBoard inputs | landing pulse and bucket; landing-material flag local only; OffBoard local only | `inputs::Contacts { instance }`, `write_rail_at`, `write_off_board_at` |
| SkateBoard routing / Class_rolling (`sub_824C5CA8`) | one pass, no held layers 0 / 3 | `Rolling` (already ported non-local); grain binds collected in `NpcSkater::routed` |
| grain bed (SkateBoard update `sub_824C6BD8`) | runs per instance; soft member via `sub_824B23C8`; local only: seam envelope `sub_824CA448`, graph-3 send / level ramp `sub_824CAEC0`, wobbles `sub_824CB180`, graph 3 (`sub_824C8878`), FlangeSub sends 21 / 22, rocket | yes (2026-10-03): `Runtime::npc_grains`, `grain_bed::Bed::for_instance`, shared pick generator (doc 11 "NPC skaters' second grain bed") |
| Rattle | as the local | yes |
| Class_Seams (`sub_824C14C8` / `sub_824C1F18`) | gate = record active byte (+52), not +72: runs for NPCs | yes (the port's `!local` early returns were removed; they were never retail) |
| Class_grind + on / off sounds | w13–w15 = 0 (local-only words); on/off bus `create` = false | yes |
| Class_wheels_skid, Class_Squeaks, Class_foot_drag | soft word via `sub_824B23C8`; w14–w16 local only | yes |
| SenseOfSpeed wind / rattle (`sub_824E7980`) | returns at once for non-local | yes (posts nothing) |
| SFXObj_Contacts Splice one-shots | non-local branches already in `player::contacts` (no landing impact, spread 0, bus create false) | yes |
| collision messages | local byte clear (no pitch override), into the shared `CSTATEMGR_Collision` | `NpcSkater::take_collisions` → host |
| body poster bail helper `sub_824BF5F8` | non-local only: finds the skater's `CSTATE_SkaterSpeech` record (sys+708, PlayerSpeech slot) by id and posts message 8206 / 115 to its +144 object = **an NPC bail grunt (speech)** | no (speech) |
| Wheels spin-down, Tricks, Treatment, OffBoard footsteps, Clothing, board slide | run per instance in retail | not yet (body foley / streams; next step) |

**Soft word `sub_824B23C8`:** local → state +684. Non-local → walk the Player records (`*(0x830CFDC4)+660` = the
slot-1 container, list at +16) for the one with +72 set (the local) and return `(+84 == 0)`; +84 is written by the
bridge with the same bit as +684 (`sub_824B0DA8`: `stw r11,684(r31); stw r11,84([r31+16])`). So an NPC posts the
**soft** words / grain members exactly when the local player's wheels are **hard** (no local record → 1).
Recomp 164620 (user's board hard): soft grain members play 0.6–4.9 voices/s around the NPC bursts against 0.3–0.9
over the session; their GAIN × SEND levels p50 / p90: `concrete_smooth_soft` 0.026 / 0.172,
`asphalt_rough_soft` 0.002 / 0.106, `concrete_aggregate_soft` 0.012 / 0.090, `asphalt_smooth_soft` 0.000 / 0.000
(the local's hard `concrete_smooth_hard` 0.238 / 0.737). Implemented as `skaters::component_state`, and the
components now post `s.soft_wheels` where they called `sub_824B23C8` (seams w11, skid w8, contacts tier,
PlayerPhysics in12): identical for the local player (`local` is always true there).

## 3. MixMap slots, keys, attenuation
Instance g = 1 of the Player slot: SkateBoard `0x40010800`, Contacts `0x40010810`, Wheels `…820`, Rail `…830`,
Cracks `…840`, SenseOfSpeed `…880`; PlayerPhysics `0x60010800`, 3DObjPos `0x60010810` / `0x60010820`
(`mixmap::keys::*(1)`). Distance roll-off and azimuth come from the slot's own B lookups on those 3-D blocks
(no Collision / Whoosh / Dynamic slot is involved: the earlier guess is withdrawn). Cross-slot sums that read
`A[Player.n]` expand to both instances (mixmap-spec §4.2), so an active NPC instance also feeds e.g. the
Ambience sum: handled by the MixMap itself.

## 4. What the engine must publish (`NpcSkaterAudioState`)
Per NPC skater and frame, in the skater list's order (spawn order is fine; retail iterates its list):
- `id`: stable while the skater lives (retail matches records by id);
- `state`: an `AudioState` filled exactly like the local player's (`skate_events::audio_state`): speeds, COM /
  board positions and velocities, wheel contacts / materials / seam patterns / wheel world positions, grind
  family / material / impact, air, brake / manual flags, deck tilt / spin, slip, revert, feet in the deck box,
  push flags, landing data, `audio_trick` / `audio_trick_2` resolved (−1 none), `dt`. `local` and `soft_wheels`
  are overwritten (§2).
The host needs the camera (listener) and the local player's COM velocity and soft flag, which it already has.
Publishing: insert into `game_audio::npc_skaters::NpcSkaters::skaters` (a `Vec` in list order) each frame;
remove despawned skaters. Off switch `SKATE_AEMS_NPC_SKATERS=0`.

## 5. Open / next
- ~~Per-owner grain bed~~ done 2026-10-03: `Runtime::npc_grains` (no graph 3, the local bed's generator), driven by
  `grain_bed::step_with` from `NpcSkater::routed` and `skateboard(1)` outputs; dropped + stopped at the release.
  Open: what retail's released instance does with its players; the owners' pick order on the shared generator.
- Wheels / Tricks / Treatment / footsteps / clothing for the NPC instance (all per instance in retail).
- What the inactive instance's inputs hold (the bridge skips an inactive record: retail keeps the last values;
  we deactivate the 3-D blocks and release the packets).
- The manager's gate (`[0x830670B8+20]` == 5, `sys+560` == 1) and the skater list order of the AI manager.
- The NPC's speech (`sub_824BF5F8`, SkaterSpeech manager `sub_824F7D10`: one record per skater, no distance gate
  there) belongs to the speech stage.

## Board slide and the NPC bed vs the recomp (2026-10-03)
- **Board slide ported for the NPC instance:** `sub_824CB3C8` / `sub_824CB4C0` have no local test; `+780` comes from
  the conditioner per skater entry and the bridge `sub_824B0DA8` copies it, so `NpcSkater` runs `BoardSlide` with
  `NpcSkaterAudioState::loose_board` (`NpcSkaterAudio::loose_board`; ghosts take it per row from their log; mods
  `loose_board`). A scripted background recomp run (Mega-Park, 4 min standing) saw no NPC bail and so no board slide post.
- **NPC grain bed vs 180430's NPC GREC rows** (owner `40C34020`, 227 k rows; a local research tool thins to 100 ms and
  joins the NPC board by SKATEB speed; test `npc_bed_follows_the_recomp_rows`, 289 straight-roll rows): truck 0 A
  record gain recomp / ours 0–10 m 0.097 / 0.108, 10–20 m 0.045 / 0.065, 20–30 m 0.005 / 0.014; A pitch 0.93–0.97
  vs 0.96. Near band within 10 %; the far bands are louder in ours (the lookups use the distance to the local
  skater, not joined: our sim keeps the local player at the camera).

## The NPC bed's distance input and the NPC slide, settled (2026-10-04, branch `audio/world-followups`)
- **Correction: the Player slot's level lookups read the camera.** B2 (SkateBoard out1 / out2 …) and B4 read
  3DObjPos input 1 (distance) and input 3 (azimuth), per-quadrant ranges 4–50 m ahead / 4–40 m at the sides /
  1–30 m behind (B2). `sub_824AE6E0`: input 1 = |listener `+0` − emitter|, input 3 = azimuth in the frame (listener
  `+0` pulled back 0.25 m, `+32`); inputs 0 / 2 = listener `+64` / `+96`. `sub_8248CC08`: listener `+0` / `+32`
  from the camera matrix, `+64` from the skater list's first entry (the local skater). The port's `ObjPos` was
  already right; the "distance to the local skater" reading above was wrong (it holds for the ped speech lookups,
  slot 5, input 0).
- **The test placed the NPC dead ahead** (always the 4–50 m quadrant). Rows now carry their geometry (the NPC board
  interpolated, the local skater's own board (the track that follows the local SkateBoard's GREC speed) excluded,
  ambiguous speed matches dropped; camera interpolated; view = camera → local board, or the camera's motion when
  the board is > 8 m away) and `npc_bed_follows_the_recomp_rows` rolls each row at its own geometry. 176 rows:
  0–10 m 0.101 / 0.123, 10–20 m 0.050 / 0.054, 20–30 m 0.0091 / 0.0111 (recomp / ours A gain); row median ratio
  0.94 (p10 0.41, p90 1.13); behind vs ahead visible in both (10–20 m: 0.021 / 0.028 vs 0.055 / 0.065). No engine
  change.
- **NPC board slide observed** in 180430: `c_board_slide` posts at 152.13 / 152.30 s while the local rolled on four
  wheels (8.5 m/s) and instance 1's skater (board `468DC6B0`, ~15 m) was off its wheels after a bail; each post is
  followed 62–68 ms later by three `board_scrapes` voices (gains 0.005–0.012). The other 13 slide posts of the
  session are the local's. Timing vs the NPC's `+780` not comparable (not logged).
