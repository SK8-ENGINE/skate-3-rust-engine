//! Launch-time asset staleness check (`ASSET_CHECK` lines, `setup::mark_stale`), on a background thread.
//! Details: docs/hails-additions/08-setup-performance.md#asset-staleness-check
use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// One failed check: the setup group to rebuild and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub group: String,
    pub reason: String,
}

fn finding(group: &str, reason: impl Into<String>) -> Finding {
    Finding { group: group.into(), reason: reason.into() }
}

/// The installation behind an asset root (a release copy's `data/installations/<id>/assets`, or the dev `assets`
/// junction to one) and its published marker, when there is one.
fn installation(assets: &Path) -> Option<(PathBuf, serde_json::Value)> {
    let assets = assets.canonicalize().ok()?;
    let install_root = assets.parent()?.to_path_buf();
    let base = install_root.parent()?.parent()?;
    let marker: serde_json::Value = serde_json::from_slice(&std::fs::read(base.join("installation.json")).ok()?).ok()?;
    let published = base.join(marker["directory"].as_str()?).canonicalize().ok()?;
    (published == install_root).then_some((install_root, marker))
}

/// Every output each group recorded at setup is still present at its size. The reason names the first few files
/// that differ, so a log shows what changed (a hand re-export that skipped setup, a deleted file, ...).
fn receipts(install_root: &Path, marker: &serde_json::Value) -> Vec<Finding> {
    let Some(groups) = marker["outputs"].as_object() else { return Vec::new() };
    groups
        .iter()
        .filter(|(_, files)| !crate::setup::outputs_present(install_root, files))
        .map(|(group, files)| {
            let differ = differing_outputs(install_root, files);
            let shown = differ.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let more = differ.len().saturating_sub(3);
            let more = if more > 0 { format!(" and {more} more") } else { String::new() };
            finding(group, format!("converted files missing or changed size ({} of {}: {shown}{more})", differ.len(), files.as_object().map_or(0, |f| f.len())))
        })
        .collect()
}

/// The recorded outputs that are missing or not at their recorded size, as `path (missing)` /
/// `path (N bytes, recorded M)`.
fn differing_outputs(install_root: &Path, files: &serde_json::Value) -> Vec<String> {
    let Some(files) = files.as_object() else { return vec!["no outputs recorded".into()] };
    files
        .iter()
        .filter_map(|(name, entry)| {
            let recorded = entry["size"].as_u64();
            match std::fs::metadata(install_root.join(name)) {
                Ok(m) if m.is_file() && Some(m.len()) == recorded => None,
                Ok(m) if m.is_file() => Some(format!("{name} ({} bytes, recorded {})", m.len(), recorded.map_or("none".into(), |r| r.to_string()))),
                _ => Some(format!("{name} (missing)")),
            }
        })
        .collect()
}

/// HUD group: the menu tables and movies this build's retail menus read.
fn hud(assets: &Path, movies: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    match std::fs::read_to_string(assets.join(crate::retail_menus::TABLES)) {
        Err(_) => out.push(finding("hud", "menu tables missing")),
        Ok(text) => match skate_data::menu_tables::MenuTables::from_json(&text) {
            Err(e) => out.push(finding("hud", format!("menu tables unreadable by this build ({e})"))),
            Ok(t) => {
                for (empty, what) in [
                    (t.screens.is_empty(), "FE screen records"),
                    (t.settings_titles.is_empty(), "settings titles"),
                    (t.sub_options.is_empty(), "sub-option tables"),
                    (t.templates.is_empty(), "screen templates"),
                ] {
                    if empty {
                        out.push(finding("hud", format!("menu tables lack the {what}")));
                    }
                }
            }
        },
    }
    match std::fs::read(movies.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        None => out.push(finding("hud", "menu movie manifest missing")),
        Some(m) => {
            if m["format"] != crate::retail_menu_movies::FORMAT || m["version"] != crate::retail_menu_movies::VERSION {
                out.push(finding("hud", format!("menu movies are version {} (this build reads {})", m["version"], crate::retail_menu_movies::VERSION)));
            } else if let Some(files) = m["files"].as_object() {
                if let Some(name) = files.keys().find(|name| !movies.join(name.as_str()).is_file()) {
                    out.push(finding("hud", format!("menu movie file missing: {name}")));
                }
            }
        }
    }
    let scene = assets.join("private/hud/scene");
    if scene.join("TrickGuide.skate").is_file() && !scene.join("MarqueeStage.skate").is_file() {
        out.push(finding("hud", "menu stage missing: MarqueeStage.skate"));
    }
    out.extend(hud_features(assets));
    out
}

