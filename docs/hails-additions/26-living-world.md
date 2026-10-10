# 26: Living world: NPC skaters, pedestrians and traffic

Branch: `world/living-world` (built on all the audio work, #32, which it publishes sounds into). One upstream PR
for all of it (user, 2026-10-04: "this work shall be one PR"; traffic added the same day: "add this to the living
world work"). Details: [`living-world/`](living-world/) (designs and the data formats of milestone 1).

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
| [26f](26f-living-world-peds.md) | Pedestrians: body, navigation and look | Peds milestones M2 (the ped body) and M3 (navigation), plus the fixes to how peds stand and render: upside down in left turns, warped bone frames, walking in place, clothes in mask colours, and peds floating in the air (the render height search). |
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
- [NPCs vanishing in view (leave fade, census centre), 2026-10-05](26b-living-world-population.md#npcs-vanishing-in-view-leave-fade-census-centre-2026-10-05)
- [NPC draw distance (QoL, not retail)](26b-living-world-population.md#npc-draw-distance-qol-not-retail)
- [Startup freeze from the first-play fixes, 2026-10-05](26b-living-world-population.md#startup-freeze-from-the-first-play-fixes-2026-10-05)
- [Population follows the board, not the player, 2026-10-05](26b-living-world-population.md#population-follows-the-board-not-the-player-2026-10-05)
- [Frame drop with the board thrown away (hidden board scanned the whole map), 2026-10-05](26b-living-world-population.md#frame-drop-with-the-board-thrown-away-hidden-board-scanned-the-whole-map-2026-10-05)

### 26c: NPC skaters: replay tier and animation

- [Change (milestone 3: NPC skaters, replay tier)](26c-living-world-npc-skaters.md#change-milestone-3-npc-skaters-replay-tier)
- [NPC skaters fading out near the player (line end), 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-fading-out-near-the-player-line-end-2026-10-05)
- [NPC skaters snapping between animations, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-snapping-between-animations-2026-10-05)
- [NPC skaters jittery while skating, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-jittery-while-skating-2026-10-05)
- [NPC skaters switching the side they stand on, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-switching-the-side-they-stand-on-2026-10-05)
- [NPC skaters popping between animations, tricks not playing, 2026-10-05](26c-living-world-npc-skaters.md#npc-skaters-popping-between-animations-tricks-not-playing-2026-10-05)
- [Silent landings (NPC skater sounds filled the voice cap), 2026-10-07](26c-living-world-npc-skaters.md#silent-landings-npc-skater-sounds-filled-the-voice-cap-2026-10-07)

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

### 26f: Pedestrians: body, navigation and look

- [Change: peds milestone M2, the ped body](26f-living-world-peds.md#change-peds-milestone-m2-the-ped-body)
- [Change: peds milestone M3, navigation](26f-living-world-peds.md#change-peds-milestone-m3-navigation)
- [Peds standing on their heads in the left turn, 2026-10-05](26f-living-world-peds.md#peds-standing-on-their-heads-in-the-left-turn-2026-10-05)
- [Peds rendered warped (bone frames twisted 90 degrees), 2026-10-05](26f-living-world-peds.md#peds-rendered-warped-bone-frames-twisted-90-degrees-2026-10-05)
- [Peds walking in place at fixed spots, one standing on a wall top, 2026-10-05](26f-living-world-peds.md#peds-walking-in-place-at-fixed-spots-one-standing-on-a-wall-top-2026-10-05)
- [Ped clothes drawn in their mask colours (looked like mixed outfits), 2026-10-05](26f-living-world-peds.md#ped-clothes-drawn-in-their-mask-colours-looked-like-mixed-outfits-2026-10-05)
- [Peds floating in the air all over the map (2026-10-06)](26f-living-world-peds.md#peds-floating-in-the-air-all-over-the-map-2026-10-06)

### 26g: Pedestrians: props and skater contact

- [Peds walking through props, 2026-10-05](26g-living-world-ped-interactions.md#peds-walking-through-props-2026-10-05)
  - [NPC skaters riding through props, 2026-10-05 (fix 19)](26g-living-world-ped-interactions.md#npc-skaters-riding-through-props-2026-10-05-fix-19)
- [Peds walking through a held prop, 2026-10-08](26g-living-world-ped-interactions.md#peds-walking-through-a-held-prop-2026-10-08)
- [Skater hits peds: knock-down or stumble (2026-10-08)](26g-living-world-ped-interactions.md#skater-hits-peds-knock-down-or-stumble-2026-10-08)

- [Ped behaviour runtime: the stock ped AI graph on each ped (2026-10-09)](26g-living-world-ped-interactions.md#ped-behaviour-runtime-the-stock-ped-ai-graph-on-each-ped-2026-10-09)
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

### 26i: Props

- [Props pulled toward the player, 2026-10-05](26i-living-world-props.md#props-pulled-toward-the-player-2026-10-05)
- [Moving a held prop: every stick direction goes forward, skater inside the prop, grab flicker, 2026-10-05](26i-living-world-props.md#moving-a-held-prop-every-stick-direction-goes-forward-skater-inside-the-prop-grab-flicker-2026-10-05)
- [Board stuck inside a prop, 2026-10-05](26i-living-world-props.md#board-stuck-inside-a-prop-2026-10-05)
- [Dragged props sinking through the floor, 2026-10-07](26i-living-world-props.md#dragged-props-sinking-through-the-floor-2026-10-07)
  - [Move Object port (2026-10-08)](26i-living-world-props.md#move-object-port-2026-10-08)
- [Move Object step 1: props' authored grab splines in the export (2026-10-09)](26i-living-world-props.md#move-object-step-1-props-authored-grab-splines-in-the-export-2026-10-09)
- [Move Object step 2: props in the grab scene (opt-in, 2026-10-09)](26i-living-world-props.md#move-object-step-2-props-in-the-grab-scene-opt-in-2026-10-09)
- [Move Object step 3c: the hand IK (opt-in, 2026-10-10)](26i-living-world-props.md#move-object-step-3c-the-hand-ik-opt-in-2026-10-10)
