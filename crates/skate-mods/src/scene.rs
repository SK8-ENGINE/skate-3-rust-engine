//! Generic presentation and attachment contracts. No vehicle-specific types.
use serde::{Deserialize, Serialize};

pub fn valid_key(key: &str) -> bool { crate::schema::valid_id(key) }
pub fn valid_node(name: &str) -> bool {
    !name.is_empty() && name.len() <= 120 && !name.chars().any(char::is_control)
}
pub fn valid_asset(path: &str) -> bool {
    path.is_empty() || (path.len() <= 256 && path.ends_with(".glb")
        && !path.starts_with('/') && !path.chars().any(|c| matches!(c, '\\' | ':' | '#'))
        && !path.chars().any(char::is_control)
        && path.split('/').all(|p| !p.is_empty() && p != "." && p != ".."))
}
pub fn valid_vector(v: &[f32;3], limit: f32) -> bool {
    v.iter().all(|x| x.is_finite() && x.abs() <= limit)
}
pub fn valid_quaternion(q: &[f32;4]) -> bool {
    q.iter().all(|x| x.is_finite() && x.abs() <= 10.)
        && q.iter().map(|x| x*x).sum::<f32>() > 1e-8
}
fn identity() -> [f32;4] { [0.,0.,0.,1.] }
fn unit_scale() -> [f32;3] { [1.;3] }
fn yes() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransformState {
    #[serde(default)] pub position: [f32;3],
    #[serde(default="identity")] pub rotation: [f32;4],
    #[serde(default="unit_scale")] pub scale: [f32;3],
}
impl Default for TransformState {
    fn default() -> Self { Self { position:[0.;3], rotation:identity(), scale:unit_scale() } }
}
impl TransformState {
    pub fn validate(&self) -> bool {
        valid_vector(&self.position,100_000.) && valid_quaternion(&self.rotation)
            && self.scale.iter().all(|x| x.is_finite() && *x > 0. && *x <= 100.)
    }
    pub fn apply(&mut self, options: &TransformOptions) {
        if let Some(v) = options.position { self.position = v; }
        if let Some(v) = options.rotation { self.rotation = v; }
        if let Some(v) = options.scale { self.scale = v; }
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformOptions {
    #[serde(default)] pub position: Option<[f32;3]>,
    #[serde(default)] pub rotation: Option<[f32;4]>,
    #[serde(default)] pub scale: Option<[f32;3]>,
    /// Optional parent-space rates for bounded remote visual extrapolation.
    #[serde(default)] pub linear_velocity: Option<[f32;3]>,
    #[serde(default)] pub angular_velocity: Option<[f32;3]>,
    /// Node only: true means a delta relative to the immutable authored pose.
    #[serde(default)] pub relative: Option<bool>,
}
impl TransformOptions {
    pub fn validate(&self) -> bool {
        self.position.as_ref().is_none_or(|p| valid_vector(p,100_000.))
            && self.rotation.as_ref().is_none_or(valid_quaternion)
            && self.scale.as_ref().is_none_or(|s| s.iter().all(|v| v.is_finite() && *v > 0. && *v <= 100.))
            && self.linear_velocity.as_ref().is_none_or(|p| valid_vector(p,1000.))
            && self.angular_velocity.as_ref().is_none_or(|p| valid_vector(p,10_000.))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NodeState {
    #[serde(default)] pub transform: TransformState,
    #[serde(default="yes")] pub relative: bool,
    #[serde(default)] pub linear_velocity: [f32;3],
    #[serde(default)] pub angular_velocity: [f32;3],
}
impl Default for NodeState {
    fn default() -> Self { Self { transform:TransformState::default(),relative:true,linear_velocity:[0.;3],angular_velocity:[0.;3] } }
}
impl NodeState {
    pub fn validate(&self) -> bool {
        self.transform.validate() && valid_vector(&self.linear_velocity,1000.) && valid_vector(&self.angular_velocity,10_000.)
    }
    pub fn apply(&mut self, options:&TransformOptions) {
        self.transform.apply(options);
        if let Some(v)=options.relative { self.relative=v; }
        // An explicit new pose with no rates is stationary, not stale motion.
        self.linear_velocity=options.linear_velocity.unwrap_or([0.;3]);
        self.angular_velocity=options.angular_velocity.unwrap_or([0.;3]);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphicsDefinition {
    pub path:String, pub body:Option<String>, pub color:[f32;3],
}
impl GraphicsDefinition {
    pub fn validate(&self) -> bool {
        valid_asset(&self.path) && self.body.as_deref().is_none_or(valid_key)
            && self.color.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v))
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DetachOptions {
    /// Candidate FOOT positions in the attached body's local coordinates.
    pub candidates:Vec<[f32;3]>, pub ground_snap:f32, pub height:f32, pub radius:f32,
}
impl Default for DetachOptions {
    fn default() -> Self { Self { candidates:Vec::new(),ground_snap:3.,height:1.8,radius:0.30 } }
}
impl DetachOptions {
    pub fn validate(&self) -> bool {
        self.candidates.len() <= 8 && self.candidates.iter().all(|p| valid_vector(p,100.))
            && self.ground_snap.is_finite() && (0.1..=10.).contains(&self.ground_snap)
            && self.height.is_finite() && (0.5..=3.).contains(&self.height)
            && self.radius.is_finite() && (0.1..=0.75).contains(&self.radius)
            && self.height >= 2.*self.radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn reject_unsafe_assets() {
        for p in ["../a.glb","/a.glb","C:/a.glb","a/../b.glb","a\\b.glb","a.glb#Scene0"] { assert!(!valid_asset(p)); }
        assert!(valid_asset("meshes/body.glb"));
    }
    #[test] fn reject_zero_quaternion_and_bad_exit() {
        assert!(!valid_quaternion(&[0.;4]));
        assert!(!DetachOptions { height:0.5,radius:0.4,..Default::default() }.validate());
    }
    #[test] fn patch_does_not_discard_other_axes() {
        let mut state=TransformState::default(); state.position=[1.,2.,3.];
        state.apply(&TransformOptions { rotation:Some([0.,1.,0.,0.]),..Default::default() });
        assert_eq!(state.position,[1.,2.,3.]); assert!(state.validate());
    }
}
