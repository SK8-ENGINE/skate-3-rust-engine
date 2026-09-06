//! Native SetPhysicsState82DB8540 publication, followed by owned Enter/Exit.
use super::*;
use skate_core::{physics::board_toolkit::BoardToolkit, player::lifecycle::*};

struct Calls;
impl PhysicalStateCalls for Calls {
    fn get_type(&mut self, state: StateBinding) -> PhysicalStateId {
        state.state
    }
    //Host lifecycle publication is completed below before applying physical
    //effects. These supported Exit methods do not inspect the active binding.
    fn exit(&mut self, call: StateCall) {
        assert!(matches!(
            call.state.state,
            PhysicalStateId::Sleeping
                | PhysicalStateId::PhysicsGround
                | PhysicalStateId::PhysicsAir
                | PhysicalStateId::KnownAir
                | PhysicalStateId::GroundAnimation
                | PhysicalStateId::SlideGround
                | PhysicalStateId::WipeoutGround
                | PhysicalStateId::Teleporting
        ));
    }
    fn enter(&mut self, call: StateCall) {
        assert!(matches!(
            call.state.state,
            PhysicalStateId::PhysicsGround
                | PhysicalStateId::PhysicsAir
                | PhysicalStateId::KnownAir
                | PhysicalStateId::GroundAnimation
                | PhysicalStateId::SlideGround
                | PhysicalStateId::WipeoutGround
                | PhysicalStateId::Teleporting
        ));
    }
}
struct InactiveController;
impl SkateboardControllerActions for InactiveController {
    fn hold_skateboard(&mut self) {
        unreachable!("active controller rejected before transition")
    }
    fn let_go_of_skateboard(&mut self) {
        unreachable!("active controller rejected before transition")
    }
}
pub(super) fn set(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
    requested: PhysicalStateId,
) -> Result<(), String> {
    skater.player_state.requested_state = requested;
    let current = skater.player_state.current();
    if current == requested {
        return Ok(());
    }
    //Keep the actual selector request for the coordinator; do not publish a
    //state whose physical Enter/Exit owner has not been connected.
    let supported = match current {
        PhysicalStateId::Sleeping => requested == PhysicalStateId::PhysicsGround,
        PhysicalStateId::PhysicsGround
        | PhysicalStateId::PhysicsAir
        | PhysicalStateId::KnownAir
        | PhysicalStateId::GroundAnimation
        | PhysicalStateId::SlideGround
        | PhysicalStateId::WipeoutGround
        | PhysicalStateId::Teleporting => matches!(
            requested,
            PhysicalStateId::PhysicsGround
                | PhysicalStateId::PhysicsAir
                | PhysicalStateId::KnownAir
                | PhysicalStateId::GroundAnimation
                | PhysicalStateId::SlideGround
                | PhysicalStateId::WipeoutGround
                | PhysicalStateId::Teleporting
        ),
        _ => false,
    };
    if !supported {
        return Err(format!(
            "Physical state transition {current:?} -> {requested:?} requires its native Enter/Exit production adapter"
        ));
    }
    if skater.skateboard_controller.fields.system_on_452 {
        return Err(
            "Ground transition requires active SkateboardController::LetGoOfSkateboard".into(),
        );
    }
    let p = &mut skater.player_input.processed;
    let player = &mut skater.player_input.player;
    let mut data = StateChangeData {
        player: PlayerStateChangeFields {
            word_1312: player.state_count_1312,
            previous_category_latch_1336: player.state_value_1336,
            scalar_1344: player.state_timer_1344,
        },
        processed: ProcessedStateChangeFields {
            flags_2480: p.flags_2480,
            requested_state_2500: requested as u32,
            previous_state_2504: p.state_2504,
            current_state_2508: p.state_2508,
            current_category_2512: p.category_2512,
            previous_category_2516: p.category_2516,
            previous_category_latch_2520: p.player_state_value_2520,
            word_2564: p.state_count_2564,
            scalar_2664: p.state_timer_2664,
        },
        skateboard_controller: skater.skateboard_controller.fields,
    };
    skater
        .player_state
        .lifecycle
        .set_physics_state(
            requested as u32,
            &mut data,
            &mut Calls,
            &mut InactiveController,
        )
        .map_err(|e| format!("Unknown physical state {}", e.0))?;
    player.state_count_1312 = data.player.word_1312;
    player.state_value_1336 = data.player.previous_category_latch_1336;
    player.state_timer_1344 = data.player.scalar_1344;
    p.flags_2480 = data.processed.flags_2480;
    p.state_2504 = data.processed.previous_state_2504;
    p.state_2508 = data.processed.current_state_2508;
    p.category_2512 = data.processed.current_category_2512;
    p.category_2516 = data.processed.previous_category_2516;
    p.player_state_value_2520 = data.processed.previous_category_latch_2520;
    p.state_count_2564 = data.processed.word_2564;
    p.state_timer_2664 = data.processed.scalar_2664;
    skater.skateboard_controller.fields = data.skateboard_controller;
    if skater.player_input.toolkit.is_none() {
        skater.player_input.toolkit = Some(BoardToolkit::from_board(
            &physics.board,
            p.flags_2468,
            p.scalar_2612,
            p.vectors_464_480_496_512_528[0].map(f32::from_bits),
            [0.0, 1.0, 0.0, 0.0],
        ));
    }
    //Native82DB8540 publishes Processed state/history BEFORE old Exit. The
    //same retained objects then receive Exit followed by the new Enter.
    match current {
        PhysicalStateId::PhysicsGround => super::super::ground_exit::exit(physics, skater),
        PhysicalStateId::PhysicsAir => super::super::air_phase::exit(skater),
        PhysicalStateId::KnownAir => {
            super::super::known_air::exit(physics, skater, requested as u32)?
        }
        PhysicalStateId::GroundAnimation => super::super::ground_animation::exit(physics, skater)?,
        PhysicalStateId::SlideGround => super::super::slide_state::exit(physics, skater)?,
        PhysicalStateId::WipeoutGround => super::super::wipeout_states::exit(physics, skater),
        PhysicalStateId::Sleeping | PhysicalStateId::Teleporting => {} //82B61BB8.
        _ => unreachable!("state support checked before publication"),
    }
    match requested {
        PhysicalStateId::PhysicsGround => super::super::ground_phase::enter(physics, skater),
        PhysicalStateId::PhysicsAir => super::super::air_phase::enter(physics, skater),
        PhysicalStateId::KnownAir => super::super::known_air::enter(physics, skater),
        PhysicalStateId::GroundAnimation => super::super::ground_animation::enter(physics, skater),
        PhysicalStateId::SlideGround => super::super::slide_state::enter(physics, skater),
        PhysicalStateId::WipeoutGround => super::super::wipeout_states::enter(physics, skater),
        PhysicalStateId::Teleporting => { skater.teleport_state.enter(); Ok(()) },
        _ => unreachable!("state support checked before publication"),
    }
}
