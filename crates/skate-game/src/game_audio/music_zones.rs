//! The music zones' music: what the Music Player's Interactive Mode plays (docs/hails-additions/11-audio.md#music-zones;
//! research .local/research/music-zones-dj.md). The winning zone (`emitters::music_zones`) names a playlist: a licensed
//! location playlist (eChannelMapPreset 4) plays its soundtrack songs in order at once; a DJ playlist (3) starts a
//! world music piece once the skater has been fast inside the zone for a few checks, the piece runs for its play time
//! and the score multiplier sets its intensity (the graph's branch control). Retail: chooser 8249C9E8,
//! UpdateEnvironmentState 8249D070, the DJ control 8249D578. Every value comes from setup data (`aud_playlists`,
//! `aud_interactive`) and a content overlay can change it.
use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::Deserialize;

use super::emitters::MusicZone;
use super::music::{Music, MusicRequest, Track};

/// The world music graph and its DJ variable and events (id24 of retail's 0x15EEFA7 start, 0x15B4DEF end,
/// 0x162EA22 the x3 stinger).
const WORLD_SET: &str = "world";
const DJ_VAR: &str = "dj";
const DJ_START: u32 = 0x5E_EFA7;
const DJ_END: u32 = 0x5B_4DEF;
const DJ_STINGER: u32 = 0x62_EA22;
/// The licensed location songs play on the soundtrack graph: `ipod` = k, event 0x416D750.
const IPOD_SET: &str = "ipod";
const IPOD_VAR: &str = "ipod";
const IPOD_PLAY: u32 = 0x16_D750;
/// The race music on game.mpf (research music-challenges.md): start race 0x3BE42D picks one of the 8 race pieces
/// (sections 5..12) and sets `raceoverlay` to -1; Win posts 0xC17631, Lose 0x88B48E (UpdateRaceState 8249E680
/// sub-states 6 / 7); after the end the music holds 250 console frames (`default.59BE3547112AAC36`) and the graph's
/// end before free roam (here the zones) returns.
const GAME_SET: &str = "game";
const RACE_VAR: &str = "raceoverlay";
const RACE_START: u32 = 0x3B_E42D;
const RACE_WIN: u32 = 0xC1_7631;
const RACE_LOSE: u32 = 0x88_B48E;
const RACE_END_FRAMES: f32 = 250.0;
/// Retail's frame clock for its music counters (the 360's ~30 fps, memory console-cadence).
const CONSOLE_FPS: f32 = 30.0;
/// eChannelMapPreset values.
const PRESET_DJ: i32 = 3;
const PRESET_LICENSED: i32 = 4;
/// The skater's speed unit for the gate: m/s x 3.6 (822F8628).
const KMH: f32 = 3.6;

/// `aud_playlists` (audio manifest `music_zones.playlists`, key = record key in hex).
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Playlist {
    /// ePlayListMode (consumer not read yet).
    #[serde(default)]
    #[allow(dead_code)]
    pub mode: i32,
    #[serde(default)]
    pub preset: i32,
    /// [id, gain, extra] per licensed song.
    #[serde(default)]
    pub songs: Vec<(u32, f32, i32)>,
    /// [id, gain, play seconds, extra] per DJ piece.
    #[serde(default)]
    pub pieces: Vec<(u32, f32, f32, i32)>,
}

/// One `aud_interactive` record (the defaults and the score multiplier tiers).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub(crate) struct Interactive {
    pub speed_gate: Option<f32>,
    pub speed_checks: Option<i32>,
    pub cooldown_short: Option<f32>,
    pub cooldown: Option<f32>,
    pub override_a: Option<i32>,
    #[allow(dead_code)]
    pub override_b: Option<i32>,
    pub override_hold: Option<f32>,
    pub value_min: Option<i32>,
    pub value_max: Option<i32>,
    pub hold: Option<f32>,
    pub burst_threshold: Option<i32>,
    pub burst_min: Option<i32>,
    pub burst_max: Option<i32>,
    pub burst_time: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct MusicZonesData {
    #[serde(default)]
    pub playlists: BTreeMap<String, Playlist>,
    #[serde(default)]
    pub interactive: BTreeMap<String, Interactive>,
}

