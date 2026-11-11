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

/// `Get-Version <id>`: that version, else the first.
fn version<'a>(l: &'a Launcher, id: &str) -> &'a Value {
    if id.is_empty() {
        return l.selected();
    }
    l.versions.iter().find(|v| text(v, "id") == id).unwrap_or(&l.versions[0])
}

/// `notes [id]`: the version's name, branch, status and what to test.
pub fn notes(l: &Launcher, id: &str) -> Vec<String> {
    let v = version(l, id);
    vec![
        format!("Version: {}", text(v, "name")),
        format!("Branch: {}   Status: {}", text(v, "branch"), version_status(l, v)),
        String::new(),
        "What to test:".into(),
        text(v, "notes"),
    ]
}

/// `version-set <id>` / `version-next` / `version-prev`: selects a version (version.txt, ASCII with CRLF as
/// Set-Content writes it), then prints its notes.
pub fn version_step(l: &Launcher, step: i64, id: &str) -> Vec<String> {
    let new = if step == 0 {
        text(version(l, id), "id")
    } else {
        let ids: Vec<String> = l.versions.iter().map(|v| text(v, "id")).collect();
        let cur = ids.iter().position(|x| *x == text(l.selected(), "id")).unwrap_or(0) as i64;
        let n = ids.len() as i64;
        ids[(cur + step + n).rem_euclid(n) as usize].clone()
    };
    let _ = std::fs::write(l.state_dir.join("version.txt"), format!("{new}\r\n"));
    notes(l, &new)
}

/// `config-set <file>`: the setup wizard's changed values (a JSON object of dotted keys) merged into config.json,
/// keeping its key order; the file is removed either way.
pub fn config_set(l: &Launcher, file: &str) -> Result<Vec<String>, String> {
    use crate::ojson::OValue;
    let read = |p: &Path| -> Result<OValue, String> {
        let t = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(t.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", p.display()))
    };
    let pairs = read(Path::new(file));
    let _ = std::fs::remove_file(file);
    let OValue::Object(pairs) = pairs? else { return Err("config-set: not a JSON object".into()) };
    let path = l.state_dir.join("config.json");
    let mut config = if path.exists() { read(&path)? } else { OValue::Object(Vec::new()) };
    for (key, value) in pairs {
        config.set_path(&key, value);
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(vec!["saved".into()])
}

/// `config-get`: the first-time setup's values, key and value per line.
pub fn config_get(l: &Launcher) -> Vec<String> {
    let sub = |section: &str, key: &str| -> String {
        l.config.get(section).and_then(|o| o.get(key)).and_then(Value::as_str)
            .filter(|v| !v.is_empty() && !v.to_ascii_lowercase().starts_with(r"c:\path\to")).unwrap_or_default().to_owned()
    };
    let path = |k: &str| l.setting_path(k).unwrap_or_default();
    let cap = crate::state::setting(&l.config, "build_cap_gb").map_or_else(|| "0".into(), |v| match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    });
    vec![
        format!("standalone\t{}", if l.standalone { "1" } else { "" }),
        format!("default_root\t{}", if l.standalone { l.here.display().to_string() } else { String::new() }),
        format!("storage_root\t{}", path("storage_root")),
        format!("target_root\t{}", path("target_root")),
        format!("build_cap_gb\t{cap}"),
        format!("disc\t{}", path("disc")),
        format!("recomp.exe\t{}", sub("recomp", "exe")),
        format!("recomp.working_dir\t{}", sub("recomp", "working_dir")),
        format!("rpcs3.exe\t{}", sub("rpcs3", "exe")),
        format!("rpcs3.game\t{}", sub("rpcs3", "game")),
    ]
}
