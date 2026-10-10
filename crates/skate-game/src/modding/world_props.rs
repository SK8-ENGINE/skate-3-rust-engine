//! Mod-created dynamic props (doc 27 "Props created mid-game"): `sdk.world.spawn_prop` copies a map
//! prop (model, collision, physics block, type data) through the engine's runtime prop path
//! (`GamePhysics::spawn_runtime_prop`, the same path a released hand prop takes) and
//! `sdk.world.remove_prop` / mod disable remove it again.
use bevy::{camera::primitives::Aabb, prelude::*};
use skate_core::math::{Basis3, Vector3};
use std::collections::BTreeMap;

type Key = (String, String);
const MAX_PROPS_PER_MOD: usize = 64;
const MAX_PROPS_TOTAL: usize = 256;

/// Live mod props by (owner, key): body id and render entity.
#[derive(Resource, Default)]
pub(super) struct ModProps {
    props: BTreeMap<Key, (u32, Option<Entity>)>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ModProps>();
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn(
    world: &mut World,
    owner: &str,
    key: String,
    from: u32,
    position: [f32; 3],
    yaw: f32,
    velocity: [f32; 3],
    spin: [f32; 3],
) -> Result<(), String> {
    remove(world, owner, &key);
    let state = world.resource::<ModProps>();
    if state.props.keys().filter(|(o, _)| o == owner).count() >= MAX_PROPS_PER_MOD || state.props.len() >= MAX_PROPS_TOTAL {
        return Err("world_spawn_prop: prop limit reached".into());
    }
    let mut physics = world.get_resource_mut::<crate::physics::GamePhysics>().ok_or("world_spawn_prop: no physics")?;
    let (mut spec, surface) = physics.runtime_copy_of(from).ok_or_else(|| format!("world_spawn_prop: unknown prop {from}"))?;
    let (s, c) = yaw.sin_cos();
    spec.origin = Vector3::new(position[0], position[1], position[2]);
    spec.basis = Basis3 { columns: [[c, 0., -s], [0., 1., 0.], [s, 0., c]] };
    spec.linear_velocity = Vector3::new(velocity[0], velocity[1], velocity[2]);
    spec.angular_velocity = Vector3::new(spin[0], spin[1], spin[2]);
    let id = physics.spawn_runtime_prop(&spec, surface).ok_or("world_spawn_prop: no body")?;
    let entity = clone_model(world, from, id);
    world.resource_mut::<ModProps>().props.insert((owner.to_string(), key), (id, entity));
    Ok(())
}

/// A copy of prop `from`'s render entity tagged with the new body id (its pose follows the body;
/// the source's scale is kept, as the collision triangles carry it).
fn clone_model(world: &mut World, from: u32, id: u32) -> Option<Entity> {
    let mut query = world.query::<(&crate::skate_world::PropInstance, &Transform, &Children)>();
    let (source, transform, children) = query.iter(world).find(|(p, ..)| p.id == from)?;
    let root = (
        Name::new(format!("mod prop {id}")),
        crate::skate_world::PropInstance { id, template: source.template.clone(), name: format!("{}#{id}", source.name) },
        Transform::from_scale(transform.scale),
        Visibility::default(),
    );
    let children: Vec<Entity> = children.iter().collect();
    let parts: Vec<_> = children
        .iter()
        .filter_map(|&child| {
            let e = world.get_entity(child).ok()?;
            Some((
                e.get::<Mesh3d>()?.clone(),
                e.get::<MeshMaterial3d<crate::retail_render::WorldMaterial>>()?.clone(),
                *e.get::<Transform>()?,
                e.get::<Aabb>().copied(),
            ))
        })
        .collect();
    let mut parent = world.spawn(root);
    parent.with_children(|c| {
        for (mesh, material, transform, aabb) in parts {
            let mut child = c.spawn((mesh, material, transform));
            if let Some(aabb) = aabb {
                child.insert(aabb);
            }
        }
    });
    Some(parent.id())
}

pub(super) fn remove(world: &mut World, owner: &str, key: &str) {
    let Some((id, entity)) = world.resource_mut::<ModProps>().props.remove(&(owner.to_string(), key.to_string())) else {
        return;
    };
    if let Some(mut physics) = world.get_resource_mut::<crate::physics::GamePhysics>() {
        physics.remove_runtime_prop(id);
    }
    if let Some(entity) = entity {
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    }
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    let keys: Vec<_> = world.resource::<ModProps>().props.keys().filter(|(o, _)| o == owner).cloned().collect();
    for (owner, key) in keys {
        remove(world, &owner, &key);
    }
}

pub(super) fn clear(world: &mut World) {
    let keys: Vec<_> = world.resource::<ModProps>().props.keys().cloned().collect();
    for (owner, key) in keys {
        remove(world, &owner, &key);
    }
}
