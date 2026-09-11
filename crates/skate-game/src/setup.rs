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
    let parts: Vec<_> = relative.components().collect();
    if parts.len() != 2 || parts[0].as_os_str() != "installations"
        || !parts[1].as_os_str().to_str().is_some_and(|s| s.len() == 32
            && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
        return Err("Invalid installation path".into());
    }
    let assets = base.join(relative).join("assets");
    if !assets.is_dir() { return Ok(None); }
    if !assets.canonicalize().map_err(|e| e.to_string())?
        .starts_with(base.canonicalize().map_err(|e| e.to_string())?) {
        return Err("Installation escapes its package data directory".into());
    }
    Ok(Some((assets, marker)))
}

/// Exact match, or an explicit old→new pair from release pipeline_equivalence.
fn pipeline_group_matches(
    installed: Option<&serde_json::Value>,
    expected: &serde_json::Value,
    group: &str,
    equivalence: Option<&serde_json::Value>,
) -> bool {
    if installed == Some(expected) {
        return true;
    }
    let (Some(old), Some(new)) = (installed.and_then(|v| v.as_str()), expected.as_str()) else {
        return false;
    };
    if old == new {
        return true;
    }
    equivalence
        .and_then(|table| table.get(group))
        .and_then(|pairs| pairs.as_array())
        .is_some_and(|pairs| {
            pairs.iter().any(|pair| {
                pair.as_array().is_some_and(|pair| {
                    pair.len() == 2
                        && pair[0].as_str() == Some(old)
                        && pair[1].as_str() == Some(new)
                })
            })
        })
}

fn pipelines_current(
    marker: &serde_json::Value,
    expected: &serde_json::Value,
    equivalence: Option<&serde_json::Value>,
) -> bool {
    if marker.get("pipelines") == Some(expected) {
        return true;
    }
    let installed = marker.get("pipelines").and_then(|v| v.as_object());
    expected.as_object().is_some_and(|wanted| {
        wanted.iter().all(|(group, new_hash)| {
            pipeline_group_matches(installed.and_then(|i| i.get(group)), new_hash, group, equivalence)
        })
    })
}

fn group_outputs_valid(marker: &serde_json::Value, install_root: &Path, group: &str) -> bool {
    marker
        .get("outputs")
        .and_then(|outputs| outputs.get(group))
        .is_some_and(|files| receipt_present(install_root, files))
}

/// Program updates may publish new extractor fingerprints even when a prepared
/// group is still valid. Reuse recorded outputs instead of forcing setup again.
fn pipelines_acceptable(
    marker: &serde_json::Value,
    expected: &serde_json::Value,
    equivalence: Option<&serde_json::Value>,
    install_root: &Path,
) -> bool {
    if pipelines_current(marker, expected, equivalence) {
        return true;
    }
    let installed = marker.get("pipelines").and_then(|v| v.as_object());
    expected.as_object().is_some_and(|wanted| {
        wanted.iter().all(|(group, new_hash)| {
            pipeline_group_matches(installed.and_then(|i| i.get(group)), new_hash, group, equivalence)
                || group_outputs_valid(marker, install_root, group)
        })
    })
}

