//! Generic actor-to-body attachments and clearance-checked, upright detach.
//! The native teleport owner performs the real on-foot state transition.
use super::{AttachState,Mods};
use bevy::prelude::*;
use skate_mods::scene::DetachOptions;
use std::time::Instant;

pub(crate) fn local_root(mods:&Mods) -> Option<Transform> {
    if let Some(a)=&mods.attach {
        let id=mods.bodies.get(&(a.owner.clone(),a.body.clone()))?;
        let body=mods.world.read(*id)?;
        let rotation=Quat::from_array(body.rotation).normalize();
        return Some(Transform::from_translation(Vec3::from_array(body.position)+rotation*a.offset).with_rotation(rotation));
    }
    mods.detach_pending.as_ref().map(|(pose,_)|*pose)
}
pub(super) fn attach(world:&mut World,mods:&mut Mods,owner:&str,body:String,offset:[f32;3]) -> Result<(),String> {
    super::resolve_body(mods,owner,&body)?;
    if mods.attach.as_ref().is_some_and(|a|a.owner!=owner) { return Err("player already attached by another mod".into()); }
    if mods.detach_pending.is_some() { return Ok(()); }
    mods.detach_error=None;
    if let Some(a)=&mut mods.attach { a.body=body; a.offset=Vec3::from_array(offset); return Ok(()); }
    let native=world.resource::<crate::physics::SkaterRuntime>().animated_skeleton.roots.animation_to_world;
    let fallback=Transform::from_translation(Vec3::new(native[3][0],native[3][1],native[3][2]))
        .with_rotation(Quat::from_rotation_y(native[2][0].atan2(native[2][2])));
    let mut hidden=Vec::new();
    let roots:Vec<_>=world.query_filtered::<Entity,With<crate::world::PlayerRoot>>().iter(world).collect();
    for entity in roots {
        if let Some(mut visible)=world.get_mut::<Visibility>(entity) {
            hidden.push((entity,*visible));*visible=Visibility::Hidden;
        }
    }
    mods.attach=Some(AttachState { owner:owner.to_owned(),body,offset:Vec3::from_array(offset),hidden,fallback });
    sync(world,mods);
    Ok(())
}

fn candidates(mods:&Mods,a:&AttachState,options:&DetachOptions) -> Vec<Vec3> {
    if !options.candidates.is_empty() { return options.candidates.iter().copied().map(Vec3::from_array).collect(); }
    let Some(id)=mods.bodies.get(&(a.owner.clone(),a.body.clone())) else { return Vec::new() };
    let Some(body)=mods.world.solid_bodies().into_iter().find(|b|b.id==*id) else { return Vec::new() };
    let mut min=Vec3::splat(f32::INFINITY);let mut max=Vec3::splat(f32::NEG_INFINITY);
    for collider in &body.colliders {
        let pose=body.pose.inverse()*collider.pose;
        let bounds=collider.shape.compute_aabb(&pose);
        min=min.min(Vec3::from_array(bounds.mins.to_array()));max=max.max(Vec3::from_array(bounds.maxs.to_array()));
    }
    if !min.is_finite() || !max.is_finite() { return Vec::new(); }
    let c=(min+max)*0.5;let gap=options.radius+0.4;
    vec![Vec3::new(max.x+gap,min.y,c.z),Vec3::new(min.x-gap,min.y,c.z),
        Vec3::new(c.x,min.y,max.z+gap),Vec3::new(c.x,min.y,min.z-gap),
        Vec3::new(max.x+gap,min.y,max.z+gap),Vec3::new(min.x-gap,min.y,min.z-gap)]
}
fn grounded_clear(mods:&Mods,seed:Vec3,options:&DetachOptions) -> Option<Vec3> {
    let origin=seed+Vec3::Y*(options.ground_snap*0.5+0.5);
    let hit=mods.world.raycast_ground(origin.to_array(),[0.,-1.,0.],options.ground_snap+options.height)?;
    if hit.normal[1] < 0.6 { return None; }
    let feet=Vec3::from_array(hit.point)+Vec3::Y*0.06;
    mods.world.capsule_clear(feet.to_array(),options.height,options.radius,0.02,&[]).then_some(feet)
}
fn safe_exit(mods:&Mods,a:&AttachState,options:&DetachOptions) -> Option<Transform> {
    let id=mods.bodies.get(&(a.owner.clone(),a.body.clone()))?;
    let body=mods.world.read(*id)?;
    let q=Quat::from_array(body.rotation).normalize();
    let f=q*Vec3::Z;let yaw=f.x.atan2(f.z);
    for offset in candidates(mods,a,options) {
        let seed=Vec3::from_array(body.position)+q*offset;
        if let Some(feet)=grounded_clear(mods,seed,options) {
            return Some(Transform::from_translation(feet).with_rotation(Quat::from_rotation_y(yaw)));
        }
    }
    None
}

pub(super) fn detach(world:&mut World,mods:&mut Mods,forced:bool,options:&DetachOptions) -> bool {
    let Some(a)=mods.attach.as_ref() else { return true };
    let pending=world.resource::<crate::physics::SkaterRuntime>().player_input.pending_teleport().is_some();
    let target=if pending && forced { None } else {
        safe_exit(mods,a,options).or_else(|| {
            forced.then(|| {
                let mut fallback=a.fallback;
                if let Some(feet)=grounded_clear(mods,fallback.translation,options) { fallback.translation=feet; }
                fallback
            })
        })
    };
    if target.is_none() && !forced {
        mods.detach_error=Some("Exit blocked: no standing clearance on the requested sides".into());
        return false;
    }
    let a=mods.attach.take().unwrap();
    if mods.camera.owner.as_deref()==Some(a.owner.as_str()) { mods.camera.clear(); }
    for (entity,visibility) in a.hidden {
        if let Some(mut current)=world.get_mut::<Visibility>(entity) { *current=visibility; }
    }
    mods.detach_error=None;
    if let Some(target)=target {
        let mut matrix=target.to_matrix().to_cols_array_2d();matrix[3][3]=0.;
        let mut skater=world.resource_mut::<crate::physics::SkaterRuntime>();
        if skater.player_input.pending_teleport().is_none() {
            let _=skater.player_input.request_teleport(matrix);
            skater.teleport_state.request_manual(matrix,false); // false = on FOOT, not board
            mods.detach_pending=Some((target,Instant::now()));
        }
    }
    sync(world,mods);
    true
}

pub(super) fn sync(world:&mut World,mods:&mut Mods) {
    if let Some(a)=&mods.attach {
        if !mods.bodies.contains_key(&(a.owner.clone(),a.body.clone())) {
            detach(world,mods,true,&DetachOptions::default());return;
        }
    }
    if let Some((target,since))=mods.detach_pending {
        let native=world.resource::<crate::physics::SkaterRuntime>();
        let p=native.animated_skeleton.roots.animation_to_world[3];
        if (Vec3::new(p[0],p[1],p[2]).distance_squared(target.translation)<0.04
            && native.player_input.physical.state.category_12==500)
            || since.elapsed().as_secs_f32()>2. {
            mods.detach_pending=None;
        }
    }
    let Some(target)=local_root(mods) else { return };
    let roots:Vec<_>=world.query_filtered::<Entity,With<crate::world::PlayerRoot>>().iter(world).collect();
    for entity in roots {
        if let Some(mut current)=world.get_mut::<Transform>(entity) { *current=target; }
    }
}
