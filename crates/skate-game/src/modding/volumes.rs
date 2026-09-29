//! World-space boxes tested against skater positions. Dynamics sensors never
//! see the native skater, so overlap is computed here from observation.
use super::{observation::WireObs, Mods};
use bevy::prelude::*;
use serde_json::{json, Map, Value};
use skate_mods::VolumeOptions;
use std::collections::BTreeMap;

const MAX_PER_MOD: usize = 32;

pub(super) struct Volume {
    pub position: Vec3,
    pub size: Vec3,
    pub rotation: Quat,
    pub visible: bool,
    pub color: [f32; 3],
    pub opacity: f32,
    pub entity: Option<Entity>,
}

impl Volume {
    fn contains(&self, point: Vec3) -> bool {
        let local = self.rotation.inverse() * (point - self.position);
        let half = self.size * 0.5;
        local.x.abs() <= half.x && local.y.abs() <= half.y && local.z.abs() <= half.z
    }
}

pub(super) fn set(
    world: &mut World,
    mods: &mut Mods,
    owner: &str,
    key: String,
    options: VolumeOptions,
) -> Result<(), String> {
    let slot = (owner.to_owned(), key.clone());
    if !mods.volumes.contains_key(&slot)
        && mods.volumes.keys().filter(|(o, _)| o == owner).count() >= MAX_PER_MOD
    {
        return Err("32 volumes per mod maximum".into());
    }
    remove_entity(world, mods, &slot);
    let rotation = options
        .rotation
        .map(Quat::from_array)
        .unwrap_or(Quat::IDENTITY)
        .normalize();
    let mut volume = Volume {
        position: Vec3::from_array(options.position),
        size: Vec3::from_array(options.size),
        rotation,
        visible: options.visible,
        color: options.color,
        opacity: options.opacity,
        entity: None,
    };
    if volume.visible {
        volume.entity = Some(spawn_box(world, &volume));
    }
    mods.volumes.insert(slot, volume);
    Ok(())
}

pub(super) fn remove(world: &mut World, mods: &mut Mods, owner: &str, key: &str) {
    remove_entity(world, mods, &(owner.to_owned(), key.to_owned()));
    mods.volumes.remove(&(owner.to_owned(), key.to_owned()));
}

pub(super) fn clear_owner(world: &mut World, mods: &mut Mods, owner: &str) {
    let keys: Vec<_> = mods
        .volumes
        .keys()
        .filter(|(o, _)| o == owner)
        .cloned()
        .collect();
    for key in keys {
        remove_entity(world, mods, &key);
        mods.volumes.remove(&key);
    }
}

pub(super) fn clear(world: &mut World, mods: &mut Mods) {
    let keys: Vec<_> = mods.volumes.keys().cloned().collect();
    for key in keys {
        remove_entity(world, mods, &key);
    }
    mods.volumes.clear();
}

pub(super) fn snapshot(
    mods: &Mods,
    owner: &str,
    local_id: &str,
    local: &WireObs,
    remotes: &BTreeMap<u64, WireObs>,
) -> Value {
    let mut out = Map::new();
    for ((mod_id, key), volume) in &mods.volumes {
        if mod_id != owner {
            continue;
        }
        let mut inside = Vec::new();
        if volume.contains(Vec3::from_array(local.p)) {
            inside.push(local_id.to_owned());
        }
        for (peer, obs) in remotes {
            if volume.contains(Vec3::from_array(obs.p)) {
                inside.push(peer.to_string());
            }
        }
        inside.sort();
        inside.dedup();
        out.insert(
            key.clone(),
            json!({
                "position": volume.position.to_array(),
                "size": volume.size.to_array(),
                "rotation": volume.rotation.to_array(),
                "inside": inside,
            }),
        );
    }
    Value::Object(out)
}

fn remove_entity(world: &mut World, mods: &mut Mods, slot: &(String, String)) {
    if let Some(volume) = mods.volumes.get_mut(slot) {
        if let Some(entity) = volume.entity.take() {
            world.despawn(entity);
        }
    }
}

fn spawn_box(world: &mut World, volume: &Volume) -> Entity {
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(volume.size.x, volume.size.y, volume.size.z));
    let a = volume.opacity.clamp(0.0, 1.0);
    let c = volume.color;
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgba(c[0], c[1], c[2], a),
            alpha_mode: if a < 0.999 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            unlit: true,
            cull_mode: None,
            ..default()
        });
    world
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform {
                translation: volume.position,
                rotation: volume.rotation,
                scale: Vec3::ONE,
            },
            Visibility::Visible,
            crate::retail_character::ModGraphicsLit,
        ))
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_box_contains_local_point() {
        let volume = Volume {
            position: Vec3::new(10.0, 1.0, 0.0),
            size: Vec3::new(2.0, 2.0, 4.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            visible: false,
            color: [1.0; 3],
            opacity: 0.3,
            entity: None,
        };
        assert!(volume.contains(Vec3::new(10.0, 1.0, 0.5)));
        assert!(!volume.contains(Vec3::new(10.0, 1.0, 3.0)));
    }
}
