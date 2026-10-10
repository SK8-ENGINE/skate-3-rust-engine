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
pub(crate) use skater::AiPhysicsSource;
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
mod input_phase;
mod landing_quality;
mod offboard;
mod player_input;
mod player_state;
pub(crate) mod prop_carry;
pub(crate) mod prop_carry_hud;
pub(crate) mod prop_dynamics;
pub(crate) mod prop_layout;
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
    pub(crate) network_proxies: network::Proxies,
    /// Other skaters' push volumes for the dynamic prop step: (stable actor id, volume), sorted by
    /// id. Rewritten each tick before the solve by `living_world::npc_skaters::push_proxies`
    /// (NPC skater body + board, doc 26 fix 19); empty when nothing fills it.
    pub(crate) actor_prop_volumes: Vec<(u64, BoardWorldVolume)>,
    pub(crate) network_active: bool,
    pub(crate) network_contacts: usize,
    clock: clock::SimulationClock,
    pub board: BoardRuntime,
    pub riding: RidingOutputs,
    world: BoardWorld,
    /// DMO prop instances as a separate collision layer so dynamic bodies can
    /// re-bake their triangle ranges (Phase 2).
    prop_layer: Option<crate::skate_world::PropCollisionLayer>,
    prop_dynamics: Option<prop_dynamics::PropDynamics>,
    /// Offboard prop carry state (Phase 3); one held prop at a time.
    pub(crate) prop_carry: prop_carry::PropCarry,
    grind_world: std::sync::Arc<crate::grind_world::StaticProvider>,
    grind_materials: grind_materials::GrindMaterials,
    offboard_grab_scene: offboard::grab_scene::Registry,
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

