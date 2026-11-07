//! Host game physics ownership and schedule. Physical calculations stay in core.
/// Shared stock physics_mode key for loaded settings and per-frame mode packets.
#[cfg(test)]
pub(crate) const PHYSICS_MODE: &str = "easy";

mod air_phase;
mod air_reckoning;
mod air_trajectory;
mod animated_skeleton;
pub(crate) mod camera_output;
mod clock;
pub(crate) mod colliders;
mod controls;
mod manual_landing_log;
mod foot_ik;
mod footplant;
mod climbing;
mod plant_skeleton;
mod boneless;
mod handplant;
mod foot_ik_queries;
mod foot_physical_output;
pub(crate) mod ground;
mod render_pose;
mod world_handle;
pub(crate) use world_handle::WorldHandle;
mod riding_outputs;
mod skateboard_controller;
mod skater;
mod skeleton_air;
pub(crate) mod skeleton_body;
pub(crate) mod skeleton_colliders;
mod skeleton_controller;
mod skeleton_feedback;
mod skeleton_input_runtime;
mod skeleton_output;
mod solve;
pub(crate) mod solid_contacts;
pub(crate) mod network;
pub(crate) use skater::SkaterRuntime;
pub(crate) use skater::{AiPhysicsSource, AiRecordCursor};
pub(crate) use skater::Takedown;
pub(crate) use offboard::mod_solid_ground::VEHICLE_GROUP;
pub(crate) use input_phase::facing_from_visual;
mod animation_feedback;
mod animation_feedback_settings;
mod animation_input;
mod animation_phase;
mod biped_ground;
#[cfg(test)]
mod board_away_tests;
#[cfg(test)]
mod air_timeout_tests;
#[cfg(test)]
mod boundary_tests;
mod board_path;
#[cfg(test)]
mod board_path_tests;
#[cfg(test)]
mod skater_context_tests;
#[cfg(test)]
mod carry_direction_tests;
mod frame;
pub(crate) mod startup_check;
#[cfg(debug_assertions)]
mod dev_trace;
mod grind;
mod grind_air_settings;
mod grind_camera;
mod grind_chromosome;
mod grind_host;
mod grind_materials;
mod ground_animation;
mod grind_trick;
mod slide_state;
mod skitch_state;
mod revert_state;
mod ground_exit;
mod ground_phase;
mod ground_runtime;
pub(crate) use ground_runtime::set_skitch_enabled;
mod input_phase;
mod landing_quality;
mod offboard;
mod player_input;
mod player_state;
pub(crate) mod grab_view;
pub(crate) mod prop_carry;
pub(crate) mod prop_dynamics;
pub(crate) mod prop_layout;
pub(crate) mod prop_water;
mod settings;
mod skeleton_grind_air;
mod teleport_state;
mod wipeout;
mod wipeout_states;
pub(crate) mod respawn;
//TEMPORARY opt-in observations for the bottom-up source audit.
mod biped_air;
mod known_air;
mod landing_on_deck;
mod offboard_audit_trace;
use skate_data::collections::Collections;
use crate::{
    app::{FrameSet, SimulationSet},
    world::PlayerRoot,
};
use bevy::prelude::*;
pub(crate) use controls::PlayerControls;
use riding_outputs::RidingOutputs;
use settings::PhysicsSettings;
#[cfg(test)]
use skate_core::physics::board::BodyId;
use skate_core::{
    math::Vector3,
    physics::{
        board_runtime::{BoardMotion, BoardRuntime},
        board_world::{BoardWorld, BoardWorldVolume, ContactRetentionSettings},
        collision::WorldContactSettings,
        drive_frames::RetailAffineTransform,
    },
};


#[derive(Resource)]
pub(crate) struct GamePhysics {
    /// The per-skater collision settings as loaded with the map (new NPC contexts start from these).
    initial_skater_settings: SkaterSettings,
    /// An NPC worker's own empty grab scene, held while it does not share the real one (`release_npc_worker`).
    parked_grab_scene: Option<std::sync::Arc<offboard::grab_scene::Registry>>,
    pub(crate) network_proxies: network::Proxies,
    /// Other skaters' push volumes for the dynamic prop step: (stable actor id, volume), sorted by
    /// id. Rewritten each tick before the solve by `living_world::npc_skaters::push_proxies`
    /// (NPC skater body + board, doc 26 fix 19); empty when nothing fills it.
    pub(crate) actor_prop_volumes: Vec<(u64, BoardWorldVolume)>,
    /// The traffic cars as on-foot ground this tick (`living_world::vehicles::publish_on_foot_solids`); only the
    /// on-foot queries (`biped_world`) read them.
    pub(crate) on_foot_vehicles: Option<std::sync::Arc<crate::modding::bridge::MovingQueries>>,
    /// Near peds' body A for the dynamic prop step: (body A id, volume, push mass kg), sorted by id. Rewritten each
    /// tick by `living_world::ped_body::push_ped_proxies` (b342); its own list, so the NPC proxy system clearing
    /// [`Self::actor_prop_volumes`] cannot drop it.
    pub(crate) ped_prop_volumes: Vec<(u64, BoardWorldVolume, f32)>,
    /// The peds' body A displacements the last prop steps solved (`living_world::ped_body::push_ped_proxies` applies
    /// them before the next step): (body A id, displacement, contact normal along it).
    pub(crate) ped_prop_shoves: Vec<(u64, Vector3, Vector3, Vector3)>,
    pub(crate) network_active: bool,
    pub(crate) network_contacts: usize,
    clock: clock::SimulationClock,
    pub board: BoardRuntime,
    /// Where the simulated NPC skaters' body proxies start in `network_proxies` (appended last by
    /// `living_world::npc_skaters::push_proxies`): each NPC's own solve drops them and adds the player and the other
    /// NPCs instead (`npc_sim::run_steps`), so no NPC collides with a copy of itself.
    pub(crate) npc_sim_proxy_mark: Option<network::ProxyMark>,
    pub riding: RidingOutputs,
    world: world_handle::WorldHandle,
    /// DMO prop instances as a separate collision layer so dynamic bodies can
    /// re-bake their triangle ranges (Phase 2).
    prop_layer: Option<crate::skate_world::PropCollisionLayer>,
    /// Shared with NPC skater workers (read only there); written only outside the NPC phase.
    prop_dynamics: Option<std::sync::Arc<prop_dynamics::PropDynamics>>,
    /// Offboard prop carry state (Phase 3); one held prop at a time.
    pub(crate) prop_carry: prop_carry::PropCarry,
    grind_world: std::sync::Arc<crate::grind_world::StaticProvider>,
    /// The living-world cars' rails at this tick's car poses (`set_car_rails`), after the props in the rail query.
    car_rails: crate::grind_world::props::PropRailPoses,
    grind_materials: grind_materials::GrindMaterials,
    /// Shared with NPC skater workers (read only there).
    offboard_grab_scene: std::sync::Arc<offboard::grab_scene::Registry>,
    pub(crate) settings: PhysicsSettings,
    animation_profile: animation_phase::AnimationProfile,
    query: WorldContactSettings,
    retention: ContactRetentionSettings,
    pub ticks: u64,
    pub contact_count: usize,
    pub failed: bool,
    exchange: SimulationExchange,
    /// ProcessedPhysIn reset82BF9EF0 sets0x2000; initial stancebit20 is clear.
    /// Animation packet publication owns subsequent stance-bit updates.
    pub processed_flags_2468: u32,
    /// Toolkit ctor82C0680C clears8384bit7; wipeout entry/exit owns changes.
    pub board_wiping_out: bool,
    /// The setup collections the per-skater parts load from (a simulated NPC skater's riding
    /// outputs at spawn, [`Self::new_skater_context`]).
    collections: std::sync::Arc<Collections>,
    /// Whether the skater in the context steps the dynamic props (the local player). A simulated
    /// NPC skater's context does not: the props step once per tick, the NPCs push them through
    /// [`Self::actor_prop_volumes`].
    owns_props: bool,
}

