//! Global authored foliage that lives outside the district stream.
use bevy::prelude::*;
use skate_data::skate_map::SkateMap;
use std::path::Path;

pub(crate) fn spawn_backdrop(
    name: &str,
    asset_root: &Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    retail_materials: &mut Assets<super::RetailWorldMaterial>,
    images: &mut Assets<Image>,
) {
    let path = asset_root.join("private/native-backdrops").join(format!("{name}.skate"));
    if !path.is_file() {
        return;
    }
    let map = match std::fs::read(&path).map_err(|e| e.to_string())
        .and_then(|data| SkateMap::parse_render_only(&data)) {
        Ok(map) => map,
        Err(error) => {
            error!("SKATE_BACKDROP: {}: {error}", path.display());
            return;
        }
    };
    // This package contributes presentation only; never route it into physics.
    if map.name != name || !map.geometry.collision.is_empty()
        || !map.lights.is_empty() || !map.doors.is_empty() || !map.rails.is_empty()
    {
        error!("SKATE_BACKDROP: invalid render-only package {}", path.display());
        return;
    }
    info!("SKATE_BACKDROP: {name} authored foliage triangles={}", map.geometry.indices.len() / 3);
    crate::skate_world::spawn(&map, commands, meshes, materials, retail_materials, images);
}
