//! Challenge Map teleports (`teleport:<fe_locations key>` handler): request, planning and apply (b244d).
//! Retail and ours: docs/hails-additions/31-retail-menus.md#challenge-map-teleports
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One Challenge Map teleport (serialisable: a host can send it to peers; multiplayer-ready, no transport here).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct TeleportRequest {
    /// The fe_locations key (`dist_downtown_aletown`).
    pub destination: String,
    /// The location's map name (`DownTown`).
    pub map: String,
    /// The locator matrix, rows as written by setup.
    pub transform: [[f32; 4]; 4],
    /// Retail IsOnBoard (0 for map teleports) and IsSessionMarkerTeleport (0).
    pub on_board: bool,
    pub session_marker: bool,
}

/// Why a teleport could not start.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Refused {
    UnknownLocation,
    MapNotInstalled,
    MultiplayerMapSwitch,
}

/// Plan a teleport to `key`: the request, and the map to load first when it is not the current one.
pub(crate) fn plan(
    key: &str,
    destinations: &[crate::teleport_menu::Destination],
    maps: &[crate::map_library::Entry],
    current: Option<&std::path::Path>,
    current_layout: Option<&str>,
    worlds: &std::collections::BTreeMap<String, crate::park_layout::WorldLayout>,
    multiplayer: bool,
) -> Result<(TeleportRequest, Option<crate::map_library::Entry>), Refused> {
    let d = destinations.iter().find(|d| d.id == key).ok_or(Refused::UnknownLocation)?;
    let transform = d.matrix.ok_or(Refused::UnknownLocation)?;
    let request = TeleportRequest { destination: d.id.clone(), map: d.map.clone(), transform, on_board: false, session_marker: false };
    // A skate.Park world places its layout on the stream (park_layout.rs): another layout on this map is a reload.
    let layout = crate::park_layout::layout_for(worlds, d.world.as_deref(), &d.map);
    if current.is_some_and(|p| crate::teleport_menu::same_map(p, &d.map)) && layout == current_layout {
        return Ok((request, None));
    }
    let entry = maps.iter().find(|m| m.path.as_deref().is_some_and(|p| crate::teleport_menu::same_map(p, &d.map))).ok_or(Refused::MapNotInstalled)?;
    if multiplayer {
        return Err(Refused::MultiplayerMapSwitch);
    }
    Ok((request, Some(crate::map_library::Entry { world: layout.and(d.world.clone()), ..entry.clone() })))
}

/// Locations the map screen's teleport handler picked (queued by `retail_menu_stage`, planned by
/// [`start_requested`]) and the teleport waiting for its map (applied by [`finish_travel`]).
#[derive(Resource, Default)]
pub(crate) struct ChallengeMapTravel {
    pub requested: Vec<String>,
    pub pending: Option<(TeleportRequest, Option<PathBuf>)>,
}

/// Plan and start the requested teleports (the destinations and installed maps read when one comes).
pub(crate) fn start_requested(
    mut travel: ResMut<ChallengeMapTravel>,
    config: Res<crate::config::Config>,
    current: Res<crate::map_transition::CurrentMap>,
    net: Res<crate::multiplayer::Multiplayer>,
    mut transition: ResMut<crate::map_transition::MapTransition>,
) {
    let Some(key) = travel.requested.pop() else { return };
    travel.requested.clear();
    let destinations = crate::teleport_menu::load(&config.asset_root).unwrap_or_default();
    let maps = crate::map_library::discover(&config.asset_root);
    let worlds = crate::park_layout::worlds(&config.asset_root);
    match plan(&key, &destinations, &maps, current.path.as_deref(), current.layout.as_deref(), &worlds, net.active()) {
        Ok(planned) => travel.start(planned, current.path.clone(), &mut transition),
        Err(refused) => warn!("CHALLENGE_MAP_TELEPORT {key} refused: {refused:?}"),
    }
}

impl ChallengeMapTravel {
    /// Start a planned teleport: queue it, and request the map transition when it is on another map.
    pub(crate) fn start(&mut self, planned: (TeleportRequest, Option<crate::map_library::Entry>), current: Option<PathBuf>, transition: &mut crate::map_transition::MapTransition) {
        let (request, map) = planned;
        let path = map.as_ref().map_or(current, |m| m.path.clone());
        info!("CHALLENGE_MAP_TELEPORT {} map={} load={}", request.destination, request.map, map.is_some());
        if let Some(entry) = map {
            transition.request(entry);
        }
        self.pending = Some((request, path));
    }
}

