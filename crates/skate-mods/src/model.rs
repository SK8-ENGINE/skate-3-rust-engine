//! Collision from an existing render GLB. Reads embedded geometry only: no
//! image decoding, external buffer URLs, sidecar files, or generated assets.
use skate_dynamics::{ModelColliderOptions, Shape, model::{MAX_MODEL_VERTICES, MAX_MODEL_TRIANGLES}};
use std::{collections::{BTreeMap, HashMap}, io::Read, path::Path, sync::{Mutex, OnceLock}};

const MAX_GLB_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 16;
static CACHE: OnceLock<Mutex<BTreeMap<[u8; 32], Shape>>> = OnceLock::new();
type Matrix = [[f32; 4]; 4];

#[derive(Debug)]
pub struct ModelGeometry {
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("model {}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_GLB_BYTES {
        return Err("model GLB exceeds 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_GLB_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_GLB_BYTES { return Err("model GLB exceeds 64 MiB".into()); }
    Ok(bytes)
}

/// Same Scene0 coordinates that graphics.mesh renders, including ALL ancestor
/// translations, rotations and scales. object is a node/mesh selector, not a
/// request to remove that node's translation (unlike legacy Shape::Mesh).
pub fn read_geometry_file(path: &Path, object: &str, options: &ModelColliderOptions) -> Result<ModelGeometry, String> {
    read_geometry(&read_bytes(path)?, object, options)
}

/// Cache is RAM-only and keyed by exact source bytes, selector and cook options.
/// Resets do not recook unchanged geometry; editing the visual GLB invalidates
/// the cache automatically. No collision file is required or written.
pub fn model_shape_file(path: &Path, object: &str, options: &ModelColliderOptions) -> Result<Shape, String> {
    options.validate()?;
    let bytes = read_bytes(path)?;
    let mut hash = blake3::Hasher::new();
    hash.update(&bytes);
    hash.update(&(object.len() as u64).to_le_bytes());
    hash.update(object.as_bytes());
    hash.update(&serde_json::to_vec(options).map_err(|e| e.to_string())?);
    let key = *hash.finalize().as_bytes();
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(shape) = cache.lock().map_err(|_| "model cache unavailable")?.get(&key).cloned() {
        return Ok(shape);
    }
    let geometry = read_geometry(&bytes, object, options)?;
    let shape = skate_dynamics::model::decompose(&geometry.vertices, &geometry.triangles, options)?;
    let mut entries = cache.lock().map_err(|_| "model cache unavailable")?;
    if entries.len() >= MAX_CACHE_ENTRIES { entries.pop_first(); }
    entries.insert(key, shape.clone());
    Ok(shape)
}

fn identity() -> Matrix {
    [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]
}
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|column| std::array::from_fn(|row| (0..4).map(|k| a[k][row] * b[column][k]).sum()))
}
fn point(matrix: Matrix, p: [f32; 3], scale: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| (matrix[0][row] * p[0] + matrix[1][row] * p[1] + matrix[2][row] * p[2] + matrix[3][row]) * scale[row])
}
fn determinant(m: Matrix, scale: [f32; 3]) -> f32 {
    (m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[1][0] * (m[0][1] * m[2][2] - m[0][2] * m[2][1])
        + m[2][0] * (m[0][1] * m[1][2] - m[0][2] * m[1][1])) * scale[0] * scale[1] * scale[2]
}
struct Collector<'a> {
    blob: &'a [u8],
    object: &'a str,
    options: &'a ModelColliderOptions,
    geometry: ModelGeometry,
    welded: HashMap<[u32; 3], u32>,
    active: Vec<bool>,
    visits: usize,
    raw_vertices: usize,
}
impl Collector<'_> {
    fn node(&mut self, node: gltf::Node<'_>, parent: Matrix, selected: bool, depth: usize) -> Result<(), String> {
        if depth > 128 || self.visits >= 4096 || self.active[node.index()] { return Err("model scene is cyclic or too deep/large".into()); }
        self.visits += 1;
        self.active[node.index()] = true;
        let transform = multiply(parent, node.transform().matrix());
        if transform.iter().flatten().any(|v| !v.is_finite()) { return Err("non-finite scene transform".into()); }
        let selected = selected || self.object.is_empty() || node.name() == Some(self.object);
        if let Some(mesh) = node.mesh() {
            if selected || mesh.name() == Some(self.object) {
                if node.skin().is_some() { return Err("model collision requires a rigid GLB node, not a skin".into()); }
                for primitive in mesh.primitives() { self.primitive(primitive, transform)?; }
            }
        }
        for child in node.children() { self.node(child, transform, selected, depth + 1)?; }
        self.active[node.index()] = false;
        Ok(())
    }
    fn primitive(&mut self, primitive: gltf::mesh::Primitive<'_>, transform: Matrix) -> Result<(), String> {
        use gltf::mesh::Mode;
        if !matches!(primitive.mode(), Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan) {
            return Err("selected model contains a non-triangle primitive".into());
        }
        if primitive.morph_targets().next().is_some() {
            return Err("model collision does not cook animated morph targets".into());
        }
        let blob = self.blob;
        let reader = primitive.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => Some(blob),
            gltf::buffer::Source::Uri(_) => None,
        });
        let positions: Vec<_> = reader.read_positions().ok_or("model primitive has no positions")?
            .take(MAX_MODEL_VERTICES + 1).collect();
        self.raw_vertices += positions.len();
        if self.raw_vertices > MAX_MODEL_VERTICES { return Err("model exceeds 65536 source vertices; no subsampling was applied".into()); }
        let mut remap = Vec::with_capacity(positions.len());
        for position in positions {
            let mut p = point(transform, position, self.options.scale);
            if p.iter().any(|v| !v.is_finite() || v.abs() > 1000.) { return Err("model vertex outside finite geometry bounds".into()); }
            // Weld equal positions across UV/normal/material seams, not spatial
            // samples. Every triangle still refers to its real source vertices.
            for value in &mut p { if *value == 0. { *value = 0.; } }
            let key = p.map(f32::to_bits);
            let index = match self.welded.get(&key) {
                Some(&index) => index,
                None => {
                    let index = self.geometry.vertices.len() as u32;
                    self.geometry.vertices.push(p);
                    self.welded.insert(key, index);
                    index
                }
            };
            remap.push(index);
        }
        let limit = MAX_MODEL_TRIANGLES * 3 + 2;
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().take(limit + 1).collect(),
            None => (0..remap.len() as u32).collect(),
        };
        if indices.len() > limit || indices.iter().any(|&i| i as usize >= remap.len()) {
            return Err("model triangle indices are invalid or oversized".into());
        }
        let reflected = determinant(transform, self.options.scale) < 0.;
        match primitive.mode() {
            Mode::Triangles => {
                if indices.len() % 3 != 0 { return Err("triangle index count is not divisible by three".into()); }
                for t in indices.chunks_exact(3) { self.triangle([remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]], reflected)?; }
            }
            Mode::TriangleStrip => {
                for (i, t) in indices.windows(3).enumerate() {
                    let mut triangle = [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]];
                    if i % 2 == 1 { triangle.swap(0, 1); }
                    self.triangle(triangle, reflected)?;
                }
            }
            Mode::TriangleFan => {
                if let Some(&first) = indices.first() {
                    for t in indices[1..].windows(2) { self.triangle([remap[first as usize], remap[t[0] as usize], remap[t[1] as usize]], reflected)?; }
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn triangle(&mut self, mut triangle: [u32; 3], reflected: bool) -> Result<(), String> {
        use skate_dynamics::rapier3d::prelude::Vector;
        if triangle[0] == triangle[1] || triangle[0] == triangle[2] || triangle[1] == triangle[2] { return Ok(()); }
        let [a, b, c] = triangle.map(|i| Vector::from_array(self.geometry.vertices[i as usize]));
        if (b - a).cross(c - a).length_squared() <= 1e-18 { return Ok(()); }
        if self.geometry.triangles.len() >= MAX_MODEL_TRIANGLES { return Err("model exceeds triangle budget".into()); }
        if reflected { triangle.swap(1, 2); }
        self.geometry.triangles.push(triangle);
        Ok(())
    }
}
fn read_geometry(bytes: &[u8], object: &str, options: &ModelColliderOptions) -> Result<ModelGeometry, String> {
    options.validate()?;
    if object.len() > 120 { return Err("model object selector exceeds 120 bytes".into()); }
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("render GLB: {e}"))?;
    if gltf.buffers().any(|b| !matches!(b.source(), gltf::buffer::Source::Bin)) {
        return Err("model GLB must embed its geometry buffer (external buffer paths are not read)".into());
    }
    let blob = gltf.blob.as_deref().ok_or("model GLB has no binary geometry")?;
    if gltf.nodes().count() > 4096 { return Err("model has too many scene nodes".into()); }
    let mut collector = Collector { blob, object, options, geometry: ModelGeometry { vertices: vec![], triangles: vec![] },
        welded: HashMap::new(), active: vec![false; gltf.nodes().count()], visits: 0, raw_vertices: 0 };
    let scene = gltf.scenes().next().ok_or("model GLB has no Scene0")?;
    for node in scene.nodes() { collector.node(node, identity(), false, 0)?; }
    if collector.geometry.vertices.len() < 4 || collector.geometry.triangles.is_empty() {
        return Err(format!("no solid triangle geometry selected by '{object}' in Scene0"));
    }
    Ok(collector.geometry)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let points = [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let mut binary = Vec::new();
        for p in points { for v in p { binary.extend_from_slice(&v.to_le_bytes()); } }
        let triangles: [u16; 12] = [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        for i in triangles { binary.extend_from_slice(&i.to_le_bytes()); }
        let document = serde_json::json!({"asset":{"version":"2.0"}, "scene":0, "scenes":[{"nodes":[0]}],
            "nodes":[{"translation":[10,0,0],"children":[1]}, {"name":"part", "translation":[2,0,0], "mesh":0}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],
            "buffers":[{"byteLength":binary.len()}],
            "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":48}, {"buffer":0,"byteOffset":48,"byteLength":24}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[0,0,0],"max":[1,1,1]},
                {"bufferView":1,"componentType":5123,"count":12,"type":"SCALAR"}]});
        let mut json = serde_json::to_vec(&document).unwrap();
        while json.len() % 4 != 0 { json.push(b' '); }
        let mut bytes = Vec::new();
        for word in [0x46546c67u32, 2, (12 + 8 + json.len() + 8 + binary.len()) as u32, json.len() as u32, 0x4e4f534a] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend(json);
        bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0x004e4942u32.to_le_bytes());
        bytes.extend(binary);
        bytes
    }
    #[test]
    fn keeps_node_and_ancestor_transforms_in_render_scene_frame() {
        let geometry = read_geometry(&fixture(), "part", &Default::default()).unwrap();
        assert_eq!(geometry.vertices.len(), 4);
        assert_eq!(geometry.triangles.len(), 4);
        assert_eq!(geometry.vertices[0], [12., 0., 0.]);
        assert!(read_geometry(&fixture(), "missing", &Default::default()).is_err());
    }
    #[test]
    fn applies_graphic_scale_and_reflected_winding() {
        let options = ModelColliderOptions { scale: [-2., 1., 1.], ..Default::default() };
        let geometry = read_geometry(&fixture(), "part", &options).unwrap();
        assert_eq!(geometry.vertices[0], [-24., 0., 0.]);
        assert_eq!(geometry.triangles[0], [0, 1, 2]);
    }
    #[test]
    fn malformed_glb_is_an_error_not_a_box_fallback() {
        assert!(read_geometry(b"not a glb", "", &Default::default()).is_err());
    }
}
