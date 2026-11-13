//! Career stats for the local player (`skate_core::career_stats`): fed from the fixed step, kept in
//! `settings/career-stats.json`, shown on the loading screen's Stats panel. Details:
//! docs/hails-additions/31-retail-menus.md "Career stats".
use bevy::prelude::*;
use skate_core::career_stats::{row, BailRecord, CareerStats, Format, PushLatch};
use std::collections::BTreeMap;

/// A position step longer than this in one fixed tick is a teleport or a map switch, not travel.
const MAX_STEP: f32 = 5.0;
/// Bail distance steps at or under this are not counted (0x82775158, 0.005 m).
const BAIL_MIN_STEP: f32 = 0.005;
/// How often changed totals are written.
const SAVE_SECONDS: f32 = 30.0;

/// The hit rows by ped entity key (0x826C0730 compares the entity key's hash64); any other ped counts as a bum.
const HIT_TYPES: [(&str, usize); 13] = [
    ("jock01", 1),
    ("teenage_female01", 2),
    ("teenage_male01", 3),
    ("adult_male01", 4),
    ("adult_female01", 5),
    ("business_male01", 6),
    ("business_female01", 7),
    ("skater_male01", 8),
    ("skater_female01", 9),
    ("tourist_male01", 10),
    ("tourist_female01", 11),
    ("security_street01", 12),
    ("security_nsz01", 12),
];
/// security_nsz02 also counts as a security guard.
const SECURITY_NSZ02: &str = "security_nsz02";

/// The ped type names by type (`ID_LIVINGWORLD_<TYPE>`), bum first.
const TYPE_NAMES: [&str; 14] = [
    "BUM", "JOCK", "TEENGIRL", "TEENGUY", "ADULTMALE", "ADULTFEMALE", "BUSINESSMAN", "BUSINESSWOMAN", "MALESKATER",
    "FEMALESKATER", "MALETOURIST", "FEMALETOURIST", "SECURITYGUARD", "GRANNY",
];

pub(crate) fn hit_type(entity: &str) -> usize {
    if entity == SECURITY_NSZ02 {
        return 12;
    }
    HIT_TYPES.iter().find(|(k, _)| *k == entity).map_or(0, |(_, t)| *t)
}

#[derive(Resource, Default)]
pub(crate) struct CareerStatsHost {
    pub stats: CareerStats,
    /// The table's labels and enabled flags (menu tables `career_stats`; empty before setup exported them).
    pub labels: Vec<String>,
    pub enabled: Vec<bool>,
    path: Option<std::path::PathBuf>,
    last: Option<[f32; 3]>,
    bail_seq: Option<u32>,
    /// Bail distance not yet added (0x82775018 adds whole metres).
    bail_metres: f32,
    pushes: PushLatch,
    /// The bail running now (Hall of Meat record), added to the rows when it ends.
    bail: Option<BailRecord>,
    dirty: bool,
    since_save: f32,
}

