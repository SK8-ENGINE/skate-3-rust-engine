# Traffic audio: retail spec, port state, hook points (2026-10-03)

Port: `crates/skate-audio/src/world/traffic.rs` (+ `owners.rs`, `keys.rs`). The game host is
`crates/skate-game/src/game_audio/world_sources.rs`. Harness: `crates/skate-audio/tests/world_sources.rs`.
Read from the TU3 (title update 3) recompilation with `tools/recomp-code-search/fn.sh`; image constants come from `img.py` (local tool).
This is reference only and our own code.

## Objects (one set per Traffic MixMap instance; 4 instances in free skate)
| object | vtable | process (vfunc 9) | update (vfunc 10) | packet | class / bank |
|---|---|---|---|---|---|
| SFXObj_TrafficEngine (obj 4.0) | `0x822FCBA8` | `sub_824D6110` | `sub_824D6478` | `sub_824D5B20`, 18 words | `TRAFFIC_CAR` / `C00_heavy01` … `C08_family03` |
| SFXObj_TrafficSkids (4.1) | `0x822FCC38` | `sub_824D7650` | `sub_824D76B8` | `sub_824D5D20`, 11 words | `TRAFFIC_SKID` / `Traffic_Skid` |
| SFXObj_TrafficHorn (4.2) | `0x822FCBF0` | `sub_824D6E88` | `sub_824D6F98` | `sub_824D5C30` (horn) / `sub_824D0D58` (alarm), 9 words | `TRAFFIC_HORN` / `Traffic_Horn`, `c_car_alarm` / `car_alarms` |
| SFXObj_TrafficWoosh (4.3) | n/a | n/a | n/a | n/a | No MixMap E record writes its outputs, so it is unused |

- Class handles are in the table at `0x8302EF00..` (engine `EF00`, horn `EF08`, skid `EF10`, alarm `EF20`). Their names are resolved by
  `sub_828E3250` from `0x8302D57C` and nearby entries.
- Posts go through `sub_828E2B48` (= AEMS post) and redeliveries through `sub_828E2D18`. Both run inside lock `*(0x83086E10)`.
- **Bank selection by patch.** Each `C0n` program destroys its instance unless packet w14 equals n. All 8 banks are loaded
  at once (recomp `all_20261002_164620`: READs at 8.8 s), and every retail post sounds in exactly one bank.
  - Harness `engine_banks_answer_only_their_patch`: patch n → only `C0n`. Patch 2 (the `default` record) and patch 9
    are silent because there is no C02 bank.

## Vehicle record the objects read (`[object+28]`)
| offset | meaning | used in |
|---|---|---|
| +48 | position (vec4) | engine front/rear split |
| +112 | a direction vec4 (taken as heading; **unverified**) | engine split |
| +144 | signed scalar × 3000 → engine w15 / skid w6 (**meaning unknown**; throttle or acceleration?) | |
| +148 | speed m/s | RPM, w13 / skid w4 |
| +156 | horn state: 0 none, 1–5 horn kind, 6 car alarm | horn |
| +160 | skidding flag (0/1) | skid w5 |
| +168 | `aud_traffic_engine` record key | engine, horn |

The owner context `[object+16]` holds +52 = active (byte) and +60 = frame dt.

## TrafficEngine
- **Activation** (6110: active and no packet yet):
  - Look up the record and store its patch at +48. Patch 1 becomes 7 if `rand()%100` < 33, 8 if < 66, else stays 1.
    Patch 3 becomes 6 if `rand()%100` < 50.
  - Post `[0,0,0,0,0,0,0,0,4096,25000,0,32767,0,0,clamp(patch,0,9),0,0,0]`.
  - Set rpm +52 = 0, wobble +60 = 0, direction +64 = 1, idle/max (+72/+76) from record +0/+4, and the tuning
    +80..+112 from the fields below.
