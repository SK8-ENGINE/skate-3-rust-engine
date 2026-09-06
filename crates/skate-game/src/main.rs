mod animation;
mod animation_pose;
mod app;
mod assets;
mod camera;
mod config;
mod graph_host;
mod graph_runtime;
mod input;
mod physics;
mod skater_animation;
mod verification;
mod world;

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
    let physics = match physics::GamePhysics::load(&config.asset_root) {
        Ok(physics) => physics,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    let skater = match physics::SkaterRuntime::load(&config.asset_root, &graphs, &physics, "easy") {
        Ok(skater) => skater,
        Err(error) => {
            eprintln!("{error}");
            return bevy::app::AppExit::error();
        }
    };
    app::build(config, manifest, graphs, physics, skater).run()
}

#[cfg(test)]
#[path = "tests/action_host.rs"]
mod action_host_tests;
