//! The launcher's folders and state files, compatible with the PowerShell launcher's (doc 44a section 4): the state
//! folder, config.json, versions.json (with the built-in versions), version.txt.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub struct Launcher {
    /// The folder of the launcher's own files (tools/steam-launcher, or the standalone download's folder).
    pub here: PathBuf,
    /// The checkout the versions come from (the standalone folder when standalone).
    pub repo: PathBuf,
    pub state_dir: PathBuf,
    pub standalone: bool,
    pub config: Value,
    pub versions: Vec<Value>,
}

/// The launcher folder: `SKATE_LAUNCHER_DIR`, else the program's own folder when it holds the launcher's files, else
/// `tools/steam-launcher` of the checkout around the current folder.
pub fn find_here() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SKATE_LAUNCHER_DIR") {
        return Some(PathBuf::from(dir));
    }
    let marks = |d: &Path| d.join("STANDALONE.txt").is_file() || d.join("launcher.ps1").is_file() || d.join("config.example.json").is_file();
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        if marks(&exe_dir) {
            return Some(exe_dir);
        }
    }
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("tools").join("steam-launcher");
        if marks(&candidate) {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// A JSON array (or one object, as PowerShell 5.1 writes a one-element array) as a list.
fn as_list(v: Value) -> Vec<Value> {
    match v {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

/// `Setting`: a set, non-empty config value.
pub fn setting<'a>(config: &'a Value, name: &str) -> Option<&'a Value> {
    config.get(name).filter(|v| !v.is_null() && v.as_str() != Some(""))
}

pub fn text(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

pub fn flag(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => false,
    }
}

impl Launcher {
    pub fn load(here: PathBuf) -> Launcher {
        let standalone = here.join("STANDALONE.txt").is_file();
        let repo = if standalone {
            here.clone()
        } else {
            here.parent().and_then(Path::parent).map_or_else(|| here.clone(), Path::to_path_buf)
        };
        let state_dir = if here.starts_with(repo.join("tools")) && !standalone {
            repo.join(".local").join("steam-launcher")
        } else {
            here.clone()
        };
        let _ = std::fs::create_dir_all(&state_dir);
        let config = read_json(&state_dir.join("config.json")).unwrap_or(Value::Null);
        let versions = versions(&config, &state_dir, standalone);
        Launcher { here, repo, state_dir, standalone, config, versions }
    }

    /// `Get-Version ''`: the selected version (version.txt), else the first.
    pub fn selected(&self) -> &Value {
        let id = std::fs::read_to_string(self.state_dir.join("version.txt")).map(|s| s.trim().trim_start_matches('\u{feff}').to_owned()).unwrap_or_default();
        self.versions.iter().find(|v| !id.is_empty() && text(v, "id") == id).unwrap_or(&self.versions[0])
    }

    /// `Setting-Path`: a configured path, or none while unset or still the example's placeholder.
    pub fn setting_path(&self, name: &str) -> Option<String> {
        let v = self.config.get(name)?.as_str()?;
        (!v.is_empty() && !v.to_ascii_lowercase().starts_with(r"c:\path\to")).then(|| v.to_owned())
    }

    /// `Version-Dir`.
    pub fn version_dir(&self, v: &Value) -> PathBuf {
        let dir = text(v, "dir");
        if dir == "." {
            return self.repo.clone();
        }
        if let (Some(root), true, Some(rest)) = (self.setting_path("versions_root"), self.standalone, dir.strip_prefix("versions/")) {
            return PathBuf::from(root).join(rest);
        }
        self.repo.join(dir)
    }

    pub fn use_release(&self, v: &Value) -> bool {
        flag(v, "release") && setting(&self.config, "use_releases").map_or(self.standalone, |s| s.as_bool().unwrap_or(true))
    }
}

/// The built-in versions plus versions.json (an entry with a built-in's id replaces it), as `launcher.ps1` builds them.
fn versions(config: &Value, state_dir: &Path, standalone: bool) -> Vec<Value> {
    let fork = setting(config, "fork_url").and_then(Value::as_str).unwrap_or("https://github.com/Hailey-Ross/rusty-trucks.git");
    let upstream = setting(config, "upstream_url").and_then(Value::as_str).unwrap_or("https://github.com/SK8-ENGINE/skate-3-rust-engine.git");
    let prefix = if standalone { "versions" } else { ".local/versions" };
    let builtin = vec![
        json!({"id": "hails-additions", "name": "hails-additions (newest on the fork)", "branch": "hails-additions", "url": fork,
               "dir": format!("{prefix}/hails-additions"), "detached": true, "release": "hails-additions",
               "notes": "Always the newest pushed hails-additions: every launch fetches it and rebuilds when it moved. What changed: docs\\hails-additions\\RELEASE-NOTES.md."}),
        json!({"id": "upstream-main", "name": "Upstream main (for comparing)", "branch": "main", "url": upstream,
               "dir": format!("{prefix}/upstream-main"), "detached": true,
               "notes": "Upstream main as it is (SK8-ENGINE), rebuilt when it moved. Use it to compare against hails-additions."}),
    ];
    let keep: Vec<String> = setting(config, "builtin_versions").map_or_else(
        || vec!["hails-additions".into(), "upstream-main".into()],
        |v| as_list(v.clone()).iter().filter_map(|x| x.as_str().map(str::to_owned)).collect());
    let file = state_dir.join("versions.json");
    let own: Vec<Value> = if file.is_file() {
        read_json(&file).map(as_list).unwrap_or_default()
    } else if !standalone {
        vec![json!({"id": "this", "name": "This checkout", "branch": "", "dir": ".", "dev": true,
                    "notes": "This checkout as it is. Add a versions.json to compare more branches (see README.md)."})]
    } else {
        Vec::new()
    };
    let own_ids: Vec<String> = own.iter().map(|v| text(v, "id")).collect();
    builtin.into_iter()
        .filter(|b| keep.contains(&text(b, "id")) && !own_ids.contains(&text(b, "id")))
        .chain(own)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_json_replaces_a_builtin_and_follows_it() {
        let dir = std::env::temp_dir().join(format!("skate-launcher-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("versions.json"), r#"{"id": "upstream-main", "name": "Mine", "dir": "x"}"#).unwrap();
        let list = versions(&Value::Null, &dir, false);
        let ids: Vec<String> = list.iter().map(|v| text(v, "id")).collect();
        assert_eq!(ids, ["hails-additions", "upstream-main"]);
        assert_eq!(text(&list[1], "name"), "Mine", "a one-object file (PowerShell's one-element array) counts");
        let only = versions(&json!({"builtin_versions": "hails-additions"}), &dir, false);
        assert_eq!(only.len(), 2, "the own entry stays even when its built-in is not kept");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_checkout_without_versions_json_lists_this_checkout_last() {
        let dir = std::env::temp_dir().join(format!("skate-launcher-this-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ids: Vec<String> = versions(&Value::Null, &dir, false).iter().map(|v| text(v, "id")).collect();
        assert_eq!(ids, ["hails-additions", "upstream-main", "this"]);
        assert_eq!(versions(&Value::Null, &dir, true).len(), 2, "standalone has no checkout entry");
        std::fs::remove_dir_all(&dir).ok();
    }
}
