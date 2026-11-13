//! Bounded, renderer-independent camera and 2D canvas contracts (extension 1).
//! No entity handles, arbitrary asset paths, shader code, or physics writes.
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraMode {
    #[default]
    Chase,
    Hood,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraRigOptions {
    pub mode: CameraMode,
    pub distance: f32,
    pub distance_gain: f32,
    pub height: f32,
    pub height_gain: f32,
    pub target_height: f32,
    /// Maximum velocity look-ahead, in seconds (bounded in distance by renderer).
    pub look_ahead: f32,
    /// Contribution of forward travel direction to chase heading; not steering.
    pub velocity_heading: f32,
    pub spring_hz: f32,
    pub heading_half_life: f32,
    pub acceleration_lag: f32,
    pub speed_reference: f32,
    /// Vertical perspective field of view in degrees.
    pub fov: f32,
    pub fov_gain: f32,
    pub near: f32,
    pub collision_radius: f32,
    pub collision: bool,
    pub hood_offset: [f32; 3],
}
impl Default for CameraRigOptions {
    fn default() -> Self {
        Self {
            mode: CameraMode::Chase,
            distance: 5.8, distance_gain: 1.8,
            height: 1.9, height_gain: 0.25, target_height: 0.78,
            look_ahead: 0.12, velocity_heading: 0.18,
            spring_hz: 2.8, heading_half_life: 0.14,
            acceleration_lag: 0.018, speed_reference: 55.556,
            fov: 55.0, fov_gain: 8.0, near: 0.07,
            collision_radius: 0.28, collision: true,
            hood_offset: [0.0, 0.48, 1.05],
        }
    }
}
fn range(v: f32, lo: f32, hi: f32) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}
impl CameraRigOptions {
    pub fn validate(&self) -> bool {
        range(self.distance, 1.0, 20.0)
            && range(self.distance_gain, 0.0, 10.0)
            && range(self.height, 0.2, 8.0)
            && range(self.height_gain, 0.0, 3.0)
            && range(self.target_height, -2.0, 5.0)
            && range(self.look_ahead, 0.0, 0.5)
            && range(self.velocity_heading, 0.0, 0.5)
            && range(self.spring_hz, 0.5, 12.0)
            && range(self.heading_half_life, 0.01, 2.0)
            && range(self.acceleration_lag, 0.0, 0.1)
            && range(self.speed_reference, 10.0, 100.0)
            && range(self.fov, 25.0, 100.0)
            && range(self.fov_gain, 0.0, 20.0)
            && self.fov + self.fov_gain <= 110.0
            && range(self.near, 0.02, 1.0)
            && range(self.collision_radius, 0.08, 1.0)
            && self.hood_offset.iter().all(|x| range(*x, -10.0, 10.0))
    }
}

/// Retail "Camera Angle" game setting (Game Settings > Control Settings): the stock
/// camera graph's `IsCameraTypeActive` type 0 (Low) or 1 (High).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAngle {
    Low,
    High,
}

