//! Version build folders: where each version builds and how big its build is (`sizes`, doc 44a 1a).
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::{setting, text, Launcher};

/// `Target-Dir`: the version's `target`, else `<target_root>/<id>`, else the version's own `target`.
pub fn target_dir(l: &Launcher, v: &Value) -> PathBuf {
    let own = text(v, "target");
    if !own.is_empty() {
        let p = PathBuf::from(&own);
        return if p.is_absolute() { p } else { l.repo.join(own) };
    }
    if let Some(root) = setting(&l.config, "target_root").and_then(Value::as_str) {
        return PathBuf::from(root).join(text(v, "id"));
    }
    l.version_dir(v).join("target")
}

/// `Folder-Bytes`: every file under the folder (0 when it is missing).
pub fn folder_bytes(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries.flatten().map(|e| {
        let p = e.path();
        match std::fs::metadata(&p) {
            Ok(m) if m.is_dir() => folder_bytes(&p),
            Ok(m) => m.len(),
            Err(_) => 0,
        }
    }).sum()
}

/// `sizes`: id, name, build cache bytes, bin bytes.
pub fn sizes(l: &Launcher) -> Vec<String> {
    l.versions.iter().map(|v| {
        format!("{}\t{}\t{}\t{}", text(v, "id"), text(v, "name"), folder_bytes(&target_dir(l, v)), folder_bytes(&l.version_dir(v).join("bin")))
    }).collect()
}
