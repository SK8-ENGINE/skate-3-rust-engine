//! Ped hand props (b87, b89): records, models and the held object on the ped; release as a prop (b90, b91).
//! Retail mechanism and gaps: docs/hails-additions/26g-living-world-ped-interactions.md#ped-hand-props

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
    /// The model's attach locator in its own (glb) space: the point retail puts at the record's offset, so the grip
    /// lands on the hand rather than the model origin. From `hand_props.json` `attach` (setup,
    /// `dynamic_props.template_attach`); identity when absent (a mod model, or an older install).
    pub attach: Mat4,
    pub carry_channel: Option<String>,
    pub disposable: bool,
    pub can_sit: bool,
    pub can_attack_throw: bool,
}

impl HandPropRecord {
    /// The prop's frame in the hand bone's frame, as retail `82E3E1B0` composes it: T(offset) * R * inverse(P).
    /// R turns the record's degrees into Rz * Ry * Rx (research b204: rows [cy cz, cy sz, -sy], ...), so the order
    /// is ZYX, not XYZ; P is the template's attach locator (`template+0x78`, written by `82C4E128`, b204 / b205).
    pub fn local(&self) -> Mat4 {
        let r = self.rotation_degrees * (std::f32::consts::PI / 180.0);
        Mat4::from_translation(self.offset) * Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, r.z, r.y, r.x)) * self.attach.inverse()
    }
}

/// Every hand prop by key (`livingworld_handprops`), and the idle actions: each entity's usage per prop
/// (`livingworld_entities` `handprops`: {handprop, f32_24, f32_28, usage}) and each usage's action list
/// (`livingworld_handprops_usagecharacteristics` `actions`) [data, b164].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HandPropData {
    pub props: BTreeMap<String, HandPropRecord>,
    pub usages: BTreeMap<String, Vec<skate_core::living_world::peds::hand_prop::HandPropAction>>,
    pub entity_usage: BTreeMap<(String, String), String>,
}

fn flag(fields: Option<&Value>, key: &str) -> bool {
    fields.and_then(|f| f.get(key)).and_then(Value::as_bool).unwrap_or(false)
}

fn vec3(v: Option<&Value>) -> Vec3 {
    let f = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Vec3::new(f("x"), f("y"), f("z"))
}

/// A 4x4 row-vector matrix as setup writes it (rows = basis vectors, translation last), as a glam matrix; `None`
/// unless it is 16 finite numbers with an invertible rotation.
fn attach_matrix(v: &Value) -> Option<Mat4> {
    let rows = v.as_array().filter(|r| r.len() == 4)?;
    let mut cols = [0.0_f32; 16];
    for (i, row) in rows.iter().enumerate() {
        let row = row.as_array().filter(|r| r.len() == 4)?;
        for (j, x) in row.iter().enumerate() {
            cols[i * 4 + j] = x.as_f64()? as f32;
        }
    }
    let m = Mat4::from_cols_array(&cols);
    (m.is_finite() && m.determinant().abs() > 1e-6).then_some(m)
}