/// Replacement values for one stock camera shot (`camera_shots` collection), named
/// after the retail attributes and in their units (metres, degrees, seconds). Unset
/// fields keep the stock value. Applies to every place the shot is used, including
/// blend trees that name it as a child.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraShotTuning {
    #[serde(rename = "PositionDistance")]
    pub position_distance: Option<f32>,
    #[serde(rename = "PositionElevation")]
    pub position_elevation: Option<f32>,
    #[serde(rename = "PositionHeading")]
    pub position_heading: Option<f32>,
    #[serde(rename = "FramingLensLength")]
    pub framing_lens_length: Option<f32>,
    #[serde(rename = "FramingRoll")]
    pub framing_roll: Option<f32>,
    #[serde(rename = "FramingYaw")]
    pub framing_yaw: Option<f32>,
    #[serde(rename = "FramingPitch")]
    pub framing_pitch: Option<f32>,
    #[serde(rename = "ReferenceBoardOffset")]
    pub reference_board_offset: Option<f32>,
    #[serde(rename = "SmoothingDirection")]
    pub smoothing_direction: Option<f32>,
    #[serde(rename = "SmoothingElevation")]
    pub smoothing_elevation: Option<f32>,
    #[serde(rename = "SmoothingYaw")]
    pub smoothing_yaw: Option<f32>,
    #[serde(rename = "SmoothingPitch")]
    pub smoothing_pitch: Option<f32>,
    #[serde(rename = "TransitionTime")]
    pub transition_time: Option<f32>,
}
impl CameraShotTuning {
    pub fn validate(&self) -> bool {
        let ok = |v: Option<f32>, lo: f32, hi: f32| v.is_none_or(|v| range(v, lo, hi));
        ok(self.position_distance, 0.0, 50.0)
            && ok(self.position_elevation, -90.0, 90.0)
            && ok(self.position_heading, -360.0, 360.0)
            && ok(self.framing_lens_length, 1.0, 200.0)
            && ok(self.framing_roll, -180.0, 180.0)
            && ok(self.framing_yaw, -180.0, 180.0)
            && ok(self.framing_pitch, -90.0, 90.0)
            && ok(self.reference_board_offset, -5.0, 5.0)
            && [self.smoothing_direction, self.smoothing_elevation, self.smoothing_yaw, self.smoothing_pitch]
                .into_iter().all(|v| ok(v, 0.0, 100.0))
            && ok(self.transition_time, 0.0, 10.0)
    }
}

