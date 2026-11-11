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

/// `clear-cache` / `clear-build <id|all>`: deletes the versions' build caches (and with `build` their bin/ too).
/// Ok(result line) or Err(message), both already saved / printed as launcher.ps1 does.
pub fn clear(l: &Launcher, whole: bool, which: &str) -> Result<String, String> {
    let entry = if whole { "clear-build" } else { "clear-cache" };
    if !crate::play::running_games().is_empty() {
        crate::play::save_result(l, "Not cleared: a game is running.");
        return Err("Not cleared: a game is running.".into());
    }
    let versions: Vec<&Value> = l.versions.iter().filter(|v| which == "all" || text(v, "id") == which).collect();
    if versions.is_empty() {
        return Err(format!("Unknown version: {which}"));
    }
    let (mut freed, mut done) = (0u64, Vec::new());
    for v in versions {
        let mut dirs = vec![target_dir(l, v)];
        if whole {
            dirs.push(l.version_dir(v).join("bin"));
        }
        for d in dirs.into_iter().filter(|d| d.exists()) {
            freed += folder_bytes(&d);
            println!("  Deleting {}...", d.display());
            let _ = std::fs::remove_dir_all(&d);
        }
        done.push(text(v, "name"));
    }
    let r = format!("Cleared the {} of {} ({:.1} GB freed).", if whole { "builds" } else { "build caches" }, done.join(", "), freed as f64 / 1073741824.0);
    crate::play::save_result(l, &format!("{}  {entry} {which}\r\n{r}", jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S")));
    Ok(format!("  {r}"))
}

/// `sizes`: id, name, build cache bytes, bin bytes.
pub fn sizes(l: &Launcher) -> Vec<String> {
    l.versions.iter().map(|v| {
        format!("{}\t{}\t{}\t{}", text(v, "id"), text(v, "name"), folder_bytes(&target_dir(l, v)), folder_bytes(&l.version_dir(v).join("bin")))
    }).collect()
}