/// The parts of [`GamePhysics`] each simulated skater owns (doc 26, "Simulated NPC skaters"):
/// its board, riding outputs, clock and exchange. A simulated NPC skater keeps one and swaps it in
/// around its own tick ([`GamePhysics::swap_skater_context`]); the world, props, grind world,
/// settings and network proxies stay shared.
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
    /// This tick's traffic cars in the grab scene (`living_world::vehicles::push_vehicle_grab_splines`).
    pub(crate) fn set_grab_cars(&mut self, cars: Vec<skate_core::player::offboard::grab_scene::Object>) -> Result<(), String> {
        self.offboard_grab_scene.set_cars(cars)
    }
    /// This tick's props in the grab scene (`PropDynamics::grab_objects`). Opt-in with `SKATE_PROP_GRAB=1` until
    /// Move Object reads the authored record (doc 26i "Move Object step 2"); off, the scene holds no props.
    pub(crate) fn refresh_grab_props(&mut self) -> Result<(), String> {
        if !prop_dynamics::prop_grab_enabled() {
            return Ok(());
        }
        let props = self.prop_dynamics.as_ref().map(|d| d.grab_objects()).unwrap_or_default();
        self.offboard_grab_scene.set_props(props)
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
    pub(crate) fn world(&self) -> &BoardWorld {
        &self.world
    }

    pub(crate) fn prop_world(&self) -> Option<&BoardWorld> {
        self.prop_layer.as_ref().map(crate::skate_world::PropCollisionLayer::world)
    }

    pub(crate) fn prop_world_mut(&mut self) -> Option<&mut BoardWorld> {
        self.prop_layer.as_mut().map(crate::skate_world::PropCollisionLayer::world_mut)
    }

    pub(crate) fn prop_dynamics(&self) -> Option<&prop_dynamics::PropDynamics> {
        self.prop_dynamics.as_ref()
    }

    pub(crate) fn prop_dynamics_mut(&mut self) -> Option<&mut prop_dynamics::PropDynamics> {
        self.prop_dynamics.as_mut()
    }

    /// Push, integrate and re-bake dynamic props against the static world.
    /// `volumes` are the skater's board and skeleton world volumes; the NPC skaters' volumes
    /// ([`Self::actor_prop_volumes`]) push props by the same rule.
    pub(crate) fn step_props(&mut self, volumes: &[BoardWorldVolume]) {
        if !self.owns_props {
            return;
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut())
        else {
            return;
        };
        dynamics.step_with_actors(&self.world, layer, volumes, &self.actor_prop_volumes);
    }

    /// Reset ONE object (retail cMsgResetDMO, doc 27 "Object Dropper and reset"): the phone's
    /// per-object Reset posts it, PlayerUI's handler 8289A048 calls the DMO manager's reset
    /// (vtable 0x82323254 slot +36, 82C4B5F0), whose worker 82C4B780 looks up the object's spawn
    /// record and puts the object back on the record's transform in one step (no fade or
    /// tween in that path). Ours: the authored pose, at rest and asleep, collision rebaked, the
    /// saved layout entry dropped. Refused (false) for an unknown id and for the held object
    /// (NOT RETAIL YET: retail's held case is undecoded). The single authority for resets.
    pub(crate) fn reset_prop(&mut self, id: u32) -> bool {
        if self.prop_carry.held() == Some(id) {
            return false;
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut())
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
        if self.prop_carry.held() == Some(id) {
            return false;
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut()) else {
            return false;
        };
        let Some((instance, origin, basis)) = dynamics.set_dormant(id, dormant, authored) else { return false };
        if let Err(error) = layer.rebake(instance, basis.columns, origin) {
            warn!("SKATE_PROP_STREAM: rebake {id}: {error}");
        }
        true
    }

    /// Create a prop body mid-game (a released hand prop, a mod's prop): collision triangles in
    /// the prop layer plus an awake body at the spec's pose and velocities, stepped, pushed and
    /// rebaked like a map prop. Works on maps without placed props (the layer starts empty).
    /// The single authority for runtime props; `None` when this context does not own the props
    /// or the spec gives no body. Returns the body id; a render entity with
    /// `PropInstance { id }` follows it (`sync_prop_transforms`).
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
                Some(prop_dynamics::PropDynamics::empty(prop_dynamics::prop_simulation(self.settings.step.simulation)));
        }
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut()) else {
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
        let (Some(layer), Some(dynamics)) = (self.prop_layer.as_mut(), self.prop_dynamics.as_mut()) else {
            return false;
        };
        let Some(instance) = dynamics.remove_body(id) else { return false };
        if let Err(error) = layer.retire_instance(instance, prop_dynamics::HELD_PARK) {
            warn!("SKATE_PROP_REMOVE: retire {id}: {error}");
        }
        info!("SKATE_PROP_REMOVE id={id}");
        true
    }

    /// Upright ONE object (retail cMsgUprightDMO, doc 27 "Upright"): the phone's per-object
    /// Upright posts it and the DMO manager slot +40 (82C4B8C0) opens the DMO's 2 s
    /// self-righting window; the prop step then turns the body back toward world up through the
    /// retail solver path ([`PropUprightSettings`](crate::physics::prop_dynamics::PropUprightSettings)).
    /// Refused (false) for an unknown id or a body without dynamics. The single authority for
    /// uprights.
    pub(crate) fn upright_prop(&mut self, id: u32) -> bool {
        let Some(dynamics) = self.prop_dynamics.as_mut() else { return false };
        let started = dynamics.upright(id);
        if started {
            info!("SKATE_PROP_UPRIGHT id={id}");
        }
        started
    }

    /// Mod convenience: [`Self::reset_prop`] for every prop away from its authored pose or with a
    /// saved placement, in id order. NOT RETAIL YET: no retail "reset all moved objects" code was
    /// found (the phone getter GetPhoneListCanResetAllObjectsOption exists, its handler is not
    /// located). Returns the reset ids.
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
        let previous = self.prop_carry.held();
        if let Some(dynamics) = self.prop_dynamics.as_mut() {
            self.prop_carry.update(dynamics, tick, carrier);
        }
        let current = self.prop_carry.held();
        if previous == current {
            return;
        }
        if let Some(dynamics) = self.prop_dynamics.as_mut() {
            dynamics.set_held(current);
        }
        // Park the newly held prop's triangles far below the world so skater
        // queries cannot be pushed by it; restore a dropped prop's triangles
        // at its final pose.
        let (Some(layer), Some(dynamics)) =
            (self.prop_layer.as_mut(), self.prop_dynamics.as_mut())
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
        let world = match map {
            Some(map) => crate::skate_world::collision_world(map, settings.floor_material)?,
            None => terrain.world(settings.floor_material),
        };
        let prop_layer = map.and_then(|map| {
            crate::skate_world::load_prop_layer(
                asset_root,
                &map.name,
                settings.floor_material,
                prop_dynamics::prop_simulation(settings.step.simulation),
            )
        });
        let (mut prop_layer, mut prop_dynamics) = match prop_layer {
            Some((layer, dynamics)) => (Some(layer), Some(dynamics)),
            None => (None, None),
        };
        // Phase 4: apply the saved layout sidecar over the authored poses.
        let mut prop_carry = prop_carry::PropCarry::default();
        if let (Some(map), Some(layer), Some(dynamics)) = (map, prop_layer.as_mut(), prop_dynamics.as_mut()) {
            let path = prop_layout::path(asset_root, &map.name);
            if let Some(layout) = prop_layout::load(&path, &map.name) {
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
        } else { crate::grind_world::StaticProvider::new(map)? });
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
        Ok(Self {
            network_proxies: network::Proxies::default(),
            actor_prop_volumes: Vec::new(),
            network_active: false,
            network_contacts: 0,
            clock: clock::SimulationClock::default(),
            board,
            riding,
            world,
            prop_layer,
            prop_dynamics,
            prop_carry,
            grind_world,
            grind_materials,
            offboard_grab_scene,
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
        })
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
    ) -> Result<SkaterRuntime, String> {
        self.swap_skater_context(context);
        let result = SkaterRuntime::load(asset_root, graphs, self, difficulty);
        self.swap_skater_context(context);
        result
    }

    /// One tick of a simulated NPC skater in its own context: its controls with a neutral pad
    /// (retail's AI presses no pad buttons for riding; its record steers), then the same frame
    /// advance as the player. The context is swapped back on error too.
    pub(crate) fn advance_npc_skater(
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

    /// The deck of a context (the simulated NPC skater's board) without swapping it in.
    pub(crate) fn context_deck(context: &SkaterPhysicsContext) -> RetailAffineTransform {
        context.board.part_transforms()[skate_core::physics::board::BodyId::Deck.index()]
    }

    /// Every part of a context's board at `velocity` (`82C04168`, the AI spawn push).
    pub(crate) fn set_context_velocity(context: &mut SkaterPhysicsContext, velocity: [f32; 3]) {
        for body in context.board.bodies_mut() {
            body.rates.linear_velocity = Vector3::new(velocity[0], velocity[1], velocity[2]);
        }
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
            .add_systems(Update, prop_dynamics::sync_prop_transforms.after(FrameSet::Physics));
        prop_carry_hud::install(app);
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