- **Record `aud_traffic_engine`** (class `0x259095163B974174`; setup exports it as `world_tuning.traffic_engine`):

  | field | default | role |
  |---|---|---|
  | idle `10C7…` | 850 | |
  | max `DD02…` | 4000 | |
  | patch `C436…` (u16) | 2 | |
  | `2C15…` | 8 | wobble bound |
  | `D048…` | 4 | wobble rate |
  | `E6B3…` | 0.5 | rise |
  | `7EA0…` | 2 | fall |
  | `7FD8…` | 2000 | slew RPM/s |
  | `E67C…` | 7 | gear speed step |
  | `07CE…` (u16) | 4 | gears |
  | `763D…` | 20000 | rear bias |

  The named records (`c00_heavy01` … `c08_family03`) override only idle, max and patch.
- **Update** (6478, per audio-manager pass):
  1. TrafficEngine.in0 = trunc(clamp01(speed·0.05)·32767).
  2. Wobble += rate·dir·dt (fused), and dir flips past ±bound.
  3. The gear top is the first n·gear_speed (n = 1..gears) above the speed, or gears·gear_speed.
     Target = speed·max/top + wobble, clamped to [idle, max].
  4. Slew: step = slew·dt. If diff ≤ −step, rpm −= fall·step and w17 = +1 for 3 updates. If diff ≥ step, rpm += rise·step and
     w17 = −1 for 3 updates. Otherwise rpm = target. Then clamp to [idle, max].
  5. Words:

     | word | value |
     |---|---|
     | w0 | rpm |
     | w1 / w2 | level 3 / 4 split by dot(direction, pos − camera) × rear_bias × −1/32767 (front loses, rear gains when driving away) |
     | w3 = w4 | level 5 |
     | w5 / w6 / w7 | raw 0 / 1 / 2 |
     | w8 | pitch 6 (B0's Doppler) |
     | w9 / w10 | filters 7 / 8 |
     | w12 | level 9 (the B7 distant layer, 4–90 m) |
     | w13 | speed·2000 |
     | w15 | +144·3000 |
- MixMap: B0 = Ctl 4.1 (c 520), B1 = Ctl 4.2 (c 1557), B2 = Ctl 4.3 (c 554). A11 = B13 near boost +650 mB, gated by
  `TrafficCarPhysics.in0` (Ctl 4.0), whose writer is not traced (we write nothing, so the gate is closed).
- Harness (C01, 12 m/s, lane 8 m):
  - Pitch is 4182 approaching and 4007 receding (Doppler).
  - out3 peaks at the closest approach and is 0 beyond about 45 m.
  - RPM rises to 3434 (12·4000/14).
  - Only C01 voices sound, and voice gain peaks at 0.199 at 8 m.
  - For comparison, the recomp's C01 GAIN×SEND is 0.002 p50 / 0.049 p90 / 0.281 max, mixed over all distances.

## TrafficHorn / car alarm
- **Process:**
  - Post the horn once, with variant `rand()%9`: `[0,var,0,4096,0,25000,0,32767,0]`.
  - While horn state = 6, also post `c_car_alarm` with variant `rand()&3`: `[0,32767,32767,0,4096,25000,0,0,var]`.
    Release it when the state leaves 6.
- **Update, state 6:**
  - Horn.in0 = 0 and horn w2 = 0.
  - Alarm words: w0 = 32767, w1 = level 6, w2 = level 10, w3 = raw 5, w4 = pitch 7 (≤ 8192), w5 = filter 9.
- **Update, other states:**
  - A horn kind sounds only when the vehicle record's patch ≠ 0 (heavy01 never honks).
  - in0 = 32767 while honking.
  - Words: w0 = 32767, w2 = kind (≤ 6), w3 = pitch 2, w4 = raw 0, w5 / w6 = filters 3 / 4, w7 = level 1, w8 = level 8.
- In 164620, Traffic_Horn had 115 starts and car_alarms 12.

## TrafficSkids
- Process: post `[0,0,0,4096,0,0,0,0,25000,32767,0]` once.
- Update:

  | word | value |
  |---|---|
  | w0 | level 1 |
  | w1 | level 2 |
  | w2 | raw 0 |
  | w3 | pitch 3 (≤ 20000) |
  | w4 | speed·2000 |
  | w5 | skid flag |
  | w6 | +144·3000 |
  | w7 | filter 5 |
  | w8 | filter 4 |
  | w10 | level 6 |

  The program gates on w5. Traffic_Skid had 139 starts in 164620.

## Car alarm trigger (2026-10-04)
What sets a car alarm off (the sound itself is "TrafficHorn / car alarm" above: horn state 6). Read from the TU3
recompilation (reference only) and checked against the user's recomp session of 2026-10-04 16:11 (a taxi pulled
over; the user ran into it on foot).

