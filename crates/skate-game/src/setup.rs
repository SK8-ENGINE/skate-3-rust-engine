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

/// Asset groups whose converter changed since setup (no exact or equivalence match) but whose recorded outputs
/// were still accepted: the game runs on them, and the PC overlay asks the player to refresh (`stale_groups`).
fn outdated_groups(marker: &serde_json::Value, expected: &serde_json::Value, equivalence: Option<&serde_json::Value>) -> Vec<String> {
    let installed = marker.get("pipelines").and_then(|v| v.as_object());
    expected.as_object().map_or(Vec::new(), |wanted| {
        wanted
            .iter()
            .filter(|(group, new_hash)| !pipeline_group_matches(installed.and_then(|i| i.get(*group)), new_hash, group, equivalence))
            .map(|(group, _)| group.clone())
            .collect()
    })
}

static STALE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The asset groups found out of date: at startup by converter version (release copies only), or later from what
/// the game finds in them (`mark_stale`).
pub(crate) fn stale_groups() -> Vec<String> {
    STALE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A feature found its group's files older than this build needs (installs an earlier build stamped as current
/// although its converter had changed, e.g. menu tables without the FE screen records).
pub(crate) fn mark_stale(group: &str) {
    let mut stale = STALE.lock().unwrap_or_else(|e| e.into_inner());
    if !stale.iter().any(|g| g == group) {
        stale.push(group.to_owned());
    }
}

/// Screenshot coverage (`SKATE_VERIFY_MENU=stale`): pretend a group is out of date in a development run.
pub(crate) fn preview_stale() {
    mark_stale("hud");
}

/// Every file in a group's setup receipt is present at its recorded size (`asset_check`).
pub(crate) fn outputs_present(install_root: &Path, files: &serde_json::Value) -> bool {
    receipt_present(install_root, files)
}

/// Restart this copy with `--refresh-assets`: setup refreshes the out-of-date groups, then the game starts again.
/// The stale groups' recorded converter versions are cleared first, so setup's refresh (which rebuilds groups whose
/// version differs) rebuilds exactly those, whatever was stamped before.
pub(crate) fn restart_with_refresh() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let root = exe.parent().ok_or("No executable directory")?;
    clear_stale_pipelines(&refresh_base(root)?)?;
    let args: Vec<_> = std::env::args_os().skip(1).filter(|a| a != "--refresh-assets").collect();
    let mut command = Command::new(&exe);
    command.args(args).arg("--refresh-assets");
    // Its own log: the restarted copy outlives the session log this one writes to (a launcher closes that file when
    // this process exits), so setup's progress and the refreshed game's result were lost (laptop 2026-10-18: two
    // refreshes ended with exit code 1 and no line saying why).
    if let Some((path, file)) = refresh_log(root) {
        eprintln!("SETUP: the refresh logs to {}", path.display());
        if let Ok(copy) = file.try_clone() {
            command.stdout(copy);
        }
        command.stderr(file);
    }
    command.spawn().map_err(|e| format!("Could not restart: {e}"))?;
    Ok(())
}

/// `logs\game-<yyyyMMdd-HHmmss>.stderr.log` (the launch scripts' name, local time) where this copy's session logs
/// go: the checkout's `logs\` beside `bin\` for a development copy (`scripts/Launch.ps1`), else `logs\` next to the exe
/// (`PLAY-WITH-LOG.bat`). A forced refresh can restart in the second the session started: then `-2`, `-3`, ...
fn refresh_log(exe_dir: &Path) -> Option<(PathBuf, std::fs::File)> {
    let checkout_logs = exe_dir.parent().map(|p| p.join("logs")).filter(|d| exe_dir.ends_with("bin") && d.is_dir());
    let dir = checkout_logs.unwrap_or_else(|| exe_dir.join("logs"));
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = local_stamp();
    let path = (1..100).map(|n| dir.join(if n == 1 { format!("game-{stamp}.stderr.log") } else { format!("game-{stamp}-{n}.stderr.log") }))
        .find(|p| !p.exists())?;
    let file = std::fs::File::create_new(&path).ok()?;
    Some((path, file))
}

/// The local time as `yyyyMMdd-HHmmss`, like the launch scripts' `Get-Date -Format`.
#[cfg(windows)]
fn local_stamp() -> String {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut SystemTime);
    }
    let mut t = SystemTime::default();
    // SAFETY: GetLocalTime only fills the struct it is given.
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// Elsewhere the same stamp in UTC (it only names the refresh log).
#[cfg(not(windows))]
fn local_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rest / 3_600, rest % 3_600 / 60, rest % 60)
}

