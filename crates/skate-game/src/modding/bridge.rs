//! Dual-world collision bridge: DynamicsWorld ↔ BoardWorld + multiplayer pose sync.
use super::Mods;
use bevy::prelude::*;
use skate_dynamics::{BodyDesc, BodyType, ExportedVolume, Shape};
use skate_core::physics::world_contact::ContactPrimitive;
use std::collections::{BTreeMap, BTreeSet};

const CULL_DIST_SQ: f32 = 15. * 15.;

pub(crate) fn push_dynamics_into_boardworld(
    mods: &Mods,
    physics: &mut crate::physics::GamePhysics,
    skater: &crate::physics::SkaterRuntime,
) {
    let skip: BTreeSet<u64> = mods.skater_proxies.values().copied().collect();
    let exports = mods.world.export_boardworld_volumes();
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    for export in exports {
        if skip.contains(&export.body) {
            continue;
        }
        proxies.append_dynamics(&export, physics, skater);
    }
    physics.network_proxies = proxies;
}

pub(crate) fn push_skater_into_rapier(
    mods: &mut Mods,
    physics: &crate::physics::GamePhysics,
    skater: &crate::physics::SkaterRuntime,
) {
    if mods.attach.is_some() {
        clear_skater_proxies(mods);
        return;
    }
    let near_dynamics = mods.bodies.values().any(|&id| {
        mods.world.read(id).is_some_and(|snap| {
            physics
                .board
                .bodies()
                .iter()
                .chain(skater.skeleton.bodies())
                .any(|b| {
                    Vec3::from_array(snap.position).distance_squared(Vec3::new(
                        b.rates.position.x,
                        b.rates.position.y,
                        b.rates.position.z,
                    )) < CULL_DIST_SQ
                })
        })
    });
    if !near_dynamics {
        clear_skater_proxies(mods);
        return;
    }
    let mut volumes = crate::physics::colliders::world_volumes(&physics.board, &physics.settings);
    volumes.retain(|v| skater.board_possession_live.volume_enabled(v.body));
    if let Ok(skeleton) =
        crate::physics::skeleton_colliders::enabled_volumes(&skater.skeleton, &skater.skeleton_collision)
    {
        volumes.extend(skeleton);
    }
    let mut live = BTreeSet::new();
    for (index, volume) in volumes.iter().enumerate() {
        let Some((shape, position, rotation)) = primitive_to_rapier(&volume.primitive) else {
            continue;
        };
        live.insert(index);
        let existing = mods.skater_proxies.get(&index).copied();
        match mods
            .world
            .upsert_kinematic_proxy(existing, shape, position, rotation, 0.3)
        {
            Ok(id) => {
                mods.skater_proxies.insert(index, id);
            }
            Err(e) => warn!("skater proxy {index}: {e}"),
        }
    }
    let removed: Vec<_> = mods
        .skater_proxies
        .keys()
        .copied()
        .filter(|k| !live.contains(k))
        .collect();
    for key in removed {
        if let Some(id) = mods.skater_proxies.remove(&key) {
            mods.world.remove(id);
        }
    }
}

fn clear_skater_proxies(mods: &mut Mods) {
    for id in std::mem::take(&mut mods.skater_proxies).into_values() {
        mods.world.remove(id);
    }
}

fn primitive_to_rapier(primitive: &ContactPrimitive) -> Option<(Shape, [f32; 3], [f32; 4])> {
    match primitive {
        ContactPrimitive::Sphere(s) => Some((
            Shape::Sphere { radius: s.radius },
            [s.center.x, s.center.y, s.center.z],
            Quat::IDENTITY.to_array(),
        )),
        ContactPrimitive::Capsule {
            center,
            axis,
            half_length,
            radius,
        } => {
            let axis = Vec3::new(axis.x, axis.y, axis.z);
            if !(axis.length() > 1e-6) {
                return None;
            }
            let rot = Quat::from_rotation_arc(Vec3::Y, axis.normalize());
            Some((
                Shape::Capsule {
                    half_height: *half_length,
                    radius: *radius,
                },
                [center.x, center.y, center.z],
                rot.to_array(),
            ))
        }
        ContactPrimitive::RoundedBox {
            center,
            basis,
            half_extents,
            radius,
        } => {
            let mat = Mat3::from_cols_array_2d(&basis.columns);
            let rot = Quat::from_mat3(&mat).normalize();
            // Rapier cuboids are sharp; inflate half-extents by rounding so overall size matches.
            let half = [
                half_extents.x + radius,
                half_extents.y + radius,
                half_extents.z + radius,
            ];
            Some((
                Shape::Box {
                    half_extents: half,
                },
                [center.x, center.y, center.z],
                rot.to_array(),
            ))
        }
        ContactPrimitive::Triangle(_) => None,
    }
}

