//! Each portable copy owns its installation; explicit --assets is for development.
use std::{path::{Path, PathBuf}, process::Command};

fn installed(base: &Path) -> Result<Option<(PathBuf, serde_json::Value)>, String> {
    let bytes = match std::fs::read(base.join("installation.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let marker: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if marker["version"].as_u64() != Some(1) { return Err("Unsupported installation version".into()); }
    let relative = Path::new(marker["directory"].as_str().ok_or("Invalid installation path")?);
    if relative.is_absolute() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("Invalid installation path".into());
    }
    let assets = base.join(relative).join("assets");
    if !assets.join("private/game.json").is_file() { return Ok(None); }
    Ok(Some((assets, marker)))
}

pub(crate) fn asset_root() -> Result<PathBuf, String> {
    if std::env::args_os().any(|arg| arg == "--assets") { return Ok(PathBuf::from("assets")); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let root = exe.parent().ok_or("No executable directory")?;
    let base = root.join("data");
    let expected = match std::fs::read(root.join("release.json")) {
        Ok(bytes) => {
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?.trim_start_matches('\u{feff}');
            let release: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            release.get("asset_pipelines").cloned()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let existing = installed(&base)?;
    if let Some((assets, marker)) = &existing {
        if expected.as_ref().is_none_or(|versions| marker.get("pipelines") == Some(versions)) {
            return Ok(assets.clone());
        }
    }
    let setup = root.join("support/skate3setup.exe");
    if !setup.is_file() {
        return Err("This copy has not been set up. Use the complete Windows package, or --assets DIRECTORY for development.".into());
    }
    let mut command = Command::new(setup);
    command.arg("--base").arg(&base).arg("--game-exe").arg(&exe);
    if existing.is_some() { command.arg("--refresh"); }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command.status().map_err(|e| format!("Could not start setup: {e}"))?;
    if !status.success() { return Err("Setup was cancelled or did not complete".into()); }
    let (assets, marker) = installed(&base)?.ok_or("Setup did not publish a complete installation")?;
    if expected.as_ref().is_some_and(|versions| marker.get("pipelines") != Some(versions)) {
        return Err("Setup helper does not match this release's asset extractors. Unpack the complete package.".into());
    }
    Ok(assets)
}
