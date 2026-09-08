//! Select local installed data before any stock-data or Bevy initialization.
use std::{path::{Path, PathBuf}, process::Command};

fn base() -> Result<PathBuf, String> {
    Ok(PathBuf::from(std::env::var_os("LOCALAPPDATA")
        .ok_or("LOCALAPPDATA is unavailable; use --assets DIRECTORY")?).join("Skate3RustEngine"))
}
fn installed(base: &Path) -> Result<Option<PathBuf>, String> {
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
    Ok(Some(assets))
}

pub(crate) fn asset_root() -> Result<PathBuf, String> {
    let explicit = std::env::args_os().any(|arg| arg == "--assets");
    if explicit { return Ok(PathBuf::from("assets")); }
    // Existing developer launches retain their explicit local assets.
    if Path::new("assets/private/game.json").is_file() { return Ok(PathBuf::from("assets")); }
    let base = base()?;
    if let Some(assets) = installed(&base)? { return Ok(assets); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let setup = exe.parent().ok_or("No executable directory")?.join("support/skate3setup.exe");
    if !setup.is_file() {
        return Err("No installed assets. Use the complete Windows release package, or --assets DIRECTORY for a prepared development asset set.".into());
    }
    let mut command = Command::new(setup);
    command.arg("--base").arg(&base).arg("--game-exe").arg(&exe);
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command.status().map_err(|e| format!("Could not start setup: {e}"))?;
    if !status.success() { return Err("Setup was cancelled or did not complete".into()); }
    installed(&base)?.ok_or_else(|| "Setup did not publish a complete installation".into())
}