/// HUD files later builds added (offered, not forced): a missing one leaves its feature off until a refresh. The
/// central message HUD (chase "ZAP!" / "BAM!" lines) and the chase alerts came after the scoring HUD.
fn hud_features(assets: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    let hud = assets.join("private/hud/runtime");
    if hud.join("trickdisplay.json").is_file() && !hud.join("hudcentral.json").is_file() {
        out.push(finding("hud", "central message HUD missing"));
    }
    if hud.join("trickdisplay.json").is_file() && !assets.join("private/hud/chase-arrows/chase_arrows.json").is_file() {
        out.push(finding("hud", "ped chase alert textures missing"));
    }
    if hud.join("trickdisplay.json").is_file() && !assets.join("private/hud/replay-shake/replay_shake.json").is_file() {
        out.push(finding("hud", "replay shake clips missing"));
    }
    out
}

/// The cheap part of the check, for the start-up splash: a few file tests and one small file read (the full check
/// waits for the loading screen; its thousands of reads would slow the start on an HDD). These are the features that
/// stay off until Refresh Game Files; out-of-date menus are not here, the full check forces their refresh.
pub(crate) fn quick_check(assets: &Path) -> Vec<Finding> {
    let mut out = hud_features(assets);
    out.extend(hand_props(assets));
    out
}

/// What stays off until a refresh, as the in-game warning box under the frame-time counter shows it (user
/// 2026-10-19): a generation (bumped on every publish) and the features' names. Set by the start-up check and
/// replaced by the full check.
static NOTICE: std::sync::Mutex<(u64, Vec<String>)> = std::sync::Mutex::new((0, Vec::new()));

/// A finding as the player knows the feature (the box and the splash name features, not files).
pub(crate) fn feature_name(f: &Finding) -> String {
    match f.reason.as_str() {
        "central message HUD missing" => "chase messages (ZAP!, BAM!)".into(),
        "ped chase alert textures missing" => "ped chase alerts".into(),
        "replay shake clips missing" => "replay camera shake".into(),
        r if r.starts_with("hand prop") => "ped hand props".into(),
        r if r.starts_with("menu") => "menus".into(),
        _ => format!("{} files", f.group),
    }
}

