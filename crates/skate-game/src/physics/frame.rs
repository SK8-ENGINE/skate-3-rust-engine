//! One production tick: controller graphs, physical input/state, shared solve,
//! physical animation output and the normal gameplay camera.
use super::{
    GamePhysics, PlayerControls, SkaterRuntime, animation_phase, camera_output, ground_phase,
    input_phase, player_state, solve,
};
use crate::{camera::CameraRuntime, graph_runtime::StockGraphs};
use skate_core::{input::controller::ActionMap, math::Vector3, physics::{board::BodyId, phase::{PhysicsEvent, PhysicalOutputSnapshot}}};

pub(super) fn advance(
    physics: &mut GamePhysics,
    skater: &mut SkaterRuntime,
    controls: &mut PlayerControls,
    graphs: &StockGraphs,
    actions: &mut dyn ActionMap,
    input_available: bool,
    camera: &mut CameraRuntime,
) -> Result<(), String> {
    let tick = physics.ticks;
    physics.exchange = super::SimulationExchange::new(tick);
    //SimController8285C968 dispatches the preceding tick's camera messages
    //before simulation. Keep End/Begin ordering when both occur in one update.
    for request in camera.simulation_rate_requests.drain(..) {
        physics.clock.apply(request)?;
    }
    if physics.ticks == 0 {
        player_state::initialize(physics, skater)?;
        //Ctor82DB3008 enters Ground without ProcessOutput. Keep the constructed
        //State/FilteredState zeros (82DE3BE8/82DE5588) until the completed tick.
        //Publishing Ground here runs its ForcePhysics Begin before the initial
        //input reset82DB8998, which would immediately clear that mode again.
    }
    if super::climbing::advance(physics, skater, controls, camera)? {
        return Ok(());
    }
    if skater.trajectory.edges.is_empty() { skater.trajectory.edges.clone_from(&physics.grind.primitives); }
    physics.board.clear_forces();
    let query_timer = crate::performance::Scope::new("wheel_and_foot_queries");
    //World8275EA20 starts both batches before actor SetUpPhysics. Foot
    //results stay pending while PlayerInput consumes the preceding records.
    physics
        .riding
        .start_wheel_queries(&physics.board, &physics.world)?;
    let skeleton_queries = super::foot_ik_queries::query(&physics.world, &skater.skeleton)?;
    drop(query_timer);
    let graph_timer = crate::performance::Scope::new("animation_graph");
    let animation = animation_phase::advance(
        physics,
        skater,
        controls,
        graphs,
        &physics.animation_profile,
    )
    .map_err(|e| format!("Animation tick{}: {e}", physics.ticks))?;
    drop(graph_timer);
    let input_timer = crate::performance::Scope::new("input_and_state");
    //82DB4048 completes the prior Sync's Biped contact queries before input
    //reset and state PreUpdate, including updates while another state is active.
    skater.offboard.begin_player_update();
    //ForcePhysics Begin82BB2868 writes the live Skeleton16420 mode once.
    //Consume the graph request so a later physical reset can retain its own0.
    if let Some(mode) = skater.animation.motion.riding.force_mode.take() {
        skater.skeleton_input.force_mode = mode.native_value();
    }
    let teleported = input_phase::advance(
        physics,
        skater,
        &animation.packet(),
        actions,
        input_available,
    )?;
    skater.trajectory.grind_board_position = solve::deck_frame(&physics.board)[3];
    skater.trajectory.grind_com_position = skater.player_input.processed.vectors_544_560_592_608[2].map(f32::from_bits);
    skater.ground_settings = skater.ground_profiles.select(
        skater.player_input.processed.state_variant_index_2528,
        skater.player_input.processed.surface_mode_2540,
    )?;
    if teleported {
        skater.player_state.reset_for_teleport();
        skater.centre_of_mass_filter.reset();
        skater.animation_feedback.reset();
        player_state::enter_after_teleport(physics, skater)?;
        //82DB8E40 calls SetUpNormal after pose reset, state100 and board reset.
        skater.wipeout_state.ragdoll.restore_normal(
            &mut skater.skeleton,
            &mut skater.skeleton_joints,
            &mut skater.skeleton_collision,
            &mut skater.collision_feedback,
        );
    }
    //World8275EC0C ends board queries before PostInput/state selection.
    physics.riding.finish_wheel_queries()?;
    let state_before_selection = skater.player_state.current();
    player_state::post_input_and_select(physics, skater)?;
    let state_after_selection = skater.player_state.current();
    if state_before_selection != state_after_selection {
        physics.exchange.emit_event(
            tick,
            PhysicsEvent::StateChanged {
                from: state_before_selection,
                to: state_after_selection,
            },
        )?;
    }
    player_state::pre_state(physics, skater)?;
    match skater.player_state.current() {
        skate_core::player::state::PhysicalStateId::GrindBoardslide
        | skate_core::player::state::PhysicalStateId::GrindFiftyFifty
        | skate_core::player::state::PhysicalStateId::GrindTipslide
        | skate_core::player::state::PhysicalStateId::GrindFiveO
        | skate_core::player::state::PhysicalStateId::GrindBackslash
        | skate_core::player::state::PhysicalStateId::GrindDarkslide
        | skate_core::player::state::PhysicalStateId::Nonspecific => super::grind::update(physics, skater)?,
        skate_core::player::state::PhysicalStateId::BipedAir => {
            super::offboard::air_state::update(physics, skater)?;
        }
        skate_core::player::state::PhysicalStateId::BipedGround => {
            super::offboard::ground_state::update(physics, skater)?;
        }
        skate_core::player::state::PhysicalStateId::PhysicsGround => {
            let p = &skater.player_input.processed;
            let com = p.animation_com_to_deck_752.map(f32::from_bits);
            physics.riding.update_ground_reckoning(
                &physics.board,
                super::riding_outputs::RidingPoseInputs {
                    com_to_deck: Vector3::new(com[0], com[1], com[2]),
                    body_spin: skater.animation_input.extra.physical_body_spin,
                },
                p.flags_2468,
                skater.animation_input.fields.balance,
                p.flags_2476 & 0x4000_0000 != 0,
            );
            //PhysicsGround::PreUpdate82D37C50 clears the previous push suppression
            //after Reckoning; this frame's propulsion may then set it again.
            skater.ground.state.push_suppressed_2730 = false;
            ground_phase::advance(physics, skater)?;
            input_phase::update_ground(physics, skater)?;
        }
        skate_core::player::state::PhysicalStateId::PhysicsAir => {
            super::air_phase::advance(physics, skater)?
        }
        skate_core::player::state::PhysicalStateId::KnownAir => {
            super::known_air::update(physics, skater)?
        }
        skate_core::player::state::PhysicalStateId::GroundAnimation => {
            super::ground_animation::advance(physics, skater)?
        }
        skate_core::player::state::PhysicalStateId::SlideGround => {
            super::slide_state::update(physics, skater)?
        }
        skate_core::player::state::PhysicalStateId::WipeoutGround => {
            super::wipeout_states::advance(physics, skater)?
        }
        skate_core::player::state::PhysicalStateId::Teleporting => {
            //VT823272FC slot8=82D431F0; slots12/40 are original empty leaves.
            skater.teleport_state.update(&skater.player_input.processed);
        }
        state => return Err(format!("Selected {state:?} requires its physical update")),
    }
    super::offboard::possession::update(physics, skater)?;
    // Source State82DB6120 submits the queue once, then advances state time.
    // BoardRuntime applies that same queue during its shared solve.
    skater.player_input.player.state_timer_1344 += skater.player_input.processed.timestep_2604;
    physics.processed_flags_2468 = skater.player_input.processed.flags_2468;
    //World8275ECA4 ends skeleton tests after state/forces and before solving.
    //Teleport resets previous observations, but preserves this pending batch.
    skeleton_queries.publish(&mut skater.player_input.player);
    drop(input_timer);
    let solve_timer = crate::performance::Scope::new("solve_total");
    solve::advance(physics, skater, skater.ground.steering.targets)?;
    drop(solve_timer);
    let _output_timer = crate::performance::Scope::new("outputs_and_camera");
    //ProcessOutput82DB6EE8 resets the packet before its component publishers.
    //All consumers of the preceding output have completed this frame's input.
    super::player_input::reset_outputs(&mut skater.player_input.physical);
    physics.finish_skater(skater)?;
    let simulation = physics.settings.step.simulation;
    skater.player_input.update_dynamic_normal(
        &physics.board,
        &physics.riding,
        simulation.gravity_acceleration,
        simulation.time_step,
    );
    skater.player_input.publish_board(&physics.riding)?;
    player_state::publish(physics, skater)?;
    let physical = &skater.player_input.physical;
    skater.centre_of_mass_output = skater.centre_of_mass_filter.update(
        physical.reckoning.vector_64.map(f32::from_bits),
        physical.reckoning.vector_16.map(f32::from_bits),
    );
    let feedback = animation_phase::publish_feedback(physics, skater);
    let deck = physics.board.bodies()[BodyId::Deck.index()];
    let rider = skater
        .skeleton
        .bodies()
        .first()
        .ok_or("Physical output requires the constructed rider body")?;
    let ground_normal = physics.riding.ground.wheel_normal;
    let predicted = skater.player_input.physical.collision.predicted_position_64;
    let predicted_position = Vector3::new(
        f32::from_bits(predicted[0]),
        f32::from_bits(predicted[1]),
        f32::from_bits(predicted[2]),
    );
    let events = physics.exchange.events().to_vec();
    physics.exchange.publish_output(PhysicalOutputSnapshot {
        tick,
        state: skater.player_state.current(),
        board_position: deck.rates.position,
        board_linear_velocity: deck.rates.linear_velocity,
        rider_root_position: rider.rates.position,
        rider_linear_velocity: rider.rates.linear_velocity,
        ground_normal,
        contact_count: physics.riding.ground.part_contact_count as u32,
        predicted_position,
        grounded: matches!(
            skater.player_state.current(),
            skate_core::player::state::PhysicalStateId::PhysicsGround
        ),
        wiping_out: skater.skeleton_collision.is_ragdoll,
        landed: events.iter().any(|event| matches!(event, PhysicsEvent::Landing)),
        events,
    });
    camera_output::advance(physics, skater, &feedback, camera)?;
    let score = &skater.animation.motion.score_packet;
    let deck_frame = physics.board.part_transforms()[BodyId::Deck.index()];
    let position = deck.rates.position;
    let velocity = deck.rates.linear_velocity;
    let filtered = skater.player_state.filtered_output;
    skater.scoring.advance(crate::scoring_runtime::Frame {
        tick: tick as u32, dt: simulation.time_step,
        category: filtered.map_or(Default::default(), |f|f.category),
        state: skater.player_state.current() as u32,
        descriptor: score.trick.or_else(||score.grab.map(|g|g.0)),
        grind_id: filtered.map_or(-1, |f|f.grind.scorable_id), flags:score.flags,
        position:[position.x,position.y,position.z], velocity:[velocity.x,velocity.y,velocity.z],
        forward:deck_frame.basis.columns[2],
        switch:skater.animation.packet.riding_switch,fakie:skater.animation.packet.riding_fakie,
        nollie:skater.animation.packet.weight_forwards,body_flip:skater.player_input.physical.air.flag_441!=0,
        landing:skater.landing_quality,teleported,reverting:skater.player_state.state_flags[70-52],
    })?;
    super::climbing::approach::advance(physics, skater, controls);
    skater.animation_input.finish_output_publication();
    skater.player_input.player.update_count_1316 =
        skater.player_input.player.update_count_1316.wrapping_add(1);
    physics.clock.finish_tick();
    Ok(())
}
