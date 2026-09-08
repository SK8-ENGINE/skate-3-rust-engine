//! Authored FE travel destinations; private content is produced by the extractor.
use std::path::Path;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(crate) struct Destination {
    pub id: String,
    pub name: String,
    pub map: String,
    pub matrix: Option<[[f32; 4]; 4]>,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
}
#[derive(Deserialize)]
struct Catalog { version: u32, destinations: Vec<Destination> }

pub(crate) fn load(assets: &Path) -> Result<Vec<Destination>, String> {
    let path = assets.join("private/teleports.json");
    let bytes = std::fs::read(&path).map_err(|e| format!("Teleport locations: {e}"))?;
    let catalog: Catalog = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if catalog.version != 1 { return Err("Unsupported teleport catalog version".into()); }
    let mut ids = std::collections::HashSet::new();
    for d in &catalog.destinations {
        if d.name.is_empty() || d.id.is_empty() || !ids.insert(&d.id)
            || d.map.is_empty() || d.map.contains(['/', '\\', ':']) || matches!(d.map.as_str(), "." | "..") {
            return Err("Invalid teleport destination identity".into());
        }
        if let Some(m) = d.matrix {
            if !valid_matrix(m) { return Err(format!("Invalid teleport transform: {}", d.name)); }
        }
    }
    Ok(catalog.destinations)
}

fn valid_matrix(m: [[f32; 4]; 4]) -> bool {
    m.iter().flatten().all(|v| v.is_finite())
        && (0..3).all(|i| m[i][3].abs() < 1e-5)
        && (m[3][3] - 1.).abs() < 1e-5
        && m[2][0] * m[2][0] + m[2][2] * m[2][2] > 1e-6
}

pub(crate) fn same_map(path: &Path, name: &str) -> bool {
    path.file_stem().is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_destinations_and_preserves_authored_heading() {
        let mut m = skate_core::physics::skeleton_animation_record::IDENTITY;
        m[2] = [1., 0., 0., 0.];
        m[3] = [350.5, 140.2, -725.3, 1.];
        assert!(valid_matrix(m));
        m[3][1] = f32::NAN;
        assert!(!valid_matrix(m));
        assert!(same_map(Path::new("maps/DownTown.skate"), "Downtown"));
        assert!(!same_map(Path::new("maps/MegaPark.skate"), "University"));
    }
}
