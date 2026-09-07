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
#[path = "grind_camera.rs"]
mod camera;
#[path = "grind_chromosome.rs"]
mod chromosome;
pub(crate) struct Runtime {
    camera: camera::GrindCamera,
    primitives: Vec<Primitive>,
    pub candidate: Option<FiftyFiftyCandidate>,
    pub name: String,
    pub active: bool,
    pub kind: u32,
    pub front_contact: bool,
    chromosome: chromosome::Chromosome,
    scoring_name: String,
    ground_frames: u32,
    crouch: f32,
    control: grind_contact::control::Control,
    test_depth_epsilon: f32,
    test_depth: f32,
    exiting: bool,
    surface_kind: u32,
    surface_normal: V,
    truck_to_wheel: f32,
    deck_to_truck: f32,
    pin_vs_slope: PointGraph<4>,
    pub distance: f32,
    pop_heights: [[f32; 2]; 5],
    boardslide_pop_heights: [[f32; 2]; 5],
    tipslide_pop_heights: [[f32; 2]; 5],
    manager_age: f32,
    pub launched: bool,
    pub launch_velocity: V,
    cooldown: u32,
    previous_position: V,
    diagnostic_tick: u32,
}
impl Runtime {
    pub fn load(
        data: &Collections,
        map: Option<&skate_data::skate_map::SkateMap>,
    ) -> Result<Self, String> {
        let graph = data
            .words::<8>("physics_grinds", "default", "PinVsSlope")?
            .map(f32::from_bits);
        Ok(Self {
            camera: camera::GrindCamera::default(),
            primitives: crate::grind_world::primitives(map)?,
            candidate: None,
            name: String::new(),
            active: false,
            kind: 0,
            front_contact: false,
            chromosome: chromosome::Chromosome::default(),
            scoring_name: String::new(),
            ground_frames: 0,
            crouch: 0.0,
            control: grind_contact::control::Control::default(),
            test_depth_epsilon: data.float("physics_grinds", "default", "TestDepthEpsilon")?,
            test_depth: data.float("physics_grinds", "default", "TestDepth")?,
            exiting: false,
            surface_kind: 0,
            surface_normal: [0.0, 1.0, 0.0, 0.0],
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
            //82D41CE8 consumes mode100/104;50-50's82D40D60 uses92/96.
            boardslide_pop_heights: ["easy", "normal", "hardcore", "motorized", "test"]
                .map(|key| {
                    Ok::<_, String>([
                        data.float("physics_mode", key, "Hash_B2B1170AFFC8AC69")?,
                        data.float("physics_mode", key, "Hash_1B3E9F9C836D287D")?,
                    ])
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .unwrap(),
            tipslide_pop_heights: ["easy", "normal", "hardcore", "motorized", "test"]
                .map(|key| {
                    Ok::<_, String>([
                        data.float("physics_mode", key, "Hash_703829BD711E54DE")?,
                        data.float("physics_mode", key, "Hash_F2473E9125079F0")?,
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
    pub fn advance_camera(&mut self, velocity: V) -> Result<(), String> {
        if self.active {
            let contact = self.candidate.ok_or("Grind camera needs active contact")?;
            self.camera.update(
                contact,
                self.primitives[contact.primitive],
                self.kind,
                velocity,
            );
        } else {
            self.camera.exit();
        }
        Ok(())
    }
    pub fn camera_output(&self) -> crate::camera::CameraGrindOutput {
        crate::camera::CameraGrindOutput {
            //Reset82DE3518 defaults still apply outside the active state.
            direction_0: if self.active {
                self.camera.direction
            } else {
                [1.0, 0.0, 0.0, 0.0]
            },
            camera_target_96: if self.active {
                self.camera.target
            } else {
                [0.0; 4]
            },
            grinding_316: u8::from(self.active),
        }
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
            kind: self.kind as i32,
            name: encode(self.name.as_bytes()),
            scoring_name: encode(self.scoring_name.as_bytes()),
            on_front: self.front_contact,
            crouch: self.crouch,
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
    let mut effective_board = board;
    if p.flags_2468 & 0x0010_0000 != 0 {
        effective_board[0] = effective_board[0].map(|v| -v);
        effective_board[2] = effective_board[2].map(|v| -v);
    }
    let pose = chromosome::Pose {
        animated_board: skater.animated_skeleton.board_frames.animation_target,
        board: effective_board,
        foot_directions: [
            skater.skeleton.record.pose[19][0],
            skater.skeleton.record.pose[15][0],
        ],
        fakie: skater.animation.packet.riding_fakie,
    };
    runtime.chromosome.observe(pose, p.category_2512);
    runtime.cooldown = runtime.cooldown.saturating_sub(1);
    runtime.manager_age = (runtime.manager_age + 0.0035).min(1.0);
    let old = runtime.candidate;

    runtime.candidate = None;
    runtime.diagnostic_tick = runtime.diagnostic_tick.wrapping_add(1);
    runtime.ground_frames = if p.category_2512 == 100 {
        runtime.ground_frames.saturating_add(1)
    } else {
        0
    };
    let gated = runtime.cooldown > 0
        || skater.player_input.grind.disabled
        || p.flags_2476 & 0x01000000 != 0;
    // Native GrindData query82D8A828: a 1.2m half-extent, at most40 chords.
    let nearby: Vec<usize> = runtime
        .primitives
        .iter()
        .enumerate()
        .filter(|(_, edge)| {
            (0..3).all(|i| {
                edge.start[i].min(edge.end[i]) <= board[3][i] + 1.2
                    && edge.start[i].max(edge.end[i]) >= board[3][i] - 1.2
            })
        })
        .map(|(i, _)| i)
        .take(40)
        .collect();
    if nearby.is_empty() {
        return;
    }
    let edges: Vec<Primitive> = nearby.iter().map(|i| runtime.primitives[*i]).collect();
    let mut hits = grind_contact::truck_contacts(
        board,
        p.flags_2484,
        runtime.truck_to_wheel,
        runtime.deck_to_truck,
        &edges,
    );
    for hit in hits.iter_mut().flatten() {
        hit.primitive = nearby[hit.primitive];
    }
    let extra = &skater.animation_input.extra;
    let fifty = grind_contact::fifty_fifty_candidate_on_splines(
        board,
        [extra.grind_translation, extra.grind_stability_nudge],
        hits,
        &runtime.primitives,
    );
    let velocity = p.vectors_400_416[0].map(f32::from_bits);
    let slide = grind_contact::deck_contact(
        board,
        p.flags_2484,
        runtime.test_depth_epsilon,
        runtime.test_depth,
        runtime.deck_to_truck,
        &edges,
    )
    .and_then(|mut hit| {
        hit.primitive = nearby[hit.primitive];
        grind_contact::boardslide_candidate(
            board,
            hit,
            runtime.primitives[hit.primitive],
            velocity,
            p.category_2512,
            skater.player_input.grind.low_wheel_frames as i32,
            p.flags_2476,
            runtime.deck_to_truck,
            runtime.truck_to_wheel,
        )
    });
    use grind_contact::families::{self, Contact};
    let five = families::five_o(
        board,
        hits,
        &runtime.primitives,
        velocity,
        p.flags_2476,
        p.flags_2472,
        runtime.ground_frames,
        extra.grind_translation,
    );
    let mut tip_hits = families::tip_contacts(
        board,
        p.flags_2484,
        p.state_2504,
        p.flags_2468,
        runtime.deck_to_truck,
        &edges,
    );
    for hit in tip_hits.iter_mut().flatten() {
        hit.primitive = nearby[hit.primitive];
    }
    let tip = families::tipslide(
        board,
        tip_hits,
        &runtime.primitives,
        velocity,
        p.category_2512,
        p.flags_2476,
        p.flags_2472,
        runtime.ground_frames,
        skater.animation_input.fields.balance,
        p.vectors_464_480_496_512_528[0].map(f32::from_bits),
    );
    let dark = families::inverted_contact(
        board,
        p.flags_2484,
        runtime.test_depth_epsilon,
        runtime.test_depth,
        &edges,
    )
    .and_then(|mut hit| {
        hit.primitive = nearby[hit.primitive];
        families::darkslide(
            board,
            hit,
            runtime.primitives[hit.primitive],
            velocity,
            p.category_2512,
            skater.player_input.grind.low_wheel_frames,
        )
    });
    let fifty = fifty.map(|geometry| Contact {
        geometry,
        front: false,
        kind: 0,
    });
    let slide = slide.map(|geometry| Contact {
        geometry,
        front: false,
        kind: 1,
    });
    //82D875A8 retains a valid current family, then truck/tip/deck/inverted.
    let current = if runtime.active {
        match runtime.kind {
            0 | 3 => fifty.or(five),
            1 => slide,
            2 | 4 => tip,
            5 => dark,
            _ => None,
        }
    } else {
        None
    };
    let selected = current.or(fifty).or(five).or(tip).or(slide).or(dark);
    let candidate = selected.map(|c| c.geometry);
    let mut kind = selected.map_or(0, |c| c.kind);
    runtime.front_contact = selected.is_some_and(|c| c.front);
    // Passive diagnostics for the user's visual run; no input simulation.
    if runtime.diagnostic_tick % 30 == 0 || (!gated && candidate.is_some()) != old.is_some() {
        bevy::log::info!(
            "GRIND_QUERY state={} gated={} nearby={} hits={:?} depth={:?} candidate={} board={:?} up={:?} forward={:?} probe={:?} flags={:08x}/{:08x}/{:08x}/{:08x}/{:08x}",
            p.state_2508,
            gated,
            nearby.len(),
            hits.map(|h| h.map(|h| h.primitive)),
            hits.map(|h| h.map(|h| dot3(sub(board[3], h.position), board[1]))),
            candidate.is_some(),
            board[3],
            board[1],
            board[2],
            [runtime.truck_to_wheel, runtime.deck_to_truck],
            p.flags_2468,
            p.flags_2472,
            p.flags_2476,
            p.flags_2480,
            p.flags_2484
        );
    }
    if gated {
        return;
    }
    let Some(mut candidate) = candidate else {
        return;
    };
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
    runtime.kind = kind;
    if kind != 0 {
        let edge = runtime.primitives[candidate.primitive];
        match skate_core::air::trajectory::grind_surface::investigate(
            &physics.world,
            edge.start,
            edge.end,
            candidate.centre,
            runtime.deck_to_truck,
        ) {
            Ok(Some(surface)) => {
                if surface.evidence.kind == 3 {
                    runtime.candidate = None;
                    return;
                }
                runtime.surface_kind = surface.evidence.kind;
                runtime.surface_normal = surface.normal;
                if kind == 2
                    && families::is_backslash(
                        board[3],
                        candidate.centre,
                        surface.far_points,
                        surface.normal,
                        runtime.deck_to_truck,
                        p.state_2508,
                    )
                {
                    kind = 4;
                    runtime.kind = kind;
                }
            }
            Ok(None) | Err(_) => {
                runtime.candidate = None;
                return;
            }
        }
    }
    //82D87460: primitive tangent follows travel, retaining its previous sign at rest.
    let edge = runtime.primitives[candidate.primitive];
    let delta = sub(edge.end, edge.start);
    let length = dot3(delta, delta).sqrt();
    let mut direction = delta.map(|v| v / length);
    let along = dot3(direction, velocity);
    if along < 0.0 {
        direction = direction.map(|v| -v);
    }
    if along.abs() < 0.1 && dot3(p.grind_direction_1136.map(f32::from_bits), direction) < -0.9 {
        direction = direction.map(|v| -v);
    }
    candidate.direction = direction;
    runtime.candidate = Some(candidate);
    p.grind_position_1120 = candidate.centre.map(f32::to_bits);
    p.grind_direction_1136 = direction.map(f32::to_bits);
    let normal = if kind == 0 {
        grind_contact::upright_normal(direction)
    } else {
        runtime.surface_normal
    };
    let (name, scoring_name) =
        runtime
            .chromosome
            .update(pose, kind, candidate.centre, direction, normal);
    runtime.name = name.into();
    runtime.scoring_name = scoring_name.into();
}
pub(super) fn enter(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    physics.grind.active = matches!(
        skater.player_state.current(),
        PhysicalStateId::GrindFiftyFifty
            | PhysicalStateId::GrindBoardslide
            | PhysicalStateId::GrindTipslide
            | PhysicalStateId::GrindFiveO
            | PhysicalStateId::GrindBackslash
            | PhysicalStateId::GrindDarkslide
    );
    physics.grind.exiting = false;
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
        let heights = if physics.grind.kind == 1 {
            &physics.grind.boardslide_pop_heights
        } else if physics.grind.kind == 2 {
            &physics.grind.tipslide_pop_heights
        } else {
            &physics.grind.pop_heights
        };
        let [min, max] = heights[p.state_variant_index_2528 as usize];
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
        let normal = if physics.grind.kind != 0 {
            physics.grind.surface_normal
        } else {
            grind_contact::upright_normal(c.direction)
        };
        physics.grind.crouch = dot3(board[2], normal).abs() * 0.4;
        let across = cross(c.direction, normal);
        physics.grind.control.update(
            physics.grind.kind,
            board[2],
            normal,
            physics.grind.front_contact,
            p.flags_2468 & 0x0010_0000 != 0,
            false,
            skater.animation_input.extra.grind_translation,
            skater.animation_input.extra.grind_stability_nudge,
            skater.animation_input.extra.grind_up_down,
            skater.animation_input.extra.grind_grab_min_height,
        );
        if matches!(physics.grind.kind, 1 | 5) {
            physics.grind.exiting |= p.flags_2468 & 0x0020_0000 != 0;
            let mass = skater
                .player_input
                .toolkit
                .as_ref()
                .ok_or("Boardslide needs board mass")?
                .total_mass;
            for force in grind_forces::boardslide_control(
                board[3],
                c.centre,
                c.direction,
                normal,
                velocity,
                //Darkslide82D41438 scales translation40 by remaining .16m
                //deck width; Boardslide82D419A0 uses constant25.
                skater.animation_input.extra.grind_translation
                    * if physics.grind.kind == 5 {
                        (1.0 - dot3(sub(board[3], c.centre), across).abs() * 6.25) * 40.0 / 25.0
                    } else {
                        1.0
                    },
                mass,
                physics.grind.exiting,
                physics.grind.surface_kind == 2,
            ) {
                append(physics, force)?;
            }
        } else if physics.grind.kind == 2 {
            //82D42390: lateral direction points toward the contact. The
            //animation nudge changes sign with the contact side.
            let offset = sub(board[3], c.centre);
            let inward = across.map(|v| {
                v * if dot3(across, offset) > 0.0 {
                    -1.0
                } else {
                    1.0
                }
            });
            let nudge = skater.animation_input.extra.grind_stability_nudge
                * if dot3(normal, cross(c.direction, offset)) > 0.0 {
                    1.0
                } else {
                    -1.0
                };
            append(physics, inward.map(|v| v * nudge * 37.0))?;
        } else {
            let (strength, forward_offset, up_offset) = match physics.grind.kind {
                3 => (3000.0, 0.234, 0.07),
                4 => (3000.0, 0.39, 0.025),
                _ => (800.0, 0.0, 0.07),
            };
            let force = grind_forces::lateral_pin(
                board,
                c.centre,
                across,
                velocity,
                strength,
                forward_offset,
                up_offset,
                physics.grind.front_contact,
                physics.grind.pin_vs_slope.evaluate(dot3(normal, normal)),
            );
            append(physics, force)?;
            if matches!(physics.grind.kind, 0 | 3) {
                //82D401F8: support load from the native pitch conditioner.
                append(
                    physics,
                    normal.map(|v| v * physics.grind.control.pitch.abs() * -600.0),
                )?;
            }
        }
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
            match physics.grind.kind {
                1 | 5 => [55., 55., 60.],
                2 => [60., 60., 60.],
                4 => [70., 80., 80.],
                _ => [50., 40., 40.],
            },
        );
        append(physics, friction)?;
        let forward = if dot3(board[2], c.direction) > 0.0 {
            c.direction
        } else {
            c.direction.map(|x| -x)
        };
        let target = if matches!(physics.grind.kind, 1 | 5) {
            //82D418B8 aligns the board's RIGHT axis with the rail;50-50
            //82D41E38 aligns its FORWARD axis instead.
            let right = if dot3(board[0], c.direction) > 0.0 {
                c.direction
            } else {
                c.direction.map(|x| -x)
            };
            let up = if physics.grind.kind == 5 {
                normal.map(|v| -v)
            } else {
                normal
            };
            [right, up, cross(right, up), board[3]]
        } else if matches!(physics.grind.kind, 2 | 4) {
            grind_contact::control::tip_frame(
                board,
                c.direction,
                normal,
                c.centre,
                physics.grind.kind == 4,
            )
        } else if physics.grind.kind == 3
            || (physics.grind.kind == 0 && physics.grind.control.pitch.abs() > 0.12)
        {
            grind_contact::control::truck_frame(
                board,
                normal,
                &physics.grind.control,
                p.flags_2468 & 0x0010_0000 != 0,
            )
        } else {
            [cross(normal, forward), normal, forward, board[3]]
        };
        //82D40890 uses constant0.1 for interpolation; its f1 parameter is
        //separate random perturbation amplitude, not the interpolation weight.
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
