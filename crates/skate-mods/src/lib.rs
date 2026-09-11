//! Lua mod packages (API 2): low-level physics/graphics hooks, no Vehicle class.
mod archive;
pub mod audio;
pub mod presentation;
pub mod scene;
mod assets;
pub mod model;
mod query;
mod schema;
mod vm;

pub use archive::{read_bounded, validate_package, Cache};
pub use assets::convex_points_file;
pub use model::model_shape_file;
pub use query::{with_host, DynamicsHost, RaycastFilter, RaycastOptions};
pub use schema::{Manifest, Setting, SettingValue};
pub use vm::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preferences {
    enabled: bool,
    #[serde(default)]
    values: BTreeMap<String, Value>,
}

pub struct Package {
    pub manifest: Manifest,
    pub root: PathBuf,
    pub error: Option<String>,
    pub settings: BTreeMap<String, Value>,
    pub enabled: bool,
    vm: Option<vm::Vm>,
    fingerprint: u64,
    pending: Option<(u64, Instant)>,
}

impl Package {
    pub fn content_fingerprint(&self) -> u64 {
        self.fingerprint
    }
    pub fn running(&self) -> bool {
        self.vm.is_some()
    }
}

pub struct Manager {
    pub packages: BTreeMap<String, Package>,
    pub diagnostics: Vec<String>,
    root: PathBuf,
    preferences: PathBuf,
    pub commands: Vec<(String, Command)>,
    pub retired: Vec<String>,
    last_scan: Instant,
    pub snapshot: Value,
    invalid_since: BTreeMap<String, Instant>,
    archives: archive::Cache,
}

impl Manager {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn new(root: PathBuf, preferences: PathBuf) -> Self {
        Self {
            packages: BTreeMap::new(),
            diagnostics: vec![],
            root,
            preferences,
            commands: vec![],
            retired: vec![],
            last_scan: Instant::now() - Duration::from_secs(2),
            snapshot: Value::Null,
            invalid_since: BTreeMap::new(),
            archives: archive::Cache::default(),
        }
    }

