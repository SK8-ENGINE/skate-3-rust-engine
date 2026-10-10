//! Ped hand props (b87, b89): the `livingworld_handprops` records (offset, carry channel) and their models
//! (`living_world/hand_props.json` + `hand_props/<key>.glb`, setup `tools/asset_pipeline/hand_props.py` from the
//! retail DMO templates), and the held object on the ped.
//!
//! Retail: SpawnInteractionBasedHandProp requests the record (`82E3DDA0`, `brain+3279` 0x01); the ped update
//! creates the DMO (`82E3DE18`) and attaches it (`82E3EC60`: `ped+5920`, `brain+3278` 0x02); every frame
//! `82E3E4F0` / `82E3E1B0` put it at the hand matrix composed with the record's local offset [code].
//! NOT RETAIL YET: the hand bone is RIGHTHANDPROP (rig 26) [inferred: every carry channel is `*RH`; which bone fills
//! the hand matrix at skeleton +19504 is not decoded]; the record fields `Hash_3FE1...` = Euler rotation in degrees
//! and `Hash_DC20...` = translation in metres, applied translation then X, Y, Z [inferred from the value shapes; the
//! record loader is not read].
//!
//! Release (b90, b91): the brain's throw / drop (`skate_core::living_world::peds::hand_prop`) releases the object here:
//! it becomes a prop body created mid-game (`GamePhysics::spawn_runtime_prop`, doc 27 "Props created mid-game") with
//! the release velocity, its model follows the body, and the ped keeps the link until the prop leaves the unlink box.
//! The ped never removes it (b90 §3); the dynamic-object pool (49) and the census cull (100 m) do.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::prelude::*;
use serde_json::Value;

/// The rig bone a hand prop follows.
pub(crate) const HAND_PROP_BONE: &str = "RIGHTHANDPROP";

const ROTATION_FIELD: &str = "Hash_3FE10F7B1115B8E6";
const OFFSET_FIELD: &str = "Hash_DC20CCEAB4B92992";
const CHANNEL_FIELD: &str = "Hash_FC1D2C4E5CCA6AED";
/// IsDisposable (record +60), CanSitWithHandProp (+61), CanAttackThrowHandProp (+62) [b90: names from the hashes].
const DISPOSABLE_FIELD: &str = "Hash_D02B4381CF62E28A";
const CAN_SIT_FIELD: &str = "Hash_BCE5969F18AEA914";
const CAN_ATTACK_THROW_FIELD: &str = "Hash_40802A8929B6B25E";

/// One hand prop record.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HandPropRecord {
    /// The model under the asset root (`private/living_world/hand_props/<key>.glb`); `None` when setup found no
    /// template for it (stock: `orange`).
    pub glb: Option<String>,
    pub rotation_degrees: Vec3,
    pub offset: Vec3,
    pub carry_channel: Option<String>,
    pub disposable: bool,
    pub can_sit: bool,
    pub can_attack_throw: bool,
}

impl HandPropRecord {
    /// The prop's frame in the hand bone's frame.
    pub fn local(&self) -> Mat4 {
        let r = self.rotation_degrees * (std::f32::consts::PI / 180.0);
        Mat4::from_translation(self.offset) * Mat4::from_quat(Quat::from_euler(EulerRot::XYZ, r.x, r.y, r.z))
    }
}

/// Every hand prop by key (`livingworld_handprops`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HandPropData {
    pub props: BTreeMap<String, HandPropRecord>,
}

fn flag(fields: Option<&Value>, key: &str) -> bool {
    fields.and_then(|f| f.get(key)).and_then(Value::as_bool).unwrap_or(false)
}

fn vec3(v: Option<&Value>) -> Vec3 {
    let f = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Vec3::new(f("x"), f("y"), f("z"))
}

