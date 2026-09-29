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
#[serde(deny_unknown_fields)]
pub struct TeleportOptions {
    pub position: [f32; 3],
    #[serde(default)]
    pub heading: Option<f32>,
    #[serde(default)]
    pub velocity: Option<[f32; 3]>,
}

impl TeleportOptions {
    pub fn validate(&self) -> bool {
        let point = |p: &[f32; 3]| p.iter().all(|v| v.is_finite() && v.abs() <= 100_000.);
        let vel = |p: &[f32; 3]| p.iter().all(|v| v.is_finite() && v.abs() <= 200.);
        point(&self.position)
            && self.heading.is_none_or(|h| h.is_finite() && h.abs() <= 1000.)
            && self.velocity.as_ref().is_none_or(vel)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeOptions {
    pub position: [f32; 3],
    pub size: [f32; 3],
    #[serde(default)]
    pub rotation: Option<[f32; 4]>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default = "volume_color")]
    pub color: [f32; 3],
    #[serde(default = "volume_opacity")]
    pub opacity: f32,
}

fn volume_color() -> [f32; 3] {
    [0.2, 0.85, 1.0]
}
fn volume_opacity() -> f32 {
    0.35
}

impl VolumeOptions {
    pub fn validate(&self) -> bool {
        let point = |p: &[f32; 3]| p.iter().all(|v| v.is_finite() && v.abs() <= 100_000.);
        point(&self.position)
            && self.size.iter().all(|v| v.is_finite() && *v > 0.01 && *v <= 500.)
            && self
                .rotation
                .as_ref()
                .is_none_or(|q| crate::scene::valid_quaternion(q))
            && self.color.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && self.opacity.is_finite()
            && (0. ..=1.).contains(&self.opacity)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureOptions {
    pub position: [f32; 3],
    pub look_at: [f32; 3],
    #[serde(default = "capture_fov")]
    pub fov: f32,
    #[serde(default = "capture_size")]
    pub width: u32,
    #[serde(default = "capture_size")]
    pub height: u32,
}

fn capture_fov() -> f32 {
    70.0_f32.to_radians()
}
fn capture_size() -> u32 {
    256
}

impl CaptureOptions {
    pub fn validate(&self) -> bool {
        let point = |p: &[f32; 3]| p.iter().all(|v| v.is_finite() && v.abs() <= 100_000.);
        point(&self.position)
            && point(&self.look_at)
            && self.fov.is_finite()
            && (0.2..=2.5).contains(&self.fov)
            && (64..=512).contains(&self.width)
            && (64..=512).contains(&self.height)
            && self.width % 16 == 0
            && self.height % 16 == 0
    }
}

fn valid_peer(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && s.bytes().all(|b| b.is_ascii_digit())
}

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
    MultiplayerDebug { key: String, text: String },
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
        #[serde(default = "opaque")]
        opacity: f32,
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
    CameraWatch {
        #[serde(default)]
        peer: Option<String>,
    },
    NetworkState {
        key: String,
        #[serde(default)]
        value: Value,
    },
    UiMenu { key:String, options:crate::extensions::MenuOptions },
    UiRemoveMenu { key:String },
    RigPart {index:usize, options:Option<crate::extensions::PartOverride>},
    GraphGate { graph:String, target:String, index:usize, enabled:Option<bool> },
    EngineInspect { system:String },
    Request { key:String, #[serde(default)] token:u64, command:Box<Command> },
    InputOverride { action:usize, value:Option<f32> },
    NativeImpulse { body:crate::extensions::NativeBodyRef, impulse:[f32;3], point:Option<[f32;3]>, angular:bool },
    PlayerJoint { joint:usize, options:crate::extensions::JointOverride },
    PlayerResetJoint { joint:usize },
    PlayerResetJoints {},
    PlayerSuspend { suspended: bool },
    PlayerTeleport {
        options: TeleportOptions,
    },
    SessionClaim {},
    SessionTransfer {
        peer: String,
    },
    SessionTeleport {
        peer: String,
        options: TeleportOptions,
    },
    VolumeBox {
        key: String,
        options: VolumeOptions,
    },
    VolumeRemove {
        key: String,
    },
    CameraCapture {
        key: String,
        options: CaptureOptions,
    },
    CameraClearCapture {
        key: String,
    },
}

fn opaque() -> f32 {
    1.0
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
            Self::RigPart {index,options} => *index<26 && options.as_ref().is_none_or(|o|o.validate()),
            Self::GraphGate {graph,target,index,..} => matches!(graph.as_str(),"action"|"motion") && matches!(target.as_str(),"state"|"transition"|"behavior") && *index<65536,
            Self::EngineInspect {system} => matches!(system.as_str(),"graphs"|"scoring"),
            Self::Request {key,command,token} => *token<=9_007_199_254_740_991 && crate::schema::valid_id(key) && !matches!(**command,Self::Request{..}) && command.validate(),
            Self::InputOverride {action,value} => (64..=81).contains(action) && value.is_none_or(|v|v.is_finite() && (-1.0..=1.0).contains(&v)),
            Self::NativeImpulse {body,impulse,point:p,..} => body.validate() && impulse.iter().all(|v|v.is_finite() && v.abs()<=100_000.) && p.as_ref().is_none_or(point),
            Self::Log { text } => text.len() <= 2048,
            Self::Overlay { key, text } | Self::MultiplayerDebug { key, text } => crate::schema::valid_id(key) && text.len() <= 1024,
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
                opacity,
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
                    && opacity.is_finite()
                    && (0. ..=1.).contains(opacity)
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
            Self::CameraWatch { peer } => peer
                .as_ref()
                .is_none_or(|p| p.is_empty() || valid_peer(p)),
            Self::NetworkState { key, value } => {
                crate::schema::valid_id(key)
                    && serde_json::to_vec(value).is_ok_and(|v| v.len() <= 512)
            }
            Self::UiMenu {key,options} => crate::schema::valid_id(key) && options.validate(),
            Self::UiRemoveMenu {key} => crate::schema::valid_id(key),
            Self::PlayerJoint {joint,options} => *joint < 22 && options.validate(),
            Self::PlayerResetJoint {joint} => *joint < 22,
            Self::PlayerResetJoints {} => true,
            Self::PlayerSuspend { .. } => true,
            Self::PlayerTeleport { options } => options.validate(),
            Self::SessionClaim {} => true,
            Self::SessionTransfer { peer } => valid_peer(peer),
            Self::SessionTeleport { peer, options } => valid_peer(peer) && options.validate(),
            Self::VolumeBox { key, options } => crate::schema::valid_id(key) && options.validate(),
            Self::VolumeRemove { key } => crate::schema::valid_id(key),
            Self::CameraCapture { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::CameraClearCapture { key } => crate::schema::valid_id(key),
        }
    }
}

/// Query results must reach Lua as `nil` when absent. `lua.to_value` maps JSON
/// null to a null *userdata*, which is truthy and blows up on indexing.
fn command_kind(command: &Command) -> &'static str {
    match command {
        Command::RigPart {..} => "rig_part",
        Command::GraphGate {..} => "graph_gate",
        Command::EngineInspect {..} => "engine_inspect",
        Command::Request {..} => "request",
        Command::InputOverride {..} => "input_override",
        Command::NativeImpulse {..} => "native_impulse",
        Command::Log { .. } => "log",
        Command::Overlay { .. } => "overlay",
        Command::MultiplayerDebug { .. } => "multiplayer_debug",
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
        Command::CameraWatch { .. } => "camera_watch",
        Command::NetworkState { .. } => "network_state",
        Command::UiMenu { .. } => "ui_menu",
        Command::UiRemoveMenu { .. } => "ui_remove_menu",
        Command::PlayerJoint { .. } => "player_joint",
        Command::PlayerResetJoint { .. } => "player_reset_joint",
        Command::PlayerResetJoints {} => "player_reset_joints",
        Command::PlayerSuspend { .. } => "player_suspend",
        Command::PlayerTeleport { .. } => "player_teleport",
        Command::SessionClaim { .. } => "session_claim",
        Command::SessionTransfer { .. } => "session_transfer",
        Command::SessionTeleport { .. } => "session_teleport",
        Command::VolumeBox { .. } => "volume_box",
        Command::VolumeRemove { .. } => "volume_remove",
        Command::CameraCapture { .. } => "camera_capture",
        Command::CameraClearCapture { .. } => "camera_clear_capture",
    }
}