/// Publish findings for the in-game warning box (names deduplicated, in order).
pub(crate) fn publish(findings: &[Finding]) {
    let mut names: Vec<String> = Vec::new();
    for f in findings {
        let n = feature_name(f);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    if let Ok(mut notice) = NOTICE.lock() {
        notice.0 += 1;
        notice.1 = names;
    }
}

/// The published notice: (generation, features that stay off until a refresh).
pub(crate) fn notice() -> (u64, Vec<String>) {
    NOTICE.lock().map(|n| n.clone()).unwrap_or_default()
}

/// The warning box's text for these features (`None` = nothing to warn about).
pub(crate) fn warning_box_text(features: &[String]) -> Option<String> {
    if features.is_empty() {
        return None;
    }
    Some(format!(
        "GAME FILES NEED A REFRESH
Off until then: {}
Launcher: Tools > Refresh Game Files (or Esc > Extras)",
        features.join(", ")
    ))
}

/// The splash warning for start-up findings (user 2026-10-19: "the refresh warning should show under the startup
/// splash once detected during the startup"); `None` when the files are current.
pub(crate) fn splash_warning(findings: &[Finding]) -> Option<String> {
    if findings.is_empty() {
        return None;
    }
    let mut what: Vec<String> = Vec::new();
    for n in findings.iter().map(feature_name) {
        if !what.contains(&n) {
            what.push(n);
        }
    }
    let them = if what.len() == 1 { "it" } else { "them" };
    Some(format!("Game files need a refresh: {} stay off until then. Refresh Game Files (the launcher's Tools, or Esc > Extras in the game) turns {them} on.", what.join(", ")))
}

/// Maps group: ped hand props carry the grip points this build reads; no model on any prop = stale.
fn hand_props(assets: &Path) -> Vec<Finding> {
    let living = assets.join("private/living_world");
    if !living.join("tables.json").is_file() {
        return Vec::new();
    }
    let missing = || vec![finding("maps", "hand prop models missing")];
    let Some(v) = std::fs::read(living.join("hand_props.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        return missing();
    };
    match v["props"].as_object() {
        Some(props) if !props.values().any(|p| p.get("file").is_some_and(serde_json::Value::is_string)) => missing(),
        Some(props) if props.values().all(|p| p.get("attach").is_none()) => vec![finding("maps", "hand props lack their grip points (attach)")],
        Some(_) => Vec::new(),
        None => missing(),
    }
}

/// The DLC folder the way setup finds it (`tools/asset_pipeline/dlc.py` `folder`): `SKATE3_DLC_DIR` (`env`), the folder
/// the last setup used, a `dlc` folder next to the game folder setup read, or a `dlc` folder next to this game copy.
fn dlc_folder(env: Option<PathBuf>, marker: &serde_json::Value, game_dir: Option<&Path>) -> Option<PathBuf> {
    let dir = |p: PathBuf| p.is_dir().then_some(p);
    if let Some(p) = env.and_then(dir).or_else(|| marker["dlc"].as_str().map(PathBuf::from).and_then(dir)) {
        return Some(p);
    }
    if let Some(source) = marker["source"].as_str().map(Path::new) {
        let start = if source.extension().is_some() { source.parent() } else { Some(source) };
        for root in start.into_iter().flat_map(|s| [s.parent(), Some(s)]).flatten() {
            if let Some(p) = dir(root.join("dlc")) {
                return Some(p);
            }
        }
    }
    game_dir.and_then(|g| dir(g.join("dlc")))
}

/// Every file in the DLC folder as `relative/path|size`, sorted (setup's `dlc.listing`, recorded as `dlc_files`).
fn dlc_listing(folder: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            match std::fs::metadata(&path) {
                Ok(m) if m.is_dir() => walk(root, &path, out),
                Ok(m) if m.is_file() => {
                    if let Ok(rel) = path.strip_prefix(root) {
                        let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                        out.push(format!("{}|{}", rel.join("/"), m.len()));
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, folder, &mut out);
    out.sort();
    out
}

/// A DLC pack added or removed since setup reports the hud group, so refresh rebuilds changed groups.
fn dlc(env: Option<PathBuf>, marker: &serde_json::Value, game_dir: Option<&Path>) -> Vec<Finding> {
    let now = dlc_folder(env, marker, game_dir).map_or_else(Vec::new, |d| dlc_listing(&d));
    let changed = match marker["dlc_files"].as_array() {
        Some(seen) => !seen.iter().map(|v| v.as_str()).eq(now.iter().map(|s| Some(s.as_str()))),
        None => marker["dlc"].is_null() && !now.is_empty(),
    };
    if changed {
        vec![finding("hud", format!("DLC packages changed since setup ({} files in the DLC folder now)", now.len()))]
    } else {
        Vec::new()
    }
}

/// All checks for an asset root; one finding per group and reason.
pub(crate) fn check(assets: &Path) -> Vec<Finding> {
    let mut out = installation(assets).map_or(Vec::new(), |(root, marker)| {
        let mut found = receipts(&root, &marker);
        // The game copy holds `data/installations/<id>`.
        let game_dir = root.parent().and_then(Path::parent).and_then(Path::parent);
        found.extend(dlc(std::env::var_os("SKATE3_DLC_DIR").map(PathBuf::from), &marker, game_dir));
        found
    });
    // The set the menus load (`retail_menu_movies::set_root`): a dev export from `SKATE3_MENU_MOVIES` is checked, not the
    // installed set it replaces.
    let movies = std::env::var("SKATE3_MENU_MOVIES").map(std::path::PathBuf::from).unwrap_or_else(|_| assets.join("private/menu-movies"));
    out.extend(hud(assets, &movies));
    out.extend(hand_props(assets));
    out.dedup();
    out
}

pub(crate) struct AssetCheckPlugin;

impl Plugin for AssetCheckPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, start.run_if(loading_done.and(bevy::ecs::schedule::common_conditions::run_once)))
            .add_systems(Update, force_menu_refresh);
    }
}

/// Set by the check when the retail menu movies or tables are older than this build reads (user, 2026-10-18: "force a
/// refresh if the menus are out of date for the new menu work"): the game restarts into setup's refresh at once
/// instead of only offering Extras > Refresh Game Files.
static FORCE_MENU_REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A finding that forces the refresh: the menu movies or tables (the `hud` group's menu files).
pub(crate) fn forces_refresh(f: &Finding) -> bool {
    f.group == "hud" && (f.reason.starts_with("menu movie") || f.reason.starts_with("menu tables") || f.reason.starts_with("menu stage"))
}

/// Restart into the refresh once. Never from a run the refresh itself started (`--refresh-assets`), so a refresh that
/// could not fix the files falls back to the menu's Refresh Game Files row instead of looping.
fn force_menu_refresh(mut exit: MessageWriter<AppExit>, mut done: Local<bool>) {
    if *done || !FORCE_MENU_REFRESH.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    *done = true;
    if std::env::args().any(|a| a == "--refresh-assets") {
        warn!("ASSET_CHECK menus still out of date after a refresh: not forcing another (Extras > Refresh Game Files)");
        return;
    }
    match crate::setup::restart_with_refresh() {
        Ok(()) => {
            warn!("ASSET_CHECK menus out of date: restarting into Refresh Game Files");
            exit.write(AppExit::Success);
        }
        Err(e) => warn!("ASSET_CHECK menus out of date, the forced refresh could not start: {e}"),
    }
}

fn loading_done(loading: Option<Res<crate::startup_loading::StartupLoading>>) -> bool {
    loading.is_none_or(|l| l.done())
}

fn start(config: Res<crate::config::Config>) {
    let assets = config.asset_root.clone();
    let _ = std::thread::Builder::new().name("asset-check".into()).spawn(move || {
        let started = std::time::Instant::now();
        let findings = check(&assets);
        publish(&findings);
        for f in &findings {
            warn!("ASSET_CHECK {}: {}", f.group, f.reason);
            crate::setup::mark_stale(&f.group);
        }
        if findings.iter().any(forces_refresh) {
            FORCE_MENU_REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        info!("ASSET_CHECK done in {} ms: {}", started.elapsed().as_millis(),
            if findings.is_empty() { "game files current".to_owned() } else { format!("{} problem(s), refresh offered", findings.len()) });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_date_menus_force_the_refresh_and_nothing_else_does() {
        assert!(forces_refresh(&finding("hud", "menu movies are version 5 (this build reads 6)")));
        assert!(forces_refresh(&finding("hud", "menu tables lack the FE screens")));
        assert!(forces_refresh(&finding("hud", "menu movie manifest missing")));
        assert!(forces_refresh(&finding("hud", "menu stage missing: MarqueeStage.skate")));
        assert!(!forces_refresh(&finding("hud", "hud textures missing")), "other hud files only offer the refresh");
        assert!(!forces_refresh(&finding("audio", "menu movies")), "other groups do not force it");
    }

    fn tables(screens: bool) -> String {
        let mut t = serde_json::json!({
            "schema": skate_data::menu_tables::SCHEMA, "source_sha256": "", "settings_rows": [], "settings_screens": [],
            "categories": [], "items": [], "keys": [], "modes": [], "addresses": {},
            "templates": [{"name": "CrossBar", "path": "x", "kind": 0}],
            "settings_titles": ["ID_GAMESETTINGS_TITLE"],
            "sub_options": [{"labels": [], "helpers": []}],
        });
        if screens {
            t["screens"] = serde_json::json!([{"id": "a", "key": "a", "blur": false, "suspends_gameplay": false, "chyron": false, "hud": 0, "input_to_movie": false}]);
        }
        t.to_string()
    }

    #[test]
    fn a_missing_or_resized_output_flags_its_group() {
        let base = std::env::temp_dir().join(format!("skate3rust-receipts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let id = "0123456789abcdef0123456789abcdef";
        let root = base.join("installations").join(id);
        std::fs::create_dir_all(root.join("assets/private")).unwrap();
        std::fs::write(root.join("assets/private/a.bin"), b"1234").unwrap();
        std::fs::write(root.join("assets/private/b.bin"), b"12").unwrap();
        let marker = serde_json::json!({"version": 1, "directory": format!("installations/{id}"), "outputs": {
            "core": {"assets/private/a.bin": {"size": 4}},
            "maps": {"assets/private/b.bin": {"size": 3}},
            "audio": {"assets/private/c.bin": {"size": 1}},
        }});
        std::fs::write(base.join("installation.json"), marker.to_string()).unwrap();
        let (found_root, found) = installation(&root.join("assets")).expect("the published installation");
        let findings = receipts(&found_root, &found);
        let groups: Vec<&str> = findings.iter().map(|f| f.group.as_str()).collect();
        assert_eq!(groups, ["audio", "maps"], "missing c.bin, b.bin's size changed; core is fine");
        // The log names the files.
        assert!(findings[0].reason.ends_with("(1 of 1: assets/private/c.bin (missing))"), "{}", findings[0].reason);
        assert!(findings[1].reason.ends_with("(1 of 1: assets/private/b.bin (2 bytes, recorded 3))"), "{}", findings[1].reason);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hand_props_without_grip_points_are_stale() {
        let dir = std::env::temp_dir().join(format!("skate-asset-check-hand-props-{}", std::process::id()));
        let file = dir.join("private/living_world/hand_props.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        assert!(hand_props(&dir).is_empty(), "no living world data: nothing to check");
        std::fs::write(dir.join("private/living_world/tables.json"), "{}").unwrap();
        let missing = vec![finding("maps", "hand prop models missing")];
        assert_eq!(hand_props(&dir), missing, "tables but no hand prop file");
        std::fs::write(&file, r#"{"props": {"orange": {"model": "orange_lw", "error": "no template"}}}"#).unwrap();
        assert_eq!(hand_props(&dir), missing, "no prop has a model");
        std::fs::write(&file, r#"{"props": {"pop": {"file": "hand_props/pop.glb"}}}"#).unwrap();
        assert_eq!(hand_props(&dir), vec![finding("maps", "hand props lack their grip points (attach)")]);
        std::fs::write(&file, r#"{"props": {"pop": {"file": "x", "attach": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]}, "orange": {"model": "orange_lw", "error": "no template"}}}"#).unwrap();
        assert!(hand_props(&dir).is_empty(), "current export (a prop without a template may lack it)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_splash_names_what_stays_off_until_a_refresh() {
        let dir = std::env::temp_dir().join(format!("skate-asset-quick-{}", std::process::id()));
        let runtime = dir.join("private/hud/runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(runtime.join("trickdisplay.json"), "{}").unwrap();
        let found = quick_check(&dir);
        let text = splash_warning(&found).expect("three features off");
        assert!(text.contains("chase messages (ZAP!, BAM!)") && text.contains("ped chase alerts") && text.contains("replay camera shake"), "{text}");
        assert!(text.contains("Refresh Game Files") && text.ends_with("turns them on."), "{text}");
        let names: Vec<String> = found.iter().map(feature_name).collect();
        let boxed = warning_box_text(&names).unwrap();
        assert!(boxed.starts_with("GAME FILES NEED A REFRESH
") && boxed.contains("Off until then: chase messages (ZAP!, BAM!), ped chase alerts, replay camera shake"), "{boxed}");
        assert_eq!(warning_box_text(&[]), None);
        std::fs::write(runtime.join("hudcentral.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("private/hud/chase-arrows")).unwrap();
        std::fs::write(dir.join("private/hud/chase-arrows/chase_arrows.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("private/hud/replay-shake")).unwrap();
        std::fs::write(dir.join("private/hud/replay-shake/replay_shake.json"), "{}").unwrap();
        assert_eq!(splash_warning(&quick_check(&dir)), None, "current files: no warning");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hud_check_reads_the_files_not_a_stamp() {
        let dir = std::env::temp_dir().join(format!("skate3rust-asset-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let private = dir.join("private");
        std::fs::create_dir_all(private.join("menu-movies")).unwrap();
        // Nothing there: both files reported.
        let reasons: Vec<String> = hud(&dir, &dir.join("private/menu-movies")).into_iter().map(|f| f.reason).collect();
        assert_eq!(reasons, ["menu tables missing", "menu movie manifest missing"]);
        // Tables from before the FE screen records, movies one version old.
        std::fs::write(private.join("menu-tables.json"), tables(false)).unwrap();
        std::fs::write(private.join("menu-movies/manifest.json"),
            serde_json::json!({"format": crate::retail_menu_movies::FORMAT, "version": crate::retail_menu_movies::VERSION - 1, "files": {}}).to_string()).unwrap();
        let reasons: Vec<String> = hud(&dir, &dir.join("private/menu-movies")).into_iter().map(|f| f.reason).collect();
        assert!(reasons.iter().any(|r| r.contains("FE screen records")), "{reasons:?}");
        assert!(reasons.iter().any(|r| r.contains("menu movies are version")), "{reasons:?}");
        // Current files: clean, and a listed movie that is gone is caught.
        std::fs::write(private.join("menu-tables.json"), tables(true)).unwrap();
        std::fs::write(private.join("menu-movies/manifest.json"),
            serde_json::json!({"format": crate::retail_menu_movies::FORMAT, "version": crate::retail_menu_movies::VERSION, "files": {"a.json": {}}}).to_string()).unwrap();
        assert_eq!(hud(&dir, &dir.join("private/menu-movies")).into_iter().map(|f| f.reason).collect::<Vec<_>>(), ["menu movie file missing: a.json"]);
        std::fs::write(private.join("menu-movies/a.json"), "{}").unwrap();
        assert!(hud(&dir, &dir.join("private/menu-movies")).is_empty());
        // A set exported before the skater select stage existed.
        let scene = private.join("hud/scene");
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("TrickGuide.skate"), "").unwrap();
        assert_eq!(hud(&dir, &dir.join("private/menu-movies")).into_iter().map(|f| f.reason).collect::<Vec<_>>(), ["menu stage missing: MarqueeStage.skate"]);
        std::fs::write(scene.join("MarqueeStage.skate"), "").unwrap();
        assert!(hud(&dir, &dir.join("private/menu-movies")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The laptop's import (2026-10-21): packs added after setup are seen, the same listing is current, an install
    /// from before the listing that already read a DLC folder stays current, and a `dlc` folder next to the game counts.
    #[test]
    fn dlc_packs_added_after_setup_offer_the_refresh() {
        let dir = std::env::temp_dir().join(format!("skate3rust-asset-check-dlc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let packs = dir.join("imported");
        std::fs::create_dir_all(packs.join("After Dark/454108E6/00000002")).unwrap();
        std::fs::write(packs.join("After Dark/454108E6/00000002/PKG"), [0u8; 7]).unwrap();
        std::fs::write(packs.join("TU_file"), [0u8; 3]).unwrap();
        assert_eq!(dlc_listing(&packs), ["After Dark/454108E6/00000002/PKG|7", "TU_file|3"]);
        let fresh = serde_json::json!({ "dlc": null, "dlc_files": [] });
        // Imported, setup not run yet: the hud group is reported; without the folder nothing is.
        assert_eq!(dlc(Some(packs.clone()), &fresh, None).len(), 1);
        assert_eq!(dlc(Some(packs.clone()), &fresh, None)[0].group, "hud");
        assert!(dlc(None, &fresh, None).is_empty());
        // After the refresh setup recorded the same listing: current.
        let done = serde_json::json!({ "dlc": packs.to_string_lossy(), "dlc_files": dlc_listing(&packs) });
        assert!(dlc(Some(packs.clone()), &done, None).is_empty());
        assert!(dlc(None, &done, None).is_empty(), "a run without the variable reads the saved folder");
        // A pack removed: reported again.
        std::fs::remove_file(packs.join("TU_file")).unwrap();
        assert_eq!(dlc(None, &done, None).len(), 1);
        // From before the listing: current when setup read a folder, reported when it never did.
        assert!(dlc(None, &serde_json::json!({ "dlc": packs.to_string_lossy() }), None).is_empty());
        assert_eq!(dlc(Some(packs.clone()), &serde_json::json!({ "dlc": null }), None).len(), 1);
        // A `dlc` folder next to the game copy.
        let game = dir.join("game copy");
        std::fs::create_dir_all(game.join("dlc/Pack")).unwrap();
        std::fs::write(game.join("dlc/Pack/PKG"), [0u8; 2]).unwrap();
        assert_eq!(dlc_folder(None, &fresh, Some(&game)), Some(game.join("dlc")));
        assert_eq!(dlc(None, &fresh, Some(&game)).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
