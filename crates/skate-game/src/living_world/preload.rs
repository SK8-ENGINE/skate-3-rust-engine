//! The living world's models preloaded behind the startup loading screen (b226, b229), following mods.
//! Details: docs/hails-additions/26b-living-world-population.md#living-world-preload
use super::{npc_skaters, peds, vehicles, PopulationState};
use bevy::asset::RecursiveDependencyLoadState;
use bevy::prelude::*;
use std::collections::BTreeSet;

/// The longest the startup loading screen waits for the models.
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Resource, Default)]
pub(crate) struct LivingWorldPreload {
    key: Option<(String, u64)>,
    paths: BTreeSet<String>,
    handles: Vec<Handle<Scene>>,
    started: Option<std::time::Instant>,
    /// The startup hold is released (all loaded or failed, or `MAX_WAIT`).
    settled: bool,
}

/// Every model path the living world's spawners can load on this map.
pub(crate) fn preload_paths(
    state: &PopulationState,
    ped_data: &peds::PedData,
    ped_looks: &peds::PedLooks,
    traffic: &vehicles::TrafficState,
    car_overrides: &vehicles::VehicleOverrides,
    npc_looks: &npc_skaters::NpcSkaterLooks,
    models: &crate::custom_models::CustomModels,
) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    paths.extend(ped_data.catalog.reachable_recipes(&ped_looks.overrides).iter().map(|recipe| peds::glb_path(ped_looks, recipe)));
    if let Some(catalog) = &state.vehicles {
        for entity in catalog.categories.values().flatten() {
            if let Some(record) = catalog.entities.get(entity) {
                paths.insert(vehicles::car_glb(&traffic.data, car_overrides, entity, &record.model));
            }
        }
    }
    if let Some(skaters) = &state.skaters {
        paths.extend(skaters.characters.iter().map(|c| npc_skaters::look_path(npc_looks, models, &c.key)));
    }
    paths.extend(ped_data.hand_props.props.values().filter_map(|p| p.glb.clone()));
    // A missing model record gives no path; the spawner tries it and fails as before.
    paths.remove("");
    paths
}

/// The paths worth loading: a stock model the install does not have (the vehicle catalog lists every district's
/// cars, not all exported) would only fail; a spawner that picks one fails as before. Mod paths are kept.
fn on_disk(asset_root: &std::path::Path, paths: BTreeSet<String>) -> BTreeSet<String> {
    paths.into_iter().filter(|p| !p.starts_with("private/") || asset_root.join(p).is_file()).collect()
}

/// Startup: the loading screen waits for the models (released by `preload_models`).
pub(crate) fn take_models_hold(startup: Option<ResMut<crate::startup_loading::StartupLoading>>) {
    if let Some(mut startup) = startup {
        startup.hold("living_world_models", "Loading people and cars");
    }
}

/// Load the set when the map's data is in (or mod looks change), and release the startup hold once it is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn preload_models(
    mut preload: ResMut<LivingWorldPreload>,
    server: Res<AssetServer>,
    map: Res<crate::map_transition::CurrentMap>,
    state: Res<PopulationState>,
    ped_data: Res<peds::PedData>,
    ped_looks: Res<peds::PedLooks>,
    traffic: Res<vehicles::TrafficState>,
    car_overrides: Res<vehicles::VehicleOverrides>,
    npc_looks: Res<npc_skaters::NpcSkaterLooks>,
    models: Option<Res<crate::custom_models::CustomModels>>,
    config: Res<crate::config::Config>,
    startup: Option<ResMut<crate::startup_loading::StartupLoading>>,
) {
    let key = Some((map.name.clone(), map.generation));
    let data_in = state.loaded_for == key && ped_data.loaded_for == key && traffic.loaded_for == key;
    let default_models = crate::custom_models::CustomModels::default();
    // Not `CustomModels`: the customiser's systems mark it changed every frame; an NPC look it changes mid-map loads
    // lazily, as before.
    let looks_changed = ped_looks.is_changed() || car_overrides.is_changed() || npc_looks.is_changed();
    let models = models.as_deref().unwrap_or(&default_models);
    if data_in && (preload.key != key || (looks_changed && preload.key.is_some())) {
        let paths = on_disk(&config.asset_root, preload_paths(&state, &ped_data, &ped_looks, &traffic, &car_overrides, &npc_looks, models));
        if preload.key != key || paths != preload.paths {
            // The new handles first: a path in both sets keeps its loaded asset.
            let handles: Vec<Handle<Scene>> = paths.iter().map(|p| server.load(GltfAssetLabel::Scene(0).from_asset(p.clone()))).collect();
            info!("LIVING_WORLD_PRELOAD {}: {} models", map.name, handles.len());
            preload.handles = handles;
            preload.paths = paths;
            preload.started.get_or_insert_with(std::time::Instant::now);
        }
        preload.key = key;
    }
    if preload.settled || preload.key.is_none() {
        return;
    }
    let pending = preload
        .handles
        .iter()
        .filter(|h| !matches!(server.recursive_dependency_load_state(h.id()), RecursiveDependencyLoadState::Loaded | RecursiveDependencyLoadState::Failed(_)))
        .count();
    let waited = preload.started.map_or(std::time::Duration::ZERO, |t| t.elapsed());
    if pending == 0 || waited >= MAX_WAIT {
        preload.settled = true;
        info!("LIVING_WORLD_PRELOAD ready in {:.0} ms ({pending} still loading)", waited.as_secs_f64() * 1000.0);
        if let Some(mut startup) = startup {
            startup.release("living_world_models");
        }
    }
}