impl CareerStatsHost {
    fn load(path: std::path::PathBuf, tables: Option<skate_data::menu_tables::MenuTables>) -> Self {
        let mut stats = CareerStats::default();
        if let Some(saved) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<BTreeMap<String, f32>>(&t).ok()) {
            for (k, v) in saved {
                if let Ok(i) = k.parse::<usize>() {
                    stats.record(i, v);
                }
            }
        }
        let rows = tables.map(|t| t.career_stats).unwrap_or_default();
        Self {
            stats,
            labels: rows.iter().map(|r| r.label.clone()).collect(),
            enabled: rows.iter().map(|r| r.enabled).collect(),
            path: Some(path),
            ..default()
        }
    }

    /// The totals as JSON (row index -> value, rows at 0 left out).
    fn snapshot(&self) -> Option<Vec<u8>> {
        let map: BTreeMap<String, f32> =
            self.stats.values.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, v)| (i.to_string(), *v)).collect();
        serde_json::to_vec_pretty(&map).ok()
    }

    fn save(&mut self) {
        if let (Some(path), Some(text)) = (self.path.clone(), self.snapshot()) {
            self.dirty = !write(&path, &text);
        }
    }

    /// Off the game thread: the snapshot is written on a short-lived thread (the exit save stays on this one).
    fn save_in_background(&mut self) {
        if let (Some(path), Some(text)) = (self.path.clone(), self.snapshot()) {
            self.dirty = false;
            if std::thread::Builder::new().name("career-stats-save".into()).spawn(move || write(&path, &text)).is_err() {
                self.dirty = true;
            }
        }
    }

    /// A scoring runtime event: trick counts (0x82773D98: success / failure, flip or late flip), move ends with their
    /// time or distance (0x82774230: invert, manual, powerslide, grind), body flips (0x82773C00 / 0x82775328).
    fn event(&mut self, event: crate::scoring_runtime::StatEvent) {
        use crate::scoring_runtime::StatEvent;
        use skate_core::scoring::catalog;
        match event {
            StatEvent::TrickEnd { id, success, announced, held, distance } => {
                let i = id as i16;
                if announced {
                    self.record(if success { row::SUCCESSFUL_TRICKS } else { row::UNSUCCESSFUL_TRICKS }, 1.0);
                    if success && catalog::category(i) == Some(catalog::FLIP) {
                        let late = catalog::base(i) == catalog::OLLIE && i != catalog::OLLIE;
                        self.record(if late { row::LATE_FLIPS } else { row::FLIP_TRICKS }, 1.0);
                    }
                }
                if success {
                    match catalog::category(i) {
                        Some(catalog::HANDPLANT) => self.record(row::INVERT_TIME, held),
                        Some(catalog::MANUAL) => self.record(row::MANUAL_DISTANCE, distance),
                        Some(catalog::POWERSLIDE) => self.record(row::POWERSLIDE_DISTANCE, distance),
                        Some(catalog::GRIND) => self.record(row::GRIND_DISTANCE, distance),
                        _ => {}
                    }
                }
            }
            StatEvent::BodyFlipAttempt => self.record(row::BODY_FLIP_ATTEMPTS, 1.0),
            StatEvent::BodyFlips(n) => self.record(row::BODY_FLIPS, n as f32),
        }
    }

    pub(crate) fn record(&mut self, index: usize, value: f32) {
        if value != 0.0 {
            self.stats.record(index, value);
            self.dirty = true;
        }
    }

    /// The Stats panel rows (label id, value text): the offline fetch's 16 at most, empty without the table.
    pub(crate) fn panel_rows(&self, language: &BTreeMap<String, String>, numbers: &crate::fe_language::NumberFormat, imperial: bool, rng: &mut dyn FnMut() -> u32) -> Vec<(String, String)> {
        let template = |id: &str| crate::fe_language::template(language, id).to_owned();
        let (metres, feet, seconds) = (template("ID_COMMON_METRIC_DISTANCE"), template("ID_COMMON_METRIC_NEW_FEET"), template("ID_COMMON_METRIC_TIME"));
        let int = |n: i64| numbers.int(n);
        // 0x8290E370: the whole part grouped like integers.
        let float = |x: f64, d: usize| {
            let text = numbers.float(x, d);
            let (whole, rest) = text.split_once(numbers.decimal).map_or((text.as_str(), None), |(w, r)| (w, Some(r)));
            let grouped = numbers.int(whole.parse::<i64>().unwrap_or(0));
            let grouped = if whole.starts_with('-') && !grouped.starts_with('-') { format!("-{grouped}") } else { grouped };
            rest.map_or(grouped.clone(), |r| format!("{grouped}{}{r}", numbers.decimal))
        };
        // NOT RETAIL YET: retail's locale currency format (0x8290DCB8); money rows are disabled in the table.
        let money = |x: f64| format!("${}", numbers.int(x as i64));
        let ped_type = |t: usize| TYPE_NAMES.get(t).map(|n| format!("ID_LIVINGWORLD_{n}")).unwrap_or_default();
        let format = Format { imperial, metres: &metres, feet: &feet, seconds: &seconds, int: &int, float: &float, money: &money, ped_type: &ped_type };
        self.stats
            .fetch(&self.enabled, rng)
            .into_iter()
            .filter_map(|i| Some((self.labels.get(i)?.clone(), self.stats.format(i, &format))))
            .collect()
    }
}

/// Atomic write (`.part`, then rename); false when it failed (logged).
fn write(path: &std::path::Path, text: &[u8]) -> bool {
    let part = path.with_extension("json.part");
    let written = path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&part, text).is_ok()
        && std::fs::rename(&part, path).is_ok();
    if !written {
        warn!("CAREER_STATS could not write {}", path.display());
    }
    written
}

fn load(mut commands: Commands, config: Res<crate::config::Config>) {
    let root = config.asset_root.parent().unwrap_or(&config.asset_root);
    let tables = std::fs::read_to_string(config.asset_root.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok());
    commands.insert_resource(CareerStatsHost::load(root.join("settings/career-stats.json"), tables));
}

