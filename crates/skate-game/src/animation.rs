//! Render the same stock skeleton that animation and physics update.
//! The GLB supplies the mesh, skin weights and hierarchy; it does not play a
//! separate idle clip or own the skater's pose.
use crate::{app::FrameSet, assets::AssetStatus, physics::SkaterRuntime, world::PlayerRoot};
use bevy::{mesh::skinning::SkinnedMesh, prelude::*};
use skate_core::animation::output::NativeMatrix;

#[derive(Resource, Default)]
pub(crate) struct AnimationStatus {
    pub ready: bool,
    bindings: Vec<BoneBinding>,
}
struct BoneBinding {
    entity: Entity,
    bone: usize,
    parent_bone: Option<usize>,
}
pub(crate) struct AnimationPlugin;
impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationStatus>()
            .add_systems(Update, (bind, present).chain().in_set(FrameSet::Animation));
    }
}
fn bind(
    status: Res<AssetStatus>,
    skater: Res<SkaterRuntime>,
    mut animation: ResMut<AnimationStatus>,
    skins: Query<(Entity, &SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    parents: Query<&ChildOf>,
    roots: Query<Entity, With<PlayerRoot>>,
    mut exit: MessageWriter<AppExit>,
) {
    if *status != AssetStatus::Ready || animation.ready {
        return;
    }
    let names = &skater.animation.evaluator.frames.bone_names;
    let result = (|| -> Result<Option<Vec<BoneBinding>>, String> {
        for (entity, skin) in &skins {
            if !parents.iter_ancestors(entity).any(|e| roots.contains(e)) {
                continue;
            }
            let mut binding = Vec::with_capacity(skin.joints.len());
            for &joint in &skin.joints {
                let (name, _) = nodes
                    .get(joint)
                    .map_err(|_| "Skater skin joint is missing its name or transform")?;
                let bone = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(name.as_str()))
                    .ok_or_else(|| {
                        format!("Skater skin bone {name} is absent from the stock rig")
                    })?;
                let mut parent_bone = None;
                for parent in parents.iter_ancestors(joint) {
                    if roots.contains(parent) {
                        break;
                    }
                    if let Ok((name, transform)) = nodes.get(parent) {
                        if let Some(index) = names
                            .iter()
                            .position(|n| n.eq_ignore_ascii_case(name.as_str()))
                        {
                            parent_bone = Some(index);
                            break;
                        }
                        // The supplied GLB's armature node is identity. Reject
                        // another import convention rather than double a pose.
                        if !transform.to_matrix().abs_diff_eq(Mat4::IDENTITY, 0.00001) {
                            return Err(format!(
                                "Skater armature ancestor {name} has an unsupported transform"
                            ));
                        }
                    }
                }
                binding.push(BoneBinding {
                    entity: joint,
                    bone,
                    parent_bone,
                });
            }
            // All ten imported mesh primitives share these joint entities and
            // their authored inverse-bind matrices. Bind the joints once and
            // retain that mesh reference: the stock initialization pose is an
            // animation pose, not a replacement skin bind pose.
            return Ok(Some(binding));
        }
        Ok(None)
    })();
    match result {
        Ok(Some(binding)) => {
            animation.bindings = binding;
            animation.ready = true;
            info!(
                "GAME_CHARACTER_READY bones={} source=physical_stock_pose",
                animation.bindings.len()
            );
        }
        Ok(None) => (),
        Err(message) => {
            error!("{message}");
            exit.write(AppExit::error());
        }
    }
}
fn present(
    history: Res<crate::presentation::Presentation>,
    time: Res<Time<Fixed>>,
    animation: Res<AnimationStatus>,
    mut nodes: Query<&mut Transform>,
) {
    if !animation.ready {
        return;
    }
    // Existing GLB was exported through Blender: its bone-local axes are
    // rotated -90 degrees about X relative to the native frames. Both files'
    // world positions are Y-up. This is a skin basis change, not a physics turn.
    let basis = render_basis();
    let Some((previous, current)) = history.pair() else { return; };
    let alpha = time.overstep_fraction().clamp(0.0, 1.0);
    for binding in &animation.bindings {
        // Blend bone-local rotations, not matrix entries or independent world
        // positions: joints retain their hierarchy while limbs turn.
        let local = |snapshot: &crate::presentation::Snapshot| {
            let global = snapshot.bones[binding.bone] * basis;
            Transform::from_matrix(if let Some(parent) = binding.parent_bone {
                (snapshot.bones[parent] * basis).inverse() * global
            } else { global })
        };
        if let Ok(mut transform) = nodes.get_mut(binding.entity) {
            *transform = crate::presentation::blend(local(previous), local(current), alpha);
        }
    }
}
fn render_basis() -> Mat4 {
    Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W)
}
pub(crate) fn native_matrix(matrix: NativeMatrix) -> Mat4 {
    Mat4::from_cols(
        Vec3::from_array(matrix[0][..3].try_into().unwrap()).extend(0.0),
        Vec3::from_array(matrix[1][..3].try_into().unwrap()).extend(0.0),
        Vec3::from_array(matrix[2][..3].try_into().unwrap()).extend(0.0),
        Vec3::from_array(matrix[3][..3].try_into().unwrap()).extend(1.0),
    )
}
