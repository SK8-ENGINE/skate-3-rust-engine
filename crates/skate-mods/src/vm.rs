use crate::{lua_list::{list, opt_list}, read_bounded, Manifest};
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

/// `sdk.triggers.box`: a mod-owned trigger volume (an oriented box) that
/// takes part in the engine's enter/exit events like a map volume.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerBoxOptions {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
    #[serde(default)]
    pub rotation: Option<[f32; 4]>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
}

impl TriggerBoxOptions {
    pub fn validate(&self) -> bool {
        self.center.iter().all(|v| v.is_finite() && v.abs() <= 100_000.)
            && self.half_extents.iter().all(|v| v.is_finite() && *v >= 0.005 && *v <= 5_000.)
            && self.rotation.as_ref().is_none_or(crate::scene::valid_quaternion)
            && self.name.as_ref().is_none_or(|n| n.len() <= 128 && !n.chars().any(char::is_control))
            && self.group.as_deref().is_none_or(|g| matches!(g, "challenge" | "stairs" | "camera"))
    }
}

/// `sdk.triggers.track`: follow one of the mod's physics bodies. The body's
/// position is the top point of its query cylinder (retail shape by default).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerTrackOptions {
    #[serde(default)]
    pub radius: Option<f32>,
    /// Distance from the top point to the bottom (retail "length" span).
    #[serde(default)]
    pub length: Option<f32>,
}

impl TriggerTrackOptions {
    pub fn validate(&self) -> bool {
        self.radius.is_none_or(|r| r.is_finite() && (0. ..=50.).contains(&r))
            && self.length.is_none_or(|l| l.is_finite() && (0. ..=100.).contains(&l))
    }
}

/// `sdk.triggers.configure`: the query-cylinder constants (retail: radius
/// 0.34, length scale 0.5, length pad 0.05, foot pad 0.02). Omitted fields keep
/// the retail value; `nil` options restore all of them.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerShapeOptions {
    #[serde(default)]
    pub radius: Option<f32>,
    #[serde(default)]
    pub length_scale: Option<f32>,
    #[serde(default)]
    pub length_pad: Option<f32>,
    #[serde(default)]
    pub foot_pad: Option<f32>,
}

impl TriggerShapeOptions {
    pub fn validate(&self) -> bool {
        [self.radius, self.length_scale, self.length_pad, self.foot_pad]
            .iter().flatten().all(|v| v.is_finite() && v.abs() <= 100.)
            && self.radius.is_none_or(|r| r >= 0.)
    }
}

fn valid_trigger_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
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