**Mechanism.**
- **The vehicle's collision callback** `sub_82C3C150` (slot +32 of the contact interface at vehicle `+136`, vtable
  `0x82322218`; reached through the vtable only, no direct caller). Per contact message `m`:
  - only while vehicle `+3424` bit 0x80 is set: `StayingParked`'s begin `sub_82C39120` sets it, its end
    `sub_82C391F0` clears it (the same begin / update / end pattern as Impatience `82C38378` / `82C38390` /
    `82C38718`);
  - the length of the message's vector at `m+48` must exceed the vehicle spec's `543475921FD9E04A`
    (`livingworld_vehicle_characteristics`, record at vehicle `+3960`; `default` = 0.1, no shipped spec
    overrides it; a zero vector counts as length 0);
  - then: alarm flag `+3424` bit 0x10 on, alarm timer `+3716` = 0, parked timer `+3712` = 0. A contact while the
    alarm sounds restarts both timers.
  - No test of who the other party is: `m+76` (the other object) only feeds a "hit by" mask at `+4248`
    (`1 << type`), and `m+32` (the contact point) minus the vehicle position, dotted with a direction, sets
    `+4401` bit 0x20. Neither touches audio.
- **StayingParked's update** `sub_82C39138` (f1 = frame time): target speeds 0; with bit 0x10 the alarm timer
  `+3716` += dt and the parked timer `+3712` = 0; without it `+3716` = 0 and `+3712` += dt.
- **StopAlarming** `sub_82C3A4D0`: true when `+3716` > the spec's `E199FC7CEA222809` (`default` = 8 s, no override).
  Its action `sub_82C3B4E8` clears bit 0x10. At the console's 30 fps that is the first update past 8 s (241
  frames, ~8.03 s).
- **Pulling out** (`sub_82C3A3A8`) is false while bit 0x10 is set; otherwise it waits for `+3712` > the field `986BB0F6F043EB3D` of the
  record at `+4112` (class not identified) and a clear road. So an alarm also delays the car leaving.
