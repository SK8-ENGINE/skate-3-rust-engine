//! Host ownership for the original 82DB4048 input phase. Every numerical
//! preparation is performed here; the two callbacks address the shared
//! skeleton/whole-player owners that cannot be borrowed twice by the host.
use super::{
    grind::{GrindInputState, Pending},
    pre_input::PreInputManager,
};
use crate::physics::ground_runtime::GroundRuntime;
use skate_core::{
    math::Vector3,
    physics::{
        board_runtime::BoardRuntime, board_toolkit::BoardToolkit,
        skeleton_animation_record::AnimationPartTransform,
    },
    player::input_phase::{
        AnimationInputPacket, InputPhaseServices, PhysicalPlayerInput, PlayerInputState,
        ProcessedPhysicsInput, RawVector,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct InputHostFrame {
    pub actor_query_56: u32,
    pub actor_query_44: u32,
    pub input_available: bool,
    /// Actual gameplay action 71 from ControllerInput::player_actions().
    pub transition_action: f32,
    /// Motion output's published +0 frame, read by the native reset request.
    pub published_board_transform: AnimationPartTransform,
    /// Native air-state manager +40, only read by the category200 grind gate.
    pub air_counter_40: i32,
}

pub(crate) trait PlayerInputCallbacks {
    fn prepare_grind(
        &mut self,
        board: &mut BoardRuntime,
        grind: &mut GrindInputState,
        processed: &ProcessedPhysicsInput,
        world: &skate_core::physics::board_world::BoardWorld,
        provider: crate::grind_world::RailView<'_>,
        air_counter: i32,
    ) -> Result<Pending, String>;
    fn process_skeleton(
        &mut self,
        board: &mut BoardRuntime,
        toolkit: &BoardToolkit,
        packet: &AnimationInputPacket<'_>,
        physical: &mut PhysicalPlayerInput,
        processed: &mut ProcessedPhysicsInput,
    ) -> Result<(), String>;

    /// Complete 82DB8998 reset including board, skeleton, Reckoning, current
    /// physical outputs and Ground entry. Returning an error retains the request.
    fn teleport(
        &mut self,
        board: &mut BoardRuntime,
        ground: &mut GroundRuntime,
        target: AnimationPartTransform,
        player: &mut PlayerInputState,
        physical: &mut PhysicalPlayerInput,
        processed: &mut ProcessedPhysicsInput,
    ) -> Result<(), String>;
}

pub(super) struct Services<'a, C> {
    pub board: &'a mut BoardRuntime,
    pub ground: &'a mut GroundRuntime,
    pub ground_frame: AnimationPartTransform,
    pub host: InputHostFrame,
    pub callbacks: &'a mut C,
    pub pre_input: &'a mut PreInputManager,
    pub grind: &'a mut GrindInputState,
    pub world_edges: crate::grind_world::RailView<'a>,
    pub world: &'a skate_core::physics::board_world::BoardWorld,
    pub pending_grind: &'a mut Option<Pending>,
    pub toolkit: &'a mut Option<BoardToolkit>,
    pub pending_teleport: &'a mut Option<AnimationPartTransform>,
    pub pending_velocity: &'a mut Option<[f32; 3]>,
    pub pending_ground_fit: &'a mut Option<skate_core::physics::teleport_ground_fit::GroundFit>,
    pub arrival: ArrivalSolids<'a>,
}

/// What a travel's arrival fit-up ray hits besides the static world: docs/hails-additions/38-dlc-support.md#what-the-fit-up-ray-hits
#[derive(Clone, Copy, Default)]
pub(crate) struct ArrivalSolids<'a> {
    pub props: Option<&'a skate_core::physics::board_world::BoardWorld>,
    /// This tick's solve proxies; only the traffic cars (`VEHICLE_GROUP`) count.
    pub proxies: &'a [(usize, skate_dynamics::SolidBody)],
}