/// Stock camera shot names are lower-case collection keys (`bl_chase`, `high_grind`, …).
fn valid_shot_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
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
    /// Audio extension 1: play one of the game's front-end sounds (a retail `fe` record by name,
    /// e.g. `cellphone_place_marker`, `challenge_count_go`), as the game's UI does.
    AudioFrontend { name: String },
    /// Audio extension 1: the game's teleport effect amount (0..=1) for the next few frames: the
    /// screen static and the skater's teleport crackle (retail's `cMsgTeleportEffectAmount`, which the
    /// session marker's Go To Marker hold ramps 0 → 1). Lapses unless sent again each frame.
    AudioTeleportEffect { amount: f32 },
    /// Audio extension 2: a post to a retail class under a key (`sdk.audio.post`); a key's post
    /// replaces its last one.
    AudioPost {
        key: String,
        class: String,
        #[serde(default, deserialize_with = "list")]
        words: Vec<i32>,
    },
    AudioRedeliver {
        key: String,
        #[serde(default, deserialize_with = "list")]
        words: Vec<i32>,
    },
    AudioRelease { key: String },
    /// Set a retail global; no `value` restores the value before this mod's first write.
    AudioSetGlobal {
        name: String,
        #[serde(default)]
        value: Option<i32>,
    },
    /// Replace this mod's watch lists (globals, MixMap outputs) read in `sdk.snapshot.audio`.
    AudioWatch {
        #[serde(default, deserialize_with = "list")]
        globals: Vec<String>,
        #[serde(default, deserialize_with = "list")]
        mixmap: Vec<crate::audio::MixMapKey>,
    },
    /// Audio events extension 1: receive this mod's audio event rows (`tags` empty = every row;
    /// no `tags` = stop).
    AudioSubscribe {
        #[serde(default, deserialize_with = "opt_list")]
        tags: Option<Vec<String>>,
    },
    /// Audio events extension 2: a mute / replace / layer rule under a key (`rule` absent = remove).
    AudioRule {
        key: String,
        #[serde(default)]
        rule: Option<crate::audio_rules::Rule>,
    },
    /// Audio tuning extension 1: patch a typed tuning domain while the mod runs (`patch` absent =
    /// restore this mod's patch of the domain).
    AudioSetTuning {
        domain: String,
        #[serde(default)]
        patch: Option<Value>,
    },
    /// World tuning extension 1: patch a typed living-world / props / carry domain while the mod
    /// runs (`patch` absent = restore this mod's patch of the domain).
    WorldSetTuning {
        domain: String,
        #[serde(default)]
        patch: Option<Value>,
    },
    /// Reset moved objects (doc 27, Object Dropper and reset): every dynamic prop away from its
    /// authored pose goes back to it (retail cMsgResetDMO per object).
    WorldResetMovedProps {},
    /// Reset one dynamic prop (stable map id) to its authored pose (retail cMsgResetDMO).
    WorldResetProp { id: u32 },
    /// Upright one dynamic prop (stable map id): retail cMsgUprightDMO, the 2 s self-righting
    /// window (doc 27, Upright).
    WorldUprightProp { id: u32 },
    /// Create a dynamic prop mid-game (the engine path behind released hand props, doc 27
    /// "Props created mid-game"): a copy of map prop `from` (its model, collision and physics
    /// block) at `position`, turned `yaw` radians about world up, moving at `velocity` (m/s) and
    /// spinning at `spin` (rad/s). Keyed per mod (`key` again replaces it); removed on disable.
    WorldSpawnProp {
        key: String,
        from: u32,
        position: [f32; 3],
        #[serde(default)]
        yaw: f32,
        #[serde(default)]
        velocity: [f32; 3],
        #[serde(default)]
        spin: [f32; 3],
    },
    /// Remove a prop this mod created with `WorldSpawnProp`.
    WorldRemoveProp { key: String },
    /// Audio extension 4 (doc 16 L2): write one input of a retail MixMap controller (`value` absent
    /// = release it: the input gets back the value before this mod's first write).
    AudioSetMixmapInput {
        slot: String,
        object: u32,
        instance: u32,
        input: u32,
        #[serde(default)]
        value: Option<f64>,
        /// The value is an f32 input (the distance inputs), not an integer word.
        #[serde(default)]
        float: bool,
    },
    /// Audio extension 4 (doc 16 L5): seed the audio random state (`seed` absent = release it).
    AudioSeed {
        #[serde(default)]
        seed: Option<u64>,
    },
    /// World audio extension 1: publish a traffic vehicle / ped / skater to the retail world audio.
    WorldAudioSpawn {
        key: String,
        object: crate::world_audio::ObjectKind,
        #[serde(default)]
        options: crate::world_audio::WorldAudioOptions,
    },
    WorldAudioUpdate {
        key: String,
        #[serde(default)]
        options: crate::world_audio::WorldAudioOptions,
    },
    WorldAudioEvent {
        key: String,
        event: String,
        #[serde(default)]
        options: crate::world_audio::WorldAudioEventOptions,
    },
    WorldAudioRemove { key: String },
    /// World audio: name the announcer character (35 / 36; nil = none: free skate's silence).
    WorldAudioAnnouncer {
        #[serde(default)]
        character: Option<u32>,
    },
    /// World audio: ask the announcer channel for an event (id or name).
    WorldAudioAnnounce {
        event: Value,
        #[serde(default)]
        options: crate::world_audio::AnnounceOptions,
    },
    /// World audio: tune or disable retail's car alarm trigger (None = back to retail's).
    WorldAudioAlarmRule {
        #[serde(default)]
        options: Option<crate::world_audio::AlarmRuleOptions>,
    },
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
        #[serde(default, deserialize_with = "list")]
        deform_nodes: Vec<String>,
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
    /// Forces the retail Camera Angle (`low` / `high`); nil hands it back to the player's setting.
    CameraAngle {
        #[serde(default)]
        angle: Option<crate::presentation::CameraAngle>,
    },
    /// Replaces stock values of one camera shot; nil restores the stock shot.
    CameraShotTune {
        shot: String,
        #[serde(default)]
        patch: Option<crate::presentation::CameraShotTuning>,
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
    TriggerBox {
        key: String,
        options: TriggerBoxOptions,
    },
    TriggerRemove {
        key: String,
    },
    TriggerEnable {
        id: String,
        enabled: bool,
    },
    TriggerTrack {
        key: String,
        #[serde(default)]
        options: Option<TriggerTrackOptions>,
    },
    TriggerUntrack {
        key: String,
    },
    TriggerConfigure {
        #[serde(default)]
        options: Option<TriggerShapeOptions>,
    },
    CameraCapture {
        key: String,
        options: CaptureOptions,
    },
    /// Retail menus extension 1 (`sdk.menus`): add or change a pause-menu category.
    MenuCategory { id: String, #[serde(default)] options: crate::menus::EntryOptions },
    /// Add or change a pause-menu item.
    MenuItem { id: String, #[serde(default)] options: crate::menus::EntryOptions },
    /// Add or change a Game Settings row.
    MenuSetting { id: String, #[serde(default)] options: crate::menus::EntryOptions },
    /// Remove an entry from the menus (`modes` empty = everywhere) until the mod stops.
    MenuHide {
        entry: String,
        id: String,
        #[serde(default, deserialize_with = "list")]
        modes: Vec<String>,
    },
    /// Handle an entry: select / highlight events, a bound value, rules, a confirmation.
    MenuHandle { entry: String, id: String, #[serde(default)] options: crate::menus::HandleOptions },
    /// Drop this mod's handler and rules for an entry.
    MenuUnhandle { entry: String, id: String },
    /// Write a settings row's value (through its handler: engine or mod).
    MenuSetValue { id: String, value: crate::menus::MenuValue },
    /// Supply or replace a front-end APT movie by name (retail import path, e.g.
    /// `source/controls/panel`) with a player-format JSON file from the mod's folder.
    MenuMovie { name: String, path: String },
    /// Replace one bitmap of a front-end APT movie by its stable id (movie import name + bitmap
    /// character id) with a PNG from the mod's folder.
    MenuBitmap { movie: String, bitmap: i32, path: String },
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
            Self::EngineInspect {system} => matches!(system.as_str(),"graphs"|"scoring"|"audio_catalog") || crate::audio_tuning::valid_inspect(system) || crate::world_tuning::valid_inspect(system),
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
            Self::AudioFrontend { name } => crate::audio::valid_frontend_name(name),
            Self::AudioTeleportEffect { amount } => amount.is_finite() && (0.0..=1.0).contains(amount),
            Self::AudioPost { key, class, words } => crate::schema::valid_id(key) && crate::audio::valid_symbol(class) && words.len() <= crate::audio::MAX_WORDS,
            Self::AudioRedeliver { key, words } => crate::schema::valid_id(key) && words.len() <= crate::audio::MAX_WORDS,
            Self::AudioRelease { key } => crate::schema::valid_id(key),
            Self::AudioSubscribe { tags } => tags.as_ref().is_none_or(|t| t.len() <= 16 && t.iter().all(|x| crate::audio::valid_symbol(x))),
            Self::AudioSetGlobal { name, .. } => crate::audio::valid_symbol(name),
            Self::AudioRule { key, rule } => crate::schema::valid_id(key) && rule.as_ref().is_none_or(crate::audio_rules::Rule::validate),
            Self::AudioSetMixmapInput { slot, object, instance, input, value, float } => crate::audio::valid_symbol(slot) && *object <= 127 && *instance <= 31 && *input <= 15
                && value.is_none_or(|v| v.is_finite() && (*float || (v.fract() == 0.0 && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&v)))),
            Self::AudioSeed { .. } => true,
            Self::WorldResetMovedProps {} => true,
            Self::WorldResetProp { .. } => true,
            Self::WorldUprightProp { .. } => true,
            Self::WorldSpawnProp { key, position, yaw, velocity, spin, .. } => crate::schema::valid_id(key)
                && position.iter().all(|v| v.is_finite() && v.abs() <= 100_000.)
                && yaw.is_finite() && yaw.abs() <= 1000.
                && velocity.iter().chain(spin).all(|v| v.is_finite() && v.abs() <= 200.),
            Self::WorldRemoveProp { key } => crate::schema::valid_id(key),
            Self::WorldSetTuning { domain, patch } => crate::world_tuning::DOMAINS.contains(&domain.as_str()) && patch.as_ref().is_none_or(|p| crate::world_tuning::valid_patch(domain, p)),
            Self::AudioSetTuning { domain, patch } => crate::audio_tuning::DOMAINS.contains(&domain.as_str()) && patch.as_ref().is_none_or(|p| crate::audio_tuning::valid_patch(domain, p)),
            Self::AudioWatch { globals, mixmap } => globals.len() <= crate::audio::MAX_WATCH && globals.iter().all(|g| crate::audio::valid_symbol(g))
                && mixmap.len() <= crate::audio::MAX_WATCH && mixmap.iter().all(crate::audio::MixMapKey::validate),
            Self::WorldAudioSpawn { key, object, options } => crate::schema::valid_id(key) && options.validate_for(*object) && options.complete_for(*object),
            Self::WorldAudioUpdate { key, options } => crate::schema::valid_id(key) && options.validate() && options.source.is_none() && options.slots.is_none(),
            Self::WorldAudioEvent { key, event, options } => crate::schema::valid_id(key) && options.validate(event),
            Self::WorldAudioRemove { key } => crate::schema::valid_id(key),
            Self::WorldAudioAnnouncer { character } => crate::world_audio::valid_announcer_character(*character),
            Self::WorldAudioAnnounce { event, options } => crate::world_audio::valid_announcer_event(event) && options.validate(),
            Self::WorldAudioAlarmRule { options } => options.as_ref().is_none_or(crate::world_audio::AlarmRuleOptions::validate),
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
                crate::schema::valid_id(key) && body.deformation.as_ref().is_none_or(|o|o.validate().is_ok()) && serde_json::to_value(body).is_ok()
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
                deform_nodes,
                ..
            } => {
                crate::schema::valid_id(key)
                    && crate::scene::valid_asset(path)
                    && deform_nodes.len()<=64 && deform_nodes.iter().all(|s|crate::scene::valid_node(s))
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
            Self::CameraAngle { .. } => true,
            Self::CameraShotTune { shot, patch } => valid_shot_name(shot)
                && patch.as_ref().is_none_or(|p| p.validate()),
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
            Self::TriggerBox { key, options } => crate::schema::valid_id(key) && options.validate(),
            Self::TriggerRemove { key } | Self::TriggerUntrack { key } => crate::schema::valid_id(key),
            Self::TriggerEnable { id, .. } => valid_trigger_id(id),
            Self::TriggerTrack { key, options } => {
                crate::schema::valid_id(key) && options.as_ref().is_none_or(TriggerTrackOptions::validate)
            }
            Self::TriggerConfigure { options } => options.as_ref().is_none_or(TriggerShapeOptions::validate),
            Self::CameraCapture { key, options } => {
                crate::schema::valid_id(key) && options.validate()
            }
            Self::MenuCategory { id, options } | Self::MenuItem { id, options } | Self::MenuSetting { id, options } => crate::menus::valid_menu_id(id) && options.validate(),
            Self::MenuHide { entry, id, modes } => crate::menus::valid_entry_kind(entry) && crate::menus::valid_menu_id(id) && modes.len() <= 16 && modes.iter().all(|m| crate::menus::valid_menu_id(m)),
            Self::MenuHandle { entry, id, options } => crate::menus::valid_entry_kind(entry) && crate::menus::valid_menu_id(id) && options.validate(),
            Self::MenuUnhandle { entry, id } => crate::menus::valid_entry_kind(entry) && crate::menus::valid_menu_id(id),
            Self::MenuSetValue { id, value } => crate::menus::valid_menu_id(id) && value.validate(),
            Self::MenuMovie { name, path } => !name.is_empty() && name.len() <= 160 && crate::audio_content::valid_content_path(path, &["json"]),
            Self::MenuBitmap { movie, bitmap, path } => !movie.is_empty() && movie.len() <= 160 && (0..=0xFFFF).contains(bitmap) && crate::graphics_dynamic::valid_texture_path(path),
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
        Command::AudioFrontend { .. } => "audio_frontend",
        Command::AudioTeleportEffect { .. } => "audio_teleport_effect",
        Command::AudioPost { .. } => "audio_post",
        Command::AudioRedeliver { .. } => "audio_redeliver",
        Command::AudioRelease { .. } => "audio_release",
        Command::AudioSetGlobal { .. } => "audio_set_global",
        Command::AudioWatch { .. } => "audio_watch",
        Command::AudioSubscribe { .. } => "audio_subscribe",
        Command::AudioSetTuning { .. } => "audio_set_tuning",
        Command::AudioRule { .. } => "audio_rule",
        Command::AudioSetMixmapInput { .. } => "audio_set_mixmap_input",
        Command::AudioSeed { .. } => "audio_seed",
        Command::WorldSetTuning { .. } => "world_set_tuning",
        Command::WorldResetMovedProps {} => "world_reset_moved_props",
        Command::WorldResetProp { .. } => "world_reset_prop",
        Command::WorldUprightProp { .. } => "world_upright_prop",
        Command::WorldSpawnProp { .. } => "world_spawn_prop",
        Command::WorldRemoveProp { .. } => "world_remove_prop",
        Command::WorldAudioSpawn { .. } => "world_audio_spawn",
        Command::WorldAudioUpdate { .. } => "world_audio_update",
        Command::WorldAudioEvent { .. } => "world_audio_event",
        Command::WorldAudioRemove { .. } => "world_audio_remove",
        Command::WorldAudioAnnouncer { .. } => "world_audio_announcer",
        Command::WorldAudioAnnounce { .. } => "world_audio_announce",
        Command::WorldAudioAlarmRule { .. } => "world_audio_alarm_rule",
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
        Command::CameraAngle { .. } => "camera_angle",
        Command::CameraShotTune { .. } => "camera_shot_tune",
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
        Command::TriggerBox { .. } => "trigger_box",
        Command::TriggerRemove { .. } => "trigger_remove",
        Command::TriggerEnable { .. } => "trigger_enable",
        Command::TriggerTrack { .. } => "trigger_track",
        Command::TriggerUntrack { .. } => "trigger_untrack",
        Command::TriggerConfigure { .. } => "trigger_configure",
        Command::CameraCapture { .. } => "camera_capture",
        Command::MenuCategory { .. } => "menu_category",
        Command::MenuItem { .. } => "menu_item",
        Command::MenuSetting { .. } => "menu_setting",
        Command::MenuHide { .. } => "menu_hide",
        Command::MenuHandle { .. } => "menu_handle",
        Command::MenuUnhandle { .. } => "menu_unhandle",
        Command::MenuSetValue { .. } => "menu_set_value",
        Command::MenuMovie { .. } => "menu_movie",
        Command::MenuBitmap { .. } => "menu_bitmap",
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
        "triggers": {"volumes": [], "bodies": {}},
        "attach": Value::Null,
        "detach_error": Value::Null,
        "detach_pending": false,
        "map": {"name": "", "generation": 0},
        "tick": 0,
        "keys": {},
        // 18 gameplay actions, IDs 64..=81.
        "actions": vec![0.0f32; 18],
        "pad": {"buttons": 0, "triggers": [0.0, 0.0], "left": [0.0, 0.0], "right": [0.0, 0.0]},
        // Identity per controller slot 0..3 (null = empty) and the slot gameplay reads.
        "controllers": {"active": Value::Null, "slots": []},
        "paused": false,
        "replay": false,
        "frame": {"frame": 0, "ms": 0.0, "fixed_steps": 0, "main_ms": 0.0, "fixed_ms": 0.0, "hitch": false, "window_s": 5.0, "frames": 0,
                  "fps": 0.0, "mean_ms": 0.0, "median_ms": 0.0, "low_1_ms": 0.0, "low_01_ms": 0.0,
                  "worst_ms": 0.0, "hitches": 0},
        "camera": Value::Null,
        "camera_angle": {"selected": "high", "active": "high", "owner": Value::Null, "shot": "", "tuned": {}},
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
            capabilities.set("deformation", 1)?;
            capabilities.set("physics_debug", 1)?;
            capabilities.set("scene_transforms", 2)?;
            capabilities.set("skater", 4)?;
            capabilities.set("menus", 3)?;
            capabilities.set("player_physics", 2)?;
            capabilities.set("engine_access", 1)?;
            capabilities.set("command_results", 1)?;
            capabilities.set("native_bodies", 1)?;
            capabilities.set("input_override", 1)?;
            // sdk.input.controller(s) (read-only identity) and sdk.input.action_ids.
            capabilities.set("controllers", 1)?;
            capabilities.set("action_ids", 1)?;
            capabilities.set("player_overlap", 1)?;
            capabilities.set("landed_details", 1)?;
            capabilities.set("camera", 4)?;
            capabilities.set("player_control", 1)?;
            capabilities.set("session", 1)?;
            capabilities.set("volumes", 1)?;
            capabilities.set("triggers", 1)?;
            capabilities.set("capture", 1)?;
            capabilities.set("multiplayer_debug", 1)?;
            // 2 (2026-10-04, audio/moddability-2): the `emitter` and `reverb_zone` kinds.
            // 3 (doc 16 L3): `slots = 'own'` (a traffic / ped object on its own MixMap instance).
            // 4 (doc 16 M3, user decision 2026-10-04): own instances are the default for a mod's
            // cars and peds (as for its emitters); `slots = 'shared'` (alias `retail`) opts into
            // retail's pools; `read(key).slots / waiting`, `info().own`.
            capabilities.set("world_audio", 4)?;
            // Audio extension 1: the mod's own WAVs (`sdk.audio.preload / play / update / stop /
            // stop_all`); before 2026-10-04 only `sdk.audio.version` advertised it.
            // 2 (2026-10-04): retail posts by class, globals, MixMap / global watch, `sdk.audio.info`.
            // 3 (2026-10-04, audio/moddability-2): `sdk.audio.play` through the native mixer, the
            // default (user decision 2026-10-04; `native = false` keeps the Bevy voice).
            // 4 (doc 16 L2 / L5): `sdk.audio.set_mixmap_input` (writable MixMap inputs) and
            // `sdk.audio.seed` (a seedable audio random state).
            capabilities.set("audio", 4)?;
            // Audio content overlays (`audio.json`: replace / add retail audio content by identity;
            // `audio_content.rs`), applied while the mod runs.
            // 2 (audio/moddability-2): `rules` in audio.json.
            // 3 (doc 16 L1 / L4 / L7): content changes are hot-swapped (no restart where exact),
            // `add.projects` (mod Csis projects), and `audio.json` reloads while the mod runs.
            capabilities.set("audio_content", 3)?;
            // Audio events, observe only (`sdk.audio.subscribe` / `sdk.audio.events`).
            // 2 (audio/moddability-2): mute / replace / layer rules (`sdk.audio.rule`).
            // 3: a rule sound's `offset` in the owner's frame (`play.frame = 'owner'`), and rule
            // sounds at a published emitter follow it.
            capabilities.set("audio_events", 3)?;
            // Tuning writes at run time (`sdk.audio.set_tuning`: player / world / bus / reverb domains).
            capabilities.set("audio_tuning", 1)?;
            capabilities.set("world_tuning", 1)?;
            // Retail menus (sdk.menus): edit the retail menu structure, handle any entry.
            capabilities.set("retail_menus", 1)?;
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

    #[test]
    fn camera_angle_api_crosses_the_lua_boundary() {
        use std::time::{SystemTime,UNIX_EPOCH};
        let root=std::env::temp_dir().join(format!("skate-camera-angle-api-{}-{}",
            std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            return { on_load=function()
                assert(sdk.capabilities.camera >= 4)
                local a = sdk.camera.angle()
                assert(a.selected == "high" and a.active == "high" and a.owner == nil)
                sdk.camera.set_angle("low")
                sdk.camera.set_angle(nil)
                sdk.camera.tune_shot("chase_flat_slow", {PositionDistance=1.4, FramingPitch=-6})
                sdk.camera.tune_shot("chase_flat_slow", nil)
            end }
        "#).unwrap();
        let manifest:Manifest=serde_json::from_value(json!({
            "id":"tests.camera_angle","api":2,"name":"Camera angle test", "version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        let snap=json!({});
        let mut vm=Vm::new(&root,&manifest,&BTreeMap::new(),&snap).unwrap();
        let out=vm.call("on_load",json!({}),&snap).unwrap();
        assert_eq!(out.len(),4);
        assert!(matches!(&out[0],Command::CameraAngle{angle:Some(crate::presentation::CameraAngle::Low)}));
        assert!(matches!(&out[1],Command::CameraAngle{angle:None}));
        match &out[2] { Command::CameraShotTune{shot,patch:Some(p)} => {
            assert_eq!(shot,"chase_flat_slow");
            assert_eq!(p.position_distance,Some(1.4));
            assert_eq!(p.framing_pitch,Some(-6.0));
            assert_eq!(p.position_elevation,None);
        }, other => panic!("wrong tune command {other:?}") }
        assert!(matches!(&out[3],Command::CameraShotTune{patch:None,..}));
        // Unknown angles / attribute names fail to deserialize; bad shot names fail validation.
        assert!(serde_json::from_value::<Command>(json!({"kind":"camera_angle","angle":"sideways"})).is_err());
        assert!(serde_json::from_value::<Command>(json!({"kind":"camera_shot_tune","shot":"bl_chase","patch":{"distance":2}})).is_err());
        let bad:Command=serde_json::from_value(json!({"kind":"camera_shot_tune","shot":"Bad Name","patch":{"PositionDistance":2}})).unwrap();
        assert!(!bad.validate());
        let far:Command=serde_json::from_value(json!({"kind":"camera_shot_tune","shot":"bl_chase","patch":{"PositionDistance":900}})).unwrap();
        assert!(!far.validate());
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
    fn trigger_api_crosses_the_lua_serde_boundary() {
        let root = std::env::temp_dir().join(format!("skate-mods-triggers-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            return { on_load=function()
                assert(sdk.capabilities.triggers == 1, 'capability')
                local reset = sdk.triggers.get('tut_sksc_reset_vol01')
                assert(reset and reset.id == '2c7017060025128f', 'get by name')
                assert(sdk.triggers.get('2c7017060025128f').group == 'challenge', 'get by id')
                assert(#sdk.triggers.list() == 1, 'list')
                assert(sdk.triggers.inside()[1] == '2c7017060025128f', 'player inside')
                assert(#sdk.triggers.inside('mod:x:ball') == 0, 'other body')
                sdk.triggers.box('goal', {center={1,2,3}, half_extents={1,1,1}, rotation={0,0,0,1}, name='goal', group='stairs'})
                sdk.triggers.remove('goal')
                sdk.triggers.set_enabled('2c7017060025128f', false)
                sdk.triggers.track('ball')
                sdk.triggers.track('ball', {radius=0.5, length=0.2})
                sdk.triggers.untrack('ball')
                sdk.triggers.configure({radius=0.4})
                sdk.triggers.configure()
            end }
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({"id":"tests.triggers","api":2,"name":"Triggers",
            "version":"1.0.0","author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        manifest.validate().unwrap();
        let snap = json!({"triggers": {"volumes": [{"id": "2c7017060025128f", "name": "tut_sksc_reset_vol01",
            "group": "challenge"}], "bodies": {"player": ["2c7017060025128f"]}}});
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &snap).unwrap();
        let out = vm.call("on_load", json!({}), &snap).unwrap();
        assert_eq!(out.iter().map(command_kind).collect::<Vec<_>>(), vec!["trigger_box", "trigger_remove",
            "trigger_enable", "trigger_track", "trigger_track", "trigger_untrack", "trigger_configure", "trigger_configure"]);
        assert!(out.iter().all(Command::validate));
        assert!(matches!(&out[2], Command::TriggerEnable { enabled: false, .. }));
        assert!(matches!(&out[3], Command::TriggerTrack { options: None, .. }));
        assert!(matches!(&out[4], Command::TriggerTrack { options: Some(o), .. } if o.radius == Some(0.5)));
        assert!(matches!(&out[7], Command::TriggerConfigure { options: None }));
        let bad = TriggerBoxOptions { center: [0.; 3], half_extents: [0., 1., 1.], rotation: None, name: None, group: None };
        assert!(!bad.validate());
        let lobby = TriggerBoxOptions { half_extents: [1.; 3], group: Some("lobby".into()), ..bad };
        assert!(!lobby.validate());
        drop(vm);
        std::fs::remove_dir_all(root).ok();
    }

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
                assert(#sdk.triggers.list() == 0 and #sdk.triggers.inside() == 0, 'triggers')
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

    /// `sdk.snapshot.frame` (frame-time statistics) is always readable: zeros
    /// before the host publishes it, the host's values afterwards.
    #[test]
    fn frame_statistics_have_defaults_and_keep_host_values() {
        let empty = complete(&Value::Null);
        assert_eq!(empty["frame"]["fps"], 0.0);
        assert_eq!(empty["frame"]["low_1_ms"], 0.0);
        assert_eq!(empty["frame"]["hitches"], 0);
        let live = complete(&json!({"frame": {"ms": 6.5, "worst_ms": 197.0, "hitches": 3}}));
        assert_eq!(live["frame"]["worst_ms"], 197.0);
        assert_eq!(live["frame"]["hitches"], 3);
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
        // `uvs = {}` reads as an empty list (`lua_list`), so the 3-vertex write fails validation
        // (two UVs per vertex) instead of deserialisation; `_submit` rejects it either way.
        assert!(lua.from_value::<Command>(value).is_ok_and(|c| !c.validate()));
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

#[cfg(test)]
mod deformation_api_tests {
    use super::*;
    #[test]
    fn generic_deformation_descriptors_cross_lua_command_boundary() {
        let lua=Lua::new();
        let value:mlua::Value=lua.load(r#"return {
            kind='physics_spawn',key='metal_prop',body={
                shape={type='box',half_extents={1,1,1}},body_type='dynamic',
                deformation={yield_speed=3,compliance=0.03,resolution={9,5,17}}
            }}"#).eval().unwrap();
        let command:Command=lua.from_value(value).unwrap();
        let Command::PhysicsSpawn {body,..}=command else {panic!()};
        assert_eq!(body.deformation.unwrap().yield_speed,3.);
        let command:Command=serde_json::from_value(serde_json::json!({"kind":"graphics_mesh","key":"visual",
            "path":"prop.glb","body":"metal_prop","deform_nodes":["shell"]})).unwrap();
        assert!(command.validate());
    }
}

#[cfg(test)]
mod world_audio_tests {
    use super::*;
    use serde_json::json;

    /// Both audio extensions are advertised in `sdk.capabilities` (feature discovery, as for the
    /// other extensions), and `sdk.audio.version` still reads 1.
    #[test]
    fn audio_capabilities_are_advertised() {
        let root = std::env::temp_dir().join(format!("skate-audio-capabilities-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            assert(sdk.capabilities.audio == 4, 'audio capability')
            assert(sdk.capabilities.world_audio == 4, 'world_audio capability')
            assert(sdk.capabilities.audio_content == 3, 'audio_content capability')
            assert(sdk.capabilities.audio_events == 3, 'audio_events capability')
            assert(sdk.audio.version == 4 and sdk.world_audio.version == 4, 'versions')
            return {}
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.audio-capabilities","api":2,"name":"Audio capabilities","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}
        })).unwrap();
        let loaded = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).map(|_| ());
        std::fs::remove_file(root.join("main.lua")).unwrap();
        std::fs::remove_dir(&root).unwrap();
        loaded.unwrap();
    }

    /// The world audio wrappers cross the serde boundary, and the bundled dev test publisher
    /// (`mods/world-audio-test`) runs frames without a Lua error or an invalid command, within the
    /// 128-commands-per-callback limit.
    #[test]
    fn world_audio_wrappers_and_the_test_publisher_run() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/world-audio-test"));
        let manifest: Manifest = serde_json::from_slice(&std::fs::read(root.join("mod.json")).unwrap()).unwrap();
        manifest.validate().unwrap();
        assert!(!manifest.enabled_by_default, "the dev test mod is opt-in");
        let settings: BTreeMap<String, Value> = manifest.settings.iter().map(|(k, s)| (k.clone(), s.default.clone())).collect();
        let mut vm = Vm::new(root, &manifest, &settings, &Value::Null).unwrap();
        let mut spawned = 0;
        let mut live = std::collections::BTreeSet::new();
        let mut most = 0;
        let mut events: BTreeMap<String, usize> = BTreeMap::new();
        let mut photo = Vec::new();
        let mut ghost_voices = Vec::new();
        let mut impacts = Vec::new();
        for frame in 0..400u32 {
            let t = frame as f32 / 60.0;
            // Frames 320..330: the skater stands in the parked taxi's box (anchor (20, 0, 0) after F9, taxi at +(8, 0.5, 6)).
            let position = if (320..330).contains(&frame) { [28.0, 0.0, 6.0] } else { [t * 4.0, 0.0, 0.0] };
            let snapshot = json!({"tick": frame, "player": {"position": position, "velocity": [4.0, 0.0, 0.0], "speed": 4.0,
                "landing_seq": frame / 100, "bail_seq": frame / 250},
                "keys": {"F8": frame == 200, "F9": frame == 300, "F7": frame == 100, "F6": frame == 120, "F5": frame == 140, "F4": frame == 160 || frame == 180, "F3": frame == 220, "F2": frame == 240 || frame == 260},
                "world_audio": {"dev-world-audio-test": {"car1": {"kind": "traffic", "audible": true, "instance": 0}}},
                "command_results": {"dev-world-audio-test": {"ghost": {"token": 1, "ok": false, "error": "logs/x.tsv: missing"}}}});
            let out = vm.call("on_update", json!({"dt": 1.0 / 60.0}), &snapshot).unwrap_or_else(|e| panic!("frame {frame}: {e}"));
            assert!(out.len() <= 128);
            for c in &out {
                assert!(c.validate(), "frame {frame}: invalid {c:?}");
                match c {
                    Command::WorldAudioSpawn { key, .. } => {
                        spawned += 1;
                        live.insert(key.clone());
                    }
                    Command::WorldAudioRemove { key } => {
                        live.remove(key);
                    }
                    Command::WorldAudioEvent { event, options, key } => {
                        if event == "impact" {
                            impacts.push((frame, key.clone(), options.speed));
                        }
                        let name = match (event.as_str(), &options.value) {
                            ("speech", Some(v)) if v == &json!(49) => "phone".to_owned(),
                            ("speech", Some(v)) if v == &json!(29) => "photographer".to_owned(),
                            ("reaction", Some(v)) => format!("reaction {}", v.as_str().unwrap_or("?")),
                            (e, _) => e.to_owned(),
                        };
                        *events.entry(name).or_default() += 1;
                    }
                    Command::WorldAudioUpdate { options, .. } if options.photo_flag.is_some() => photo.push(options.photo_flag.unwrap()),
                    Command::WorldAudioUpdate { key, options } if key == "ghost" && options.voice.is_some() => ghost_voices.push(options.voice.unwrap()),
                    _ => {}
                }
            }
            most = most.max(live.len());
        }
        assert_eq!(spawned, 2 * 36, "16 cars and 20 peds, twice (F9 re-centres); the ghost goes through a request");
        // The new ped events: F7 a zap, F6 a knock-down (four BodyFallType keys over 0.74 s), F5 a
        // phone call (value 49), F4 a photographer (value 29 with the photo flag) and F4 again off.
        assert_eq!(events.get("tazer"), Some(&1), "{events:?}");
        assert_eq!(events.get("body_fall"), Some(&4), "{events:?}");
        assert_eq!(events.get("phone"), Some(&1), "{events:?}");
        assert_eq!(events.get("photographer"), Some(&1), "{events:?}");
        assert_eq!(photo, vec![true, false]);
        // The ghost: F3 a trick reaction, F2 twice a crash (first as the pro 24, then as its AI voice).
        assert_eq!(events.get("reaction trick"), Some(&1), "{events:?}");
        assert_eq!(events.get("reaction crash"), Some(&2), "{events:?}");
        assert_eq!(ghost_voices, vec![24, 91]);
        // The impact demo: one impact per frame while the skater stands in the parked taxi (car16), with its speed.
        assert_eq!(impacts.len(), 10, "{impacts:?}");
        assert!(impacts.iter().all(|(f, key, speed)| (320..330).contains(f) && key == "car16" && *speed == Some(4.0)), "{impacts:?}");
        // Everything it publishes at once (+ the ghost) fits the host's per-mod limit: no
        // "World audio object limit reached" (the 16 / 64 limits refused 21 of 37).
        assert!(most + 1 <= crate::world_audio::MAX_OBJECTS_PER_MOD, "{most} + ghost objects");
    }

    /// The audio extension 2 commands cross the serde boundary (valid, invalid, unknown field)
    /// and the Lua wrappers submit them; the read helpers find this mod's snapshot rows.
    #[test]
    fn audio_api_commands_deserialize_and_validate() {
        for value in [
            json!({"kind":"audio_post","key":"siren","class":"c_emitter","words":[32767,16000,0,0,4096,25000,0,0,3]}),
            json!({"kind":"audio_post","key":"siren","class":"c_emitter"}),
            json!({"kind":"audio_redeliver","key":"siren","words":[1,2]}),
            json!({"kind":"audio_release","key":"siren"}),
            json!({"kind":"audio_set_global","name":"g_snd","value":5}),
            json!({"kind":"audio_set_global","name":"g_snd"}),
            json!({"kind":"audio_watch","globals":["g_snd"],"mixmap":[{"slot":"player","object":0,"instance":0,"output":4}]}),
            json!({"kind":"audio_watch"}),
            json!({"kind":"engine_inspect","system":"audio_catalog"}),
            json!({"kind":"audio_subscribe","tags":["pop","land"]}),
            json!({"kind":"audio_subscribe","tags":[]}),
            json!({"kind":"audio_subscribe"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(c.validate(), "{value}");
        }
        for value in [
            json!({"kind":"audio_post","key":"siren","class":"c emitter"}),
            json!({"kind":"audio_post","key":"../x","class":"c_emitter"}),
            json!({"kind":"audio_post","key":"siren","class":"c_emitter","words":vec![0; 33]}),
            json!({"kind":"audio_set_global","name":""}),
            json!({"kind":"audio_watch","mixmap":[{"slot":"music","output":0}]}),
            json!({"kind":"audio_watch","mixmap":[{"slot":"player","output":40}]}),
            json!({"kind":"audio_watch","globals":vec!["g"; 17]}),
            json!({"kind":"audio_subscribe","tags":["no tag"]}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(!c.validate(), "accepted {value}");
        }
        for value in [
            json!({"kind":"audio_post","key":"siren","class":"c_emitter","typo":1}),
            json!({"kind":"audio_watch","mixmap":[{"slot":"player","output":1,"extra":2}]}),
            json!({"kind":"audio_set_global","name":"g","value":1.5}),
        ] {
            assert!(serde_json::from_value::<Command>(value.clone()).is_err(), "{value}");
        }
        let root = std::env::temp_dir().join(format!("skate-audio-api-lua-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            function M.on_update()
                sdk.audio.post('siren', 'c_emitter', {32767, 1})
                sdk.audio.redeliver('siren', {1})
                sdk.audio.release('siren')
                sdk.audio.set_global('g_snd', 3)
                sdk.audio.set_global('g_snd', nil)
                sdk.audio.watch{mixmap={{slot='emitter', object=0, instance=2, output=4}}}
                sdk.audio.post('empty', 'c_emitter', {})
                sdk.audio.watch{globals={'g_snd'}, mixmap={{slot='emitter', object=0, instance=2, output=4}}}
                local h = sdk.audio.handle('siren')
                assert(h and h.live == true, 'handle')
                assert(sdk.audio.global('g_snd') == 7, 'global')
                local m = sdk.audio.mixmap('emitter', 0, 2, 4)
                assert(m and m.level == 123, 'mixmap')
                assert(sdk.audio.info().native == true, 'info')
                assert(sdk.audio.handle('nope') == nil)
                sdk.audio.subscribe{tags={'pop'}}
                sdk.audio.subscribe{tags={}}
                sdk.audio.subscribe(nil)
                local rows = sdk.audio.events()
                assert(#rows == 1 and rows[1].tag == 'pop', 'events')
                assert(#sdk.audio.events() == 0, 'each serial once')
            end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.audio-api","api":2,"name":"Audio API","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let snapshot = json!({
            "audio": {"tests.audio-api": {"handles": {"siren": {"live": true, "class": "c_emitter"}},
                "watch": {"globals": {"g_snd": 7}, "mixmap": [{"slot": "emitter", "object": 0, "instance": 2, "output": 4, "level": 123}]},
                "events": {"serial": 4, "rows": [{"kind": "splice", "tag": "pop"}], "truncated": false}}},
            "audio_info": {"native": true}});
        let cmds = vm.call("on_update", json!({"dt": 0.016}), &snapshot).unwrap();
        let kinds: Vec<_> = cmds.iter().map(command_kind).collect();
        assert_eq!(kinds, ["audio_post", "audio_redeliver", "audio_release", "audio_set_global", "audio_set_global", "audio_watch", "audio_post", "audio_watch", "audio_subscribe", "audio_subscribe", "audio_subscribe"]);
        // Empty Lua tables reach serde as maps: they still read as empty lists.
        assert!(matches!(&cmds[5], Command::AudioWatch { globals, mixmap } if globals.is_empty() && mixmap.len() == 1), "globals = {{}}");
        assert!(matches!(&cmds[6], Command::AudioPost { words, .. } if words.is_empty()), "words = {{}}");
        assert!(matches!(&cmds[9], Command::AudioSubscribe { tags: Some(t) } if t.is_empty()), "an empty Lua table = every row");
        assert!(matches!(&cmds[10], Command::AudioSubscribe { tags: None }), "nil stops");
        assert!(cmds.iter().all(Command::validate));
        assert!(matches!(&cmds[4], Command::AudioSetGlobal { value: None, .. }), "nil restores");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Audio tuning extension 1: `audio_set_tuning` crosses the serde boundary (valid, invalid,
    /// unknown field); the tuning read is an `engine_inspect` of `audio_tuning:<domain>[/path]`; the
    /// Lua wrappers submit both and `audio_tuning` is advertised.
    #[test]
    fn audio_tuning_commands_deserialize_and_validate() {
        for value in [
            json!({"kind":"audio_set_tuning","domain":"world","patch":{"traffic_engine":{"c04_taxi01":{"idle_rpm":1200}}}}),
            json!({"kind":"audio_set_tuning","domain":"reverb","patch":{"BEEFC8E3DE04FBAE":{"3":0.5}}}),
            json!({"kind":"audio_set_tuning","domain":"player"}),
            json!({"kind":"engine_inspect","system":"audio_tuning:world"}),
            json!({"kind":"engine_inspect","system":"audio_tuning:world/traffic_engine/c04_taxi01"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(c.validate(), "{value}");
        }
        for value in [
            json!({"kind":"audio_set_tuning","domain":"mixmap","patch":{"a":1}}),
            json!({"kind":"audio_set_tuning","domain":"world","patch":{"a":null}}),
            json!({"kind":"audio_set_tuning","domain":"world","patch":7}),
            json!({"kind":"engine_inspect","system":"audio_tuning:mixmap"}),
            json!({"kind":"engine_inspect","system":"audio_tuning:world/../x"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(!c.validate(), "accepted {value}");
        }
        assert!(serde_json::from_value::<Command>(json!({"kind":"audio_set_tuning","domain":"world","patch":{},"owner":"x"})).is_err());
        let root = std::env::temp_dir().join(format!("skate-audio-tuning-lua-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            function M.on_update()
                assert(sdk.capabilities.audio_tuning == 1, 'capability')
                sdk.audio.set_tuning('world', {traffic_engine = {c04_taxi01 = {idle_rpm = 1200}}})
                sdk.audio.set_tuning('world', nil)
                sdk.audio.tuning('t', 'world', 'traffic_engine/c04_taxi01')
            end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.audio-tuning","api":2,"name":"Audio tuning","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let cmds = vm.call("on_update", json!({"dt": 0.016}), &json!({})).unwrap();
        let kinds: Vec<_> = cmds.iter().map(command_kind).collect();
        assert_eq!(kinds, ["audio_set_tuning", "audio_set_tuning", "request"]);
        assert!(cmds.iter().all(Command::validate));
        assert!(matches!(&cmds[1], Command::AudioSetTuning { patch: None, .. }), "nil restores");
        assert!(matches!(&cmds[2], Command::Request { command, .. } if matches!(&**command, Command::EngineInspect { system } if system == "audio_tuning:world/traffic_engine/c04_taxi01")));
        let _ = std::fs::remove_dir_all(root);
    }

    /// World tuning extension 1: `world_set_tuning` validates per domain, the Lua wrappers submit
    /// set / restore / read and `world_tuning` is advertised.
    #[test]
    fn world_tuning_commands_deserialize_and_validate() {
        for (value, ok) in [
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"npc_draw_distance":2.0}}), true),
            (json!({"kind":"world_set_tuning","domain":"carry"}), true),
            (json!({"kind":"engine_inspect","system":"world_tuning:props"}), true),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"nope":1}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"npc_tricks":{"mode":"recorded","min_air_frames":30},"skater_trick_profiles":{"default":{"regular":[{"trick":96,"weight":1.0}]}}}}), true),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"npc_tricks":{"mode":"scripted"}}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"npc_simulated":{"enabled":true,"radius":30.0,"max":2}}}), true),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"npc_simulated":{"radius":-1.0}}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"free_play":{"traffic":0.3,"ai_skaters":false},"pedestrians":{"density":2.0},"ambient_skaters":5}}), true),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"free_play":{"traffic":1.5}}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"vehicles":{"density":9.0}}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"ambient_skaters":20}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"skater_trick_profiles":{"default":{"regular":[{"trick":400,"weight":1.0}]}}}}), false),
            (json!({"kind":"world_set_tuning","domain":"living_world","patch":{"skater_trick_profiles":{"default":{"nollie":[{"trick":117,"weight":-1.0}]}}}}), false),
            (json!({"kind":"world_set_tuning","domain":"roads","patch":{}}), false),
            (json!({"kind":"world_reset_moved_props"}), true),
            (json!({"kind":"world_reset_prop","id":7}), true),
            (json!({"kind":"world_upright_prop","id":7}), true),
            (json!({"kind":"world_spawn_prop","key":"can","from":7,"position":[1,2,3],"velocity":[4,2,0]}), true),
            (json!({"kind":"world_spawn_prop","key":"can","from":7,"position":[1,2,3],"velocity":[400,0,0]}), false),
            (json!({"kind":"world_spawn_prop","key":"../x","from":7,"position":[1,2,3]}), false),
            (json!({"kind":"world_remove_prop","key":"can"}), true),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(c.validate(), ok, "{value}");
        }
        let root = std::env::temp_dir().join(format!("skate-world-tuning-lua-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            function M.on_update()
                assert(sdk.capabilities.world_tuning == 1, 'capability')
                sdk.world.set_tuning('living_world', {skater_fade = {fade_seconds = 2}})
                sdk.world.set_tuning('living_world', nil)
                sdk.world.tuning('t', 'carry')
                sdk.world.reset_moved_props()
                sdk.world.reset_prop(7)
                sdk.world.upright_prop(7)
                sdk.world.spawn_prop('can', 7, {position = {1, 2, 3}, velocity = {4, 2, 0}})
                sdk.world.remove_prop('can')
            end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.world-tuning","api":2,"name":"World tuning","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let cmds = vm.call("on_update", json!({"dt": 0.016}), &json!({})).unwrap();
        let kinds: Vec<_> = cmds.iter().map(command_kind).collect();
        assert_eq!(kinds, ["world_set_tuning", "world_set_tuning", "request", "world_reset_moved_props", "world_reset_prop", "world_upright_prop", "world_spawn_prop", "world_remove_prop"]);
        assert!(cmds.iter().all(Command::validate));
        assert!(matches!(&cmds[1], Command::WorldSetTuning { patch: None, .. }), "nil restores");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Audio extension 4 (doc 16 L2, L5): `audio_set_mixmap_input` and `audio_seed` cross the serde
    /// boundary (valid, invalid, unknown field) and the Lua wrappers submit them (nil releases).
    #[test]
    fn mixmap_input_and_seed_commands_deserialize_and_validate() {
        for value in [
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":2,"instance":0,"input":1,"value":8192}),
            json!({"kind":"audio_set_mixmap_input","slot":"emitter","object":0,"instance":3,"input":1,"value":12.5,"float":true}),
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":6,"instance":0,"input":0}),
            json!({"kind":"audio_seed","seed":42}),
            json!({"kind":"audio_seed"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(c.validate(), "{value}");
        }
        for value in [
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":2,"instance":0,"input":16,"value":1}),
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":200,"instance":0,"input":1,"value":1}),
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":2,"instance":40,"input":1,"value":1}),
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":2,"instance":0,"input":1,"value":0.5}),
            json!({"kind":"audio_set_mixmap_input","slot":"global","object":2,"instance":0,"input":1,"value":4294967296.0}),
            json!({"kind":"audio_set_mixmap_input","slot":"bad slot","object":2,"instance":0,"input":1,"value":1}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(!c.validate(), "accepted {value}");
        }
        assert!(serde_json::from_value::<Command>(json!({"kind":"audio_seed","seed":1,"owner":"x"})).is_err());
        assert!(serde_json::from_value::<Command>(json!({"kind":"audio_seed","seed":-1})).is_err());
        let root = std::env::temp_dir().join(format!("skate-audio-l2l5-lua-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            function M.on_update()
                assert(sdk.capabilities.audio == 4, 'capability')
                sdk.audio.set_mixmap_input('global', 2, 0, 1, 8192)
                sdk.audio.set_mixmap_input('emitter', 0, 0, 1, 3.5, {float = true})
                sdk.audio.set_mixmap_input('global', 2, 0, 1, nil)
                sdk.audio.seed(42)
                sdk.audio.seed(nil)
                assert(#sdk.audio.mixmap_inputs() == 0)
            end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.audio-l2l5","api":2,"name":"Audio L2 L5","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let cmds = vm.call("on_update", json!({"dt": 0.016}), &json!({})).unwrap();
        let kinds: Vec<_> = cmds.iter().map(command_kind).collect();
        assert_eq!(kinds, ["audio_set_mixmap_input", "audio_set_mixmap_input", "audio_set_mixmap_input", "audio_seed", "audio_seed"]);
        assert!(cmds.iter().all(Command::validate));
        assert!(matches!(&cmds[1], Command::AudioSetMixmapInput { float: true, .. }));
        assert!(matches!(&cmds[2], Command::AudioSetMixmapInput { value: None, .. }), "nil releases");
        assert!(matches!(&cmds[3], Command::AudioSeed { seed: Some(42) }));
        assert!(matches!(&cmds[4], Command::AudioSeed { seed: None }));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Audio events extension 2: `audio_rule` crosses the serde boundary (valid, invalid, unknown
    /// field) and the Lua wrapper submits it (a nil rule removes).
    #[test]
    fn audio_rule_commands_deserialize_and_validate() {
        for value in [
            json!({"kind":"audio_rule","key":"quiet_pop","rule":{"match":{"tag":"pop"},"action":"mute"}}),
            json!({"kind":"audio_rule","key":"my_pop","rule":{"match":{"tag":"pop"},"action":"replace","play":{"path":"audio/pop.wav","volume":0.8}}}),
            json!({"kind":"audio_rule","key":"horns","rule":{"match":{"source":"world","class":"TRAFFIC_HORN"},"action":"layer","play":{"path":"a.wav"},"min_interval":0.3}}),
            json!({"kind":"audio_rule","key":"quiet_pop"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(c.validate(), "{value}");
        }
        for value in [
            json!({"kind":"audio_rule","key":"../x","rule":{"match":{"tag":"pop"},"action":"mute"}}),
            json!({"kind":"audio_rule","key":"x","rule":{"match":{},"action":"mute"}}),
            json!({"kind":"audio_rule","key":"x","rule":{"match":{"tag":"pop"},"action":"replace"}}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(!c.validate(), "accepted {value}");
        }
        assert!(serde_json::from_value::<Command>(json!({"kind":"audio_rule","key":"x","rule":{"match":{"tag":"pop"},"action":"mute"},"extra":1})).is_err());
        let root = std::env::temp_dir().join(format!("skate-audio-rule-lua-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            function M.on_update()
                assert(sdk.capabilities.audio_events == 3 and sdk.capabilities.audio_content == 3, 'capabilities')
                sdk.audio.rule('my_pop', {match = {tag = 'pop'}, action = 'replace', play = {path = 'audio/pop.wav', volume = 0.8}})
                sdk.audio.rule('my_pop', nil)
                -- Where the rule's sound plays (2026-10-04): an offset from the owner, a fixed spot.
                sdk.audio.rule('honk', {match = {tag = 'horn'}, action = 'layer', play = {path = 'a.wav', offset = {0, 1.5, 0}, falloff = {radius = 20}}})
                sdk.audio.rule('spot', {match = {tag = 'land'}, action = 'replace', play = {path = 'a.wav', at = 'world', position = {1, 2, 3}}})
                -- Native routing is the default; native = false passes through.
                sdk.audio.play('a', {path = 'a.wav'})
                sdk.audio.play('b', {path = 'a.wav', native = false})
            end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.audio-rule","api":2,"name":"Audio rule","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let cmds = vm.call("on_update", json!({"dt": 0.016}), &json!({})).unwrap();
        assert_eq!(cmds.iter().map(command_kind).collect::<Vec<_>>(), ["audio_rule", "audio_rule", "audio_rule", "audio_rule", "audio_play", "audio_play"]);
        assert!(cmds.iter().all(Command::validate));
        assert!(matches!(&cmds[0], Command::AudioRule { rule: Some(r), .. } if r.action == crate::audio_rules::RuleAction::Replace && r.on.tag.as_deref() == Some("pop")));
        assert!(matches!(&cmds[1], Command::AudioRule { rule: None, .. }));
        assert!(matches!(&cmds[2], Command::AudioRule { rule: Some(r), .. } if r.play.as_ref().is_some_and(|p| p.at == crate::audio_rules::RuleAt::Owner && p.offset == Some([0.0, 1.5, 0.0]) && p.falloff.is_some_and(|f| f.radius == 20.0))));
        assert!(matches!(&cmds[3], Command::AudioRule { rule: Some(r), .. } if r.play.as_ref().is_some_and(|p| p.at == crate::audio_rules::RuleAt::World && p.position == Some([1.0, 2.0, 3.0]))));
        assert!(matches!(&cmds[4], Command::AudioPlay { options, .. } if options.native.is_none() && options.wants_native()));
        assert!(matches!(&cmds[5], Command::AudioPlay { options, .. } if options.native == Some(false)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn world_audio_commands_deserialize() {
        for value in [
            json!({"kind":"world_audio_spawn","key":"car1","object":"traffic","options":{"engine":"c04_taxi01","position":[0,0,0]}}),
            json!({"kind":"world_audio_update","key":"car1","options":{"speed":3,"load":-2}}),
            json!({"kind":"world_audio_event","key":"car1","event":"alarm"}),
            json!({"kind":"world_audio_event","key":"ped1","event":"speech","options":{"value":"warn"}}),
            json!({"kind":"world_audio_event","key":"ped1","event":"tazer"}),
            json!({"kind":"world_audio_event","key":"ped1","event":"tazer","options":{"seconds":2}}),
            json!({"kind":"world_audio_event","key":"ped1","event":"body_fall","options":{"kind":9}}),
            json!({"kind":"world_audio_event","key":"sk1","event":"reaction","options":{"value":"crash"}}),
            json!({"kind":"world_audio_event","key":"sk1","event":"reaction","options":{"value":"slam","by":0}}),
            json!({"kind":"world_audio_update","key":"ped1","options":{"tazing":true,"photo_flag":true}}),
            json!({"kind":"world_audio_update","key":"sk1","options":{"loose_board":1}}),
            json!({"kind":"world_audio_remove","key":"car1"}),
            json!({"kind":"world_audio_spawn","key":"fountain","object":"emitter","options":{"bank":"water_fountain","patch":81,"position":[0,0,0],"extent":[6,6,6]}}),
            json!({"kind":"world_audio_spawn","key":"cave","object":"reverb_zone","options":{"preset":"BEEFC8E3DE04FBAE","position":[0,0,0],"extent":[20,8,12]}}),
            json!({"kind":"world_audio_update","key":"fountain","options":{"volume":0.3,"position":[1,0,0]}}),
            json!({"kind":"world_audio_spawn","key":"car2","object":"traffic","options":{"slots":"shared"}}),
            json!({"kind":"world_audio_spawn","key":"car3","object":"traffic","options":{"slots":"own"}}),
            json!({"kind":"world_audio_spawn","key":"ped2","object":"ped","options":{"slots":"retail"}}),
            json!({"kind":"world_audio_announcer","character":36}),
            json!({"kind":"world_audio_announcer"}),
            json!({"kind":"world_audio_announce","event":"480_slam_pro","options":{"pro":4}}),
            json!({"kind":"world_audio_announce","event":24703,"options":{"words":{}}}),
            json!({"kind":"world_audio_event","key":"car1","event":"impact","options":{"speed":3.3}}),
            json!({"kind":"world_audio_event","key":"car1","event":"impact","options":{"speed":1,"source":"vehicle"}}),
            json!({"kind":"world_audio_update","key":"car1","options":{"parked":true}}),
            json!({"kind":"world_audio_alarm_rule","options":{"enabled":false}}),
            json!({"kind":"world_audio_alarm_rule","options":{"min_impact":1.5,"seconds":4}}),
            json!({"kind":"world_audio_alarm_rule"}),
        ] {
            let c: Command = serde_json::from_value(value.clone()).unwrap();
            assert!(c.validate(), "{value}");
        }
        let c: Command = serde_json::from_value(json!({"kind":"world_audio_spawn","key":"x","object":"ped","options":{"engine":"c04_taxi01"}})).unwrap();
        assert!(!c.validate(), "a traffic field on a ped");
        for incomplete in [json!({"kind":"world_audio_spawn","key":"e","object":"emitter","options":{"bank":"x"}}), json!({"kind":"world_audio_spawn","key":"z","object":"reverb_zone","options":{"extent":[1,1,1]}})] {
            let c: Command = serde_json::from_value(incomplete.clone()).unwrap();
            assert!(!c.validate(), "{incomplete}");
        }
        assert!(serde_json::from_value::<Command>(json!({"kind":"world_audio_spawn","key":"x","object":"bus"})).is_err());
        let c: Command = serde_json::from_value(json!({"kind":"world_audio_update","key":"x","options":{"source":"lite"}})).unwrap();
        assert!(!c.validate(), "the source is fixed at spawn");
        let c: Command = serde_json::from_value(json!({"kind":"world_audio_update","key":"x","options":{"slots":"shared"}})).unwrap();
        assert!(!c.validate(), "the slots are fixed at spawn");
    }
}

/// An empty Lua table reaches serde as a map. Every list field a script can fill must still read
/// `{}` as the empty list (`lua_list`), through the real `_submit` path (`Vm` + `api.lua`).
#[cfg(test)]
mod empty_table_lists {
    use super::*;
    use serde_json::json;

    /// One case per callback (the sandbox has no `pcall`): `ok` cases must submit; `rejected`
    /// cases must deserialise and then fail validation, never with a serde "expected a sequence".
    #[test]
    fn every_list_field_takes_an_empty_table_and_a_list() {
        let root = std::env::temp_dir().join(format!("skate-empty-lists-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.lua"), r#"
            local M = {}
            local req = sdk.commands.request
            local tri = {{0,0,0},{1,0,0},{0,1,0}}
            local cases = {
                -- audio
                post_empty = function() sdk.audio.post('a', 'c_emitter', {}) end,
                post_list = function() sdk.audio.post('a', 'c_emitter', {1, 2}) end,
                redeliver_empty = function() sdk.audio.redeliver('a', {}) end,
                redeliver_list = function() sdk.audio.redeliver('a', {3}) end,
                watch_empty = function() sdk.audio.watch{globals={}, mixmap={}} end,
                watch_list = function() sdk.audio.watch{globals={'g_snd'}, mixmap={{slot='player', output=4}}} end,
                subscribe_empty = function() sdk.audio.subscribe{tags={}} end,
                subscribe_list = function() sdk.audio.subscribe{tags={'pop'}} end,
                post_named_keys = function() sdk.audio.post('a', 'c_emitter', {x=1}) end,
                mesh_named_keys = function() req('m', {kind='graphics_mesh', key='m', deform_nodes={x=1}}) end,
                -- graphics (raw requests skip the api.lua wrappers that drop empty tables)
                mesh_empty = function() req('m', {kind='graphics_mesh', key='m', deform_nodes={}}) end,
                mesh_list = function() req('m', {kind='graphics_mesh', key='m', deform_nodes={'panel'}}) end,
                write_list = function() req('w', {kind='graphics_mesh_buffer_write', key='b', data={positions=tri, indices={0,1,2},
                    uvs={0,0, 1,0, 0,1}, normals={{0,0,1},{0,0,1},{0,0,1}}, colors={1,1,1,1, 1,1,1,1, 1,1,1,1}}}) end,
                write_empty = function() req('w', {kind='graphics_mesh_buffer_write', key='b', data={positions={}, indices={}}}) end,
                write_uvs_empty = function() req('w', {kind='graphics_mesh_buffer_write', key='b', data={positions=tri, indices={0,1,2}, uvs={}}}) end,
                write_normals_empty = function() req('w', {kind='graphics_mesh_buffer_write', key='b', data={positions=tri, indices={0,1,2}, normals={}}}) end,
                write_colors_empty = function() req('w', {kind='graphics_mesh_buffer_write', key='b', data={positions=tri, indices={0,1,2}, colors={}}}) end,
                append_empty = function() req('a', {kind='graphics_mesh_buffer_append', key='b', data={positions=tri, indices={}, uvs={}, normals={}, colors={}}}) end,
                append_list = function() req('a', {kind='graphics_mesh_buffer_append', key='b', data={positions=tri, indices={0,1,2}}}) end,
                wrapper_write = function() sdk.graphics.mesh_buffer_write('b', {positions=tri, indices={0,1,2}, uvs={}}) end,
                -- ui
                canvas_empty = function() req('c', {kind='ui_canvas', key='c', options={items={}}}) end,
                canvas_list = function() req('c', {kind='ui_canvas', key='c', options={items={{key='t', text='hi'}}}}) end,
                canvas_wrapper_empty = function() sdk.ui.canvas('c', {items={}}) end,
                menu_items_empty = function() sdk.ui.menu('u', {title='T', items={}}) end,
                menu_children_empty = function() sdk.ui.menu('u', {title='T', items={{id='a', label='A', children={}}}}) end,
                menu_children_list = function() sdk.ui.menu('u', {title='T', items={{id='a', label='A', children={{id='b', label='B'}}}}}) end,
                -- player
                detach_empty = function() sdk.player.detach{candidates={}} end,
                detach_list = function() sdk.player.detach{candidates={{0,0,1}}} end,
                -- physics query (deserialised directly, not via a command)
                raycast_empty = function() sdk.physics.raycast({0,0,0}, {0,-1,0}, {exclude={}}) end,
                raycast_list = function() sdk.physics.raycast({0,0,0}, {0,-1,0}, {exclude={'car'}}) end,
            }
            function M.on_event(p) cases[p.case]() end
            return M
        "#).unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "id":"tests.empty-lists","api":2,"name":"Empty lists","version":"1.0.0",
            "author":"test","description":"test","entry":"main.lua","settings":{}})).unwrap();
        let mut vm = Vm::new(&root, &manifest, &BTreeMap::new(), &Value::Null).unwrap();
        let mut run = |case: &str| -> Result<Option<Command>, String> {
            let cmds = vm.call("on_event", json!({"case": case}), &json!({}))?;
            assert!(cmds.len() <= 1, "{case}");
            Ok(cmds.into_iter().next().map(|c| match c { Command::Request { command, .. } => *command, c => c }))
        };
        let mut one = |case: &str| -> Command { run(case).unwrap_or_else(|e| panic!("{case}: {e}")).unwrap_or_else(|| panic!("{case}: no command")) };
        assert!(matches!(one("post_empty"), Command::AudioPost { words, .. } if words.is_empty()));
        assert!(matches!(one("post_list"), Command::AudioPost { words, .. } if words == [1, 2]));
        assert!(matches!(one("redeliver_empty"), Command::AudioRedeliver { words, .. } if words.is_empty()));
        assert!(matches!(one("redeliver_list"), Command::AudioRedeliver { words, .. } if words == [3]));
        assert!(matches!(one("watch_empty"), Command::AudioWatch { globals, mixmap } if globals.is_empty() && mixmap.is_empty()));
        assert!(matches!(one("watch_list"), Command::AudioWatch { globals, mixmap } if globals == ["g_snd"] && mixmap.len() == 1));
        assert!(matches!(one("subscribe_empty"), Command::AudioSubscribe { tags: Some(t) } if t.is_empty()));
        assert!(matches!(one("subscribe_list"), Command::AudioSubscribe { tags: Some(t) } if t == ["pop"]));
        assert!(matches!(one("mesh_empty"), Command::GraphicsMesh { deform_nodes, .. } if deform_nodes.is_empty()));
        assert!(matches!(one("mesh_list"), Command::GraphicsMesh { deform_nodes, .. } if deform_nodes == ["panel"]));
        assert!(matches!(one("write_list"), Command::GraphicsMeshBufferWrite { data, .. }
            if data.positions.len() == 3 && data.indices == [0, 1, 2] && data.uvs.as_ref().is_some_and(|u| u.len() == 6)
                && data.normals.as_ref().is_some_and(|n| n.len() == 3) && data.colors.as_ref().is_some_and(|c| c.len() == 12)));
        assert!(matches!(one("append_empty"), Command::GraphicsMeshBufferAppend { data, .. }
            if data.positions.len() == 3 && data.indices.is_empty() && data.uvs.as_deref() == Some(&[][..])
                && data.normals.as_deref() == Some(&[][..]) && data.colors.as_deref() == Some(&[][..])));
        assert!(matches!(one("append_list"), Command::GraphicsMeshBufferAppend { data, .. } if data.indices == [0, 1, 2] && data.uvs.is_none()));
        // api.lua's payload builder still drops an empty uvs table (no UVs).
        assert!(matches!(one("wrapper_write"), Command::GraphicsMeshBufferWrite { data, .. } if data.uvs.is_none()));
        assert!(matches!(one("canvas_empty"), Command::UiCanvas { options, .. } if options.items.is_empty()));
        assert!(matches!(one("canvas_list"), Command::UiCanvas { options, .. } if options.items.len() == 1 && options.items[0].key == "t"));
        assert!(matches!(one("canvas_wrapper_empty"), Command::UiCanvas { options, .. } if options.items.is_empty()));
        assert!(matches!(one("menu_children_empty"), Command::UiMenu { options, .. } if options.items[0].children.is_empty()));
        assert!(matches!(one("menu_children_list"), Command::UiMenu { options, .. } if options.items[0].children[0].id == "b"));
        assert!(matches!(one("detach_empty"), Command::PlayerDetach { options } if options.candidates.is_empty()));
        assert!(matches!(one("detach_list"), Command::PlayerDetach { options } if options.candidates == [[0., 0., 1.]]));
        for case in ["raycast_empty", "raycast_list"] {
            assert!(run(case).unwrap_or_else(|e| panic!("{case}: {e}")).is_none(), "{case}");
        }
        // Empty where the command needs entries: read fine, refused by validation.
        for case in ["write_empty", "write_uvs_empty", "write_normals_empty", "write_colors_empty", "menu_items_empty"] {
            let e = run(case).expect_err(case);
            assert!(e.contains("Invalid command arguments") && !e.contains("expected a sequence"), "{case}: {e}");
        }
        // A table that is not a list is still refused.
        for case in ["post_named_keys", "mesh_named_keys"] {
            let e = run(case).expect_err("named keys");
            assert!(e.contains("expected a list"), "{e}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Fields that are maps or free values keep reading `{}` as a map / an object.
    #[test]
    fn map_fields_keep_empty_tables_as_maps() {
        let lua = Lua::new();
        let eval = |src: &str| -> Command { lua.from_value(lua.load(src).eval::<mlua::Value>().unwrap()).unwrap() };
        assert!(matches!(eval("return {kind='network_state', key='k', value={}}"), Command::NetworkState { value, .. } if value == json!({})));
        assert!(matches!(eval("return {kind='network_state', key='k', value={1,2}}"), Command::NetworkState { value, .. } if value == json!([1, 2])));
        assert!(matches!(eval("return {kind='world_audio_event', key='k', event='speech', options={value={}}}"),
            Command::WorldAudioEvent { options, .. } if options.value == Some(json!({}))));
        assert!(matches!(eval("return {kind='ui_canvas', key='c', options={}}"), Command::UiCanvas { options, .. } if options.visible && options.items.is_empty()));
        assert!(matches!(eval("return {kind='world_audio_update', key='k', options={}}"), Command::WorldAudioUpdate { .. }));
    }

    /// `sdk.menus` commands as the Lua wrappers submit them.
    #[test]
    fn menu_commands_from_lua() {
        let lua = Lua::new();
        let eval = |src: &str| -> Command { lua.from_value(lua.load(src).eval::<mlua::Value>().unwrap()).unwrap() };
        let cases = [
            "return {kind='menu_item', id='m.Races', options={label='Races', icon='map', category='SinglePlayer', modes={'Career'}, position=0}}",
            "return {kind='menu_item', id='GameSettings', options={label='Settings'}}",
            "return {kind='menu_category', id='m.Tab', options={image='icons/tab.png', modes={}}}",
            "return {kind='menu_setting', id='m.Bass', options={widget='slider', screen=3}}",
            "return {kind='menu_hide', entry='item', id='SkateFeed', modes={'Career', 'FreePlay'}}",
            "return {kind='menu_hide', entry='setting', id='ID_GAMESETTINGS_SFXPACK'}",
            "return {kind='menu_handle', entry='item', id='ChallengeMap'}",
            "return {kind='menu_handle', entry='setting', id='m.Bass', options={value=0.5, enabled={fact='game_mode', ne=4}}}",
            "return {kind='menu_handle', entry='item', id='m.Races', options={highlight=true, visible={any={true, {modes={'Career'}}}}, confirm={title='Sure?', when={['not']={fact='park_dirty', eq=0}}}}}",
            "return {kind='menu_unhandle', entry='item', id='m.Races'}",
            "return {kind='menu_set_value', id='m.Bass', value=1}",
            "return {kind='menu_set_value', id='ID_GAMESETTINGS_SUBTITLE_TOGGLE', value=true}",
        ];
        for case in cases {
            assert!(eval(case).validate(), "{case}");
        }
        for bad in [
            "return {kind='menu_hide', entry='tab', id='SkateFeed'}",
            "return {kind='menu_item', id='bad id', options={}}",
            "return {kind='menu_setting', id='m.x', options={widget='knob'}}",
            "return {kind='menu_handle', entry='item', id='x', options={enabled={fact='a', gt=1, lt=2}}}",
        ] {
            assert!(!eval(bad).validate(), "{bad}");
        }
        let movie = eval("return {kind='menu_movie', name='source/controls/panel', path='fe/panel.json'}");
        assert!(matches!(movie, Command::MenuMovie { .. }) && movie.validate());
        assert!(!eval("return {kind='menu_movie', name='source/controls/panel', path='../panel.json'}").validate());
        let bitmap = eval("return {kind='menu_bitmap', movie='source/screens/main/core_menu', bitmap=2, path='fe/glow.png'}");
        assert!(matches!(bitmap, Command::MenuBitmap { bitmap: 2, .. }) && bitmap.validate());
        assert!(!eval("return {kind='menu_bitmap', movie='source/screens/main/core_menu', bitmap=-1, path='fe/glow.png'}").validate());
        assert!(!eval("return {kind='menu_bitmap', movie='source/screens/main/core_menu', bitmap=2, path='fe/glow.json'}").validate());
        assert!(matches!(eval("return {kind='menu_set_value', id='a', value=0.25}"), Command::MenuSetValue { value: crate::menus::MenuValue::Float(v), .. } if v == 0.25));
    }
}