/// Replacement values for the water look layer (family 33 water: DownTown
/// fountains, the Aletown canal). Unset fields keep the player's / default
/// value. `enabled = false` gives the plain retail `water_defaultPS` look.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaterLookPatch {
    pub enabled: Option<bool>,
    /// Ripple texture scale multiplier (retail 1).
    pub ripple_scale: Option<f32>,
    /// Extra environment cube mip levels: a blurrier reflection (retail 0).
    pub reflection_blur: Option<f32>,
    /// Reflection colour multiplier (retail 1, 1, 1).
    pub reflection_tint: Option<[f32; 3]>,
    /// Third, rotated wave sample against visible tiling.
    pub third_wave: Option<bool>,
    /// Calmer motion on small water bodies.
    pub body_calming: Option<bool>,
    /// Wave animation speed relative to retail's 30 Hz (retail 1).
    pub wave_rate: Option<f32>,
    /// Water texture scroll speed relative to retail (retail 1).
    pub scroll_rate: Option<f32>,
    /// Scroll clock that never visibly restarts (default on; false gives the
    /// ported routine's restart every 10 s).
    pub seamless_clock: Option<bool>,
    /// The look on flowing water too (`water.flowing`: University's fountains; retail off).
    pub flowing_water: Option<bool>,
    /// Ripple texture scale multiplier on flowing water (retail 1).
    pub flowing_ripple_scale: Option<f32>,
    /// Splash in shallow water (fountains) as a share of the deep-water plume, 0 = none (retail none).
    pub shallow_splash: Option<f32>,
    /// Calm open-sea swells (`ocean.default`) as a slope amplitude, 0 = none (retail none).
    pub ocean_swell: Option<f32>,
    /// Wavelength of the longest swell in metres.
    pub ocean_swell_length: Option<f32>,
    /// Speed of the longest swell in m/s.
    pub ocean_swell_speed: Option<f32>,
    /// Fountain basins take the large water's waves, scaled down (retail off).
    pub fountain_water: Option<bool>,
    /// Metres per basin wave tile.
    pub fountain_tile: Option<f32>,
    /// Least wave strength on small basins, 0..1 (retail 0).
    pub fountain_waves: Option<f32>,
    /// Cascade scroll speed relative to retail (retail 1).
    pub cascade_rate: Option<f32>,
    /// Foam on cascades, 0 = none (retail none).
    pub cascade_foam: Option<f32>,
    /// Basin body colour under the lighting (linear rgb, 0..1 each).
    pub fountain_colour: Option<[f32; 3]>,
    /// Basin reflection strength as a share of the material's own mask, 0..2.
    pub fountain_reflection: Option<f32>,
    /// Basin wave tilt gain, 0..8 (1 = the waves as the large water draws them).
    pub fountain_ripple: Option<f32>,
    /// Props in water sink or float by their own density, with water drag (retail: they fall through to the floor).
    pub prop_water: Option<bool>,
    /// Water density for prop buoyancy, kg/m3 (100..5000).
    pub water_density: Option<f32>,
    /// Linear water drag on submerged props, per second (0..20).
    pub prop_water_drag: Option<f32>,
    /// Angular water drag on submerged props, per second (0..20).
    pub prop_water_spin_drag: Option<f32>,
    /// Deep water carries every prop: its density counts as at most this, kg/m3 (10..5000).
    pub prop_deep_float_density: Option<f32>,
    /// Waves props bob on in deep water: height in m (0..1) and period in s (0.5..30).
    pub prop_bob_height: Option<f32>,
    pub prop_bob_period: Option<f32>,
    /// Material density from which a prop always sinks, kg/m3 (10..20000).
    pub prop_heavy_density: Option<f32>,
    /// In water too deep to see the bottom: how far under (m) a sinking prop has faded out, 0..20 (0 = no fade).
    pub prop_fade_depth: Option<f32>,
    /// A sunk prop touching the floor below this speed (m/s, 0..5) gets no water forces and can sleep (ours).
    pub prop_settle_speed: Option<f32>,
}
impl WaterLookPatch {
    pub fn validate(&self) -> bool {
        let ok = |v: Option<f32>, lo: f32, hi: f32| v.is_none_or(|v| range(v, lo, hi));
        ok(self.ripple_scale, 0.1, 16.0)
            && ok(self.water_density, 100.0, 5000.0)
            && ok(self.prop_water_drag, 0.0, 20.0)
            && ok(self.prop_water_spin_drag, 0.0, 20.0)
            && ok(self.prop_deep_float_density, 10.0, 5000.0)
            && ok(self.prop_bob_height, 0.0, 1.0)
            && ok(self.prop_bob_period, 0.5, 30.0)
            && ok(self.prop_heavy_density, 10.0, 20000.0)
            && ok(self.prop_fade_depth, 0.0, 20.0)
            && ok(self.prop_settle_speed, 0.0, 5.0)
            && ok(self.fountain_tile, 0.5, 100.0)
            && ok(self.fountain_waves, 0.0, 1.0)
            && ok(self.cascade_rate, 0.0, 8.0)
            && ok(self.cascade_foam, 0.0, 2.0)
            && ok(self.fountain_reflection, 0.0, 2.0)
            && ok(self.fountain_ripple, 0.0, 8.0)
            && self.fountain_colour.is_none_or(|c| c.iter().all(|&v| range(v, 0.0, 1.0)))
            && ok(self.flowing_ripple_scale, 0.1, 16.0)
            && ok(self.shallow_splash, 0.0, 2.0)
            && ok(self.ocean_swell, 0.0, 0.3)
            && ok(self.ocean_swell_length, 1.0, 400.0)
            && ok(self.ocean_swell_speed, 0.0, 40.0)
            && ok(self.reflection_blur, 0.0, 8.0)
            && self.reflection_tint.is_none_or(|t| t.iter().all(|&c| range(c, 0.0, 4.0)))
            && ok(self.wave_rate, 0.0, 4.0)
            && ok(self.scroll_rate, 0.0, 4.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasAnchor {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasKind {
    Rect,
    #[default]
    Text,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CanvasItem {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: CanvasKind,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub text: String,
    pub font_size: f32,
    pub color: [f32; 4],
}
impl Default for CanvasItem {
    fn default() -> Self {
        Self { key: String::new(), kind: CanvasKind::Text,
            position: [0.0, 0.0], size: [100.0, 30.0], text: String::new(),
            font_size: 20.0, color: [1.0; 4] }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CanvasOptions {
    pub anchor: CanvasAnchor,
    /// Pixel margins from the selected viewport edges, before scale.
    pub offset: [f32; 2],
    /// Design-space dimensions. Children use top-left-relative coordinates.
    pub size: [f32; 2],
    pub scale: f32,
    pub visible: bool,
    #[serde(deserialize_with = "crate::lua_list::list")]
    pub items: Vec<CanvasItem>,
}
impl Default for CanvasOptions {
    fn default() -> Self {
        Self { anchor: CanvasAnchor::BottomRight, offset: [24.0, 24.0],
            size: [340.0, 176.0], scale: 1.0, visible: true, items: Vec::new() }
    }
}
impl CanvasOptions {
    pub fn validate(&self) -> bool {
        if !self.offset.iter().all(|x| range(*x, 0.0, 4096.0))
            || !self.size.iter().all(|x| range(*x, 1.0, 2048.0))
            || !range(self.scale, 0.5, 2.0) || self.items.len() > 64 {
            return false;
        }
        let mut keys = std::collections::BTreeSet::new();
        self.items.iter().all(|i| {
            crate::schema::valid_id(&i.key) && keys.insert(&i.key)
                && i.position.iter().all(|x| range(*x, 0.0, 2048.0))
                && i.size.iter().all(|x| range(*x, 0.0, 2048.0))
                && i.position[0] + i.size[0] <= self.size[0] + 0.1
                && i.position[1] + i.size[1] <= self.size[1] + 0.1
                && i.text.len() <= 256 && range(i.font_size, 6.0, 160.0)
                && i.color.iter().all(|x| range(*x, 0.0, 1.0))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rig_defaults_are_valid() { assert!(CameraRigOptions::default().validate()); }
    #[test]
    fn reject_nonfinite_and_unbounded_camera() {
        let mut o=CameraRigOptions::default(); o.fov=f32::NAN; assert!(!o.validate());
        o.fov=100.0; o.fov_gain=20.0; assert!(!o.validate());
        o.fov=55.0; o.near=0.0; assert!(!o.validate());
    }
    #[test]
    fn camera_shot_tuning_uses_retail_names_and_rejects_bad_values() {
        let t: CameraShotTuning = serde_json::from_str(r#"{"PositionDistance":2.5,"PositionElevation":-4}"#).unwrap();
        assert_eq!(t.position_distance, Some(2.5));
        assert_eq!(t.position_elevation, Some(-4.0));
        assert!(t.validate());
        assert!(serde_json::from_str::<CameraShotTuning>(r#"{"distance":2.5}"#).is_err());
        let bad = CameraShotTuning { position_distance: Some(f32::NAN), ..Default::default() };
        assert!(!bad.validate());
        let far = CameraShotTuning { position_distance: Some(500.0), ..Default::default() };
        assert!(!far.validate());
        assert_eq!(serde_json::from_str::<CameraAngle>(r#""low""#).unwrap(), CameraAngle::Low);
        assert!(serde_json::from_str::<CameraAngle>(r#""medium""#).is_err());
    }
    #[test]
    fn reject_duplicate_canvas_keys_and_out_of_bounds() {
        let mut o=CanvasOptions::default();
        let item=CanvasItem { key:"speed".into(), ..Default::default() };
        o.items.push(item.clone()); assert!(o.validate());
        o.items.push(item); assert!(!o.validate());
        o.items.pop(); o.items[0].size[0]=999.0; assert!(!o.validate());
    }
    #[test]
    fn zero_alpha_and_zero_width_are_valid() {
        let mut o=CanvasOptions::default();
        o.items.push(CanvasItem { key:"rpm".into(), kind:CanvasKind::Rect,
            size:[0.0,10.0], color:[0.0;4], ..Default::default() });
        assert!(o.validate());
    }
}
