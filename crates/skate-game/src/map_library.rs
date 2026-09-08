//! Installed maps are selected by path; switching restarts the whole simulation.
//! This prevents old collision bodies, graph state and GPU assets surviving a change.
use std::{path::{Path, PathBuf}, process::Command};

pub(crate) struct Entry {
    pub label: String,
    pub path: Option<PathBuf>,
}

pub(crate) fn discover(assets: &Path) -> Vec<Entry> {
    let mut maps = Vec::new();
    let root = assets.parent().unwrap_or(assets).join("maps");
    for directory in [&root, &root.join("private")] {
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("skate")) {
                    maps.push(Entry {
                        label: path.file_stem().unwrap().to_string_lossy().replace('_', " "),
                        path: Some(path),
                    });
                }
            }
        }
    }
    maps.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    maps.insert(0, Entry { label: "Test world".into(), path: None });
    maps
}

pub(crate) fn default_map(assets: &Path) -> Result<Option<PathBuf>, String> {
    // Only a completed release installation creates this pointer. Existing
    // development checkouts continue to boot the test world.
    let pointer = assets.parent().unwrap_or(assets).join("settings/default-map.json");
    let bytes = match std::fs::read(&pointer) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", pointer.display())),
    };
    let relative: String = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let relative = Path::new(&relative);
    if relative.is_absolute() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("Invalid installed default map path".into());
    }
    Ok(Some(assets.parent().unwrap_or(assets).join(relative)))
}

pub(crate) fn switch(assets: &Path, entry: &Entry) -> Result<(), String> {
    if let Some(path) = &entry.path {
        let map = skate_data::skate_map::SkateMap::load(path)?;
        crate::skate_world::validate_runtime(&map)?;
    }
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command.arg("--assets").arg(assets);
    if let Some(path) = &entry.path {
        command.arg("--map").arg(path);
    } else {
        command.arg("--test-world");
    }
    command.spawn().map_err(|e| format!("Could not open map: {e}"))?;
    Ok(())
}