/// One fixed step of the local player: time played, distance on and off the board, skitching, push starts, bails and
/// their distance (0x82775158, 0x82774DA0), and the scoring runtime's trick, move and body flip events.
fn track(
    host: Option<ResMut<CareerStatsHost>>,
    skater: Option<ResMut<crate::physics::SkaterRuntime>>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    time: Res<Time<Fixed>>,
    transition: Res<crate::map_transition::MapTransition>,
) {
    let (Some(mut host), Some(mut s)) = (host, skater) else { return };
    let events = std::mem::take(&mut s.scoring.stat_events);
    for event in events {
        host.event(event);
    }
    let bail_seq = s.scoring.bail_seq;
    if let Some(last) = host.bail_seq.filter(|l| bail_seq > *l) {
        host.record(row::WIPEOUTS, (bail_seq - last) as f32);
    }
    host.bail_seq = Some(bail_seq);
    let packet = &s.animation.packet;
    if let Some(i) = host.pushes.update(s.player_input.processed.flags_2468, packet.regular_stance, packet.riding_switch) {
        host.record(i, 1.0);
    }
    let dt = time.delta_secs();
    let root = s.animated_skeleton.roots.animation_to_world;
    let at = [root[3][0], root[3][1], root[3][2]];
    if transition.busy() {
        host.last = None;
        host.bail = None;
        return;
    }
    host.record(row::TIME_PLAYED, dt);
    let wiping = physics.as_ref().is_some_and(|p| p.board_wiping_out);
    if wiping {
        let skeleton = &s.player_input.physical.skeleton;
        let (over, strength) = (skeleton.over_599 != 0, skeleton.response_strength_580);
        let peds = s.ped_hits.iter().map(|h| h.0);
        let vehicles = s.vehicle_hits.iter().map(|h| h.0);
        let contacts = &s.collision_feedback.bones;
        let airborne = skate_core::hall_of_meat::airborne(&contacts.map(|b| b.groups[3]), &contacts.map(|b| b.normal[1]),
            &contacts.map(|b| b.specific_normal[1]));
        let bail = host.bail.get_or_insert_default();
        let frames = &s.animated_skeleton.board_frames;
        let speed = frames.com_velocity[..3].iter().map(|v| v * v).sum::<f32>().sqrt();
        let hips = s.skeleton.record.pose[23];
        let tick = skate_core::career_stats::BailTick {
            dt,
            over,
            frozen: s.player_input.physical.air.use_air_reckoning_452 != 0,
            response_strength: strength,
            airborne,
            centre: (frames.centre_of_mass[1], speed),
            hips: [hips[0], hips[1], hips[2]],
            stick: s.animation_input.extra.wipeout_gesture,
            poses: physics.as_ref().and_then(|p| p.settings.hall_of_meat_poses.as_ref()),
        };
        bail.tick(&tick, peds, vehicles);
        if let Some(tuning) = physics.as_ref().and_then(|p| p.settings.hall_of_meat.as_ref()) {
            let bones = &s.collision_feedback.bones;
            let impacts = skate_core::hall_of_meat::part_impacts(&bones.map(|b| b.force), &bones.map(|b| b.specific_force));
            bail.injuries.tick(tuning, &impacts, dt * BailRecord::TICK_RATE);
        }
    } else if let Some(bail) = host.bail.take() {
        for (i, v) in bail.rows() {
            host.record(i, v);
        }
    }
    let step = host.last.map(|l| ((at[0] - l[0]).powi(2) + (at[1] - l[1]).powi(2) + (at[2] - l[2]).powi(2)).sqrt()).filter(|d| d.is_finite() && *d <= MAX_STEP);
    host.last = Some(at);
    let Some(step) = step else { return };
    let on_board = s.player_input.physical.state.category_12 != 500;
    host.record(if on_board { row::DISTANCE } else { row::DISTANCE_OFFBOARD }, step);
    if s.skitch_state.car.is_some() {
        host.record(row::SKITCH_DISTANCE, step);
        host.record(row::SKITCH_TIME, dt);
    }
    if wiping && step > BAIL_MIN_STEP {
        host.bail_metres += step;
        let whole = host.bail_metres.floor();
        if whole >= 1.0 {
            host.bail_metres -= whole;
            host.record(row::BAIL_DISTANCE, whole);
        }
    }
}

