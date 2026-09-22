# Engine defects found while porting player audio (as of 2026-09-20)

These are **engine/physics/gameplay** defects, not audio-port gaps. Porting the audio exactly made
them visible, because retail's audio reads native physics fields directly and therefore only sounds
right when those fields carry retail's values.

Each entry says how it was found, how confident the diagnosis is, and what would prove it fixed.
Confidence is deliberately separated from severity: some of these are measured, others were seen
once in a playtest and have no repro yet.

---

## 1. Jump height and the landing's weight — **resolved by measurement; no workaround left**

**History, because two wrong turns are recorded in the git log.** An earlier session measured
`jump_height_200` at 0.10–0.13 m against a retail capture's 1.1–2.2 m and concluded the field was
~10x short, so a 10x was applied to the copy handed to audio. On 2026-09-20 that was moved onto the
real launch (`ground_jump`'s `remaining` term) on the theory that the jump itself was short; the
owner playtested it and went into orbit, so the launch was ruled out and the change reverted.
`ground_jump` is untouched and retail-exact.

**What the trace actually shows.** Measuring Class_Treatment word 9 directly from a playtest
(`SKATE_AUDIO_TRACE`, 27541 updates, 1559 with a live landing):

| | word 9 (`trunc(clamp(h × 166.667, 0, 1000))`) |
|---|---|
| Retail | 183–367 |
| This engine **with** the 10x | min 62, **p50 1000, p90 1000** — pegged at the clamp |

So the 10x drove word 9 3–5x past retail and into saturation: every landing played at maximum
weight regardless of the hop, which is what the owner heard as "landings too loud for the height".
Back-solving the clamp puts the *reported* height at **≥0.6 m**, not the 0.10–0.13 m the original
measurement claimed — so the field is far less wrong than believed, and word 9 without any scale
falls around 100–300, inside retail's range.

**Resolution.** The scale is removed entirely. No workaround remains in the tree.

**Residual, low priority.** Reported height (~0.6 m) still sits below retail's 1.1–2.2 m plateau,
so word 9 may run slightly low on big drops. If that ever matters, the thing to check is that
`max_y` tracks the deck rigid body (`air_phase.rs:202-206`, `part_transforms()[6]`) while `start_y`
is a wheel-centre ground reference — a constant offset between two different references is
subtracted out of every hop. PhysicsAir and KnownAir also disagree on that reference: `+500`
(current, `air_phase/input.rs:49`) versus `+480` (previous, `known_air.rs:94`). **Do not "fix" this
at the launch** — that was tried and it sends the skater into orbit.

## 2. The engine rarely enters KnownAir — **diagnosed from code + capture**

**Symptom.** Originally: no landing impact at all. Every per-wheel landing bucket stayed 0.

**Evidence.** Retail is in KnownAir (state 201) for essentially every hop. This engine selects it
only while Processed2468 bit 10 (trajectory valid) is published, and nothing publishes that bit
except transiently, so ordinary ollies run in PhysicsAir (200/202). PhysicsAir publishes nothing
for Air+176 and a constant 4.0 for Air+184, so audio state `+236` stayed 0 and with it `+244` →
`+448`. Documented at `crates/skate-game/src/physics/audio_observation.rs:182-207`.

**Current workaround.** Audio reads the owning state's own timer/prediction instead
(`audio_observation::air_timing`). This restored landing impacts, but it is audio-side
compensation for a physics-side defect — anything else that reads air timing still gets the wrong
values.

**Fixed when.** An ordinary ollie runs in state 201, and `air_timing` can be deleted in favour of
reading Air+176/+184 directly.

### Retail's actual ollie path, recovered 2026-09-20 — and why the premise now needs re-measuring

**Retail does not go 100 → 201.** An ordinary ollie runs **100 → 103 (GroundAnimation) → 201**, and
the trajectory is launched *and completed in the same frame* inside GroundAnimation's
`sub_82D33E30`, gated on `Toolkit_CalcGroundJump` (`sub_82D93618`) returning `JumpInfo+20 != 0`:

```
86.cpp:6996   bl 0x82d93618      Toolkit_CalcGroundJump
86.cpp:7022   beq cr6,...        not active -> ordinary ground forces
86.cpp:7080   bl 0x82d67848      LAUNCH
86.cpp:7085   bl 0x82d68800      COMMIT, same frame -> selector+9658 valid = 1
```

Then PostInput (`sub_82DB5588`) calls `sub_82D68800` again — nothing is pending, so it early-outs
and returns the **retained** `valid` — and stamps bit 0x400:
`91.cpp:62477 rlwimi r4,r5,10,21,21`. The 103 arm of the selector reads `+2572 == 1` and yields
`200 | bit` = 201 (`90.cpp:13258` → `90.cpp:14056`). `valid` is sticky: only `sub_82D67848`
(launch) and `sub_82D67228` (reset) clear it. PhysicsGround launches **only** on the animated-board
branch (`86.cpp:19001 lbz r8,2708(r31)`), which is a contact condition, not the ollie.

**This engine already matches all of that.** `ground_animation/board.rs:71-104` launches on
`jump.active` and calls `trajectory.update` on the very next lines, and `Trajectory::launch` fills
`pending_results` synchronously (`air_trajectory/mod.rs:50-54`), so the completion really does
happen in-frame. `complete_batch` sets `self.valid = true` at `selector.rs:278` **before** it
starts any second pass, and `launch_pass` does not clear it — so a pending second pass does not
take `valid` away. Both selector arms (`selector/ground.rs:33-35` for 100, `:107-109` for 103)
match retail's `field_2572 == 1 → air_variant_from_2468()`.

**So the "nothing publishes bit 10 except transiently" premise is not supported by the code as it
now stands, and it has not been re-measured since the workaround was written.** Note also that
rolling off an edge *correctly* yields PhysicsAir in both retail and here — `selector/ground.rs:42`
and `:110` hard-code it, exactly as retail does — so a roll-off test proves nothing about ollies,
and `tests/air_playback.rs` only ever asserts `category() == 200`, which 200/201/202 all satisfy.

**Do this before changing any physics.** Play, ollie, and read the new `st=` field:
`grep -o "AS st=[0-9]*" <trace> | sort | uniq -c`. If 201 appears on hops, this defect is stale
like #3 and #7 were and the only work left is deleting the `air_timing` workaround. If it never
does, the thing to instrument next is `jump.active` and the selector's `valid` at the moment
PostInput publishes, since every other link above is confirmed present.

## 3. Powerslide squeaks — **the "never fires" premise was wrong (corrected 2026-09-20)**

**Symptom as originally recorded.** No powerslide squeak sound, ever.

**That premise does not survive the traces.** Retail's gate is: both feet inside the deck box
(`+615`/`+616`), more than one wheel down, and `|deck tilt +264| × 114.5916 ≥ 15` (the threshold is
vault-loaded, `Hash_A129B33B4A2C7961`). Counting over the existing `logs/audio-trace-*.log`, the
gate passes on thousands of frames per session and `Class_Squeaks` **is posted**: 85 posts in
`audio-trace-20260920-122739.log`, 70 in `...-111039.log`, with thousands of `UP` redeliveries.
`|tilt264|` reaches 0.38–0.64 rad, comfortably past retail's 0.14–0.28 band, so the tilt input is
not short either. The gate and its four inputs are fine.

**What is actually unknown.** `tilt264` is the *steering* tilt (an exponential blend in
`TruckSteeringState::update`), not a slide-specific angle, so a passing gate does not prove the
squeak fired *during* PhysicsSlideGround. The `AS` trace line carried no state id, so the logs
could not answer it. That is now fixed: `AS` carries `st=` (State+16) and `cat=` (State+12) as of
this commit.

**How to finish it.** Play, powerslide, and filter the new trace for `st=101`:
`grep "AS st=101" <trace>` — then check whether those frames pass the gate and whether a
`Class_Squeaks` `PO` sits on the same frame number. If they do, the squeak is firing and the
remaining question is its rendered level (the posted speed word runs as low as 19/1000 at slide
onset while the turn word saturates at 1000); if they do not, compare the four fields on those
frames against the retail capture's `state.tsv`.

**Note for whoever picks this up:** the `DirectMixer` block in `player_audio.rs` also references
`Brd_Squeaks.abk` on a different gate, but it is dead code — the struct is never constructed. It
is not a confound.

## 4. Bail / ragdoll native outputs — **was a gap; the outputs now exist, unverified by ear**

This entry used to say the bail families were silent for want of engine outputs. That is no longer
true, and the text stood long after the ports landed. What is actually there, re-checked
2026-09-21:

- **Ragdoll contacts reach the audio state.** `sub_82BD60C8`'s per-region work is ported at
  `crates/skate-game/src/physics/audio_observation.rs:135` (`body_contacts`), `sub_82773298` at
  `player_audio/audio_state.rs:286` (`Conditioner::body_contacts`), and words **496–611** are
  stored every frame at `audio_state.rs:798-806`.
- **The speeds and flags exist**: `+328` (`audio_state.rs:822`), `+672` (`:797`), `+676`/`+677`
  (`:830-831`), `+780` loose board (`:853`).
- **The consumers are ported and registered.** `c_body_slide` and `c_cloth_falls` are in
  `player_audio/components/clothing.rs`, wrapped on the `Clothing` component and registered at
  `player_audio/sound.rs:454`. There is no env gate.
- **The surface tag reaches them.** A contact's material rides word 23 of the narrow-phase row and
  word 55 of the compiled one (`solver/contact_build/publication.rs:72-83`), which is what
  `contact_feedback/spy.rs:43` reads; the 23→55 move is asserted whole-row by
  `crates/skate-core/tests/physics_rebuild_contact_build.rs:63,83`. Both real map sources supply a
  non-zero surface (`skate_world.rs:172` for RWCM, `:336` for portable `.skate`).

**What is genuinely unresolved.** Nobody has heard a bail and checked it, and there was no way to:
none of these fields appeared in any trace. The `BC` line added 2026-09-21 carries them, so the
next playtest can answer the open questions instead of re-deriving them:

1. Does `mat560` ever resolve? It is zero on the built-in test terrain (`physics/ground.rs:199`
   authors material 0) and legitimately zero for body-vs-body contacts
   (`solve/assembly_contacts.rs:108`), so an all-zero reading on a **stock map** would be the real
   defect, and every body slide would take `body_contact`'s default type 2.
2. Is `+328` right? The layout table (`player-audio-retail-drivers.md:74`) fixes the source as
   `|Skeleton+288|` and the port reads it, but whether that offset is the part's *angular* or
   *linear* velocity is not settled from the lifted C++. It drives `c_body_slide` w10 and
   `c_cloth_falls` w3 directly, so it is worth settling before tuning either by ear.

**Fixed when.** A playtest `BC` trace on a stock map shows `mat560` resolving, and the owner has
A/B'd a bail against retail.

## 5. Surfaces are not implemented — **known, deliberate**

Everything runs on default surface 2 (concrete_rough). Retail routes rolling grains, seam patterns,
wheel-pop sample categories, grind timbre and skid sounds per surface. Until surfaces exist, all of
those are correct-but-monotonous: the right sound for concrete, everywhere, including on wood,
metal and grass.

**Fixed when.** The engine publishes real per-wheel audio materials and the map's seam pattern
(`surface_tag >> 12 & 0xF`), so the already-ported per-surface tables get exercised.

## 6. Physics NaN torque crash on landing — **seen once, no repro**

A playtest crashed on a landing with a NaN torque in the physics solver. It is unrelated to audio
and has not been reproduced since; no current log carries it. Listed so it is not forgotten, but it
needs a repro before it can be chased. Treat the diagnosis as unconfirmed.

## 7. Broadphase disagrees with a full scan — **resolved 2026-09-20: the test was wrong**

The engine is correct; the oracle was not. `tiled()` lays all 1024 fixture faces in the plane
`y=0` and the probe volumes sit at `y=0.25`. The recovered candidate producer `82ACEA30` offers
only the triangle's own face normal for point/segment/triangle volumes — no edge or vertex axes —
so a coplanar tile 10 km away in x still projects to a ~0.05 separation and survives `82ACE968`'s
`separation > fat + limit` gate. The linear reference therefore handed the narrow phase 1024 tiles
it would never see natively and collected 1025 contacts for a sphere that touches two triangles;
the hierarchy returned the correct 2. The mismatch only surfaced once `capacity = 100` truncated
the bogus set, which is why it looked like a retention bug.

That is native behaviour, not a defect: the native narrow phase is only ever reached through a
broadphase that has already rejected laterally, and **every in-game `BoardWorld` is built with
`with_query_metadata`**, so the per-triangle bounds cull is always active. `BoardWorld::new`
without metadata is test-only. Staggering the distant tiles in Y — separating them along the one
axis the SAT does test — makes the full scan a valid oracle again; linear and accelerated then
agree at 2/4/8/6 contacts for the sphere, capsule, rounded box and triangle. Fixed in the commit
that added this paragraph; 606 `skate-core` tests pass.

*Original entry, kept because the wrong diagnosis is instructive:*

`physics::board_world::broadphase_tests::predictive_contacts_and_retention_match_full_scan_for_every_primitive`
(`crates/skate-core/src/physics/board_world/tests.rs:232`) fails deterministically:
`world.query_primitives(..)` returns a different contact set from the linear reference scan
`linear.query_primitives(..)` for some primitive/capacity/`deferred_reduction` combination.

Repro: `cargo test -p skate-core --lib --locked predictive_contacts_and_retention`

Confirmed pre-existing as of commit `330a3e9` — it is not a consequence of the pop-height change.
It matters because the broadphase is what feeds collision to the whole board simulation, so a
disagreement with the reference scan is a correctness bug in contact generation, not just a
failing test. The assertion dump is large; the first divergence is in the retention records, so
start by narrowing which `capacity` / `deferred_reduction` pair diverges.

## 8. Renderer crash on surface teardown — **seen once, did not reproduce**

`wgpu_hal::vulkan` panics with `Trying to destroy a SurfaceAcquireSemaphores that is still in use
by a SurfaceTexture` (`instance.rs:194`), taking the process down with exit code 0xC0000409.
Observed once at +24.8 s on 2026-09-20 (crash report
`report-1789928798791616000-15424.txt`); an immediate relaunch of the same binary ran fine, and the
four launches before it never hit it. Audio was up and healthy at the time (`player_sound ready`,
241 Treatment packets traced), so it is a renderer-side race on surface destruction, not audio.
Intermittent, so it needs repeated launches or a GPU-validation run to pin down.

## 9. Rail / grind landing sounds are not driven — **known gap, needs engine inputs**

Landing *onto* a rail makes no grind-onset sound. `ContactSound::GrindOnset` and
`ContactSound::PopRoll` are deliberately dropped in `player_audio/contact_voices.rs` rather than
played from an invented sample: retail routes them through a different subsystem — the
contact-sound manager at `[manager+668]`, fed by `sub_82496C58`'s per-material level interpolation
and `sub_82486EF0`'s 48-byte message. That path is also what carries retail's *continuous*
per-material impact level (see `docs/player-audio-retail-drivers.md` §9), so porting it fixes rail
landings and adds material-dependent landing weight at the same time. `sub_82486EF0`'s sink was
traced as far as `[g[0x830CFDC4]+668]->vfunc12`; the handler beyond that is not yet followed.

### Decoded 2026-09-20 — everything except the sink's own dispatch

**Why a rail landing is silent specifically.** `Contacts::step` suppresses the ordinary landing
voice during a grind (`contacts.rs:708`, `else if !airborne && !grinding`), so the grind onset is
the *only* sound retail plays for that event — and it is the one that is dropped. The two
suppressions compound into silence. Retail has no such gap because its onset always posts:
`sub_824BB0E0` has no zero-level guard, unlike the landing `sub_824BA630`, which skips its message
when either level is 0 (`11.cpp:26040-26050`).

**The game-side chain is already complete and correct**: contact geometry → `impact_speed_128` →
`AudioState.grind_impact_228` / `grind_material_692` / `grind_family_192` →
`ContactsOwner::grind_onset` → `VoiceRequest { material, family_base, tier }` →
`ContactVoices::play(GrindOnset, ..)`. Only the sink is missing.

**The message is fully decoded.** `sub_82486EF0` allocates 48 bytes and writes: `+0x00` material A
(the family base, 95 or 96), `+0x04` material B (the grind material, 143→10), `+0x08`/`+0x0C` the
impact tier, `+0x10` a `vec4` world position copied wholesale (from audio state `+48` for the grind
onset, `+144` for a landing), `+0x20`/`+0x24` the two 0…32767 levels, and three flag bytes at
`+0x28`/`+0x29`/`+0x2A`. Delivery is two-stage: `[manager+668]->vtable[+12](msg)` returns another
object and the message is handed to *its* `vtable[+12]` too.

**The levels are decoded too.** `sub_82496C58` returns
`low + (high - low) * (min(value, hi) - lo) / (hi - lo)`, truncated, with `(low, high)` chosen by a
(mode × material-category) table and defaulting to `(0, 32767)`; it returns 0 for a negative
material or mode 3. The tier selects which impact-speed window the interpolation runs over —
`[0.0, 0.25]` for tier 0 and `[0.25, 0.5]` for tier 1, both vault floats
(`086B66C3D4FFEE8F`, `B2ACAFDBCD963C93`) on the grind-material class. The Rust `VoiceRequest`
carries the tier but not those two bounds.

**What is still unknown is only the dispatch behind `vtable[+12]`** — i.e. how the material pair
picks the actual sound. Nothing in `sub_82486EF0` or `sub_824BB0E0` names a bank or a sample, which
is why `contact_voices.rs` is right to refuse to invent one. **Porting the onset means porting the
contact-sound manager, not adding an arm to `ContactVoicePlayer::selection`.**

### The sink is pinned to three words of `.data`/`.rdata` (2026-09-20)

The manager global `0x830CFDC4` is written in exactly one place, `sub_826D5500` (`28.cpp:52333`):
a 2192-byte object allocated and constructed by `sub_824845B8`. There is **no `stw rX,668`
anywhere**, because `+668` is not a named field: the constructor zero-fills a **14-entry pointer
array at `+656..+708`** (`9.cpp:44075-44088`), and `sub_82484FE8` (`9.cpp:45097`) fills it with
`modules[i] = sub_828DED90(i)`. So **the sink is `manager->modules[3]`** — array slot 3.

`sub_828DED90` (`46.cpp:57277`) is a type-id registry lookup over a vector at `0x830BBE00` of
12-byte descriptors (`+0` id, `+4` pool, `+8` create fn). The 14 descriptors are registered by
`sub_8248D2A0` (`9.cpp:64145`), whose addresses, create functions, object sizes and vtable
addresses are all recovered. **But the descriptors' id words live in `.data`, and the vtables'
contents live in `.rdata`, and the recompiler lifts only `.text`** — so neither "which class is
slot 3" nor "what function is at its `vtable[+12]`" is derivable from `generated/`.

Also corrected: **`sub_828AAF88` is not a fallback sink.** It returns the default
`EA::Allocator::ICoreAllocator` (`45.cpp:7080`), and the `loc_82486FBC` tail is
`GetDefaultAllocator()->Free(msg, 0)` — the null-sink path *destroys* the message. Model it as
"drop the event", not as a second delivery route. (Its twin `sub_828AAE90` is what allocated the
48 bytes, which is how the `vt[8]=Alloc` / `vt[12]=Free` slots were confirmed.)

**Three reads close this**, against a decrypted retail image at base `0x82000000`:
1. the id word at `0x8302CD1C` and at `0x8302CD80 + 12n` for n=0..12 — find the one equal to 3;
2. that descriptor's `+8` create fn, cross-checked against the recovered table, which names the
   class (each create fn passes a distinct allocation-tag string, so the class name is readable);
3. the word at `<that class's vtable> + 12` — the concrete `vtable[+12]`, which can then be pulled
   out of `generated/` with the usual awk and read directly.

**Getting a decrypted image is the blocker, and it is not hard in principle** — the retail
`default.xex` at `out/build/windows-release/game/` is `encryption=0001, compression=0002` (AES +
LZX), and the SDK ships `rex/system/xex_module.cpp` and `lzx.cpp` that do exactly this at runtime.
Note that `rexglue.exe dump-xex` **looks like a stub in the SDK build on this machine**: it exits 0
in 0.2 s and writes nothing, at any log level. Either fix that subcommand, add a tiny host that
calls the SDK's `XexModule` loader and writes the image out, or read the words from the running
recomp's memory.

### The sink is no longer blocked: the image dumps, and the chain resolves (2026-09-20, later)

`rexglue.exe dump-xex` is **not** a stub after all. Its own `_putenv_s` does not reach the loader,
but the loader reads `REXGLUE_DUMP_XEX_IMAGE_DIR` from the environment, so setting that variable
*externally* dumps the decrypted image:

```
cd C:\dev\skate3recomp\outuild\windows-release\game
REXGLUE_DUMP_XEX_IMAGE_DIR=<out> rexglue.exe dump-xex default.xex <out>
```

That writes `default_82000000_011B0000.bin` (18,546,688 bytes, base `0x82000000`), i.e. `.data`
and `.rdata` included. File offset = address − `0x82000000`.

With it, the three reads are done:

1. **Slot 3 is the FIRST descriptor**, `0x8302CD1C`: id 3, name pointer `0x82247544`, create fn
   `sub_824F16D0`. (An earlier inferred bijection guessed reg pos 4 — it was wrong. Read the ids,
   do not derive them.) Full table, descriptor → id: CD1C→3, CD80→10, CD8C→2, CD98→0, CDA4→1,
   CDB0→4, CDBC→5, CDC8→6, CDD4→7, CDE0→8, CDEC→9, CDF8→11, CE04→12, CE10→13.
2. **The class is `CSTATEMGR_Collision`** (the name string at `0x82247544`).
3. **Its vtable is `0x822FD4FC`**, read from the create function itself
   (`lis r9,-32208; addi r8,r9,-11012; stw r8,0(r3)` = `0x82300000 − 0x2B04`) rather than from an
   inherited table, which had it 0x10000 too high. Slots: +0 `824F1B70`, +4 `828DE848`,
   +8 `824F17B8`, **+12 `824F1818`**, +16 `828DEE58`, +20 `828DEED8`, +24 `828DEF38`,
   +28 `824F16B0`, +32 `824F16C0`. (**Correction 2026-09-20:** an earlier revision of this section
   listed a tenth slot `+36 824F88A8`. That was an over-read — the manager vtable ends at `+32`,
   and `0x822FD520` is already the *next* class's vtable. See below.)

**`sub_824F1818` (hop 1) is a voice-slot router, not the voice starter.** It walks the linked list
at `[this+16]` through `[node+4]`, keeps the node with the lowest signed `[node+64]` (a priority or
age), calls `[node->vtable+28](node)` on the winner and returns it. `sub_82486EF0` then delivers
the 48-byte message to *that node's* `vtable[+12]` — hop 2, which is where the material pair and
the two levels finally choose a sound.

### The chain is closed: the sink is a state machine, and the sound choice is a table (2026-09-20)

`[manager+16]` is populated, the node class is read, and the chain runs all the way to the material
→ sound-index tables. **There is no single "handler" — the sink is a generic data-driven audio
state machine, and what the material pair and the levels actually pick is a small set of integer
indices plus two controller parameters.** Every address below was read from the dumped image or
the lifted asm, not inferred.

**There are two parallel registries, not one.** The one already documented (12-byte descriptors,
vector at `0x830BBE00`, looked up by `sub_828DED90`) holds the 14 `CSTATEMGR_*` managers. Two more
sit beside it: 16-byte descriptors at `0x8302CE1C + 16n` (vector `0x830BBE20`, pushed by
`sub_828DE8C0`) holding the 14 matching `CSTATE_*` **state** classes, and descriptors at
`0x8302CF70…0x8302D3A0` (vector `0x830BBE10`, pushed by `sub_828DE928`, 46 of them) holding the
`SFXObj_*` / `SFXCTL_*` **component** classes. Descriptor word 0 packs `category<<16 | kind<<4 |
ctor-arg`; `+4` is the class-name string, `+12` the create function. Dumping all three tables gives
the whole audio object model by name — `SFXObj_Contacts`, `SFXObj_Wheels`, `SFXObj_Rail`,
`SFXObj_Treatments`, `SFXObj_Tricks`, `SFXObj_OffBoard` and so on are all in there, which is worth
revisiting for defects 4 and 5. **All three tables are written out in
`docs/audio-object-registry.md`.**

**`[manager+12]` is the manager's own registry id, read not derived.** `sub_828DED90` calls
`obj->vtable[+4](obj, id)` right after construction, and `CSTATEMGR_Collision::vtable[+4]` is
`sub_828DE848`, whose entire body is `stw r4,12(r3)`. So `[manager+12] = 3`, and 3 is what selects
every `*_Collision` class out of the two other registries.

**The nodes on `[manager+16]` are `CSTATE_Collision`.** `CSTATEMGR_Collision::vtable[+8]`
(`sub_824F17B8`) loops **exactly 10 times** calling `sub_828DEC20(manager)`, then sets `[mgr+24]=1`.
`sub_828DEC20` picks the descriptor whose category equals `[manager+12]` — descriptor `0x8302CE1C`,
name `CSTATE_Collision`, create fn `sub_824F8808` — makes the node, and links it: `[node+12]=manager`,
`[node+16]=[manager+20]` (its index), `[node+20]=[manager+12]`, `[node+24]=0`, appended through
`[node+4]` (next) / `[node+8]` (prev) with the head at `[manager+16]`, and `[manager+20]++`.
So the list is **10 LRU voice slots**, built once at manager init.

**`CSTATE_Collision` is 80 bytes, vtable `0x822FD520`** (from `sub_824F8808`'s own
`lis r9,-32208; addi r8,r9,-10976`). Slots: +0 `824F88A8`, +4 `828DF098`, +8 `82B61BB8` (a bare
`blr`), **+12 `824F8990`**, +16 `828DF4C8`, +20 `824F8B50`, +24 `828DF518`, +28 `824F8B58`,
+32 `828DF648`, +36 `824F87E8`, +40 `824F87F8`.

**Hop 2 is `sub_824F8990`, and it is five instructions.** It does *not* choose a sound:

```
[node+64] = [[0x830CFD94] + 16]     ; a global frame/time stamp
[node+68] = msg                     ; park the 48-byte message on the slot
tail -> sub_828DF6F0(node, msg)     ; activate the state
```

That closes the loop on the router: `[node+64]` is a **timestamp**, so `sub_824F1818` picking the
lowest signed `[node+64]` is picking the **least-recently-used** of the 10 slots. Before returning
the winner it calls `vtable[+28]` (`sub_824F8B58`), which frees that slot's previous message through
the default allocator and nulls `[node+68]` — i.e. it *evicts* before reuse.

**Activation reaches exactly one component.** `sub_828DF6F0` sets `[node+28]=msg`, `[node+52]=1`
(active), walks the `[node+32]` child list calling `vtable[+28]`, then calls
`[node->vtable+16]` = `sub_828DF4C8`, which walks the `[node+36]` child list calling `vtable[+24]`.
The children come from `sub_828DF098(state, mask)` — called with **mask = 1** by `sub_824F17B8` — so
bit 0 only: one child of kind 0, built by `sub_828DEA00(manager, index, 0)` and appended to
`[state+36]`. For category 3 / kind 0 that descriptor is `0x8302D160`, name **`SFXObj_Collision`**,
create fn `sub_824D1B48`.

**`SFXObj_Collision` is 104 bytes, vtable `0x822FC890`** (from ctor `sub_824D1BD8`:
`lis r9,-32208; addi r6,r9,-14192`). Its tail is **two 32-byte voice records** at `+40` and `+72`,
each initialised `{0, 0, 4096, 0, 0, -1, -1, -1}`. `sub_824F1468` (vt+20) binds the owner:
`[child+16] = [child+32] = state`.

Its `vtable[+24]` is `sub_82E1F0A8`, a thunk straight to `vtable[+28]` = **`sub_824D1DB8`**, which
is what actually receives the activation:

```
[[this+12]+12]->+60 = 1
msg = [ [this+32] + 68 ]            ; the message off the owning state
[this+36] = msg
tail -> [this+28]->vtable[+56]( [this+28], msg+16 )   ; msg+16 is the vec4 world position
```

**The material pair picks a sound in `sub_824D2318`** (`SFXObj_Collision::vtable[+40]`, the voice
starter, reached from the state update). It runs both material words through two resolvers and
drives the two voice records:

- **`sub_824D20E8(this, material)`** — if `material >= 143` the category is forced to 8; otherwise
  `cat = sub_82496FD0(material)`, *the same material-category function `sub_82496C58` uses for the
  levels*. Then a jump table at `0x824D212C` maps **cat → index: 0→13, 1→14, 2→15, 3→16, 4→17,
  5→18, 6→12, 7→19, 8→20, 9→21**, and any `cat > 9` also lands on 20. The index goes to
  `this->vtable[+60]` (`sub_824AF240`).
- **`sub_824D22B8(this, material)`** — `cat == 9` (or `material >= 143`) → **22**, everything else
  → **1**. That index goes to `this->vtable[+56]` (`sub_824C5910`).

So each of the two materials yields a pair of small integer ids, and those four ids (locals at
`r1+80…92`) are what the voice records are started from, together with float constants at
`0x824D0664`, `0x824D078C`, `0x824D1664` and `0x8232520C`.

**The continuous landing weight is `sub_824D1E00`** (`vtable[+36]`, the per-frame update called from
`sub_828DF568`). This is the piece §9 of `player-audio-retail-drivers.md` wanted, and it is tiny:

```
param0 = 0 ; param1 = 0                        ; via [this+12]->vtable[+8](index, value)
if ![state+52]                       -> done   ; state not active
if [this+40] == 0 && [this+72] == 0  -> done   ; both voice records idle
param0 = 32767                                 ; gate on
sel = msg[+0x08], msg[+0x0C] with 3 as a "none" sentinel:
        msg[+0x08] == 3 -> sel = msg[+0x0C]
        msg[+0x0C] == 3 -> sel = msg[+0x08]
        otherwise       -> sel = max(msg[+0x08], msg[+0x0C])
param1 = 20000 if sel == 1, 32767 if sel == 2, else 10000   ; clamped to [0, 32767]
```

`[this+12]` is the controller block and `vtable[+8]` is `set(index, 0…32767)`. Note both parameters
are **rewritten from zero every frame**, so this is a continuously driven pair, not a one-shot.

**What this means for the port.** `ContactSound::GrindOnset` / `PopRoll` do not need an invented
sample: they need the ten-slot LRU, the message park, and the two controller parameters above, with
the sound identity coming from the cat→index tables rather than from anything in `contact_voices.rs`.
The remaining unread piece is the body of `sub_824D2318` past the resolver calls — the float math
that turns the four indices plus the two 0…32767 levels into the two voice records — and
`sub_824AF240` / `sub_824C5910`, which turn an index into an actual bank entry.

### The control plane is ported (2026-09-20, later still)

`player_audio/collision_states.rs` + `collision_materials.rs` now hold the manager, and
`contact_voices.rs` posts the grind onset to it instead of dropping it. 10 unit tests, plus an
`--ignored` test that resolves all 143 material categories against the owner's real vault.
What the port covers, and the three things the port itself established:

**The indices are MixMap controller outputs, not bank entries.** `SFXObj_Collision::vtable[+60]`
(`sub_824AF240`) and `vtable[+56]` (`sub_824C5910`) are the *same* two accessors the other
components already use — `[[this+12]+12]` indexed as packed 16-bit words, the first masked to 15
bits, the second sign-extended and scaled by `0x822F889C` = **4096.0**. So `sub_824D20E8` /
`sub_824D22B8` choose *which output id* a material reads, nothing more.

That predicts the ten Collision controllers are `0x40030000 + 0x800·slot`, because
`sub_828DEA00` packs a component key as `0x40000000 | category<<16 | index<<11 | kind<<4`.
**The retail MixMap confirms it exactly** (`cargo run -p skate-audio-core --example mixmap_dump`):
ten `SFXObj_Collision` controllers, `40030000` … `40034800`, whose outputs run `0..=22` with
**precisely 1 and 22 typed `t1`** — the signed/scaled type `sub_824C5910` reads — and the rest
`t0`. An independent source agreeing with the vtable walk end to end.

**The material category is a vault lookup, not a table.** `sub_82496FD0(material)` reads the
material's 64-bit AttribSys key from a 16-byte-stride table at `0x8302D6E8` (`+0` the material's
collision sound id, `-1` = silent; `+8` the key), looks the record up under class
`D40CB4C0FFE45676`, and returns the field `D5EF686287A57AFE` — retail type
`Sk8::Audio::eMaterialNicotineType`, exactly ten values, which is what the `0..=9` jump table
indexes. Over the owner's vault the histogram is `[12, 49, 2, 7, 11, 11, 3, 9, 34, 5]` = 143, so
every arm of the table is reachable from real data. Only material **94** has no record — its key
is all zeroes and its sound id is the table's only `-1`, the same slot `SurfaceMap` clamps to.
(Worth noting: the `skater-collections.json` export is missing five further material records that
the vault itself has. Read the vault, not the export.)

**Correction to the `grind_gate_124` note below: it is not a cooldown.** Across every lifted
function, `+124` on `SFXObj_Contacts` is written in exactly two places — zeroed by the constructor
`sub_824B7CA8`, and set to `0.5` (`0x8209975C`) at `sub_824BB0E0`'s tail. **Nothing ever clears
it**, including `sub_824BB330` and the `sub_82489058` reset (which only touches `[p+0]`). Read
literally, retail plays the grind onset at most *once per component lifetime*. That is odd enough
that it is more likely a clear exists through a pointer the lifter obscures, so the port
deliberately **does not write the gate**: implementing it as written would silence the very sound
this work is adding. The read at `contacts.rs:667` stays harmless while the field stays 0.

### The sample chooser is ported, and a rail grind resolves (2026-09-20, last)

`sub_824965D0` -> `sub_824967F8` is a **table, not code**, and the last piece needed to read it was
the AttribSys *class layout*, which `skaterschema.vlt` carries as a per-field offset
(`tools/asset_pipeline/vlt.py`'s `fkey, typ, offset, ...`). With it, every offset the lifted code
uses resolves to a named field:

* The image table at `0x8302D6E8` gives each material a **kind word** (`+0`) and its record key
  (`+8`). The kind selects which *family* of fields the sample comes from -- and an AttribSys
  field's retail **type name is its bank**: `Skate_Collisions` -> `Skate_Collisions.bnk`,
  `Skate_Metal` -> `Skate_Metal.bnk`, `HOM_Set_1` -> `HOM_Set_1.bnk`. All three are in a stock
  `audiofiles.big`. Kind 0 covers 88 materials, kind 1 (metal) 49, kind 2 five (102..=106), and
  `-1` the single silent material 94.
* Within a family, `(tier, the paired material's class)` picks one of seven fields -- `tier == 2`
  ignores the pair; `tier == 0` and everything else take one of three by class.
* The paired class is `sub_82497910`: materials 95..=113 answer from a jump table at `0x82497944`
  (96 -> 2, `98/103/107/108/109` -> 0, the rest -> 1, and 110..=112 fall through), and everything
  else reads the `AudioSurfaceMap` word at `+28`, whose out-of-range clamp to element 94 is the one
  `SurfaceMap::lookup` already implements.

**Kind 2 is the tell that this reading is right.** It is the only family retail looks up *by hash*
(`sub_82B72420`) instead of by offset -- and the class layout says exactly why: those are the only
fields with no static offset at all. Its one hole, `tier == 0` against class 0, goes through
`sub_824825D0`, an accessor that is not decoded, so the port leaves that combination silent rather
than guessing.

**What a rail grind now resolves to**, measured against the owner's vault:

```
family 95: Skate_Collisions.bnk #879 level 28000  /  Skate_Metal.bnk #399 level 20000
family 96: Skate_Collisions.bnk #883 level 27000  /  Skate_Metal.bnk #401 level 20000
```

Two voices, one per material, each chosen against the *other* material's class -- the board's
family base out of the collisions bank and the rail out of the metal bank. That is why a rail
grind was never going to be one sound.

`Skate_Metal.bnk` and `HOM_Set_1.bnk` are now in `OPTIONAL_SPLICE_BANKS`. They need ffmpeg to
decode, which is not present everywhere, so a decode failure on an *optional* bank now logs and
skips instead of taking the whole player-sound path down -- without that, listing them turned a
missing ffmpeg into "no player audio at all".

**Still approximated, and worth revisiting:** retail opens these voices through `sub_82975700` and
keeps their properties current under `sub_824D2318` / `sub_82975A60`; the port uses the same Splice
one-shot path the pops and the landing already use, with the material record's `+52` level as the
voice gain. The spatialisation and the per-frame property updates are not ported.

**One more gap that must land with the rest:** the two `+0x20`/`+0x24` levels in the message are
still zero. They come from `sub_82496C58`'s interpolation (decoded above but not ported); nothing
the port reads uses them.

As of this commit the two dropped sounds also **report themselves once per run** through the
existing `SKATE_PLAYER_AUDIO contact_voice_unavailable` channel instead of disappearing silently.

---

## Suggested order (updated 2026-09-20)

**Three of the nine entries turned out to rest on premises the evidence does not support** — #1
(2026-09-20, earlier), then #7 and #3 here. A fourth, #2, is now in the same position. The pattern
is consistent enough to be a rule: **re-measure the premise before writing code against it.**

**One playtest answers two of these at once.** Play, ollie a few times, powerslide a few times,
with `SKATE_AUDIO_TRACE=logs\...`, then:

- `grep -o "AS st=[0-9]*" <trace> | sort | uniq -c` → does an ollie reach **201**? (#2)
- `grep "AS st=101" <trace> | head` → does the squeak gate pass during a real powerslide, and is
  there a `Class_Squeaks` `PO` on the same frame? (#3)

Both questions were unanswerable before this session because `AS` carried no state id; it does now.

1. **The playtest above** — it is the gate on both #2 and #3, and costs one run.
2. **#5 (surfaces)** — large, but the audio side is already ported and waiting, and it is the
   single biggest audible gap now that the impacts are measured-correct: every surface currently
   sounds like concrete.
3. **#9 (rail/grind landings)** — everything is decoded except the contact-sound manager's
   `vtable[+12]` dispatch (see above). That one subsystem unlocks rail landings *and* the
   continuous material-dependent landing weight in `player-audio-retail-drivers.md` §9. Land the
   `grind_gate_124` re-arm with it.
4. **#4 (ragdoll/loose board)**, **#6 (NaN crash)** and **#8 (renderer race)** — all need a repro
   before they can be chased.

Resolved and needing nothing: **#1**, **#7**, and the offboard static matching-group bug below.

## 10. Static geometry was hidden from offboard queries — **fixed 2026-09-20**

Not previously listed; found because two tests contradicted each other. `skate_world.rs` filed the
RWCM cluster's packed unit group on `QueryMesh::matching_group`. Since the filter is
`a == -1 || b == -1 || a == b`, that made every offboard ground and line query **reject static
geometry unless the querying actor's matching id happened to equal the cluster's group**.

Retail writes **-1** there: the mesh-record constructor `sub_8276CB18` takes matchingID in `r8`
(`stw r8,180`), and every call site passes a constant `li r8,-1` except one pass-through that
reads an actor/unit object's `+40` — never cluster data. The static path pairs that -1 with the
`li r7,0` / `li r6,0` this code already reproduced as `geometry: 0` / `rejection_flags: 0`. The
cluster group is per-*triangle* in retail: `sub_82AC8A68` stores it at triangle+84, beside the
surface code at +88 that `packed_surfaces` carries. The group is still used to partition clusters
into meshes; only the misfiling is gone.

Worth a playtest look: anything offboard (walking, bailing, ragdoll) that seemed to pass through
static world geometry may simply have been unable to see it.

## 11. Landing dynamics — **three port bugs fixed; the premise behind the old plan was wrong**

Filed 2026-09-20 after "landings all sound the same and loud". Everything below was re-measured,
because the recorded plan rested on two claims that did not survive the evidence.

### The two dead premises

**"Our air time is several times retail's, which is why the level saturates."** False. Retail's own
`+236` is recoverable from the recomp traces: `Class_Treatment` update word 7 is
`fctiwz(air × 1000)` clamped to 10000, i.e. milliseconds. Over `probe/traces/sessions/*`:

| trace | airs | p25 | p50 | p75 | max |
|---|---|---|---|---|---|
| `play4` | 48 | 383 | 583 | 700 | 1583 |
| `play1` | 23 | 516 | 633 | 716 | 1049 |
| `play2` | 5 | 616 | 816 | 1083 | 1366 |

The owner's own landings sit in the same band (0.08–3.5 s, median ≈ 0.5 s). Our air time is
retail's. **Defect #2 is not the cause of anything here.**

**"Retail's continuous landing weight comes from `sub_82496C58` / `sub_824D1E00`."** Also false.
`sub_824D1E00`'s weight input is derived purely from the message's *tier* words — 10000 / 20000 /
32767 — so it is a three-step quantizer, not a continuous weight. And the level curve saturates
sooner than this port's did (see below), so it cannot be the source of loud-vs-quiet either.

### What retail actually varies

**Retail does not make a landing louder for being harder.** Measured from
`.local/captures/retail-levels-20260920-004114.log` (music off, `OUT` peaks against
`Class_Treatment` word 7), 15 landings spanning 350–783 ms of air: peaks run −7.8 to +3.7 dBFS with
**no correlation to air time** — 733 ms produced both −7.8 and +0.8. The MixMap agrees: probing the
real graph under the retail pre-roll (`contacts_input_probe`), the landing class moves Contacts
output 15 only from 2584 to 3650 and output 3 from 6590 to 9309 — **3.0 dB each**.

What changes is the *sample*. `sub_824BA630` starts three voices, and two of them pick by air time:

| voice | slot | sample chosen by | level |
|---|---|---|---|
| impact | `+56` | fixed (`0x447`) | fixed |
| **ladder** | `+496` | `sub_82494D78(deck material)` × `air ≥ 0.75 s` → `0x35C`…`0x35F` | fixed |
| class | `+52` | landing class (0.62 s / 1.02 s) via `sub_824BA3F0` | fixed |

### The three bugs fixed

1. **The ladder voice was never played.** `LandingTuning::ladder` / `ladder_sample` existed in
   `splice.rs` with no callers anywhere — dead code since it was written. Every landing played one
   fixed impact sample where retail picks between four. This is the one most likely to be audible.
2. **The level curve ran on seconds, not retail's ratio.** `sub_824BA630` @ `0x824BA7D0` computes
   `clamp(air / D, 0, 1)` with `D` = `Hash_6D68BC2D1A23C29A` = 0.4, *before* the tier test and both
   windows. The port passed raw seconds, stretching the curve 2.5×: retail's tier boundary is
   **0.04 s** of air and its ceiling **0.12 s**, not 0.1 and 0.3. Note the direction — fixing this
   makes the contact levels *more* uniform, because retail's really are.
3. **Two vault post-gains were missing, and the level/material pairing was crossed.** Each level
   word is scaled before it reaches the message (`fmuls`/`fctiwz` @ `0x824BAAC0`, `0x824BAB20`):
   `Hash_31DEEF8FA219950F` = 0.65 on the first, `Hash_0EC6EEF5366FEA85` = 1.0 on the second. And
   `sub_82486EF0(this, r28 = board, r27 = surface, …)` pairs `msg[+0x20]` with the **surface**
   while `msg[+0x00]` is the board; `sub_824D2318` reads `msg[0x20 + 4 × record]`, so record 0 (the
   board) is leveled by the surface's table entry. The port had them uncrossed.

### The level mechanism — found on a second pass, after a wider playtest

**The "retail keeps the gain flat" reading above was drawn from too narrow a sample** (15 landings,
all 350–783 ms, i.e. all class 0/1). A 35-landing capture spanning 16 ms to 1.2 s says otherwise:

```
corr(peak, air time) = +0.64        peak range 15.8 dB
  air    0-150 ms  n= 4  mean  -4.0 dBFS
  air  150-400 ms  n=12  mean  -4.4
  air  400-700 ms  n= 8  mean  -4.6
  air  700-1050ms  n= 5  mean  +1.0     <- +5.5 dB step
  air 1050+   ms   n= 6  mean  +2.3     <- +6.8 dB
```

Flat to ~0.7 s, then a step — landing exactly on the **landing-class** thresholds (0.62 s and
1.02 s; air factor 0.31 / 0.50 against factor = air × 0.5).

**`sub_824BA630`'s tail writes the class to Contacts controller input 2** — `[this+12]->vtable[8]
(2, clamp(word, 0, 32767))` with word = 0 / 16000 / 32767 for class 0 / 1 / 2 (@ 0x824BAF08 and
0x824BAF44). Input 2 drives output 15, the landing voice's owner send. **This port never wrote any
Contacts controller input**, so output 15 sat at its class-0 value forever: the class picked a
different sample but had *zero* effect on level. That is the direct cause of "every landing the
same loudness", and the port said so itself — `sub_824B90D8 without the controller-input resets`,
and "raises controller input 1 (the MixMap port owns the input)", which it never did.

Also recovered: `sub_824B90D8`'s head zeroes inputs **0, 1 and 6** every frame and pointedly *not*
2 — the class latches until the next landing, holding the send up for the voice's whole life.
`sub_824BA630`'s head raises input 1 to 32767 (the landing pulse).

**Known approximation.** Retail routes the class voice through the send bus, so its level follows
output 15 continuously as the MixMap settles. This engine gives the one-shot a fixed gain at open,
and reading output 15 back would be both a frame stale and pre-settling, so the class indexes the
*settled* measured values (2584 / 3103 / 3650) instead. Retail's send ramps across the sample;
ours is flat for its length.

### Still open

The measured retail step is ~6 dB but the MixMap supplies only 3.0 dB of it, so the remaining ~3 dB
must come from the class samples themselves being louder. Worth checking the three class samples'
own levels before adding any gain. Also unexplained: the 4–10 dB spread *within* a single air-time
band, which the owner hears as slope-vs-flat — a slope landing has lower vertical impact speed, and
`sub_82772B88`'s `landing_bucket_44` (|min of last four COM vertical velocities| against
1.5 / 2.6 / 3.45 m/s) is the obvious candidate, but in this port it reaches only the footsteps
packet. Where retail takes it beyond that is not yet traced.

**Do not** re-open this by widening the `[0.1, 0.3]` window or removing the `/0.4`: that window is
retail's, it is in ratio units, and it is meant to be spent by 0.12 s of air.

### The landing's two collision voices are off by default — a deliberate deviation

`sub_824BA630` posts to the contact-sound manager, so retail plays them. But `sub_824D2318` gives
a contact voice `controllerOutput × messageLevel × materialLevel`, the controller output coming
from `sub_824D20E8`'s jump table (material category → Collision controller output: 13, 14, 15, 16,
17, 18, 12, 19, 21, 20 for categories 0..=9). **This port applies the two level terms and not the
controller one.** It is not recoverable yet: the one-shot path has no per-frame gain update, and
every one of those outputs reads 0 in the only MixMap fixture available
(`crates/skate-audio-core/examples/collision_output_probe.rs` against `mixmap_replay_4400.bin`).

At full material level they swamp the class voice. Solving the mix from a playtest,
`(C + 1.95X)/(C + X) = 1.05`, puts the class voice at **~5 % of the landing peak**, and the class
step measured +0.5 dB against retail's +4.8. A/B'd by the owner at the same trim: with them on,
"low ollies still sound too loud and different than retail"; with them off, "pretty close to
correct". So they default **off**, because a voice at a knowingly wrong level is further from
retail than no voice.

`SKATE_AUDIO_LANDING_COLLISION=1` restores them. **Recovering the controller term is the real fix;
delete the switch and default it back on when that lands.**

