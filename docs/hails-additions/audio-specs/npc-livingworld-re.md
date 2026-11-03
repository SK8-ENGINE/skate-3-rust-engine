# Retail living world (NPCs, traffic, security): how TU3 does it

From the TU3 (title update 3) image, the recompiled code and disc data (reference only; 2026-10-02). Addresses are
evidence; never copy code or game data into the repo. Describe behaviour in our own words.
Tools: local research scripts (listed at the end). Data: local extracts.

## 0. Big picture
- The living world (`LivingWorld`, LW*) has four entity managers, each with Memory/Execution/Factory/
  Census parts: **pedestrians** (`LWPedestrian*`, strings 0x82063E5C..), **vehicles** (`LWVehicleMan::*`
  0x820BD4C8..), **dynamic objects / DMOs** (`LWDynamicObjectMan::*` 0x820BAB40..) and **props**
  (`LWPropCensusMan` 0x820670C8; waypoint groups, conversation and gathering areas).
- Entity types (enum strings 0x82084684): Pedestrian, Vehicle, Skater, DMO, StaticObject. Ped
  sub-types: MaleTeen, FemaleTeen, FemaleAdult, MaleAdult, BusinessMan/Woman, Female/MaleSkater,
  Female/MaleTourist, Granny, SecurityGuard, Bum, Jock, Pro. Vehicle types: Car, SportsCar, MuscleCar,
  SUV, Truck.
- **Behaviour is data-driven**: AI state graphs ship as plain XML plus compiled `.stategraph` files in
  `data/big/miscload.big` (`data/state/livingworldentities/{pedestrian,vehicle,marquee,test_pro}/…`,
  `data/livingworld/PluginDescriptor/*`). 52 ped `aigraph/*.xml`, 18 motion graphs, 13 plugins
  (sit, ATM, vending machine, water fountain, newspaper box, trash bin, look-at, conversation,
  spectate, actor-tracker spectate). The C++ side supplies named behaviours and conditions.
- **Tuning is in the attribute database** (`skatercollections.vlt` → our skater-collections.json):
  classes `livingworld`, `livingworld_census`, `livingworld_census_ranges`, `livingworld_categorygroups`,
  `livingworld_entitycategories`, `livingworld_entities` (157), `livingworld_models` (145),
  `livingworld_entities_{chase,perceptions,navigation,locomotion,patrolzone,protect,knowledge,
  moodreactions,moodresults}`, `livingworld_moodeventcategories`, `livingworld_entity_takedown`,
  `livingworld_vehicle_{characteristics,drivers}`, `livingworld_dynamicobject_*`, `livingworld_handprops*`,
  `livingworld_conversations*`, `ai_skater_profiles` (193), `ai_characters` (179), `aud_traffic_engine`.
  Class and record names resolve by `name_id`; **field names are compiled out** (schema has only type
  names like `Sk8::LivingWorld::tLWCensusEntry`, `tLWGroupEntry`, `tLWCensusCircle`). Fields read by hash
  have code sites (`fieldxref.py`); fields with no site are read through the class layout by offset.
- Navigation middleware: **NavPower (BabelFlux)**: `bfxSystem/bfxPlanner/bfxMover(3D)/bfxBuilder`
  0x8216E1AC.., `NavPower Runtime` 0x820BD170, `NavPower Crosswalks` 0x82077068, `NavPower Obstacle/Link`.
- World scripts can drive NPCs: `WorldScriptSim::mPedestrianManager / mVehicleManager / mDmoManager /
  mSplineManager` 0x82176658.., commands Spawn/Unspawn/MoveToLocation/ReleaseToLW/KnockDown,
  `SplineManager::cFollowSpline`, Attach/DetachPedestrian|Vehicle. Challenge script functions:
  `IsLocalPlayerBeingChased` 0x82192554, `GetIfPlayerBeingChased`, `ForceCullLivingWorld` 0x82192948,
  `Activate/DeactivateSpectatorWaypoint`, `LoadLargeCrowd` (challenge crowds are separate:
  `Sk8ChallengeSystem:LargeCrowdGenerator` 0x82096B20, crowd imposters).

## 1. Population (census)
### Data
- Region layers (world painter, RW type 0x00EB000F): `livingworld_npc_census`,
  `livingworld_vehicle_census`, `livingworld_security_guard_areas`, `livingworld_dmo_safety`. Each painted
  cell holds a `livingworld_census` record key. Painted area (`census_layers.py`, output
  `census_layers.txt`):
  - DownTown peds: residential 13.9 ha, business_center 12.5, memorial 6.7, aletown 5.1, mall 2.2;
    vehicles: `dwntwn` 89.7 ha.
  - Industrial peds: reclaimed 5.0, loadingdocks 3.0, `downtown` 4.2; vehicles `indust` 52.5 ha.
  - University peds: campus 17.8, observatory 1.2; vehicles `univ` 96.8 ha.
  - Parks have no census layers (no peds/traffic there unless a challenge spawns them).
- `livingworld_census` record = `tLWCensusEntry`: RefSpec to a `livingworld_categorygroups` record +
  (8 bytes runtime) + **u32 max population**. Values: peds default 40; downtown/university/industrial 20;
  aletown/business_center/mall/memorial/residential/campus 15; reclaimed 10; loadingdocks 8;
  observatory 6. Vehicles: default 25; dwntwn 30, indust 25, univ 10.
  - Extra fields: peds `Hash_4936C5A55AB38AF3` (6 / univ 7.5 / downtown 6.5 / industrial 5) and
    `Hash_4432C5ACB168F5BA` (5 / 7 / – / 4.5): meaning unknown (no hash-read site; probably spawn
    rate or per-second budget). Vehicles `Hash_FFE5E258BD468196` (15; dwntwn/indust 20; univ 5), read at
    sub_826B7EE4 (fork result below).
- `livingworld_categorygroups` = array of `tLWGroupEntry` (RefSpec to `livingworld_entitycategories` +
  f32 weight at +24). Examples: downtown peds adult .2/jock .1/tourist .1/skater .1/teen .05/business .3/
  bum .15; university adult .15/jock .2/tourist .1/skater .15/teen .25/business .1/bum .05; generic
  `pedestrians` includes `ped_class_security_street` .1. District sub-areas (aletown, campus…) use a
  single category listing 6–23 concrete entities (e.g. campus = tourists, teens, skaters, jocks,
  `male_ambassador_big`, a granny). Vehicles: dwntwn hatchbacks .1, minivans .1, muscles .075, sedans .2,
  sports .175, suvs .125, taxis .1; univ adds pickups .125, sedans .2…; indust pickups .175.
  Full dump: `census.txt` (`census_dump.py --cats`).
- `livingworld_census_ranges` (records pedestrians/vehicles/props/dynamicobjects/…): two float[5] sets
  per type. Read in sub_826B7530 (+ siblings) via accessor sub_82553128.
  - **Set chosen by player speed**: `speed = |velocity| * 3.6` (km/h, constant 0x822F8628). The two
    sets are lerped by `t = clamp((speed − A[4]) / (B[4] − A[4]), 0, 1)` (sub_826B7D60); if
    `A[4] ≥ B[4]` set A is used as is.
  - Elements: [0] spawn ring inner radius, [1] spawn ring outer radius (both passed to the spawners),
    [2] **cull radius** (entities beyond it are removed, squared-distance test in sub_826BA8B0),
    [3] **forward offset** of the census circle along the velocity direction, [4] speed key.
  - Probable layout order (A = field E0C6647F889A71FC, B = D993B45FFEB335F6; the other order would make
    the lerp dead code): pedestrians A (50, 60, 70, 0 @45 km/h) → B (50, 80, 90, 20 @80 km/h);
    vehicles 80/100/110/0 both; props 65/80/90/0 (30 vs 10 km/h); dynamicobjects 0/90/100;
    skatepark DMOs 0/150/200; extended-challenge DMOs 0/100/130.
    So at skating speed: peds spawn 50–60 m away and vanish beyond 70 m, i.e. **just out of view**.
