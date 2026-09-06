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
pub(crate) use subject::{CameraSubjectFrame, CameraSubjectSnapshot};
pub(crate) use graph_subject::{CameraGraphEnvironment, CameraGraphSubject};
pub(crate) use runtime::CameraRuntime;
use bevy::prelude::*;
use crate::{app::FrameSet, config::Config};

#[derive(Component)]
struct GameplayCamera;

pub(crate) struct CameraPlugin;
impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSubjectFrame>()
            .add_systems(Startup, spawn)
            .add_systems(Update, present.after(FrameSet::Animation).before(FrameSet::Verification));
    }
}
fn spawn(mut commands: Commands, config: Res<Config>) {
    let runtime = CameraRuntime::load(&config.asset_root)
        .unwrap_or_else(|error| panic!("Cannot initialize normal gameplay camera: {error}"));
    commands.insert_resource(runtime);
    commands.spawn((
        GameplayCamera,
        Camera3d::default(),
        Camera { is_active: false, ..default() },
        Transform::default(),
    ));
}

fn present(mut runtime: ResMut<CameraRuntime>, windows: Query<&Window>,
    history: Res<crate::presentation::Presentation>, time: Res<Time<Fixed>>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<GameplayCamera>>) {
    if let Ok(window) = windows.single() {
        runtime.set_aspect_ratio(window.width() / window.height());
    }
    let Some((previous, current)) = history.pair() else { return; };
    let alpha = time.overstep_fraction().clamp(0.0, 1.0);
    for (mut camera, mut transform, mut projection) in &mut cameras {
        *transform = crate::presentation::blend(previous.camera, current.camera, alpha);
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = previous.fov + (current.fov - previous.fov) * alpha;
        }
        camera.is_active = true;
    }
}
