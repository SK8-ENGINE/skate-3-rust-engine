use crate::{read_bounded, Manifest};
use mlua::{HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, VmState};
use serde::Deserialize;
use serde_json::Value;
use skate_dynamics::{BodyDesc, JointMotorDesc, JointSpringDesc, Shape};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Log {
        text: String,
    },
    Overlay {
        key: String,
        text: String,
    },
    UiCanvas { key: String, options: crate::presentation::CanvasOptions },
    UiRemove { key: String },
    CameraRig { body: String, options: crate::presentation::CameraRigOptions },
    AudioPreload { path: String },
    AudioPlay { key: String, options: crate::audio::AudioPlayOptions },
    AudioUpdate { key: String, options: crate::audio::AudioUpdateOptions },
    AudioStop {
        key: String,
        #[serde(default = "crate::audio::default_fade")]
        fade_out: f32,
    },
    AudioStopAll {},
    GraphicsMeshBuffer {
        key: String,
        options: crate::graphics_dynamic::MeshBufferOptions,
    },
    GraphicsMeshBufferWrite {
        key: String,
        data: crate::graphics_dynamic::MeshBufferWrite,
    },
    GraphicsMeshBufferAppend {
        key: String,
        data: crate::graphics_dynamic::MeshBufferWrite,
    },
    GraphicsLight {
        key: String,
        options: crate::graphics_dynamic::LightOptions,
    },
    PhysicsSpawn {
        key: String,
        body: BodyDesc,
    },
    PhysicsRemove {
        key: String,
    },
    PhysicsForce {
        key: String,
        force: [f32; 3],
        #[serde(default)]
        point: Option<[f32; 3]>,
    },
    PhysicsImpulse {
        key: String,
        impulse: [f32; 3],
        #[serde(default)]
        point: Option<[f32; 3]>,
    },
    PhysicsTorque {
        key: String,
        torque: [f32; 3],
    },
    PhysicsTorqueImpulse {
        key: String,
        torque: [f32; 3],
    },
    PhysicsSetLinvel {
        key: String,
        linvel: [f32; 3],
    },
    PhysicsSetAngvel {
        key: String,
        angvel: [f32; 3],
    },
    PhysicsSetPose {
        key: String,
        position: [f32; 3],
        rotation: [f32; 4],
    },
    PhysicsRevolute {
        key: String,
        body_a: String,
        body_b: String,
        anchor_a: [f32; 3],
        anchor_b: [f32; 3],
        #[serde(default = "axis_y")]
        axis: [f32; 3],
        #[serde(default)]
        limits: Option<[f32; 2]>,
        #[serde(default = "true_fn")]
        contacts_enabled: bool,
    },
    PhysicsJointMotor {
        key: String,
        motor: JointMotorDesc,
    },
    PhysicsPrismatic {
        key: String,
        body_a: String,
        body_b: String,
        anchor_a: [f32; 3],
        anchor_b: [f32; 3],
        #[serde(default = "neg_y")]
        axis: [f32; 3],
        #[serde(default)]
        limits: Option<[f32; 2]>,
        #[serde(default = "true_fn")]
        contacts_enabled: bool,
    },
    PhysicsJointSpring {
        key: String,
        spring: JointSpringDesc,
    },
    PhysicsRemoveJoint {
        key: String,
    },
    /// Extra convex/mesh collider on an existing body (e.g. high-friction tires).
    PhysicsAddCollider {
        key: String,
        shape: Shape,
        #[serde(default)]
        position: [f32; 3],
        #[serde(default = "one_friction")]
        friction: f32,
    },
    GraphicsMesh {
        key: String,
        /// Package-relative GLB path, or empty for a debug box.
        #[serde(default)]
        path: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        position: Option<[f32; 3]>,
        #[serde(default)]
        rotation: Option<[f32; 4]>,
        #[serde(default = "one_scale")]
        scale: [f32; 3],
        #[serde(default = "white")]
        color: [f32; 3],
        #[serde(default = "true_fn")]
        visible: bool,
    },
    GraphicsTransform { key:String, options:crate::scene::TransformOptions },
    GraphicsNode { key:String, node:String, options:crate::scene::TransformOptions },
    GraphicsResetNode { key:String, node:String },
    PhysicsDebug { enabled:bool },
    GraphicsRemove {
        key: String,
    },
    GraphicsVisibility {
        key: String,
        visible: bool,
    },
    PlayerAttach {
        body: String,
        #[serde(default)]
        offset: [f32; 3],
    },
    PlayerDetach { #[serde(default)] options:crate::scene::DetachOptions },
    CameraFollow {
        body: Option<String>,
        #[serde(default = "cam_offset")]
        offset: [f32; 3],
    },
    CameraSet {
        position: [f32; 3],
        #[serde(default)]
        look_at: Option<[f32; 3]>,
    },
    NetworkState {
        key: String,
        #[serde(default)]
        value: Value,
    },
}

