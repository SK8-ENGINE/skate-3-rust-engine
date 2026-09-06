//! Host game physics ownership and schedule. Physical calculations stay in core.
mod animated_skeleton;
mod air_reckoning;
mod air_phase;
mod air_trajectory;
pub(crate) mod camera_output;
mod clock;
mod colliders;
mod controls;
mod foot_ik;
mod footplant;
mod foot_ik_queries;
mod foot_physical_output;
pub(crate) mod ground;
mod render_pose;
mod riding_outputs;
mod skateboard_controller;
mod skater;
mod skeleton_body;
mod skeleton_air;
mod skeleton_controller;
mod skeleton_colliders;
mod skeleton_feedback;
mod skeleton_input_runtime;
mod skeleton_output;
mod solve;
pub(crate) use skater::SkaterRuntime;
mod animation_feedback;
mod animation_feedback_settings;
mod animation_input;
mod animation_phase;
mod landing_quality;
mod frame;
mod ground_phase;
mod ground_animation;
mod slide_state;
mod ground_exit;
mod ground_runtime;
mod input_phase;
mod player_input;
mod player_state;
mod settings;
mod wipeout;
mod wipeout_states;
mod teleport_state;
mod offboard;
mod known_air;
use crate::{
    app::{FrameSet, SimulationSet},
    world::PlayerRoot,
};
use bevy::prelude::*;
pub(crate) use controls::PlayerControls;
use riding_outputs::RidingOutputs;
use settings::PhysicsSettings;
use skate_core::{
    math::Vector3,
    physics::{
        board::BodyId,
        board_runtime::{BoardMotion, BoardRuntime},
        board_world::{BoardWorld, ContactRetentionSettings},
        collision::WorldContactSettings,
        drive_frames::RetailAffineTransform,
    },
};
use skate_data::collections::Collections;

#[derive(Resource)]
pub(crate) struct GamePhysics {
    clock: clock::SimulationClock,
    pub board: BoardRuntime,
    pub riding: RidingOutputs,
    world: BoardWorld,
    settings: PhysicsSettings,
    animation_profile: animation_phase::AnimationProfile,
    query: WorldContactSettings,
    retention: ContactRetentionSettings,
    pub ticks: u64,
    pub contact_count: usize,
    pub failed: bool,
    /// ProcessedPhysIn reset82BF9EF0 sets0x2000; initial stancebit20 is clear.
    /// Animation packet publication owns subsequent stance-bit updates.
    pub processed_flags_2468: u32,
    /// Toolkit ctor82C0680C clears8384bit7; wipeout entry/exit owns changes.
    pub board_wiping_out: bool,
}

impl GamePhysics {
    pub(crate) fn set_difficulty(&mut self, difficulty: crate::difficulty::Difficulty) {
        // Actor publication carries this selector into the next physical packet.
        // Keep the board, active trick, equipment preferences and controller history.
        self.animation_profile.physics_mode = difficulty as u32;
    }

    pub(crate) fn difficulty_index(&self) -> u32 { self.animation_profile.physics_mode }

    pub(crate) fn world(&self) -> &BoardWorld {
        &self.world
    }

    pub(crate) fn world_triangles(&self) -> &[skate_core::physics::board_world::WorldTriangle] {
        self.world.triangles()
    }

    pub fn load(asset_root: &std::path::Path) -> Result<Self, String> {
        Self::load_with_map(asset_root, None)
    }

    pub fn load_with_map(asset_root: &std::path::Path, map: Option<&skate_data::skate_map::SkateMap>) -> Result<Self, String> {
        Self::load_with_difficulty(asset_root, map, crate::difficulty::Difficulty::Easy)
    }

