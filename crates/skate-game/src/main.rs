/// A data-gated test whose private inputs are missing prints one `TEST_SKIPPED` line (also appended to the file in
/// `SKATE_TEST_SKIPS` when set) and returns (docs/hails-additions/36-known-test-failures.md#skipped-without-private-inputs).
#[cfg(test)]
macro_rules! test_skip {
    ($($why:tt)*) => {{
        let line = format!("TEST_SKIPPED {}: {}", module_path!(), format_args!($($why)*));
        eprintln!("{line}");
        if let Some(path) = std::env::var_os("SKATE_TEST_SKIPS") {
            use std::io::Write as _;
            // One write per line, so parallel tests do not interleave inside a line.
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(format!("{line}\n").as_bytes());
            }
        }
        return;
    }};
}
mod autopilot;
mod frame_timing;
mod system_timing;
mod animation;
mod crash_report;
mod crash_context;
mod multiplayer;
mod apt_vm;
mod apt_display;
mod apt_movie;
mod apt_stage;
mod crossbar_host;
mod fe_language;
mod apt_text;
mod apt_scene;
mod apt_imports;
mod hud_challenge;
mod hud_imports;
mod music_pack;
mod music_playlist;
mod ui_fonts;
mod hud_central;
mod hud_runtime;
mod scoring_runtime;
mod scoring_hud;
mod animation_pose;
mod app;
mod assets;
mod camera;
mod config;
mod setup;
mod map_library;
mod map_render;
mod map_transition;
mod map_read_ahead;
mod difficulty;
mod custom_difficulty;
mod graph_host;
mod graph_runtime;
mod input;
mod session_marker;
mod physics;
mod skater_animation;
mod verification;
mod performance;
mod profiling;
mod graphics_menu;
mod fe_backdrop;
mod trick_guide_scene;
mod marquee_preview;
mod retail_menus;
mod prompt_glyphs;
mod popup_host;
mod settings_host;
mod trick_guide_host;
mod replay_menu_host;
mod challenge_map_host;
mod offline_profile_host;
mod reward_summary_host;
mod end_dialog_host;
mod poster_host;
mod extras_host;
mod spawn_skater_host;
mod music_player_host;
mod tips_loader_host;
mod career_stats_host;
mod free_skate_options_host;
mod hud_minimap;
mod text_entry_ui;
mod image_renderer_host;
mod retail_menu_movies;
mod retail_menu_stage;
mod save_tag;
mod rumble;
mod asset_check;
mod startup_loading;
#[cfg(windows)]
mod splash;
/// The splash is a Win32 window (splash.rs); other platforms draw none (or their own, behind these calls).
#[cfg(not(windows))]
mod splash {
    pub(crate) fn show() {}
    pub(crate) fn close() {}
    pub(crate) fn stage(_label: &str) {}
    pub(crate) fn detail(_text: String) {}
    pub(crate) fn warning(_text: String) {}
    pub(crate) fn active() -> bool { false }
    pub(crate) fn refresh_mode(_groups: &[String]) {}
    pub(crate) fn progress(_fraction: Option<f32>) {}
    pub(crate) fn fail(_text: &str) {}
    pub(crate) fn end_refresh() {}
    pub(crate) fn cancel_requested() -> bool { false }
}
mod modding;
mod customiser;
mod customiser_parts;
mod customiser_material;
mod custom_models;
mod teleport_menu;
mod teleporters;
mod career;
mod challenge_objectives;
mod tutorial_run;
mod challenge_records;
mod challenges;
mod race_ai_host;
mod race_host;
mod contest_ai_host;
mod contest_host;
mod ftl_host;
mod visual_indicators;
mod challenge_map;
mod render_capacity;
mod retail_backdrop;
mod retail_render;
mod video_screens;
mod retail_character;
mod retail_exposure;
mod colour_matrix;
mod retail_irradiance;
mod retail_sky;
mod skater_ghost;
mod presentation;
mod presentation_run;
mod debug_cam;
mod replay;
mod world;
mod grind_world;
mod park_layout;
mod skate_world;
mod map_validation;
mod water_splash;
mod water_bodies;
pub(crate) mod water_look;
mod game_audio;
pub(crate) mod world_audio;
pub(crate) mod ui_audio;
mod living_world;
mod trigger_volumes;
mod trace_all;