impl HandPropData {
    /// From `private/living_world/tables.json` (inheritance already merged by the exporter) and `hand_props.json`.
    pub fn load(root: &Path) -> Result<Self, String> {
        let read = |p: &str| -> Result<Value, String> { serde_json::from_slice(&std::fs::read(root.join(p)).map_err(|e| format!("{p}: {e}"))?).map_err(|e| format!("{p}: {e}")) };
        let tables = read("private/living_world/tables.json")?;
        let records = tables.pointer("/classes/livingworld_handprops").and_then(Value::as_object).ok_or("tables.json has no livingworld_handprops")?;
        let models = read("private/living_world/hand_props.json").ok();
        let mut props = BTreeMap::new();
        for (key, record) in records {
            let fields = record.get("fields");
            let file = models.as_ref().and_then(|m| m.pointer(&format!("/props/{key}/file"))).and_then(Value::as_str);
            props.insert(
                key.clone(),
                HandPropRecord {
                    glb: file.map(|f| format!("private/living_world/{f}")),
                    rotation_degrees: vec3(fields.and_then(|f| f.get(ROTATION_FIELD))),
                    offset: vec3(fields.and_then(|f| f.get(OFFSET_FIELD))),
                    carry_channel: fields.and_then(|f| f.get(CHANNEL_FIELD)).and_then(Value::as_str).map(str::to_string),
                    disposable: flag(fields, DISPOSABLE_FIELD),
                    can_sit: flag(fields, CAN_SIT_FIELD),
                    can_attack_throw: flag(fields, CAN_ATTACK_THROW_FIELD),
                },
            );
        }
        Ok(Self { props })
    }
}

/// The object a ped holds (the created DMO, `ped+5920`).
#[derive(Component, Clone, Debug)]
pub(crate) struct HeldHandProp {
    pub key: String,
    pub entity: Entity,
    pub local: Mat4,
}

/// A released hand prop still linked to its ped (`ped+5920` after the release; unlinked by `82E3FAE0`).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ReleasedHandProp {
    pub body: u32,
}

/// Live released hand props in creation order: body id and model entity (the dynamic-object manager's share).
#[derive(Resource, Default)]
pub(crate) struct ReleasedHandProps(pub Vec<(u32, Entity)>);