/// Apply the queued teleport once no map transition runs and the right map is current.
pub(crate) fn finish_travel(
    mut travel: ResMut<ChallengeMapTravel>,
    transition: Res<crate::map_transition::MapTransition>,
    current: Res<crate::map_transition::CurrentMap>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
) {
    if transition.busy() {
        return;
    }
    let Some((request, path)) = travel.pending.take() else { return };
    // A failed map transaction keeps the previous world: never apply another map's coordinates there.
    if current.path != path {
        warn!("CHALLENGE_MAP_TELEPORT {} dropped: the map did not load", request.destination);
        return;
    }
    if let Err(e) = skater.travel_ex(request.transform, None, request.on_board) {
        warn!("CHALLENGE_MAP_TELEPORT {} failed: {e}", request.destination);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_library::Entry;
    use crate::teleport_menu::Destination;

    fn destination(id: &str, map: &str) -> Destination {
        let mut m = [[0.0; 4]; 4];
        m[3] = [1.0, 2.0, 3.0, 1.0];
        Destination { id: id.into(), name: id.into(), map: map.into(), world: None, label_key: None, matrix: Some(m), source: None }
    }

    #[test]
    fn a_teleport_is_instant_on_its_map_loads_another_and_is_refused_cleanly() {
        let destinations = [destination("dist_downtown_aletown", "DownTown"), destination("dist_university_x", "University")];
        let maps = [
            Entry { label: "DownTown".into(), path: Some(PathBuf::from("maps/DownTown.skate")), world: None },
            Entry { label: "University".into(), path: Some(PathBuf::from("maps/University.skate")), world: None },
        ];
        let here = PathBuf::from("maps/DownTown.skate");
        let (request, load) = plan("dist_downtown_aletown", &destinations, &maps, Some(&here), None, &Default::default(), false).unwrap();
        assert!(load.is_none(), "same map: no load");
        assert_eq!((request.on_board, request.session_marker, request.transform[3]), (false, false, [1.0, 2.0, 3.0, 1.0]), "off the board, the matrix as is");
        let (_, load) = plan("dist_university_x", &destinations, &maps, Some(&here), None, &Default::default(), false).unwrap();
        assert_eq!(load.and_then(|m| m.path), Some(PathBuf::from("maps/University.skate")));
        assert_eq!(plan("dist_university_x", &destinations, &maps, Some(&here), None, &Default::default(), true).unwrap_err(), Refused::MultiplayerMapSwitch);
        assert!(plan("dist_downtown_aletown", &destinations, &maps, Some(&here), None, &Default::default(), true).is_ok(), "same map is fine in multiplayer");
        assert_eq!(plan("nowhere", &destinations, &maps, Some(&here), None, &Default::default(), false).unwrap_err(), Refused::UnknownLocation);
        assert_eq!(plan("dist_university_x", &destinations, &maps[..1], Some(&here), None, &Default::default(), false).unwrap_err(), Refused::MapNotInstalled);
        // A skate.Park world on the current map with another layout reloads the map as that world.
        let mut park = destination("megapark_full", "MegaPark");
        park.world = Some("dist_megapark_full".into());
        let stadium = [Entry { label: "Stadium".into(), path: Some(PathBuf::from("maps/MegaPark.skate")), world: None }];
        let worlds = [("dist_megapark_full".to_string(), crate::park_layout::WorldLayout { layout: "MegaPark_Fullpark".into(), stream: "DIST_MegaPark".into() })].into();
        let at_stadium = PathBuf::from("maps/MegaPark.skate");
        let (_, load) = plan("megapark_full", &[park.clone()], &stadium, Some(&at_stadium), None, &worlds, false).unwrap();
        assert_eq!(load.and_then(|m| m.world), Some("dist_megapark_full".into()), "Empty to Full: reload with the world");
        assert!(plan("megapark_full", &[park], &stadium, Some(&at_stadium), Some("MegaPark_Fullpark"), &worlds, false).unwrap().1.is_none(), "same layout: no load");
        // The event survives serialisation unchanged (what a host sends a peer).
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<TeleportRequest>(&json).unwrap(), request);
    }
}
