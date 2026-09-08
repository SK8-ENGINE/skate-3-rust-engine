use super::*;
use bevy::mesh::skinning::SkinnedMesh;
use skate_net::interpolation::{Buffer, Clock, position};
#[derive(Resource)]
struct RemoteSkins {
    reported: f64,
    actors: BTreeMap<u64, RemoteSkin>,
    rest: Vec<Mat4>,
    parents: Vec<i32>,
}
#[derive(Default)]
struct RemoteSkin {
    root: Option<Entity>,
    bindings: Vec<(Entity, usize, Option<usize>)>,
    positions: Buffer<[f32; 3]>,
    roots: Buffer<Transform>,
    poses: Buffer<Vec<Transform>>,
    clock: Clock,
    epoch: u64,
}
pub(super) struct RemoteRenderPlugin;
impl Plugin for RemoteRenderPlugin {
    fn build(&self, app: &mut App) {
        let idle = app
            .world()
            .resource::<crate::assets::AssetManifest>()
            .0
            .initial_animation
            .clone();
        let skater = app.world().resource::<SkaterRuntime>();
        let pose = skater
            .animation
            .evaluator
            .evaluate(&[
                skate_core::animation::playback_tree::PoseCommand::Clip {
                    name: idle,
                    previous_time: 0.,
                    time: 0.,
                    loops: 0,
                },
                skate_core::animation::playback_tree::PoseCommand::Pose {
                    name: "RIG_TPOSE".into(),
                },
                skate_core::animation::playback_tree::PoseCommand::Add { motion_is_a: true },
            ])
            .unwrap_or_else(|_| skater.animation.pose.clone());
        let rest = pose
            .iter()
            .copied()
            .map(skate_core::animation::output::sqt_to_matrix)
            .map(crate::animation::native_matrix)
            .collect();
        let parents = skater
            .animation
            .evaluator
            .frames
            .parents
            .iter()
            .map(|&p| p as i32)
            .collect();
        app.insert_resource(RemoteSkins {
            reported: 0.,
            actors: BTreeMap::new(),
            rest,
            parents,
        })
        .add_systems(
            Update,
            (spawn, bind, present).chain().after(FrameSet::Animation),
        );
    }
}
fn spawn(
    mut commands: Commands,
    net: Res<Multiplayer>,
    mut skins: ResMut<RemoteSkins>,
    server: Res<AssetServer>,
) {
    let removed: Vec<_> = skins
        .actors
        .keys()
        .filter(|id| !net.remotes.contains_key(id))
        .copied()
        .collect();
    for id in removed {
        if let Some(root) = skins.actors.remove(&id).and_then(|s| s.root) {
            commands.entity(root).despawn();
        }
    }
    for (&id, _) in &net.remotes {
        let skin = skins.actors.entry(id).or_insert_with(|| RemoteSkin {
            clock: Clock::for_connection(net.loopback),
            ..default()
        });
        if skin.root.is_some() {
            continue;
        }
        skin.root = Some(
            commands
                .spawn((
                    Transform::default(),
                    Visibility::default(),
                    Name::new("Remote default skater"),
                ))
                .with_children(|root| {
                    root.spawn(SceneRoot(
                        server.load(GltfAssetLabel::Scene(0).from_asset("private/skater.glb")),
                    ));
                })
                .id(),
        );
    }
}

fn bind(
    mut skins: ResMut<RemoteSkins>,
    skater: Res<SkaterRuntime>,
    meshes: Query<(Entity, &SkinnedMesh)>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
) {
    for skin in skins.actors.values_mut() {
        let Some(root) = skin.root else {
            continue;
        };
        if !skin.bindings.is_empty() {
            continue;
        }
        let bone_names = &skater.animation.evaluator.frames.bone_names;
        for (entity, mesh) in &meshes {
            if !parents.iter_ancestors(entity).any(|p| p == root) {
                continue;
            }
            let mut bindings = vec![];
            for &joint in &mesh.joints {
                let Ok(name) = names.get(joint) else {
                    return;
                };
                let Some(bone) = bone_names
                    .iter()
                    .position(|b| b.eq_ignore_ascii_case(name.as_str()))
                else {
                    return;
                };
                let parent = parents
                    .iter_ancestors(joint)
                    .take_while(|&p| p != root)
                    .find_map(|p| {
                        names.get(p).ok().and_then(|n| {
                            bone_names
                                .iter()
                                .position(|b| b.eq_ignore_ascii_case(n.as_str()))
                        })
                    });
                bindings.push((joint, bone, parent));
            }
            skin.bindings = bindings;
            skin.poses = Buffer::default();
            break;
        }
    }
}

