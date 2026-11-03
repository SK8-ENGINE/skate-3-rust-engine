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
mod updater;
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
mod extras_host;
mod spawn_skater_host;
mod tips_loader_host;
mod career_stats_host;
mod free_skate_options_host;
mod hud_minimap;
mod text_entry_ui;
mod image_renderer_host;
mod retail_menu_movies;
mod retail_menu_stage;
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
}
mod modding;
mod customiser;
mod customiser_parts;
mod customiser_material;
mod custom_models;
mod teleport_menu;
mod teleporters;
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
    match updater::recover() {
        Ok(true) => return bevy::app::AppExit::Success,
        Err(error) => { eprintln!("{error}"); return bevy::app::AppExit::Success; },
        Ok(false) => {}
    }
    // Setup tool mode: `--extract-ocean-pca <default.xex> <ocean-pca.json>`.
    // Before the crash supervisor, whose report window would block setup on
    // a failed (non-zero) extraction.
    if let Some(code) = extract_ocean_pca() { std::process::exit(code); }
    // Setup tool mode: `--extract-menu-tables <default.xex> <menu-tables.json>`.
    if let Some(code) = extract_menu_tables() { std::process::exit(code); }
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