/// The nearest hit of the fit-up ray over the world, the props and the cars.
pub(super) fn arrival_hit(
    world: &skate_core::physics::board_world::BoardWorld,
    arrival: ArrivalSolids<'_>,
    fit: skate_core::physics::teleport_ground_fit::GroundFit,
    s: [f32; 3],
    e: [f32; 3],
) -> Option<[f32; 3]> {
    use skate_core::physics::board_world::ExternalQueries;
    let v = |p: [f32; 3]| skate_core::math::Vector3::new(p[0], p[1], p[2]);
    let mut hits = vec![world.query_thin_line(v(s), v(e)).ok().flatten()];
    if fit.props {
        hits.push(arrival.props.and_then(|p| p.query_thin_line(v(s), v(e)).ok().flatten()));
    }
    if fit.vehicles {
        let cars: Vec<_> = arrival.proxies.iter()
            .filter(|(_, b)| b.contact_group == crate::physics::VEHICLE_GROUP)
            .map(|(_, b)| b.clone())
            .collect();
        if !cars.is_empty() {
            hits.push(crate::modding::bridge::MovingQueries(cars).line(v(s), v(e), 0.0).map(|h| h.hit));
        }
    }
    hits.into_iter().flatten()
        .min_by(|a, b| a.geometry.fraction.total_cmp(&b.geometry.fraction))
        .map(|hit| [hit.geometry.position.x, hit.geometry.position.y, hit.geometry.position.z])
}
impl<C: PlayerInputCallbacks> InputPhaseServices for Services<'_, C> {
    type Error = String;
    fn update_pre_input_manager_82d81610(
        &mut self,
        player: &mut PlayerInputState,
        _physical: &mut PhysicalPlayerInput,
    ) -> Result<(), String> {
        self.pre_input.prepare(&mut player.manager_1856_counter_320)
    }
    fn reset_processed_input_82bf9ef0(
        &mut self,
        output: &mut ProcessedPhysicsInput,
    ) -> Result<(), String> {
        super::reset::reset_processed(output);
        Ok(())
    }
    fn actor_query_slot_56(&mut self) -> Result<u32, String> {
        Ok(self.host.actor_query_56)
    }
    fn actor_query_slot_44(&mut self) -> Result<u32, String> {
        Ok(self.host.actor_query_44)
    }
    fn reset_player_probe_82d7a330(
        &mut self,
        player: &mut PlayerInputState,
        _physical: &mut PhysicalPlayerInput,
    ) -> Result<(), String> {
        // Native clears every represented field after copying it to Processed.
        player.probe = Default::default();
        Ok(())
    }
    fn check_teleport_82db88c8(
        &mut self,
        player: &mut PlayerInputState,
        physical: &mut PhysicalPlayerInput,
        output: &mut ProcessedPhysicsInput,
    ) -> Result<(), String> {
        // Native handles its state request before its explicit reset bit.
        // Host requests remain owned until the entire reset succeeds.
        if physical.state.flag_61 != 0 {
            let mut target = self.host.published_board_transform;
            // A travel's arrival fit-up (retail 82598428 in the teleport message, b475): height onto the static world.
            if let Some(fit) = self.pending_ground_fit.take() {
                let (world, arrival) = (self.world, self.arrival);
                let asked = target[3];
                target = skate_core::physics::teleport_ground_fit::fit_arrival_to_ground(target, fit, |s, e| {
                    arrival_hit(world, arrival, fit, s, e)
                });
                // Logs must diagnose arrival reports: where the teleport asked to land and where the fit put it.
                bevy::log::info!(
                    "TELEPORT_ARRIVAL asked=[{:.2}, {:.2}, {:.2}] fitted_y={:.2} ground_fit={}",
                    asked[0], asked[1], asked[2], target[3][1], fit.enabled
                );
            }
            self.callbacks
                .teleport(self.board, self.ground, target, player, physical, output)?;
            // A mod's arrival velocity (`travel`, ours) after the one reset of the reply path (b471).
            if self.pending_teleport.is_none() {
                if let Some(v) = self.pending_velocity.take() {
                    apply_board_linvel(self.board, v);
                }
            }
        }
        if player.flags_1296 & (1 << 19) != 0 {
            let deck = self.board.part_transforms()[6];
            let mut target = [[0.0; 4]; 4];
            for (dst, src) in target[..3].iter_mut().zip(deck.basis.columns) {
                dst[..3].copy_from_slice(&src);
            }
            let p = deck.translation;
            target[3] = [p.x, p.y, p.z, 0.0];
            self.callbacks
                .teleport(self.board, self.ground, target, player, physical, output)?;
            player.flags_1296 &= !(1 << 19);
        }
        if let Some(target) = *self.pending_teleport {
            self.callbacks
                .teleport(self.board, self.ground, target, player, physical, output)?;
            *self.pending_teleport = None;
            if let Some(v) = self.pending_velocity.take() {
                apply_board_linvel(self.board, v);
            }
        }
        Ok(())
    }
    fn actor_input_available_slot_4(&mut self) -> Result<bool, String> {
        Ok(self.host.input_available)
    }
    fn transition_action_825903c8(&mut self) -> Result<f32, String> {
        Ok(self.host.transition_action)
    }
    fn calculate_ground_position_82c02840(
        &mut self,
        _physical: &PhysicalPlayerInput,
    ) -> Result<RawVector, String> {
        Ok(super::ground_position::ground_position(
            self.board,
            &self.ground_frame,
        ))
    }
    fn prepare_board_toolkit_82c013f0(
        &mut self,
        _player: &mut PlayerInputState,
        _physical: &mut PhysicalPlayerInput,
        output: &mut ProcessedPhysicsInput,
    ) -> Result<(), String> {
        let toolkit = self.ground.prepare_toolkit(self.board, output);
        //82C01744 publishes this during toolkit preparation, not PhysOut fill.
        output.scalar_2616 = toolkit.absolute_speed;
        *self.toolkit = Some(toolkit);
        Ok(())
    }
    fn process_skeleton_82bd8918(
        &mut self,
        packet: &AnimationInputPacket<'_>,
        physical: &mut PhysicalPlayerInput,
        output: &mut ProcessedPhysicsInput,
    ) -> Result<(), String> {
        let toolkit = self
            .toolkit
            .as_ref()
            .ok_or("Input toolkit must precede Skeleton::ProcessData")?;
        self.callbacks
            .process_skeleton(self.board, toolkit, packet, physical, output)
    }
    fn update_grind_manager_82d8a828(
        &mut self,
        _player: &mut PlayerInputState,
        _physical: &mut PhysicalPlayerInput,
        output: &mut ProcessedPhysicsInput,
    ) -> Result<(), String> {
        if self.pending_grind.is_some() {
            return Err("Previous grind input was not consumed by PostInput".into());
        }
        *self.pending_grind = Some(self.callbacks.prepare_grind(
            self.board,
            self.grind,
            output,
            self.world,
            self.world_edges,
            self.host.air_counter_40,
        )?);
        Ok(())
    }
}