- **No other sound.** The only vehicle-side post at the trigger is the alarm (`c_car_alarm`, class slot 31, caller
  `824D0DEC < 824D6F40`, the horn object's process). There is no impact or crunch sound for a parked car.

**Evidence (recomp session, 2026-10-04 16:11).**
- The taxi (VEHAUD obj `40C686E0`, key `11C6A3A54D90F447` = `c04_taxi01`) pulled over (maneuver 3) and stopped at
  (103.8, 34.0, 200.3) by 249.8 s. Its planner stops logging once parked (parked cars don't run the speed
  planner), so `+3716` itself is not in the trace.
- The player rolled up to its rear bumper at 0.2–0.3 m/s (no alarm), stepped off at 255.90 s (material 143, the
  board picked up) and ran into the car. The alarm post came at 256.032 s, ~130 ms later; the horn state went
  0 → 6 in the next VEHAUD line.
- The alarm then sounded until the session ended (car_alarms stream 7 restarted every ~2.0 s, 33 voices, 256.0 –
  318.2 s, 64 s). The board positions show the user at the car the whole time (on foot beside it, skating along
  it, deck contacts at the car's own position): every contact restarted the 8 s, as the code says.
- The user's account: the user found a taxi and skitched on it; it ended up parked at the side of the
  road, and when the user ran into it the car alarm went off.

**Answered (hook `VEHHIT`, the recomp, 2026-10-04 session with a parked taxi).**
- `m+48` is the relative velocity at the contact in m/s (the car's minus the other body's), not an impulse: a
  riderless board's deck speed and |m+48| at the same moment agree within about 0.01 m/s (0.0736 / 0.0737,
  0.3313 / 0.3335), a walking pedestrian gives values of the same size as the light board, and a body resting on
  the car gives about 0. The threshold is strict: 0.074–0.084 m/s did not restart the alarm, 0.150 did.
- Every body reaches the callback (the board carried, swung, riderless or in a bail; the skater's body; a
  pedestrian), and who hit the car is not tested: a passing pedestrian set it off too.
- The alarm ends at exactly 241 console frames (8.0333 s) in all six recorded ends, and its end restarts the parked
  time (StayingParked is left and entered again with the parked timer at 0), so an alarming car stays parked longer.
- The port's reading (a speed, `min_impact` 0.1) is right as built.

**Port.** `crate::world_audio`: `VehicleParked` (StayingParked), `VehicleImpact { vehicle, by, impact }` (the
callback message), `CarAlarmRule` / `AlarmTuning` (`min_impact`, `seconds`, `enabled`; setup export
`world_tuning.vehicle_alarm`), `VehicleAlarmStarted` (read-back: the AI restarts its parked timer and must not pull
out while the alarm sounds). `game_audio/car_alarm.rs` applies the rule; the bridge holds horn state 6 for
`AlarmTuning::hold_seconds` (whole console frames past `seconds`). Mods: event `impact {speed, source}`, traffic
option `parked`, `sdk.world_audio.alarm_rule{...}`, `read(key).alarm`.

## Waiting on the engine
1. **A vehicle system** (traffic AI on the road network, notes `npc-livingworld-re.md` §2/§6). Per vehicle it fills
   `world_sources::WorldOwners.vehicles[id]` with a `VehicleState`: position, velocity, direction, speed, load,
   horn state (honk when blocked; the alarm: send `VehicleImpact` on contacts and keep `VehicleParked` while
   parked, "Car alarm trigger" above), skid flag, and the `EngineRecord` of its model
   (`Library::world_tuning().engine("c04_taxi01")`). Which record each model uses is not traced
   (`livingworld_models` / `vehicle_characteristics` → the record key at +168).
2. Nothing else: banks load on the first published frame, and instances, inputs, posts and releases are handled.

## Open
- **2026-10-03, gap run G1 (`world-audio-hookin-spec.md` §7.3): settled** the model → record table, +112 = heading,
  +144 = acceleration (m/s²), +152 = horizontal listener distance (40 m list), the TrafficCarPhysics.in0 writer
  (`sub_824B2A28`), nearest-4 holders. The items below are superseded where they say otherwise.
- Retail's instance assignment (which 4 vehicles get the Traffic slots): ours is nearest-4, provisional (`owners.rs`).
- Where the 3DObjPos blocks 4.1 / 4.2 / 4.3 point (body / engine / exhaust?): find the binder that stores into
  `[pos+32]` for traffic. We put all three at the body.
- Vehicle +112 (heading or velocity?), +144 (what), the TrafficCarPhysics.in0 writer.
- `rand()` = `sub_82A8AF10` (CRT): its generator is not identified; ours is an LCG behind `world::Draw`.
- Process/update order in our host: update first, then process. So inputs are one console frame old; move the
  calls into `native::mixmap_frame` once a system exists.
- Validation still to do: compare per-voice gain vs distance with VEHSTATE positions. This needs a camera position
  in the trace: hook the listener (`*(0x830CFDD4)`) or use PEDSEE targets with care. The `speech_nearfar` test
  showed those joins are unreliable.