- Density scalars at census object +136/+140 come from a global at 0x830B7AE8 (mode 3 → floats
  +336/+332 clamped 0..1) in sub_826B7010 (probably challenge/online density overrides; unverified).

### Code
- LW census tick **sub_826B71F0** (called from sub_826BDB50 ← sub_82859E70). Per type, in several
  game modes: peds **sub_826B7530** (range record at census+104), vehicles **sub_826B7760** (+112),
  DMOs **sub_826B7980** (+128), props **sub_826B7BA8** (+120, cull only).
- Peds: cull sub_826BA8B0 (circle centre, cull radius); spawn **sub_826B9940** (ring spawn) and
  **sub_826B90F8** (larger; uses entity/category data, probably waypoint/plugin-placed peds; unverified).
  - Ring spawn: per tick 2 attempts and at most 1 spawn. Each attempt: random point in the ring
    (sub_82E17508, onto the nav mesh), random heading, then **sub_826B8B88** picks a category from the
    census at that point (census lookup sub_826B8A28 → `livingworld_census` accessor sub_8269AF00) and
    checks the budget; then the factory spawns through the manager vtable (+4).
  - **Initial populate** (flag r7): 6000 attempts, up to 600 spawns, ring **8–80 m**
    (0x82099250 / 0x820E5748).
- Vehicles: spawn sub_826B9B90 (same pattern), cull sub_826BAAB8. DMOs: spawn sub_826B9D58 (→
  sub_82C4A130), cull sub_826BAD98; props cull sub_826BAC48.
- Global `livingworld` record: `entities` (two flags), `dynamicobjects` (2.5, 450, 2, 250, 300; read in
  sub_82C4A130 / sub_82C55160), `trafficlights` (7, 0.5, 0.4, 1; sub_826B1540), `roadnetwork` (1.15, 1.3,
  10, 10, 5, 4; getter sub_826B3738 via LW object +13340).

## 2. Paths and navigation (streams)
cSim tile streams (RW type ids from the table at 0x830384A0: (id, name) pairs; 0x00EB000F = layer):
- **0x00EB0027 NAVPOWERDATA**: 2 per DownTown tile (240); NavPower nav meshes for peds.
- **0x00EB0014 AIPATHDATA**: ~1 per tile (100 in DownTown). Header u32 count (e.g. 0x18) + u32 0x10,
  then 0x60-byte entries: bbox min/max (vec4), district tag (`dwtn`), 64-bit ids, offset + point count,
  a small int (1–3), 7, mask 0x3FFFFFFF_FFFFFFFF. Point records (36 bytes) hold a position at street
  height plus small floats and packed normals; a second block holds position + velocity triples with
  upward components (2–3.5 m/s) and 0.85: looks like **AI-skater lines with jump launches**, not roads.
  Challenge versions: `data/content/aipaths/AIPATH_Challenge_*.rx2` (59 files, miscload.big).
- **0x00EB001A WAYPOINTDATA** (18 in DownTown): waypoint groups with name strings, e.g.
  `gd_DMO_GLBL_DrPepperVendingMachine_1002 | WayPoints1 | WP_VM_position_1001` with position and
  facing: the plugin anchors (`waypoint_vendingmachine`, `_atm`, `_sit`, `_patrol`, `_spectator`,
  `_conversation`, `_lookat`, … strings 0x82066F00). `DMO Waypoint Group (From Hotpoint)` 0x820BAD18:
  DMOs (vending machines, benches, trash bins) also create waypoints.
- 0x00EB001D DMODATA (226 in DownTown): DMO placements.
- 0x00EB0004 SPLINEDATA (~2/tile): straight segments at ledge height with editor GUIDs: grind splines,
  not roads.
- **0x00EB0013 (table name "RAINDATA", really the road network)**: standalone arena in 52 DownTown
  tiles; loader sub_82C9B3A0 (registered in stream dispatcher sub_82C990A8). Header bbox (2× vec4) +
  counts/offsets; segments with from/to node ids (u64), link id, length (e.g. 143.5 m), width 8 m,
  speed 14.17 m/s (51 km/h), lanes per direction (2/2); lane samples every 4 m (pos, next pos, dir),
  lanes 4 m apart; 25-sample stride blocks. Sample: `cSim_-150_-50_high_*_00eb0013_1.bin`.
  Runtime object LivingWorld+0xC0A0, global pointer 0x830854C0 (set in sub_826BBC40); vtable slot 16 =
  segment by id, slot 64 = query; 248-byte records.
- Other stream callbacks (sub_82C990A8): NavPower 0x82C9B250, AIPATH 0x82C9B4F0, WAYPOINT 0x82C9B3F0,
  shared unload 0x82C9B2A0.
