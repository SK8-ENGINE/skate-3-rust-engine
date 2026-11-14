//! A map switch's game data (living world, peds, traffic, character lighting), read on the map loader thread so the
//! commit frame only installs it. The loaders take it when it was read for their map and otherwise read the files
//! themselves with the same functions, so the result is the same either way.
//! Details: docs/hails-additions/25-perf-diagnostics.md#map-switch-game-data-read-on-the-loader-thread-2026-10-24-user-asks-299
use crate::living_world::peds::PedData;
use skate_core::living_world::peds::NavRules;
use crate::living_world::vehicles::VehicleData;
use crate::living_world::LoadedData;
use crate::retail_character::PreparedLighting;
use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// What the loader thread read for one map.
#[derive(Default)]
pub(crate) struct MapData {
    living: Option<LoadedData>,
    peds: Option<(NavRules, PedData)>,
    traffic: Option<VehicleData>,
    lighting: Option<Result<PreparedLighting, Option<String>>>,
}

/// Reads them in parallel. A reader that panics leaves its part empty; the loader then reads it on the game thread
/// as before. `rules` is the ped nav rules at the start of the load (none: no living world).
pub(crate) fn read(root: &Path, name: &str, path: Option<&Path>, retail: bool, rules: Option<NavRules>) -> MapData {
    let _span = info_span!("map_read_ahead").entered();
    std::thread::scope(|scope| {
        let living = scope.spawn(|| crate::living_world::load_data(root, name));
        let peds = rules.map(|rules| scope.spawn(move || {
            let data = crate::living_world::peds::prepare(root, name, &rules);
            (rules, data)
        }));
        let traffic = scope.spawn(|| crate::living_world::vehicles::read_vehicle_data(root));
        let lighting = path.filter(|_| retail).map(|p| crate::retail_character::read(root, p));
        MapData {
            living: living.join().ok(),
            peds: peds.and_then(|h| h.join().ok()),
            traffic: traffic.join().ok(),
            lighting,
        }
    })
}

/// The committed map's read-ahead data until each loader takes its part.
#[derive(Resource, Default)]
pub(crate) struct PreparedMapData {
    key: Option<(String, u64)>,
    path: Option<PathBuf>,
    data: MapData,
}

impl PreparedMapData {
    pub(crate) fn new(name: String, generation: u64, path: Option<PathBuf>, data: MapData) -> Self {
        Self { key: Some((name, generation)), path, data }
    }

    fn for_key(&mut self, key: &(String, u64)) -> Option<&mut MapData> {
        (self.key.as_ref() == Some(key)).then_some(&mut self.data)
    }

    pub(crate) fn take_living(&mut self, key: &(String, u64)) -> Option<LoadedData> {
        self.for_key(key)?.living.take()
    }

    /// Only when read with the current nav rules (a mod may change them during the load).
    pub(crate) fn take_peds(&mut self, key: &(String, u64), rules: &NavRules) -> Option<PedData> {
        let (read_with, data) = self.for_key(key)?.peds.take()?;
        (read_with == *rules).then_some(data)
    }

    pub(crate) fn take_traffic(&mut self, key: &(String, u64)) -> Option<VehicleData> {
        self.for_key(key)?.traffic.take()
    }

    pub(crate) fn take_lighting(&mut self, path: &Path) -> Option<Result<PreparedLighting, Option<String>>> {
        (self.path.as_deref() == Some(path)).then(|| self.data.lighting.take()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_go_only_to_their_map_and_only_once() {
        let data = MapData { traffic: Some(VehicleData::default()), ..default() };
        let mut prepared = PreparedMapData::new("DownTown".into(), 2, None, data);
        assert!(prepared.take_traffic(&("DownTown".into(), 1)).is_none(), "an older generation");
        assert!(prepared.take_traffic(&("University".into(), 2)).is_none(), "another map");
        assert!(prepared.take_traffic(&("DownTown".into(), 2)).is_some());
        assert!(prepared.take_traffic(&("DownTown".into(), 2)).is_none(), "taken once");
        assert!(prepared.take_lighting(Path::new("x.skate")).is_none());
    }

    #[test]
    fn peds_read_with_the_current_nav_rules_are_taken() {
        let rules = NavRules::default();
        let data = MapData { peds: Some((rules.clone(), PedData::default())), ..default() };
        let mut prepared = PreparedMapData::new("DownTown".into(), 1, None, data);
        assert!(prepared.take_peds(&("DownTown".into(), 1), &rules).is_some());
    }
}
