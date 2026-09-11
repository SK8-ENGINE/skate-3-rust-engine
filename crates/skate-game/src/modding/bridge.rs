//! Solid dual-world boundary. Native local bones participate in BoardStep;
//! remote actor mirrors participate in Rapier. Never solve the local actor in
//! both worlds, and never feed reactions back to another peer's body.
use super::Mods;
use bevy::prelude::*;
use skate_core::{math::Vector3, physics::{
    board_step::CollisionBody,
    board_world::{ExternalLineHit, ExternalQueries, WorldLineHit},
    triangle_query::TriangleLineHit,
}};
use skate_dynamics::{SolidBody, solid::{collider_triangles, sweep_sphere}};
use std::{collections::BTreeSet, sync::Arc};

fn xyz(v: Vector3) -> [f32;3] { [v.x,v.y,v.z] }
fn native(v: [f32;3]) -> Vector3 { Vector3::new(v[0],v[1],v[2]) }

pub(crate) struct MovingQueries(pub Vec<SolidBody>);
impl ExternalQueries for MovingQueries {
    fn line(&self, start: Vector3, end: Vector3, radius: f32) -> Option<ExternalLineHit> {
        let (id, c) = sweep_sphere(&self.0, xyz(start), xyz(end), radius)?;
        let body = self.0.iter().find(|b| b.id == id)?;
        let q = body.pose.rotation;
        let transform = Mat4::from_rotation_translation(
            Quat::from_xyzw(q.x,q.y,q.z,q.w), Vec3::from_array(body.pose.translation.to_array()));
        Some(ExternalLineHit {
            hit: WorldLineHit { geometry: TriangleLineHit {
                position: native(c.point_b.to_array()), normal: native(c.normal.to_array()),
                fraction: c.time_of_impact, volume_parameter: [0.;3],
            }, tag: 0 }, surface: 0, geometry_id: 0x8000_0000 | (id as u32 & 0x7fff_ffff),
            frame: transform.to_cols_array_2d(),
        })
    }
    fn nearby(&self, center: Vector3, radius: f32) -> Vec<[Vector3;3]> {
        let center = Vec3::from_array(xyz(center));
        if !center.is_finite() || !radius.is_finite() || radius < 0. { return Vec::new(); }
        let mut triangles = Vec::new();
        for body in &self.0 {
            for collider in &body.colliders {
                let aabb = collider.shape.compute_aabb(&collider.pose);
                let min = Vec3::from_array(aabb.mins.to_array());
                let max = Vec3::from_array(aabb.maxs.to_array());
                if center.distance_squared(center.clamp(min,max)) > radius * radius { continue; }
                for t in collider_triangles(collider) {
                    let verts = t.map(Vec3::from_array);
                    let min = verts[0].min(verts[1]).min(verts[2]);
                    let max = verts[0].max(verts[1]).max(verts[2]);
                    if center.distance_squared(center.clamp(min,max)) <= radius*radius {
                        triangles.push(t.map(native));
                        if triangles.len() >= 64 { return triangles; }
                    }
                }
            }
        }
        triangles
    }
}

pub(crate) fn push_dynamics_into_boardworld(
    mods: &Mods, physics: &mut crate::physics::GamePhysics,
    skater: &crate::physics::SkaterRuntime,
) {
    let skip: BTreeSet<u64> = mods.skater_proxies.values().copied().collect();
    let attached = mods.attach.as_ref().and_then(|a| mods.bodies.get(&(a.owner.clone(),a.body.clone()))).copied();
    let local: BTreeSet<_> = mods.bodies.iter().filter(|((o,_),_)| !o.starts_with('@')).map(|(_,id)| *id).collect();
    let solids: Vec<_> = mods.world.solid_bodies().into_iter()
        .filter(|b| !skip.contains(&b.id) && Some(b.id) != attached).collect();
    physics.set_external_queries(Some(Arc::new(MovingQueries(solids.clone()))));
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    for solid in solids {
        let owned = local.contains(&solid.id);
        proxies.append_solid(solid, physics, skater, owned);
    }
    physics.network_proxies = proxies;
}

pub(crate) fn take_reactions(mods: &mut Mods, physics: &mut crate::physics::GamePhysics) {
    for (id, linear, angular, position, orientation) in std::mem::take(&mut physics.network_proxies.dynamics_deltas) {
        if !mods.bodies.iter().any(|((o,_),body)| *body == id && !o.starts_with('@')) { continue; }
        mods.world.add_velocity_delta(id, linear, angular);
        mods.world.correct_pose(id, position, orientation);
    }
}

pub(crate) fn push_skater_into_rapier(
    mods: &mut Mods, physics: &crate::physics::GamePhysics,
    skater: &crate::physics::SkaterRuntime,
) {
    use skate_dynamics::rapier3d::prelude::{Pose,Rotation,Vector};
    let base = skater.skeleton.bodies().len() + skater.skeleton_drives.targets.bodies.len();
    let mut parts=std::collections::BTreeMap::<usize,Vec<skate_dynamics::SolidCollider>>::new();
    for volume in &physics.network_proxies.volumes {
        let CollisionBody::Attached(body_index)=volume.body else {continue};
        let Some((shape,pose))=crate::physics::solid_contacts::shape(volume.primitive) else {continue};
        parts.entry(body_index).or_default().push(skate_dynamics::SolidCollider {
            shape,pose,friction:volume.material.dynamic_friction,
        });
    }
    let mut live=BTreeSet::new();
    for (index,parts) in parts {
        let Some(body)=index.checked_sub(base).and_then(|i|physics.network_proxies.bodies.get(i)) else {continue};
        let q=body.rates.orientation;
        let pose=Pose::from_parts(Vector::from_array(xyz(body.rates.position)),
            Rotation::from_xyzw(q.x,q.y,q.z,q.w).normalize());
        live.insert(index);
        match mods.world.upsert_actor_shadow(mods.skater_proxies.get(&index).copied(),pose,
            body.inertia.inverse_mass,xyz(body.inertia.inverse_tensor),
            xyz(body.rates.linear_velocity),xyz(body.rates.angular_velocity),&parts) {
            Ok(id)=>{mods.skater_proxies.insert(index,id);},
            Err(error)=>warn!("remote actor shadow {index}: {error}"),
        }
    }
    let stale:Vec<_>=mods.skater_proxies.keys().filter(|key|!live.contains(key)).copied().collect();
    for key in stale {
        if let Some(id)=mods.skater_proxies.remove(&key) {mods.world.remove(id);}
    }
}

pub(crate) fn dynamics_to_board(
    mods: Option<Res<Mods>>, mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>, replay: Res<crate::replay::Replay>,
) {
    if replay.active { return; }
    if let Some(mods) = mods { push_dynamics_into_boardworld(&mods,&mut physics,&skater); }
}

pub(crate) fn sync_network(world: &mut World) { super::replication::sync(world); }