/// The parts of [`GamePhysics`] each simulated skater owns, swapped in around its tick.
pub(crate) struct SkaterPhysicsContext {
    clock: clock::SimulationClock,
    board: BoardRuntime,
    riding: RidingOutputs,
    prop_carry: prop_carry::PropCarry,
    ticks: u64,
    contact_count: usize,
    failed: bool,
    exchange: SimulationExchange,
    processed_flags_2468: u32,
    board_wiping_out: bool,
    owns_props: bool,
    /// The per-skater collision settings the state handlers write (`SkaterSettings`).
    skater_settings: SkaterSettings,
}

/// Mutable access to state shared with NPC skater workers: only outside the NPC phase (no worker alive then).
fn shared_mut<T>(shared: &mut std::sync::Arc<T>) -> &mut T {
    std::sync::Arc::get_mut(shared).expect("physics state shared with an NPC worker is changed only outside the NPC phase")
}

/// Per-skater collision settings in the shared `PhysicsSettings`, swapped with each simulated NPC's context.
#[derive(Clone, Debug)]
pub(crate) struct SkaterSettings {
    wheel_material: skate_core::physics::contact::RetailContactMaterial,
    truck_material: skate_core::physics::contact::RetailContactMaterial,
    deck_material: skate_core::physics::contact::RetailContactMaterial,
    truck_collisions: bool,
    deck_children: Vec<bool>,
}

impl SkaterSettings {
    fn of(settings: &settings::PhysicsSettings) -> Self {
        Self {
            wheel_material: settings.wheel_material,
            truck_material: settings.truck_material,
            deck_material: settings.deck_material,
            truck_collisions: settings.truck_collisions,
            deck_children: settings.deck_geometry.children.iter().map(|c| c.collision_enabled).collect(),
        }
    }

    fn swap_with(&mut self, settings: &mut settings::PhysicsSettings) {
        std::mem::swap(&mut self.wheel_material, &mut settings.wheel_material);
        std::mem::swap(&mut self.truck_material, &mut settings.truck_material);
        std::mem::swap(&mut self.deck_material, &mut settings.deck_material);
        std::mem::swap(&mut self.truck_collisions, &mut settings.truck_collisions);
        for (enabled, child) in self.deck_children.iter_mut().zip(settings.deck_geometry.children.iter_mut()) {
            std::mem::swap(enabled, &mut child.collision_enabled);
        }
    }
}

/// Cross-phase records for the current fixed tick. Subsystems retain their
/// private native-shaped storage; only these buffers cross the coordinator.
pub(crate) struct SimulationExchange {
    commands: skate_core::physics::phase::PhysicsCommandBuffer,
    events: skate_core::physics::phase::PhysicsEventBuffer,
    physical_output: Option<skate_core::physics::phase::PhysicalOutputSnapshot>,
}

impl SimulationExchange {
    fn new(tick: u64) -> Self {
        Self {
            commands: skate_core::physics::phase::PhysicsCommandBuffer::new(tick),
            events: skate_core::physics::phase::PhysicsEventBuffer::new(tick),
            physical_output: None,
        }
    }

    fn emit_event(
        &mut self,
        tick: u64,
        event: skate_core::physics::phase::PhysicsEvent,
    ) -> Result<(), String> {
        self.events.emit(tick, event)
    }

    fn publish_output(&mut self, output: skate_core::physics::phase::PhysicalOutputSnapshot) {
        self.physical_output = Some(output);
    }

    fn output(&self) -> Option<&skate_core::physics::phase::PhysicalOutputSnapshot> {
        self.physical_output.as_ref()
    }

    fn events(&self) -> &[skate_core::physics::phase::PhysicsEvent] {
        self.events.events()
    }

    pub(super) fn request_state(
        &mut self,
        state: skate_core::player::state::PhysicalStateId,
    ) -> Result<(), String> {
        let tick = self.commands.tick();
        self.commands.push(
            tick,
            skate_core::physics::phase::PhysicsCommand::RequestState(state),
        )
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;

    #[test]
    fn exchange_keeps_events_on_the_authoritative_tick() {
        let mut exchange = SimulationExchange::new(8);
        assert!(
            exchange
                .emit_event(
                    7,
                    skate_core::physics::phase::PhysicsEvent::StateChanged {
                        from: skate_core::player::state::PhysicalStateId::PhysicsGround,
                        to: skate_core::player::state::PhysicalStateId::PhysicsAir,
                    },
                )
                .is_err()
        );
        assert!(
            exchange
                .emit_event(
                    8,
                    skate_core::physics::phase::PhysicsEvent::StateChanged {
                        from: skate_core::player::state::PhysicalStateId::PhysicsGround,
                        to: skate_core::player::state::PhysicalStateId::PhysicsAir,
                    },
                )
                .is_ok()
        );
        assert!(
            exchange
                .request_state(skate_core::player::state::PhysicalStateId::PhysicsAir)
                .is_ok()
        );
        assert_eq!(exchange.commands.tick(), 8);
        assert_eq!(exchange.commands.commands().len(), 1);
        assert_eq!(exchange.events().len(), 1);
    }
}

impl GamePhysics {
    /// The setup collections every skater part loads from (read once per world).
    pub(crate) fn collections(&self) -> &Collections {
        &self.collections
    }
    /// This tick's traffic cars in the grab scene (`living_world::vehicles::push_vehicle_grab_splines`).
    pub(crate) fn set_car_rails(&mut self, rails: crate::grind_world::props::PropRailPoses) {
        self.car_rails = rails;
    }
    pub(crate) fn set_grab_cars(&mut self, cars: Vec<skate_core::player::offboard::grab_scene::Object>) -> Result<(), String> {
        shared_mut(&mut self.offboard_grab_scene).set_cars(cars)
    }
    /// This tick's props in the grab scene (`PropDynamics::grab_objects`). Opt-in with `SKATE_PROP_GRAB=1` until
    /// Move Object reads the authored record (doc 26i "Move Object step 2"); off, the scene holds no props.
    pub(crate) fn refresh_grab_props(&mut self) -> Result<(), String> {
        if !prop_dynamics::prop_grab_enabled() {
            // Switched off in the PC overlay: the props leave the scene once.
            return if self.offboard_grab_scene.has_props() { shared_mut(&mut self.offboard_grab_scene).set_props(Vec::new()) } else { Ok(()) };
        }
        let props = self.prop_dynamics.as_ref().map(|d| d.grab_objects()).unwrap_or_default();
        shared_mut(&mut self.offboard_grab_scene).set_props(props)
    }
    pub(crate) fn set_gesture_preferences(&mut self, gestures: Option<[u32; 4]>) {
        self.animation_profile.gesture_selections = Some(gestures.filter(|g| g.iter().all(|v| *v < 37)).unwrap_or([0, 1, 2, 3]));
    }
    pub(crate) fn set_equipment_preferences(&mut self, truck: f32, wheel: f32) {
        if truck.is_finite() && wheel.is_finite() {
            self.animation_profile.truck_tightness = truck.clamp(0.0, 1.0);
            self.animation_profile.wheel_hardness = wheel.clamp(0.0, 1.0);
        }
    }
    pub(crate) fn set_difficulty(&mut self, difficulty: crate::difficulty::Difficulty) {
        // Actor publication carries this selector into the next physical packet.
        // Keep the board, active trick, equipment preferences and controller history.
        self.animation_profile.physics_mode = difficulty as u32;
    }
    pub(crate) fn period(&self) -> std::time::Duration { self.clock.period() }