fn apply_board_linvel(board: &mut BoardRuntime, v: [f32; 3]) {
    let vel = Vector3::new(v[0], v[1], v[2]);
    for body in board.bodies_mut() {
        body.rates.linear_velocity = vel;
    }
}

#[cfg(test)]
mod tests {
    use super::{arrival_hit, ArrivalSolids};
    use skate_core::physics::teleport_ground_fit::GroundFit;
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};

    const H: f32 = crate::physics::ground::HEIGHT;

    fn material() -> skate_core::physics::contact::RetailContactMaterial {
        skate_core::physics::contact::RetailContactMaterial { static_friction: 0.5, dynamic_friction: 0.5, restitution: 0.0 }
    }

    /// A 4 m plate at `H + 1` around x = 10, as a props world.
    fn props_plate() -> skate_core::physics::board_world::BoardWorld {
        let y = H + 1.0;
        let plate = [[[8., y, -2.], [8., y, 2.], [12., y, 2.]], [[8., y, -2.], [12., y, 2.], [12., y, -2.]]];
        let pieces = crate::park_layout::Pieces {
            collision: plate.iter().map(|&points| skate_data::skate_map::Collision {
                points, surface: 0, material: 1, native_edges: Some([0x60; 3]),
            }).collect(),
            ranges: vec![0..2], materials: vec![], rails: vec![],
            fuse: crate::park_layout::FuseRules::default(), unfused: vec![true], tags: vec![],
        };
        crate::skate_world::props_only_world(&pieces, material()).unwrap().unwrap()
    }

    /// A 1.5 m tall box solid at x = 0 (top at `H + 1.5`) in `group`.
    fn car(group: u32) -> (usize, skate_dynamics::SolidBody) {
        let centre = Vector::new(0., H + 0.75, 0.);
        (0, skate_dynamics::SolidBody {
            id: 5,
            pose: Pose::from_parts(centre, Rotation::IDENTITY),
            center_of_mass: centre,
            inertia_rotation: Rotation::IDENTITY,
            inverse_mass: 0.0,
            inverse_inertia: Vector::new(0.0, 0.0, 0.0),
            linvel: Vector::new(0.0, 0.0, 0.0),
            angvel: Vector::new(0.0, 0.0, 0.0),
            contact_group: group,
            colliders: vec![skate_dynamics::SolidCollider {
                shape: SharedShape::cuboid(1.0, 0.75, 2.0), pose: Pose::from_parts(centre, Rotation::IDENTITY), friction: 0.5,
            }],
        })
    }

    fn height(arrival: ArrivalSolids<'_>, fit: GroundFit, x: f32) -> f32 {
        let world = crate::physics::ground::Terrain::Flat.world(material());
        arrival_hit(&world, arrival, fit, [x, H + 5.0, 0.], [x, H - 50.0, 0.]).unwrap()[1]
    }

    /// Retail's fit-up mask 7 (b493): the ray lands on props and traffic cars, nearest first; each is a data switch.
    #[test]
    fn the_fit_up_ray_hits_props_and_cars() {
        let props = props_plate();
        let cars = [car(crate::physics::VEHICLE_GROUP)];
        let arrival = ArrivalSolids { props: Some(&props), proxies: &cars };
        let fit = GroundFit::default();
        assert!((height(arrival, fit, 10.0) - (H + 1.0)).abs() < 1e-3, "prop");
        assert!((height(arrival, fit, 0.0) - (H + 1.5)).abs() < 1e-3, "car");
        assert!((height(arrival, fit, -5.0) - H).abs() < 1e-3, "floor");
        let off = GroundFit { props: false, vehicles: false, ..fit };
        assert!((height(arrival, off, 10.0) - H).abs() < 1e-3);
        assert!((height(arrival, off, 0.0) - H).abs() < 1e-3);
        // Other solve proxies (peds, NPC skaters, mod bodies) are not in the mask.
        let ped = [car(crate::living_world::ped_body::PED_GROUP)];
        assert!((height(ArrivalSolids { props: None, proxies: &ped }, fit, 0.0) - H).abs() < 1e-3);
    }
}