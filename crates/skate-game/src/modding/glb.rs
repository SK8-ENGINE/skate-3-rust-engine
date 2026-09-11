//! Sync package-GLB helpers for named nodes/meshes → convex hull points.
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObjectLists {
    pub nodes: Vec<String>,
    pub meshes: Vec<String>,
}

pub fn list_objects(path: &Path) -> Result<ObjectLists, String> {
    let (doc, _) = open(path)?;
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

/// Collect unique vertex positions for a named node (with descendants) or mesh, in that
/// node's local space (identity if mesh-only). Suitable for Rapier convex hulls.
pub fn convex_points(path: &Path, object: &str) -> Result<Vec<[f32; 3]>, String> {
    skate_mods::convex_points_file(path, object)
}

fn open(path: &Path) -> Result<(gltf::Document, Vec<gltf::buffer::Data>), String> {
    if !path.is_file() {
        return Err(format!("GLB not found: {}", path.display()));
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    let (doc, buffers, _) =
        gltf::import(path).map_err(|e| format!("GLB load {}: {e}", path.display()))?;
    Ok((doc, buffers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_errors() {
        assert!(list_objects(Path::new("nope.glb")).is_err());
    }

    #[test]
    fn named_node_hull_is_body_local_not_scene_translated() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sdk/examples/skyline/skyline.glb");
        if !path.is_file() {
            return;
        }
        for name in ["wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr"] {
            let points = convex_points(&path, name).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            for point in &points {
                for axis in 0..3 {
                    min[axis] = min[axis].min(point[axis]);
                    max[axis] = max[axis].max(point[axis]);
                }
            }
            let center = std::array::from_fn::<_, 3, _>(|axis| (min[axis] + max[axis]) * 0.5);
            assert!(
                center.iter().all(|v| v.abs() < 0.03),
                "{name} hull retained scene translation: center={center:?}"
            );
            let extent = std::array::from_fn::<_, 3, _>(|axis| max[axis] - min[axis]);
            assert!(
                extent[1] > 0.5 && extent[2] > 0.5,
                "{name} must contain the authored tire geometry: extent={extent:?}"
            );
        }
    }
}