impl HandPropData {
    /// From `private/living_world/tables.json` (inheritance already merged by the exporter) and `hand_props.json`.
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::load_with(root, None)
    }

    /// [`Self::load`] with `tables.json` already parsed (`None`: read and parse it here).
    pub fn load_with(root: &Path, parsed: Option<&Value>) -> Result<Self, String> {
        let read = |p: &str| -> Result<Value, String> { serde_json::from_slice(&std::fs::read(root.join(p)).map_err(|e| format!("{p}: {e}"))?).map_err(|e| format!("{p}: {e}")) };
        let own;
        let tables: &Value = match parsed {
            Some(v) => v,
            None => {
                own = read("private/living_world/tables.json")?;
                &own
            }
        };
        let records = tables.pointer("/classes/livingworld_handprops").and_then(Value::as_object).ok_or("tables.json has no livingworld_handprops")?;
        let models = read("private/living_world/hand_props.json").ok();
        let mut props = BTreeMap::new();
        for (key, record) in records {
            let fields = record.get("fields");
            let file = models.as_ref().and_then(|m| m.pointer(&format!("/props/{key}/file"))).and_then(Value::as_str);
            let attach = models.as_ref().and_then(|m| m.pointer(&format!("/props/{key}/attach"))).and_then(attach_matrix);
            props.insert(
                key.clone(),
                HandPropRecord {
                    glb: file.map(|f| format!("private/living_world/{f}")),
                    rotation_degrees: vec3(fields.and_then(|f| f.get(ROTATION_FIELD))),
                    offset: vec3(fields.and_then(|f| f.get(OFFSET_FIELD))),
                    attach: attach.unwrap_or(Mat4::IDENTITY),
                    carry_channel: fields.and_then(|f| f.get(CHANNEL_FIELD)).and_then(Value::as_str).map(str::to_string),
                    disposable: flag(fields, DISPOSABLE_FIELD),
                    can_sit: flag(fields, CAN_SIT_FIELD),
                    can_attack_throw: flag(fields, CAN_ATTACK_THROW_FIELD),
                },
            );
        }
        let key_of = |v: Option<&Value>| v.and_then(|r| r.get("key")).and_then(Value::as_str).map(str::to_string);
        let mut usages = BTreeMap::new();
        if let Some(records) = tables.pointer("/classes/livingworld_handprops_usagecharacteristics").and_then(Value::as_object) {
            for (key, record) in records {
                let list = record.pointer("/fields/actions").and_then(Value::as_array).map_or(Vec::new(), |a| {
                    a.iter()
                        .map(|e| {
                            let f = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                            skate_core::living_world::peds::hand_prop::HandPropAction {
                                action: e.get("action").and_then(Value::as_i64).unwrap_or(0) as i32,
                                weight: f("f32_4"),
                                min_seconds: f("f32_8"),
                                max_seconds: f("f32_12"),
                            }
                        })
                        .collect()
                });
                usages.insert(key.clone(), list);
            }
        }
        let mut entity_usage = BTreeMap::new();
        if let Some(records) = tables.pointer("/classes/livingworld_entities").and_then(Value::as_object) {
            for (entity, record) in records {
                for e in record.pointer("/fields/handprops").and_then(Value::as_array).into_iter().flatten() {
                    if let (Some(prop), Some(usage)) = (key_of(e.get("handprop")), key_of(e.get("usage"))) {
                        entity_usage.insert((entity.clone(), prop), usage);
                    }
                }
            }
        }
        Ok(Self { props, usages, entity_usage })
    }

    /// The idle actions of `entity` holding `prop` (empty: the ped never acts with it).
    pub fn actions_for(&self, entity: &str, prop: &str) -> Vec<skate_core::living_world::peds::hand_prop::HandPropAction> {
        self.entity_usage.get(&(entity.to_string(), prop.to_string())).and_then(|u| self.usages.get(u)).cloned().unwrap_or_default()
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
    mut peds: Query<(Entity, &super::peds::Pedestrian, &super::peds::PedBody, &mut super::peds::PedMind, Option<&super::peds::PedPuppet>, Option<&HeldHandProp>, Option<&ReleasedHandProp>, &Transform)>,
    mut world_key: Local<Option<(String, u64)>>,
) {
    let tick = state.world.tick();
    // A map change (b216): the released props' bodies were in the old world's physics, whose runtime ids the new one
    // reuses, so only their models go (removing by id would hit a new map's prop).
    if *world_key != data.loaded_for {
        if world_key.is_some() {
            for (_, entity) in released.0.drain(..) {
                if let Ok(mut model) = commands.get_entity(entity) {
                    model.despawn();
                }
            }
        }
        *world_key = data.loaded_for.clone();
    }
    let s = settings.ped_brain.values.hand_prop;
    let hand_bone = data.rig.names.iter().position(|n| n.eq_ignore_ascii_case(HAND_PROP_BONE));
    for (e, ped, body, mut mind, puppet, held, linked, ped_transform) in &mut peds {
        let mind = &mut *mind;
        // The tazer's clip-driven states (`82E3ED50`): drawn near the draw clip's end, put away (and the "tazr" prop
        // deleted) near the holster clip's end.
        if mind.brain.tazer_state.is_some_and(|s| s == 1 || s == 3) {
            let left = body.player.channel_remaining(&*data);
            mind.brain.update_tazer_clip(&settings.ped_brain.values, body.plugin_motion.tazer_played, left);
        }
        if let Some(h) = held {
            // The tick's hand pose (present_ped_pose with no overstep), not the interpolated drawn one.
            let followers = puppet.map_or(&[][..], |p| &p.followers[..]);
            let pose = hand_bone
                .and_then(|i| super::peds::ped_globals(&data.rig, body, &*data, 0.0, followers)?.get(i).copied())
                .map(|bone| Transform::from_matrix(ped_transform.to_matrix() * bone * h.local));
            let at = pose.map(|p| p.translation.to_array());
            // A throw's release, or a litter drop (`82E3ED50`: timer 34 after HandPropLitter).
            let thrown = at.and_then(|at| mind.brain.update_hand_prop_release(&s, at)).or_else(|| mind.brain.update_litter());
            if let Some(velocity) = thrown.or(mind.hand_prop_release.take()) {
                commands.entity(e).remove::<HeldHandProp>();
                // The held frames keep their hand-space track on a hidden copy; the dropped prop starts its own (b453).
                crate::replay::keep_model(&mut commands, h.entity);
                // The idle action runs only while held (`82E3ED50`, b649): no talk after the drop.
                mind.action_cue = None;
                let room = released.0.len() < s.max_live;
                let id = physics.as_deref_mut().and_then(|p| release(&mut commands, p, h, pose, velocity, &globals, &children, &mesh_parts, &meshes, room));
                match id {
                    Some(id) => {
                        released.0.push((id, h.entity));
                        commands.entity(e).insert(ReleasedHandProp { body: id });
                        info!("PED_HAND_PROP ped=#{} {} released body={id} velocity=[{:.2}, {:.2}, {:.2}] tick={tick}", ped.id.serial, h.key, velocity[0], velocity[1], velocity[2]);
                    }
                    None => {
                        commands.entity(h.entity).despawn();
                        mind.brain.hand_prop.clear();
                        mind.unlink_seq = mind.unlink_seq.wrapping_add(1);
                        info!("PED_HAND_PROP ped=#{} {} released without a body tick={tick}", ped.id.serial, h.key);
                    }
                }
                continue;
            }
            let want = mind.brain.hand_prop.has().then(|| mind.brain.hand_prop.key.clone()).flatten();
            if want.as_deref() != Some(h.key.as_str()) {
                crate::replay::keep_model(&mut commands, h.entity);
                commands.entity(h.entity).despawn();
                commands.entity(e).remove::<HeldHandProp>();
                mind.brain.hand_prop.holding = false;
                mind.action_cue = None;
                mind.unlink_seq = mind.unlink_seq.wrapping_add(1);
                info!("PED_HAND_PROP ped=#{} {} removed tick={tick}", ped.id.serial, h.key);
                continue;
            }
            // The idle action (`82E3F470`): the channel cue changes only when the action moves on.
            let left = body.player.channel_remaining(&*data);
            if let Some(cue) = mind.brain.update_hand_prop_action(&s.actions, left) {
                if mind.action_cue != Some(cue) {
                    info!("PED_HAND_PROP_ACTION ped=#{} {} {cue:?} action={} tick={tick}", ped.id.serial, h.key, mind.brain.hand_prop.action);
                    mind.action_cue = Some(cue);
                    mind.action_seq = mind.action_seq.wrapping_add(1);
                }
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
                mind.unlink_seq = mind.unlink_seq.wrapping_add(1);
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
        // The attach (`82E3DE18`): this entity's usage of the prop, then the first pick.
        mind.brain.attach_hand_prop_actions(data.hand_props.actions_for(&ped.entity, &key));
        mind.action_cue = None;
        info!("PED_HAND_PROP ped=#{} {key} created actions={} tick={tick}", ped.id.serial, mind.brain.hand_prop.actions.len());
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
        crate::replay::keep_model(&mut commands, entity);
        if let Ok(mut model) = commands.get_entity(entity) {
            model.despawn();
        }
        info!("PED_HAND_PROP culled body={id} tick={tick}");
        false
    });
}

/// Released hand props in the skater's contact solve (groups 12 / 14; b94, b92): docs/hails-additions/26g-living-world-ped-interactions.md#hand-prop-proxies
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

/// Turn the held object into a prop body (default physics block: NOT RETAIL YET, b91); `None` if pool full.
#[allow(clippy::too_many_arguments)]
fn release(
    commands: &mut Commands,
    physics: &mut crate::physics::GamePhysics,
    held: &HeldHandProp,
    pose: Option<Transform>,
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
    // Shape from the drawn model relative to its own root; pose from the tick.
    let drawn = globals.get(held.entity).ok()?.compute_transform();
    let root = pose?;
    let to_local = |p: Vec3| drawn.rotation.inverse() * (p - drawn.translation);
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

    /// Retail `82E3E1B0` builds the rotation as Rz * Ry * Rx; its first basis row is [cy cz, cy sz, -sy] (b204).
    #[test]
    fn record_rotation_is_rz_ry_rx_like_retail() {
        // The stock cellphone's angles (15, -10, 15 degrees): with two or more non-zero axes the order matters.
        let d = Vec3::new(15.0, -10.0, 15.0);
        let r = HandPropRecord { rotation_degrees: d, ..Default::default() }.local();
        let (x, y, z) = (d.x.to_radians(), d.y.to_radians(), d.z.to_radians());
        let row0 = Vec3::new(y.cos() * z.cos(), y.cos() * z.sin(), -y.sin());
        assert!(r.transform_vector3(Vec3::X).distance(row0) < 1e-5, "{:?} vs {row0:?}", r.transform_vector3(Vec3::X));
        let rzyx = Mat4::from_rotation_z(z) * Mat4::from_rotation_y(y) * Mat4::from_rotation_x(x);
        assert!(r.abs_diff_eq(rzyx, 1e-5));
    }

    /// Retail places the attach locator P, not the model origin, at the record's offset: World = H * T * R * inv(P).
    #[test]
    fn attach_locator_lands_on_the_offset() {
        // A briefcase-like locator as setup writes it (row vectors): handle 0.343 m up, turned 90 degrees.
        let rows = serde_json::json!([[0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.343, 0.0, 1.0]]);
        let attach = attach_matrix(&rows).unwrap();
        assert!(attach.w_axis.truncate().distance(Vec3::new(0.0, 0.343, 0.0)) < 1e-6);
        assert!(attach.transform_vector3(Vec3::X).distance(Vec3::new(0.0, -1.0, 0.0)) < 1e-6, "row 0 is the image of +x");
        let r = HandPropRecord { rotation_degrees: Vec3::new(15.0, -10.0, 15.0), offset: Vec3::new(0.01, 0.005, 0.0), attach, ..Default::default() };
        let m = r.local();
        // The handle lands on the offset, with the locator's frame turned by R.
        assert!(m.transform_point3(Vec3::new(0.0, 0.343, 0.0)).distance(r.offset) < 1e-5);
        let rot = Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, 15f32.to_radians(), (-10f32).to_radians(), 15f32.to_radians()));
        assert!((m * attach).abs_diff_eq(Mat4::from_translation(r.offset) * rot, 1e-5));
        // Missing or broken: identity (the model origin, as before).
        assert_eq!(HandPropRecord::default().attach, Mat4::IDENTITY);
        assert!(attach_matrix(&serde_json::json!([[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]])).is_none());
        assert!(attach_matrix(&serde_json::json!(null)).is_none());
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