/// A race's music tuning (class 4ADAC090BCE2A930; UpdateRaceState's control 8249E9B8).
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct RaceTuning {
    #[serde(default)]
    pub delay: f32,
    #[serde(default)]
    pub start: f32,
    #[serde(default)]
    pub mode: i32,
    #[serde(default)]
    pub speed_scale: f32,
    #[serde(default)]
    pub rise: f32,
    #[serde(default)]
    pub fall: f32,
    #[serde(default)]
    pub curve_x: Vec<f32>,
    #[serde(default)]
    pub curve_y: Vec<f32>,
}

/// 82481E10: the 8-point curve, clamped piecewise-linear (a missing curve gives 0, as retail's zero fallback).
pub(crate) fn curve(xs: &[f32], ys: &[f32], x: f32) -> f32 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return 0.0;
    }
    if x < xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = (1..n).find(|&i| x < xs[i]).unwrap_or(n - 1);
    let dx = xs[i] - xs[i - 1];
    if dx <= 0.0 { ys[i] } else { ys[i - 1] + (ys[i] - ys[i - 1]) * (x - xs[i - 1]) / dx }
}

/// Retail's race control steps per call; NOT RETAIL YET: the call rate is assumed 60 per second (its delay counter steps
/// 1/60, 0x820849C8).
const RACE_CALLS_PER_SECOND: f32 = 60.0;

/// The song and piece id tables (menu-tables `ipod_ids`, `dj_ids`): an id's value is its position + 1.
#[derive(Clone, Debug, Default)]
pub(crate) struct EnumIds {
    pub ipod: Vec<u32>,
    pub dj: Vec<u32>,
}

impl EnumIds {
    fn value(list: &[u32], id: u32) -> Option<i32> {
        list.iter().position(|&v| v == id).map(|i| i as i32 + 1)
    }
}

/// The score multiplier tier record for a multiplier (8249D578: bits 13..15 of the score object).
fn tier(multiplier: f32) -> &'static str {
    if multiplier >= 3.0 {
        "times3"
    } else if multiplier >= 2.0 {
        "times2"
    } else if multiplier >= 1.5 {
        "times15"
    } else {
        "nomultiplier"
    }
}

/// What plays and the chooser's timers (the manager's environment block).
#[derive(Default, Debug)]
pub(crate) struct Environment {
    /// The playing playlist key and its preset.
    pub playing: Option<(u64, i32)>,
    /// Fast checks inside a DJ zone (`+0x67C`).
    counter: i32,
    /// The DJ piece's play time left (`+0x630`) and the cooldown before the next piece (`+0x634`).
    piece_left: f32,
    cooldown: f32,
    /// The x3 stinger: once per x3 phase (`+0x638`), after its cooldown (`+0x63C`).
    stinger_latch: bool,
    stinger_cooldown: f32,
    /// The DJ control: the tier, value and hold time, the burst window, the score at the tier's entry.
    control_tier: Option<&'static str>,
    control: i32,
    control_hold: f32,
    burst_left: f32,
    score_at_entry: f32,
    sent_control: Option<i32>,
    rng: u32,
    /// The race music runs (state 6): `Some(ended)`, with the end hold left once the race ended.
    race: Option<bool>,
    race_hold: f32,
    /// The race's tuning, its control (+0x458) and delay counter (+0x588), the control last sent.
    race_tuning: Option<RaceTuning>,
    race_control: f32,
    race_counter: f32,
}

