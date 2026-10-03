//! Native skater contact observations and temporary, mod-owned joint overrides.
use crate::physics::{GamePhysics, SkaterRuntime};
use bevy::prelude::*;
use serde_json::{Value, json};
use skate_core::physics::{
    assembly::BodySnapshot,
    board_step::CollisionBody,
    skeleton_body::{SkeletonDriveBatch, SkeletonDriveIdentity, SkeletonJoints},
};
use skate_mods::extensions::JointOverride;

pub(super) fn set(
    world: &mut World,
    owner: &str,
    joint: usize,
    options: Option<JointOverride>,
) -> Result<(), String> {
    let mut s = world.resource_mut::<SkaterRuntime>();
    if joint >= s.skeleton_joints.records.len() {
        return Err("unknown native joint".into());
    }
    if s.mod_joint_overrides
        .get(&joint)
        .is_some_and(|(o, _)| o != owner)
    {
        return Err("joint override is owned by another mod".into());
    }
    if let Some(options) = options {
        s.mod_joint_overrides.insert(joint, (owner.into(), options));
    } else {
        s.mod_joint_overrides.remove(&joint);
    }
    Ok(())
}
pub(super) fn clear(world: &mut World, owner: Option<&str>) {
    if let Some(mut s) = world.get_resource_mut::<SkaterRuntime>() {
        s.mod_part_overrides
            .retain(|_, (o, _)| owner.is_some_and(|owner| o != owner));
        s.mod_joint_overrides
            .retain(|_, (o, _)| owner.is_some_and(|owner| o != owner));
        if owner.is_none() {
            s.mod_contact_frame = Default::default();
        }
    }
}
fn apply(joints: &mut SkeletonJoints, index: usize, options: &JointOverride) {
    let p = &mut joints.records[index].parameters.words;
    if let Some(v) = options.swing_limit {
        p[10] = v.to_bits();
        p[12] = skate_core::trigonometry::cos(v).to_bits();
    }
    if let Some(v) = options.twist_limit {
        p[11] = v.to_bits();
        p[13] = skate_core::trigonometry::cos(v).to_bits();
    }
    if let Some(free) = options.free_swing {
        p[14] = if free { 4 } else { 1 };
    }
    if let Some(free) = options.free_twist {
        p[15] = if free { 2 } else { 1 };
    }
}
pub(crate) fn joints(s: &SkaterRuntime) -> SkeletonJoints {
    let mut result = SkeletonJoints {
        records: s.skeleton_joints.records,
    };
    for (&index, (_, options)) in &s.mod_joint_overrides {
        apply(&mut result, index, options);
    }
    result
}
pub(crate) fn filter_drives(s: &SkaterRuntime, batch: &mut SkeletonDriveBatch) {
    let disabled = disabled_drives(s, false);
    let rows = std::mem::take(&mut batch.rows);
    let ids = std::mem::take(&mut batch.identities);
    let spies = std::mem::take(&mut batch.spy);
    for ((row, id), spy) in rows.into_iter().zip(ids).zip(spies) {
        if matches!(id,SkeletonDriveIdentity::Bone{part,..} if disabled.contains(&part)) {
            continue;
        }
        batch.rows.push(row);
        batch.identities.push(id);
        batch.spy.push(spy);
    }
}
fn body(id: u32, p: &GamePhysics, s: &SkaterRuntime) -> Option<BodySnapshot> {
    match CollisionBody::from_contact_id(id) {
        CollisionBody::Board(id) => Some(p.board.bodies()[id.index()]),
        CollisionBody::Attached(i) => s
            .skeleton
            .bodies()
            .iter()
            .chain(s.skeleton_drives.targets.bodies.iter())
            .chain(p.network_proxies.bodies.iter())
            .nth(i)
            .copied(),
        CollisionBody::StaticWorld => None,
    }
}
fn identity(id: u32, p: &GamePhysics) -> Value {
    match CollisionBody::from_contact_id(id) {
        CollisionBody::Board(id) => json!({"kind":"board","index":id.index()}),
        CollisionBody::Attached(i) if i < 26 => json!({"kind":"skater","index":i}),
        CollisionBody::Attached(i) => {
            if let Some((peer, part)) = p.network_proxies.actors.get(&i) {
                json!({"kind":"remote","index":i,"peer":peer.to_string(),"part_kind":if *part<7 {"board"}else{"skater"},"part":if *part<7 {*part}else{*part-7}})
            } else {
                json!({"kind":"external","index":i,"solid_id":p.network_proxies.solids.iter().find(|(index,_)|*index==i).map(|(_,b)|b.id.to_string())})
            }
        }
        CollisionBody::StaticWorld => json!({"kind":"world"}),
    }
}
pub(super) fn snapshot(world: &World, mods: &super::Mods) -> Value {
    let p = world.resource::<GamePhysics>();
    let s = world.resource::<SkaterRuntime>();
    let dt = p.settings.step.simulation.time_step;
    let mut contacts = s.mod_contact_frame.contacts.clone();
    for contact in &mut contacts {
        for side in ["a", "b"] {
            if let Some(solid_id) = contact[side]["solid_id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
            {
                {
                    if let Some(((owner, key), _)) =
                        mods.bodies.iter().find(|(_, id)| **id == solid_id)
                    {
                        contact[side]["owner"] = json!(owner);
                        contact[side]["key"] = json!(key);
                    }
                }
            }
        }
    }
    let animation_disabled = disabled_drives(s, false);
    let possession_disabled = disabled_drives(s, true);
    let joints=joints(s).records.iter().enumerate().map(|(i,j)| {
        let w=&j.parameters.words;
        json!({"index":i,"name":crate::physics::skeleton_body::JOINT_NAMES[i],"parent":j.parent,"child":j.child,
            "swing_limit":f32::from_bits(w[10]),"twist_limit":f32::from_bits(w[11]),"free_swing":w[14]==4,"free_twist":w[15]==2,
            "enabled":!s.mod_joint_overrides.get(&i).is_some_and(|(_,o)|o.enabled==Some(false)),
            "drive_enabled":!animation_disabled.contains(&j.child),
            "possession_enabled":!possession_disabled.contains(&j.child),
            "load":s.mod_contact_frame.joint_loads.get(&i),
            "parameters":w,"frames":j.frames.words.to_vec(),
            "override_owner":s.mod_joint_overrides.get(&i).map(|(o,_)|o)})
    }).collect::<Vec<_>>();
    let parts=s.skeleton.bodies().iter().enumerate().map(|(i,b)| {
        let mut out=body_value("skater",i,b);
        let joint=s.skeleton_joints.records.iter().enumerate().find(|(_,j)|j.child==i);
        out["joint"]=json!(joint.map(|(i,_)|i));
        out["name"]=json!(part_name(i,s));
        out["override"]=json!(s.mod_part_overrides.get(&i).map(|(owner,options)|json!({"owner":owner,"options":options})));
        let options=s.mod_part_overrides.get(&i).map(|(_,o)|o);
        let collision=&s.skeleton_collision.parts[i];
        let friction=options.and_then(|o|o.friction);
        out["state_flags"]=json!(options.and_then(|o|o.motion.as_deref()).map_or(b.state_flags,motion_flags));
        out["collision_enabled"]=json!(options.and_then(|o|o.collision).unwrap_or(collision.enabled));
        out["animation_drives"]=json!(!animation_disabled.contains(&i));
        out["possession_drives"]=json!(!possession_disabled.contains(&i));
        out["material"]=json!({"static_friction":friction.unwrap_or(collision.material.static_friction),"dynamic_friction":friction.unwrap_or(collision.material.dynamic_friction)});
        out
    }).collect::<Vec<_>>();
    let board = p
        .board
        .bodies()
        .iter()
        .enumerate()
        .map(|(i, b)| body_value("board", i, b))
        .collect::<Vec<_>>();
    json!({"tick":s.mod_contact_frame.tick,"dt":dt,"contacts":contacts,"contacts_truncated":s.mod_contact_frame.truncated,"joints":joints,"parts":parts,"board":board,
        "ragdoll":s.skeleton_collision.is_ragdoll,"partial_ragdoll":s.skeleton_collision.partial_ragdoll})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_rotation_preserves_linear_joint_and_native_defaults() {
        use skate_core::physics::{
            joint_records::{RetailJointFramesRaw, RetailJointParametersRaw},
            skeleton_body::SkeletonJoint,
        };
        let joint = SkeletonJoint {
            parent: 0,
            child: 1,
            parameters: RetailJointParametersRaw { words: [0; 16] },
            frames: RetailJointFramesRaw { words: [0; 20] },
        };
        let native = SkeletonJoints {
            records: [joint; 22],
        };
        let mut modified = SkeletonJoints {
            records: native.records,
        };
        apply(
            &mut modified,
            0,
            &JointOverride {
                swing_limit: Some(1.2),
                twist_limit: Some(0.5),
                free_swing: Some(true),
                free_twist: Some(true),
                drive_enabled: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(
            &modified.records[0].parameters.words[..10],
            &native.records[0].parameters.words[..10]
        );
        assert_eq!(modified.records[0].parameters.words[14..], [4, 2]);
        assert_eq!(
            f32::from_bits(modified.records[0].parameters.words[10]),
            1.2
        );
        assert_eq!(native.records[0].parameters.words, [0; 16]);
        assert_eq!(modified.records[1].parameters.words, [0; 16]);
    }
}

fn selected_parts(
    s: &SkaterRuntime,
    predicate: impl Fn(&JointOverride) -> bool,
) -> std::collections::BTreeSet<usize> {
    let mut selected = std::collections::BTreeSet::new();
    for (&i, (_, o)) in &s.mod_joint_overrides {
        if !predicate(o) {
            continue;
        }
        let mut branch = std::collections::BTreeSet::from([s.skeleton_joints.records[i].child]);
        if o.descendants {
            for _ in 0..26 {
                for j in &s.skeleton_joints.records {
                    if branch.contains(&j.parent) {
                        branch.insert(j.child);
                    }
                }
            }
        }
        selected.extend(branch);
    }
    selected
}
fn disabled_drives(s: &SkaterRuntime, possession: bool) -> std::collections::BTreeSet<usize> {
    let mut disabled = selected_parts(s, |o| {
        if possession {
            o.possession_enabled == Some(false)
        } else {
            o.drive_enabled == Some(false)
        }
    });
    disabled.extend(
        s.mod_part_overrides
            .iter()
            .filter(|(_, (_, o))| {
                if possession {
                    o.possession_drives == Some(false)
                } else {
                    o.animation_drives == Some(false)
                }
            })
            .map(|(&i, _)| i),
    );
    disabled
}
pub(crate) fn filter_possession(
    s: &SkaterRuntime,
    rows: &mut Vec<skate_core::physics::drive_solver::RetailDriveRows>,
    start: usize,
) {
    let disabled = disabled_drives(s, true);
    let mut index = 0;
    rows.retain(|row| {
        let keep = index < start
            || ![
                row.frame_a_body.reaction_index,
                row.frame_b_body.reaction_index,
            ]
            .iter()
            .any(|r| {
                r.checked_sub(skate_core::physics::board_step::ATTACHED_REACTION_BASE)
                    .is_some_and(|i| disabled.contains(&i))
            });
        index += 1;
        keep
    });
}
pub(crate) fn filter_joints(
    s: &SkaterRuntime,
    rows: &mut Vec<skate_core::physics::solver::JointConstraint>,
) {
    let disabled: std::collections::BTreeSet<_> = s
        .mod_joint_overrides
        .iter()
        .filter(|(_, (_, o))| o.enabled == Some(false))
        .map(|(&i, _)| {
            s.skeleton_joints.records[i].child
                + skate_core::physics::board_step::ATTACHED_REACTION_BASE
        })
        .collect();
    rows.retain(|r| !disabled.contains(&r.reaction_a));
}
fn part_name(i: usize, s: &SkaterRuntime) -> String {
    s.animated_skeleton
        .bone_indices
        .get(i)
        .and_then(|i| s.animation.evaluator.frames.bone_names.get(*i))
        .cloned()
        .unwrap_or_else(|| format!("PART_{i}"))
}

fn body_value(kind: &str, index: usize, b: &BodySnapshot) -> Value {
    let q = b.rates.orientation;
    json!({"kind":kind,"index":index,"position":arr(b.rates.position),"rotation":[q.x,q.y,q.z,q.w],
        "velocity":arr(b.rates.linear_velocity),"angvel":arr(b.rates.angular_velocity),
        "inverse_mass":b.inertia.inverse_mass,"inverse_inertia":b.rates.world_inverse_inertia.columns,"state_flags":b.state_flags})
}
fn arr(v: skate_core::math::Vector3) -> [f32; 3] {
    [v.x, v.y, v.z]
}
fn vec(v: [f32; 3]) -> skate_core::math::Vector3 {
    skate_core::math::Vector3::new(v[0], v[1], v[2])
}
fn point_velocity(b: Option<&BodySnapshot>, p: [f32; 3]) -> Vec3 {
    b.map_or(Vec3::ZERO, |b| {
        Vec3::from_array(arr(b.rates.linear_velocity))
            + Vec3::from_array(arr(b.rates.angular_velocity))
                .cross(Vec3::from_array(p) - Vec3::from_array(arr(b.rates.position)))
    })
}
pub(super) fn impulse(
    world: &mut World,
    reference: &skate_mods::extensions::NativeBodyRef,
    impulse: [f32; 3],
    point: Option<[f32; 3]>,
    angular: bool,
) -> Result<(), String> {
    fn apply(
        b: &mut BodySnapshot,
        j: [f32; 3],
        point: Option<[f32; 3]>,
        angular: bool,
    ) -> Result<(), String> {
        if b.inertia.inverse_mass <= 0. {
            return Err("native body has no dynamic mass".into());
        }
        let impulse = Vec3::from_array(j);
        let torque = if angular {
            impulse
        } else {
            point.map_or(Vec3::ZERO, |p| {
                (Vec3::from_array(p) - Vec3::from_array(arr(b.rates.position))).cross(impulse)
            })
        };
        let basis = b.rates.world_inverse_inertia.columns;
        let delta = Vec3::from_array(basis[0]) * torque.x
            + Vec3::from_array(basis[1]) * torque.y
            + Vec3::from_array(basis[2]) * torque.z;
        if !angular {
            b.rates.linear_velocity = vec((Vec3::from_array(arr(b.rates.linear_velocity))
                + impulse * b.inertia.inverse_mass)
                .to_array());
        }
        b.rates.angular_velocity =
            vec((Vec3::from_array(arr(b.rates.angular_velocity)) + delta).to_array());
        Ok(())
    }
    if reference.kind == "board" {
        let mut p = world.resource_mut::<GamePhysics>();
        apply(
            &mut p.board.bodies_mut()[reference.index],
            impulse,
            point,
            angular,
        )
    } else {
        let mut s = world.resource_mut::<SkaterRuntime>();
        apply(
            &mut s.skeleton.bodies_mut()[reference.index],
            impulse,
            point,
            angular,
        )
    }
}
#[derive(Default)]
pub(crate) struct ContactFrame {
    pub tick: u64,
    pub contacts: Vec<Value>,
    pub truncated: bool,
    pub joint_loads: std::collections::BTreeMap<usize, Value>,
    pairs: std::collections::BTreeMap<String, Value>,
}
pub(crate) fn before_solve(
    p: &GamePhysics,
    s: &SkaterRuntime,
) -> std::collections::BTreeMap<u32, BodySnapshot> {
    let mut out = std::collections::BTreeMap::new();
    for (i, b) in p.board.bodies().iter().enumerate() {
        out.insert(i as u32, *b);
    }
    for (i, b) in s
        .skeleton
        .bodies()
        .iter()
        .chain(s.skeleton_drives.targets.bodies.iter())
        .chain(p.network_proxies.bodies.iter())
        .enumerate()
    {
        out.insert(CollisionBody::Attached(i).contact_id(), *b);
    }
    out
}
pub(crate) fn after_solve(
    p: &GamePhysics,
    s: &SkaterRuntime,
    before: &std::collections::BTreeMap<u32, BodySnapshot>,
) -> ContactFrame {
    let dt = p.settings.step.simulation.time_step;
    let mut frame = ContactFrame {
        tick: p.ticks,
        ..Default::default()
    };
    for row in p.board.solved_contacts() {
        let mut storage = *row.words();
        let mut count = 0;
        skate_core::physics::contact_feedback::spy_contact_jacobians(
            &mut storage,
            1,
            &mut count,
            p.settings.step.simulation.frequency,
            |id| {
                body(id, p, s).map_or([0; 4], |b| {
                    [
                        b.rates.position.x.to_bits(),
                        b.rates.position.y.to_bits(),
                        b.rates.position.z.to_bits(),
                        0,
                    ]
                })
            },
        );
        if count == 0 {
            continue;
        }
        let v = |n: usize| -> [f32; 3] { std::array::from_fn(|i| f32::from_bits(storage[n + i])) };
        let a = storage[24];
        let b = storage[25];
        let point = v(12);
        let normal = v(0);
        let normal_force = v(16);
        let friction_force = v(20);
        let force: [f32; 3] = std::array::from_fn(|i| normal_force[i] + friction_force[i]);
        let relative =
            point_velocity(before.get(&a), point) - point_velocity(before.get(&b), point);
        if !point.iter().chain(force.iter()).all(|v| v.is_finite()) || !relative.is_finite() {
            continue;
        }
        // Stable body-pair identity. All manifold points share this lifecycle.
        let stable = |id| {
            let b = identity(id, p);
            if b["kind"] == "remote" {
                format!("peer:{}:{}:{}", b["peer"], b["part_kind"], b["part"])
            } else if let Some(solid) = b["solid_id"].as_str() {
                format!("solid:{solid}")
            } else {
                format!("native:{id}")
            }
        };
        let mut keys = [stable(a), stable(b)];
        keys.sort();
        let key = keys.join("/");
        let phase = if s.mod_contact_frame.pairs.contains_key(&key) {
            "stay"
        } else {
            "begin"
        };
        let value = json!({"id":key,"phase":phase,"a":identity(a,p),"b":identity(b,p),"point":point,"normal":normal,
            "relative_velocity_before_solve":relative.to_array(),"closing_speed":(-relative.dot(Vec3::from_array(normal))).max(0.),
            "force":force,"normal_force":normal_force,"friction_force":friction_force,"impulse":force.map(|v|v*dt),
            "static_friction":row.static_friction(),"dynamic_friction":row.dynamic_friction(),"material_tags":[storage[26]&0xffff,storage[26]>>16]});
        frame.pairs.insert(key, value.clone());
        if frame.contacts.len() < 128 {
            frame.contacts.push(value);
        } else {
            frame.truncated = true;
        }
    }
    for (key, value) in &s.mod_contact_frame.pairs {
        if !frame.pairs.contains_key(key) {
            let mut v = value.clone();
            v["phase"] = json!("end");
            v["force"] = json!([0, 0, 0]);
            v["normal_force"] = json!([0, 0, 0]);
            v["friction_force"] = json!([0, 0, 0]);
            v["impulse"] = json!([0, 0, 0]);
            v["closing_speed"] = json!(0);
            v["relative_velocity_before_solve"] = json!([0, 0, 0]);
            if frame.contacts.len() < 256 {
                frame.contacts.push(v);
            } else {
                frame.truncated = true;
            }
        }
    }
    frame
}

pub(crate) fn joint_loads(
    s: &SkaterRuntime,
    rows: &[skate_core::physics::solver::JointConstraint],
    dt: f32,
) -> std::collections::BTreeMap<usize, Value> {
    let mut out = std::collections::BTreeMap::new();
    for row in rows {
        let Some(child) = row
            .reaction_a
            .checked_sub(skate_core::physics::board_step::ATTACHED_REACTION_BASE)
        else {
            continue;
        };
        let Some(index) = s
            .skeleton_joints
            .records
            .iter()
            .position(|j| j.child == child)
        else {
            continue;
        };
        let w = &row.jacobian.words;
        // Joint solve accumulators at bytes32/48 and projection axes192/240.
        // They produce displacement corrections; divide by dt for impulse.
        let project = |acc: usize, axes: usize| -> Vec3 {
            (0..3).fold(Vec3::ZERO, |v, i| {
                v + Vec3::new(
                    f32::from_bits(w[axes + i * 4]),
                    f32::from_bits(w[axes + i * 4 + 1]),
                    f32::from_bits(w[axes + i * 4 + 2]),
                ) * f32::from_bits(w[acc + i])
            })
        };
        let linear = project(8, 48) / dt;
        let angular = project(12, 60) / dt;
        if linear.is_finite() && angular.is_finite() {
            out.insert(index,json!({"linear_impulse":linear.to_array(),"angular_impulse":angular.to_array(),"force":(linear/dt).to_array(),"torque":(angular/dt).to_array(),"solver_words":w.to_vec()}));
        }
    }
    out
}

pub(super) fn set_part(
    world: &mut World,
    owner: &str,
    index: usize,
    options: Option<skate_mods::extensions::PartOverride>,
) -> Result<(), String> {
    let mut s = world.resource_mut::<SkaterRuntime>();
    if index >= s.skeleton.bodies().len() {
        return Err("unknown physical part".into());
    }
    if s.mod_part_overrides
        .get(&index)
        .is_some_and(|(o, _)| o != owner)
    {
        return Err("part is owned by another mod".into());
    }
    if let Some(options) = options {
        s.mod_part_overrides.insert(index, (owner.into(), options));
    } else {
        s.mod_part_overrides.remove(&index);
    }
    Ok(())
}
pub(crate) struct PartRestore(
    Vec<(
        usize,
        u32,
        skate_core::physics::skeleton_body::SkeletonPartCollision,
    )>,
);
pub(crate) fn apply_parts(s: &mut SkaterRuntime) -> PartRestore {
    let mut restore = Vec::new();
    for (&i, (_, o)) in &s.mod_part_overrides {
        restore.push((
            i,
            s.skeleton.bodies()[i].state_flags,
            s.skeleton_collision.parts[i],
        ));
        if let Some(motion) = &o.motion {
            s.skeleton.bodies_mut()[i].state_flags = motion_flags(motion);
        }
        let c = &mut s.skeleton_collision.parts[i];
        if let Some(enabled) = o.collision {
            c.enabled = enabled;
        }
        if let Some(friction) = o.friction {
            c.material.static_friction = friction;
            c.material.dynamic_friction = friction;
        }
    }
    PartRestore(restore)
}
impl PartRestore {
    pub(crate) fn restore(self, s: &mut SkaterRuntime) {
        for (i, flags, collision) in self.0 {
            s.skeleton.bodies_mut()[i].state_flags = flags;
            s.skeleton_collision.parts[i] = collision;
        }
    }
}

fn motion_flags(motion: &str) -> u32 {
    match motion {
        "dynamic" => 4,
        "frozen" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    #[ignore = "requires SKATE3_ASSET_ROOT; headless native rig integration"]
    fn authored_rig_overrides_release_drives_preserve_anchors_and_restore() {
        let root =
            std::path::PathBuf::from(std::env::var_os("SKATE3_ASSET_ROOT").expect("asset root"));
        let assets = skate_data::GameAssets::load(&root).unwrap();
        let graphs = crate::graph_runtime::StockGraphs::load(&root, &assets).unwrap();
        let physics = GamePhysics::load(&root).unwrap();
        let mut s = SkaterRuntime::load(&root, &graphs, &physics, "normal").unwrap();
        let index = crate::physics::skeleton_body::JOINT_NAMES
            .iter()
            .position(|n| n.contains("LEFT") && n.contains("FOREARM"))
            .expect("native arm joint");
        let original = s.skeleton_joints.records[index];
        s.mod_joint_overrides.insert(
            index,
            (
                "test".into(),
                JointOverride {
                    free_swing: Some(true),
                    free_twist: Some(true),
                    drive_enabled: Some(false),
                    possession_enabled: Some(false),
                    descendants: true,
                    ..Default::default()
                },
            ),
        );
        let changed = joints(&s);
        assert_eq!(changed.records[index].frames.words, original.frames.words);
        assert_eq!(
            &changed.records[index].parameters.words[..10],
            &original.parameters.words[..10]
        );
        let disabled = disabled_drives(&s, false);
        assert!(disabled.contains(&original.child));
        assert!(!disabled.contains(&original.parent));
        assert_eq!(disabled, disabled_drives(&s, true));
        let dt = physics.settings.step.simulation.time_step;
        let mut batch = s.skeleton_drives.build(
            s.skeleton.bodies(),
            skate_core::physics::board_step::ATTACHED_REACTION_BASE,
            skate_core::physics::board_step::ATTACHED_REACTION_BASE + 26,
            dt,
        );
        let expected = batch
            .identities
            .iter()
            .filter(
                |id| !matches!(id,SkeletonDriveIdentity::Bone{part,..} if disabled.contains(part)),
            )
            .count();
        filter_drives(&s, &mut batch);
        assert_eq!(batch.rows.len(), expected);
        assert_eq!(batch.rows.len(), batch.identities.len());
        assert_eq!(batch.rows.len(), batch.spy.len());
        let part = original.child;
        let flags = s.skeleton.bodies()[part].state_flags;
        let collision = s.skeleton_collision.parts[part];
        s.mod_part_overrides.insert(
            part,
            (
                "test".into(),
                skate_mods::extensions::PartOverride {
                    motion: Some("frozen".into()),
                    collision: Some(false),
                    friction: Some(0.1),
                    ..Default::default()
                },
            ),
        );
        let restore = apply_parts(&mut s);
        assert_eq!(s.skeleton.bodies()[part].state_flags, 2);
        assert!(!s.skeleton_collision.parts[part].enabled);
        assert_eq!(
            s.skeleton_collision.parts[part].material.dynamic_friction,
            0.1
        );
        restore.restore(&mut s);
        assert_eq!(s.skeleton.bodies()[part].state_flags, flags);
        assert_eq!(s.skeleton_collision.parts[part].enabled, collision.enabled);
        assert_eq!(
            s.skeleton_collision.parts[part].material.dynamic_friction,
            collision.material.dynamic_friction
        );
        let mut body = s.skeleton.bodies()[part];
        body.rates.position = vec([0., 0., 0.]);
        body.rates.linear_velocity = vec([1., 0., 0.]);
        body.rates.angular_velocity = vec([0., 0., 2.]);
        assert_eq!(
            point_velocity(Some(&body), [0., 1., 0.]),
            Vec3::new(-1., 0., 0.)
        );
        // Check exposed joint impulses against the actual native solver reactions.
        let base = skate_core::physics::board_step::ATTACHED_REACTION_BASE;
        let mut probe = *s.skeleton.bodies();
        probe[part].rates.linear_velocity = vec([5., -3., 2.]);
        let mut rows = s.skeleton_joints.build(&probe, base, dt);
        rows.retain(|r| r.reaction_a == base + part);
        assert_eq!(rows.len(), 1);
        let mut reactions =
            vec![skate_core::physics::rigid_body::RetailReactionCorrections::default(); base + 26];
        skate_core::physics::solver::solve_constraints(
            &mut [],
            &mut rows,
            &mut [],
            &mut reactions,
            8,
        );
        let loads = joint_loads(&s, &rows, dt);
        let load = &loads[&index];
        let reported: [f32; 3] = serde_json::from_value(load["linear_impulse"].clone()).unwrap();
        let expected = Vec3::from_array(arr(reactions[base + part].linear_displacement)) / dt;
        assert!(expected.length() > 0.001);
        assert!(
            (Vec3::from_array(reported) * probe[part].inertia.inverse_mass - expected).length()
                < 0.001
        );

        let initial = s.skeleton.bodies()[part].rates.linear_velocity;
        let inverse_mass = s.skeleton.bodies()[part].inertia.inverse_mass;
        let mut world = World::new();
        world.insert_resource(s);
        world.insert_resource(physics);
        impulse(
            &mut world,
            &skate_mods::extensions::NativeBodyRef {
                kind: "skater".into(),
                index: part,
            },
            [2., 0., 0.],
            None,
            false,
        )
        .unwrap();
        assert!(
            (world.resource::<SkaterRuntime>().skeleton.bodies()[part]
                .rates
                .linear_velocity
                .x
                - initial.x
                - 2. * inverse_mass)
                .abs()
                < 0.0001
        );
        assert!(set(&mut world, "other", index, None).is_err());
        assert!(set_part(&mut world, "other", part, None).is_err());
        clear(&mut world, Some("other"));
        assert_eq!(
            world.resource::<SkaterRuntime>().mod_joint_overrides.len(),
            1
        );
        clear(&mut world, Some("test"));
        assert!(
            world
                .resource::<SkaterRuntime>()
                .mod_joint_overrides
                .is_empty()
        );
        assert!(
            world
                .resource::<SkaterRuntime>()
                .mod_part_overrides
                .is_empty()
        );
    }
}

/// Effective volume properties outside the temporary solver override scope.
pub(crate) fn collision_parts(
    s: &SkaterRuntime,
) -> [skate_core::physics::skeleton_body::SkeletonPartCollision; 26] {
    let mut parts = s.skeleton_collision.parts;
    for (&i, (_, o)) in &s.mod_part_overrides {
        if let Some(enabled) = o.collision {
            parts[i].enabled = enabled;
        }
        if let Some(friction) = o.friction {
            parts[i].material.static_friction = friction;
            parts[i].material.dynamic_friction = friction;
        }
    }
    parts
}