    pub fn load_with_difficulty(asset_root: &std::path::Path, map: Option<&skate_data::skate_map::SkateMap>, difficulty: crate::difficulty::Difficulty) -> Result<Self, String> {
        let data = Collections::load(asset_root)?;
        let settings = PhysicsSettings::load(&data)?;
        let animation_profile = animation_phase::AnimationProfile::load(&data, difficulty.key())?;
        let mut spawn = RetailAffineTransform {
            translation: Vector3::new(
                0.0,
                ground::HEIGHT + settings.wheel_radius - settings.authored[0].translation.y,
                0.0,
            ),
            ..RetailAffineTransform::IDENTITY
        };
        if let Some(map) = map {
            // Native collision placement aligns the package spawn with the
            // wheel-ground point (skate3_native_collision.cpp), not skeleton COM.
            spawn.translation = Vector3::new(map.spawn[0], map.spawn[1] + settings.wheel_radius - settings.authored[0].translation.y, map.spawn[2]);
            spawn.basis = skate_core::math::Basis3 { columns: Mat3::from_rotation_y(map.heading).to_cols_array_2d() };
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
            None => ground::world(settings.floor_material),
        };
        let processed_flags_2468 = 0x2000;
        let riding = RidingOutputs::load(&data, &board, processed_flags_2468)?;
        let (query, retention) = ground::query_settings();
        Ok(Self {
            clock: clock::SimulationClock::default(),
            board,
            riding,
            world,
            settings,
            animation_profile,
            query,
            retention,
            ticks: 0,
            contact_count: 0,
            failed: false,
            processed_flags_2468,
            board_wiping_out: false,
        })
    }

    #[cfg(test)]
    fn advance_board(&mut self) -> Result<(), String> {
        self.board.clear_forces();
        self.riding.start_wheel_queries(&self.board, &self.world)?;
        self.riding.finish_wheel_queries()?;
        let volumes = colliders::world_volumes(&self.board, &self.settings);
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

pub(crate) struct PhysicsPlugin;
impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        let period = app.world().resource::<GamePhysics>().clock.period();
        app.insert_resource(Time::<Fixed>::from_duration(period))
            .init_resource::<PlayerControls>()
            .add_systems(
                FixedUpdate,
                controls::sample.in_set(SimulationSet::Controls),
            )
            .add_systems(FixedUpdate, advance.in_set(SimulationSet::Physics))
            .add_systems(Update, present.in_set(FrameSet::Physics));
    }
}

fn advance(
    mut physics: ResMut<GamePhysics>,
    mut skater: ResMut<SkaterRuntime>,
    mut controls: ResMut<PlayerControls>,
    graphs: Res<crate::graph_runtime::StockGraphs>,
    input: Res<crate::input::ControllerInput>,
    mut camera: ResMut<crate::camera::CameraRuntime>,
    mut cadence: ResMut<Time<Fixed>>,
    mut exit: MessageWriter<AppExit>,
    mut performance: Option<ResMut<crate::performance::Performance>>,
) {
    if physics.failed {
        return;
    }
    let timer = performance.as_ref().map(|_| std::time::Instant::now());
    let mut actions = input.player_actions();
    let input_available = input
        .status
        .iter()
        .any(|s| *s == crate::input::ControllerStatus::Ready);
    if let Err(message) = frame::advance(
        &mut physics,
        &mut skater,
        &mut controls,
        &graphs,
        &mut actions,
        input_available,
        &mut camera,
    ) {
        physics.failed = true;
        error!(
            "{message}; tick={}; mapped_input={:?}; force_mode={}; board_axis_y={}; flags={:08x}/{:08x}/{:08x}/{:08x}/{:08x}",
            physics.ticks,
            input.mapped_actions,
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
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::WipeoutGround {
            wipeout_states::post_physics(skater);
        }
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::PhysicsAir {
            air_phase::update_apex(self, &mut skater.air_state);
        }
        if skater.player_state.current() == skate_core::player::state::PhysicalStateId::KnownAir {
            known_air::post_physics(self, skater)?;
        }
        wipeout::check_after_physics(self, skater)?;
        let compression = skater.skeleton_output.average_compressions(&self.board);
        render_pose::publish(self, skater, compression)?;
        self.ticks += 1;
        let invalid = self
            .board
            .bodies()
            .iter()
            .chain(skater.skeleton.bodies())
            .any(|body| {
                let position = body.rates.position;
                !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite()
            });
        if invalid {
            self.failed = true;
            return Err(format!(
                "Shared skater solver produced a non-finite pose on tick{}",
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
#[path = "tests/difficulty.rs"]
mod difficulty_tests;

fn present(
    physics: Res<GamePhysics>,
    skater: Res<SkaterRuntime>,
    mut roots: Query<&mut Transform, With<PlayerRoot>>,
) {
    if physics.failed {
        return;
    }
    for mut root in &mut roots {
        *root = Transform::from_matrix(crate::animation::native_matrix(
            skater.animated_skeleton.roots.animation_to_world,
        ));
    }
}
