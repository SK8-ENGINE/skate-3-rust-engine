use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleDefinition {
    pub version: u32,
    pub model: String,
    pub model_scale: f32,
    pub model_offset: [f32; 3],
    pub model_yaw: f32,
    pub half_extents: [f32; 3],
    pub center_of_mass: [f32; 3],
    pub inertia_half_extents: Option<[f32; 3]>,
    pub collider_offset: [f32; 3],
    pub collider_rounding: f32,
    pub chassis_friction: f32,
    pub engine_audio: EngineAudio,
    pub rider_safety: RiderSafety,
    pub mass: f32,
    pub engine_force: f32,
    pub brake_impulse: f32,
    pub max_speed: f32,
    pub steering_angle: f32,
    pub suspension_length: f32,
    pub suspension_stiffness: f32,
    pub suspension_damping: f32,
    pub tire_grip: f32,
    pub ground_stability: f32,
    pub air_control: f32,
    pub wheels: Vec<WheelDefinition>,
    pub seat: [f32; 3],
    pub exit: [f32; 3],
    pub camera_distance: f32,
    pub camera_height: f32,
    pub animations: Animations,
}
/// Occupied-seat collision and automatic ejection. Coordinates are chassis-local.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RiderSafety {
    pub enabled: bool,
    pub offset: [f32; 3],
    pub radius: f32,
    pub half_height: f32,
    pub crash_delta_v: f32,
    pub hit_impulse: f32,
    pub inverted_up_y: f32,
    pub inverted_seconds: f32,
    pub eject_up_speed: f32,
}
impl Default for RiderSafety {
    fn default() -> Self { Self { enabled: true, offset: [0.,0.5,0.], radius: 0.25,
        half_height: 0.25, crash_delta_v: 6., hit_impulse: 180., inverted_up_y: -0.2,
        inverted_seconds: 0.2, eject_up_speed: 2. } }
}
/// Built-in synthesized engine; no external recording is required.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineAudio {
    pub enabled: bool,
    pub volume: f32,
    pub idle_pitch: f32,
    pub max_pitch: f32,
}
impl Default for EngineAudio {
    fn default() -> Self { Self { enabled: false, volume: 0.45, idle_pitch: 0.7, max_pitch: 2.8 } }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WheelDefinition {
    pub position: [f32; 3],
    pub radius: f32,
    pub steering: bool,
    pub driven: bool,
    #[serde(default)]
    pub node: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Animations {
    pub file: Option<String>,
    pub enter: Option<String>,
    pub exit: Option<String>,
    pub drive: Option<String>,
    pub idle: Option<String>,
    pub reverse: Option<String>,
    pub brake: Option<String>,
    pub steer_left: Option<String>,
    pub steer_right: Option<String>,
}
impl Default for VehicleDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            model: String::new(),
            model_scale: 1.,
            model_offset: [0.; 3],
            model_yaw: 0.,
            half_extents: [0.7, 0.2, 1.1],
            center_of_mass: [0.; 3],
            inertia_half_extents: None,
            collider_offset: [0.; 3],
            collider_rounding: 0.,
            chassis_friction: 0.3,
            engine_audio: EngineAudio::default(),
            rider_safety: RiderSafety::default(),
            mass: 220.,
            engine_force: 1800.,
            brake_impulse: 100.,
            max_speed: 25.,
            steering_angle: 0.5,
            suspension_length: 0.25,
            suspension_stiffness: 30.,
            suspension_damping: 4.,
            tire_grip: 1.3,
            ground_stability: 0.,
            air_control: 0.,
            wheels: vec![],
            seat: [0., 0.25, 0.],
            exit: [1.8, 0., 0.],
            camera_distance: 5.,
            camera_height: 2.,
            animations: Animations::default(),
        }
    }
}
pub fn package_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains([':', '\\', '#'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && !path.chars().any(char::is_control)
}
pub fn point(p: &[f32; 3], max: f32) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= max)
}
impl VehicleDefinition {
    pub fn validate(&self) -> Result<(), String> {
        let range = |v: f32, a: f32, b: f32| v.is_finite() && (a..=b).contains(&v);
        if self.version != 1 || !package_path(&self.model) || !self.model.ends_with(".glb") {
            return Err("Vehicle version must be 1 and model a package-relative GLB".into());
        }
        if !range(self.model_scale, 0.001, 100.)
            || !point(&self.model_offset, 100.)
            || !self.model_yaw.is_finite()
            || !self.half_extents.iter().all(|x| range(*x, 0.05, 10.))
            || !point(&self.center_of_mass, 10.)
            || self.inertia_half_extents.is_some_and(|v| !v.iter().all(|&x| range(x, 0.05, 10.)))
            || !point(&self.collider_offset, 10.)
            || !range(self.collider_rounding, 0., self.half_extents.iter().copied().fold(f32::INFINITY, f32::min) * 0.95)
            || !range(self.chassis_friction, 0., 2.)
            || !range(self.engine_audio.volume, 0., 1.)
            || !range(self.engine_audio.idle_pitch, 0.25, 2.)
            || !range(self.engine_audio.max_pitch, self.engine_audio.idle_pitch, 5.)
            || !point(&self.rider_safety.offset, 5.)
            || !range(self.rider_safety.radius, 0.1, 1.)
            || !range(self.rider_safety.half_height, 0.05, 1.)
            || !range(self.rider_safety.crash_delta_v, 1., 50.)
            || !range(self.rider_safety.hit_impulse, 10., 10000.)
            || !range(self.rider_safety.inverted_up_y, -1., 0.5)
            || !range(self.rider_safety.inverted_seconds, 0.05, 3.)
            || !range(self.rider_safety.eject_up_speed, 0., 10.)
            || !range(self.mass, 10., 10000.)
            || !range(self.engine_force, 0., 100000.)
            || !range(self.brake_impulse, 0., 10000.)
            || !range(self.max_speed, 1., 100.)
            || !range(self.steering_angle, 0.01, 1.2)
            || !range(self.suspension_length, 0.01, 2.)
            || !range(self.suspension_stiffness, 1., 200.)
            || !range(self.suspension_damping, 0.1, 30.)
            || !range(self.tire_grip, 0.1, 20.)
            || !range(self.ground_stability, 0., 1.)
            || !range(self.air_control, 0., 10.)
            || !point(&self.seat, 10.)
            || !point(&self.exit, 10.)
            || !range(self.camera_distance, 2., 30.)
            || !range(self.camera_height, 0.5, 15.)
        {
            return Err("Vehicle dimensions/tuning outside supported finite ranges".into());
        }
        if !(2..=8).contains(&self.wheels.len())
            || !self.wheels.iter().any(|w| w.driven)
            || !self.wheels.iter().all(|w| {
                point(&w.position, 10.)
                    && range(w.radius, 0.05, 2.)
                    && w.node
                        .as_ref()
                        .is_none_or(|n| !n.is_empty() && n.len() <= 128)
            })
        {
            return Err("Vehicle needs 2..8 valid wheels and at least one driven wheel".into());
        }
        if self
            .animations
            .file
            .as_ref()
            .is_some_and(|p| !package_path(p))
        {
            return Err("Invalid vehicle animation path".into());
        }
        for name in [
            &self.animations.enter,
            &self.animations.exit,
            &self.animations.drive,
            &self.animations.idle,
            &self.animations.reverse,
            &self.animations.brake,
            &self.animations.steer_left,
            &self.animations.steer_right,
        ]
        .into_iter()
        .flatten()
        {
            if self.animations.file.is_none() || name.is_empty() || name.len() > 128 {
                return Err("Animation slots require a file and valid clip names".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleTuning {
    pub engine_volume: Option<f32>,
    pub engine_force: Option<f32>,
    pub max_speed: Option<f32>,
    pub brake_impulse: Option<f32>,
    pub steering_angle: Option<f32>,
    pub tire_grip: Option<f32>,
}
impl VehicleTuning {
    pub fn apply(&self, definition: &VehicleDefinition) -> Result<VehicleDefinition, String> {
        let mut d = definition.clone();
        if let Some(v) = self.engine_volume { d.engine_audio.volume = v; }
        if let Some(v) = self.engine_force {
            d.engine_force = v;
        }
        if let Some(v) = self.max_speed {
            d.max_speed = v;
        }
        if let Some(v) = self.brake_impulse {
            d.brake_impulse = v;
        }
        if let Some(v) = self.steering_angle {
            d.steering_angle = v;
        }
        if let Some(v) = self.tire_grip {
            d.tire_grip = v;
        }
        d.validate()?;
        Ok(d)
    }
    pub fn valid(&self) -> bool {
        [
            self.engine_volume.map(|x| (x, 0., 1.)),
            self.engine_force.map(|x| (x, 0., 100000.)),
            self.max_speed.map(|x| (x, 1., 100.)),
            self.brake_impulse.map(|x| (x, 0., 10000.)),
            self.steering_angle.map(|x| (x, 0.01, 1.2)),
            self.tire_grip.map(|x| (x, 0.1, 20.)),
        ]
        .into_iter()
        .flatten()
        .all(|(x, a, b)| x.is_finite() && (a..=b).contains(&x))
    }
}
