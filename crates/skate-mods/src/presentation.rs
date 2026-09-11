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
