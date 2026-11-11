//! The launcher's text commands, line for line as `launcher.ps1` prints them (doc 44a section 1a), so the old window,
//! scripts and the new one read the same output.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::state::{flag, text, Launcher};

/// The game's file name on this OS.
pub const GAME_EXE: &str = if cfg!(windows) { "skate3rust.exe" } else { "skate3rust" };

fn git(dir: &Path, args: &[&str]) -> String {
    Command::new("git").arg("-C").arg(dir).args(args).output().ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// A file's modification time as local `MM-dd HH:mm`.
fn local_stamp(path: &Path) -> String {
    let Some(modified) = std::fs::metadata(path).and_then(|m| m.modified()).ok() else { return String::new() };
    let Ok(ts) = jiff::Timestamp::try_from(modified) else { return String::new() };
    ts.to_zoned(jiff::tz::TimeZone::system()).strftime("%m-%d %H:%M").to_string()
}

/// `Version-Status`.
pub fn version_status(l: &Launcher, v: &Value) -> String {
    let dir = l.version_dir(v);
    if l.use_release(v) {
        let release = std::fs::read_to_string(dir.join("release.json")).ok()
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok());
        return match release {
            Some(r) if dir.join(GAME_EXE).is_file() => {
                let revision = text(&r, "revision");
                format!("release build {} ({})", text(&r, "build"), revision.get(..7).unwrap_or(&revision))
            }
            _ => "not downloaded yet (first launch downloads it)".into(),
        };
    }
    if !dir.join(".git").exists() {
        return "not set up yet (first launch builds it)".into();
    }
    let exe = dir.join("bin").join(GAME_EXE);
    if !exe.is_file() {
        return "not built yet (first launch builds it)".into();
    }
    if flag(v, "dev") {
        return format!("built from the working tree ({})", local_stamp(&exe));
    }
    let head = git(&dir, &["rev-parse", "--short", "HEAD"]);
    let built = std::fs::read_to_string(dir.join("bin").join("built_commit.txt")).map(|s| s.trim().to_owned()).unwrap_or_default();
    if built == head {
        return format!("built at {head}");
    }
    format!("out of date (built {built}, now {head}): next launch rebuilds")
}

/// `versions`: id, name, status, `*` on the selected one, tab-separated.
pub fn versions(l: &Launcher) -> Vec<String> {
    let selected = text(l.selected(), "id");
    l.versions.iter().map(|v| {
        let id = text(v, "id");
        format!("{}\t{}\t{}\t{}", id, text(v, "name"), version_status(l, v), if id == selected { "*" } else { "" })
    }).collect()
}

/// `version-set <id>`: selects a version (version.txt).
pub fn version_set(l: &Launcher, id: &str) -> Result<(), String> {
    if !l.versions.iter().any(|v| text(v, "id") == id) {
        return Err(format!("Unknown version: {id}"));
    }
    std::fs::write(l.state_dir.join("version.txt"), id).map_err(|e| e.to_string())
}