impl Environment {
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        self.rng = self.rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        if hi <= lo { lo } else { lo + (self.rng >> 8) as i32 % (hi - lo + 1) }
    }

    /// A mission music trigger (eSk8MissionMusicTrigger) for a race (8248B258 -> 8249E498): 0 Countdown / 1 Start start
    /// the race music (Start only when nothing plays), 8 Win / 9 Lose post the ending, 12 StopChallengeMusic stops it
    /// (NOT RETAIL YET: 8249BB00 not read). HalfWay / ThreeQuarters / StraightAway (2..4) are not ported (their
    /// sub-states 3..5 are not read).
    pub(crate) fn race_trigger(&mut self, code: u8, nothing_playing: bool, overlay: Option<(std::path::PathBuf, f32)>, tuning: Option<RaceTuning>) -> Vec<MusicRequest> {
        let starting = matches!(code, 0 | 1) && (self.race.is_none() || code == 1 && nothing_playing && self.race == Some(false));
        let mut out = self.race_trigger_inner(code, nothing_playing, overlay);
        if starting && !out.is_empty() {
            self.race_tuning = None;
        }
        if let Some(tuning) = tuning.filter(|_| starting && !out.is_empty()) {
            // 8249E8E8: the tuning record, the starting control (an install without the record keeps the graph's own).
            self.race_control = tuning.start;
            self.race_counter = 0.0;
            self.sent_control = Some(tuning.start as i32);
            out.push(MusicRequest::Control(tuning.start as i32));
            self.race_tuning = Some(tuning);
        }
        out
    }

    fn race_trigger_inner(&mut self, code: u8, nothing_playing: bool, overlay: Option<(std::path::PathBuf, f32)>) -> Vec<MusicRequest> {
        let start = || MusicRequest::Queue {
            tracks: vec![Track::Graph { set: GAME_SET.into(), var: RACE_VAR.into(), value: -1, play: RACE_START }],
            once: true,
        };
        match code {
            0 if self.race.is_none() => {
                self.race = Some(false);
                vec![start()]
            }
            1 if self.race.is_none() || nothing_playing && self.race == Some(false) => {
                self.race = Some(false);
                vec![start()]
            }
            8 | 9 if self.race == Some(false) => {
                self.race = Some(true);
                self.race_hold = RACE_END_FRAMES / CONSOLE_FPS;
                // UpdateRaceState sub-states 6 / 7: the race overlay (8249F130), then the ending. NOT RETAIL YET: the
                // overlay is not synced to the beat (829559A8) and its extra gain factors (+0x440) are not applied.
                let mut out: Vec<MusicRequest> = overlay.map(|(file, gain)| MusicRequest::Overlay { file, gain }).into_iter().collect();
                out.push(MusicRequest::Post { event: if code == 8 { RACE_WIN } else { RACE_LOSE } });
                out
            }
            12 if self.race == Some(false) => {
                self.race = None;
                self.race_tuning = None;
                self.playing = None;
                vec![MusicRequest::Stop { owner: None }]
            }
            _ => Vec::new(),
        }
    }

    /// One frame: the requests for the music (empty when nothing changes).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn frame(&mut self, data: &MusicZonesData, ids: &EnumIds, zone: Option<&super::emitters::MusicZoneWinner>, speed: f32,
        unridden: bool, multiplier: f32, score: f32, piece_done: bool, dt: f32) -> Vec<MusicRequest> {
        let mut out = Vec::new();
        if let Some(ended) = self.race {
            // The race owns the music; its control follows the speed curve (8249E9B8, mode 0). NOT RETAIL YET: mode 1
            // (the control from the place) is not ported; no disc race uses it.
            if let Some(t) = self.race_tuning.clone().filter(|t| t.mode == 0) {
                if self.race_counter < t.delay {
                    self.race_counter += dt;
                } else {
                    let x = if t.speed_scale > 0.0 { (speed.abs() * KMH / t.speed_scale).clamp(0.0, 1.0) } else { 0.0 };
                    let target = curve(&t.curve_x, &t.curve_y, x) * 127.0;
                    let calls = dt * RACE_CALLS_PER_SECOND;
                    let delta = target - self.race_control;
                    self.race_control += if delta > 0.0 { delta.min(t.rise * calls) } else { delta.max(-t.fall * calls) };
                }
                if unridden && !ended {
                    self.race_control = 0.0;
                }
                let value = self.race_control as i32;
                if self.sent_control != Some(value) {
                    self.sent_control = Some(value);
                    out.push(MusicRequest::Control(value));
                }
            }
            if ended {
                self.race_hold -= dt;
                if self.race_hold <= 0.0 && piece_done {
                    self.race = None;
                    self.race_tuning = None;
                    self.playing = None;
                    self.sent_control = None;
                }
            }
            return out;
        }
        let default = data.interactive.get("default").copied().unwrap_or_default();
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.stinger_cooldown = (self.stinger_cooldown - dt).max(0.0);
        let dj_playing = matches!(self.playing, Some((_, PRESET_DJ))) && !piece_done;
        if let Some((key, PRESET_DJ)) = self.playing {
            if piece_done {
                self.playing = None;
            } else if self.piece_left > 0.0 {
                self.piece_left -= dt;
                if self.piece_left <= 0.0 {
                    // The piece's time is up: its end section plays out, then the cooldown (NOT RETAIL YET: which of
                    // the two cooldown fields 824984A0 reads).
                    out.push(MusicRequest::Post { event: DJ_END });
                    self.cooldown = default.cooldown_short.unwrap_or(2.5);
                    let _ = key;
                }
            }
        }
        if let Some(w) = zone {
            let key = format!("{:016X}", w.playlist);
            if let Some(list) = data.playlists.get(&key) {
                match list.preset {
                    PRESET_LICENSED if self.playing.map(|p| p.0) != Some(w.playlist) => {
                        let tracks: Vec<Track> = list.songs.iter()
                            .filter_map(|s| EnumIds::value(&ids.ipod, s.0))
                            .map(|k| Track::Graph { set: IPOD_SET.into(), var: IPOD_VAR.into(), value: k, play: IPOD_PLAY })
                            .collect();
                        if !tracks.is_empty() {
                            out.push(MusicRequest::Queue { tracks, once: false });
                            self.playing = Some((w.playlist, PRESET_LICENSED));
                        }
                    }
                    PRESET_DJ if !dj_playing && self.cooldown <= 0.0 && w.distance <= 0.0 => {
                        let fast = !unridden && speed.abs() * KMH >= default.speed_gate.unwrap_or(30.0);
                        self.counter = if fast { self.counter + 1 } else { 0 };
                        if self.counter > default.speed_checks.unwrap_or(5) && !list.pieces.is_empty() {
                            self.counter = 0;
                            let pick = self.random(0, list.pieces.len() as i32 - 1) as usize;
                            let piece = list.pieces[pick];
                            if let Some(value) = EnumIds::value(&ids.dj, piece.0) {
                                out.push(MusicRequest::Queue { tracks: vec![Track::Graph { set: WORLD_SET.into(), var: DJ_VAR.into(), value, play: DJ_START }], once: true });
                                self.playing = Some((w.playlist, PRESET_DJ));
                                self.piece_left = piece.2;
                                self.control_tier = None;
                                self.sent_control = None;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if matches!(self.playing, Some((_, PRESET_DJ))) {
            self.control(data, &default, unridden, multiplier, score, dt);
            if self.sent_control != Some(self.control) {
                self.sent_control = Some(self.control);
                out.push(MusicRequest::Control(self.control));
            }
            if multiplier >= 3.0 {
                if !self.stinger_latch && self.stinger_cooldown <= 0.0 {
                    self.stinger_latch = true;
                    out.push(MusicRequest::Post { event: DJ_STINGER });
                    self.stinger_cooldown = data.interactive.get("times3").and_then(|t| t.cooldown).unwrap_or(10.0);
                }
            } else {
                self.stinger_latch = false;
            }
        }
        out
    }

    /// 8249D578: the tier's value (random in its range) held for its hold time; a score burst switches to the burst
    /// value; off the board the override value. NOT RETAIL YET: the two skater flags are both read as "unridden".
    fn control(&mut self, data: &MusicZonesData, default: &Interactive, unridden: bool, multiplier: f32, score: f32, dt: f32) {
        if unridden {
            self.control = default.override_a.unwrap_or(21);
            self.control_hold = default.override_hold.unwrap_or(3.0);
            self.control_tier = None;
            return;
        }
        let name = tier(multiplier);
        let record = data.interactive.get(name).copied().unwrap_or_default();
        self.control_hold -= dt;
        self.burst_left -= dt;
        if self.control_tier != Some(name) || self.control_hold <= 0.0 && self.burst_left <= 0.0 {
            if self.control_tier != Some(name) {
                self.score_at_entry = score;
            }
            self.control_tier = Some(name);
            self.control = self.random(record.value_min.unwrap_or(0), record.value_max.unwrap_or(0));
            self.control_hold = record.hold.unwrap_or(3.0);
        }
        let threshold = record.burst_threshold.unwrap_or(-1);
        if threshold > 0 && self.burst_left <= 0.0 && score - self.score_at_entry >= threshold as f32 {
            self.control = self.random(record.burst_min.unwrap_or(0), record.burst_max.unwrap_or(0));
            self.burst_left = record.burst_time.unwrap_or(0.0);
            self.score_at_entry = score;
        }
    }
}

/// Loads the id tables once from the install's menu tables.
pub(crate) fn enum_ids(asset_root: &std::path::Path) -> EnumIds {
    std::fs::read_to_string(asset_root.join(crate::retail_menus::TABLES))
        .ok()
        .and_then(|t| skate_data::menu_tables::MenuTables::from_json(&t).ok())
        .map(|t| EnumIds { ipod: t.ipod_ids, dj: t.dj_ids })
        .unwrap_or_default()
}

#[derive(Default)]
pub(crate) struct EnvironmentHost {
    env: Environment,
    ids: Option<EnumIds>,
    active: bool,
}

/// Per frame in the Interactive Mode (the Music Player's mode 1): the zones choose the music. Nothing while a mod owns
/// the queue or another mode plays the soundtrack.
pub(crate) fn environment(
    mut host: Local<EnvironmentHost>,
    zone: Res<MusicZone>,
    library: Option<Res<super::Library>>,
    mut music: ResMut<Music>,
    mut races: Option<ResMut<crate::race_host::Races>>,
    cues: Res<super::skate_events::Cues>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    config: Res<crate::config::Config>,
    time: Res<Time>,
) {
    let Some(library) = library else { return };
    let interactive = music.soundtrack_off && music.owner.is_none();
    let triggers = races.as_mut().map(|r| std::mem::take(&mut r.music)).unwrap_or_default();
    if !interactive {
        if host.active {
            host.active = false;
            host.env = Environment::default();
        }
        return;
    }
    if !host.active {
        host.active = true;
        host.env.rng = time.elapsed().as_nanos() as u32 | 1;
    }
    let ids = host.ids.get_or_insert_with(|| enum_ids(&config.asset_root)).clone();
    let (multiplier, score) = skater.as_ref().map_or((1.0, 0.0), |s| (s.scoring.session.combo.multiplier, s.scoring.session.combo.timer.points));
    let done = music.queue_done();
    for code in triggers {
        // NOT RETAIL YET: the slot is the race piece's `raceoverlay` (0..7); every slot names the same sting on the disc.
        let requests = host.env.race_trigger(code, done, library.race_overlay(0), library.race_tuning(None));
        for r in &requests {
            info!("AUDIO_MUSIC race trigger {code}: {r:?}");
        }
        music.requests.extend(requests);
    }
    let requests = host.env.frame(library.music_zones(), &ids, zone.winner.as_ref(), cues.riding.speed, cues.riding.unridden, multiplier, score, done, time.delta_secs());
    for r in &requests {
        if !matches!(r, MusicRequest::Control(_)) {
            info!("AUDIO_MUSIC zone {:?}: {r:?}", zone.winner.as_ref().map(|w| format!("{:016X}", w.playlist)));
        }
    }
    music.requests.extend(requests);
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::emitters::MusicZoneWinner;

    fn data() -> MusicZonesData {
        let mut d = MusicZonesData::default();
        d.playlists.insert("00000000000000AA".into(), Playlist { mode: 1, preset: 3, songs: vec![], pieces: vec![(0xD1, 1.8, 2.0, 0)] });
        d.playlists.insert("00000000000000BB".into(), Playlist { mode: 1, preset: 4, songs: vec![(0x11, 1.0, 0), (0x99, 1.0, 0), (0x22, 1.0, 0)], pieces: vec![] });
        d.interactive.insert("default".into(), Interactive { speed_gate: Some(30.0), speed_checks: Some(5), cooldown_short: Some(2.5), override_a: Some(21), ..Default::default() });
        d.interactive.insert("nomultiplier".into(), Interactive { value_min: Some(15), value_max: Some(15), hold: Some(3.0), ..Default::default() });
        d.interactive.insert("times2".into(), Interactive { value_min: Some(112), value_max: Some(112), hold: Some(10.0), ..Default::default() });
        d
    }

    fn ids() -> EnumIds {
        EnumIds { ipod: vec![0x11, 0x22], dj: vec![0xC0, 0xD1] }
    }

    fn zone(playlist: u64) -> MusicZoneWinner {
        MusicZoneWinner { record: 1, attribute: 2, playlist, distance: 0.0, level: 0.17 }
    }

    #[test]
    fn a_licensed_zone_queues_its_known_songs_at_once() {
        let mut e = Environment::default();
        let out = e.frame(&data(), &ids(), Some(&zone(0xBB)), 0.0, false, 1.0, 0.0, false, 0.1);
        assert_eq!(out.len(), 1);
        let MusicRequest::Queue { tracks, once } = &out[0] else { panic!("{out:?}") };
        assert!(!once);
        assert_eq!(tracks.iter().map(|t| match t { Track::Graph { value, .. } => *value, _ => 0 }).collect::<Vec<_>>(), [1, 2]);
        assert!(e.frame(&data(), &ids(), Some(&zone(0xBB)), 0.0, false, 1.0, 0.0, false, 0.1).is_empty(), "the same zone keeps playing");
    }

    #[test]
    fn a_dj_piece_needs_speed_for_more_than_the_checks_then_ends_by_its_time() {
        let (d, ids) = (data(), ids());
        let mut e = Environment::default();
        let fast = 10.0;
        for _ in 0..5 {
            assert!(e.frame(&d, &ids, Some(&zone(0xAA)), fast, false, 1.0, 0.0, false, 0.1).is_empty());
        }
        assert!(e.frame(&d, &ids, Some(&zone(0xAA)), 2.0, false, 1.0, 0.0, false, 0.1).is_empty(), "slow resets the count");
        let mut started = Vec::new();
        for _ in 0..6 {
            started = e.frame(&d, &ids, Some(&zone(0xAA)), fast, false, 1.0, 0.0, false, 0.1);
        }
        assert!(matches!(&started[0], MusicRequest::Queue { tracks, once: true } if tracks[0] == Track::Graph { set: "world".into(), var: "dj".into(), value: 2, play: DJ_START }), "{started:?}");
        assert_eq!(started[1], MusicRequest::Control(15));
        let doubled = e.frame(&d, &ids, None, fast, false, 2.0, 0.0, false, 0.1);
        assert_eq!(doubled, [MusicRequest::Control(112)], "leaving the zone does not cut the piece; x2 lifts the control");
        let mut ended = Vec::new();
        for _ in 0..20 {
            ended.extend(e.frame(&d, &ids, None, fast, false, 2.0, 0.0, false, 0.1));
        }
        assert!(ended.contains(&MusicRequest::Post { event: DJ_END }), "{ended:?}");
    }

    #[test]
    fn a_race_owns_the_music_from_the_countdown_to_its_end_hold() {
        let (d, ids) = (data(), ids());
        let mut e = Environment::default();
        let start = e.race_trigger(0, true, None, None);
        assert!(matches!(&start[0], MusicRequest::Queue { tracks, once: true } if tracks[0] == Track::Graph { set: "game".into(), var: "raceoverlay".into(), value: -1, play: RACE_START }));
        assert!(e.race_trigger(1, false, None, None).is_empty(), "Start while the countdown's piece plays changes nothing");
        assert!(e.frame(&d, &ids, Some(&zone(0xBB)), 0.0, false, 1.0, 0.0, false, 0.1).is_empty(), "zones wait during the race");
        let cymbal = (std::path::PathBuf::from("music_overlays/Cymbal_Overlay.wav"), 1.2);
        assert_eq!(e.race_trigger(8, false, Some(cymbal.clone()), None), [MusicRequest::Overlay { file: cymbal.0, gain: 1.2 }, MusicRequest::Post { event: RACE_WIN }]);
        e.frame(&d, &ids, None, 0.0, false, 1.0, 0.0, true, 9.0);
        assert!(e.race.is_none(), "after 250 console frames and the graph's end the race lets go");
        assert_eq!(e.frame(&d, &ids, Some(&zone(0xBB)), 0.0, false, 1.0, 0.0, false, 0.1).len(), 1, "the zones choose again");
    }

    #[test]
    fn the_race_control_starts_at_the_tuning_then_follows_the_speed_curve_after_the_delay() {
        let tuning = RaceTuning { delay: 1.0, start: 77.0, mode: 0, speed_scale: 75.0, rise: 1.0, fall: 1.7,
            curve_x: vec![0.0, 1.0], curve_y: vec![0.0, 1.0] };
        let (d, ids) = (data(), ids());
        let mut e = Environment::default();
        let start = e.race_trigger(0, true, None, Some(tuning));
        assert_eq!(start.last(), Some(&MusicRequest::Control(77)));
        assert!(e.frame(&d, &ids, None, 20.0, false, 1.0, 0.0, false, 0.5).is_empty(), "the delay holds the control");
        e.frame(&d, &ids, None, 20.0, false, 1.0, 0.0, false, 0.6);
        // 72 km/h of 75 -> target 0.96 x 127 = 121.9; one 1/60 s call may rise 1.
        let out = e.frame(&d, &ids, None, 20.0, false, 1.0, 0.0, false, 1.0 / 60.0);
        assert_eq!(out, [MusicRequest::Control(78)]);
        assert_eq!(e.frame(&d, &ids, None, 20.0, true, 1.0, 0.0, false, 1.0 / 60.0), [MusicRequest::Control(0)], "a bail drops it");
    }

    #[test]
    fn the_curve_is_clamped_piecewise_linear() {
        let (xs, ys) = ([0.0, 0.5, 1.0], [0.0, 1.0, 0.5]);
        assert_eq!((curve(&xs, &ys, -1.0), curve(&xs, &ys, 0.25), curve(&xs, &ys, 0.75), curve(&xs, &ys, 2.0)), (0.0, 0.5, 0.75, 0.5));
        assert_eq!(curve(&[], &[], 0.5), 0.0);
    }

    #[test]
    fn off_the_board_the_control_drops_to_the_override() {
        let mut e = Environment { playing: Some((0xAA, PRESET_DJ)), piece_left: 100.0, ..Default::default() };
        assert_eq!(e.frame(&data(), &ids(), None, 0.0, true, 2.0, 0.0, false, 0.1), [MusicRequest::Control(21)]);
    }
}
