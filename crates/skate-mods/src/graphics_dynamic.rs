//! Graphics extension 3: procedural mesh buffers and scene lights.
use serde::Deserialize;

pub const MAX_MESH_BUFFER_VERTICES: usize = 8192;
pub const MAX_MESH_BUFFER_INDICES: usize = 24_576;
pub const MAX_MESH_BUFFER_APPEND_VERTICES: usize = 256;

fn one_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn white() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn point_kind() -> String {
    "point".into()
}
fn default_intensity() -> f32 {
    1000.0
}
fn default_range() -> f32 {
    10.0
}
fn default_inner() -> f32 {
    0.4
}
fn default_outer() -> f32 {
    0.7
}
fn true_fn() -> bool {
    true
}

fn between(x: f32, lo: f32, hi: f32) -> bool {
    x.is_finite() && (lo..=hi).contains(&x)
}
fn point(v: &[f32; 3]) -> bool {
    v.iter().all(|x| between(*x, -100_000.0, 100_000.0))
}
fn offset(v: &[f32; 3]) -> bool {
    v.iter().all(|x| between(*x, -100.0, 100.0))
}
fn normal(v: &[f32; 3]) -> bool {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    len.is_finite() && len > 1e-4 && len <= 2.0
}
fn color3(v: &[f32; 3]) -> bool {
    v.iter().all(|x| between(*x, 0.0, 1.0))
}
fn quat(v: &[f32; 4]) -> bool {
    crate::scene::valid_quaternion(v)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshBufferOptions {
    #[serde(default)] pub body: Option<String>,
    #[serde(default)] pub position: Option<[f32; 3]>,
    #[serde(default)] pub rotation: Option<[f32; 4]>,
    #[serde(default = "one_scale")] pub scale: [f32; 3],
    #[serde(default = "true_fn")] pub blend: bool,
    #[serde(default = "true_fn")] pub unlit: bool,
    #[serde(default = "true_fn")] pub visible: bool,
    #[serde(default)] pub depth_bias: f32,
    #[serde(default)] pub texture: Option<String>,
    #[serde(default = "white")] pub tint: [f32; 3],
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshBufferWrite {
    #[serde(default)] pub positions: Vec<[f32; 3]>,
    #[serde(default)] pub normals: Option<Vec<[f32; 3]>>,
    /// Flat RGBA per vertex: `[r,g,b,a, r,g,b,a, ...]`.
    #[serde(default)] pub colors: Option<Vec<f32>>,
    /// Flat UV pairs: `[u,v, u,v, ...]`.
    #[serde(default)] pub uvs: Option<Vec<f32>>,
    #[serde(default)] pub indices: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightOptions {
    #[serde(default = "point_kind")] pub kind: String,
    #[serde(default)] pub body: Option<String>,
    #[serde(default)] pub position: Option<[f32; 3]>,
    #[serde(default)] pub offset: [f32; 3],
    #[serde(default)] pub direction: Option<[f32; 3]>,
    #[serde(default = "white")] pub color: [f32; 3],
    #[serde(default = "default_intensity")] pub intensity: f32,
    #[serde(default = "default_range")] pub range: f32,
    #[serde(default = "default_inner")] pub inner_angle: f32,
    #[serde(default = "default_outer")] pub outer_angle: f32,
}

impl MeshBufferOptions {
    pub fn validate(&self) -> bool {
        self.body.as_deref().is_none_or(crate::schema::valid_id)
            && self.position.as_ref().is_none_or(point)
            && self.rotation.as_ref().is_none_or(quat)
            && self.scale.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 100.0)
            && self.depth_bias.is_finite() && self.depth_bias.abs() <= 10.0
            && self.texture.as_deref().is_none_or(valid_texture)
            && color3(&self.tint)
    }
}

pub fn valid_texture_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.to_ascii_lowercase().ends_with(".png")
        && !path.chars().any(|c| matches!(c, '\\' | ':' | '#' | '?'))
        && !path.chars().any(char::is_control)
        && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

fn valid_texture(path: &str) -> bool {
    valid_texture_path(path)
}

impl MeshBufferWrite {
    pub fn validate(&self) -> bool {
        if self.positions.is_empty() || self.positions.len() > MAX_MESH_BUFFER_VERTICES {
            return false;
        }
        if self.indices.is_empty() || self.indices.len() > MAX_MESH_BUFFER_INDICES {
            return false;
        }
        if !self.positions.iter().all(|p| point(p)) {
            return false;
        }
        if self.normals.as_ref().is_some_and(|n| n.len() != self.positions.len() || !n.iter().all(normal)) {
            return false;
        }
        if self.colors.as_ref().is_some_and(|c| {
            c.len() != self.positions.len() * 4 || !c.iter().all(|v| between(*v, 0.0, 1.0))
        }) {
            return false;
        }
        if self.uvs.as_ref().is_some_and(|u| {
            u.len() != self.positions.len() * 2 || !u.iter().all(|v| between(*v, -1000.0, 1000.0))
        }) {
            return false;
        }
        let max = self.positions.len() as u32;
        self.indices.iter().all(|i| *i < max)
    }

    /// Append new vertices/indices onto an existing buffer. Indices are absolute in the
    /// combined mesh (`0..base_verts + new_positions`).
    pub fn validate_append(&self, base_verts: usize, base_indices: usize) -> bool {
        if self.positions.is_empty() || self.positions.len() > MAX_MESH_BUFFER_APPEND_VERTICES {
            return false;
        }
        let total_verts = base_verts + self.positions.len();
        if total_verts > MAX_MESH_BUFFER_VERTICES {
            return false;
        }
        if !self.positions.iter().all(|p| point(p)) {
            return false;
        }
        if self.normals.as_ref().is_some_and(|n| n.len() != self.positions.len() || !n.iter().all(normal)) {
            return false;
        }
        if self.colors.as_ref().is_some_and(|c| {
            c.len() != self.positions.len() * 4 || !c.iter().all(|v| between(*v, 0.0, 1.0))
        }) {
            return false;
        }
        if self.uvs.as_ref().is_some_and(|u| {
            u.len() != self.positions.len() * 2 || !u.iter().all(|v| between(*v, -1000.0, 1000.0))
        }) {
            return false;
        }
        if self.indices.is_empty() {
            return true;
        }
        let total_indices = base_indices + self.indices.len();
        if total_indices > MAX_MESH_BUFFER_INDICES {
            return false;
        }
        self.indices.iter().all(|i| *i < total_verts as u32)
    }
}

impl LightOptions {
    pub fn validate(&self) -> bool {
        matches!(self.kind.as_str(), "point" | "spot")
            && self.body.as_deref().is_none_or(crate::schema::valid_id)
            && self.position.as_ref().is_none_or(point)
            && !(self.body.is_some() && self.position.is_some())
            && offset(&self.offset)
            && self.direction.as_ref().is_none_or(normal)
            && color3(&self.color)
            && between(self.intensity, 0.0, 1_000_000.0)
            && between(self.range, 0.1, 200.0)
            && between(self.inner_angle, 0.0, std::f32::consts::FRAC_PI_2)
            && between(self.outer_angle, self.inner_angle, std::f32::consts::PI)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn texture_paths() {
        assert!(valid_texture_path("textures/skid_tread.png"));
        assert!(!valid_texture_path("builtin:skid"));
        assert!(!valid_texture_path("../textures/skid.png"));
        assert!(!valid_texture_path("textures/skid.jpg"));
    }

    #[test]
    fn mesh_buffer_commands() {
        assert!(MeshBufferWrite {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            colors: Some(vec![0.9, 0.9, 0.9, 0.4, 0.9, 0.9, 0.9, 0.4, 0.9, 0.9, 0.9, 0.4]),
            indices: vec![0, 1, 2],
            ..Default::default()
        }
        .validate());
        assert!(MeshBufferWrite {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            uvs: Some(vec![0.0, 0.0, 1.0, 0.0]),
            indices: vec![],
            ..Default::default()
        }
        .validate_append(4, 6));
        assert!(!MeshBufferWrite {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            indices: vec![3, 4, 5, 4, 6, 5],
            ..Default::default()
        }
        .validate_append(4, 6));
        for kind in [
            "graphics_mesh_buffer",
            "graphics_mesh_buffer_write",
            "graphics_mesh_buffer_append",
            "graphics_light",
        ] {
            let value = match kind {
                "graphics_mesh_buffer" => json!({"kind": kind, "key": "buf", "options": {"blend": true}}),
                "graphics_mesh_buffer_write" | "graphics_mesh_buffer_append" => json!({
                    "kind": kind,
                    "key": "buf",
                    "data": {
                        "positions": [[0,0,0],[1,0,0],[0,1,0]],
                        "indices": [0,1,2]
                    }
                }),
                "graphics_light" => json!({"kind": kind, "key": "l", "options": {"intensity": 100}}),
                _ => json!({}),
            };
            let c: crate::Command = serde_json::from_value(value).unwrap();
            assert!(c.validate());
        }
    }
}