    pub fn scan(&mut self, force: bool) {
        if !force && self.last_scan.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_scan = Instant::now();
        self.diagnostics.clear();
        let mut found = BTreeMap::<String, (PathBuf, Manifest, u64)>::new();
        let mut duplicates = std::collections::BTreeSet::new();
        let dirs = match std::fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) => {
                self.diagnostics
                    .push(format!("Mods directory {}: {e}", self.root.display()));
                return;
            }
        };
        let mut paths: Vec<_> = dirs
            .filter_map(Result::ok)
            .map(|d| d.path())
            .filter(|p| {
                !p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                    && (p.is_dir()
                        || p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("zip")))
            })
            .collect();
        paths.sort();
        if paths.len() > 128 {
            self.diagnostics.push(
                "Only the first 128 packages are supported; remove excess packages".into(),
            );
        }
        for source in paths.into_iter().take(128) {
            let path = if source.is_file() {
                match self.archives.materialize(&self.root, &source) {
                    Ok(path) => path,
                    Err(e) => {
                        self.diagnostics
                            .push(format!("{}: {e}", source.display()));
                        continue;
                    }
                }
            } else {
                source.clone()
            };
            let result = (|| {
                let bytes = read_bounded(&path, "mod.json", 64 * 1024)?;
                let manifest: Manifest =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                manifest.validate()?;
                let hash = archive::fingerprint(&path)?;
                Ok::<_, String>((manifest, hash))
            })();
            match result {
                Ok((manifest, hash)) => {
                    let id = manifest.id.clone();
                    if found.contains_key(&id) {
                        duplicates.insert(id.clone());
                    }
                    found.insert(id, (path, manifest, hash));
                }
                Err(e) => self.diagnostics.push(format!("{}: {e}", source.display())),
            }
        }
        for id in duplicates {
            found.remove(&id);
            self.diagnostics
                .push(format!("Duplicate mod ID {id}: all copies rejected"));
        }
        let removed: Vec<_> = self
            .packages
            .iter()
            .filter_map(|(id, p)| {
                if found.contains_key(id) {
                    self.invalid_since.remove(id);
                    return None;
                }
                if !p.root.exists() || force {
                    return Some(id.clone());
                }
                let since = self
                    .invalid_since
                    .entry(id.clone())
                    .or_insert_with(Instant::now);
                (since.elapsed() >= Duration::from_millis(750)).then(|| id.clone())
            })
            .collect();
        for id in removed {
            self.stop(&id);
            self.packages.remove(&id);
        }
        for (id, (root, manifest, hash)) in found {
            if let Some(p) = self.packages.get_mut(&id) {
                if p.fingerprint == hash {
                    p.pending = None;
                    continue;
                }
                let ready = match p.pending {
                    Some((h, since)) if h == hash => since.elapsed() >= Duration::from_millis(750),
                    _ => {
                        p.pending = Some((hash, Instant::now()));
                        false
                    }
                };
                if !force && !ready {
                    continue;
                }
                let enabled = p.enabled;
                self.stop(&id);
                let p = self.packages.get_mut(&id).unwrap();
                p.manifest = manifest;
                p.root = root;
                p.fingerprint = hash;
                p.pending = None;
                p.settings = p
                    .manifest
                    .settings
                    .iter()
                    .map(|(k, s)| {
                        (
                            k.clone(),
                            p.settings
                                .get(k)
                                .filter(|v| s.accepts(v))
                                .cloned()
                                .unwrap_or_else(|| s.default.clone()),
                        )
                    })
                    .collect();
                if enabled {
                    self.start(&id);
                }
            } else {
                let saved = self.read_preferences(&id);
                let settings = manifest
                    .settings
                    .iter()
                    .map(|(k, s)| {
                        (
                            k.clone(),
                            saved
                                .values
                                .get(k)
                                .filter(|v| s.accepts(v))
                                .cloned()
                                .unwrap_or_else(|| s.default.clone()),
                        )
                    })
                    .collect();
                let enabled = saved.enabled;
                self.packages.insert(
                    id.clone(),
                    Package {
                        root,
                        manifest,
                        fingerprint: hash,
                        pending: None,
                        error: None,
                        settings,
                        enabled,
                        vm: None,
                    },
                );
                if enabled {
                    self.start(&id);
                }
            }
        }
    }

    fn read_preferences(&mut self, id: &str) -> Preferences {
        match std::fs::read(self.preferences.join(format!("{id}.json"))) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                self.diagnostics
                    .push(format!("Settings {id}: {e}; defaults used"));
                Preferences::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Preferences {
                enabled: true,
                values: BTreeMap::new(),
            },
            Err(e) => {
                self.diagnostics.push(format!("Settings {id}: {e}"));
                Preferences {
                    enabled: true,
                    values: BTreeMap::new(),
                }
            }
        }
    }

    fn save(&self, id: &str) -> Result<(), String> {
        let p = self.packages.get(id).ok_or("Unknown mod")?;
        std::fs::create_dir_all(&self.preferences).map_err(|e| e.to_string())?;
        let file = self.preferences.join(format!("{id}.json"));
        let temp = self.preferences.join(format!("{id}.tmp"));
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(&Preferences {
                enabled: p.enabled,
                values: p.settings.clone(),
            })
            .unwrap(),
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temp, file).map_err(|e| e.to_string())
    }

    pub fn enable(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        self.packages.get_mut(id).ok_or("Unknown mod")?.enabled = enabled;
        if enabled {
            self.reload(id);
        } else {
            self.stop(id);
        }
        self.save(id)
    }

    pub fn reload(&mut self, id: &str) {
        self.stop(id);
        if self.packages.get(id).is_some_and(|p| p.enabled) {
            self.start(id);
        }
    }

    fn start(&mut self, id: &str) {
        let Some(p) = self.packages.get_mut(id) else {
            return;
        };
        p.error = None;
        match vm::Vm::new(&p.root, &p.manifest, &p.settings, &self.snapshot) {
            Ok(vm) => {
                p.vm = Some(vm);
                self.call_one(id, "on_load", Value::Null);
            }
            Err(e) => {
                eprintln!("Lua [{id}] load: {e}");
                p.error = Some(e);
                self.retired.push(id.to_owned());
            }
        }
    }

    fn stop(&mut self, id: &str) {
        if let Some(p) = self.packages.get_mut(id) {
            if let Some(mut vm) = p.vm.take() {
                if let Err(e) = vm.call("on_unload", Value::Null, &self.snapshot) {
                    p.error = Some(format!("on_unload: {e}"));
                }
            }
        }
        // Keep on_unload commands for one apply pass; apply() skips non-running mods
        // but retire_mod() clears attach/camera/bodies for this owner.
        self.retired.push(id.to_owned());
    }

    pub fn fail(&mut self, id: &str, error: String) {
        eprintln!("Lua [{id}]: {error}");
        self.stop(id);
        if let Some(p) = self.packages.get_mut(id) {
            p.error = Some(error);
        }
    }

    fn call_one(&mut self, id: &str, callback: &str, payload: Value) {
        let result = self
            .packages
            .get_mut(id)
            .and_then(|p| p.vm.as_mut())
            .map(|vm| vm.call(callback, payload, &self.snapshot));
        match result {
            Some(Ok(cmds)) => self
                .commands
                .extend(cmds.into_iter().map(|c| (id.to_owned(), c))),
            Some(Err(e)) => self.fail(id, format!("{callback}: {e}")),
            None => {}
        }
    }

    pub fn call(&mut self, id: &str, callback: &str, payload: Value) {
        self.call_one(id, callback, payload);
    }

    pub fn dispatch(&mut self, callback: &str, payload: Value) {
        let ids: Vec<_> = self
            .packages
            .iter()
            .filter(|(_, p)| p.running())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.call_one(&id, callback, payload.clone());
        }
    }

    pub fn setting(&mut self, id: &str, key: &str, value: Value) -> Result<(), String> {
        let p = self.packages.get_mut(id).ok_or("Unknown mod")?;
        if !p
            .manifest
            .settings
            .get(key)
            .ok_or("Unknown setting")?
            .accepts(&value)
        {
            return Err("Invalid setting value".into());
        }
        p.settings.insert(key.into(), value.clone());
        if let Some(vm) = p.vm.as_mut() {
            if let Err(e) = vm.settings(&p.settings) {
                let e = e.to_string();
                self.fail(id, e.clone());
                return Err(e);
            }
        }
        self.call_one(
            id,
            "on_settings",
            serde_json::json!({ "key": key, "value": value }),
        );
        self.save(id)
    }

    pub fn reset(&mut self, id: &str) -> Result<(), String> {
        let values: Vec<_> = self
            .packages
            .get(id)
            .ok_or("Unknown mod")?
            .manifest
            .settings
            .iter()
            .map(|(k, s)| (k.clone(), s.default.clone()))
            .collect();
        for (k, v) in values {
            self.setting(id, &k, v)?;
        }
        Ok(())
    }
}
