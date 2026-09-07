//! Live straight-spline grind owner. TU3 truck query82C1FDC0, paired scorer
//!82D89150, 50-50 update82D41D70 and exit82D3F430.
use super::{GamePhysics, SkaterRuntime};
use skate_core::riding::ground_correction_math::dot_product as dot3;
use skate_core::{
    animation::skeleton_input::name::encode,
    math::{Basis3, Vector3},
    physics::{
        drive_frames::RetailAffineTransform,
        filtered_state::GrindState,
        force_queue::QueuedPointForce,
        grind_contact::{self, FiftyFiftyCandidate, Primitive},
        grind_forces,
    },
    player::state::PhysicalStateId,
    point_graph::PointGraph,
};
use skate_data::collections::Collections;
type V = [f32; 4];
pub(crate) struct Runtime {
    primitives: Vec<Primitive>,
    pub candidate: Option<FiftyFiftyCandidate>,
    pub name: String,
    pub active: bool,
    truck_to_wheel: f32,
    deck_to_truck: f32,
    pin_vs_slope: PointGraph<4>,
    pub distance: f32,
    pop_heights: [[f32; 2]; 5],
    manager_age: f32,
    pub launched: bool,
    pub launch_velocity: V,
    cooldown: u32,
    previous_position: V,
    diagnostic_tick: u32,
}
impl Runtime {
    pub fn load(data: &Collections, map: Option<&skate_data::skate_map::SkateMap>) -> Result<Self, String> {
        let graph = data
            .words::<8>("physics_grinds", "default", "PinVsSlope")?
            .map(f32::from_bits);
        Ok(Self {
            primitives: crate::grind_world::primitives(map)?,
            candidate: None,
            name: String::new(),
            active: false,
            truck_to_wheel: data.float("physics_grinds", "default", "TruckToWheel")?,
            deck_to_truck: data.float("physics_grinds", "default", "DeckCenterToTruck")?,
            pin_vs_slope: PointGraph {
                x: graph[..4].try_into().unwrap(),
                y: graph[4..8].try_into().unwrap(),
            },
            pop_heights: ["easy", "normal", "hardcore", "motorized", "test"]
                .map(|key| {
                    Ok::<_, String>([
                        data.float("physics_mode", key, "Hash_3097A69281990652")?,
                        data.float("physics_mode", key, "Hash_FA4CDBAE0DFD1FAD")?,
                    ])
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap(),
            manager_age: 0.0,
            launched: false,
            launch_velocity: [0.; 4],
            cooldown: 0,
            distance: 0.0,
            previous_position: [0.; 4],
            diagnostic_tick: 0,
        })
    }
    pub fn output(&self) -> GrindState {
        if !self.active {
            return GrindState::default();
        }
        let owner = self
            .candidate
            .map(|c| self.primitives[c.primitive].owner)
            .unwrap_or(u64::MAX);
        GrindState {
            kind: 0,
            name: encode(self.name.as_bytes()),
            scoring_name: encode(self.name.as_bytes()),
            pathed_guid: owner,
            local_guid: owner,
            ..GrindState::default()
        }
    }
}
///Run after ProcessInput, before the native state selector. Geometry is queried
///against both current trucks; merely being near a rail cannot acquire it.
pub(super) fn query(physics: &mut GamePhysics, skater: &mut SkaterRuntime) {
    let p = &mut skater.player_input.processed;
    let runtime = &mut physics.grind;
    let board = super::solve::deck_frame(&physics.board);
    runtime.cooldown = runtime.cooldown.saturating_sub(1);
    runtime.manager_age = (runtime.manager_age + 0.0035).min(1.0);
    let old = runtime.candidate;
    runtime.candidate = None;
    runtime.diagnostic_tick = runtime.diagnostic_tick.wrapping_add(1);
    let gated = runtime.cooldown > 0 || skater.player_input.grind.disabled
        || p.flags_2476 & 0x01000000 != 0;
    // Native GrindData query82D8A828: a 1.2m half-extent, at most40 chords.
    let nearby: Vec<usize> = runtime.primitives.iter().enumerate().filter(|(_,edge)|
        (0..3).all(|i| edge.start[i].min(edge.end[i]) <= board[3][i]+1.2
            && edge.start[i].max(edge.end[i]) >= board[3][i]-1.2))
        .map(|(i,_)|i).take(40).collect();
    if nearby.is_empty() { return; }
    let edges: Vec<Primitive> = nearby.iter().map(|i|runtime.primitives[*i]).collect();
    let mut hits = grind_contact::truck_contacts(
        board, p.flags_2484, runtime.truck_to_wheel, runtime.deck_to_truck, &edges,
    );
    for hit in hits.iter_mut().flatten() { hit.primitive = nearby[hit.primitive]; }
    let extra = &skater.animation_input.extra;
    let candidate = grind_contact::fifty_fifty_candidate_on_splines(
        board, [extra.grind_translation, extra.grind_stability_nudge], hits,
        &runtime.primitives,
    );
    // Passive diagnostics for the user's visual run; no input simulation.
    if runtime.diagnostic_tick % 30 == 0 || (!gated && candidate.is_some()) != old.is_some() {
        bevy::log::info!("GRIND_QUERY state={} gated={} nearby={} hits={:?} depth={:?} candidate={} board={:?} up={:?} forward={:?} probe={:?} flags={:08x}/{:08x}/{:08x}/{:08x}/{:08x}",
            p.state_2508, gated, nearby.len(), hits.map(|h|h.map(|h|h.primitive)),
            hits.map(|h|h.map(|h|dot3(sub(board[3],h.position),board[1]))),
            candidate.is_some(),board[3],board[1],board[2],
            [runtime.truck_to_wheel,runtime.deck_to_truck],
            p.flags_2468,p.flags_2472,p.flags_2476,p.flags_2480,p.flags_2484);
    }
    if gated { return; }
    let Some(candidate) = candidate else { return; };
    let velocity = p.vectors_400_416[0].map(f32::from_bits);
    //82D886B8: air accepts an actual intersection; same active grind uses90deg.
    //The ordinary level-ground admission uses17deg.
    let angle = if p.category_2512 == 400 || p.state_2508 == 701 {
        90.0
    } else {
        17.0
    };
    if p.category_2512 != 200
        && !grind_contact::within_approach_angle(candidate.direction, velocity, angle)
    {
        return;
    }
    runtime.candidate = Some(candidate);
    p.grind_position_1120 = candidate.centre.map(f32::to_bits);
    p.grind_direction_1136 = candidate.direction.map(f32::to_bits);
    if !runtime.active || old.is_none() {
        let backwards = dot3(board[2], velocity) < 0.0;
        let side = dot3(sub(candidate.centre, board[3]), board[0]) >= 0.0;
        runtime.name = format!(
            "{}{}_50_50",
            if backwards { "BF_" } else { "" },
            if side { "FS" } else { "BS" }
        );
    }
}
pub(super) fn enter(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    physics.grind.active = skater.player_state.current() == PhysicalStateId::GrindFiftyFifty;
    physics.grind.distance = 0.0;
    physics.grind.previous_position = super::solve::deck_frame(&physics.board)[3];
    skater.ground_lifecycle.skeleton_elapsed_16505 = true;
    physics
        .board
        .hook_mut()
        .drive
        .disable_animation(&mut skater.ground_lifecycle.board_animated_290);
    skater.air_reckoning.state.reset_spin();
    if physics.grind.active {
        bevy::log::info!(
            "Grind entered: {} rail={:?}",
            physics.grind.name,
            physics
                .grind
                .candidate
                .map(|c| physics.grind.primitives[c.primitive].owner)
        );
    }
    Ok(())
}
pub(super) fn exit(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    physics.grind.active = false;
    physics
        .board
        .hook_mut()
        .drive
        .disable_animation(&mut skater.ground_lifecycle.board_animated_290);
    //82D3F7D0 restores all four wheel drags to zero.
    for body in &mut physics.board.bodies_mut()[..4] {
        body.inertia.linear_drag = 0.0;
    }
    Ok(())
}
pub(super) fn update(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    physics.grind.launched = false;
    let board = super::solve::deck_frame(&physics.board);
    let p = &skater.player_input.processed;
    if physics.grind.active && p.flags_2468 & 0x00400000 != 0 {
        let c = physics
            .grind
            .candidate
            .ok_or("Grind pop has no primitive")?;
        let [min, max] = physics.grind.pop_heights[p.state_variant_index_2528 as usize];
        let strength = skater.animation_input.extra.jump_strength;
        let velocity = grind_forces::fifty_fifty_pop(
            p.vectors_400_416[0].map(f32::from_bits),
            grind_contact::upright_normal(c.direction),
            c.direction,
            board[3],
            c.centre,
            (1.0 - strength).mul_add(min, max * strength),
            skater.animation_input.extra.grind_stability_nudge,
            physics.grind.manager_age,
        );
        physics.grind.launched = true;
        physics.grind.launch_velocity = velocity;
        physics.grind.cooldown = 10;
        physics.grind.manager_age = (physics.grind.manager_age - 0.2).max(0.0);
        skater
            .ground_runtime
            .set_animated_velocity(&mut physics.board, velocity);
        physics
            .board
            .hook_mut()
            .drive
            .disable_animation(&mut skater.ground_lifecycle.board_animated_290);
        return super::input_phase::update_grind_jump(physics, skater);
    }
    let velocity = p.vectors_400_416[0].map(f32::from_bits);
    //Both state vtables neutralize the trucks before the shared board solve.
    skater.ground.steering.targets = [0.; 2];
    let normal = if physics.grind.active {
        let c = physics
            .grind
            .candidate
            .ok_or("Active grind lost its candidate before selection")?;
        let normal = grind_contact::upright_normal(c.direction);
        let across = cross(c.direction, normal);
        let force = grind_forces::lateral_pin(
            board,
            c.centre,
            across,
            velocity,
            800.,
            0.,
            0.07,
            true,
            physics.grind.pin_vs_slope.evaluate(dot3(normal, normal)),
        );
        append(physics, force)?;
        //The primitive support frame is static and level for the authored rails.
        //82D86318 publishes this unit support normal in candidate304.
        let friction = grind_forces::friction(
            velocity,
            normal,
            normal,
            skater.player_input.grind.friction_vs_time,
            false,
            1.3,
            0,
            [50., 40., 40.],
        );
        append(physics, friction)?;
        let forward = if dot3(board[2], c.direction) > 0.0 {
            c.direction
        } else {
            c.direction.map(|x| -x)
        };
        let target = [cross(normal, forward), normal, forward, board[3]];
        let (frame, _) = skate_core::animation::foot_ik::interpolate_native(&board, &target, 0.1);
        physics.board.set_hook_transform(RetailAffineTransform {
            basis: Basis3 {
                columns: std::array::from_fn(|i| [frame[i][0], frame[i][1], frame[i][2]]),
            },
            translation: xyz(frame[3]),
        });
        physics
            .board
            .hook_mut()
            .drive
            .enable_angular_only(&mut skater.ground_lifecycle.board_animated_290);
        physics.grind.distance +=
            dot3(sub(board[3], physics.grind.previous_position), c.direction).abs();
        physics.grind.previous_position = board[3];
        normal
    } else {
        //Nonspecific82D42F50 applies the native -40 support force while the
        //selector waits for actual wheel/collision observations after rail exit.
        append(
            physics,
            p.vectors_544_560_592_608[0]
                .map(f32::from_bits)
                .map(|v| v * -40.0),
        )?;
        board[1]
    };
    physics.riding.update_grind_reckoning(
        &mut skater.air_reckoning.state,
        normal,
        board[2],
        skater.player_input.processed.flags_2468,
        if physics.grind.active { 0.9 } else { 0.98 },
    );
    //82D40D40 and the ordinary nonspecific branch both call Skeleton82BDF530.
    super::input_phase::update_ground(physics, skater)
}
fn append(physics: &mut GamePhysics, force: V) -> Result<(), String> {
    if !physics.board.forces_mut().append(QueuedPointForce {
        tag: 0,
        force_world: xyz(force),
        point_body: Vector3::ZERO,
    }) {
        return Err("Grind force queue exhausted".into());
    }
    Ok(())
}

fn xyz(v: V) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
        0.,
    ]
}