pub(crate) fn dynamics_to_board(
    mods: Option<Res<Mods>>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
) {
    if replay.active {
        return;
    }
    let Some(mods) = mods else {
        return;
    };
    push_dynamics_into_boardworld(&mods, &mut physics, &skater);
}

/// Binary pose snapshot for APPLICATION channel (`dyn:{mod}:{key}`).
fn pack_pose(export: &ExportedVolume, fingerprint: u64, mod_id: &str, body_key: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128);
    bytes.push(1); // version
    bytes.extend_from_slice(&fingerprint.to_le_bytes());
    let mid = mod_id.as_bytes();
    let kid = body_key.as_bytes();
    bytes.push(mid.len().min(64) as u8);
    bytes.extend_from_slice(&mid[..mid.len().min(64)]);
    bytes.push(kid.len().min(64) as u8);
    bytes.extend_from_slice(&kid[..kid.len().min(64)]);
    match &export.shape {
        skate_dynamics::ExportedShape::Box { half_extents, rounding } => {
            bytes.push(0);
            for v in half_extents {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            bytes.extend_from_slice(&rounding.to_le_bytes());
        }
        skate_dynamics::ExportedShape::Sphere { radius } => {
            bytes.push(1);
            bytes.extend_from_slice(&radius.to_le_bytes());
        }
        skate_dynamics::ExportedShape::Capsule {
            half_height,
            radius,
        } => {
            bytes.push(2);
            bytes.extend_from_slice(&half_height.to_le_bytes());
            bytes.extend_from_slice(&radius.to_le_bytes());
        }
        skate_dynamics::ExportedShape::Triangles { .. } => {
            // Remotes fall back to a bounding box; trimesh sync deferred.
            bytes.push(0);
            for _ in 0..3 {
                bytes.extend_from_slice(&0.5f32.to_le_bytes());
            }
            bytes.extend_from_slice(&0f32.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&export.mass.to_le_bytes());
    for v in export.position {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in export.rotation {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in export.linvel {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in export.angvel {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

struct UnpackedPose {
    fingerprint: u64,
    mod_id: String,
    body_key: String,
    shape: Shape,
    mass: f32,
    position: [f32; 3],
    rotation: [f32; 4],
    linvel: [f32; 3],
    angvel: [f32; 3],
}

fn unpack_pose(bytes: &[u8]) -> Option<UnpackedPose> {
    if bytes.first().copied()? != 1 {
        return None;
    }
    let mut i = 1;
    let read_u64 = |i: &mut usize| -> Option<u64> {
        let v = u64::from_le_bytes(bytes.get(*i..*i + 8)?.try_into().ok()?);
        *i += 8;
        Some(v)
    };
    let read_f32 = |i: &mut usize| -> Option<f32> {
        let v = f32::from_le_bytes(bytes.get(*i..*i + 4)?.try_into().ok()?);
        *i += 4;
        Some(v)
    };
    let read_str = |i: &mut usize| -> Option<String> {
        let len = *bytes.get(*i)? as usize;
        *i += 1;
        let s = std::str::from_utf8(bytes.get(*i..*i + len)?).ok()?.to_owned();
        *i += len;
        Some(s)
    };
    let fingerprint = read_u64(&mut i)?;
    let mod_id = read_str(&mut i)?;
    let body_key = read_str(&mut i)?;
    let kind = *bytes.get(i)?;
    i += 1;
    let shape = match kind {
        0 => {
            let half_extents = [read_f32(&mut i)?, read_f32(&mut i)?, read_f32(&mut i)?];
            let _rounding = read_f32(&mut i)?;
            Shape::Box { half_extents }
        }
        1 => Shape::Sphere {
            radius: read_f32(&mut i)?,
        },
        2 => Shape::Capsule {
            half_height: read_f32(&mut i)?,
            radius: read_f32(&mut i)?,
        },
        _ => return None,
    };
    let mass = read_f32(&mut i)?;
    let position = [read_f32(&mut i)?, read_f32(&mut i)?, read_f32(&mut i)?];
    let rotation = [
        read_f32(&mut i)?,
        read_f32(&mut i)?,
        read_f32(&mut i)?,
        read_f32(&mut i)?,
    ];
    let linvel = [read_f32(&mut i)?, read_f32(&mut i)?, read_f32(&mut i)?];
    let angvel = [read_f32(&mut i)?, read_f32(&mut i)?, read_f32(&mut i)?];
    if !mass.is_finite()
        || mass <= 0.
        || position.iter().chain(&linvel).chain(&angvel).any(|v| !v.is_finite())
        || rotation.iter().any(|v| !v.is_finite())
    {
        return None;
    }
    Some(UnpackedPose {
        fingerprint,
        mod_id,
        body_key,
        shape,
        mass,
        position,
        rotation,
        linvel,
        angvel,
    })
}

fn wire_key(mod_id: &str, body_key: &str) -> String {
    let full = format!("dyn:{mod_id}:{body_key}");
    if full.len() <= 128 {
        full
    } else {
        format!(
            "dyn:{:016x}",
            skate_net::hash(full.as_bytes())
        )
    }
}

pub(crate) fn sync_network(world: &mut World) {
    let active = world
        .get_resource::<crate::multiplayer::Multiplayer>()
        .is_some_and(|n| n.active());
    if !active {
        return;
    }
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        let packages: BTreeMap<_, _> = mods
            .manager
            .packages
            .iter()
            .filter(|(_, p)| p.running())
            .map(|(id, p)| (id.clone(), p.content_fingerprint()))
            .collect();
        let skip: BTreeSet<u64> = mods.skater_proxies.values().copied().collect();
        let exports: Vec<_> = mods
            .world
            .export_boardworld_volumes()
            .into_iter()
            .filter(|e| !skip.contains(&e.body))
            .collect();
        let local_keys: BTreeMap<u64, (String, String)> = mods
            .bodies
            .iter()
            .filter(|((owner, _), _)| !owner.starts_with('@'))
            .map(|((o, k), id)| (*id, (o.clone(), k.clone())))
            .collect();
        let mut published = BTreeSet::new();
        let mut net = world.resource_mut::<crate::multiplayer::Multiplayer>();
        for export in &exports {
            let Some((mod_id, body_key)) = local_keys.get(&export.body) else {
                continue;
            };
            let Some(&fp) = packages.get(mod_id) else {
                continue;
            };
            let key = wire_key(mod_id, body_key);
            let value = pack_pose(export, fp, mod_id, body_key);
            if value.len() <= skate_net::lobby::MAX_APP_VALUE {
                net.publish_application(&key, value);
                published.insert(key);
            }
        }
        for key in mods.dyn_published.difference(&published) {
            net.publish_application(key, vec![]);
        }
        mods.dyn_published = published;

        let records = net.application_records();
        drop(net);
        let mut live_remote = BTreeSet::new();
        for (peer, _wire, _seq, bytes) in records {
            if bytes.is_empty() {
                continue;
            }
            let Some(pose) = unpack_pose(&bytes) else {
                continue;
            };
            let Some(&fp) = packages.get(&pose.mod_id) else {
                continue;
            };
            if fp != pose.fingerprint {
                continue;
            }
            let owner = format!("@{}:{}", peer, pose.mod_id);
            let slot = (owner.clone(), pose.body_key.clone());
            live_remote.insert(slot.clone());
            let existing = mods.bodies.get(&slot).copied();
            let need_spawn = existing.is_none_or(|id| {
                mods.world.read(id).is_none_or(|s| s.body_type != BodyType::Kinematic)
            });
            if need_spawn {
                if let Some(old) = existing {
                    mods.world.remove(old);
                    mods.bodies.remove(&slot);
                }
                match mods.world.spawn(BodyDesc {
                    shape: pose.shape.clone(),
                    body_type: BodyType::Kinematic,
                    mass: pose.mass,
                    position: pose.position,
                    friction: 0.7,
                    ..Default::default()
                }) {
                    Ok(id) => {
                        mods.world.set_pose(id, pose.position, pose.rotation);
                        mods.world.set_linvel(id, pose.linvel);
                        mods.world.set_angvel(id, pose.angvel);
                        mods.bodies.insert(slot, id);
                    }
                    Err(e) => warn!("remote dynamics spawn: {e}"),
                }
            } else if let Some(id) = existing {
                mods.world.set_pose(id, pose.position, pose.rotation);
                mods.world.set_linvel(id, pose.linvel);
                mods.world.set_angvel(id, pose.angvel);
            }
        }
        let stale: Vec<_> = mods
            .bodies
            .keys()
            .filter(|(owner, _)| owner.starts_with('@'))
            .filter(|slot| !live_remote.contains(slot))
            .cloned()
            .collect();
        for slot in stale {
            if let Some(id) = mods.bodies.remove(&slot) {
                mods.world.remove(id);
            }
        }
    });
}
