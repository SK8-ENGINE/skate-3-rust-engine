//! Normal gameplay camera rendering endpoint. Simulation owns the cadence of
//! CameraRuntime::advance after its complete physical/animation publication.
mod settings;
mod collision;
mod world_query;
mod shot_data;
mod stock_names;
mod shake_data;
mod trajectory;
mod subject;
mod graph_subject;
mod graph_conditions;
mod graph;
mod runtime;
mod publication;
pub(crate) use publication::{
    snapshot as publish_camera_subject, CameraPublicationInputs, CameraStateOutput,
    CameraAnimationOutput, CameraAirOutput, CameraOffboardOutput, CameraGrindOutput,
    CameraEventsOutput, CameraPreferences,
};
pub(crate) use graph_subject::CameraGraphEnvironment;
pub(crate) use runtime::CameraRuntime;
use bevy::prelude::*;
use crate::{app::FrameSet, config::Config};

#[derive(Component)]
struct GameplayCamera;

pub(crate) struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn)
            .add_systems(Update, present.after(FrameSet::Animation).before(FrameSet::Verification));
    }
}
fn spawn(mut commands: Commands, config: Res<Config>, retail: Res<crate::retail_render::RetailScene>) {
    let runtime = CameraRuntime::load(&config.asset_root)
        .unwrap_or_else(|error| panic!("Cannot initialize normal gameplay camera: {error}"));
    commands.insert_resource(runtime);
    let mut camera = commands.spawn((
        GameplayCamera,
        Camera3d::default(),
        Camera { is_active: false, ..default() },
        Transform::default(),
    ));
    if retail.0 {
        camera.insert((bevy::render::view::Hdr, bevy::core_pipeline::tonemapping::Tonemapping::None,
            crate::retail_render::RetailTone::default()));
    }
}

fn present(mut runtime: ResMut<CameraRuntime>, windows: Query<&Window>,
    history: Res<crate::presentation::Presentation>, time: Res<Time<Fixed>>,
    replay: Res<crate::replay::Replay>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<GameplayCamera>>) {
    if let Ok(window) = windows.single() {
        runtime.set_aspect_ratio(window.width() / window.height());
    }
    let Some((previous, current, alpha)) = history.view(&replay, time.overstep_fraction()) else { return; };
    for (mut camera, mut transform, mut projection) in &mut cameras {
        *transform = crate::presentation::blend(previous.camera, current.camera, alpha);
        if replay.active {
            if let Some(free) = replay.free_camera { *transform = free; }
        }
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = previous.fov + (current.fov - previous.fov) * alpha;
        }
        camera.is_active = true;
    }
}