fn main() -> bevy::app::AppExit {
    startup_loading::start_clock();
    // The build uses hardware FMA (`.cargo/config.toml`): say so on an older CPU instead of an illegal-instruction crash.
    #[cfg(target_arch = "x86_64")]
    if !std::arch::is_x86_feature_detected!("fma") {
        let text = "Skate 3 Rust Engine needs a CPU with FMA3 support (Intel Haswell / AMD Piledriver, 2013, or newer).";
        eprintln!("{text}");
        crash_report::present_message(text);
        return bevy::app::AppExit::error();
    }
    // Setup tool mode: `--extract-ocean-pca <default.xex> <ocean-pca.json>`.
    // Before the crash supervisor, whose report window would block setup on
    // a failed (non-zero) extraction.
    if let Some(code) = extract_ocean_pca() { std::process::exit(code); }
    // Setup tool mode: `--extract-menu-tables <default.xex> <menu-tables.json>`.
    if let Some(code) = extract_menu_tables() { std::process::exit(code); }
    if let Some(code) = pack_music() { std::process::exit(code); }
    if let Some(code) = pack_music_folder() { std::process::exit(code); }
    if let Some(code) = music_tools() { std::process::exit(code); }
    // Launcher mode: `--update-game-files [--assets <dir>]` checks the game files and refreshes them if needed.
    if let Some(code) = setup::update_game_files() { std::process::exit(code); }
    if let Some(code) = crash_report::entry() { std::process::exit(code); }
    // Before any thread and the log subscriber: `SKATE_TRACE_ALL=1` turns every diagnostic on.
    trace_all::apply();
    // The splash, before anything slow (not for the windowless check modes).
    if !std::env::args().any(|a| a == "--validate-maps" || a == "--check-assets") {
        splash::show();
    }
    let _trace = match profiling::init() {
        Ok(guard) => guard,
        Err(error) => { eprintln!("{error}"); return bevy::app::AppExit::error(); }
    };
    trace_all::announce();
    let _startup = bevy::log::info_span!("startup").entered();
    startup_loading::stage_meta("configuration_and_installation");
    splash::stage("Reading the settings and the map");
    let config = match bevy::log::info_span!("load_configuration_and_map").in_scope(config::Config::from_env) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    profiling::map_metadata(&config);
    if !config.validate_maps && !config.check_assets {
        game_audio::native::begin_early(&config);
        retail_menu_movies::begin_early(&config);
    }
    // Features that stay off until Refresh Game Files go on the splash at once (the full check runs after loading).
    let quick = asset_check::quick_check(&config.asset_root);
    asset_check::publish(&quick);
    if let Some(text) = asset_check::splash_warning(&quick) {
        bevy::log::warn!("ASSET_CHECK start-up: {text}");
        splash::warning(text);
    }
    eprintln!("REPORT_META startup=map_fingerprint:{:016x} difficulty:{} multiplayer_requested:{} custom_appearance:{} renderer:Vulkan", config.map_fingerprint, config.difficulty.key(), config.multiplayer.host.is_some() || config.multiplayer.direct.is_some(), config.multiplayer.appearance.is_some());
    startup_loading::stage_meta("gameplay_configuration");
    if let Err(error) = skate_data::input_config::StockGameplayConfig::load(&config.asset_root) {
        eprintln!("{error}");
        return bevy::app::AppExit::error();
    }
    startup_loading::stage_meta("asset_manifest");
    splash::stage("Reading the game file list");
    let manifest = match bevy::log::info_span!("load_manifest").in_scope(|| skate_data::GameAssets::load(&config.asset_root)) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    startup_loading::stage_meta("stock_graphs");
    splash::stage("Loading animation graphs");
    let graphs = match bevy::log::info_span!("load_graphs").in_scope(|| graph_runtime::StockGraphs::load(&config.asset_root, &manifest)) {
        Ok(graphs) => graphs,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    if config.validate_maps {
        return match map_validation::run(&config, &graphs) {
            Ok(()) => bevy::app::AppExit::Success,
            Err(error) => {
                eprintln!("{error}");
                bevy::app::AppExit::error()
            }
        };
    }
    startup_loading::stage_meta("map_validation");
    splash::stage("Checking the map");
    if let Some(map) = &config.map {
        if let Err(error) = skate_world::validate_runtime(map) {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    }
    eprintln!("SKATE_DIFFICULTY mode={} native_index={}", config.difficulty.key(), config.difficulty as u32);
    startup_loading::stage_meta("physics_initialization");
    splash::stage("Loading physics");
    let mut physics = match bevy::log::info_span!("load_physics").in_scope(|| physics::GamePhysics::load_with_difficulty(&config.asset_root, config.map.as_ref(), config.difficulty)) {
        Ok(physics) => physics,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    if config.multiplayer.spawn_offset != 0.0 {
        let mut spawn = physics.board.part_transforms()[skate_core::physics::board::BodyId::Deck.index()];
        spawn.translation.x += config.multiplayer.spawn_offset;
        physics.board.set_transform(spawn);
    }
    startup_loading::stage_meta("skater_initialization");
    splash::stage("Loading the skater");
    let skater = match bevy::log::info_span!("load_skater").in_scope(|| physics::SkaterRuntime::load(&config.asset_root, &graphs, &physics, config.difficulty.profile_key())) {
        Ok(skater) => skater,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    startup_loading::stage_meta("controls_initialization");
    let controls = match physics::PlayerControls::load(&config.asset_root) {
        Ok(controls) => controls,
        Err(error) => { eprintln!("{error}"); return bevy::app::AppExit::error(); }
    };
    if config.check_assets {
        if let Err(error) = camera::CameraRuntime::load(&config.asset_root) {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
        match trigger_volumes::check(config.map_path.as_deref(), config.map.as_ref()) {
            Ok(count) => eprintln!("SKATE_TRIGGERS_READY volumes={count}"),
            Err(error) => {
                eprintln!("{error}");
                return bevy::app::AppExit::error();
            }
        }
        // The same file check the game runs after loading (laptop 2026-10-19: this printed READY with v9 menus while the
        // game reads v10, so the launch restarted into Refresh Game Files). A finding that forces that refresh fails the
        // check; the others (features that stay off until a refresh) are listed and the check still passes.
        let findings = asset_check::check(&config.asset_root);
        for f in &findings {
            eprintln!("SKATE_ASSETS_STALE group={} reason={}", f.group, f.reason);
        }
        if findings.iter().any(asset_check::forces_refresh) {
            eprintln!("SKATE_ASSETS_REFRESH_NEEDED (the game would restart into Refresh Game Files)");
            return bevy::app::AppExit::error();
        }
        eprintln!("SKATE_ASSETS_READY");
        return bevy::app::AppExit::Success;
    }
    startup_loading::stage_meta("renderer_and_app_initialization");
    splash::stage("Starting the renderer");
    let mut app = app::build(config, manifest, graphs, physics, skater);
    app.insert_resource(controls);
    startup_loading::stage_meta("app_run");
    splash::stage("Building the world, menus and HUD");
    drop(_startup);
    let exit = app.run();
    // Window close, quit and Ctrl+C all end here: leave no pad rumbling.
    input::platform::stop_rumble_on_exit();
    exit
}

#[cfg(test)]
#[path = "tests/action_host.rs"]
mod action_host_tests;

/// Writes the retail water/ocean animation table for setup (see
/// skate_data::ocean_pca). Runs before any game initialization.
fn extract_ocean_pca() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "--extract-ocean-pca" {
        return None;
    }
    let (Some(xex), Some(out), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("Usage: skate3rust --extract-ocean-pca <default.xex> <ocean-pca.json>");
        return Some(2);
    };
    let result = std::fs::read(&xex)
        .map_err(|e| format!("{}: {e}", std::path::Path::new(&xex).display()))
        .and_then(|bytes| skate_data::ocean_pca::json_from_xex(&bytes))
        .and_then(|json| std::fs::write(&out, json).map_err(|e| format!("{}: {e}", std::path::Path::new(&out).display())));
    match result {
        Ok(()) => {
            println!("OCEAN_PCA_READY");
            Some(0)
        }
        Err(error) => {
            eprintln!("Ocean PCA extraction failed: {error}");
            Some(1)
        }
    }
}

/// Packs setup's decoded music segments (`<dir>/1.wav` .. `<dir>/<count>.wav`, vgmstream's subsong order) into a
/// FLAC music bank (skate_audio::music::bank; doc 11 "Music banks"). Runs before any game initialization.
fn pack_music() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "--pack-music" {
        return None;
    }
    let (Some(dir), Some(count), Some(out), None) = (args.next(), args.next(), args.next(), args.next()) else {
        eprintln!("Usage: skate3rust --pack-music <segment dir> <count> <out.bank>");
        return Some(2);
    };
    let Some(count) = count.to_str().and_then(|c| c.parse::<u32>().ok()) else {
        eprintln!("Music pack: the count must be a number");
        return Some(2);
    };
    let dir = std::path::PathBuf::from(dir);
    let segments = (1..=count).map(|i| {
        let path = dir.join(format!("{i}.wav"));
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|b| skate_audio::music::bank::wav16(&b).map_err(|e| format!("{}: {e}", path.display())))
    });
    let result = std::fs::File::create(&out).map_err(|e| e.to_string()).and_then(|f| {
        let mut f = std::io::BufWriter::new(f);
        skate_audio::music::bank::write_bank(&mut f, count, segments, &[])
    });
    match result {
        Ok(entries) => {
            let seconds: f32 = entries.iter().map(|e| e.seconds()).sum();
            println!("MUSIC_BANK_READY {} segments {:.0} s", entries.len(), seconds);
            Some(0)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&out);
            eprintln!("Music pack failed: {error}");
            Some(1)
        }
    }
}

/// Packs a folder of the player's own music (most formats: MP3, AAC / M4A, ALAC, Ogg Vorbis, FLAC, WAV, AIFF; one
/// track per file, sorted by name; names from the tags, else `Artist - Title.ext`) into a music bank with its track
/// table, each track cut into 10 s segments (doc 11 "Music banks", mod music). Runs before any game initialization.
fn pack_music_folder() -> Option<i32> {
    use skate_audio::music::bank::{write_bank, Track};
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "--pack-music-folder" {
        return None;
    }
    let (Some(dir), Some(out), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("Usage: skate3rust --pack-music-folder <folder of music files> <out.bank>");
        return Some(2);
    };
    println!("{}", music_pack::NOTICE);
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| music_pack::is_music_file(p)).collect(),
        Err(e) => {
            eprintln!("Music pack: {}: {e}", std::path::Path::new(&dir).display());
            return Some(1);
        }
    };
    files.sort();
    // First pass: each file's segment count and names (the bank's index comes first); unreadable files are skipped.
    let mut tracks = Vec::new();
    let mut readable = Vec::new();
    let mut first = 0u32;
    for path in &files {
        let mut count = 0u32;
        match music_pack::decode(path, |_| { count += 1; Ok(()) }) {
            Ok(info) if count > 0 => {
                tracks.push(Track { first, count, name: info.title, artist: info.artist });
                readable.push(path.clone());
                first += count;
            }
            Ok(_) => eprintln!("skipped {}: no audio", path.display()),
            Err(e) => eprintln!("skipped {}: {e}", path.display()),
        }
    }
    if tracks.is_empty() {
        eprintln!("Music pack: no readable music files in {}", std::path::Path::new(&dir).display());
        return Some(1);
    }
    // Second pass: decode on a thread, segments through a small channel into the bank writer.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<skate_audio::music::bank::Segment, String>>(4);
    let decoder = std::thread::spawn(move || {
        for path in readable {
            let send = |s| tx.send(Ok(s)).map_err(|_| "stopped".to_string());
            if let Err(e) = music_pack::decode(&path, send) {
                let _ = tx.send(Err(format!("{}: {e}", path.display())));
                return;
            }
        }
    });
    let result = std::fs::File::create(&out).map_err(|e| e.to_string())
        .and_then(|f| write_bank(&mut std::io::BufWriter::new(f), first, rx, &tracks));
    let _ = decoder.join();
    match result {
        Ok(entries) => {
            let seconds: f32 = entries.iter().map(|e| e.seconds()).sum();
            for t in &tracks {
                println!("track {:?} by {:?}: {} segments", t.name, t.artist, t.count);
            }
            println!("MUSIC_BANK_READY {} tracks {} segments {:.0} s", tracks.len(), entries.len(), seconds);
            Some(0)
        }
        Err(error) => {
            let _ = std::fs::remove_file(&out);
            eprintln!("Music pack failed: {error}");
            Some(1)
        }
    }
}

