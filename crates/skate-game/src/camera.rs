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
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<GameplayCamera>>) {
    if let Ok(window) = windows.single() {
        runtime.set_aspect_ratio(window.width() / window.height());
    }
    let Some(frame) = runtime.frame else { return; };
    for (mut camera, mut transform, mut projection) in &mut cameras {
        let [right, up, at] = frame.basis.columns.map(Vec3::from_array);
        // World rendering retains nativeXYZ. Native camera At points forward;
        // Bevy camera looks along local-Z, hence the two-axis basis conversion.
        transform.rotation = Quat::from_mat3(&Mat3::from_cols(-right, up, -at));
        transform.translation = Vec3::new(frame.position[0], frame.position[1], frame.position[2]);
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = frame.field_of_view_degrees.to_radians();
        }
        camera.is_active = true;
    }
}