- AIPATH = recorded human runs (`d:\aipaths\out\` strings 0x82255D08, AIPath::Node / ExtendedNodeData,
  flags Race/Vert/Street/Shortcut/Slowdown/AllOffBoard/SomeOffBoard/Spectate/Escape, difficulty
  Easy–Extreme).
- Census layer lookup: sub_82C0EAC0(painter, layer index, pos); index 2 = npc census, 3 = vehicle census
  (via sub_826B8A28); fallback record `default`.

## 3. Behaviour runtime (state graphs)
- Loader sub_826BC250 (from LW init sub_826D9040), path table base 0x821DE6BC. Each `.xml` has a
  `.stategraph` beside it = the same tree in binary with includes merged (tag, attr count, name/value
  pairs, children). Which one the runtime reads is unverified (no ".stategraph" literal).
- Registration: static initialisers 0x82F76000–0x82F7B000 (peds) and 0x82F8C568–0x82F8CC88 (vehicle) call
  sub_826C3D18(registry, name, factory): name-sorted tree. Registries: behaviours 0x826C1670 (160),
  conditions 0x826C1730 (127), plugin-descriptor conditions 0x826C2458 (23), plugins 0x826C2518 (6:
  LookAt, UseWorldProp, PedestrianConversation, GatherBehaviourGeneric, SpectatorWaypointGeneric,
  ActorTrackerSpectate), motion-graph hooks 0x826C25D8. Full list `aireg.txt`
  (762 incl. vehicle), `ai_registry.txt`. Condition evaluate = vtable slot 12, behaviour update = slot 13.
- Example behaviour SuggestVelocity: factory 0x826C9488, vtable 0x8230A384, attribute `linear` → +32;
  begin sub_8269F6A8 sets the mover's suggested speed (mover vtable +168).
- Managers: LWPedestrianCensusMan created in sub_82E25EE0 (712 B, vtable 0x8232D1C0); ExecutionMan
  sub_82E25CE8; per-frame ped tick probably sub_82E239A8 / sub_82E238C0 (unverified).
- Graph shape (ped): every state runs MonitorEnvironment; InitialState → Plugin | AddressWants | Wander.
  Wander (speed 3.0) → Plugin, Scatter, AddressWants, ZombieFollow, WanderOnRoad (on road, not at an
  intersection), RunFromHonker (≤30 s), Colliding. Road wander waits at crosswalks until the walk sign.
  AddressWants handles wants in a fixed priority: observe (collision/slam/trick reactions, 3.5 s
  watch + cheer speech), warn, throwhandprop, greet, startconversation, startspectate, taunt, taze,
  chase; interruptible: alert, flee (escaped at > 15 m).
- Vehicle graph: FollowingLane ↔ PassingIntersection / Impatience → ChangingLane / PullingOver →
  StayingParked (alarm) → PullingOut. Handlers (slot 13): FollowingLane 82C376E8, Impatience 82C38390,
  ChangingLane 82C38730, PullingOver 82C38C70, StayingParked 82C39138, PullingOut 82C39208,
  PassingIntersection 82C39490. Conditions (slot 12): 82C39CF0..82C3A850, StopAlarming 82C3A4D0.

## 4. Mood → wants (reactions to the player)
- Events (`livingworld_moodeventcategories`): each has a range pair (collision 20/20, skatertrick 10/50,
  nearbysession 100/100, presence radius 20 every 0.5 s, skaternoise 0/40, nearbychase 20/20 prio 3), a
  continuous flag, priority. Presence/trespass monitor sub_82E3CA20.
- Per ped type, `livingworld_entities_moodreactions` = prerequisites + weighted list of
  `moodresults` (fields: event, target filter, Nth occurrence, probability 0.4–0.8, cooldown 10–31 s,
  exposure time, distance 3.5/5/20). Evaluator sub_82BFE010 / sub_82E41EB0 / sub_82BFDF28.
  Decoded: `mood.txt`, `moodresults.txt`.
- Want enum (0x820646BC / 0x820C9080): 0 none, 1 angrychase, 2 warn, 3 taunt, 4 returngreet, 5 greet,
  6 startconversation, 7 alertto, 8 joinchase, 9 throwhandprop, 10 flee, 11 startle, 12 taze,
  13 nearbycollisionreaction, 14 slamreaction, 15 startspectate, 16 nearbyskatertrick.
- (Data reading; **measured in §5d: male peds bump 1 warn, bump 2 angrychase; female peds warn+taze or
  flee**.) Ordinary peds: bump #1 warn, #2 warn, #3 angry chase; damaging an in-use object → chase; nearby trick
  → watch + cheer; nearby slam → watch; nearby session → spectate (p 0.8, cd 30 s); females flee on the
  2nd bump, warn+taze on the 3rd; jocks also warn/chase on a close avoid (3.5 m); hand-prop throw p 0.8.
- Perception sub_82E26F38 (from sub_82E27240): near sphere (range × 1.0, mongo 1.5) = seen at once;
  else range × D238, 120° cone (sub_82E16588) and line of sight through the collision world
  (sub_82E169C8). Global record (10, 0.01, 100, 0.1, 0.2, 0.6; sub_82E24B60): use unknown.

## 5. Security guards and chases
- `livingworld_security_guard_areas` (HighAlert_Area / NoSkateZone_Area, test sub_82C0E890 from
  sub_8289B7E8) is **empty or absent on the shipped maps**: no area-painted zones in free roam.
- **CONCLUSION (2026-10-02): Skate 3 has no active security guards / busted system in free roam.** User
  source: Skate 2's MongoCorp guards chased and busted you in specific downtown areas; Skate 3 removed
  that to make Port Carverton a relaxed sandbox. Our evidence agrees: guard-area layer empty on all three
  districts; zero SECCHASE/SECTAKE/SECTAZE/SECZONE calls in both NPC runs. The guard code, chase records,
  nsz types and `security_street01` census entries are Skate 2 leftovers. **Not part of the Skate 3 port**; but keep the guard research: the user wants Skate 2 support eventually (a planned item).
- **But ordinary pedestrians DO chase and take you down (measured 2026-10-02, user session
  recomp session `npc_20261002_143554`, the user angering peds):** a regular ped (red jacket) got a
  **red down-arrow marker over its head** (angry), chased the player and tackled: SECTAKE attempt 59.04 s →
  success 59.20 s, screenshot shows a **"KAPOW!!"** caption over the downed skater, then the ped gloats
  (gesture, 61.5 s). The chase lasted ~10 s (49.9–60.4 s): every frame (dt 0.0333 = 30 fps) one chaser
  (ctx r4 4074F9C0) evaluated escape `sub_826AC668`, rest `sub_826AC898` and give-up `sub_826AD690` (all
  returned 0 = keep chasing), plus the takedown entry choice `sub_82E3C000` (f1 = 13.06 constant, ret 7 on
  39 calls then 1 once = an entry chosen just before the attempt). The quoted claim that Skate 3 removed
  pedestrian hostility is **wrong for peds**; only guards/busting are gone. The "SEC*" hook kinds are the
  generic chase code (named after the chase records), not guard-only.
- NPCSTATE bursts at load (8.8 s, 27.6 s: PedestrianColliding/Scatter/InterceptChasee factories) are
  state-graph setup, not reactions.
- (Earlier) **User (2026-10-02): guards don't wander, they stay in specific areas.** So guards are not (only) census
  peds: find the placement source. Searched: world sim streams have no patrol/security/nsz names
  (`tools/stream_strings.py`); census groups do list `ped_class_security_street` (weights 0.05–0.1, e.g.
  `district_park01_peds_census`), which would spawn them anywhere in the painted census area. Open: patrol
  zones (`livingworld_entities_patrolzone` 5/10, EntityOfInterestIsInZone sub_826AF310) and where a guard's
  zone centre comes from (spawn position? waypoint_patrol in another stream type? challenge hulls?).
  The user offered a hand-played session angering peds/guards (a traced recomp session with the NPC hooks).
- Scripted "wander" runs (make_wander_script.py) are open-loop and get stuck against walls (user saw it
  stuck in a corner): unsuitable for reaching NPCs. Use the user's passive session instead.
- Guards are census peds: `security_street01` (weight .05–.1 in many groups), `security_nsz01` (University
  group .1); patrol cars `vehicle_class_patrol`. Types: nsz01 (chase `mongo`, perceptions 1.5×),
  nsz02 (`no_skate_zone`), street01 (`street`); reactions `security01` / `security_street`.
- Patrol zones: conditions EntityOfInterestIsInZone sub_826AF310, …BufferRegion sub_826AF3F0,
  …MaintainInterestRadius sub_826AF5B0, ReturnToPatrolZone sub_826AEE80; patrolzone record 5 / 10
  (which is buffer vs interest radius unverified); zones probably from `waypoint_patrol` groups.
- Triggers: no-skate-zone guard: first bump = chase; presence → trespass warn (0.5 s), still on board
  → chase (3.5 s); chase while it walks back to its zone; joins anyone's chase; other skaters nearby:
  warn p .4 / chase p .6. Street guard: skitch → warn (2 s). Bumps as ordinary peds.
- Chase fields (livingworld_entities_chase): 240EB11A0CB469C3 **escape distance** (peds 65, security 500,
  street 45, zombie 8; ChaseeEscaped sub_826AC668); 4686FEC68A60BD69 **exhaustion limit** (30/45/jocks 60;
  NeedToRest sub_826AC898); 78E2E1B19721FD4E **give up after N takedowns** (2, street 3, zombie 1;
  sub_826AD690); 314597EDA6B921F2 alert distance 25; FA762B2EA065A6D7 alert timer 30 s;
  5A3C4A7700DDA13C unreachable timer 5 s; probable 1AE29113978BE5AA investigate time (10/20/jocks 2);
  unmapped CD6575C0E03860E7 (5–12, run speed?), CEA5D982FD05BE0B (max chasers?).
- Chase graph (disc XML): 1.25 s hold before starting (speech), intercept 4–10 m/s with ≤20 m lead and
  22.5° prediction; takedown attempt 3 s timeout; unreachable → warn, wait 5 s, taze (face ≤20°, ≤15 m,
  1.5 s draw, 2 s cycle), give up, mood suppressed 10 s; mood suppressed 5.5 s when a chase can't
  start. End: escaped, lost + investigate time out, exhausted (rest, music cooldown), takedown cap,
  successful takedown ends the whole chaser group ("aggressivecapture").
- Takedown: AttemptTakeDownTargetable sub_826A49D8, success sub_826A4E50, entry choice sub_82E3C000 (by
  approach side and relative speed; 7 entries generic_male, 2 bum; windows 0.08–0.3 / 0.1–0.5 s, reach
  ≤1.2 m, angles −70..30°). Skater-side bail response not found (skater condition `IsTakeDownByBoard`
  0x820DC888 exists). Taze: DrawTazer sub_826A8258, TazeWantTarget sub_826A8398. Timer set/get
  sub_82E40940 / sub_82E40A80 (index = position in the name list at 0x82064BCC).

> **2026-10-02 19:00: the recomp sessions behind the measured sections below were deleted (pre-fix trace writer, a few malformed lines). Treat those findings as "re-verify on clean data" (only all_20261002_163809 / _164620 / _180430 and world_hooks are clean).**

## 5b. Ped chase decoded (user session `npc_20261002_143554`, Aletown, 2026-10-02)
**Object layout (from the trace's address spacing):** peds are 0x1780 bytes apart (spawn returns 47B9C480 +
i·0x1780); perception object = ped + 0x920 (NPCSEE r3); takedown context = ped + 0x728 (SECTAKE entry r3);
the "brain" (mood + timers, NPCMOOD/SECTIMER r3) is a separate array 0xCE0 apart from 47BDD8E0, same slot
index. The chaser was **slot 1**: ped 47B9DC00, perception 47B9E520 (583 checks, the most), brain
47BDE5C0 (299 timer sets, the most).

**Timer indices = order of the name list at 0x82064BCC** (sub_82E40940 r4; debug string
"Tried (%s) with probability (%f) -- Set Timer (%d, %f)" at 0x82064B94):
0 NextWarnTimer, 1 InvestigateTimer, 2 ChaseSkaterOutViewTimer, 3 ChaseExhaustionTimer, 4 RestTimer,
5 OutOfViewTimer, 6 LookAtTimer, 7 AlertTimer, 8 TimedRangeRandom, 9 SpectatorBoredomTimer,
10 SpectatorCheerTimer, 11 AmbientBehaviourTimer, 12 CellPhoneTimer, 13 GatherTimer, 14 WanderWaitTimer,
15 PatrolZoneMaintainInterestTimer, 16 AttemptTakeDownTimeout, 17 TakeDownSuccessTimeout,
18 ReturnToPatrolZone, 19 ProRecTimer, 20 TargetConverse, 21 StartChase, 22 ForceWarnTimer,
23 WaitAfterCheer, 24 SitTimer, 25 PresenceCheckTimer, 26 TrespassCheck, 27 PatrolTimer, 28 IdleTimer,
29 TimeUntilICanBePrimaryChaserTimer, 30 ConversationSpeakingTimer, 31 HandPropActionTimer,
32 HandPropUsageTimer, 33 HandPropUsageTimer2, 34 ThrowHandPropTimer, 35 ThrowHandPropReactionTimer,
36 InterestTimer, 37 TazerIntoTime, 38 TazerWaitTime, 39 TazerPlayerDropTime, 40 TazerCycTime,
41 WanderRoadCheck, 42 TargetUnreachableTimer, 43 WanderTargetTime, 44 CollisionVolumeUnlock,
45 PluginTimeout, 46 ScatterTime, 47 UnspawnTime.
Every ped re-arms **25 PresenceCheckTimer 0.5 s** and **26 TrespassCheck 0.1 s** continuously.

**Timeline (trace time; screenshots `shots/` at CLOCK + ms):**
| t (s) | trace | screen |
|---|---|---|
| 32.1 | mood eval → 1 | player running at the ped, board in hand |
| 41.0 | mood → 1 | player bailed into the ped |
| 44.2 | mood → 1 | bump on foot (board on the ground) |
| 45.7 | mood → 1 | another bump, ped stumbles |
| 48.5–48.6 | mood → 1, **timer 21 StartChase = 1.25 s** | **yellow down-arrow** over the ped, pointing = warn |
| 49.9 | chase starts (= 48.6 + 1.25) | **red arrow** = angry chase |
| 49.9–60.4 | every frame (dt 0.0333): give-up sub_826AD690, escape sub_826AC668, rest sub_826AC898 → 0 (keep chasing); takedown entry choice sub_82E3C000 → 7 (none of the 7 entries fits) | ped standing on/at the player |
| 59.00 | entry choice → **1** | |
| 59.04 | takedown attempt sub_826A49D8, f1 = **3.0** (= AttemptTakeDownTimeout 3 s, chase graph) | |
| 59.20 | takedown success sub_826A4E50 | **"KAPOW!!"**, player knocked down |
| 61.5 | | ped gloats (gesture) |
| 64.8–66.7 | mood → 1, timer 8 TimedRangeRandom 5.0 s; 71.7 timer 36 InterestTimer 3.5 s (5 s later); again 84.6 / 89.6 | post-chase interest |
- So: warning (yellow) → 1.25 s StartChase hold → chase (red) → takedown (3 s attempt window) → gloat.
- The entry-choice floats: f1 = 13.06 constant, f2 ≈ −126 drifting slowly; hypothesis: target position
  components (x ≈ −126 lies in `ots_dwtn_05`'s X range where the player was); unverified.
- Not measurable from this session: how many bumps escalate (mood calls were rate-limited; 4 positives
  before the warn), chase speed (no positions logged), chase timers 2/3/16/17 (SECTIMER lost 956 of ~1,256
  calls to the 20/s limit during the chase). Hook fix (2026-10-02): SECTIMER named, unthrottled except
  25/26; see hooks_npc.cpp. Positions need a ped/skater position read (offset TBD).

## 5c. Second user session (`npc_20261002_144545`, ~200 s, many peds angered, improved hooks)
Analysis: the local tool `ped_chase.py <trace>` (saved
`ped_chase_144545.txt`). 4 chases, 3 takedowns, 1 tazer ped, 159 mood hits.
**Chase sequence, measured 4 times:**
| ped | trigger mood hit | StartChase (1.25 s) | chase evals | end |
|---|---|---|---|---|
| slot 3 | 42.99 | 43.89 | 44.9–59.6 (14.7 s) | takedown attempt 59.65 → success 59.72 |
| slot 3 | 79.85 | 80.74 | 81.7–82.1 | attempt 82.09 → success 82.31 |
| slot 3 | 89.32 | 90.37 | 92.0–111.2 (19.1 s); InvestigateTimer 10 s set/cleared at 94.5/95.3 (lost then regained sight) | attempt 111.30 → success 111.36 |
| slot 10 | 173.62 | 174.52 | 176.2–189.6 (13.4 s); InvestigateTimer 10 s 177.8; **TargetUnreachableTimer 5 s at 187.05** (player in a deep pit, screenshot) | gave up, no takedown |
- **Trigger → StartChase ≈ 0.9 s** every time (0.90, 0.89, 1.05, 0.90; the warn reaction plays), then the
  1.25 s hold, then the chase. The trigger hits are the frame-rate evaluations (f1 = 0.0333/0.0667); the
  1 Hz hits with f1 = 0.1 are periodic presence/interest evaluations (followed by InterestTimer 3.5 s and
  TimedRangeRandom 5 s), not escalations.
- **No lasting cooldown:** the same ped (slot 3) chased three times in ~70 s.
- **Takedown:** entry 1 chosen → AttemptTakeDownTimeout 3 s → success 0.06–0.22 s later (all three).
  Entry-choice floats: f1 10.54, f2 −211.2…−211.7 here (13.06 / −126 last session); per-chase constants,
  probably target position (hypothesis).
- **Tazer (slot 8; §5d: tazer peds are FEMALE; 'heavy man' was a misread; YELLOW arrow = warn stage, not a chase):** draw at 142.87
  and 144.63 (TazerIntoTime **0.13 s**, TazerWaitTime **1.5 s**), at 146.29 TazerPlayerDropTime **0.3 s**,
  TazerCycTime **2.0 s**; again 162.63. Screenshot 142.9 s: aiming the tazer at the player. So tazing is a
  warn-stage reaction of some ordinary peds (matches the mood data: warn + taze), no guard needed.
- **Correction:** the tazer ped is most likely female: `tazeperp` / `fleeperp` are default_female results
  (mood.txt); male peds use skatercollisionwarn/chase. Re-check with the next session's MOODOUT.
- **Position + mood-output hooks added (2026-10-02):** ped position getter `sub_82E3D0D8` (vtable slot 9
  of the component at owner+188; owner = ped − 0xB0; r3 → out vec) → PEDXYZ; PEDSEE (target position,
  range, seen); MOODOUT (evaluator output words: r5/r6/r8/r9/r10 words, r7's 4 words, result list). Smoke
  run: PEDXYZ addresses = slot bases, plausible walking motion; background hits read result 6, word 8,
  handles 020001D0/02000091.
- Still unmeasured: chase speed (no positions), how many bumps per escalation (bump events aren't hooked;
  mood hits don't carry the event id yet).

## 5d. Third user session: reactions decoded (`npc_20261002_150519`, ~340 s, PEDXYZ/PEDSEE/MOODOUT hooks)
Tool: the local `ped_chase.py <trace>` now prints decoded reactions.
**MOODOUT decoded:** result list words = **want enum** (0x820646BC: 0 none, 1 angrychase, 2 warn, 3 taunt,
4 returngreet, 5 greet, 6 startconversation, 7 alertto, 8 joinchase, 9 throwhandprop, 10 flee, 11 startle,
12 taze, 13 nearbycollisionreaction, 14 slamreaction, 15 startspectate, 16 nearbyskatertrick); output word 2
(r6) = **mood event id**: **10 = skater collision (bump)**, 11 = collision nearby, 2 = slam nearby,
6 = trick nearby, 8 = presence (greet/conversation), 16 = greeted (returngreet). Counts this session:
startconversation 350, greet 117, returngreet 26, warn 11, slamreaction 7, angrychase 6, flee 5,
nearbycollisionreaction 9, nearbyskatertrick 4. r7's first word = 1 for target-based results, 0 for event
results; words 4/5 are floats on some results (5.0 / 0.5, 0.1, 0.8, 0.3); likely duration/probability (?).
**Escalation rule (measured):**
- **Male peds: bump 1 → warn, bump 2 → angrychase**, every time: slot 8 (44.25 warn, 45.56 chase),
  slot 5 (75.69 → 76.96; again 121.34 → 122.41: the count resets after a chase), slot 10 (307.74 → 308.91;
  330.75 → 333.04). The data's "bump #1 warn, #2 warn, #3 chase" reading (§4) is off by one: the second
  collision chases.
- **Female peds (tazer users): bump → "warn + taze" (two results) or flee**, never chase: slot 6 (96.59
  warn+taze → 101.18 flee), slot 13 (61.0 flee, 142.41 flee, 148.55 warn+taze, 155.77 flee), slot 9 (190.28,
  275.40, 286.10 warn+taze, 289.26 flee). Screenshot 190.3 s: a woman in a red top, yellow arrow; confirms
  the tazer ped is female (§5c's "heavy man" was wrong).
- Tazer: TazerIntoTime 0.13 s + TazerWaitTime 1.5 s per draw (repeated draws while aiming); a hit adds
  TazerPlayerDropTime 0.3 s + TazerCycTime 2.0 s (slot 9 at 193.60 and 293.32).
- Bystanders: event 11 → nearbycollisionreaction; slams nearby → slamreaction; tricks → nearbyskatertrick.

**Speeds (PEDXYZ, 4 Hz per ped, horizontal):** walking median **1.30 m/s** (p90 1.57; 9,300 moving samples);
during the 1.25 s StartChase hold ~0.8 m/s; chase bursts p90 **7–9 m/s** (graph: intercept 4–10 m/s).
Takedowns 4/4 successful (attempt → success 0.10–0.13 s). Chases were short (player stayed close).

## 6. Traffic driving (beyond the graph)
- Manoeuvre decider sub_82C41CD0 (from 82C3D830) → vehicle+4396 (1 lane change on a timer, 2 overtake a
  slow/stopped car, 3 pull over), target lane +4380. Impatience: stopped (≤0.1 m/s) + gap test.
- Speed planner sub_82C3FA08 → target speed +3408 (lane speed + driver offset); look-ahead braking over
  20 m, 5 m stop/reverse distance. Car alarm stops after 8 s (+3716).
- **Car alarm trigger (2026-10-04):** the vehicle's collision callback `sub_82C3C150` (interface at `+136`) sets the
  alarm flag `+3424` bit 0x10 and zeroes the alarm timer `+3716` and the parked timer `+3712` when the car is in
  StayingParked (`+3424` bit 0x80) and the contact vector (`msg+48`) is longer than
  `vehicle_characteristics.543475921FD9E04A` (0.1); any collider, every contact restarts it. StayingParked's update
  counts `+3716` while alarming; StopAlarming = `+3716` > `E199FC7CEA222809` (8 s), action `sub_82C3B4E8` clears
  the flag; pull-out (`sub_82C3A3A8`) is blocked while it is set. Details: `world-traffic-audio.md` "Car alarm trigger".
- Traffic lights sub_826B1540: 7 / 1 / 0.5 / 0.4 s phases via sub_82E156D8 (which is which unknown).
- Sounds: SFXObj_TrafficHorn sub_824D6BE8, SFXObj_TrafficSkids 824D7440, banks Traffic_Horn,
  Traffic_Skid, car_alarms.abk; engine `aud_traffic_engine` (idle 800–1870 rpm, max 2100–4000, patch
  0–7; sub_824D6110).
- Peds' IsBeingHonkedAt (826ABF98) reads ped+5896 → +3232 honker id; setter not found. Skater vs car:
  no traffic-specific bail found; the contact-force bail code has a vehicle term (string 0x820CFF14).
- **Vehicle position (2026-10-02):** component at vehicle+144 (vtable 0x82322240, all vehicles) slot 9
  `sub_82C45CB0` → `sub_82C48248` copies the world matrix at `*(vehicle+164)+16` (row 2 forward, row 3
  position); slot +88 returns the current speed (f1, used by the speed planner). Traffic hooks:
  `hooks_traffic.cpp` (VEHSTATE etc.); smoke run Crystal Towers: positions/speeds plausible.
- `livingworld_vehicle_characteristics` layout and `livingworld_vehicle_drivers` (default/reckless/taxi/
  fast/normal): `fieldxref_traffic.txt`.

## 6b. Vehicle session (user, `npc_20261002_152302`, ~140 s, hooks_traffic.cpp)
Tool: `py -3.13 tools/recomp-trace/veh_trace.py <trace> [--vehicle ADDR]` (saved
`veh_152302.txt`).
- 17 vehicles tracked. **Moving traffic: median 5.8 m/s (21 km/h), p90 14.9 m/s (54 km/h)**, typical max
  15–18.6 m/s (jumps of 39/47 m/s = spawn/teleport glitches, ignore).
- **Cars stop for the skater:** the user landed on a car roof (screenshot 85 s: heelflip onto the green
  car) → that car stops; standing in the lane in front of it (112 s) → it stays stopped and **traffic queues
  behind** (yellow cab, red car): 3 cars stop/go in step 84–107 s (2–4 s stops), then all held 14–20 s at
  108–123 s while the skater blocked the lane. Several later stops 125–140 s.
- Manoeuvre (+4396) / target lane (+4380) changed only twice (→ 0 at 30.05 / 44.81 s): no lane changes or
  overtakes around a blocking skater in this session; cars wait.
- **+3408 is not a plain target speed:** it ranges −76…+5 (negative while stopped/queued). Treat as a speed
  delta / planner output (?) until decoded.
- **Traffic light timers (TRAFPHASE, set once at load, 5.93 s):** 20 phase timers in two orders: 0.5 / 8.0 /
  0.5 / 7.0 / 1.0 and 0.5 / 7.0 / 1.0 / 0.5 / 8.0 s → per direction **green 7 or 8 s, amber 1 s, all-red 0.5 s**
  (interpretation; notes §6 had 7 / 1 / 0.5 / 0.4 from data).
- **Horn / skid hooks fired only at load** (4 each at 6.15 s, constructors): actual honks/skids are not in this
  trace. Use a session with every trace category (audio + npc + world + traffic + audio capture): the audio
  hooks log every horn/skid/speech POST and PLAY, so they can be tied to vehicle/ped state. PEDHONKED: 0.

## 6c. Skitching + AI skaters (combined session `all_20261002_153038`, ~8.5 min, audio+npc+world+traffic)
**Skitch (measured, screenshots 490 s holding the yellow cab, 509.5 s let go):** vehicle 47CAEDA0.
- The car's flags byte **+4403 becomes 0xC0 for the whole skitch** (484.15 → 508.46 s; seen nowhere else:
  91 of 12,740 VEHSTATE samples). User: no special skitch sounds, "the vehicle just changed its logic".
- While 0xC0: the +3408 field locks at **0.20**, the car accelerates 5.5 → **16.8 m/s (≈60 km/h)** over ~4 s and
  holds 13–16.8 m/s for ~24 s / ~270 m: faster than normal traffic (median 5.8, p90 14.9 m/s).
- When 0xC0 clears (skater lets go): +3408 → −7.25 and the car **brakes to a stop**: 16.8 → 0.9 m/s in
  ~2.6 s (≈6.2 m/s²), then waits.
- Before the skitch (472–477 s) the car crept/stopped (skater near it). Other +4403 bits: 0x10 toggles
  often (braking/yield, 353 samples), 0x20 with manoeuvre 3 (pull over, 166), 0x08 on +3424 (28).
- Next: find who sets 0xC0 (store to +4403 with 0xC0 / 0x40 / 0x80 bits) = the skitch attach/detach code,
  and the skater side (skitch state, attach offset).

**AI skaters, user (2026-10-02): AI skaters are all over the map, most at PCU Library; they skate what the
player skates and features around him: grinding rails, small basic tricks off features.** Now traced: category
`aiskater` (hooks_physics.cpp): `SKATEB` = every board's post-physics contact pass `sub_82C07D20` (player and
AI, each frame), per board ≤4/s: contacts w0–w3 / front truck / back truck / deck, deck contact point
(0 in the air), deck velocity. PCU Library check (60 s): player board 469434F0 + ~12 other boards; contact
patterns 0000000 air 367, 1111000 rolling 362, **0000110 trucks only = grind 9**, 0000001 deck slide 32,
0011000/1100000 manual 37. `SKATER` (GroundAnimation fill sub_82D34150) is event-driven, few lines. The
skater list (index 0 = player, 1.. = AI) is `*(*(ctx+1588)+8)` (vtable +4 count, +12 get; position via
skater+52 component vtable +12): sub_82C74EB8 GetDistanceToNearestAISkater; manager string
`Sk8::AI::TheAISkaterManager` (0x8218A99C), `IsAISkater` 0x820DCD00, `GetAISkatersSetting` 0x8221E670.

**AI skaters' board audio (2026-10-03): the second CONTACT object `0x40C710A0` is the Player slot's 2nd instance, which
retail gives to one NPC skater within 30 m of the camera: `world-npc-skater-audio.md`.**
Earlier note (wrong, kept for the record): **Other skating NPCs: no evidence in this trace.** No tracked LW ped moves at skating speed outside a chase
(PEDXYZ). The two board-contact audio objects (CONTACT 0x40C70C80 / 0x40C710A0) are both the player's:
frames where only the second fires (83.2 s, 277.9 s) show only the player. AI skaters (if they spawn in free
roam) use skater physics, not the LW ped census, so none of our hooks cover them yet: candidates are the
`vlt_ai_skater*` records, AIPATH skate lines (§2) and `ai_*` mood results (ai_spectatecollision,
ai_skaterpresence_chase…). Hook plan: find the AI skater spawner/controller and log its skaters'
positions/states.

## 6d. Long combined session (`all_20261002_160126`, ~10.5 min, light-mode hooks, all 5 categories, NO crash)
Light-mode tracing held under real play (the only crash record is the earlier 15:53 one). 526 of ~390k lines
malformed (mostly empty); parsers skip them (`trace.py` now ignores unparsable MARK/CLOCK/CAPTURE).
**Peds:** 4 chases, 3 takedown attempts / 2 successes, reactions: slamreaction 13, nearbycollisionreaction 7,
nearbyskatertrick 4, bump→angrychase 4, bump→warn 1. **Chase without a preceding warn** on a single bump
(slot 2 at 242.96, slot 13 at 347.44), and slot 1: warn 290.39 → chase 291.49 → takedown 302 → next bump
326.01 → chase immediately. With §5d (a ped re-warned 45 s after its chase) this suggests the "already
warned" memory lasts ~35–45 s; or hard bumps skip the warn (?). **Hand-prop throws:** ThrowHandPropTimer
0.20 / 0.92 s and ThrowHandPropReactionTimer 0.81 s on several peds (throwing held items).
**Skitches (6, vehicle +4403 bits 0xC0):** 17.0 / 14.6 / 18.3 / 22.2 / 3.5 / 21.7 s. Speed at grab 0–17 m/s
(grabbing a stopped car makes it drive), peak 15.6–22.7 m/s (56–82 km/h; one 27 = likely a jump), reached
after 3–11 s, mean 8–14 m/s, 28–281 m per skitch. Release: the car does NOT always brake to a stop
(2 of 7 with the §6c one); others keep driving at 7–19 m/s. Traffic overall: moving median 6.0, p90 14.9 m/s.
**AI skaters (SKATEB):** player board 469434F0 + **72 AI boards** over the session (6 alive > 60 s; 4–14
active per minute; they spawn/despawn around the player). Contact mix, player: rolling 60.5 %, air
27.4 %, manual 5.3 %, slide 2.8 %, grind 0.2 %; AI: rolling 70.5 %, air 15.2 %, manual 6.0 %, slide 2.7 %,
grind 0.6 %. Speed |v| moving: player median 8.05 / p90 13.3 m/s; AI median 8.2 / p90 11.8 m/s.
**Gap:** SKATEB's position is the deck contact point only (239 of 6,375 AI samples), so "skate the same
features as the player" (user) can't be measured yet → next hook change: log the first touching part's
contact point (wheels/trucks/deck) or the board matrix, then measure AI-to-player distance and shared
features.
**Fixed (2026-10-02):** SKATEB now logs the first touching part's contact point + part index; PCU check:
531 of 885 lines carry a position (rest = air). Sessions before this fix have deck-only points.


## 6e. Speech (measured 2026-10-02, session all_20261002_161849; tool `speech_reads.py` (local tool))
Ped/world speech is NOT in AEMS banks: it streams from `data/audio/english/livingworldspeech.big` (EB v3,
3,014 entries; also maincastspeech / cameramanspeech / announcerspeech; each has `<x>_Events.evt`,
`<x>hdr.big`, `<x>sth.big` read at boot = event tables). 132 clips in ~11 min. Clip names
`<line id>_<voice id>[_<voice>]_<line>`: voices torf1/torm2/torm3 (tourist f/m), busm1/busm2/busw2/busw3
(business), bum2/bum3, joc2/joc3 (jock), tenm2 (teen m), sktf1/sktf3 (skater f), adtf4, grn2, secg2 (security
guard), generic 91. **Trigger → clip (start 0.03 s after the reaction):** nearbyskatertrick → SpecPos_n;
slamreaction → Slam_n; nearbycollisionreaction → SpecCol; StartChase → SpecChs (bystander) + maincast
`906_<pro>_aislam` (pro/AI-skater lines, e.g. Ratt, Kost, Dill); greet/startconversation/returngreet → Shout,
Int_c<n>_<partner> (ped-ped conversation), Rct_c2; cell phone HelCel → ConCell → ByeCel (+ Silence spacers);
bums PanHndle/RandmBum; guard radio RadioChirps / secg2_radio; player grinds → GrindLand / GrindMid; Gasp,
grunt; cameraman `704_40_spot_ots` (own-the-spot). Levels: this session had no `dsp` category (levels 0);
The all-categories session now includes dsp.
Second speech session (all_20261002_163809, 260 s, WITH voice levels, dsp on): bump→warn → `501_<voice>_Warn_n`
(also for warn+taze, e.g. 501_49_adtf4_Warn_n); collision nearby → SpecCol / `HImpRct_n` (high-impact
reaction) / Gasp; slam nearby → Slam_n / `Rct_c14_tour`; maincast pro lines around the player (`104_21_Ladd_Slam`,
`130_21_Ladd_col`, `101_09_Ratt_pos`). Levels (GAIN×SEND, linear) in its report.md: Skate_Collisions median
0.168 / p90 0.379, sk8_foley 0.244, ped footsteps fstep_livingworld median 0.005 / p90 0.072 (distance),
skater shoes 0.014 / 0.146, sirens up to 0.26, people_yelling up to 0.20.

## 6f. Session all_20261002_164620 (8.3 min, all categories incl. dsp, single writer: 522,380 lines, 0 malformed, NO crash)
- **Peds:** slamreaction 17, nearbycollisionreaction 16, nearbyskatertrick 8, bump→warn+taze 4, →flee 3,
  →warn 2; no chases this time; 3 tazer hits on the player (TazerPlayerDropTime).
- **Speech by trigger (speech_reads.py):** bump→warn → Warn_n; female warn+taze → Warn_f; bump→flee → ChsFlee;
  slam nearby → Slam_n; collision nearby → SpecCol (+ grunt/Gasp); trick nearby → SpecPos_n; greet/conversation
  → Int_c14_tour, Shout, grunt, Gasp, PanHndle (bums); ambient (no reaction nearby): phone HelCel/ConCell/ByeCel,
  guard radio RadioChirps/secg1_radio, pro lines (Duff_pos, Duff_AiSlam, Cole_col). 168 clips.
- **Skitches:** 5 (1.6–14.1 s). Traffic moving median 6.0 / p90 14.7 m/s.
- **AI skaters (positions valid now):** 54 boards; distance to player median 76 m (p10 27, p90 108), within
  10 m only 2 %; their grinds/slides never within 2 m of the player's grind/slide spots. **User: they do not
  always skate the same thing as you, but if you skate one spot over and over, an AI skater may come
  to hit that spot as well.** Measured: at 5 of 6 spots the player hit ≥3 times, an AI came
  within 8 m (e.g. (−174, 650): player 6 hits 138–165 s, an AI 13 samples within 8 m rolling/manual; (−400, 429):
  player 342–346 s, AIs arrived 386 s and 397 s). Not yet seen: an AI grinding the same feature. Next: find the
  AI skater's spot/target selection (Sk8::AI::TheAISkaterManager, AIPATH skate lines §2, `nearbysession`
  mood / session spots) and hook its target choice.
- Levels: the report of recomp session `all_20261002_164620` (GAIN 90,231 / SEND 77,650).
- **All sessions use the user's 100 % completion save** (the user: they skate all over): AI skater density
  (4–14 around the player, 54–72 per session) may depend on progression; check the AI skater spawn rules
  (`GetAISkatersSetting` 0x8221E670) before porting the counts.
- Traffic/world levels (GAIN×SEND, linear; median / p90): Traffic_Horn 115 starts 0.000 / 0.207, Traffic_Skid
  139: 0.003 / 0.164, Tire_Squeals 0.018 / 0.088, car_alarms 0.024 / 0.516, truck horns ~0.02 / 0.06–0.15,
  engines C00_heavy01 0.011 / 0.088, C01_family01 0.008 / 0.062, C03_sports01 0.005 / 0.085, C04_taxi01
  0.016 / 0.079, C05_truck01 0.011 / 0.096, C06_sports02 0.003 / 0.038, C07_family02 0.008 / 0.076; ped
  footsteps fstep_livingworld 0.010 / 0.086; sirens Siren_city_* up to 0.19.

## 7. Hook plan (recomp trace, kind prefix NPC*/TRAF*/SEC*)
1. NPCPOP: sub_826B7530 / 7760 exit: speed, lerped ranges, census circle; sub_826B9940 / 9B90 entry
   (f1/f2 ring, r7 fill flag) + spawn result; sub_826BA8B0 / BAAB8 culls (id, distance). → population,
   spawn/despawn radii, rates.
2. NPCMOOD: sub_82E41EB0 / sub_82BFE010: entity, event, chosen result, roll → reaction probabilities.
3. SECCHASE: sub_826AC668 (escape), sub_826AC898 (rest), sub_826AD690, sub_826AD3F8 results;
   sub_82E40940 timer sets (r4 index, f1 length; indices 1–4, 15, 25, 26) → chase timings per guard.
4. NPCSEE: sub_82E26F38 result with distance/angle → perception.
5. NPCSTATE: sub_8269F6A8 (suggested speed per state), factories InterceptChasee 0x826C75A8, Scatter
   0x826C8618, PedestrianColliding 0x826C5D90 → state timeline.
6. SECTAKE: sub_826A49D8 / sub_826A4E50 / sub_82E3C000 → takedown success rule.
7. TRAFSTATE: vehicle slot-13 handlers: id, state, speed, +3408, +4396/+4380; sub_82C41CD0 exit.
8. TRAFSND / TRAFLIGHT: horn/skid constructors, alarm +3716, sub_82E156D8.
9. Stream: sub_82C9B3A0 / 82C9B250 / 82C9B4F0: which road/nav/AIPATH blobs load where.
Guard every read with Readable(). Short scripted runs: stand still in a census area (population), bump a
ped 3× (mood), skate past a guard (chase), stand on a road (traffic).

## 7b. Live trace, first pass (2026-10-02, `hooks_npc.cpp`, recomp session `npc_hooks`)
Run: Locations Midtown_Promenade then Hotel_District, standing still 45 s each (`scripts/npc_hooks.txt`,
`SKATE3_TRACE=npc,world`, 147 s, no crash). Raw-register hooks; summary via
`tools/recomp-trace/first_pass.py <trace> [KIND…]`. Measured (retail code, recomp timing):
- **Ped ring spawn `sub_826B9940`: f1 = 50 m, f2 = 60 m on every call** (free skate; the 8–80 m ring is the
  initial populate, r7 = 1, never seen here: teleports didn't use it). Called ~12×/s from the census tick
  (caller 826B7740 ← 826B73C8 ← 826BDFB8 ← 82859F6C); returns a ped pointer on 47 of 1,627 calls (~3 %).
- **Placed ped spawn `sub_826B90F8`**: same 50/60, returns 1 on 81 of 1,626 calls.
- **Ped cull `sub_826BA8B0`: f1 = 70 m** (1,646 of 1,675; 90 m on 12 calls, ? which mode).
- **Vehicles: spawn ring 80–100 m (`sub_826B9B90`), cull 110 m (`sub_826BAAB8`).**
- **Mood `sub_82E41EB0`**: r3 = ped (13 peds seen), f1 = 0.1 (step, mostly), returns 1 on 63 of 1,255.
- **Perception `sub_82E26F38`**: r3 = ped, f1 = range by caller: **20 m** (caller …82E3DAC4, presence),
  **30 m** (…82E38028), **10 m** (…82E3D224); returns seen on 84 %.
- **Timers `sub_82E40940`** (r3 = ped, r4 = index, f1 = length): **index 26 = 0.1 s and index 25 = 0.5 s,
  re-armed on every ped continuously** (presence monitor; 25 matches "presence every 0.5 s"); index 31 and
  8 (5 s) rarer. So the "chase timer" indices 25/26 are general periodic timers, not chase-only.
- **Suggested speed `sub_8269F6A8`** returned f1 = 3.0 on every call (? check that f1 is its result).
- **States while standing still:** PedestrianColliding ×5, Scatter ×2, InterceptChasee ×2 (factories called
  from 8241C06C ← 82C16424/82416114: the state-graph runtime).
- **AI stream blobs** (`sub_82C9B250/B4F0/B3A0`): r6 = asset type **0x00EB0027 (26), 0x00EB0014 (24),
  0x00EB0013 (15)**, r8 = 0x34, f1 = streaming distance (13–36). Map these types to road/nav/AIPATH next.
- No security chase calls (SECCHASE/SECTAKE) on this route: expected: Skate 3 has no active guards (§5).
- Trace writer: a few lines were interleaved (hooks fire on several threads; `audio_trace::line` isn't
  atomic per line). `first_pass.py` skips malformed lines.
Next: name the fields (spawn result → ped type/category, cull distances, mood event/result), a guard-area
run (security), a bump run (pad script pushing into a ped: mood → warn/chase).

## 8. Export for our engine
Census layers + census/category/entity records (done as text), road graphs (type 0x13) per district,
waypoint groups (0x1A), AIPATH lines (0x14) for AI skaters, NavPower meshes (0x27; format unknown;
probably replace with our own navmesh from collision), ai_skater_profiles trick weights, chase/
perception/mood records, vehicle characteristics/drivers. State graphs: re-implement the structure in our
own code (don't ship the XML).

## 9. Prior work / reuse
- Nothing upstream (PRs, issues) on retail NPCs, traffic AI or guards. Upstream main `crates/skate-dynamics`
  (Rapier island: spawn/remove bodies, convex colliders, kinematic proxies, spring rays, joints; cars
  composed in Lua) + `mods/Skyline_Drive_Mod`, branch `skyline-driving-update`; @andrewnakas fork
  `mx/vehicle`, `mx/engine`, PR #1 "Mx/audio engine vehicles": all player-driven. Reusable: traffic car
  bodies as kinematic proxies on skate-dynamics (skater collision), engine-sound work for
  `aud_traffic_engine`. Upstream main is GPLv3 (`GPL-3.0-only`, since 2026-10-03, `4488651`); the
  unmerged fork branches and PR #1 have no licence of their own: describe, don't copy.


## Tools (local research scripts, mostly not published; `veh_trace.py`, `first_pass.py`, `callctx.sh`,
`big_list.py` and `sim_types.py` are in `tools/`)
- `img_strings.py <regex>`, `img_range.py a b`: strings in the TU3 image.
- `hashxref.py <hex64|name>`: code sites that build a 64-bit attribute hash (lis/ori halves + rldimi).
- `vlt_resolve.py <class-regex>`: resolve hashed classes/records/fields of skater-collections.json →
  `vlt_<class>.json` (arrays kept whole as `items`); cache `name_ids.json`.
- `fieldxref.py <class>…`: per field, values per record and reading code sites.
- `census_dump.py [--cats]`: census → category group → categories → entities.
- `census_layers.py [DIST…]`: census record per painted area from the world-painter layers.
- `rwtypes.py [regex]`: RW object type id table. `sim_dump.py DIST tile type…`: save stream objects.
- `big_list.py <big> [regex] [--out dir]`: list/extract .big entries.
- Added later: `ai_registry.py`, `aireg.py`, `reg_table.py`, `reg_vtables.py` (graph name → factory →
  handlers), `mood_dump.py`, `guard_areas.py`, `vlt_show.py`, `vlt_arrays.py`, `callctx.sh`,
  `vcalls.py`, `castcalls.py`.
- Disposable data: `name_ids.json` (9 MB cache), `disc/data/db/skatercollections.bin`
  (1 MB).