fn stamp_pipelines(
    base: &Path,
    marker: &serde_json::Value,
    expected: &serde_json::Value,
) -> Result<(), String> {
    let mut updated = marker.clone();
    updated["pipelines"] = expected.clone();
    let bytes = serde_json::to_vec_pretty(&updated).map_err(|e| e.to_string())?;
    let temporary = base.join("installation.json.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, base.join("installation.json")).map_err(|e| e.to_string())
}

fn installation_ready(
    assets: &Path,
    marker: &serde_json::Value,
    expected: Option<&serde_json::Value>,
    equivalence: Option<&serde_json::Value>,
    expected_customiser: Option<&str>,
) -> bool {
    let install_root = assets.parent().unwrap();
    expected.is_none_or(|versions| {
        pipelines_acceptable(marker, versions, equivalence, install_root)
    })
        && assets.join("private/game.json").is_file()
        && marker.get("outputs").is_none_or(|groups| {
            groups.as_object().is_some_and(|groups| {
                groups
                    .values()
                    .all(|files| receipt_present(install_root, files))
            })
        })
        && customiser_current(assets, expected_customiser)
}

pub(crate) fn asset_root() -> Result<PathBuf, String> {
    if std::env::args_os().any(|arg| arg == "--assets") { return Ok(PathBuf::from("assets")); }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let root = exe.parent().ok_or("No executable directory")?;
    let base = root.join("data");
    let mut expected_customiser = None;
    let mut equivalence = None;
    let expected = match std::fs::read(root.join("release.json")) {
        Ok(bytes) => {
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?.trim_start_matches('\u{feff}');
            let release: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            expected_customiser = release["character_customiser"].as_str().map(str::to_owned);
            equivalence = release.get("pipeline_equivalence").cloned();
            release.get("asset_pipelines").cloned()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let existing = installed(&base)?;
    if let Some((assets, marker)) = &existing {
        if installation_ready(
            assets,
            marker,
            expected.as_ref(),
            equivalence.as_ref(),
            expected_customiser.as_deref(),
        ) {
            // Adopt this release's fingerprints when only an equivalence pair differed,
            // so later launches do not keep re-evaluating historical hashes.
            if let Some(versions) = &expected {
                if marker.get("pipelines") != Some(versions) {
                    stamp_pipelines(&base, marker, versions)?;
                }
            }
            if let Some(fingerprint) = expected_customiser.as_deref() {
                stamp_customiser(assets, fingerprint)?;
            }
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
    if !customiser_current(&assets, expected_customiser.as_deref()) {
        return Err("Character customiser preparation did not complete for this release.".into());
    }
    Ok(assets)
}

fn customiser_files_ready(assets: &Path) -> bool {
    std::fs::read(assets.join("private/customisation/current.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|v| {
            v["version"].as_u64() == Some(1)
                && v["set"]
                    .as_str()
                    .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        && ["library-v3.json", "extra-menu.json", "native-lighting.json", "native-roster/complete.json"]
            .iter()
            .all(|name| crate::customiser_parts::asset_directory(assets).join(name).is_file())
        && ["catalog", "library", "menu", "lighting", "roster"].iter().all(|stage| {
            let directory = crate::customiser_parts::asset_directory(assets);
            std::fs::read(directory.join(format!("{stage}-complete.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .is_some_and(|v| receipt_present(&directory, &v["files"]))
        })
}

fn customiser_current(assets: &Path, expected: Option<&str>) -> bool {
    expected.is_none_or(|expected| {
        let degraded = std::fs::read(assets.join("private/customisation/customiser-availability.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        if let Some(v) = degraded.filter(|v| {
            v["version"].as_u64() == Some(1) && v["fingerprint"].as_str() == Some(expected)
        }) {
            if v["status"] == "unavailable" {
                return true;
            }
            if v["status"] == "retained" {
                let directory = crate::customiser_parts::asset_directory(assets);
                return ["catalog", "library", "menu", "lighting", "roster"].iter().all(|stage| {
                    std::fs::read(directory.join(format!("{stage}-complete.json")))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                        .is_some_and(|v| receipt_present(&directory, &v["files"]))
                });
            }
        }
        customiser_files_ready(assets)
    })
}

fn stamp_customiser(assets: &Path, expected: &str) -> Result<(), String> {
    let path = assets.join("private/customisation/current.json");
    let mut current: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if current["fingerprint"].as_str() == Some(expected) {
        return Ok(());
    }
    current["fingerprint"] = serde_json::Value::String(expected.to_owned());
    let bytes = serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    Ok(())
}

// Cheap launch-time completeness check. Setup verifies SHA-256 before reuse;
// hashing every map on every game launch would read gigabytes unnecessarily.
fn receipt_present(root: &Path, files: &serde_json::Value) -> bool {
    files.as_object().is_some_and(|files| !files.is_empty() && files.iter().all(|(name, entry)| {
        let relative = Path::new(name);
        !relative.is_absolute()
            && relative.components().all(|c| matches!(c, std::path::Component::Normal(_)))
            && std::fs::metadata(root.join(relative)).ok()
                .is_some_and(|m| m.is_file() && Some(m.len()) == entry["size"].as_u64())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acknowledged_optional_failure_is_versioned_and_retained_data_is_checked() {
        let root = std::env::temp_dir().join(format!("sk8-availability-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let base = root.join("private/customisation");
        std::fs::create_dir_all(&base).unwrap();
        let availability = base.join("customiser-availability.json");
        std::fs::write(&availability, br#"{"version":1,"fingerprint":"new","status":"unavailable"}"#).unwrap();
        assert!(customiser_current(&root, Some("new")));
        assert!(!customiser_current(&root, Some("future")));
        assert_eq!(crate::customiser_parts::asset_directory(&root), base.join("unavailable"));
        std::fs::write(&availability, br#"{"version":1,"fingerprint":"new","status":"retained"}"#).unwrap();
        assert!(!customiser_current(&root, Some("new")));
        std::fs::write(base.join("current.json"), br#"{"set":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#).unwrap();
        let directory = base.join("sets/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("payload"), b"ok").unwrap();
        for stage in ["catalog", "library", "menu", "lighting", "roster"] {
            std::fs::write(directory.join(format!("{stage}-complete.json")),
                br#"{"files":{"payload":{"size":2}}}"#).unwrap();
        }
        assert!(customiser_current(&root, Some("new")));
        std::fs::remove_file(directory.join("payload")).unwrap();
        assert!(!customiser_current(&root, Some("new")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn receipts_reject_missing_files_empty_lists_and_traversal() {
        let root = std::env::temp_dir();
        assert!(!receipt_present(&root, &serde_json::json!({})));
        assert!(!receipt_present(&root, &serde_json::json!({"../missing": {"size": 0}})));
        assert!(!receipt_present(&root, &serde_json::json!({"nonexistent-skate-setup-test": {"size": 0}})));
    }

    #[test]
    fn customiser_accepts_installed_data_when_release_fingerprint_changes() {
        let root = std::env::temp_dir().join(format!(
            "sk8-customiser-receipt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let base = root.join("private/customisation/sets/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            root.join("private/customisation/current.json"),
            br#"{"version":1,"set":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","fingerprint":"old","source":"source"}"#,
        )
        .unwrap();
        for name in [
            "library-v3.json",
            "extra-menu.json",
            "native-lighting.json",
            "native-roster/complete.json",
        ] {
            let path = base.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, b"ok").unwrap();
        }
        std::fs::write(base.join("payload"), b"ok").unwrap();
        for stage in ["catalog", "library", "menu", "lighting", "roster"] {
            std::fs::write(
                base.join(format!("{stage}-complete.json")),
                br#"{"files":{"payload":{"size":2}}}"#,
            )
            .unwrap();
        }
        assert!(customiser_current(&root, Some("new")));
        stamp_customiser(&root, "new").unwrap();
        let current = serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(root.join("private/customisation/current.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(current["fingerprint"].as_str(), Some("new"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pipelines_accept_valid_group_outputs_when_fingerprint_changes() {
        let expected = serde_json::json!({"core": "new", "maps": "maps-new"});
        let marker = serde_json::json!({
            "pipelines": {"core": "old", "maps": "maps-old"},
            "outputs": {
                "maps": {"maps/university.skate": {"size": 4}}
            }
        });
        let root = std::env::temp_dir().join(format!(
            "sk8-pipeline-receipt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let maps = root.join("maps/university.skate");
        std::fs::create_dir_all(maps.parent().unwrap()).unwrap();
        std::fs::write(&maps, b"test").unwrap();
        assert!(pipelines_acceptable(&marker, &expected, None, &root));
        assert!(!pipelines_current(&marker, &expected, None));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pipelines_accept_exact_and_listed_equivalence_only() {
        let expected = serde_json::json!({"core": "new", "hud": "hud"});
        let marker = serde_json::json!({"pipelines": {"core": "new", "hud": "hud"}});
        assert!(pipelines_current(&marker, &expected, None));
        let migrated = serde_json::json!({"pipelines": {"core": "old", "hud": "hud"}});
        let table = serde_json::json!({"core": [["old", "new"]]});
        assert!(pipelines_current(&migrated, &expected, Some(&table)));
        assert!(!pipelines_current(&migrated, &expected, None));
        let other = serde_json::json!({"core": [["other", "new"]]});
        assert!(!pipelines_current(&migrated, &expected, Some(&other)));
        let reverse = serde_json::json!({"core": [["new", "old"]]});
        assert!(!pipelines_current(&migrated, &expected, Some(&reverse)));
    }
}