fn one_scale() -> [f32; 3] {
    [1., 1., 1.]
}
fn one_friction() -> f32 {
    0.7
}
fn white() -> [f32; 3] {
    [0.85, 0.85, 0.9]
}
fn true_fn() -> bool {
    true
}
fn cam_offset() -> [f32; 3] {
    [0., 2.5, -6.]
}
fn axis_y() -> [f32; 3] {
    [0., 1., 0.]
}
fn neg_y() -> [f32; 3] {
    [0., -1., 0.]
}

impl Command {
    pub fn validate(&self) -> bool {
        let point = |p: &[f32; 3]| p.iter().all(|v| v.is_finite() && v.abs() <= 100_000.);
        let vec3 = |p: &[f32; 3]| p.iter().all(|v| v.is_finite());
        let quat = crate::scene::valid_quaternion;
        match self {
            Self::Log { text } => text.len() <= 2048,
            Self::Overlay { key, text } => crate::schema::valid_id(key) && text.len() <= 1024,
            Self::UiCanvas { key, options } => crate::schema::valid_id(key) && options.validate(),
            Self::UiRemove { key } => crate::schema::valid_id(key),
            Self::CameraRig { body, options } => crate::schema::valid_id(body) && options.validate(),
            Self::AudioPreload { path } => crate::audio::valid_audio_path(path),
            Self::AudioPlay { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::AudioUpdate { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::AudioStop { key, fade_out } => {
                crate::schema::valid_id(key) && fade_out.is_finite()
                    && (0.0..=2.0).contains(fade_out)
            }
            Self::AudioStopAll {} => true,
            Self::GraphicsMeshBuffer { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::GraphicsMeshBufferWrite { key, data } => {
                crate::schema::valid_id(key) && data.validate()
            }
            Self::GraphicsMeshBufferAppend { key, data } => {
                crate::schema::valid_id(key)
                    && !data.positions.is_empty()
                    && data.positions.len() <= crate::graphics_dynamic::MAX_MESH_BUFFER_APPEND_VERTICES
            }
            Self::GraphicsLight { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::PhysicsSpawn { key, body } => {
                crate::schema::valid_id(key) && serde_json::to_value(body).is_ok()
            }
            Self::PhysicsRemove { key }
            | Self::PhysicsRemoveJoint { key }
            | Self::GraphicsRemove { key }
            | Self::GraphicsVisibility { key, .. } => crate::schema::valid_id(key),
            Self::PhysicsAddCollider {
                key,
                shape,
                position,
                friction,
            } => {
                crate::schema::valid_id(key)
                    && point(position)
                    && friction.is_finite()
                    && (0. ..=2.).contains(friction)
                    && match shape {
                        Shape::Convex { points } => {
                            (4..=512).contains(&points.len())
                                && points.iter().flatten().all(|v| v.is_finite() && v.abs() <= 1000.)
                        }
                        Shape::Mesh { path, object } => {
                            !path.is_empty()
                                && path.len() <= 256
                                && !path.contains("..")
                                && !path.contains('\\')
                                && !object.is_empty()
                                && object.len() <= 120
                        }
                        _ => false,
                    }
            }
            Self::PhysicsForce { key, force, point: p }
            | Self::PhysicsImpulse {
                key,
                impulse: force,
                point: p,
            } => {
                crate::schema::valid_id(key)
                    && vec3(force)
                    && p.as_ref().is_none_or(|p| point(p))
            }
            Self::PhysicsTorque { key, torque }
            | Self::PhysicsTorqueImpulse { key, torque }
            | Self::PhysicsSetLinvel { key, linvel: torque }
            | Self::PhysicsSetAngvel { key, angvel: torque } => {
                crate::schema::valid_id(key) && vec3(torque)
            }
            Self::PhysicsSetPose {
                key,
                position,
                rotation,
            } => crate::schema::valid_id(key) && point(position) && quat(rotation),
            Self::PhysicsRevolute {
                key,
                body_a,
                body_b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                ..
            } => {
                crate::schema::valid_id(key)
                    && crate::schema::valid_id(body_a)
                    && crate::schema::valid_id(body_b)
                    && vec3(anchor_a)
                    && vec3(anchor_b)
                    && vec3(axis)
                    && limits.is_none_or(|[min, max]| {
                        min.is_finite()
                            && max.is_finite()
                            && min <= max
                            && min.abs() <= std::f32::consts::TAU
                            && max.abs() <= std::f32::consts::TAU
                    })
            }
            Self::PhysicsJointMotor { key, motor } => {
                crate::schema::valid_id(key)
                    && match motor {
                        JointMotorDesc::Velocity {
                            target_velocity,
                            factor,
                            max_force,
                        } => {
                            target_velocity.is_finite()
                                && factor.is_finite()
                                && *factor >= 0.
                                && max_force.is_none_or(|force| {
                                    force.is_finite() && (0. ..=1_000_000.).contains(&force)
                                })
                        }
                        JointMotorDesc::Position {
                            target_position,
                            stiffness,
                            damping,
                            max_force,
                        } => {
                            target_position.is_finite()
                                && stiffness.is_finite()
                                && (0. ..=100_000.).contains(stiffness)
                                && damping.is_finite()
                                && (0. ..=10_000.).contains(damping)
                                && max_force.is_none_or(|force| {
                                    force.is_finite() && (0. ..=1_000_000.).contains(&force)
                                })
                        }
                    }
            }
            Self::PhysicsPrismatic {
                key,
                body_a,
                body_b,
                anchor_a,
                anchor_b,
                axis,
                limits,
                contacts_enabled: _,
            } => {
                crate::schema::valid_id(key)
                    && crate::schema::valid_id(body_a)
                    && crate::schema::valid_id(body_b)
                    && point(anchor_a)
                    && point(anchor_b)
                    && vec3(axis)
                    && limits.is_none_or(|[min, max]| {
                        min.is_finite() && max.is_finite() && min <= max && min.abs() <= 10. && max.abs() <= 10.
                    })
            }
            Self::PhysicsJointSpring { key, spring } => {
                crate::schema::valid_id(key)
                    && spring.target_position.is_finite()
                    && spring.stiffness.is_finite()
                    && (0. ..=100_000.).contains(&spring.stiffness)
                    && spring.damping.is_finite()
                    && (0. ..=10_000.).contains(&spring.damping)
                    && spring
                        .max_force
                        .is_none_or(|f| f.is_finite() && (0. ..=1_000_000.).contains(&f))
            }
            Self::GraphicsMesh {
                key,
                path,
                body,
                position,
                rotation,
                scale,
                color,
                ..
            } => {
                crate::schema::valid_id(key)
                    && crate::scene::valid_asset(path)
                    && body.as_ref().is_none_or(|b| crate::schema::valid_id(b))
                    && position.as_ref().is_none_or(|p| point(p))
                    && rotation.as_ref().is_none_or(|q| quat(q))
                    && scale.iter().all(|v| v.is_finite() && *v > 0. && *v <= 100.)
                    && color
                        .iter()
                        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            }
            Self::PlayerAttach { body, offset } => {
                crate::schema::valid_id(body) && point(offset)
            }
            Self::GraphicsTransform { key, options } => crate::schema::valid_id(key) && options.validate()
                && options.relative.is_none() && options.linear_velocity.is_none() && options.angular_velocity.is_none(),
            Self::GraphicsNode { key, node, options } => crate::schema::valid_id(key) && crate::scene::valid_node(node) && options.validate(),
            Self::GraphicsResetNode { key, node } => crate::schema::valid_id(key) && crate::scene::valid_node(node),
            Self::PhysicsDebug { .. } => true,
            Self::PlayerDetach { options } => options.validate(),
            Self::CameraFollow { body, offset } => {
                body.as_ref().is_none_or(|b| crate::schema::valid_id(b)) && vec3(offset)
            }
            Self::CameraSet { position, look_at } => {
                point(position) && look_at.as_ref().is_none_or(|p| point(p))
            }
            Self::NetworkState { key, value } => {
                crate::schema::valid_id(key)
                    && serde_json::to_vec(value).is_ok_and(|v| v.len() <= 512)
            }
        }
    }
}

/// Query results must reach Lua as `nil` when absent. `lua.to_value` maps JSON
/// null to a null *userdata*, which is truthy and blows up on indexing.
fn command_kind(command: &Command) -> &'static str {
    match command {
        Command::Log { .. } => "log",
        Command::Overlay { .. } => "overlay",
        Command::UiCanvas { .. } => "ui_canvas",
        Command::UiRemove { .. } => "ui_remove",
        Command::CameraRig { .. } => "camera_rig",
        Command::AudioPreload { .. } => "audio_preload",
        Command::AudioPlay { .. } => "audio_play",
        Command::AudioUpdate { .. } => "audio_update",
        Command::AudioStop { .. } => "audio_stop",
        Command::AudioStopAll {} => "audio_stop_all",
        Command::GraphicsMeshBuffer { .. } => "graphics_mesh_buffer",
        Command::GraphicsMeshBufferWrite { .. } => "graphics_mesh_buffer_write",
        Command::GraphicsMeshBufferAppend { .. } => "graphics_mesh_buffer_append",
        Command::GraphicsLight { .. } => "graphics_light",
        Command::PhysicsSpawn { .. } => "physics_spawn",
        Command::PhysicsRemove { .. } => "physics_remove",
        Command::PhysicsForce { .. } => "physics_force",
        Command::PhysicsImpulse { .. } => "physics_impulse",
        Command::PhysicsTorque { .. } => "physics_torque",
        Command::PhysicsTorqueImpulse { .. } => "physics_torque_impulse",
        Command::PhysicsSetLinvel { .. } => "physics_set_linvel",
        Command::PhysicsSetAngvel { .. } => "physics_set_angvel",
        Command::PhysicsSetPose { .. } => "physics_set_pose",
        Command::PhysicsRevolute { .. } => "physics_revolute",
        Command::PhysicsJointMotor { .. } => "physics_joint_motor",
        Command::PhysicsPrismatic { .. } => "physics_prismatic",
        Command::PhysicsJointSpring { .. } => "physics_joint_spring",
        Command::PhysicsRemoveJoint { .. } => "physics_remove_joint",
        Command::PhysicsAddCollider { .. } => "physics_add_collider",
        Command::GraphicsMesh { .. } => "graphics_mesh",
        Command::GraphicsTransform { .. } => "graphics_transform",
        Command::GraphicsNode { .. } => "graphics_node",
        Command::GraphicsResetNode { .. } => "graphics_reset_node",
        Command::PhysicsDebug { .. } => "physics_debug",
        Command::GraphicsRemove { .. } => "graphics_remove",
        Command::GraphicsVisibility { .. } => "graphics_visibility",
        Command::PlayerAttach { .. } => "player_attach",
        Command::PlayerDetach { .. } => "player_detach",
        Command::CameraFollow { .. } => "camera_follow",
        Command::CameraSet { .. } => "camera_set",
        Command::NetworkState { .. } => "network_state",
    }
}

/// JSON null becomes a truthy null userdata via `lua.to_value`; map it to real nil.
fn json_to_lua(lua: &Lua, value: &Value) -> mlua::Result<mlua::Value> {
    if value.is_null() {
        return Ok(mlua::Value::Nil);
    }
    if let Some(obj) = value.as_object() {
        let t = lua.create_table()?;
        for (k, v) in obj {
            t.set(k.clone(), json_to_lua(lua, v)?)?;
        }
        return Ok(mlua::Value::Table(t));
    }
    if let Some(arr) = value.as_array() {
        let t = lua.create_table()?;
        for (i, v) in arr.iter().enumerate() {
            t.set(i + 1, json_to_lua(lua, v)?)?;
        }
        return Ok(mlua::Value::Table(t));
    }
    lua.to_value(value)
}

fn query_value(lua: &Lua, value: Value) -> mlua::Result<mlua::Value> {
    json_to_lua(lua, &value)
}

/// Lua hook fires every N VM instructions; each hook tick consumes one budget unit.
pub const LUA_INSTRUCTIONS_PER_BUDGET_UNIT: usize = 1000;
/// Per-callback instruction budget (each unit ~= 1000 Lua instructions).
pub const LUA_BUDGET_UNITS: usize = 800;

pub struct Vm {
    lua: Lua,
    callbacks: Table,
    advance: mlua::Function,
    budget: Arc<AtomicUsize>,
    queue: Arc<Mutex<Vec<Command>>>,
}

impl Vm {
    pub fn new(
        root: &Path,
        manifest: &Manifest,
        settings: &BTreeMap<String, Value>,
        snapshot: &Value,
    ) -> Result<Self, String> {
        let build = || -> mlua::Result<Self> {
            let lua = Lua::new_with(
                StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
                LuaOptions::default(),
            )?;
            lua.set_memory_limit(16 * 1024 * 1024)?;
            for key in [
                "pcall",
                "xpcall",
                "load",
                "loadfile",
                "dofile",
                "collectgarbage",
                "print",
            ] {
                lua.globals().set(key, mlua::Value::Nil)?;
            }
            let budget = Arc::new(AtomicUsize::new(LUA_BUDGET_UNITS));
            let counter = budget.clone();
            lua.set_hook(
                HookTriggers::new().every_nth_instruction(LUA_INSTRUCTIONS_PER_BUDGET_UNIT as u32),
                move |_, _| {
                    if counter
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                        .is_err()
                    {
                        return Err(mlua::Error::RuntimeError(
                            "Lua instruction budget exhausted".into(),
                        ));
                    }
                    Ok(VmState::Continue)
                },
            )?;
            let queue = Arc::new(Mutex::new(Vec::<Command>::new()));
            let out = queue.clone();
            let sdk = lua.create_table()?;
            sdk.set("api_version", 2)?;
            let capabilities = lua.create_table()?;
            capabilities.set("solid_bridge", 3)?;
            capabilities.set("model_collision", 1)?;
            capabilities.set("physics_debug", 1)?;
            capabilities.set("scene_transforms", 2)?;
            sdk.set("_native_capabilities", capabilities)?;
            sdk.set("mod_id", manifest.id.clone())?;
            sdk.set(
                "_submit",
                lua.create_function(move |lua, value: mlua::Value| {
                    let c: Command = lua.from_value(value)?;
                    if !c.validate() {
                        return Err(mlua::Error::RuntimeError(format!(
                            "Invalid command arguments ({})",
                            command_kind(&c)
                        )));
                    }
                    let mut q = out.lock().unwrap();
                    if q.len() >= 128 {
                        return Err(mlua::Error::RuntimeError(
                            "128 commands per callback maximum".into(),
                        ));
                    }
                    q.push(c);
                    Ok(())
                })?,
            )?;
            let asset_root = root.to_path_buf();
            let asset_root_objects = asset_root.clone();
            sdk.set(
                "read_text",
                lua.create_function(move |_, path: String| {
                    let b = read_bounded(&asset_root, &path, 256 * 1024)
                        .map_err(mlua::Error::RuntimeError)?;
                    String::from_utf8(b).map_err(mlua::Error::external)
                })?,
            )?;
            sdk.set(
                "_assets_objects",
                lua.create_function(move |lua, path: String| {
                    let lists = crate::assets::list_objects(&asset_root_objects, &path)
                        .map_err(mlua::Error::RuntimeError)?;
                    lua.to_value(&lists)
                })?,
            )?;
            sdk.set(
                "_raycast",
                lua.create_function(|lua, (origin, direction, options): (mlua::Value, mlua::Value, mlua::Value)| {
                    let origin: [f32; 3] = lua.from_value(origin)?;
                    let direction: [f32; 3] = lua.from_value(direction)?;
                    let options: crate::query::RaycastOptions = lua.from_value(options)?;
                    query_value(lua, crate::query::raycast_json(origin, direction, options))
                })?,
            )?;
            sdk.set(
                "_velocity_at",
                lua.create_function(|lua, (key, point): (String, mlua::Value)| {
                    let point: [f32; 3] = lua.from_value(point)?;
                    query_value(lua, crate::query::velocity_at_json(key, point))
                })?,
            )?;
            sdk.set(
                "_effective_inv_mass",
                lua.create_function(|lua, (key, point, direction): (String, mlua::Value, mlua::Value)| {
                    let point: [f32; 3] = lua.from_value(point)?;
                    let direction: [f32; 3] = lua.from_value(direction)?;
                    query_value(
                        lua,
                        crate::query::effective_inv_mass_json(key, point, direction),
                    )
                })?,
            )?;
            sdk.set(
                "_spring_ray",
                lua.create_function(|lua, (key, desc): (String, mlua::Value)| {
                    let desc: skate_dynamics::SpringRayDesc = lua.from_value(desc)?;
                    query_value(lua, crate::query::spring_ray_json(key, desc))
                })?,
            )?;
            sdk.set(
                "_local_ang_accel_impulse",
                lua.create_function(|lua, (key, local_accel, dt): (String, mlua::Value, f32)| {
                    let local_accel: [f32; 3] = lua.from_value(local_accel)?;
                    query_value(
                        lua,
                        crate::query::local_ang_accel_impulse_json(key, local_accel, dt),
                    )
                })?,
            )?;
            sdk.set("settings", lua.to_value(settings)?)?;
            sdk.set("snapshot", json_to_lua(&lua, snapshot)?)?;
            lua.globals().set("sdk", sdk)?;
            lua.load(include_str!("api.lua"))
                .set_name("@skate-sdk-2")
                .exec()?;
            let sdk: Table = lua.globals().get("sdk")?;
            let advance = sdk.get("_advance")?;
            sdk.set("_advance", mlua::Value::Nil)?;
            let code = read_bounded(root, &manifest.entry, 256 * 1024)
                .map_err(mlua::Error::RuntimeError)?;
            let source = std::str::from_utf8(&code).map_err(mlua::Error::external)?;
            let callbacks: Table = lua
                .load(source)
                .set_name(format!("@{}/{}", manifest.id, manifest.entry))
                .eval()?;
            for pair in callbacks.clone().pairs::<String, mlua::Value>() {
                let (key, value) = pair?;
                if ![
                    "on_load",
                    "on_unload",
                    "on_update",
                    "on_fixed_update",
                    "on_event",
                    "on_settings",
                ]
                .contains(&key.as_str())
                    || !matches!(value, mlua::Value::Function(_))
                {
                    return Err(mlua::Error::RuntimeError(format!(
                        "Unknown callback or non-function: {key}"
                    )));
                }
            }
            Ok(Self {
                lua,
                callbacks,
                advance,
                budget,
                queue,
            })
        };
        build().map_err(|e| e.to_string())
    }

    #[allow(dead_code)]
    pub fn settings(&mut self, settings: &BTreeMap<String, Value>) -> mlua::Result<()> {
        self.lua
            .globals()
            .get::<Table>("sdk")?
            .set("settings", self.lua.to_value(settings)?)
    }

    pub fn call(
        &mut self,
        name: &str,
        payload: Value,
        snapshot: &Value,
    ) -> Result<Vec<Command>, String> {
        self.budget.store(LUA_BUDGET_UNITS, Ordering::Relaxed);
        let invoke = || -> mlua::Result<()> {
            self.lua
                .globals()
                .get::<Table>("sdk")?
                .set("snapshot", json_to_lua(&self.lua, snapshot)?)?;
            if let Some(f) = self.callbacks.get::<Option<mlua::Function>>(name)? {
                f.call::<()>(self.lua.to_value(&payload)?)?;
            }
            if name == "on_update" {
                self.advance
                    .call::<()>(payload["dt"].as_f64().unwrap_or(0.))?;
            }
            Ok(())
        };
        let result = invoke();
        let remaining = self.budget.load(Ordering::Relaxed);
        let used = LUA_BUDGET_UNITS.saturating_sub(remaining);
        let approx_instructions = used * LUA_INSTRUCTIONS_PER_BUDGET_UNIT;
        let commands = std::mem::take(&mut *self.queue.lock().unwrap());
        match result {
            Ok(()) => {
                if used > LUA_BUDGET_UNITS * 9 / 10 {
                    eprintln!(
                        "Lua budget warning [{name}]: used {used}/{LUA_BUDGET_UNITS} units (~{approx_instructions} instructions)"
                    );
                }
                Ok(commands)
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("instruction budget exhausted") {
                    eprintln!(
                        "Lua budget exhausted [{name}]: used {used}/{LUA_BUDGET_UNITS} units (~{approx_instructions} instructions)"
                    );
                    Err(format!(
                        "{msg} (used {used}/{LUA_BUDGET_UNITS} budget units, ~{approx_instructions} instructions)"
                    ))
                } else {
                    Err(msg)
                }
            }
        }
    }
}

#[cfg(test)]
mod driving_extension_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn camera_and_canvas_commands_deserialize_and_validate() {
        let commands = [
            json!({"kind":"camera_rig", "body":"chassis", "options":{"mode":"hood", "collision":false, "fov_gain":0.0}}),
            json!({"kind":"ui_canvas", "key":"dash", "options":{"visible":false,"items":[
                {"key":"bar","type":"rect","size":[0.0,9.0],"color":[0.0,0.0,0.0,0.0]},
                {"key":"speed","type":"text","text":"100","position":[0.0,10.0]}
            ]}}),
            json!({"kind":"ui_remove", "key":"dash"}),
        ];
        for value in commands {
            let c:Command=serde_json::from_value(value).unwrap();
            assert!(c.validate());
        }
        let c:Command=serde_json::from_value(json!({"kind":"camera_rig","body":"chassis","options":{"fov":1000.0}})).unwrap();
        assert!(!c.validate());
        assert!(serde_json::from_value::<Command>(json!({"kind":"camera_rig","body":"chassis","options":{"unknown":2}})).is_err());
    }

    #[test]
    fn real_lua_wrapper_crosses_serde_boundary() {
        use std::time::{SystemTime,UNIX_EPOCH};
        let root=std::env::temp_dir().join(format!("skate-driving-api-{}-{}",
            std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            return { on_load=function()
                sdk.camera.rig('chassis', {mode='hood', collision=false, fov_gain=0})
                sdk.ui.canvas('dash', {visible=false, items={}})
                sdk.ui.canvas('dash', {items={
                    {key='bar',type='rect',size={0,9},color={0,0,0,0}},
                    {key='speed',type='text',text='100',position={0,10}}
                }})
                sdk.ui.remove('dash')
                sdk.camera.clear()
            end }
        "#).unwrap();
        let manifest:Manifest=serde_json::from_value(json!({
            "id":"tests.driving","api":2,"name":"Driving contract test", "version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        manifest.validate().unwrap();
        let snap=json!({"physics":{"bodies":{}}});
        let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snap).unwrap();
        let out=vm.call("on_load",json!({}),&snap).unwrap();
        assert_eq!(out.len(),5);
        match &out[0] { Command::CameraRig{options,..}=>{
            assert_eq!(options.mode,crate::presentation::CameraMode::Hood);
            assert!(!options.collision);assert_eq!(options.fov_gain,0.0);
        }, _=>panic!("wrong camera command") }
        match &out[1] { Command::UiCanvas{options,..}=>{
            assert!(!options.visible);assert!(options.items.is_empty());
        }, _=>panic!("wrong canvas command") }
        match &out[2] { Command::UiCanvas{options,..}=>{
            assert_eq!(options.items.len(),2);assert_eq!(options.items[0].size[0],0.0);
            assert_eq!(options.items[0].color,[0.0;4]);
        }, _=>panic!("wrong canvas command") }
        assert!(matches!(out[3],Command::UiRemove{..}));
        assert!(matches!(&out[4],Command::CameraFollow{body:None,..}));
        drop(vm);std::fs::remove_dir_all(root).unwrap();
    }
}


#[cfg(test)]
mod solid_extension_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scene_and_safe_detach_commands_cross_the_serde_boundary() {
        for value in [
            json!({"kind":"graphics_transform", "key":"scene", "options":{"position":[1,2,3], "rotation":[0,0,0,1]}}),
            json!({"kind":"graphics_node", "key":"scene", "node":"pivot", "options":{"relative":true,"rotation":[1,0,0,0],"angular_velocity":[3,0,0]}}),
            json!({"kind":"graphics_reset_node", "key":"scene", "node":"pivot"}),
            json!({"kind":"physics_debug", "enabled":true}),
            json!({"kind":"player_detach"}),
            json!({"kind":"player_detach", "options":{"candidates":[[2,0,0]],"height":1.8,"radius":0.3}}),
        ] {
            let command: Command = serde_json::from_value(value).unwrap();
            assert!(command.validate());
        }
        for value in [
            json!({"kind":"graphics_transform", "key":"scene", "options":{"relative":true}}),
            json!({"kind":"graphics_transform", "key":"scene", "options":{"rotation":[0,0,0,0]}}),
            json!({"kind":"graphics_node", "key":"scene", "node":"", "options":{}}),
            json!({"kind":"player_detach", "options":{"height":0.5,"radius":0.4}}),
        ] {
            let command: Command = serde_json::from_value(value).unwrap();
            assert!(!command.validate());
        }
        assert!(serde_json::from_value::<Command>(json!({"kind":"graphics_node", "key":"scene", "node":"pivot", "options":{"unknown":1}})).is_err());
    }
}

#[cfg(test)]
mod model_collision_extension_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_capabilities_debug_and_model_spawn_cross_real_lua_serde_boundary() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let root = std::env::temp_dir().join(format!("skate-model-api-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            return {on_load=function()
                assert(sdk.capabilities.solid_bridge == 3)
                assert(sdk.capabilities.model_collision == 1)
                assert(sdk.capabilities.physics_debug == 1)
                assert(sdk.capabilities.scene_transforms == 2)
                assert(sdk._native_capabilities == nil)
                sdk.physics.debug_colliders(true)
                sdk.physics.spawn('object', {shape={type='model',path='visual.glb',object='part',
                    options={max_hulls=32,resolution=96,concavity=.0025}}, body_type='dynamic',mass=100})
            end}
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({"id":"tests.model", "api":2,
            "name":"Model API contract", "version":"1.0.0", "author":"test", "description":"test",
            "entry":"main.lua", "settings":{}})).unwrap();
        manifest.validate().unwrap();
        let snapshot = json!({"physics":{"bodies":{}}});
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &snapshot).unwrap();
        let commands = vm.call("on_load", json!({}), &snapshot).unwrap();
        assert_eq!(commands.len(), 2);
        assert!(matches!(&commands[0], Command::PhysicsDebug {enabled:true}));
        match &commands[1] {
            Command::PhysicsSpawn {body,..} => match &body.shape {
                Shape::Model {path,object,options} => {
                    assert_eq!(path, "visual.glb"); assert_eq!(object, "part");
                    assert_eq!(options.scale, [1.;3]); assert!(options.validate().is_ok());
                }
                _ => panic!("model descriptor was not retained"),
            },
            _ => panic!("wrong native spawn command"),
        }
        drop(vm);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod graphics_mesh_buffer_tests {
    use super::*;
    use mlua::{Lua, LuaOptions, LuaSerdeExt, StdLib};
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mesh_buffer_write_rejects_empty_uv_table() {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )
        .unwrap();
        let value = lua
            .load(
                r#"return {kind="graphics_mesh_buffer_write",key="skids",data={
                    positions={{0,0,0},{1,0,0},{0,1,0}},
                    indices={0,1,2},
                    uvs={}
                }}"#,
            )
            .eval::<mlua::Value>()
            .unwrap();
        assert!(lua.from_value::<Command>(value).is_err());
    }

    #[test]
    fn real_lua_mesh_buffer_write_crosses_serde_boundary() {
        let root = std::env::temp_dir().join(format!(
            "skate-mesh-write-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("main.lua"),
            r#"
            return { on_load=function()
                sdk.graphics.mesh_buffer_write("skids", {
                    positions={{0,0,0},{1,0,0},{0,1,0},{1,1,0}},
                    indices={0,1,2,1,3,2},
                    uvs={{0,0},{1,0},{0,0.25},{1,0.25}},
                })
            end }
        "#,
        )
        .unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.mesh","api":2,"name":"Mesh write test","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        }))
        .unwrap();
        manifest.validate().unwrap();
        let snap = json!({"physics":{"bodies":{}}});
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &snap).unwrap();
        let out = vm.call("on_load", json!({}), &snap).unwrap();
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Command::GraphicsMeshBufferWrite { .. }));
        drop(vm);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_lua_mesh_buffer_append_crosses_serde_boundary() {
        let root = std::env::temp_dir().join(format!(
            "skate-mesh-append-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let png = include_bytes!("../tests/fixtures/texture.png");
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(root.join("textures/test.png"), png).unwrap();
        std::fs::write(
            root.join("main.lua"),
            r#"
            return { on_load=function()
                sdk.graphics.mesh_buffer("skids", {texture="textures/test.png"})
                sdk.graphics.mesh_buffer_append("skids", {
                    positions={{0,0,0},{1,0,0}},
                    uvs={{0,0},{1,0}},
                })
                sdk.graphics.mesh_buffer_append("skids", {
                    positions={{1,1,0},{2,1,0}},
                    indices={0,1,2,1,3,2},
                    uvs={{0,0.25},{1,0.25}},
                })
            end }
        "#,
        )
        .unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.mesh","api":2,"name":"Mesh append test","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        }))
        .unwrap();
        manifest.validate().unwrap();
        let snap = json!({"physics":{"bodies":{}}});
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &snap).unwrap();
        let out = vm.call("on_load", json!({}), &snap).unwrap();
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0], Command::GraphicsMeshBuffer { .. }));
        assert!(matches!(out[1], Command::GraphicsMeshBufferAppend { .. }));
        assert!(matches!(out[2], Command::GraphicsMeshBufferAppend { .. }));
        drop(vm);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Mirrors Skyline skid strip helpers — single-point strips used to crash in norm(dot(nil)).
    #[test]
    fn skid_strip_single_point_forward_lua() {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )
        .unwrap();
        lua.load(
            r#"
            local function vec(x)
                return type(x) == "table"
                    and type(x[1]) == "number"
                    and type(x[2]) == "number"
                    and type(x[3]) == "number"
            end
            local function sub(a, b) return { a[1] - b[1], a[2] - b[2], a[3] - b[3] } end
            local function mul(a, s) return { a[1] * s, a[2] * s, a[3] * s } end
            local function dot(a, b)
                if not vec(a) or not vec(b) then return 0 end
                return a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
            end
            local function norm(a)
                if not vec(a) then return nil end
                local l = math.sqrt(dot(a, a))
                return l > 1e-8 and mul(a, 1 / l) or nil
            end
            local function strip_forward(pts, i)
                local pt = pts[i]
                if not pt or not vec(pt.pos) then return { 1, 0, 0 } end
                if i < #pts and vec(pts[i + 1].pos) then return sub(pts[i + 1].pos, pt.pos) end
                if i > 1 and vec(pts[i - 1].pos) then return sub(pt.pos, pts[i - 1].pos) end
                return { 1, 0, 0 }
            end
            local function start_chunk_strip(mesh, pts, i, width)
                local pt = pts[i]
                if not pt or not vec(pt.pos) then return false end
                local fwd = norm(strip_forward(pts, i)) or { 1, 0, 0 }
                mesh.positions[#mesh.positions + 1] = { pt.pos[1], pt.pos[2], pt.pos[3] }
                mesh.positions[#mesh.positions + 1] = { pt.pos[1] + width, pt.pos[2], pt.pos[3] }
                return true
            end

            local pts = { { pos = { 0, 0, 0 }, normal = { 0, 1, 0 } } }
            local mesh = { positions = {} }
            assert(start_chunk_strip(mesh, pts, 1, 0.3))
            assert(#mesh.positions == 2)

            local pts2 = {
                { pos = { 0, 0, 0 }, normal = { 0, 1, 0 } },
                { pos = { 2, 0, 0 }, normal = { 0, 1, 0 } },
            }
            local fwd = norm(strip_forward(pts2, 1)) or { 1, 0, 0 }
            assert(math.abs(fwd[1] - 1) < 1e-6)
        "#,
        )
        .exec()
        .unwrap();
    }
}
