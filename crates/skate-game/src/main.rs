mod animation;
mod animation_pose;
mod app;
mod assets;
mod camera;
mod config;
mod difficulty;
mod graph_host;
mod graph_runtime;
mod input;
mod physics;
mod skater_animation;
mod verification;
mod performance;
mod graphics_menu;
mod render_capacity;
mod presentation;
mod replay;
mod world;
mod grind_world;
mod skate_world;

fn main() -> bevy::app::AppExit {
    let config = match config::Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    if let Err(error) = skate_data::input_config::StockGameplayConfig::load(&config.asset_root) {
        eprintln!("{error}");
        return bevy::app::AppExit::error();
    }
    let manifest = match skate_data::GameAssets::load(&config.asset_root) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    let graphs = match graph_runtime::StockGraphs::load(&config.asset_root, &manifest) {
        Ok(graphs) => graphs,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    if let Some(map) = &config.map {
        if let Err(error) = skate_world::validate_runtime(map) {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    }
    eprintln!("SKATE_DIFFICULTY mode={} native_index={}", config.difficulty.key(), config.difficulty as u32);
    let physics = match physics::GamePhysics::load_with_difficulty(&config.asset_root, config.map.as_ref(), config.difficulty) {
        Ok(physics) => physics,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    let skater = match physics::SkaterRuntime::load(&config.asset_root, &graphs, &physics, config.difficulty.key()) {
        Ok(skater) => skater,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    let controls = match physics::PlayerControls::load(&config.asset_root) {
        Ok(controls) => controls,
        Err(error) => { eprintln!("{error}"); return bevy::app::AppExit::error(); }
    };
    let mut app = app::build(config, manifest, graphs, physics, skater);
    app.insert_resource(controls);
    app.run()
}

#[cfg(test)]
#[path = "tests/action_host.rs"]
mod action_host_tests;