/// JSON null becomes a truthy null userdata via `lua.to_value`; map it to real nil.
/// Fills in any snapshot field the API reads, so `sdk.snapshot` accessors are
/// total.
///
/// `on_load`, `on_unload`, `on_settings` and `on_event` can all run before the
/// host has ever built a snapshot — `Manager::start` passes whatever it has,
/// which on the first frame is `Value::Null`. A nil `sdk.snapshot` turns every
/// reader (`sdk.player.attached`, `sdk.input.down`, ...) into a hard Lua error
/// during startup, which is a host-lifecycle detail no mod can be expected to
/// defend against. Missing subtables are filled individually so a partial
/// snapshot cannot throw either.
fn complete(snapshot: &Value) -> Value {
    let mut value = snapshot.clone();
    let Some(object) = value.as_object_mut() else {
        return default_snapshot();
    };
    if let Value::Object(defaults) = default_snapshot() {
        for (key, default) in defaults {
            let missing = object.get(&key).is_none_or(Value::is_null);
            // `attach`, `camera` and `detach_error` are meaningfully null, so a
            // null default keeps them absent rather than inventing a value.
            if missing && !default.is_null() {
                object.insert(key, default);
            }
        }
    }
    value
}

fn default_snapshot() -> Value {
    serde_json::json!({
        "player": {
            "position": [0.0, 0.0, 0.0],
            "velocity": [0.0, 0.0, 0.0],
            "angvel": [0.0, 0.0, 0.0],
            "forward": [0.0, 0.0, 1.0],
            "heading": 0.0,
            "rotation": [0.0, 0.0, 0.0, 1.0],
            "speed": 0.0,
            "on_board": false,
            "state": 0,
            "category": 0,
            "filtered": 0,
            "mode": "ground",
            "grind": Value::Null,
            "bailing": false,
            "trick": "",
            "trick_seq": 0,
            "landing_seq": 0, "landed_trick": "", "bail_seq": 0,
            "new_trick": false,
            "modified_trick": false,
            "close_tricks": false,
            "sequence": false,
            "score": 0,
            "line": 0,
            "multiplier": 1.0,
            "line_time": 0,
            "clean": false,
            "sketchy": false,
            "switch": false,
            "fakie": false,
            "nollie": false,
            "intents": {},
        },
        "skaters": {},
        "player_physics": {"contacts":[],"joints":[],"parts":[],"dt":0,"ragdoll":false,"partial_ragdoll":false},
        "session": {
            "active": false,
            "local_id": "0",
            "is_host": true,
            "authority": "0",
            "players": ["0"]
        },
        "volumes": {},
        "attach": Value::Null,
        "detach_error": Value::Null,
        "detach_pending": false,
        "map": {"name": "", "generation": 0},
        "tick": 0,
        "keys": {},
        // 18 gameplay actions, IDs 64..=81.
        "actions": vec![0.0f32; 18],
        "pad": {"buttons": 0, "triggers": [0.0, 0.0], "left": [0.0, 0.0], "right": [0.0, 0.0]},
        "paused": false,
        "replay": false,
        "camera": Value::Null,
        "physics": {"bodies": {}, "contacts": []},
        "network": {
            "active": false,
            "local_id": "0",
            "is_host": true,
            "host_id": "0",
            "players": ["0"],
            "states": {},
            "status": "",
        },
    })
}