/// Create the requested object (then `holding`, `82E3EC60`; the request bit clears whether or not the create worked,
/// `82E262D0`), release it on the brain's throw / drop, unlink it, cull released props, and remove a held object the
/// brain no longer has.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_hand_props(
    mut commands: Commands,
    server: Res<AssetServer>,
    data: Res<super::peds::PedData>,
    state: Res<super::PopulationState>,
    settings: Res<super::LivingWorldSettings>,
    observers: Res<super::LivingWorldObservers>,
    mut released: ResMut<ReleasedHandProps>,
    mut physics: Option<ResMut<crate::physics::GamePhysics>>,
    meshes: Res<Assets<Mesh>>,
    globals: Query<&GlobalTransform>,
    children: Query<&Children>,
    mesh_parts: Query<(&Mesh3d, &GlobalTransform)>,
    mut peds: Query<(Entity, &super::peds::Pedestrian, &super::peds::PedBody, &mut super::peds::PedMind, Option<&super::peds::PedPuppet>, Option<&HeldHandProp>, Option<&ReleasedHandProp>)>,
) {
    let tick = state.world.tick();
    let s = settings.ped_brain.values.hand_prop;
    for (e, ped, body, mut mind, puppet, held, linked) in &mut peds {
        let mind = &mut *mind;
        if let Some(h) = held {
            let at = globals.get(h.entity).ok().map(|g| g.translation().to_array());
            let thrown = at.and_then(|at| mind.brain.update_hand_prop_release(&s, at));
            if let Some(velocity) = thrown.or(mind.hand_prop_release.take()) {
                commands.entity(e).remove::<HeldHandProp>();
                let room = released.0.len() < s.max_live;
                let id = physics.as_deref_mut().and_then(|p| release(&mut commands, p, h, velocity, &globals, &children, &mesh_parts, &meshes, room));
                match id {
                    Some(id) => {
                        released.0.push((id, h.entity));
                        commands.entity(e).insert(ReleasedHandProp { body: id });
                        info!("PED_HAND_PROP ped=#{} {} released body={id} velocity=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, h.key, velocity[0], velocity[1], velocity[2]);
                    }
                    None => {
                        commands.entity(h.entity).despawn();
                        mind.brain.hand_prop.clear();
                        info!("PED_HAND_PROP ped=#{} {} released without a body tick={tick}", ped.id.serial, h.key);
                    }
                }
                continue;
            }
            let want = mind.brain.hand_prop.has().then(|| mind.brain.hand_prop.key.clone()).flatten();
            if want.as_deref() != Some(h.key.as_str()) {
                commands.entity(h.entity).despawn();
                commands.entity(e).remove::<HeldHandProp>();
                mind.brain.hand_prop.holding = false;
                info!("PED_HAND_PROP ped=#{} {} removed tick={tick}", ped.id.serial, h.key);
            }
            continue;
        }
        mind.hand_prop_release = None;
        // The link (`82E3F090`): unlinked once the released prop leaves the box around the ped, or when it is gone.
        if let Some(r) = linked {
            let at = physics.as_deref().and_then(|p| p.prop_dynamics()).and_then(|d| d.position_of(r.body));
            let out = match at {
                Some(at) => mind.brain.update_hand_prop_link(&s, [at.x, at.y, at.z], body.position.to_array()),
                None => {
                    mind.brain.hand_prop.clear();
                    true
                }
            };
            if out {
                commands.entity(e).remove::<ReleasedHandProp>();
                info!("PED_HAND_PROP ped=#{} unlinked body={} tick={tick}", ped.id.serial, r.body);
            }
            continue;
        }
        let want = mind.brain.hand_prop.requested.then(|| mind.brain.hand_prop.key.clone()).flatten();
        let (Some(key), Some(scene)) = (want, puppet.and_then(|p| p.scene)) else { continue };
        mind.brain.hand_prop.requested = false;
        let record = data.hand_props.props.get(&key);
        let Some(glb) = record.and_then(|r| r.glb.clone()) else {
            info!("PED_HAND_PROP ped=#{} {key} not created (no model) tick={tick}", ped.id.serial);
            continue;
        };
        if released.0.len() >= s.max_live {
            info!("PED_HAND_PROP ped=#{} {key} not created (pool full) tick={tick}", ped.id.serial);
            continue;
        }
        let entity = commands.spawn((SceneRoot(server.load(GltfAssetLabel::Scene(0).from_asset(glb))), Transform::default(), Visibility::Inherited, ChildOf(scene))).id();
        commands.entity(e).insert(HeldHandProp { key: key.clone(), entity, local: record.map(HandPropRecord::local).unwrap_or_default() });
        mind.brain.hand_prop.holding = true;
        info!("PED_HAND_PROP ped=#{} {key} created tick={tick}", ped.id.serial);
    }
    // Released props outside every observer's census cull ring go (b91 / dmo-plan: `dynamicobjects` cull 100 m).
    let Some(physics) = physics.as_deref_mut() else { return };
    released.0.retain(|&(id, entity)| {
        let at = physics.prop_dynamics().and_then(|d| d.position_of(id)).map(|v| Vec3::new(v.x, v.y, v.z));
        let near = at.is_some_and(|p| observers.observers.is_empty() || observers.observers.iter().any(|o| Vec3::from_array(o.position).distance(p) <= s.cull_distance));
        if near {
            return true;
        }
        physics.remove_runtime_prop(id);
        if let Ok(mut model) = commands.get_entity(entity) {
            model.despawn();
        }
        info!("PED_HAND_PROP culled body={id} tick={tick}");
        false
    });
}

/// The released hand props in the skater's body contact solve (b94): each one is a solid proxy with its box, mass and
/// velocity in contact group 12 (heavy) or 14 (small), so a hit goes through the ordinary skeleton contact: the relative
/// normal speed (halved for small objects) feeds the region forces and the usual wipeout checks (`82BD4A30` ->
/// `82BD88A0`). Retail has no hand-prop hit code (b92 Q2). The prop's own reaction stays the prop step's skater push.
pub(crate) fn push_hand_prop_proxies(
    released: Res<ReleasedHandProps>,
    settings: Res<super::LivingWorldSettings>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    replay: Res<crate::replay::Replay>,
) {
    let s = settings.ped_brain.values.hand_prop;
    if replay.active || released.0.is_empty() || !s.skater_contact {
        return;
    }
    let small_mass = skater.collision_feedback.settings.small_object_mass;
    let solids: Vec<_> = {
        let Some(dynamics) = physics.prop_dynamics() else { return };
        released.0.iter().filter_map(|&(id, _)| dynamics.body_state(id).map(|b| hand_prop_proxy(id, b, small_mass, &s))).collect()
    };
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    for solid in solids {
        proxies.append_solid(solid, &physics, &skater, false);
    }
    physics.network_proxies = proxies;
}

/// Proxy ids of released hand props: `HAND_PROP_PROXY_TAG | body id`.
pub(crate) const HAND_PROP_PROXY_TAG: u64 = 0x4850_0000_0000_0000;

type BodyState = (skate_core::math::Vector3, skate_core::math::Basis3, skate_core::math::Vector3, skate_core::math::Vector3, skate_core::math::Vector3, skate_core::physics::rigid_body::RetailInertiaDynamics);

fn hand_prop_proxy(id: u32, (c, basis, half, v, w, inertia): BodyState, small_mass: f32, s: &skate_core::living_world::peds::hand_prop::HandPropSettings) -> skate_dynamics::SolidBody {
    use skate_dynamics::rapier3d::prelude::{Pose, Rotation, SharedShape, Vector};
    let q = Quat::from_mat3(&Mat3::from_cols_array_2d(&basis.columns)).normalize();
    let rotation = Rotation::from_xyzw(q.x, q.y, q.z, q.w).normalize();
    let p = |v: skate_core::math::Vector3| Vector::new(v.x, v.y, v.z);
    let mass = if inertia.inverse_mass > 0.0 { 1.0 / inertia.inverse_mass } else { f32::INFINITY };
    skate_dynamics::SolidBody {
        id: HAND_PROP_PROXY_TAG | u64::from(id),
        pose: Pose::from_parts(p(c), rotation),
        center_of_mass: p(c),
        inertia_rotation: rotation,
        inverse_mass: inertia.inverse_mass,
        inverse_inertia: p(inertia.inverse_tensor),
        linvel: p(v),
        angvel: p(w),
        contact_group: if mass >= small_mass { s.heavy_group } else { s.small_group },
        colliders: vec![skate_dynamics::SolidCollider {
            shape: SharedShape::cuboid(half.x.max(0.01), half.y.max(0.01), half.z.max(0.01)),
            pose: Pose::from_parts(p(c), rotation),
            friction: 0.5,
        }],
    }
}

/// Turn the held object into a prop body at its world pose moving at `velocity`: collision from its meshes (in the
/// object's frame, scale folded in), the default physics block (NOT RETAIL YET: the template's
/// `livingworld_dynamicobject_characteristics` record is not resolved for hand prop templates, b91), and its model
/// detached from the hand and following the body. `None` when the pool is full or the model has no triangles yet.
#[allow(clippy::too_many_arguments)]
fn release(
    commands: &mut Commands,
    physics: &mut crate::physics::GamePhysics,
    held: &HeldHandProp,
    velocity: [f32; 3],
    globals: &Query<&GlobalTransform>,
    children: &Query<&Children>,
    mesh_parts: &Query<(&Mesh3d, &GlobalTransform)>,
    meshes: &Assets<Mesh>,
    room: bool,
) -> Option<u32> {
    if !room {
        return None;
    }
    let root = globals.get(held.entity).ok()?.compute_transform();
    let to_local = |p: Vec3| root.rotation.inverse() * (p - root.translation);
    let mut local = Vec::new();
    for part in children.iter_descendants(held.entity) {
        let Ok((mesh, gt)) = mesh_parts.get(part) else { continue };
        let Some(mesh) = meshes.get(&mesh.0) else { continue };
        local.extend(mesh_triangles(mesh).into_iter().map(|t| {
            t.map(|p| {
                let q = to_local(gt.transform_point(p));
                skate_core::math::Vector3::new(q.x, q.y, q.z)
            })
        }));
    }
    let m = Mat3::from_quat(root.rotation);
    let spec = crate::physics::prop_dynamics::RuntimeProp {
        template: format!("handprop/{}", held.key),
        local,
        physics: Default::default(),
        type_data: None,
        origin: skate_core::math::Vector3::new(root.translation.x, root.translation.y, root.translation.z),
        basis: skate_core::math::Basis3 { columns: [m.x_axis.to_array(), m.y_axis.to_array(), m.z_axis.to_array()] },
        linear_velocity: skate_core::math::Vector3::new(velocity[0], velocity[1], velocity[2]),
        angular_velocity: skate_core::math::Vector3::ZERO,
    };
    let id = physics.spawn_runtime_prop(&spec, 0)?;
    commands.entity(held.entity).remove::<ChildOf>().insert((
        Transform { translation: root.translation, rotation: root.rotation, scale: root.scale },
        crate::skate_world::PropInstance { id, template: spec.template.clone(), name: held.key.clone() },
    ));
    Some(id)
}

/// The triangles of a triangle-list mesh in its own space (positions and indices as stored).
fn mesh_triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return Vec::new() };
    let p = |i: usize| positions.get(i).map(|v| Vec3::from_array(*v));
    let indices: Vec<usize> = match mesh.indices() {
        Some(i) => i.iter().collect(),
        None => (0..positions.len()).collect(),
    };
    indices.chunks_exact(3).filter_map(|c| Some([p(c[0])?, p(c[1])?, p(c[2])?])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A released prop's skater-contact proxy: its box, velocity and mass, group 14 below the small-object mass
    /// (5.5) and 12 at or above it (`82C56BA0`).
    #[test]
    fn hand_prop_proxy_group_by_mass() {
        use skate_core::math::{Basis3, Vector3};
        let s = skate_core::living_world::peds::hand_prop::HandPropSettings::default();
        let identity = Basis3 { columns: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] };
        let inertia = |mass: f32| skate_core::physics::rigid_body::RetailInertiaDynamics { inverse_tensor: Vector3::ZERO, inverse_mass: 1.0 / mass, spherical: 0.0, maximum_linear_velocity: 100.0, maximum_angular_velocity: 100.0, linear_drag: 0.0, angular_drag: 0.0 };
        let state = |mass| (Vector3::new(1.0, 2.0, 3.0), identity, Vector3::new(0.03, 0.06, 0.03), Vector3::new(10.0, 1.0, 0.0), Vector3::ZERO, inertia(mass));
        let can = hand_prop_proxy(7, state(0.4), 5.5, &s);
        assert_eq!((can.id, can.contact_group, can.inverse_mass), (HAND_PROP_PROXY_TAG | 7, 14, 2.5));
        assert_eq!((can.linvel.x, can.center_of_mass.y), (10.0, 2.0));
        assert_eq!(hand_prop_proxy(7, state(5.5), 5.5, &s).contact_group, 12);
    }

    #[test]
    fn record_offset_is_translation_then_rotation() {
        let r = HandPropRecord { rotation_degrees: Vec3::new(90.0, 0.0, 0.0), offset: Vec3::new(0.01, 0.0, 0.01), ..Default::default() };
        let m = r.local();
        assert!(m.w_axis.truncate().distance(Vec3::new(0.01, 0.0, 0.01)) < 1e-6);
        // +Y of the prop turns to +Z of the hand under 90 degrees about X.
        assert!(m.transform_vector3(Vec3::Y).distance(Vec3::Z) < 1e-6);
    }

    #[test]
    fn stock_hand_props_load_with_models() {
        let Some(raw) = std::env::var_os("SKATE3_ASSET_ROOT") else { return };
        let Some(root) = std::env::split_paths(&raw).find(|r| r.join("private/living_world/tables.json").exists()) else { return };
        let d = HandPropData::load(&root).unwrap();
        let pop = &d.props["pop"];
        assert_eq!((pop.rotation_degrees, pop.offset), (Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.01, 0.0, 0.01)));
        assert_eq!(pop.carry_channel.as_deref(), Some("CarrySmallRHChannel"));
        assert!(pop.disposable && pop.can_sit && pop.can_attack_throw);
        let news = &d.props["newspaper"];
        assert!(!news.disposable && news.can_sit && news.can_attack_throw);
        eprintln!("hand prop models: {} of {}", d.props.values().filter(|p| p.glb.is_some()).count(), d.props.len());
    }
}