fn globals(bones: &[skate_net::Bone], skin: &RemoteSkins) -> Vec<Mat4> {
    let mut result = vec![None; skin.rest.len()];
    for b in bones {
        if let Some(slot) = result.get_mut(b.index as usize) {
            *slot = Some(network::matrix(b.pose));
        }
    }
    fn visit(i: usize, skin: &RemoteSkins, result: &mut [Option<Mat4>]) -> Mat4 {
        if let Some(m) = result[i] {
            return m;
        }
        let p = skin.parents[i];
        let matrix = if p >= 0 {
            visit(p as usize, skin, result) * skin.rest[i]
        } else {
            skin.rest[i]
        };
        result[i] = Some(matrix);
        matrix
    }
    for i in 0..result.len() {
        visit(i, skin, &mut result);
    }
    result.into_iter().map(Option::unwrap).collect()
}
fn present(
    mut net: ResMut<Multiplayer>,
    mut skins: ResMut<RemoteSkins>,
    mut nodes: Query<&mut Transform>,
) {
    let basis = Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W);
    let now = net.started.elapsed().as_secs_f64();
    for (&id, remote) in &net.remotes {
        let Some(mut skin) = skins.actors.remove(&id) else {
            continue;
        };
        if skin.epoch != remote.epoch {
            skin.positions = Buffer::default();
            skin.roots = Buffer::default();
            skin.poses = Buffer::default();
            skin.clock = Clock::for_connection(net.loopback);
            skin.epoch = remote.epoch;
        }
        for sample in &remote.roots {
            if skin
                .roots
                .samples
                .back()
                .is_some_and(|p| p.time >= sample.captured)
            {
                continue;
            }
            if skin.roots.insert(
                sample.captured,
                Transform::from_matrix(network::matrix(sample.pose)),
            ) {
                skin.positions.insert(sample.captured, sample.pose.p);
                skin.clock.observe(0, sample.captured, sample.received);
            }
        }
        // Each animation sample is resolved once; physics arrivals never duplicate it.
        if !skin.bindings.is_empty() {
            for sample in &remote.poses {
                if skin
                    .poses
                    .samples
                    .back()
                    .is_some_and(|p| p.time >= sample.captured)
                {
                    continue;
                }
                let g = globals(&sample.bones, &skins);
                let locals = skin
                    .bindings
                    .iter()
                    .map(|&(_, i, parent)| {
                        Transform::from_matrix(
                            parent
                                .map_or(g[i] * basis, |p| (g[p] * basis).inverse() * g[i] * basis),
                        )
                    })
                    .collect();
                if skin.poses.insert(sample.captured, locals) {
                    skin.clock.observe(1, sample.captured, sample.received);
                }
            }
        }
        if let (Some(root), Some(time)) = (skin.root, skin.clock.step(now)) {
            if let Some((a, b, alpha)) = skin.roots.pair(time) {
                let mut transform = crate::presentation::blend(
                    skin.roots.samples[a].value,
                    skin.roots.samples[b].value,
                    alpha,
                );
                if let Some(p) = position(&skin.positions, time) {
                    transform.translation = Vec3::from_array(p);
                }
                if let Ok(mut t) = nodes.get_mut(root) {
                    *t = transform;
                }
            }
            if let Some((a, b, alpha)) = skin.poses.pair(time) {
                let ag = &skin.poses.samples[a].value;
                let bg = &skin.poses.samples[b].value;
                for ((&(entity, _, _), a), b) in skin.bindings.iter().zip(ag).zip(bg) {
                    if let Ok(mut t) = nodes.get_mut(entity) {
                        *t = crate::presentation::blend(*a, *b, alpha);
                    }
                }
            }
        }
        skins.actors.insert(id, skin);
    }
    if now < skins.reported || now - skins.reported >= 1. {
        let delay = skins
            .actors
            .values()
            .map(|s| s.clock.delay)
            .fold(0., f64::max);
        let stalls: u64 = skins.actors.values().map(|s| s.clock.underruns).sum();
        net.visual_status = format!(
            "Visual interpolation: {:.0} ms target buffer | stalls {}",
            delay * 1000.,
            stalls
        );
        if net.active() {
            info!("MULTIPLAYER_INTERPOLATION {}", net.visual_status);
        }
        skins.reported = now;
    }
}