fn json_to_lua(lua: &Lua, value: &Value) -> mlua::Result<mlua::Value> {
    if value.is_null() {
        return Ok(mlua::Value::Nil);
    }
    if let Some(obj) = value.as_object() {
        let t = lua.create_table_with_capacity(0, obj.len())?;
        for (k, v) in obj {
            t.set(k.clone(), json_to_lua(lua, v)?)?;
        }
        return Ok(mlua::Value::Table(t));
    }
    if let Some(arr) = value.as_array() {
        let t = lua.create_table_with_capacity(arr.len(), 0)?;
        for (i, v) in arr.iter().enumerate() {
            t.set(i + 1, json_to_lua(lua, v)?)?;
        }
        return Ok(mlua::Value::Table(t));
    }
    lua.to_value(value)
}

// Only a root field that Lua reads is converted. Returned field tables are
// ordinary complete Lua tables, preserving length, pairs and serde behavior.
#[cfg(test)]
fn lazy_snapshot(lua:&Lua, snapshot:Arc<Value>, physics:Option<Value>)->mlua::Result<Table> {
    lazy_snapshot_fields(lua, snapshot, physics, crate::SnapshotFields::default())
}

fn lazy_snapshot_fields(lua:&Lua, snapshot:Arc<Value>, physics:Option<Value>, fields:crate::SnapshotFields)->mlua::Result<Table> {
    let defaults=std::sync::LazyLock::force(&SNAPSHOT_DEFAULTS);
    let table=lua.create_table()?;
    if let Some(physics)=physics {table.raw_set("physics",json_to_lua(lua,&physics)?)?;}
    let meta=lua.create_table()?;
    let index_snapshot=snapshot.clone();
    let index_fields=fields.clone();
    meta.set("__index",lua.create_function(move |lua,(table,key):(Table,String)| {
        let value=index_fields.get(&key).map(|v|v.as_ref()).or_else(||index_snapshot.get(&key)).filter(|v|!v.is_null()).or_else(||SNAPSHOT_DEFAULTS.get(&key));
        let value=value.map_or(Ok(mlua::Value::Nil),|v|json_to_lua(lua,v))?;
        table.raw_set(key,value.clone())?;
        Ok(value)
    })?)?;
    // pairs(snapshot) explicitly requests every root field.
    let next:mlua::Function=lua.globals().get("next")?;
    meta.set("__pairs",lua.create_function(move |_,table:Table| {
        // Most callbacks read a few named fields. Build the root-key union
        // only when iteration actually requests the complete snapshot.
        let keys=snapshot.as_object().into_iter().flat_map(|o|o.keys())
            .chain(fields.keys()).chain(defaults.as_object().unwrap().keys())
            .collect::<std::collections::BTreeSet<_>>();
        for key in keys {let _:mlua::Value=table.get(key.as_str())?;}
        Ok((next.clone(),table,mlua::Value::Nil))
    })?)?;
    table.set_metatable(Some(meta))?;
    Ok(table)
}
static SNAPSHOT_DEFAULTS:std::sync::LazyLock<Value>=std::sync::LazyLock::new(default_snapshot);

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
    timers_due: mlua::Function,
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
            // Snapshot closures own native allocations which Lua's heap counter
            // cannot see. Keep finalization batches small and advance collection
            // after dispatch, instead of retaining hundreds of old native frames.
            lua.gc_set_mode(mlua::state::GcMode::Incremental(
                mlua::state::GcIncParams::default().step_size(10),
            ));
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
            capabilities.set("skater", 4)?;
            capabilities.set("menus", 3)?;
            capabilities.set("player_physics", 2)?;
            capabilities.set("engine_access", 1)?;
            capabilities.set("command_results", 1)?;
            capabilities.set("native_bodies", 1)?;
            capabilities.set("input_override", 1)?;
            capabilities.set("player_overlap", 1)?;
            capabilities.set("landed_details", 1)?;
            capabilities.set("camera", 3)?;
            capabilities.set("player_control", 1)?;
            capabilities.set("session", 1)?;
            capabilities.set("volumes", 1)?;
            capabilities.set("capture", 1)?;
            capabilities.set("multiplayer_debug", 1)?;
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
            sdk.set("snapshot", json_to_lua(&lua, &complete(snapshot))?)?;
            lua.globals().set("sdk", sdk)?;
            lua.load(include_str!("api.lua"))
                .set_name("@skate-sdk-2")
                .exec()?;
            let sdk: Table = lua.globals().get("sdk")?;
            let advance = sdk.get("_advance")?;
            sdk.set("_advance", mlua::Value::Nil)?;
            let timers_due = sdk.get("_timers_due")?;
            sdk.set("_timers_due", mlua::Value::Nil)?;
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
                    "on_ui_update",
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
                timers_due,
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

    #[cfg(test)]
    pub fn call(
        &mut self,
        name: &str,
        payload: Value,
        snapshot: &Value,
    ) -> Result<Vec<Command>, String> {
        self.call_shared(name,payload,&Arc::new(snapshot.clone()),None,&crate::SnapshotFields::default())
    }

    pub fn call_shared(&mut self,name:&str,payload:Value,snapshot:&Arc<Value>,physics:Option<Value>,fields:&crate::SnapshotFields)->Result<Vec<Command>,String> {
        self.budget.store(LUA_BUDGET_UNITS, Ordering::Relaxed);
        let invoke = || -> mlua::Result<bool> {
            let callback=self.callbacks.get::<Option<mlua::Function>>(name)?;
            if callback.is_none() {
                if name != "on_update" { return Ok(false); }
                let dt = payload["dt"].as_f64().unwrap_or(0.);
                // Advance the clock even for mods with no update callback. Only
                // a due timer can observe this frame, so otherwise no snapshot
                // or native closures need to be allocated at render frequency.
                if !self.timers_due.call::<bool>(dt)? {
                    self.advance.call::<()>(dt)?;
                    return Ok(true);
                }
            }
            let sdk = self.lua.globals().get::<Table>("sdk")?;
            let rig_source = snapshot.clone();
            let rig_fields = fields.clone();
            sdk.set("_rig_snapshot", self.lua.create_function(move |lua, fields: Vec<String>| {
                let result = lua.create_table_with_capacity(0, fields.len())?;
                let rig = rig_fields.get("player_physics").map(|v|v.as_ref()).or_else(||rig_source.get("player_physics")).filter(|v| !v.is_null())
                    .unwrap_or(&SNAPSHOT_DEFAULTS["player_physics"]);
                for field in fields {
                    if let Some(value) = rig.get(&field) {
                        result.raw_set(field, json_to_lua(lua, value)?)?;
                    }
                }
                Ok(result)
            })?)?;
            sdk.set("snapshot", lazy_snapshot_fields(&self.lua,snapshot.clone(),physics,fields.clone())?)?;
            if let Some(f) = callback {
                f.call::<()>(self.lua.to_value(&payload)?)?;
            }
            if name == "on_update" {
                self.advance
                    .call::<()>(payload["dt"].as_f64().unwrap_or(0.))?;
            }
            Ok(true)
        };
        let result = invoke().and_then(|ran| {
            if ran { self.lua.gc_step().map(|_| ()) } else { Ok(()) }
        });
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
                sdk.ui.multiplayer_debug("replication", "Ready")
                sdk.ui.multiplayer_debug("replication", "")
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
        assert_eq!(out.len(),7);
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
        assert!(matches!(&out[5],Command::MultiplayerDebug{key,text} if key=="replication" && text=="Ready"));
        assert!(matches!(&out[6],Command::MultiplayerDebug{text,..} if text.is_empty()));
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

    /// `Manager::start` runs `on_load` with whatever snapshot it has, which on
    /// the first frame is `Value::Null`. Every snapshot reader must still work:
    /// mods legitimately check attachment state during startup cleanup.
    #[test]
    fn snapshot_readers_work_before_the_host_has_built_a_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "skate-mods-null-snapshot-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("main.lua"),
            r#"
            return {on_load=function()
                assert(sdk.player.attached() == nil, 'attached')
                assert(sdk.player.detaching() == false, 'detaching')
                assert(sdk.player.detach_error() == nil, 'detach_error')
                assert(type(sdk.player.read()) == 'table', 'read')
                assert(type(sdk.player.skaters()) == 'table', 'skaters')
                assert(sdk.player.read().trick == '', 'trick')
                assert(sdk.input.down('KeyW') == false, 'down')
                assert(sdk.input.action(64) == 0.0, 'action')
                assert(type(sdk.input.pad()) == 'table', 'pad')
                assert(type(sdk.net.info()) == 'table', 'net')
            end}
        "#,
        )
        .unwrap();
        let manifest: Manifest = serde_json::from_value(json!({"id":"tests.snapshot", "api":2,
            "name":"Null snapshot", "version":"1.0.0", "author":"test", "description":"test",
            "entry":"main.lua", "settings":{}}))
        .unwrap();
        manifest.validate().unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        vm.call("on_load", Value::Null, &Value::Null)
            .expect("snapshot readers must not fault on a null snapshot");
        drop(vm);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A real snapshot must survive completion untouched, so the default cannot
    /// mask live host state.
    #[test]
    fn completion_preserves_host_supplied_fields() {
        let live = json!({
            "attach": {"body": "chassis", "owner": "examples.skyline"},
            "tick": 581,
            "keys": {"KeyW": true},
        });
        let filled = complete(&live);
        assert_eq!(filled["attach"]["body"], "chassis");
        assert_eq!(filled["tick"], 581);
        assert_eq!(filled["keys"]["KeyW"], true);
        // Absent fields gain defaults; meaningfully-null ones stay null.
        assert_eq!(filled["detach_pending"], false);
        assert_eq!(filled["physics"]["bodies"], json!({}));
        assert!(filled["camera"].is_null());
        assert_eq!(complete(&Value::Null)["paused"], false);
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

#[cfg(test)]
mod multiplayer_debug_tests {
    use super::Command;
    #[test]
    fn multiplayer_debug_reports_are_bounded_and_clearable() {
        for text in [String::new(), "Ready".into(), "x".repeat(1024)] {
            assert!(Command::MultiplayerDebug { key: "replication".into(), text }.validate());
        }
        assert!(!Command::MultiplayerDebug { key: "replication".into(), text: "x".repeat(1025) }.validate());
        assert!(!Command::MultiplayerDebug { key: "../bad".into(), text: "Ready".into() }.validate());
    }
}

#[cfg(test)]
mod snapshot_performance_tests {
    use super::*;
    #[test]
    fn timer_only_mods_observe_fresh_snapshots_and_keep_callback_order() {
        let root=std::env::temp_dir().join(format!("skate-timer-snapshot-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"),r#"
            return {on_load=function()
                sdk.time.after('first',0.02,function()
                    assert(sdk.snapshot.tick==2 and sdk.time.elapsed==0.02)
                    sdk.ui.text('timer','first')
                    sdk.time.after('next',0,function()
                        assert(sdk.snapshot.tick==3)
                        sdk.ui.text('timer','next')
                    end)
                end)
            end}
        "#).unwrap();
        let manifest:Manifest=serde_json::from_value(serde_json::json!({
            "id":"tests.timer-snapshot","api":2,"name":"Timer snapshots","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&Value::Null).unwrap();
        vm.call("on_load",serde_json::json!({}),&Value::Null).unwrap();
        for (tick,expected) in [(1,None),(2,Some("first")),(3,Some("next")),(4,None)] {
            let commands=vm.call("on_update",serde_json::json!({"dt":0.01}),
                &serde_json::json!({"tick":tick})).unwrap();
            if let Some(expected)=expected {
                assert!(matches!(&commands[..], [Command::Overlay{text,..}] if text==expected));
            } else { assert!(commands.is_empty()); }
        }
        drop(vm);
        std::fs::remove_file(root.join("main.lua")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn incremental_collection_releases_native_frames_but_preserves_retained_snapshots() {
        let root=std::env::temp_dir().join(format!("skate-snapshot-gc-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"),r#"
            local first
            return {on_fixed_update=function(e)
                if not first then first=sdk.snapshot end
                if e.tick==1024 then
                    -- Read a field for the first time after many collections.
                    assert(first.tick==1 and #first.player_physics.joints==22)
                end
            end}
        "#).unwrap();
        let manifest:Manifest=serde_json::from_value(serde_json::json!({
            "id":"tests.snapshot-gc","api":2,"name":"Snapshot collection","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&Value::Null).unwrap();
        let mut frames=Vec::new();
        for tick in 1..=1024 {
            let mut snapshot=fixture();
            Arc::make_mut(&mut snapshot)["tick"]=serde_json::json!(tick);
            frames.push(Arc::downgrade(&snapshot));
            vm.call_shared("on_fixed_update",serde_json::json!({"tick":tick}),
                &snapshot,None,&crate::SnapshotFields::default()).unwrap();
        }
        assert!(frames[0].upgrade().is_some(),"Lua retained the first snapshot");
        assert!(frames[1..512].iter().all(|f| f.upgrade().is_none()),
            "discarded native snapshots must be reclaimed during dispatch");
        drop(vm);
        assert!(frames.iter().all(|f|f.upgrade().is_none()));
        std::fs::remove_file(root.join("main.lua")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn shared_native_fields_support_projected_full_and_retained_reads() {
        let root=std::env::temp_dir().join(format!("skate-shared-fields-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"),r#"
            local old
            return {on_update=function()
                local current=sdk.rig.read({'tick'})
                assert(current.tick==sdk.snapshot.tick and current.joints==nil)
                assert(rawget(sdk.snapshot,'player_physics')==nil)
                if old then assert(old.tick<current.tick) end
                old=current
                local full=sdk.rig.read()
                assert(#full.joints==22 and #full.joints[1].load.solver_words==96)
                assert(full.joints[1].name=='JOINT_LEFT_ARM')
                full.joints[1].name='mod-local mutation'
            end}
        "#).unwrap();
        let manifest:Manifest=serde_json::from_value(serde_json::json!({
            "id":"tests.shared-fields","api":2,"name":"Shared fields","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&Value::Null).unwrap();
        for tick in [1,2] {
            let mut rig=fixture()["player_physics"].clone();rig["tick"]=serde_json::json!(tick);
            let fields=Arc::new(BTreeMap::from([("player_physics".into(),Arc::new(rig))]));
            vm.call_shared("on_update",serde_json::json!({"dt":0.01}),
                &Arc::new(serde_json::json!({"tick":tick})),None,&fields).unwrap();
            assert_eq!(fields["player_physics"]["joints"][0]["name"],"JOINT_LEFT_ARM");
        }
        drop(vm);std::fs::remove_file(root.join("main.lua")).unwrap();std::fs::remove_dir(root).unwrap();
    }
    fn fixture()->Arc<Value> {
        let joints=(0..22).map(|i|serde_json::json!({"index":i,"name":"JOINT_LEFT_ARM","parameters":vec![0;16],"frames":vec![0;20],"load":{"solver_words":vec![0;96],"force":[1,2,3]}})).collect::<Vec<_>>();
        Arc::new(serde_json::json!({"tick":1,"player":{"score":123,"position":[1,2,3]},"player_physics":{"joints":joints,"contacts":(0..64).map(|i|serde_json::json!({"id":i,"point":[1,2,3],"force":[1,2,3],"impulse":[1,2,3],"normal":[0,1,0],"a":{"kind":"skater","index":1},"b":{"kind":"world"}})).collect::<Vec<_>>()},"engine":{"graphs":{"states":vec![0;1500]}}}))
    }
    #[test]
    fn lazy_fields_are_complete_isolated_and_retained_across_callbacks() {
        let lua=Lua::new();let snapshot=fixture();
        let a=lazy_snapshot(&lua,snapshot.clone(),Some(serde_json::json!({"bodies":{"owned":{}}}))).unwrap();
        lua.globals().set("a",a.clone()).unwrap();
        lua.load("assert(rawget(a,'player_physics')==nil); assert(a.player.score==123); a.player.score=5; assert(rawget(a,'player_physics')==nil); assert(a.physics.bodies.owned)").exec().unwrap();
        let b=lazy_snapshot(&lua,snapshot,None).unwrap();lua.globals().set("b",b).unwrap();
        lua.load("assert(b.player.score==123); assert(a.player.score==5); local n=0;for k,v in pairs(a) do n=n+1 end;assert(n>10);assert(#a.player_physics.joints==22);assert(#a.player_physics.joints[1].load.solver_words==96)").exec().unwrap();
    }
    #[test]
    #[ignore="headless marshaling benchmark; not an FPS claim"]
    fn compare_eager_and_demand_driven_snapshot_cost() {
        let snapshot=fixture();let lua=Lua::new();let reads:mlua::Function=lua.load("return function(s) return s.player.score end").eval().unwrap();
        let iterations=120;let mods=6;
        let begin=std::time::Instant::now();
        for _ in 0..iterations {for _ in 0..mods {for _ in 0..3 {
            let full=json_to_lua(&lua,&complete(&snapshot)).unwrap();std::hint::black_box(reads.call::<i64>(full).unwrap());
        }}}
        let eager=begin.elapsed();lua.gc_collect().unwrap();
        let begin=std::time::Instant::now();
        for _ in 0..iterations {for _ in 0..mods {
            // Missing UI callback does no conversion; update/fixed read only player.
            for _ in 0..2 {let lazy=lazy_snapshot(&lua,snapshot.clone(),None).unwrap();std::hint::black_box(reads.call::<i64>(lazy).unwrap());}
        }}
        let lazy=begin.elapsed();
        lua.gc_collect().unwrap();
        let rig_reads:mlua::Function=lua.load("return function(s) local rig=s.player_physics; local n=#rig.contacts; for _,j in ipairs(rig.joints) do n=n+j.index end; return n end").eval().unwrap();
        let begin=std::time::Instant::now();
        for _ in 0..iterations {for mod_index in 0..mods {
            for callback in 0..2 {
                let lazy=lazy_snapshot(&lua,snapshot.clone(),None).unwrap();
                std::hint::black_box(reads.call::<i64>(lazy.clone()).unwrap());
                if mod_index==0 && callback==1 {std::hint::black_box(rig_reads.call::<i64>(lazy).unwrap());}
            }
        }}
        eprintln!("SNAPSHOT_RIG_BENCH six_mods_one_full_rig_reader_ms_per_frame={:.3}",begin.elapsed().as_secs_f64()*1000./iterations as f64);
        eprintln!("SNAPSHOT_BENCH bytes={} mods={mods} iterations={iterations} eager_ms_per_frame={:.3} lazy_ms_per_frame={:.3} ratio={:.1}",serde_json::to_vec(&*snapshot).unwrap().len(),eager.as_secs_f64()*1000./iterations as f64,lazy.as_secs_f64()*1000./iterations as f64,eager.as_secs_f64()/lazy.as_secs_f64());
    }
}