/// The local player's ped hits by type (0x826C0730).
fn hits(
    host: Option<ResMut<CareerStatsHost>>,
    mut events: MessageReader<crate::living_world::peds::PedEvent>,
    peds: Query<&crate::living_world::peds::Pedestrian>,
) {
    let Some(mut host) = host else {
        events.clear();
        return;
    };
    for event in events.read() {
        if let crate::living_world::peds::PedEvent::Hit { id, toucher, .. } = event {
            if *toucher != crate::living_world::peds::PLAYER_TARGET_BASE {
                continue;
            }
            let entity = peds.iter().find(|p| p.id == *id).map(|p| p.entity.as_str()).unwrap_or("");
            host.record(row::PED_HITS + hit_type(entity), 1.0);
        }
    }
}

fn save(host: Option<ResMut<CareerStatsHost>>, time: Res<Time<Real>>, mut exit: MessageReader<AppExit>) {
    let Some(mut host) = host else { return };
    host.since_save += time.delta_secs();
    let exiting = exit.read().count() > 0;
    if host.dirty && exiting {
        host.save();
    } else if host.dirty && host.since_save >= SAVE_SECONDS {
        host.since_save = 0.0;
        host.save_in_background();
    }
}

pub(crate) struct CareerStatsPlugin;

impl Plugin for CareerStatsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load)
            .add_systems(FixedUpdate, track.after(crate::app::SimulationSet::Physics))
            .add_systems(Update, hits)
            .add_systems(Last, save);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_keys_pick_the_hit_row_and_others_count_as_bums() {
        assert_eq!(hit_type("jock01"), 1);
        assert_eq!(hit_type("security_nsz02"), 12);
        assert_eq!(hit_type("jock02"), 0);
        assert_eq!(hit_type(""), 0);
    }

    /// Trick ends count announced tricks, flips and late flips apart, and add a move's time or distance only when it
    /// completed (0x82773D98, 0x82774230).
    #[test]
    fn scoring_events_feed_the_trick_and_move_rows() {
        use crate::scoring_runtime::StatEvent;
        let mut host = CareerStatsHost::default();
        let end = |id: usize, success: bool, held: f32, distance: f32| StatEvent::TrickEnd { id, success, announced: true, held, distance };
        host.event(end(92, true, 0.0, 0.0)); // a flip
        host.event(end(100, true, 0.0, 0.0)); // a late flip
        host.event(end(2, true, 0.0, 4.5)); // nose manual
        host.event(end(0, false, 0.0, 9.0)); // a failed powerslide: no distance
        host.event(StatEvent::TrickEnd { id: 7, success: true, announced: false, held: 0.0, distance: 3.0 }); // grind
        host.event(StatEvent::BodyFlipAttempt);
        host.event(StatEvent::BodyFlips(2));
        let v = |r: usize| host.stats.value(r);
        assert_eq!((v(row::SUCCESSFUL_TRICKS), v(row::UNSUCCESSFUL_TRICKS)), (3.0, 1.0));
        assert_eq!((v(row::FLIP_TRICKS), v(row::LATE_FLIPS)), (1.0, 1.0));
        assert_eq!((v(row::MANUAL_DISTANCE), v(row::POWERSLIDE_DISTANCE), v(row::GRIND_DISTANCE)), (4.5, 0.0, 3.0));
        assert_eq!((v(row::BODY_FLIP_ATTEMPTS), v(row::BODY_FLIPS)), (1.0, 2.0));
    }

    #[test]
    fn totals_survive_a_save_and_load() {
        let dir = std::env::temp_dir().join(format!("career-stats-{}", std::process::id()));
        let path = dir.join("career-stats.json");
        let mut host = CareerStatsHost::load(path.clone(), None);
        host.record(row::DISTANCE, 12.5);
        host.record(row::PED_HITS + 1, 1.0);
        host.save();
        let again = CareerStatsHost::load(path, None);
        assert_eq!((again.stats.value(row::DISTANCE), again.stats.value(row::PED_HITS + 1)), (12.5, 1.0));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn panel_rows_need_the_table_and_format_by_kind() {
        let mut host = CareerStatsHost::default();
        host.record(row::DISTANCE, 1234.5);
        let language = BTreeMap::from([("ID_COMMON_METRIC_DISTANCE".to_owned(), "m".to_owned())]);
        let numbers = crate::fe_language::NumberFormat::default();
        assert!(host.panel_rows(&language, &numbers, false, &mut || 0).is_empty(), "no table: no Stats panel");
        host.labels = (0..skate_core::career_stats::ROWS).map(|i| format!("ID_RECORD_{i}")).collect();
        host.enabled = vec![true; skate_core::career_stats::ROWS];
        assert_eq!(host.panel_rows(&language, &numbers, false, &mut || 0), [("ID_RECORD_7".to_owned(), "1,234.0 m".to_owned())]);
    }
}