    pub(crate) fn difficulty_index(&self) -> u32 { self.animation_profile.physics_mode }

    pub(crate) fn world_triangles(&self) -> &[skate_core::physics::board_world::WorldTriangle] { self.world.triangles() }

    pub(crate) fn set_external_queries(&mut self, queries: Option<std::sync::Arc<dyn skate_core::physics::board_world::ExternalQueries>>) {
        self.world.set_external_queries(queries);
    }
    /// The last board query's static-world contacts.
    pub(crate) fn world_contacts(&self) -> &[skate_core::physics::board_step::BoardCollision] {
        self.world.contacts()
    }

    pub(crate) fn world(&self) -> &BoardWorld {
        &self.world
    }

    /// What the on-foot queries see: the map and the prop world (`offboard::BipedWorld`).
    pub(crate) fn biped_world(&self) -> offboard::BipedWorld<'_> {
        offboard::BipedWorld { map: &self.world, props: self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::world), vehicles: self.on_foot_vehicles.as_deref() }
    }

    pub(crate) fn prop_world(&self) -> Option<&BoardWorld> {
        self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::world)
    }

    pub(crate) fn prop_world_mut(&mut self) -> Option<&mut BoardWorld> {
        self.prop_layer.as_mut().map(crate::skate_world::PropCollisionLayer::world_mut)
    }

    /// The prop world's query handle (its own scratch; the world is only read).
    pub(crate) fn prop_query(&mut self) -> Option<&mut WorldHandle> {
        self.prop_layer.as_mut().map(crate::skate_world::PropCollisionLayer::world_handle_mut)
    }

    pub(crate) fn prop_dynamics(&self) -> Option<&prop_dynamics::PropDynamics> {
        self.prop_dynamics.as_deref()
    }

    pub(crate) fn prop_dynamics_mut(&mut self) -> Option<&mut prop_dynamics::PropDynamics> {
        self.prop_dynamics.as_mut().map(shared_mut)
    }

    /// Better Water's props in water ([`prop_dynamics::PropDynamics::set_water`]); how many props woke.
    pub(crate) fn set_prop_water(&mut self, water: Option<prop_water::PropWater>) -> Option<usize> {
        let dynamics = self.prop_dynamics.as_mut().map(shared_mut)?;
        Some(dynamics.set_water(water, &self.world))
    }

    /// Push, integrate and re-bake dynamic props against the static world.
    /// `volumes` are the skater's board and skeleton world volumes; the NPC skaters' volumes
    /// ([`Self::actor_prop_volumes`]) push props by the same rule.
    pub(crate) fn step_props(&mut self, volumes: &[BoardWorldVolume]) {
        if !self.owns_props {
            return;
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut))
        else {
            return;
        };
        // The bail clamp's gate: the local skater in a wipeout (retail's actor byte is not decoded; b208).
        dynamics.set_bailing(if self.board_wiping_out { &[prop_dynamics::LOCAL_PUSHER] } else { &[] });
        // Peds' body A after the skaters (b342): it pushes with its own mass.
        let masses: Vec<(u64, f32)> = self.ped_prop_volumes.iter().map(|(id, _, mass)| (*id, *mass)).collect();
        dynamics.set_actor_push_mass(&masses);
        if self.ped_prop_volumes.is_empty() {
            dynamics.step_with_actors(&self.world, layer, volumes, &self.actor_prop_volumes);
        } else {
            let mut others = self.actor_prop_volumes.clone();
            others.extend(self.ped_prop_volumes.iter().map(|(id, v, _)| (*id, *v)));
            dynamics.step_with_actors(&self.world, layer, volumes, &others);
        }
        let mut shoves = Vec::new();
        dynamics.take_actor_shoves(&mut shoves);
        for (id, shove, normal, velocity) in shoves {
            if !self.ped_prop_volumes.iter().any(|v| v.0 == id) {
                continue;
            }
            match self.ped_prop_shoves.iter_mut().find(|s| s.0 == id) {
                Some(s) => {
                    s.1 = Vector3::new(s.1.x + shove.x, s.1.y + shove.y, s.1.z + shove.z);
                    s.3 = velocity;
                }
                None => self.ped_prop_shoves.push((id, shove, normal, velocity)),
            }
        }
        // Each prop's retail collision type on its query meshes (heavy 12 / light 14 / held 13), which the on-foot
        // walker's queries filter.
        dynamics.for_each_collision_type(self.prop_carry.held(), |instance, collision_type| {
            layer.set_collision_type(instance, collision_type);
        });
    }

    /// Reset ONE object (cMsgResetDMO: 8289A048 -> 82C4B5F0 -> 82C4B780); the single authority for resets: docs/hails-additions/27-dynamic-props.md#reset-one-prop
    pub(crate) fn reset_prop(&mut self, id: u32) -> bool {
        if self.prop_carry.held() == Some(id) {
            return false;
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut))
        else {
            return false;
        };
        let Some(instance) = dynamics.reset_to_spawn(id) else { return false };
        if let Some((origin, basis)) = dynamics.spawn_pose(id) {
            if let Err(error) = layer.rebake(instance, basis.columns, origin) {
                warn!("SKATE_PROP_RESET: rebake {id}: {error}");
            }
        }
        self.prop_carry.forget_layout(&[id]);
        info!("SKATE_PROP_RESET id={id}");
        true
    }

    /// Stream a map prop out (`dormant`: body asleep and still, collision parked, model hidden) or back in (at its
    /// authored pose when `authored`, retail; else where it was left) for the DMO census (`living_world::dmo_stream`).
    /// Refused for the held prop and unknown ids. The single authority for streaming.
    pub(crate) fn stream_prop(&mut self, id: u32, dormant: bool, authored: bool) -> bool {
        self.stream_props(&[(id, dormant, authored)])[0]
    }

    /// [`Self::stream_prop`] for a whole stream pass `(id, dormant, authored)`, in order, with one collision rebake
    /// at the end: each prop used to rebuild the whole collision index (b111: ~410 ms for the first pass). Nothing
    /// reads prop collision between the props of a pass, and the batched rebake gives the identical index.
    pub(crate) fn stream_props(&mut self, props: &[(u32, bool, bool)]) -> Vec<bool> {
        let held = self.prop_carry.held();
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut)) else {
            return vec![false; props.len()];
        };
        let mut poses = Vec::new();
        let mut ids = std::collections::HashMap::new();
        let done = props
            .iter()
            .map(|&(id, dormant, authored)| {
                if held == Some(id) {
                    return false;
                }
                let Some((instance, origin, basis)) = dynamics.set_dormant(id, dormant, authored) else { return false };
                poses.push((instance, basis.columns, origin));
                ids.insert(instance, id);
                true
            })
            .collect();
        for (instance, error) in layer.rebake_many(&poses) {
            warn!("SKATE_PROP_STREAM: rebake {}: {error}", ids.get(&instance).copied().unwrap_or(u32::MAX));
        }
        done
    }

    /// Create a prop body mid-game (released hand prop, mod prop); the single authority; returns the body id.
    pub(crate) fn spawn_runtime_prop(&mut self, spec: &prop_dynamics::RuntimeProp, surface: u32) -> Option<u32> {
        if !self.owns_props {
            return None;
        }
        if self.prop_layer.is_none() {
            match crate::skate_world::PropCollisionLayer::empty(self.settings.floor_material) {
                Ok(layer) => self.prop_layer = Some(layer),
                Err(error) => {
                    warn!("SKATE_PROP_SPAWN: empty layer: {error}");
                    return None;
                }
            }
        }
        if self.prop_dynamics.is_none() {
            self.prop_dynamics =
                Some(std::sync::Arc::new(prop_dynamics::PropDynamics::empty(prop_dynamics::prop_simulation(self.settings.step.simulation))));
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut)) else {
            return None;
        };
        let id = dynamics.next_runtime_id();
        let instance = match layer.add_instance(id, spec.local.clone(), surface, spec.basis.columns, spec.origin) {
            Ok(instance) => instance,
            Err(error) => {
                warn!("SKATE_PROP_SPAWN: {}: {error}", spec.template);
                return None;
            }
        };
        if !dynamics.spawn_body(instance, id, spec) {
            if let Err(error) = layer.retire_instance(instance, prop_dynamics::HELD_PARK) {
                warn!("SKATE_PROP_SPAWN: retire {id}: {error}");
            }
            warn!("SKATE_PROP_SPAWN: {}: no body", spec.template);
            return None;
        }
        info!("SKATE_PROP_SPAWN id={id} template={} instance={instance}", spec.template);
        Some(id)
    }

    /// A runtime prop spec copying prop `from` (template, collision triangles, physics block,
    /// type data) plus its packed surface, for [`Self::spawn_runtime_prop`] (mods' copies).
    pub(crate) fn runtime_copy_of(&self, from: u32) -> Option<(prop_dynamics::RuntimeProp, u32)> {
        let (layer, dynamics) = (self.prop_layer.as_ref()?, self.prop_dynamics.as_ref()?);
        let mut spec = dynamics.copy_spec(from)?;
        let entry = layer.instances().get(dynamics.instance_of(from)?)?;
        spec.local = entry.local_points().to_vec();
        let surface = layer.world().triangles().get(entry.range.start)?.tag;
        Some((spec, surface))
    }

    /// Remove a prop created with [`Self::spawn_runtime_prop`] (retail removes a released hand
    /// prop's DMO); its collision slot is parked for reuse. Refused (false) for map props and
    /// unknown ids. The single authority for removals.
    pub(crate) fn remove_runtime_prop(&mut self, id: u32) -> bool {
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut)) else {
            return false;
        };
        let Some(instance) = dynamics.remove_body(id) else { return false };
        if let Err(error) = layer.retire_instance(instance, prop_dynamics::HELD_PARK) {
            warn!("SKATE_PROP_REMOVE: retire {id}: {error}");
        }
        info!("SKATE_PROP_REMOVE id={id}");
        true
    }

    /// Upright ONE object (cMsgUprightDMO -> 82C4B8C0, 2 s window); false for unknown or non-dynamic bodies.
    pub(crate) fn upright_prop(&mut self, id: u32) -> bool {
        let Some(dynamics) = self.prop_dynamics.as_mut().map(shared_mut) else { return false };
        let started = dynamics.upright(id);
        if started {
            info!("SKATE_PROP_UPRIGHT id={id}");
        }
        started
    }

    /// Mod convenience: reset every moved or placed prop in id order (NOT RETAIL YET: no retail reset-all found).
    pub(crate) fn reset_moved_props(&mut self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.prop_dynamics.as_ref().map(|d| d.moved_ids()).unwrap_or_default();
        ids.extend(self.prop_carry.layout().keys().copied());
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|&id| self.reset_prop(id));
        ids
    }

    /// Offboard grab/carry/place of dynamic props (Phases 3-4).
    pub(crate) fn update_prop_carry(&mut self, tick: prop_carry::Tick, carrier: prop_carry::Carrier) {
        if !self.owns_props {
            // A simulated NPC (its step may run on a worker that only reads the shared props): with nothing held and
            // no grab, `PropCarry::update` returns before it touches the props, so skipping it is identical. NPCs do
            // not grab today (neutral pad, no grab intent); if one ever does, it is logged and skipped.
            if self.prop_carry.held().is_some() || tick.grab {
                bevy::log::warn_once!("NPC_SKATER_SIM: a simulated NPC asked to carry a prop; not supported, skipped");
            }
            return;
        }
        let previous = self.prop_carry.held();
        if let Some(dynamics) = self.prop_dynamics.as_mut().map(shared_mut) {
            self.prop_carry.update(dynamics, tick, carrier);
        }
        let current = self.prop_carry.held();
        if previous == current {
            return;
        }
        if let Some(dynamics) = self.prop_dynamics.as_mut().map(shared_mut) {
            dynamics.set_held(current);
        }
        // Park the newly held prop's triangles far below the world so skater
        // queries cannot be pushed by it; restore a dropped prop's triangles
        // at its final pose.
        let (Some(layer), Some(dynamics)) =
            (self.prop_layer.as_mut(), self.prop_dynamics.as_mut().map(shared_mut))
        else {
            return;
        };
        if let Some(id) = current {
            if let (Some(instance), Some((_, basis))) =
                (dynamics.instance_of(id), dynamics.pose(id))
            {
                if let Err(error) =
                    layer.rebake(instance, basis.columns, prop_dynamics::HELD_PARK)
                {
                    warn!("SKATE_PROP_CARRY: park {id}: {error}");
                }
            }
        } else if let Some(id) = previous {
            if let (Some(instance), Some((origin, basis))) =
                (dynamics.instance_of(id), dynamics.pose(id))
            {
                if let Err(error) = layer.rebake(instance, basis.columns, origin) {
                    warn!("SKATE_PROP_CARRY: unpark {id}: {error}");
                }
            }
        }
    }

    /// Flat-world convenience used by private-asset integration tests.
    #[cfg(test)]
    pub fn load(asset_root: &std::path::Path) -> Result<Self, String> {
        Self::load_with_terrain(asset_root, ground::Terrain::Flat)
    }

    #[cfg(test)]
    pub(crate) fn load_with_terrain(
        asset_root: &std::path::Path,
        terrain: ground::Terrain,
    ) -> Result<Self, String> {
        Self::load_with_world(asset_root, terrain, None)
    }

    #[cfg(test)]
    pub(crate) fn load_with_world(
        asset_root: &std::path::Path,
        terrain: ground::Terrain,
        map: Option<&skate_data::skate_map::SkateMap>,
    ) -> Result<Self, String> {
        Self::load_world_difficulty(asset_root, terrain, map, crate::difficulty::Difficulty::Easy)
    }

    pub fn load_with_difficulty(asset_root: &std::path::Path, map: Option<&skate_data::skate_map::SkateMap>, difficulty: crate::difficulty::Difficulty) -> Result<Self, String> {
        Self::load_world_difficulty(asset_root, ground::Terrain::Course, map, difficulty)
    }

    fn load_world_difficulty(asset_root: &std::path::Path, terrain: ground::Terrain, map: Option<&skate_data::skate_map::SkateMap>, difficulty: crate::difficulty::Difficulty) -> Result<Self, String> {
        let data = crate::custom_difficulty::load_collections(asset_root)?;
        let settings = PhysicsSettings::load(&data)?;
        let animation_profile = animation_phase::AnimationProfile::load(&data, difficulty.profile_key())?;
        eprintln!(
            "SKATE_PHYSICS_MODE {} index={}",
            difficulty.key(), animation_profile.physics_mode
        );
        let mut spawn = RetailAffineTransform {
            translation: Vector3::new(
                0.0,
                ground::HEIGHT + settings.wheel_radius - settings.authored[0].translation.y,
                0.0,
            ),
            ..RetailAffineTransform::IDENTITY
        };
        if let Some(map) = map {
            // Package spawn is the wheel-ground anchor, in native Y-up metres.
            spawn.translation = Vector3::new(
                map.spawn[0],
                map.spawn[1] + settings.wheel_radius - settings.authored[0].translation.y,
                map.spawn[2],
            );
            spawn.basis = skate_core::math::Basis3 {
                columns: Mat3::from_rotation_y(map.heading).to_cols_array_2d(),
            };
        }
        let board = BoardRuntime::new(
            settings.masses,
            settings.authored,
            spawn,
            settings.step.simulation,
            BoardMotion::Active,
        );
        // The skate.Park layout the map was loaded with (park_layout.rs): fixed pieces in the static world and rails.
        let pieces = map.and_then(|map| crate::park_layout::load_pieces(asset_root, map));
        let world = match map {
            Some(map) => crate::skate_world::collision_world_with(map, settings.floor_material, pieces.as_ref())?,
            None => terrain.world(settings.floor_material),
        };
        let prop_layer = map.and_then(|map| {
            crate::skate_world::load_map_prop_layer(
                asset_root,
                map,
                settings.floor_material,
                prop_dynamics::prop_simulation(settings.step.simulation),
            )
        });
        let (mut prop_layer, mut prop_dynamics) = match prop_layer {
            Some((layer, dynamics)) => (Some(layer), Some(dynamics)),
            None => (None, None),
        };
        if let (Some(pieces), Some(dynamics)) = (pieces.as_ref(), prop_dynamics.as_mut()) {
            dynamics.set_props_only_world(crate::skate_world::props_only_world(pieces, settings.floor_material)?);
        }
        // Phase 4: apply the saved layout sidecar over the authored poses.
        let mut prop_carry = prop_carry::PropCarry::default();
        if let (Some(map), Some(layer), Some(dynamics)) = (map, prop_layer.as_mut(), prop_dynamics.as_mut()) {
            // A skate.Park layout's movable pieces keep their own saved poses (keyed by the layout; ours).
            let (_, _, name) = crate::skate_world::prop_files(asset_root, map);
            let path = prop_layout::path(asset_root, &name);
            if let Some(layout) = prop_layout::load(&path, &name) {
                for pose in layout.values() {
                    let origin = Vector3::new(pose.origin[0], pose.origin[1], pose.origin[2]);
                    let basis = skate_core::math::Basis3 { columns: pose.basis };
                    if let Some(instance) = dynamics.teleport(pose.id, origin, basis) {
                        if let Err(error) = layer.rebake(instance, basis.columns, origin) {
                            warn!("SKATE_PROP_LAYOUT: rebake {}: {error}", pose.id);
                        }
                    } else {
                        warn!("SKATE_PROP_LAYOUT: unknown prop id {}", pose.id);
                    }
                }
                prop_carry = prop_carry::PropCarry::with_layout(layout, Some(path));
            }
        }
        prop_carry.set_base_tuning(settings.move_object);
        let grind_world = std::sync::Arc::new(if map.is_none() && terrain == ground::Terrain::Course {
            crate::grind_world::StaticProvider::authored(&crate::grind_world::test_rails())?
        } else {
            let provider = crate::grind_world::StaticProvider::new(map)?;
            match &pieces { Some(pieces) => provider.with_pieces(&pieces.rails)?, None => provider }
        });
        if let Some(map) = map {
            eprintln!(
                "SKATE_GRIND_READY splines={} primitives={}",
                map.rails.len(),
                grind_world.primitives().len()
            );
        }
        let grind_materials = grind_materials::GrindMaterials::new(&settings);
        //Both authored terrains contain static collision surfaces only, with
        //no interactable objects or assembly-bound grab splines. Do not infer
        //those identities from triangle/mesh IDs. Queries use the real registry.
        let offboard_grab_scene =
            offboard::grab_scene::Registry::new(&world, Vec::new(), Vec::new())?;
        let processed_flags_2468 = 0x2000;
        let riding = RidingOutputs::load(&data, &board, processed_flags_2468)?;
        let (query, retention) = ground::query_settings();
        let initial_skater_settings = SkaterSettings::of(&settings);
        Ok(Self {
            network_proxies: network::Proxies::default(),
            npc_sim_proxy_mark: None,
            actor_prop_volumes: Vec::new(),
            on_foot_vehicles: None,
            ped_prop_volumes: Vec::new(),
            ped_prop_shoves: Vec::new(),
            network_active: false,
            network_contacts: 0,
            clock: clock::SimulationClock::default(),
            board,
            riding,
            world: world.into(),
            prop_layer,
            prop_dynamics: prop_dynamics.map(std::sync::Arc::new),
            prop_carry,
            grind_world,
            grind_materials,
            offboard_grab_scene: std::sync::Arc::new(offboard_grab_scene),
            car_rails: Default::default(),
            settings,
            animation_profile,
            query,
            retention,
            ticks: 0,
            contact_count: 0,
            failed: false,
            exchange: SimulationExchange::new(0),
            processed_flags_2468,
            board_wiping_out: false,
            collections: std::sync::Arc::new(data),
            owns_props: true,
            initial_skater_settings,
            parked_grab_scene: None,
        })
    }

    /// A worker `GamePhysics` for one simulated NPC on another thread, sharing worlds; never steps props.
    pub(crate) fn npc_worker(&self) -> Result<GamePhysics, String> {
        let spawn = RetailAffineTransform::IDENTITY;
        let parts = self.new_skater_context(spawn)?;
        Ok(GamePhysics {
            initial_skater_settings: self.initial_skater_settings.clone(),
            network_proxies: self.network_proxies.clone(),
            npc_sim_proxy_mark: None,
            actor_prop_volumes: self.actor_prop_volumes.clone(),
            on_foot_vehicles: self.on_foot_vehicles.clone(),
            ped_prop_volumes: self.ped_prop_volumes.clone(),
            ped_prop_shoves: Vec::new(),
            network_active: self.network_active,
            network_contacts: 0,
            clock: parts.clock,
            board: parts.board,
            riding: parts.riding,
            world: self.world.share(),
            prop_layer: self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::share),
            prop_dynamics: self.prop_dynamics.clone(),
            prop_carry: parts.prop_carry,
            grind_world: std::sync::Arc::clone(&self.grind_world),
            grind_materials: self.grind_materials.clone(),
            offboard_grab_scene: std::sync::Arc::clone(&self.offboard_grab_scene),
            car_rails: self.car_rails.clone(),
            settings: self.settings.clone(),
            animation_profile: self.animation_profile.clone(),
            query: self.query,
            retention: self.retention,
            ticks: parts.ticks,
            contact_count: 0,
            failed: false,
            exchange: parts.exchange,
            processed_flags_2468: parts.processed_flags_2468,
            board_wiping_out: false,
            collections: std::sync::Arc::clone(&self.collections),
            owns_props: false,
            parked_grab_scene: Some(std::sync::Arc::new(offboard::grab_scene::Registry::new(&self.world, Vec::new(), Vec::new())?)),
        })
    }

    /// Bring a worker's shared parts and per-tick copies up to this physics' state (before the NPC phase's steps):
    /// what a step through the shared physics would have read at this point of the tick.
    pub(crate) fn sync_npc_worker(&self, worker: &mut GamePhysics) {
        worker.network_proxies.clone_from(&self.network_proxies);
        worker.actor_prop_volumes.clone_from(&self.actor_prop_volumes);
        worker.ped_prop_volumes.clone_from(&self.ped_prop_volumes);
        worker.network_active = self.network_active;
        worker.world.reshare(&self.world);
        worker.prop_layer = self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::share);
        worker.prop_dynamics.clone_from(&self.prop_dynamics);
        worker.grind_world = std::sync::Arc::clone(&self.grind_world);
        worker.grind_materials.clone_from(&self.grind_materials);
        worker.offboard_grab_scene = std::sync::Arc::clone(&self.offboard_grab_scene);
        worker.car_rails.clone_from(&self.car_rails);
        // The per-skater fields in it are placeholders here: the NPC's own are swapped in for its step.
        worker.settings.clone_from(&self.settings);
        worker.animation_profile.clone_from(&self.animation_profile);
        worker.query = self.query;
        worker.retention = self.retention;
        worker.collections = std::sync::Arc::clone(&self.collections);

    }

    /// Release a worker's shares (after the NPC phase) so this physics can change the shared parts again.
    pub(crate) fn release_npc_worker(worker: &mut GamePhysics) {
        worker.world.unshare();
        worker.prop_layer = None;
        worker.prop_dynamics = None;
        if let Some(parked) = &worker.parked_grab_scene {
            worker.offboard_grab_scene = std::sync::Arc::clone(parked);
        }
    }

    /// A fresh skater context at `spawn` (a simulated NPC skater): a new board, riding outputs from
    /// the same collections, a clock and exchange at tick 0; it does not step the props.
    pub(crate) fn new_skater_context(&self, spawn: RetailAffineTransform) -> Result<SkaterPhysicsContext, String> {
        let board = BoardRuntime::new(self.settings.masses, self.settings.authored, spawn, self.settings.step.simulation, BoardMotion::Active);
        let processed_flags_2468 = 0x2000;
        let riding = RidingOutputs::load(&self.collections, &board, processed_flags_2468)?;
        let mut prop_carry = prop_carry::PropCarry::default();
        prop_carry.set_base_tuning(self.settings.move_object);
        Ok(SkaterPhysicsContext {
            clock: clock::SimulationClock::default(),
            board,
            riding,
            prop_carry,
            ticks: 0,
            contact_count: 0,
            failed: false,
            exchange: SimulationExchange::new(0),
            processed_flags_2468,
            board_wiping_out: false,
            owns_props: false,
            // The values loaded with the map, not whatever the player's current state left in the live settings.
            skater_settings: self.initial_skater_settings.clone(),
        })
    }

    /// Swap the per-skater parts with `context` (call again with the same context to swap back).
    /// A simulated NPC skater's runtime, loaded against its own context (`SkaterRuntime::load` reads
    /// the board it starts on).
    pub(crate) fn load_skater_in_context(
        &mut self,
        context: &mut SkaterPhysicsContext,
        asset_root: &std::path::Path,
        graphs: &crate::graph_runtime::StockGraphs,
        difficulty: &str,
        source: Option<std::sync::Arc<crate::skater_animation::AnimationSource>>,
    ) -> Result<SkaterRuntime, String> {
        self.swap_skater_context(context);
        // `source`: the immutable animation banks and decoded poses another body already loaded (`SkaterAnimation::load`
        // is `from_source` over a fresh `AnimationSource::load`; sharing the `Arc` gives the same runtime, no disk read).
        let result = SkaterRuntime::load_for_world(asset_root, graphs, self, difficulty, source);
        self.swap_skater_context(context);
        result
    }

    /// One tick of a simulated NPC skater in its own context: its controls with a neutral pad
    /// (retail's AI presses no pad buttons for riding; its record steers), then the same frame
    /// advance as the player. While it runs, [`log_actor`] says "npc" on this thread.
    pub(crate) fn advance_npc_skater(
        &mut self,
        context: &mut SkaterPhysicsContext,
        skater: &mut SkaterRuntime,
        controls: &mut PlayerControls,
        graphs: &crate::graph_runtime::StockGraphs,
        camera: &mut crate::camera::CameraRuntime,
        ai_intents: &[(String, f32)],
    ) -> Result<(), String> {
        STEPPING_NPC.with(|c| c.set(true));
        let result = self.advance_npc_skater_inner(context, skater, controls, graphs, camera, ai_intents);
        STEPPING_NPC.with(|c| c.set(false));
        result
    }

    /// [`Self::advance_npc_skater`]'s step: its controls with a neutral pad
    /// (retail's AI presses no pad buttons for riding; its record steers), then the same frame
    /// advance as the player. The context is swapped back on error too.
    fn advance_npc_skater_inner(
        &mut self,
        context: &mut SkaterPhysicsContext,
        skater: &mut SkaterRuntime,
        controls: &mut PlayerControls,
        graphs: &crate::graph_runtime::StockGraphs,
        camera: &mut crate::camera::CameraRuntime,
        ai_intents: &[(String, f32)],
    ) -> Result<(), String> {
        self.swap_skater_context(context);
        controls.ai_driven = true;
        let mut actions = skate_core::input::tick::TickInput::new(0, skate_core::input::gameplay_map::GameplayActions::from_values([0.0; 18]), true).actions();
        let result = controls.update_for_physics(&mut actions, self, skater, camera).and_then(|()| {
            // The AI's ActionGraph signals (`skate_core::living_world::ai_signals`) go where the
            // player's gestures go (`publish_gestures`).
            for (name, value) in ai_intents {
                controls.action_intents.insert(name, *value);
                if value.abs() > 0.01 {
                    controls.named_intents.insert(name.clone(), *value);
                }
            }
            frame::advance(self, skater, controls, graphs, &mut actions, true, camera)
        });
        self.swap_skater_context(context);
        result
    }

    /// The board and skeleton bodies of a context's skater ([`network::capture_body`] for a simulated NPC skater).
    pub(crate) fn capture_context_body(context: &SkaterPhysicsContext, skater: &SkaterRuntime) -> skate_net::packed::BodyState {
        network::capture_body_of(&context.board, skater)
    }

    /// The deck of a context (the simulated NPC skater's board) without swapping it in.
    pub(crate) fn context_deck(context: &SkaterPhysicsContext) -> RetailAffineTransform {
        context.board.part_transforms()[skate_core::physics::board::BodyId::Deck.index()]
    }

    /// The deck rigid body's position (its centre of mass, retail `Reckoning+144`) in a context.
    pub(crate) fn context_deck_position(context: &SkaterPhysicsContext) -> [f32; 3] {
        let p = context.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.position;
        [p.x, p.y, p.z]
    }

    /// The deck's linear velocity in a context (diagnostics).
    pub(crate) fn context_deck_velocity(context: &SkaterPhysicsContext) -> [f32; 3] {
        let v = context.board.bodies()[skate_core::physics::board::BodyId::Deck.index()].rates.linear_velocity;
        [v.x, v.y, v.z]
    }

    /// A context's effective board forward (`riding.motion.effective_basis` column 2, as the animation phase reads
    /// it; diagnostics).
    pub(crate) fn context_effective_forward(context: &SkaterPhysicsContext) -> [f32; 3] {
        context.riding.motion.effective_basis.columns[2]
    }

    /// Every part of a context's board at `velocity` (`82C04168`, the AI spawn push).
    pub(crate) fn set_context_velocity(context: &mut SkaterPhysicsContext, velocity: [f32; 3]) {
        for body in context.board.bodies_mut() {
            body.rates.linear_velocity = Vector3::new(velocity[0], velocity[1], velocity[2]);
        }
    }

    /// A context whose tick failed part-way: forget the query batches it started, so the next owner of the pooled body
    /// starts its own (docs/hails-additions/26c-living-world-npc-skaters.md#failed-tick-leaves-queries).
    pub(crate) fn abandon_context_queries(context: &mut SkaterPhysicsContext) {
        context.riding.abandon_queries();
    }

    /// AI spawn push without a read acceleration (outcome-matched, b194): docs/hails-additions/27-dynamic-props.md#spawn-push-acceleration
    pub(crate) fn push_context_velocity(context: &mut SkaterPhysicsContext, velocity: [f32; 3]) {
        Self::set_context_velocity(context, velocity);
        let v = Vector3::new(velocity[0], velocity[1], velocity[2]);
        context.riding.ground.previous_velocities.fill(v);
    }

    pub(crate) fn swap_skater_context(&mut self, context: &mut SkaterPhysicsContext) {
        std::mem::swap(&mut self.clock, &mut context.clock);
        std::mem::swap(&mut self.board, &mut context.board);
        std::mem::swap(&mut self.riding, &mut context.riding);
        std::mem::swap(&mut self.prop_carry, &mut context.prop_carry);
        std::mem::swap(&mut self.ticks, &mut context.ticks);
        std::mem::swap(&mut self.contact_count, &mut context.contact_count);
        std::mem::swap(&mut self.failed, &mut context.failed);
        std::mem::swap(&mut self.exchange, &mut context.exchange);
        std::mem::swap(&mut self.processed_flags_2468, &mut context.processed_flags_2468);
        std::mem::swap(&mut self.board_wiping_out, &mut context.board_wiping_out);
        context.skater_settings.swap_with(&mut self.settings);
        std::mem::swap(&mut self.owns_props, &mut context.owns_props);
    }

    #[cfg(test)]
    fn advance_board(&mut self) -> Result<(), String> {
        self.board.clear_forces();
        self.riding.start_wheel_queries(&self.board, &self.world, self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::world))?;
        self.riding.finish_wheel_queries()?;
        let volumes = colliders::world_volumes(&self.board, &self.settings);
        self.step_props(&volumes);
        let contacts = self
            .world
            .query_primitives(&volumes, self.query, self.retention);
        self.contact_count = contacts.len();
        // The graph/ground-state consumer must supply recovered steering and
        // forces before this is playable. This tick verifies physical assembly.
        self.board.advance(contacts, [0.0; 2], self.settings.step);
        self.riding.finish_post_physics(
            &mut self.board,
            self.board_wiping_out,
            self.processed_flags_2468,
            self.settings.step.simulation.time_step,
        )?;
        self.ticks += 1;
        if self.board.bodies().iter().any(|body| {
            let p = body.rates.position;
            !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite()
        }) {
            self.failed = true;
            return Err(format!(
                "Board solver produced a non-finite pose on tick {}",
                self.ticks
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/map_startup.rs"]
mod map_startup;

pub(crate) struct PhysicsPlugin;
impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        let period = app.world().resource::<GamePhysics>().clock.period();
        let asset_root = app
            .world()
            .resource::<crate::config::Config>()
            .asset_root
            .clone();
        let controls = PlayerControls::load(&asset_root)
            .unwrap_or_else(|error| panic!("Cannot initialize trick gesture recognizers: {error}"));
        app.insert_resource(Time::<Fixed>::from_duration(period))
            .insert_resource(controls)
            .add_systems(
                FixedUpdate,
                controls::sample.in_set(SimulationSet::Controls),
            )
            .init_resource::<prop_dynamics::PropTuningSettings>()
            .add_systems(
                FixedUpdate,
                prop_dynamics::apply_prop_tuning.before(SimulationSet::Physics),
            )
            .init_resource::<prop_carry::CarrySettings>()
            .add_systems(
                FixedUpdate,
                prop_carry::apply_carry_settings.before(SimulationSet::Physics),
            )
            .init_resource::<respawn::RespawnSettings>()
            .add_message::<respawn::PlayerRespawn>()
            .add_systems(
                FixedUpdate,
                respawn::apply_respawn_settings.before(SimulationSet::Physics),
            )
            .add_systems(FixedUpdate, advance.in_set(SimulationSet::Physics))
            .add_systems(FixedUpdate, respawn::emit_respawns.after(SimulationSet::Physics))
            .add_systems(FixedUpdate, manual_landing_log::log_manual_landings.after(SimulationSet::Physics))
            .add_systems(Update, present.in_set(FrameSet::Physics))
            .add_systems(Update, prop_dynamics::sync_prop_transforms.after(FrameSet::Physics))
            .add_systems(Update, prop_dynamics::apply_better_object_mover)
            .add_systems(Update, prop_water::apply_prop_water.before(FrameSet::Physics))
            .add_systems(Update, prop_water::apply_prop_fade.after(FrameSet::Physics));
    }
}

/// Collision+16 low 16 bits: the board's surface vote (82C08818, 12 when a board
/// contact is water), as respawn reads it. Read-only, for game_audio (the audio
/// record's `+813`, the board in water).
pub(crate) fn board_surface(physics: &GamePhysics) -> u32 {
    ground_runtime::active_surface(&physics.riding, &physics.board) & 0xffff
}

pub(crate) fn advance(
    mut physics: ResMut<GamePhysics>,
    mut skater: ResMut<SkaterRuntime>,
    mut controls: ResMut<PlayerControls>,
    graphs: Res<crate::graph_runtime::StockGraphs>,
    input: Res<crate::input::PublishedTickInput>,
    mut camera: ResMut<crate::camera::CameraRuntime>,
    mut cadence: ResMut<Time<Fixed>>,
    mut exit: MessageWriter<AppExit>,
    mut performance: Option<ResMut<crate::performance::Performance>>,
    mods: Option<Res<crate::modding::Mods>>,
) {
    if physics.failed {
        return;
    }
    if mods
        .as_ref()
        .is_some_and(|m| crate::modding::player_attached(m) || crate::modding::player_suspended(m))
    {
        return;
    }
    let timer = performance.as_ref().map(|_| std::time::Instant::now());
    let mut actions = input.0.actions();
    let input_available = input.0.controller_available();
    #[cfg(debug_assertions)]
    let _physics_trace = dev_trace::begin(&physics, &skater, *input.0.actions().values());
    if let Err(message) = frame::advance(
        &mut physics,
        &mut skater,
        &mut controls,
        &graphs,
        &mut actions,
        input_available,
        &mut camera,
    ) {
        #[cfg(debug_assertions)]
        dev_trace::dump(&format!("tick={} physics_error={message}", physics.ticks));
        physics.failed = true;
        error!(
            "{message}; state={:?}; tick={}; mapped_input={:?}; force_mode={}; board_axis_y={}; flags={:08x}/{:08x}/{:08x}/{:08x}/{:08x}",
            skater.player_state.current(),
            physics.ticks,
            input.0.actions(),
            skater.skeleton_input.force_mode,
            skater.skeleton_input.drive_frames[0][2][1],
            skater.player_input.processed.flags_2468,
            skater.player_input.processed.flags_2472,
            skater.player_input.processed.flags_2476,
            skater.player_input.processed.flags_2480,
            skater.player_input.processed.flags_2484,
        );
        exit.write(AppExit::error());
    }
    cadence.set_timestep(physics.clock.period());
    if let (Some(performance), Some(timer)) = (performance.as_mut(), timer) {
        performance.physics(timer.elapsed());
    }
}

impl GamePhysics {
    /// A static rail's [spline id, type signature] from a grind owner (Grind.IsGrindingGUID compares them).
    pub(crate) fn rail_guids(&self, owner: u64) -> Option<[u64; 2]> {
        self.grind_world.spline_guids(owner)
    }

    fn finish_skater(&mut self, skater: &mut SkaterRuntime) -> Result<(), String> {
        //Skateboard::UpdatePostPhysics82C02158 prepares the wall probe for
        //the next StartBoard, using this input phase's cached deck toolkit.
        self.riding.probes.prepare_wall(
            &skater.player_input.processed,
            skater
                .player_input
                .toolkit
                .as_ref()
                .ok_or("Postphysics wall probe requires the current board toolkit")?,
        );
        self.riding.finish_post_physics(
            &mut self.board,
            self.board_wiping_out,
            self.processed_flags_2468,
            self.settings.step.simulation.time_step,
        )?;
        let partial = skateboard_controller::partial_request(
            &skater.skateboard_controller,
            &self.riding.ground,
        );
        skeleton_feedback::publish(self, skater, partial);
        if skater.player_state.current()
            == skate_core::player::state::PhysicalStateId::WipeoutGround
        {
            wipeout_states::post_physics(skater);
        }
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::PhysicsAir {
            air_phase::update_apex(self, &mut skater.air_state);
        }
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::KnownAir {
            known_air::post_physics(self, skater)?;
        }
        if skater.player_state.current().is_grind()
            || skater.player_state.current()
                == skate_core::player::state::PhysicalStateId::Nonspecific
        {
            grind::post(self, skater)?;
        }
        wipeout::check_after_physics(self, skater)?;
        // Ground / Slide UpdatePostPhysics 82D387A8: the AI board path follows the wipeout check.
        board_path::post_physics(self, skater);
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::PhysicsAirSecondary {
            grind_trick::post_velocity(self, skater);
        }
        offboard::post_physics::advance(self, skater)?;
        let compression = skater.skeleton_output.average_compressions(&self.board);
        render_pose::publish(self, skater, compression)?;
        self.ticks += 1;
        let invalid_board = self.board.bodies().iter().enumerate().find(|(_, body)| {
            let p = body.rates.position;
            let v = body.rates.linear_velocity;
            let w = body.rates.angular_velocity;
            [p.x, p.y, p.z, v.x, v.y, v.z, w.x, w.y, w.z]
                .into_iter()
                .any(|value| !value.is_finite())
        });
        let invalid_skeleton = skater
            .skeleton
            .bodies()
            .iter()
            .enumerate()
            .find(|(_, body)| {
                let p = body.rates.position;
                let v = body.rates.linear_velocity;
                let w = body.rates.angular_velocity;
                [p.x, p.y, p.z, v.x, v.y, v.z, w.x, w.y, w.z]
                    .into_iter()
                    .any(|value| !value.is_finite())
            });
        if invalid_board.is_some() || invalid_skeleton.is_some() {
            self.failed = true;
            return Err(format!(
                "Shared skater solver produced non-finite rates on tick{}; board={invalid_board:?}; skeleton={invalid_skeleton:?}",
                self.ticks
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/physics_startup.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/air_playback.rs"]
mod air_tests;

#[cfg(test)]
#[path = "tests/wipeout_playback.rs"]
mod wipeout_tests;

#[cfg(test)]
#[path = "tests/water_drop.rs"]
mod water_drop_tests;

#[cfg(test)]
#[path = "tests/grind_bail.rs"]
mod grind_bail_tests;

#[cfg(test)]
#[path = "tests/audio_state_capture.rs"]
mod audio_state_capture_tests;

#[cfg(test)]
#[path = "tests/flip_hitch_timing.rs"]
mod flip_hitch_timing_tests;

#[cfg(test)]
#[path = "tests/trigger_points.rs"]
mod trigger_points_tests;

fn present(
    physics: Res<GamePhysics>,
    history: Res<crate::presentation::Presentation>,
    replay: Res<crate::replay::Replay>,
    time: Res<Time<Fixed>>,
    mut roots: Query<&mut Transform, With<PlayerRoot>>,
) {
    if physics.failed {
        return;
    }
    let Some((previous, current, alpha)) = history.view(&replay, time.overstep_fraction()) else { return; };
    for mut root in &mut roots {
        *root = crate::presentation::blend(previous.root, current.root,
            alpha);
    }
}

#[cfg(test)]
#[path = "tests/powerslide_playback.rs"]
mod powerslide_tests;

#[cfg(test)]
#[path = "tests/manual_playback.rs"]
mod manual_tests;

#[cfg(test)]
#[path = "tests/hippy_playback.rs"]
mod hippy_tests;

#[cfg(test)]
#[path = "tests/recorded_playback.rs"]
mod recorded_tests;

#[cfg(test)]
#[path = "tests/offboard_air_playback.rs"]
mod offboard_air_tests;

#[cfg(test)]
#[path = "tests/offboard_midair_playback.rs"]
mod offboard_midair_playback;

#[cfg(test)]
#[path = "tests/offboard_recall_playback.rs"]
mod offboard_recall_playback_tests;

#[cfg(test)]
#[path = "tests/offboard_jump_playback.rs"]
mod offboard_jump_playback_tests;

#[cfg(test)]
#[path = "tests/offboard_root_trace.rs"]
mod offboard_root_trace;

#[cfg(test)]
#[path = "tests/gameplay_gestures.rs"]
mod gameplay_gesture_tests;

#[cfg(test)]
#[path = "tests/difficulty.rs"]
mod difficulty_tests;

// Which body a shared log line belongs to: NPC skaters step through the same physics as the player, so their
// `player teleport spawn` / `physical state transition` lines read like the player's (laptop notes 2026-10-21 23:07).
// Set per thread by GamePhysics::advance_npc_skater (NPC steps run on worker threads).
thread_local! {
    static STEPPING_NPC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NPC_LOG_SERIAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The NPC serial shared physics log lines name on this thread.
pub(crate) fn set_log_npc(serial: u32) {
    NPC_LOG_SERIAL.with(|c| c.set(serial));
}

pub(crate) fn log_npc() -> u32 {
    NPC_LOG_SERIAL.with(std::cell::Cell::get)
}

/// `npc` while an NPC skater steps on this thread, else `player`: the `actor` field of shared physics log lines.
pub(crate) fn log_actor() -> &'static str {
    if STEPPING_NPC.with(|c| c.get()) { "npc" } else { "player" }
}