/// The music bank and playlist tools (doc 11 "Music banks"): `--music-bank-info <bank>` lists a bank's tracks;
/// `--music-playlist <out.json> <name> [--shuffle] <bank>...` writes a playlist of every track of the banks.
fn music_tools() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next()?;
    if command == "--music-bank-info" {
        let (Some(path), None) = (args.next(), args.next()) else {
            eprintln!("Usage: skate3rust --music-bank-info <bank>");
            return Some(2);
        };
        return Some(match music_playlist::read_bank(std::path::Path::new(&path)) {
            Ok(bank) => {
                let seconds: f32 = bank.entries.iter().map(|e| e.seconds()).sum();
                println!("{}: {} tracks, {} segments, {:.0} s", std::path::Path::new(&path).display(), bank.tracks.len(), bank.entries.len(), seconds);
                for line in music_playlist::describe(&bank) {
                    println!("{line}");
                }
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }
    if command != "--music-playlist" {
        return None;
    }
    let rest: Vec<std::ffi::OsString> = args.collect();
    let shuffle = rest.iter().any(|a| a == "--shuffle");
    let rest: Vec<&std::ffi::OsString> = rest.iter().filter(|a| *a != "--shuffle").collect();
    let [out, name, banks @ ..] = rest.as_slice() else {
        eprintln!("Usage: skate3rust --music-playlist <out.json> <name> [--shuffle] <bank>...");
        return Some(2);
    };
    if banks.is_empty() {
        eprintln!("Usage: skate3rust --music-playlist <out.json> <name> [--shuffle] <bank>...");
        return Some(2);
    }
    let out = std::path::PathBuf::from(out);
    let banks: Vec<std::path::PathBuf> = banks.iter().map(std::path::PathBuf::from).collect();
    let result = music_playlist::from_banks(&out, &name.to_string_lossy(), shuffle, &banks)
        .and_then(|p| serde_json::to_string_pretty(&p).map_err(|e| e.to_string()).map(|j| (p, j)))
        .and_then(|(p, j)| std::fs::write(&out, j).map_err(|e| e.to_string()).map(|()| p));
    Some(match result {
        Ok(p) => {
            println!("MUSIC_PLAYLIST_READY {} tracks -> {}", p.tracks.len(), out.display());
            0
        }
        Err(e) => {
            eprintln!("Music playlist failed: {e}");
            1
        }
    })
}

/// Writes the retail front-end menu tables for setup (see
/// skate_data::menu_tables). Runs before any game initialization.
fn extract_menu_tables() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "--extract-menu-tables" {
        return None;
    }
    let (Some(xex), Some(out), None) = (args.next(), args.next(), args.next()) else {
        eprintln!("Usage: skate3rust --extract-menu-tables <default.xex> <menu-tables.json>");
        return Some(2);
    };
    let result = std::fs::read(&xex)
        .map_err(|e| format!("{}: {e}", std::path::Path::new(&xex).display()))
        .and_then(|bytes| skate_data::menu_tables::json_from_xex(&bytes))
        .and_then(|json| std::fs::write(&out, json).map_err(|e| format!("{}: {e}", std::path::Path::new(&out).display())));
    match result {
        Ok(()) => {
            println!("MENU_TABLES_READY");
            Some(0)
        }
        Err(error) => {
            eprintln!("Menu table extraction failed: {error}");
            Some(1)
        }
    }
}
