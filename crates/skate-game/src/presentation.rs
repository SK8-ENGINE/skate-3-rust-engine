//! Render-only snapshots of completed fixed ticks. Camera, root and skin use
//! the same interval; interpolated values never feed back into simulation.
use bevy::prelude::*;
use crate::{app::SimulationSet, camera::CameraRuntime, physics::SkaterRuntime};
use skate_core::camera::CameraFrame;

pub(crate) struct PresentationPlugin;
impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Presentation>()
            .add_systems(FixedUpdate, capture.after(SimulationSet::Physics));
    }
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub root: Transform,
    pub bones: Vec<Mat4>,
    pub camera: Transform,
    pub fov: f32,
}

#[derive(Resource, Default)]
pub(crate) struct Presentation {
    pair: Option<(Snapshot, Snapshot)>,
    generation: u64,
    period: std::time::Duration,
}
impl Presentation {
    pub fn pair(&self) -> Option<(&Snapshot, &Snapshot)> {
        self.pair.as_ref().map(|(a, b)| (a, b))
    }
}

fn capture(skater: Res<SkaterRuntime>, camera: Res<CameraRuntime>,
    time: Res<Time<Fixed>>, mut history: ResMut<Presentation>) {
    if skater.pose_generation == history.generation { return; }
    let Some(frame) = camera.frame else { return; };
    let next = Snapshot {
        root: Transform::from_matrix(crate::animation::native_matrix(
            skater.animated_skeleton.roots.animation_to_world)),
        bones: skater.render_pose.iter().copied()
            .map(crate::animation::native_matrix).collect(),
        camera: camera_transform(frame),
        fov: frame.field_of_view_degrees.to_radians(),
    };
    // Snap teleports/cuts and cadence changes instead of sweeping through the
    // world or blending snapshots across different simulation-clock periods.
    let reset = frame.discontinuity
        || skater.player_input.processed.flags_2472 & (1 << 10) != 0
        || history.period != time.timestep();
    if let Some((previous, current)) = history.pair.as_mut() {
        if reset || current.bones.len() != next.bones.len() {
            *previous = next.clone();
        } else {
            std::mem::swap(previous, current);
        }
        *current = next;
    } else {
        history.pair = Some((next.clone(), next));
    }
    history.generation = skater.pose_generation;
    history.period = time.timestep();
}

fn camera_transform(frame: CameraFrame) -> Transform {
    let [right, up, at] = frame.basis.columns.map(Vec3::from_array);
    Transform {
        translation: Vec3::new(frame.position[0], frame.position[1], frame.position[2]),
        // Native At is forward; Bevy cameras look along local -Z.
        rotation: Quat::from_mat3(&Mat3::from_cols(-right, up, -at)).normalize(),
        ..default()
    }
}

pub(crate) fn blend(a: Transform, b: Transform, alpha: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, alpha),
        rotation: a.rotation.normalize().slerp(b.rotation.normalize(), alpha),
        scale: a.scale.lerp(b.scale, alpha),
    }
}
