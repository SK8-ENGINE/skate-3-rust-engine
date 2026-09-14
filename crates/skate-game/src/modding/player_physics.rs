//! Native skater contact observations and temporary, mod-owned joint overrides.
use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::physics::{assembly::BodySnapshot, board_step::CollisionBody,
    skeleton_body::{SkeletonJoints,SkeletonDriveBatch,SkeletonDriveIdentity}};
use skate_mods::extensions::JointOverride;
use crate::physics::{GamePhysics,SkaterRuntime};

pub(super) fn set(world:&mut World, owner:&str, joint:usize, options:Option<JointOverride>) -> Result<(),String> {
    let mut s=world.resource_mut::<SkaterRuntime>();
    if joint>=s.skeleton_joints.records.len() { return Err("unknown native joint".into()); }
    if s.mod_joint_overrides.get(&joint).is_some_and(|(o,_)| o!=owner) { return Err("joint override is owned by another mod".into()); }
    if let Some(options)=options { s.mod_joint_overrides.insert(joint,(owner.into(),options)); }
    else { s.mod_joint_overrides.remove(&joint); }
    Ok(())
}
pub(super) fn clear(world:&mut World,owner:Option<&str>) {
    if let Some(mut s)=world.get_resource_mut::<SkaterRuntime>() {
        s.mod_joint_overrides.retain(|_,(o,_)| owner.is_some_and(|owner| o!=owner));
    }
}
fn apply(joints:&mut SkeletonJoints, index:usize, options:&JointOverride) {
    let p=&mut joints.records[index].parameters.words;
    if let Some(v)=options.swing_limit {p[10]=v.to_bits();p[12]=skate_core::trigonometry::cos(v).to_bits();}
    if let Some(v)=options.twist_limit {p[11]=v.to_bits();p[13]=skate_core::trigonometry::cos(v).to_bits();}
    if let Some(free)=options.free_swing {p[14]=if free {4} else {1};}
    if let Some(free)=options.free_twist {p[15]=if free {2} else {1};}
}
pub(crate) fn joints(s:&SkaterRuntime) -> SkeletonJoints {
    let mut result=SkeletonJoints {records:s.skeleton_joints.records};
    for (&index,(_,options)) in &s.mod_joint_overrides {apply(&mut result,index,options);}
    result
}
pub(crate) fn filter_drives(s:&SkaterRuntime,batch:&mut SkeletonDriveBatch) {
    let disabled:Vec<_>=s.mod_joint_overrides.iter().filter(|(_,(_,o))| o.drive_enabled==Some(false))
        .map(|(&i,_)| s.skeleton_joints.records[i].child).collect();
    let rows=std::mem::take(&mut batch.rows);
    let ids=std::mem::take(&mut batch.identities);
    let spies=std::mem::take(&mut batch.spy);
    for ((row,id),spy) in rows.into_iter().zip(ids).zip(spies) {
        if matches!(id,SkeletonDriveIdentity::Bone{part,..} if disabled.contains(&part)) {continue;}
        batch.rows.push(row);batch.identities.push(id);batch.spy.push(spy);
    }
}
fn body(id:u32,p:&GamePhysics,s:&SkaterRuntime)->Option<BodySnapshot> {
    match CollisionBody::from_contact_id(id) {
        CollisionBody::Board(id)=>Some(p.board.bodies()[id.index()]),
        CollisionBody::Attached(i)=>s.skeleton.bodies().iter().chain(s.skeleton_drives.targets.bodies.iter()).chain(p.network_proxies.bodies.iter()).nth(i).copied(),
        CollisionBody::StaticWorld=>None,
    }
}
fn identity(id:u32)->Value {
    match CollisionBody::from_contact_id(id) {
        CollisionBody::Board(id)=>json!({"kind":"board","index":id.index()}),
        CollisionBody::Attached(i) if i<26=>json!({"kind":"skater","index":i}),
        CollisionBody::Attached(i)=>json!({"kind":"external","index":i}),
        CollisionBody::StaticWorld=>json!({"kind":"world"}),
    }
}
pub(super) fn snapshot(world:&World)->Value {
    let p=world.resource::<GamePhysics>();let s=world.resource::<SkaterRuntime>();
    let dt=p.settings.step.simulation.time_step;
    let mut contacts=Vec::new();
    for row in p.board.solved_contacts() {
        let mut storage=*row.words();let mut count=0;
        skate_core::physics::contact_feedback::spy_contact_jacobians(&mut storage,1,&mut count,p.settings.step.simulation.frequency,|id| {
            body(id,p,s).map_or([0;4],|b| [b.rates.position.x.to_bits(),b.rates.position.y.to_bits(),b.rates.position.z.to_bits(),0])
        });
        if count==0 {continue;}
        let v=|n:usize| -> [f32;3] {std::array::from_fn(|i|f32::from_bits(storage[n+i]))};
        let normal_force=v(16);let friction_force=v(20);
        let force:[f32;3]=std::array::from_fn(|i|normal_force[i]+friction_force[i]);
        if !v(12).iter().chain(force.iter()).all(|v|v.is_finite()) {continue;}
        contacts.push(json!({"a":identity(storage[24]),"b":identity(storage[25]),"point":v(12),"normal":v(0),
            "force":force,"normal_force":normal_force,"friction_force":friction_force,"impulse":force.map(|v|v*dt),
            "static_friction":row.static_friction(),"dynamic_friction":row.dynamic_friction(),"material_tags":[storage[26]&0xffff,storage[26]>>16]}));
        if contacts.len()==128 {break;}
    }
    let joints=joints(s).records.iter().enumerate().map(|(i,j)| {
        let w=&j.parameters.words;
        json!({"index":i,"name":crate::physics::skeleton_body::JOINT_NAMES[i],"parent":j.parent,"child":j.child,
            "swing_limit":f32::from_bits(w[10]),"twist_limit":f32::from_bits(w[11]),"free_swing":w[14]==4,"free_twist":w[15]==2,
            "drive_enabled":!s.mod_joint_overrides.get(&i).is_some_and(|(_,o)|o.drive_enabled==Some(false)),
            "override_owner":s.mod_joint_overrides.get(&i).map(|(o,_)|o)})
    }).collect::<Vec<_>>();
    let parts=s.skeleton.bodies().iter().enumerate().map(|(i,b)|json!({"index":i,"position":[b.rates.position.x,b.rates.position.y,b.rates.position.z],
        "velocity":[b.rates.linear_velocity.x,b.rates.linear_velocity.y,b.rates.linear_velocity.z],
        "angvel":[b.rates.angular_velocity.x,b.rates.angular_velocity.y,b.rates.angular_velocity.z],"inverse_mass":b.inertia.inverse_mass})).collect::<Vec<_>>();
    json!({"tick":p.ticks,"dt":dt,"contacts":contacts,"joints":joints,"parts":parts,"ragdoll":s.skeleton_collision.is_ragdoll,"partial_ragdoll":s.skeleton_collision.partial_ragdoll})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_rotation_preserves_linear_joint_and_native_defaults() {
        use skate_core::physics::{skeleton_body::SkeletonJoint,joint_records::{RetailJointParametersRaw,RetailJointFramesRaw}};
        let joint=SkeletonJoint {parent:0,child:1,parameters:RetailJointParametersRaw{words:[0;16]},frames:RetailJointFramesRaw{words:[0;20]}};
        let native=SkeletonJoints{records:[joint;22]};
        let mut modified=SkeletonJoints{records:native.records};
        apply(&mut modified,0,&JointOverride {swing_limit:Some(1.2),twist_limit:Some(0.5),free_swing:Some(true),free_twist:Some(true),drive_enabled:Some(false)});
        assert_eq!(&modified.records[0].parameters.words[..10],&native.records[0].parameters.words[..10]);
        assert_eq!(modified.records[0].parameters.words[14..],[4,2]);
        assert_eq!(f32::from_bits(modified.records[0].parameters.words[10]),1.2);
        assert_eq!(native.records[0].parameters.words,[0;16]);
        assert_eq!(modified.records[1].parameters.words,[0;16]);
    }
}
