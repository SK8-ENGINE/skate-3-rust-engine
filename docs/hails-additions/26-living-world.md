# 26: Living world: NPC skaters, pedestrians and traffic

Branch: `world/living-world` (built on all the audio work, #32, which it publishes sounds into). One upstream PR
for all of it (the user's decision, 2026-10-04: this work is one PR; the user added traffic to the living
world work the same day). Details: [`living-world/`](living-world/) (designs and the data formats of milestone 1).

**Status (2026-10-10): feature complete for this PR, to be play-tested by Hails.** The scope is frozen; the
remaining step is the user's play session and the fixes it shows. Work for later PRs is listed in the #52 description
under "Follow-ups".

This doc is split into the files below so each topic can be read on its own. Every section keeps its heading, so
an old link `26-living-world.md#<section>` becomes `<file>#<section>` with the same anchor. New sections go into
the file of their topic (and get a line here).

| File | Topic | Contains |
|---|---|---|
| [26a](26a-living-world-overview.md) | Overview, plan and modding | Start here. The problem, what retail does (census, lines, peds, traffic), the milestone plan inside the one PR, the mod surface (`sdk.world.set_tuning` domains, `living_world` events), verification, credits and the open questions. |
| [26b](26b-living-world-population.md) | Data and the population core | Milestone 1 (the `livingworld` setup group, AIPATH lines, tables, census grids, ped / prop GLBs) and milestone 2 (`skate-core::living_world`, `LivingWorldPlugin`), plus the population fixes: NPCs vanishing in view, the NPC draw distance option, the startup freeze, the census following the board, and the hidden-board frame drop. |
| [26c](26c-living-world-npc-skaters.md) | NPC skaters: replay tier and animation | Milestone 3 (NPC skaters replaying recorded lines) and the fixes to how they look and sound: fading at line ends, animation snapping and popping, jitter, switching sides, and silent landings from the voice cap. |
| [26d](26d-living-world-skater-stance.md) | NPC skaters: stance and riding direction | Riding backwards (fix 23) and its correction, then the retail stance ports: fakie drawing, natural stance, trick-clip stance toggles, and the turn rate / pro stance / off-board steer research. The largest single topic. |
| [26e](26e-living-world-skater-ai.md) | NPC skaters: AI and simulated tier | The M5 ports (AI board path steering, trick choice) and M7 steps 1 to 3: per-skater physics context, the AI record driving the physics, and simulated NPC skaters in the game. |
| [26f](26f-living-world-peds.md) | Pedestrians: body, navigation and look | Peds milestones M2 (the ped body) and M3 (navigation), plus the fixes to how peds stand and render: upside down in left turns, warped bone frames, walking in place, clothes in mask colors, and peds floating in the air (the render height search). |
| [26g](26g-living-world-ped-interactions.md) | Pedestrians: props and skater contact | Peds (and NPC skaters, fix 19) walking through props, peds walking through a held prop, and the skater knocking peds down or making them stumble. |
| [26h](26h-living-world-traffic.md) | Traffic | Milestones V0 to V3 (vehicle data, road graph / lane cursor / signals / junction entry, the vehicle census, cars on screen) and the fixes: cars flying off, car shadows from bridges, and the retail reaction when cars hit peds. |
| [26i](26i-living-world-props.md) | Props | Dynamic props and the held / moved prop: props pulled toward the player, moving a held prop (stick directions, skater inside the prop, grab flicker), the board stuck inside a prop, dragged props sinking, and the Move Object port. The dynamic props themselves (movable DMOs from upstream PR #15) are doc 27. |

## Sections per file

### 26a: Overview, plan and modding

- [Problem](26a-living-world-overview.md#problem)
- [What retail does](26a-living-world-overview.md#what-retail-does)
- [Verification](26a-living-world-overview.md#verification)
- [Plan (milestones inside the one PR)](26a-living-world-overview.md#plan-milestones-inside-the-one-pr)
- [Modding](26a-living-world-overview.md#modding)
- [Credits](26a-living-world-overview.md#credits)
- [Open questions](26a-living-world-overview.md#open-questions)

### 26b: Data and the population core

- [Change (milestone 1: data)](26b-living-world-population.md#change-milestone-1-data)
- [Change (milestone 2: population core)](26b-living-world-population.md#change-milestone-2-population-core)
- [NPCs vanishing in view (leave fade, census center), 2026-10-05](26b-living-world-population.md#npcs-vanishing-in-view-leave-fade-census-center-2026-10-05)
- [NPC draw distance (QoL, not retail)](26b-living-world-population.md#npc-draw-distance-qol-not-retail)
- [Startup freeze from the first-play fixes, 2026-10-05](26b-living-world-population.md#startup-freeze-from-the-first-play-fixes-2026-10-05)
- [Population follows the board, not the player, 2026-10-05](26b-living-world-population.md#population-follows-the-board-not-the-player-2026-10-05)
- [Frame drop with the board thrown away (hidden board scanned the whole map), 2026-10-05](26b-living-world-population.md#frame-drop-with-the-board-thrown-away-hidden-board-scanned-the-whole-map-2026-10-05)
- [Opt-in switches in the PC overlay, 2026-10-11](26b-living-world-population.md#opt-in-switches-in-the-pc-overlay-2026-10-11)
- [Map change: fresh NPC bodies, ids that never repeat, ped state cleared, 2026-10-18](26b-living-world-population.md#map-change-fresh-npc-bodies-ids-that-never-repeat-ped-state-cleared-2026-10-18)

### 26c: NPC skaters: replay tier and animation

- [Change (milestone 3: NPC skaters, replay tier)](26c-living-world-npc-skaters.md#change-milestone-3-npc-skaters-replay-tier)
- [NPC skaters fading out near the player (line end), 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-fading-out-near-the-player-line-end-2026-10-05)
- [NPC skaters snapping between animations, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-snapping-between-animations-2026-10-05)
- [NPC skaters jittery while skating, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-jittery-while-skating-2026-10-05)
- [NPC skaters switching the side they stand on, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-switching-the-side-they-stand-on-2026-10-05)
- [NPC skaters popping between animations, tricks not playing, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-popping-between-animations-tricks-not-playing-2026-10-05)
- [Silent landings (NPC skater sounds filled the voice cap), 2026-10-07](26c-living-world-npc-skaters.md#silent-landings-npc-skater-sounds-filled-the-voice-cap-2026-10-07)
- [NPC Skater Physics: hitches, skaters vanishing up close, path projection (2026-10-13)](26c-living-world-npc-skaters.md#npc-skater-physics-hitches-skaters-vanishing-up-close-path-projection-2026-10-13)
- [NPC Skater Physics: bails (2026-10-13 night)](26c-living-world-npc-skaters.md#npc-skater-physics-bails-2026-10-13-night)
- [NPC Skater Physics: bails right after a placement (2026-10-15)](26c-living-world-npc-skaters.md#npc-skater-physics-bails-right-after-a-placement-2026-10-15)

### 26d: NPC skaters: stance and riding direction

- [NPC skaters riding backwards (fix 23), 2026-10-08](26d-living-world-skater-stance.md#npc-skaters-riding-backwards-fix-23-2026-10-08)
  - [Fix 23 corrected (2026-10-08)](26d-living-world-skater-stance.md#fix-23-corrected-2026-10-08)
  - [NPC skater fakie drawing (stance port, 2026-10-08)](26d-living-world-skater-stance.md#npc-skater-fakie-drawing-stance-port-2026-10-08)
  - [NPC skater turn rate, pro stance and off-board steer (research, 2026-10-08)](26d-living-world-skater-stance.md#npc-skater-turn-rate-pro-stance-and-off-board-steer-research-2026-10-08)
  - [NPC skater natural stance (port, 2026-10-08)](26d-living-world-skater-stance.md#npc-skater-natural-stance-port-2026-10-08)
  - [NPC skater trick-clip stance toggles (port, 2026-10-08)](26d-living-world-skater-stance.md#npc-skater-trick-clip-stance-toggles-port-2026-10-08)

### 26e: NPC skaters: AI and simulated tier

- [NPC skater steering: the AI board path (M5 port, 2026-10-08)](26e-living-world-skater-ai.md#npc-skater-steering-the-ai-board-path-m5-port-2026-10-08)
- [NPC skater trick choice (M5 port, 2026-10-08)](26e-living-world-skater-ai.md#npc-skater-trick-choice-m5-port-2026-10-08)
- [Simulated NPC skaters: per-skater physics context (M7 step 1, 2026-10-08)](26e-living-world-skater-ai.md#simulated-npc-skaters-per-skater-physics-context-m7-step-1-2026-10-08)
- [Simulated NPC skaters: the AI record drives the physics (M7 step 2, 2026-10-08)](26e-living-world-skater-ai.md#simulated-npc-skaters-the-ai-record-drives-the-physics-m7-step-2-2026-10-08)
- [Simulated NPC skaters in the game (M7 step 3, 2026-10-08)](26e-living-world-skater-ai.md#simulated-npc-skaters-in-the-game-m7-step-3-2026-10-08)
- [NPC skater obstacle avoider (M5 port, 2026-10-09)](26e-living-world-skater-ai.md#npc-skater-obstacle-avoider-m5-port-2026-10-09)
- [NPC skater proxies in retail's collision groups (2026-10-09)](26e-living-world-skater-ai.md#npc-skater-proxies-in-retails-collision-groups-2026-10-09)
- [Simulated NPC skaters bail and respawn (M7, 2026-10-09)](26e-living-world-skater-ai.md#simulated-npc-skaters-bail-and-respawn-m7-2026-10-09)
- [Simulated NPC skaters do their recorded jumps and tricks (M7, 2026-10-09)](26e-living-world-skater-ai.md#simulated-npc-skaters-do-their-recorded-jumps-and-tricks-m7-2026-10-09)
- [Avoider mode 7: stepping off and walking back to the line (2026-10-10)](26e-living-world-skater-ai.md#avoider-mode-7-stepping-off-and-walking-back-to-the-line-2026-10-10)
- [Avoider modes 6 and 7: the controller sub-modes (2026-10-09)](26e-living-world-skater-ai.md#avoider-modes-6-and-7-the-controller-sub-modes-2026-10-09)
- [The stuck tracker: stalled NPC skaters recover in stages (2026-10-15)](26e-living-world-skater-ai.md#the-stuck-tracker-stalled-npc-skaters-recover-in-stages-2026-10-15)
- [Avoider: steer around a partial blocker, and the real closing speed (2026-10-16)](26e-living-world-skater-ai.md#avoider-steer-around-a-partial-blocker-and-the-real-closing-speed-2026-10-16)
- [Recorded grinds and slides: the input schedule and the takeoff board yaw (2026-10-16)](26e-living-world-skater-ai.md#recorded-grinds-and-slides-the-input-schedule-and-the-takeoff-board-yaw-2026-10-16)
- [Bumps: the player and simulated NPC skaters knock each other down (2026-10-16)](26e-living-world-skater-ai.md#bumps-the-player-and-simulated-npc-skaters-knock-each-other-down-2026-10-16)
- [Bumped NPC skaters: no anger, a collide line; pros stay calm (2026-10-16)](26e-living-world-skater-ai.md#bumped-npc-skaters-no-anger-a-collide-line-pros-stay-calm-2026-10-16)
- [A dropped trick slot shows no trick pose (2026-10-17)](26e-living-world-skater-ai.md#a-dropped-trick-slot-shows-no-trick-pose-2026-10-17)
- [Simulated NPC skaters hold a manual on its side (2026-10-17)](26e-living-world-skater-ai.md#simulated-npc-skaters-hold-a-manual-on-its-side-2026-10-17)
- [Simulated NPC skaters steer by the effective deck after a flip (2026-10-17)](26e-living-world-skater-ai.md#simulated-npc-skaters-steer-by-the-effective-deck-after-a-flip-2026-10-17)

### 26f: Pedestrians: body, navigation and look

- [Change: peds milestone M2, the ped body](26f-living-world-peds.md#change-peds-milestone-m2-the-ped-body)
- [Change: peds milestone M3, navigation](26f-living-world-peds.md#change-peds-milestone-m3-navigation)
- [Peds standing on their heads in the left turn, 2026-10-05](26f-living-world-peds.md#peds-standing-on-their-heads-in-the-left-turn-2026-10-05)
- [Peds rendered warped (bone frames twisted 90 degrees), 2026-10-05](26f-living-world-peds.md#peds-rendered-warped-bone-frames-twisted-90-degrees-2026-10-05)
- [Peds walking in place at fixed spots, one standing on a wall top, 2026-10-05](26f-living-world-peds.md#peds-walking-in-place-at-fixed-spots-one-standing-on-a-wall-top-2026-10-05)
- [Ped clothes drawn in their mask colors (looked like mixed outfits), 2026-10-05](26f-living-world-peds.md#ped-clothes-drawn-in-their-mask-colors-looked-like-mixed-outfits-2026-10-05)
- [Peds floating in the air all over the map (2026-10-06)](26f-living-world-peds.md#peds-floating-in-the-air-all-over-the-map-2026-10-06)

### 26g: Pedestrians: props and skater contact

- [Peds walking through props, 2026-10-05](26g-living-world-ped-interactions.md#peds-walking-through-props-2026-10-05)
  - [NPC skaters riding through props, 2026-10-05 (fix 19)](26g-living-world-ped-interactions.md#npc-skaters-riding-through-props-2026-10-05-fix-19)
- [Peds walking through a held prop, 2026-10-08](26g-living-world-ped-interactions.md#peds-walking-through-a-held-prop-2026-10-08)
- [Skater hits peds: knock-down or stumble (2026-10-08)](26g-living-world-ped-interactions.md#skater-hits-peds-knock-down-or-stumble-2026-10-08)

- [Ped behavior runtime: the stock ped AI graph on each ped (2026-10-09)](26g-living-world-ped-interactions.md#ped-behavior-runtime-the-stock-ped-ai-graph-on-each-ped-2026-10-09)
- [Ped mood system: wants from the stock mood tables (2026-10-09)](26g-living-world-ped-interactions.md#ped-mood-system-wants-from-the-stock-mood-tables-2026-10-09)
- [Fleeing peds run away from the threat (2026-10-09)](26g-living-world-ped-interactions.md#fleeing-peds-run-away-from-the-threat-2026-10-09)
- [Peds speak from their AI graph (2026-10-09)](26g-living-world-ped-interactions.md#peds-speak-from-their-ai-graph-2026-10-09)
  - [Graph timers: SetSimpleTimer and SimpleTimerExpired (2026-10-09)](26g-living-world-ped-interactions.md#graph-timers-setsimpletimer-and-simpletimerexpired-2026-10-09)
- [Ped chases: chase record, chase groups and the intercept (2026-10-09)](26g-living-world-ped-interactions.md#ped-chases-chase-record-chase-groups-and-the-intercept-2026-10-09)
- [Ped takedowns and chase exhaustion (2026-10-09)](26g-living-world-ped-interactions.md#ped-takedowns-and-chase-exhaustion-2026-10-09)
- [Ped perception, secondary chasers and tazers (2026-10-09)](26g-living-world-ped-interactions.md#ped-perception-secondary-chasers-and-tazers-2026-10-09)
- [Peds greet each other (2026-10-09)](26g-living-world-ped-interactions.md#peds-greet-each-other-2026-10-09)
- [Ped conversations: the plugin runner and the conversation object (2026-10-09)](26g-living-world-ped-interactions.md#ped-conversations-the-plugin-runner-and-the-conversation-object-2026-10-09)
- [Conversation speech, gather timer and abort (2026-10-09)](26g-living-world-ped-interactions.md#conversation-speech-gather-timer-and-abort-2026-10-09)
- [Peds taunt after a takedown (2026-10-09)](26g-living-world-ped-interactions.md#peds-taunt-after-a-takedown-2026-10-09)
- [Ped plugins on world props: data, rolls and the offer scan (groundwork, 2026-10-10)](26g-living-world-ped-interactions.md#ped-plugins-on-world-props-data-rolls-and-the-offer-scan-groundwork-2026-10-10)
- [Ped plugins on world props: motion states and the wiring (2026-10-10)](26g-living-world-ped-interactions.md#ped-plugins-on-world-props-motion-states-and-the-wiring-2026-10-10)
- [Ped plugins on world props: benches, bins and newspaper boxes from the model hotpoints (2026-10-10)](26g-living-world-ped-interactions.md#ped-plugins-on-world-props-benches-bins-and-newspaper-boxes-from-the-model-hotpoints-2026-10-10)
- [Ped hand props: the vending machine can, the newspaper (2026-10-10)](26g-living-world-ped-interactions.md#ped-hand-props-the-vending-machine-can-the-newspaper-2026-10-10)
- [A ped stuck in front of the bench it wants to sit on (2026-10-14)](26g-living-world-ped-interactions.md#a-ped-stuck-in-front-of-the-bench-it-wants-to-sit-on-2026-10-14)
- [Ped hand props: the bin throw, the drop and the released prop (2026-10-10)](26g-living-world-ped-interactions.md#ped-hand-props-the-bin-throw-the-drop-and-the-released-prop-2026-10-10)
- [Angry peds, knocked-down peds on their way to a seat, moved benches and a plugin crash (2026-10-12)](26g-living-world-ped-interactions.md#angry-peds-knocked-down-peds-on-their-way-to-a-seat-moved-benches-and-a-plugin-crash-2026-10-12)
- [Zombie mode: zombies chase the skater (2026-10-13)](26g-living-world-ped-interactions.md#zombie-mode-zombies-chase-the-skater-2026-10-13)
- [Zombie takedowns on a downed skater, NPC skater sim flip-flop (2026-10-13)](26g-living-world-ped-interactions.md#zombie-takedowns-on-a-downed-skater-npc-skater-sim-flip-flop-2026-10-13)
- [Floating peds on coarse navmesh triangles (2026-10-13)](26g-living-world-ped-interactions.md#floating-peds-on-coarse-navmesh-triangles-2026-10-13)
- [Peds give up a chase they cannot reach (2026-10-13)](26g-living-world-ped-interactions.md#peds-give-up-a-chase-they-cannot-reach-2026-10-13)
- [Diagnostics: still peds report what they are doing (2026-10-16)](26g-living-world-ped-interactions.md#diagnostics-still-peds-report-what-they-are-doing-2026-10-16)
- [Standing pros: where the challenge hosts stand (placement export, 2026-10-16)](26g-living-world-ped-interactions.md#standing-pros-where-the-challenge-hosts-stand-placement-export-2026-10-16)
- [IsFacingSkater: a half angle through a cosine, and its distance (2026-10-16)](26g-living-world-ped-interactions.md#isfacingskater-a-half-angle-through-a-cosine-and-its-distance-2026-10-16)
- [A ped that lost its prop no longer stalls a conversation (2026-10-17)](26g-living-world-ped-interactions.md#a-ped-that-lost-its-prop-no-longer-stalls-a-conversation-2026-10-17)
- [A full mood store evicts by priority (2026-10-17)](26g-living-world-ped-interactions.md#a-full-mood-store-evicts-by-priority-2026-10-17)
- [Peds stuck at walls: retail has no give-up (research, 2026-10-17)](26g-living-world-ped-interactions.md#peds-stuck-at-walls-retail-has-no-give-up-research-2026-10-17)
- [On foot: the player passes a ped; the ped reacts to its part's speed after the solve (2026-10-18)](26g-living-world-ped-interactions.md#on-foot-the-player-passes-a-ped-the-ped-reacts-to-its-parts-speed-after-the-solve-2026-10-18)

### 26h: Traffic

- [Change: milestone V0, vehicle data](26h-living-world-traffic.md#change-milestone-v0-vehicle-data)
- [Change: milestone V1, road graph, lane cursor, traffic signals and junction entry](26h-living-world-traffic.md#change-milestone-v1-road-graph-lane-cursor-traffic-signals-and-junction-entry)
- [Change: milestone V2, the vehicle census](26h-living-world-traffic.md#change-milestone-v2-the-vehicle-census)
- [Change: milestone V3, cars on screen](26h-living-world-traffic.md#change-milestone-v3-cars-on-screen)
- [Cars flying off, population gone, 2026-10-05](26h-living-world-traffic.md#cars-flying-off-population-gone-2026-10-05)
- [Car shadows from a bridge printed on the ground below, 2026-10-08](26h-living-world-traffic.md#car-shadows-from-a-bridge-printed-on-the-ground-below-2026-10-08)
- [Cars hit peds: retail reaction, 2026-10-08](26h-living-world-traffic.md#cars-hit-peds-retail-reaction-2026-10-08)
- [Skitching: research and the tow spring (2026-10-09, groundwork)](26h-living-world-traffic.md#skitching-research-and-the-tow-spring-2026-10-09-groundwork)
- [Cars knock the skater down, and brake when hit (V5 start, 2026-10-09)](26h-living-world-traffic.md#cars-knock-the-skater-down-and-brake-when-hit-v5-start-2026-10-09)
- [Traffic: obstacles ahead (V4 look-ahead, 2026-10-09)](26h-living-world-traffic.md#traffic-obstacles-ahead-v4-look-ahead-2026-10-09)
- [Traffic: the horn, and peds running from it (V4, 2026-10-09)](26h-living-world-traffic.md#traffic-the-horn-and-peds-running-from-it-v4-2026-10-09)
- [Skitching step 1: car grab splines in the vehicle data (2026-10-09)](26h-living-world-traffic.md#skitching-step-1-car-grab-splines-in-the-vehicle-data-2026-10-09)
- [Skitching step 4a: the state-104 frame step, and the GRABDATA header fix (2026-10-09)](26h-living-world-traffic.md#skitching-step-4a-the-state-104-frame-step-and-the-grabdata-header-fix-2026-10-09)
- [Skitching step 2: cars in the grab scene (2026-10-09)](26h-living-world-traffic.md#skitching-step-2-cars-in-the-grab-scene-2026-10-09)
- [Skitching step 3: the riding skitch query (2026-10-09)](26h-living-world-traffic.md#skitching-step-3-the-riding-skitch-query-2026-10-09)
- [Skitching step 4b: the hold target and the release impulses (2026-10-09)](26h-living-world-traffic.md#skitching-step-4b-the-hold-target-and-the-release-impulses-2026-10-09)
- [Skitching step 4c: the hold step and the release (2026-10-09)](26h-living-world-traffic.md#skitching-step-4c-the-hold-step-and-the-release-2026-10-09)
- [Skitching step 4d: the along-the-bumper hand chain (2026-10-09)](26h-living-world-traffic.md#skitching-step-4d-the-along-the-bumper-hand-chain-2026-10-09)
- [Skitching step 4e: state 104 in the game (2026-10-09)](26h-living-world-traffic.md#skitching-step-4e-state-104-in-the-game-2026-10-09)
- [Skitching step 5: the held car (2026-10-09)](26h-living-world-traffic.md#skitching-step-5-the-held-car-2026-10-09)
- [Skitching step 4f: the lean (2026-10-09)](26h-living-world-traffic.md#skitching-step-4f-the-lean-2026-10-09)
- [Skitching step 4g: the hands (2026-10-09)](26h-living-world-traffic.md#skitching-step-4g-the-hands-2026-10-09)
- [Skitching step 4h: the skitch animation graph nodes (2026-10-09)](26h-living-world-traffic.md#skitching-step-4h-the-skitch-animation-graph-nodes-2026-10-09)
- [Traffic: the car alarm on parked cars (2026-10-10)](26h-living-world-traffic.md#traffic-the-car-alarm-on-parked-cars-2026-10-10)
- [Grinding on cars (2026-10-14)](26h-living-world-traffic.md#grinding-on-cars-2026-10-14)
- [Skitching step 6: cars slow down for a skater behind them (2026-10-09)](26h-living-world-traffic.md#skitching-step-6-cars-slow-down-for-a-skater-behind-them-2026-10-09)
- [Skitching step 4i: PushSpeed's latch and step (2026-10-09)](26h-living-world-traffic.md#skitching-step-4i-pushspeeds-latch-and-step-2026-10-09)
- [Skitching step 4j: the board's effective transform rows (2026-10-09)](26h-living-world-traffic.md#skitching-step-4j-the-boards-effective-transform-rows-2026-10-09)
- [Skitching step 7: NPC skaters skitch (opt-in, 2026-10-09)](26h-living-world-traffic.md#skitching-step-7-npc-skaters-skitch-opt-in-2026-10-09)
- [Maneuvers groundwork: lane-change passage, stop spots, gap check (2026-10-09)](26h-living-world-traffic.md#maneuvers-groundwork-lane-change-passage-stop-spots-gap-check-2026-10-09)
- [Traffic: the horn's driver fields corrected (2026-10-09)](26h-living-world-traffic.md#traffic-the-horns-driver-fields-corrected-2026-10-09)
- [Traffic: cars change lane (2026-10-09)](26h-living-world-traffic.md#traffic-cars-change-lane-2026-10-09)
- [Traffic: cars pull over, park and pull out (2026-10-09)](26h-living-world-traffic.md#traffic-cars-pull-over-park-and-pull-out-2026-10-09)
- [Car materials: the retail vehicle shaders (2026-10-11)](26h-living-world-traffic.md#car-materials-the-retail-vehicle-shaders-2026-10-11)
- [Skitching session logging (2026-10-15)](26h-living-world-traffic.md#skitching-session-logging-2026-10-15)
- [Skitching crash on the first grab (2026-10-15)](26h-living-world-traffic.md#skitching-crash-on-the-first-grab-2026-10-15)

### 26i: Props

- [Props pulled toward the player, 2026-10-05](26i-living-world-props.md#props-pulled-toward-the-player-2026-10-05)
- [Moving a held prop: every stick direction goes forward, skater inside the prop, grab flicker, 2026-10-05](26i-living-world-props.md#moving-a-held-prop-every-stick-direction-goes-forward-skater-inside-the-prop-grab-flicker-2026-10-05)
- [Board stuck inside a prop, 2026-10-05](26i-living-world-props.md#board-stuck-inside-a-prop-2026-10-05)
- [Dragged props sinking through the floor, 2026-10-07](26i-living-world-props.md#dragged-props-sinking-through-the-floor-2026-10-07)
  - [Move Object port (2026-10-08)](26i-living-world-props.md#move-object-port-2026-10-08)
- [Move Object step 1: props' authored grab splines in the export (2026-10-09)](26i-living-world-props.md#move-object-step-1-props-authored-grab-splines-in-the-export-2026-10-09)
- [Move Object step 2: props in the grab scene (opt-in, 2026-10-09)](26i-living-world-props.md#move-object-step-2-props-in-the-grab-scene-opt-in-2026-10-09)
- [Move Object step 3c: the hand IK (opt-in, 2026-10-10)](26i-living-world-props.md#move-object-step-3c-the-hand-ik-opt-in-2026-10-10)
- [Grinding on props (2026-10-14)](26i-living-world-props.md#grinding-on-props-2026-10-14)
- [Move Object step 3: carrying a prop by its authored grab record (opt-in, 2026-10-09)](26i-living-world-props.md#move-object-step-3-carrying-a-prop-by-its-authored-grab-record-opt-in-2026-10-09)
- [Move Object step 3b: the rebind block, re-grab and hand points (opt-in, 2026-10-09)](26i-living-world-props.md#move-object-step-3b-the-rebind-block-re-grab-and-hand-points-opt-in-2026-10-09)
- [Prop diagnostics for play sessions (2026-10-16)](26i-living-world-props.md#prop-diagnostics-for-play-sessions-2026-10-16)