/// The data folder a refresh rebuilds: the installation behind a development copy's `--assets` link (refreshed by
/// the checkout's setup), or a release copy's own `data`.
fn refresh_base(root: &Path) -> Result<PathBuf, String> {
    match assets_arg() {
        Some(assets) => {
            dev_setup(root).ok_or("No setup found: this copy has neither support/skate3setup.exe nor a checkout's tools/setup.py.")?;
            dev_base(&assets)
        }
        None if root.join("support/skate3setup.exe").is_file() => Ok(root.join("data")),
        None => Err("This copy has no setup helper: unpack the complete package, or run setup by hand.".into()),
    }
}

/// Clears the stale groups' recorded converter versions, so setup's refresh (which rebuilds groups whose version
/// differs) rebuilds exactly those, whatever was stamped before.
fn clear_stale_pipelines(base: &Path) -> Result<(), String> {
    if let Some((_, marker)) = installed(base)? {
        let mut cleared = marker.clone();
        for group in stale_groups() {
            if let Some(pipelines) = cleared["pipelines"].as_object_mut() {
                pipelines.insert(group, serde_json::Value::String("stale".into()));
            }
        }
        let temporary = base.join("installation.json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&cleared).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, base.join("installation.json")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Launcher mode `--update-game-files` (the Steam launcher's Refresh Game Files): runs the check the game runs at
/// start (`asset_check`) without starting the game and, when it finds a problem, the same refresh as the PC overlay's
/// Extras > Refresh Game Files. Exit code 0 = current or refreshed, 1 = the refresh failed, 2 = no game files yet.
pub(crate) fn update_game_files() -> Option<i32> {
    if !std::env::args_os().skip(1).any(|a| a == "--update-game-files") {
        return None;
    }
    Some(match update_game_files_now() {
        Ok(()) => 0,
        Err((code, error)) => { eprintln!("GAME_FILES failed: {error}"); code }
    })
}

fn update_game_files_now() -> Result<(), (i32, String)> {
    let failed = |e: String| (1, e);
    let exe = std::env::current_exe().map_err(|e| failed(e.to_string()))?;
    let root = exe.parent().ok_or_else(|| failed("No executable directory".into()))?;
    let assets = match assets_arg() {
        Some(assets) => assets,
        None => installed(&root.join("data")).map_err(failed)?.map(|(assets, _)| assets)
            .ok_or_else(|| (2, "this copy has no game files yet: start the game once to run setup".to_owned()))?,
    };
    if !assets.is_dir() {
        return Err((2, format!("no game files at {}", assets.display())));
    }
    let started = std::time::Instant::now();
    let mut findings = crate::asset_check::check(&assets);
    findings.extend(dev_converter_findings(root, &assets));
    eprintln!("ASSET_CHECK done in {} ms: {} problem(s)", started.elapsed().as_millis(), findings.len());
    let refreshed = refresh_for(findings, |groups| {
        for group in groups {
            mark_stale(group);
        }
        let base = refresh_base(root)?;
        clear_stale_pipelines(&base)?;
        eprintln!("GAME_FILES out of date: refreshing {}", groups.join(", "));
        match assets_arg() {
            Some(link) => refresh_dev_copy(&link),
            None => {
                let existing = installed(&base)?;
                run_release_setup(&root.join("support/skate3setup.exe"), &base, &exe, existing.as_ref().map(|(a, _)| a.as_path()))?;
                installed(&base)?.map(|_| ()).ok_or_else(|| "Setup did not publish a complete installation".to_owned())
            }
        }
    })
    .map_err(failed)?;
    eprintln!("{}", if refreshed { "GAME_FILES refreshed" } else { "GAME_FILES current: nothing to refresh" });
    Ok(())
}

/// Development copies have no release.json to compare converters with, so an explicit refresh asks the checkout's
/// `versions.py`. Details: docs/hails-additions/08-setup-performance.md#asset-staleness-check
fn dev_converter_findings(exe_dir: &Path, assets: &Path) -> Vec<crate::asset_check::Finding> {
    let Some(checkout) = exe_dir.ancestors().find(|d| d.join("tools/asset_pipeline/versions.py").is_file()) else { return Vec::new() };
    let Ok(base) = dev_base(assets) else { return Vec::new() };
    let Ok(Some((_, marker))) = installed(&base) else { return Vec::new() };
    let tools = checkout.join("tools/asset_pipeline");
    let output = Command::new("py").arg("-3.13").arg("-I").arg(tools.join("versions.py")).current_dir(checkout).output();
    let expected = match output {
        Ok(o) if o.status.success() => serde_json::from_slice::<serde_json::Value>(&o.stdout).ok(),
        _ => None,
    };
    let Some(expected) = expected else {
        eprintln!("ASSET_CHECK converter versions unavailable: checking the files only");
        return Vec::new();
    };
    let equivalence = std::fs::read(tools.join("pipeline-equivalence.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
    converter_findings(&marker, &expected, equivalence.as_ref())
}

fn converter_findings(marker: &serde_json::Value, expected: &serde_json::Value, equivalence: Option<&serde_json::Value>) -> Vec<crate::asset_check::Finding> {
    // Setup appends the DLC identity; a changed pack is the DLC check's finding, not a converter change.
    let mut marker = marker.clone();
    if let Some(pipelines) = marker["pipelines"].as_object_mut() {
        for hash in pipelines.values_mut() {
            if let Some((converter, _)) = hash.as_str().and_then(|h| h.split_once("+dlc:")) {
                *hash = serde_json::Value::String(converter.to_owned());
            }
        }
    }
    outdated_groups(&marker, expected, equivalence).into_iter()
        .map(|group| crate::asset_check::Finding { group, reason: "converter changed since setup".into() })
        .collect()
}

/// What the start-of-game check found (`asset_check::check`): `refresh` runs once with the out-of-date groups, only
/// when there is a problem. True = refreshed.
fn refresh_for(findings: Vec<crate::asset_check::Finding>, refresh: impl FnOnce(&[String]) -> Result<(), String>) -> Result<bool, String> {
    for f in &findings {
        eprintln!("ASSET_CHECK {}: {}", f.group, f.reason);
    }
    if findings.is_empty() {
        return Ok(false);
    }
    let mut groups: Vec<String> = findings.into_iter().map(|f| f.group).collect();
    groups.sort();
    groups.dedup();
    refresh(&groups)?;
    Ok(true)
}

/// Runs a release copy's setup helper on `base`: a refresh when an installation exists (`existing`, its assets).
fn run_release_setup(setup: &Path, base: &Path, exe: &Path, existing: Option<&Path>) -> Result<(), String> {
    let mut command = Command::new(setup);
    command.arg("--base").arg(base).arg("--game-exe").arg(exe);
    if existing.is_some() { command.arg("--refresh"); }
    // The PC overlay's Ped Speech row (saved in the install's audio settings; on by default): setup decodes the speech
    // unless the player switched it off.
    let speech = existing.is_none_or(crate::game_audio::decode_speech_saved);
    eprintln!("SETUP: speech decode {}", if speech { "on (Ped Speech is on)" } else { "off (Ped Speech is off)" });
    command.env("SKATE_SETUP_SPEECH", if speech { "1" } else { "0" });
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command.status().map_err(|e| format!("Could not start setup: {e}"))?;
    if !status.success() { return Err("Setup was cancelled or did not complete".into()); }
    Ok(())
}

fn stamp_pipelines(
    base: &Path,
    marker: &serde_json::Value,
    expected: &serde_json::Value,
    keep: &[String],
) -> Result<(), String> {
    let mut updated = marker.clone();
    let mut pipelines = expected.clone();
    // Out-of-date groups keep their old fingerprint, so the next start still knows to ask for a refresh.
    for group in keep {
        if let (Some(old), Some(map)) = (marker["pipelines"].get(group).cloned(), pipelines.as_object_mut()) {
            map.insert(group.clone(), old);
        }
    }
    updated["pipelines"] = pipelines;
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

/// The `--assets` directory, when given (development copies and the Steam launcher's versions).
fn assets_arg() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip_while(|a| a != "--assets");
    args.next()?;
    Some(args.next().map_or_else(|| PathBuf::from("assets"), PathBuf::from))
}

/// Setup for a development copy: the packaged helper beside the exe when there is one, else the checkout's
/// `tools/setup.py` (the source the helper is packaged from) found above the exe (`<checkout>/bin/skate3rust.exe`).
fn dev_setup(exe_dir: &Path) -> Option<Command> {
    let packaged = exe_dir.join("support/skate3setup.exe");
    if packaged.is_file() {
        return Some(Command::new(packaged));
    }
    let checkout = exe_dir.ancestors().find(|d| d.join("tools/setup.py").is_file())?;
    let mut command = Command::new("py");
    command.arg("-3.13").arg(checkout.join("tools/setup.py")).current_dir(checkout);
    Some(command)
}

/// The data folder (holding `installation.json`) behind an `--assets` link to `<base>/installations/<id>/assets`.
fn dev_base(assets: &Path) -> Result<PathBuf, String> {
    let assets = assets.canonicalize().map_err(|e| format!("{}: {e}", assets.display()))?;
    let base = assets.parent().and_then(Path::parent).and_then(Path::parent).ok_or("--assets is not inside an installation")?;
    if !base.join("installation.json").is_file() {
        return Err(format!("--assets is not a set-up installation: no installation.json in {}", base.display()));
    }
    Ok(base.to_path_buf())
}

/// Refresh for a development copy (the PC overlay's Refresh Game Files restarted it with `--refresh-assets`): run
/// setup's refresh on the installation behind the `--assets` link, then point the link at the new installation
/// (setup publishes a new `installations/<id>` and removes the old one).
fn refresh_dev_copy(link: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = exe.parent().ok_or("No executable directory")?;
    let base = dev_base(link)?;
    let mut command = dev_setup(exe_dir).ok_or("No setup found for this development copy")?;
    command.arg("--base").arg(&base).arg("--game-exe").arg(&exe).arg("--refresh");
    if installed(&base)?.is_some_and(|(assets, _)| crate::game_audio::decode_speech_saved(&assets)) {
        command.env("SKATE_SETUP_SPEECH", "1");
    }
    eprintln!("SETUP: refreshing the development installation in {}", base.display());
    let status = command.status().map_err(|e| format!("Could not start setup: {e}"))?;
    if !status.success() { return Err("Setup was cancelled or did not complete".into()); }
    let (assets, _) = installed(&base)?.ok_or("Setup did not publish a complete installation")?;
    repoint_link(link, &assets)
}

/// Points a directory link (junction) at `target`, replacing the old link; a real directory is left alone.
fn repoint_link(link: &Path, target: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(link).map_err(|e| format!("{}: {e}", link.display()))?;
    if link.canonicalize().ok().as_deref() == target.canonicalize().ok().as_deref() {
        return Ok(());
    }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: a junction or symlink, whose removal leaves the target alone.
        if meta.file_attributes() & 0x400 == 0 {
            return Err(format!("{} is a real folder, not a link; point --assets at {}", link.display(), target.display()));
        }
        std::fs::remove_dir(link).map_err(|e| format!("Could not remove the old link {}: {e}", link.display()))?;
        // mklink reads a '/' as a switch: installation paths are joined from "installations/<id>".
        let windows = |p: &Path| p.to_string_lossy().replace('/', "\\");
        let status = Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(windows(link)).arg(windows(target)).status()
            .map_err(|e| e.to_string())?;
        if !status.success() { return Err(format!("Could not link {} to {}", link.display(), target.display())); }
    }
    #[cfg(not(windows))] {
        let _ = meta;
        return Err("Re-pointing the assets link is Windows-only".into());
    }
    eprintln!("SETUP: {} now points at {}", link.display(), target.display());
    Ok(())
}

/// A development `--assets` link left pointing at an installation a refresh elsewhere removed (another checkout or
/// launcher version shares the data folder; laptop note 2026-10-19): point it at that data folder's current
/// installation, so scripts and checkouts started outside the launcher keep working.
fn repair_stale_link(link: &Path) -> Result<bool, String> {
    if link.is_dir() {
        return Ok(false);
    }
    let Ok(target) = std::fs::read_link(link) else { return Ok(false) };
    let installations = target.parent().and_then(Path::parent);
    let Some(base) = installations.filter(|i| i.file_name().is_some_and(|n| n == "installations")).and_then(Path::parent) else {
        return Ok(false);
    };
    let Some((assets, _)) = installed(base)? else { return Ok(false) };
    eprintln!("SETUP: {} pointed at a removed installation ({}); re-pointing it", link.display(), target.display());
    repoint_link(link, &assets)?;
    Ok(true)
}

pub(crate) fn asset_root() -> Result<PathBuf, String> {
    if let Some(assets) = assets_arg() {
        if let Err(e) = repair_stale_link(&assets) {
            eprintln!("SETUP: could not repair the assets link {}: {e}", assets.display());
        }
        if std::env::args_os().any(|arg| arg == "--refresh-assets") {
            refresh_dev_copy(&assets)?;
        }
        return Ok(assets);
    }
    if let Some(root) = std::env::var_os("SKATE3_ASSETS") {
        let path = PathBuf::from(root);
        if path.is_dir() {
            return Ok(path);
        }
        return Err(format!(
            "SKATE3_ASSETS points to a missing directory: {}",
            path.display()
        ));
    }
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
    // The PC overlay's Refresh Game Files restarts with this: run setup's refresh even though the copy runs.
    let refresh_asked = std::env::args_os().any(|arg| arg == "--refresh-assets");
    if let Some((assets, marker)) = existing.as_ref().filter(|_| !refresh_asked) {
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
                let stale = outdated_groups(marker, versions, equivalence.as_ref());
                if !stale.is_empty() {
                    eprintln!("SETUP: game files out of date for {} (converter changed); the PC overlay offers a refresh", stale.join(", "));
                }
                for group in &stale {
                    mark_stale(group);
                }
                if marker.get("pipelines") != Some(versions) {
                    stamp_pipelines(&base, marker, versions, &stale)?;
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
        return Err(
            "This copy has not been set up. Use the complete Windows package, pass --assets DIRECTORY, or set SKATE3_ASSETS for development.".into(),
        );
    }
    run_release_setup(&setup, &base, &exe, existing.as_ref().map(|(assets, _)| assets.as_path()))?;
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
    /// A development copy's refresh finds the data folder behind its `--assets` junction and re-points the junction
    /// at the installation setup publishes (the Steam launcher's copies had no way to refresh, 2026-10-12).
    #[cfg(windows)]
    #[test]
    fn dev_refresh_finds_the_base_and_repoints_the_link() {
        let dir = std::env::temp_dir().join(format!("skate3rust-dev-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["old", "new"] {
            std::fs::create_dir_all(dir.join("data").join("installations").join(d).join("assets")).unwrap();
        }
        std::fs::write(dir.join("data").join("installation.json"), "{}").unwrap();
        let (data, installs) = (dir.join("data"), dir.join("data").join("installations"));
        let (old, new) = (installs.join("old").join("assets"), installs.join("new").join("assets"));
        let link = dir.join("assets");
        assert!(Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(&link).arg(&old).status().unwrap().success());
        assert_eq!(dev_base(&link).unwrap(), data.canonicalize().unwrap());
        // Setup's published path is joined with '/' ("installations/<id>"), which mklink must not read as a switch.
        repoint_link(&link, &data.join("installations/new/assets")).unwrap();
        assert_eq!(link.canonicalize().unwrap(), new.canonicalize().unwrap());
        assert!(old.is_dir(), "re-pointing removes the link, never its old target");
        // A real folder is never replaced.
        assert!(repoint_link(&old, &new).is_err());
        std::fs::remove_dir(&link).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// A link to an installation that a refresh in another checkout removed is re-pointed at the data folder's
    /// current installation; a working link and a real folder are left alone.
    #[cfg(windows)]
    #[test]
    fn a_link_to_a_removed_installation_is_repointed() {
        let dir = std::env::temp_dir().join(format!("skate3rust-stale-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (old_id, new_id) = ("a".repeat(32), "b".repeat(32));
        let data = dir.join("data");
        for id in [&old_id, &new_id] {
            std::fs::create_dir_all(data.join("installations").join(id).join("assets")).unwrap();
        }
        std::fs::write(data.join("installation.json"), format!(r#"{{"version":1,"directory":"installations/{new_id}"}}"#)).unwrap();
        let link = dir.join("assets");
        let old = data.join("installations").join(&old_id).join("assets");
        assert!(Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(&link).arg(&old).status().unwrap().success());
        assert!(!repair_stale_link(&link).unwrap(), "a working link stays");
        std::fs::remove_dir_all(data.join("installations").join(&old_id)).unwrap();
        assert!(repair_stale_link(&link).unwrap());
        assert_eq!(link.canonicalize().unwrap(), data.join("installations").join(&new_id).join("assets").canonicalize().unwrap());
        assert!(!repair_stale_link(&data.join("installations").join(&new_id).join("assets")).unwrap(), "a real folder stays");
        std::fs::remove_dir(&link).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// The Steam launcher's Refresh Game Files (`--update-game-files`): current files run no refresh; an out-of-date
    /// group runs it with exactly the groups the start-of-game check found.
    #[test]
    fn update_game_files_refreshes_only_what_the_check_finds() {
        use crate::asset_check::Finding;
        let found = |group: &str, reason: &str| Finding { group: group.into(), reason: reason.into() };
        let mut asked = Vec::new();
        assert_eq!(refresh_for(Vec::new(), |g| { asked.push(g.to_vec()); Ok(()) }), Ok(false));
        assert!(asked.is_empty(), "current files: no refresh");
        let findings = vec![found("maps", "hand props lack their grip points (attach)"), found("hud", "menu tables missing"), found("maps", "x")];
        assert_eq!(refresh_for(findings.clone(), |g| { asked.push(g.to_vec()); Ok(()) }), Ok(true));
        assert_eq!(asked, vec![vec!["hud".to_owned(), "maps".to_owned()]], "one refresh, each group once");
        assert_eq!(refresh_for(findings, |_| Err("cancelled".into())), Err("cancelled".to_owned()), "a failed refresh is reported");
    }
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
    fn an_explicit_refresh_reports_changed_converters() {
        let marker = serde_json::json!({"version": 1, "pipelines": {"core": "a", "hud": "old", "maps": "m1"}});
        let expected = serde_json::json!({"core": "a", "hud": "new", "maps": "m2"});
        let equivalence = serde_json::json!({"maps": [["m1", "m2"]]});
        let groups: Vec<String> = converter_findings(&marker, &expected, Some(&equivalence)).into_iter().map(|f| f.group).collect();
        assert_eq!(groups, ["hud"]);
        assert!(converter_findings(&marker, &marker["pipelines"], None).is_empty());
        let with_dlc = serde_json::json!({"version": 1, "pipelines": {"hud": "new+dlc:pack"}});
        assert!(converter_findings(&with_dlc, &serde_json::json!({"hud": "new"}), None).is_empty(), "DLC identity is not a converter change");
    }

    #[test]
    fn changed_converters_are_reported_and_keep_their_old_fingerprint_when_stamped() {
        let marker = serde_json::json!({"version": 1, "pipelines": {"core": "a", "hud": "old", "maps": "m1"}});
        let expected = serde_json::json!({"core": "a", "hud": "new", "maps": "m2"});
        let equivalence = serde_json::json!({"maps": [["m1", "m2"]]});
        assert_eq!(outdated_groups(&marker, &expected, Some(&equivalence)), vec!["hud".to_string()], "an equivalence pair is current");
        assert_eq!(outdated_groups(&marker, &marker["pipelines"], None), Vec::<String>::new());
        let dir = std::env::temp_dir().join(format!("skate3rust-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        stamp_pipelines(&dir, &marker, &expected, &["hud".to_string()]).unwrap();
        let stamped: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("installation.json")).unwrap()).unwrap();
        assert_eq!(stamped["pipelines"], serde_json::json!({"core": "a", "hud": "old", "maps": "m2"}), "the stale group still asks next time");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pipelines_accept_valid_group_outputs_when_fingerprint_changes() {
        let expected = serde_json::json!({"core": "new", "maps": "maps-new"});
        let marker = serde_json::json!({
            "pipelines": {"core": "old", "maps": "maps-old"},
            "outputs": {
                "core": {"private/game.json": {"size": 2}},
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
        let game = root.join("private/game.json");
        std::fs::create_dir_all(game.parent().unwrap()).unwrap();
        std::fs::write(&game, b"{}").unwrap();
        // Every group with a new fingerprint has its recorded outputs on disk: reused.
        assert!(pipelines_acceptable(&marker, &expected, None, &root));
        assert!(!pipelines_current(&marker, &expected, None));
        // A group with a new fingerprint and no recorded outputs is not: setup has to run for it.
        let mut unrecorded = marker.clone();
        unrecorded["outputs"].as_object_mut().unwrap().remove("core");
        assert!(!pipelines_acceptable(&unrecorded, &expected, None, &root));
        // Nor is a recorded output that is missing or has the wrong size.
        std::fs::write(&game, b"{ }").unwrap();
        assert!(!pipelines_acceptable(&marker, &expected, None, &root));
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
