//! Package-relative GLB introspection for `sdk.assets.objects`.
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ObjectLists {
    pub nodes: Vec<String>,
    pub meshes: Vec<String>,
}

const MAX_VERTS: usize = 65_536;
const MAX_HULL: usize = 256;

pub fn list_objects(root: &Path, relative: &str) -> Result<ObjectLists, String> {
    if relative.is_empty()
        || relative.len() > 256
        || relative.contains("..")
        || relative.contains('\\')
        || relative.contains(':')
        || !relative.to_ascii_lowercase().ends_with(".glb")
    {
        return Err("invalid GLB path".into());
    }
    let rel = Path::new(relative);
    if rel
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Use a relative path without traversal".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(rel).canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Asset escapes mod root".into());
    }
    if !path.is_file() {
        return Err(format!("GLB not found: {relative}"));
    }
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    let (doc, _, _) = gltf::import(&path).map_err(|e| format!("GLB load: {e}"))?;
    let mut nodes: Vec<_> = doc
        .nodes()
        .filter_map(|n| n.name().map(str::to_owned))
        .collect();
    let mut meshes: Vec<_> = doc
        .meshes()
        .filter_map(|m| m.name().map(str::to_owned))
        .collect();
    nodes.sort();
    nodes.dedup();
    meshes.sort();
    meshes.dedup();
    Ok(ObjectLists { nodes, meshes })
}

/// Resolve a named GLB node/mesh into body-local convex-hull points.
///
/// For a node, its scene translation is intentionally removed: the rigid body's
/// `position` supplies world placement. Authored node rotation/scale and all
/// descendant transforms remain part of the shape.
pub fn convex_points_file(path: &Path, object: &str) -> Result<Vec<[f32; 3]>, String> {
    if object.is_empty() || object.len() > 120 || object.contains("..") {
        return Err("invalid GLB object name".into());
    }
    if !path.is_file() {
        return Err(format!("GLB not found: {}", path.display()));
    }
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    let (doc, buffers, _) =
        gltf::import(path).map_err(|e| format!("GLB load {}: {e}", path.display()))?;
    let mut points = Vec::new();
    if let Some(node) = doc.nodes().find(|node| node.name() == Some(object)) {
        let mut root = node.transform().matrix();
        root[3][0] = 0.;
        root[3][1] = 0.;
        root[3][2] = 0.;
        collect_node_contents(&node, &buffers, root, &mut points);
    } else if let Some(mesh) = doc.meshes().find(|mesh| mesh.name() == Some(object)) {
        collect_mesh(&mesh, &buffers, identity(), &mut points);
    } else {
        return Err(format!("GLB object '{object}' not found"));
    }
    points.sort_by(|a, b| {
        a[0]
            .total_cmp(&b[0])
            .then(a[1].total_cmp(&b[1]))
            .then(a[2].total_cmp(&b[2]))
    });
    points.dedup_by(|a, b| {
        (a[0] - b[0]).abs() < 1e-4
            && (a[1] - b[1]).abs() < 1e-4
            && (a[2] - b[2]).abs() < 1e-4
    });
    if points.len() > MAX_HULL {
        // Preserve all six axis extrema so decimation cannot shave one side of
        // a wheel/chassis hull. Fill the remaining slots uniformly.
        let mut selected = Vec::with_capacity(MAX_HULL);
        for axis in 0..3 {
            let min = points
                .iter()
                .min_by(|a, b| a[axis].total_cmp(&b[axis]))
                .copied()
                .unwrap();
            let max = points
                .iter()
                .max_by(|a, b| a[axis].total_cmp(&b[axis]))
                .copied()
                .unwrap();
            selected.push(min);
            selected.push(max);
        }
        let slots = MAX_HULL - selected.len();
        for index in 0..slots {
            let source = index * (points.len() - 1) / slots.max(1);
            selected.push(points[source]);
        }
        selected.sort_by(|a, b| {
            a[0]
                .total_cmp(&b[0])
                .then(a[1].total_cmp(&b[1]))
                .then(a[2].total_cmp(&b[2]))
        });
        selected.dedup_by(|a, b| {
            (a[0] - b[0]).abs() < 1e-4
                && (a[1] - b[1]).abs() < 1e-4
                && (a[2] - b[2]).abs() < 1e-4
        });
        points = selected;
    }
    if points.len() < 4 {
        return Err(format!(
            "GLB object '{object}' needs ≥4 unique verts for a convex hull (got {})",
            points.len()
        ));
    }
    Ok(points)
}

fn identity() -> [[f32; 4]; 4] {
    [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ]
}

fn matrix_mul(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

fn transform_point(matrix: [[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| {
        matrix[0][row] * point[0]
            + matrix[1][row] * point[1]
            + matrix[2][row] * point[2]
            + matrix[3][row]
    })
}

fn collect_node_contents(
    node: &gltf::Node<'_>,
    buffers: &[gltf::buffer::Data],
    transform: [[f32; 4]; 4],
    out: &mut Vec<[f32; 3]>,
) {
    if let Some(mesh) = node.mesh() {
        collect_mesh(&mesh, buffers, transform, out);
    }
    for child in node.children() {
        collect_node(&child, buffers, transform, out);
    }
}

fn collect_node(
    node: &gltf::Node<'_>,
    buffers: &[gltf::buffer::Data],
    parent: [[f32; 4]; 4],
    out: &mut Vec<[f32; 3]>,
) {
    let transform = matrix_mul(parent, node.transform().matrix());
    collect_node_contents(node, buffers, transform, out);
}

fn collect_mesh(
    mesh: &gltf::Mesh<'_>,
    buffers: &[gltf::buffer::Data],
    transform: [[f32; 4]; 4],
    out: &mut Vec<[f32; 3]>,
) {
    for primitive in mesh.primitives() {
        let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|d| d.0.as_slice()));
        let Some(positions) = reader.read_positions() else {
            continue;
        };
        for (index, point) in positions.enumerate() {
            if out.len() >= MAX_VERTS {
                break;
            }
            if index > 0 && out.len() > MAX_HULL * 4 && index % 4 != 0 {
                continue;
            }
            let point = transform_point(transform, point);
            if point.iter().all(|value| value.is_finite()) {
                out.push(point);
            }
        }
    }
}
