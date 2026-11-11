//! The in-game music host (docs/hails-additions/11-audio.md#music-banks): a queue of tracks played through the
//! runtime's music stream ([`skate_audio::music::player`]). A retail track plays through its set's PathFinder graph
//! (research b1016: track k = `ipod` = k, then the play event); a player's own track (a music bank's track table,
//! from a playlist) plays its segments in order. The next segment decodes on a worker thread, a few seconds ahead.
//! Default queue: the disc soundtrack (every `ipod` track, shuffled). NOT RETAIL YET: which retail mode free roam plays
//! (soundtrack or the interactive DJ music) and the stream gain (the Music option volume directly; the MixMap output
//! that scales retail's music is open). Test aids: `SKATE_MUSIC=0` (off), `SKATE_MUSIC_PLAYLIST=<playlist.json>`.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use skate_audio::mixer::Pcm;
use skate_audio::music::bank::Bank;
use skate_audio::music::pathfinder::format::Graph;
use skate_audio::music::pathfinder::stream::Stream;

use super::native::Native;

/// Seconds of music the host keeps decoded ahead of the play position.
const AHEAD: f32 = 6.0;
/// The music stream's base level under the Music option (user 2026-11-10: full scale was too loud against the game).
/// NOT RETAIL YET: retail's level comes from a MixMap output not identified (b1015); a mod may change it.
pub(crate) const MUSIC_LEVEL: f32 = 0.35;
/// The soundtrack set and its play event (the id the game posts for a track, b1016 §3; data-driven names).
pub(crate) const SOUNDTRACK_SET: &str = "ipod";
const SOUNDTRACK_VAR: &str = "ipod";
const SOUNDTRACK_PLAY: u32 = 0x16D750;

/// One track of the queue.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Track {
    /// A retail graph track: entry `track` (1-based) of the set's soundtrack section.
    Retail { set: String, track: u32 },
    /// A player's own track: `count` segments from `first` in a music bank.
    Bank { bank: PathBuf, first: u32, count: u32, name: String, artist: String },
}

/// What plays now.
enum Playing {
    Retail { set: String, stream: Box<Stream> },
    Bank { bank: PathBuf, next: u32, end: u32 },
}

/// The queue and the playing track; pure (graphs given), so tests drive it without files.
pub(crate) struct Sequencer {
    pub tracks: Vec<Track>,
    pub shuffle: bool,
    order: Vec<usize>,
    at: usize,
    playing: Option<Playing>,
    rng: u32,
    /// The track that started last (logs, sdk).
    pub current: Option<Track>,
}

impl Sequencer {
    pub(crate) fn new(tracks: Vec<Track>, shuffle: bool, seed: u32) -> Self {
        let mut s = Self { order: (0..tracks.len()).collect(), tracks, shuffle, at: 0, playing: None, rng: seed.max(1), current: None };
        s.reorder();
        s
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn reorder(&mut self) {
        self.order = (0..self.tracks.len()).collect();
        if self.shuffle {
            for i in (1..self.order.len()).rev() {
                let j = self.random() as usize % (i + 1);
                self.order.swap(i, j);
            }
        }
        self.at = 0;
    }

    fn start(&mut self, track: Track, graphs: &HashMap<String, Arc<Graph>>) -> bool {
        let playing = match &track {
            Track::Retail { set, track: k } => {
                let Some(g) = graphs.get(set) else { return false };
                let mut stream = Stream::new(g, self.random());
                if !stream.set_var(g, SOUNDTRACK_VAR, *k as i32) || !stream.post(g, SOUNDTRACK_PLAY) {
                    return false;
                }
                Playing::Retail { set: set.clone(), stream: Box::new(stream) }
            }
            Track::Bank { bank, first, count, .. } => Playing::Bank { bank: bank.clone(), next: *first, end: first + count },
        };
        self.playing = Some(playing);
        self.current = Some(track);
        true
    }

    /// The next segment to queue: (set name or bank file, 0-based segment). Moves on to the next track when one ends;
    /// `None` when the queue is empty.
    pub(crate) fn next(&mut self, graphs: &HashMap<String, Arc<Graph>>) -> Option<(Source, u32)> {
        for _ in 0..=self.tracks.len() * 2 + 1 {
            match self.playing.as_mut() {
                Some(Playing::Retail { set, stream }) => {
                    if let Some(g) = graphs.get(set.as_str()) {
                        if let Some(seg) = stream.next_segment(g) {
                            return Some((Source::Set(set.clone()), seg));
                        }
                    }
                }
                Some(Playing::Bank { bank, next, end }) => {
                    if *next < *end {
                        *next += 1;
                        return Some((Source::File(bank.clone()), *next - 1));
                    }
                }
                None => {}
            }
            self.playing = None;
            if self.tracks.is_empty() {
                return None;
            }
            if self.at >= self.order.len() {
                self.reorder();
            }
            let track = self.tracks[self.order[self.at]].clone();
            self.at += 1;
            self.start(track, graphs);
        }
        None
    }

    /// Ends the playing track: the next call starts the following one.
    pub(crate) fn skip(&mut self) {
        self.playing = None;
    }

    /// Puts back the last `n` started tracks (decoded ahead but not heard yet), so they start again.
    pub(crate) fn back(&mut self, n: usize) {
        self.at = self.at.saturating_sub(n);
    }

    /// Adds tracks after the queue's last.
    pub(crate) fn append(&mut self, tracks: Vec<Track>) {
        let start = self.tracks.len();
        self.tracks.extend(tracks);
        self.order.extend(start..self.tracks.len());
    }

    /// Plays `track` now (the queue goes on after it).
    pub(crate) fn play_now(&mut self, track: Track, graphs: &HashMap<String, Arc<Graph>>) -> bool {
        self.start(track, graphs)
    }

    /// Sets a variable of the playing graph stream (by name); false without one.
    pub(crate) fn set_var(&mut self, graphs: &HashMap<String, Arc<Graph>>, name: &str, value: i32) -> bool {
        match self.playing.as_mut() {
            Some(Playing::Retail { set, stream }) => graphs.get(set.as_str()).is_some_and(|g| stream.set_var(g, name, value)),
            _ => false,
        }
    }

    /// Posts an event (id24) to the playing graph stream; false without one or without that event.
    pub(crate) fn post(&mut self, graphs: &HashMap<String, Arc<Graph>>, event: u32) -> bool {
        match self.playing.as_mut() {
            Some(Playing::Retail { set, stream }) => graphs.get(set.as_str()).is_some_and(|g| stream.post(g, event)),
            _ => false,
        }
    }
}

/// A change to the music: the one way in for engine systems and mods (`sdk.music`). `owner` is a mod id: a mod's
/// queue and volume end with the mod (`ClearOwner`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MusicRequest {
    /// A playlist file (its banks relative to it), replacing the queue or added to it.
    Playlist { owner: Option<String>, path: PathBuf, queue: bool },
    /// The disc soundtrack: one track now (1-based) then the rest shuffled, or all shuffled.
    Soundtrack { owner: Option<String>, track: Option<u32> },
    Stop { owner: Option<String> },
    Skip,
    /// 0..2 on the engine's level; None: the engine's.
    Volume { owner: Option<String>, level: Option<f32> },
    Var { name: String, value: i32 },
    Post { event: u32 },
    ClearOwner(String),
}

/// Where a segment comes from: a retail set's bank, or a player's bank file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Source {
    Set(String),
    File(PathBuf),
}

type Decoded = Arc<Mutex<Option<Result<Pcm, String>>>>;

#[derive(Resource, Default)]
pub(crate) struct Music {
    /// Retail set -> its bank file.
    sets: HashMap<String, PathBuf>,
    graphs: HashMap<String, Arc<Graph>>,
    /// Bank file -> index (opened on first use).
    banks: HashMap<PathBuf, Arc<Bank>>,
    loaded: bool,
    pub(crate) sequencer: Option<Sequencer>,
    decoding: Option<(Source, u32, Decoded)>,
    /// Segments queued into the stream (diagnostics, tests).
    pub(crate) queued: u32,
    /// The stream id of the next queued segment, and the first segment of each track (announced when it plays).
    next_id: u32,
    decoding_track: Option<Track>,
    marks: HashMap<u32, Track>,
    /// Requests to apply on the next frame (engine systems and mods).
    pub(crate) requests: Vec<MusicRequest>,
    /// The mod that set the queue (None: the engine's default).
    pub(crate) owner: Option<String>,
    /// A volume a mod set: (mod, 0..2).
    pub(crate) volume: Option<(String, f32)>,
    /// The track heard now.
    pub(crate) playing: Option<MusicTrackStarted>,
    /// `music_track_started` events for the mods (newest 64).
    pub(crate) events: Vec<serde_json::Value>,
    seed: u32,
}

/// A track became audible (its first segment started playing): the caption, mods' `sdk.music` events.
#[derive(Message, Clone, Debug)]
pub(crate) struct MusicTrackStarted {
    pub title: String,
    pub artist: String,
    /// A player's own bank (the caption shows only these).
    pub custom: bool,
}

/// A playlist's tracks resolved against its banks (missing ones skipped, logged).
pub(crate) fn playlist_tracks(path: &Path) -> Result<(Vec<Track>, bool), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let list: crate::music_playlist::Playlist = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tracks = Vec::new();
    for entry in &list.tracks {
        if let Some(k) = entry.soundtrack {
            tracks.push(Track::Retail { set: SOUNDTRACK_SET.into(), track: k });
            continue;
        }
        let file = path.parent().unwrap_or(Path::new("")).join(&entry.bank);
        let bank = match crate::music_playlist::read_bank(&file) {
            Ok(b) => b,
            Err(e) => {
                warn!("AUDIO_MUSIC playlist {}: {e}", list.name);
                continue;
            }
        };
        match crate::music_playlist::resolve(path, entry, &bank).and_then(|(f, i)| bank.tracks.get(i).map(|t| (f, t.clone()))) {
            Some((f, t)) => tracks.push(Track::Bank { bank: f, first: t.first, count: t.count, name: t.name, artist: t.artist }),
            None => warn!("AUDIO_MUSIC playlist {}: track {:?} not in {}", list.name, entry.name, file.display()),
        }
    }
    Ok((tracks, list.shuffle))
}

impl Music {
    fn load(&mut self, library: &super::Library) {
        self.loaded = true;
        if std::env::var("SKATE_MUSIC").is_ok_and(|v| v == "0") {
            info!("AUDIO_MUSIC off (SKATE_MUSIC=0)");
            return;
        }
        for set in library.music_sets() {
            if let Some(bank) = library.music_bank(&set) {
                self.sets.insert(set.clone(), bank);
            }
            if let Some(path) = library.music_graph(&set) {
                match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| Graph::parse(&b)) {
                    Ok(g) => {
                        self.graphs.insert(set.clone(), Arc::new(g));
                    }
                    Err(e) => warn!("AUDIO_MUSIC graph {set}: {e}"),
                }
            }
        }
        self.seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u32);
        if let Some(path) = std::env::var_os("SKATE_MUSIC_PLAYLIST") {
            self.requests.insert(0, MusicRequest::Playlist { owner: None, path: path.into(), queue: false });
            return;
        }
        self.sequencer = self.soundtrack();
    }

    /// The engine's default queue: every soundtrack track, shuffled (None without the soundtrack graph).
    fn soundtrack(&mut self) -> Option<Sequencer> {
        let g = self.graphs.get(SOUNDTRACK_SET)?;
        let count = g.entries(1).len() as u32;
        let tracks = (1..=count).map(|k| Track::Retail { set: SOUNDTRACK_SET.into(), track: k }).collect();
        self.seed = self.seed.wrapping_mul(747_796_405).wrapping_add(1);
        info!("AUDIO_MUSIC soundtrack: {count} tracks, shuffled");
        Some(Sequencer::new(tracks, true, self.seed))
    }

    /// Applies the queued requests; true when the stream must start over (a new queue, a skip, a stop).
    fn apply(&mut self) -> bool {
        let mut restart = false;
        for request in std::mem::take(&mut self.requests) {
            match request {
                MusicRequest::Playlist { owner, path, queue } => match playlist_tracks(&path) {
                    Ok((tracks, shuffle)) => {
                        info!("AUDIO_MUSIC playlist {}: {} tracks{}", path.display(), tracks.len(), if queue { " queued" } else { "" });
                        match (queue, self.sequencer.as_mut()) {
                            (true, Some(seq)) => seq.append(tracks),
                            _ => {
                                self.seed = self.seed.wrapping_add(1);
                                self.sequencer = Some(Sequencer::new(tracks, shuffle, self.seed));
                                restart = true;
                            }
                        }
                        self.owner = owner;
                    }
                    Err(e) => warn!("AUDIO_MUSIC {e}"),
                },
                MusicRequest::Soundtrack { owner, track } => {
                    let graphs = self.graphs.clone();
                    self.sequencer = self.soundtrack();
                    if let (Some(k), Some(seq)) = (track, self.sequencer.as_mut()) {
                        seq.play_now(Track::Retail { set: SOUNDTRACK_SET.into(), track: k }, &graphs);
                    }
                    self.owner = owner;
                    restart = true;
                }
                MusicRequest::Stop { owner } => {
                    self.sequencer = None;
                    self.owner = owner;
                    restart = true;
                }
                MusicRequest::Skip => {
                    // Tracks already decoded ahead are not heard yet: the skip goes to the one after the heard track.
                    let unheard = self.marks.len() + usize::from(self.decoding_track.is_some());
                    let unheard = if self.playing.is_none() { unheard.saturating_sub(1) } else { unheard };
                    if let Some(seq) = self.sequencer.as_mut() {
                        seq.back(unheard);
                        seq.skip();
                    }
                    restart = true;
                }
                MusicRequest::Volume { owner, level } => {
                    self.volume = owner.zip(level);
                }
                MusicRequest::Var { name, value } => {
                    let graphs = self.graphs.clone();
                    if !self.sequencer.as_mut().is_some_and(|s| s.set_var(&graphs, &name, value)) {
                        warn!("AUDIO_MUSIC no playing graph has variable {name:?}");
                    }
                }
                MusicRequest::Post { event } => {
                    let graphs = self.graphs.clone();
                    if !self.sequencer.as_mut().is_some_and(|s| s.post(&graphs, event)) {
                        warn!("AUDIO_MUSIC no playing graph has event {event:#x}");
                    }
                }
                MusicRequest::ClearOwner(id) => {
                    if self.owner.as_deref() == Some(id.as_str()) {
                        self.owner = None;
                        self.sequencer = self.soundtrack();
                        restart = true;
                    }
                    if self.volume.as_ref().is_some_and(|(o, _)| *o == id) {
                        self.volume = None;
                    }
                }
            }
        }
        if restart {
            self.decoding = None;
            self.decoding_track = None;
            self.marks.clear();
            self.playing = None;
        }
        restart
    }

    fn bank(&mut self, source: &Source) -> Option<(PathBuf, Arc<Bank>)> {
        let path = match source {
            Source::Set(s) => self.sets.get(s)?.clone(),
            Source::File(p) => p.clone(),
        };
        if let Some(b) = self.banks.get(&path) {
            return Some((path, b.clone()));
        }
        match crate::music_playlist::read_bank(&path) {
            Ok(b) => {
                let b = Arc::new(b);
                self.banks.insert(path.clone(), b.clone());
                Some((path, b))
            }
            Err(e) => {
                warn!("AUDIO_MUSIC {e}");
                None
            }
        }
    }

    fn start_decode(&mut self) {
        let graphs = self.graphs.clone();
        let before = self.sequencer.as_ref().and_then(|s| s.current.clone());
        let Some((source, index)) = self.sequencer.as_mut().and_then(|s| s.next(&graphs)) else { return };
        let now = self.sequencer.as_ref().and_then(|s| s.current.clone());
        // A track that just started: its first segment is this one.
        self.decoding_track = if now != before { now } else { None };
        let Some((path, bank)) = self.bank(&source) else { return };
        let cell: Decoded = Arc::new(Mutex::new(None));
        let result = cell.clone();
        let spawned = std::thread::Builder::new().name("music-decode".into()).spawn(move || {
            let pcm = std::fs::File::open(&path).map_err(|e| e.to_string()).and_then(|mut f| bank.decode(&mut f, index as usize));
            if let Ok(mut r) = result.lock() {
                *r = Some(pcm);
            }
        });
        if spawned.is_ok() {
            self.decoding = Some((source, index, cell));
        }
    }
}

pub(super) fn frame(mut music: ResMut<Music>, native: Option<Res<Native>>, library: Option<Res<super::Library>>,
                    settings: Option<Res<super::AudioSettings>>, mut started: MessageWriter<MusicTrackStarted>) {
    let (Some(native), Some(library)) = (native, library) else { return };
    if !music.loaded {
        music.load(&library);
    }
    let restart = music.apply();
    let ready = music.decoding.as_ref().and_then(|(_, _, cell)| cell.lock().ok().and_then(|mut c| c.take()));
    let Ok(mut rt) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK) else { return };
    if restart {
        rt.music.clear();
    }
    let mod_volume = music.volume.as_ref().map_or(1.0, |v| v.1);
    rt.music.gain = MUSIC_LEVEL * mod_volume * settings.as_ref().map_or(1.0, |s| s.retail_volume(super::RetailVolume::Music));
    if let Some(result) = ready {
        let (source, index, _) = music.decoding.take().expect("ready implies decoding");
        let id = music.next_id;
        music.next_id = music.next_id.wrapping_add(1);
        match result.and_then(|pcm| rt.music.push(id, pcm)) {
            Ok(()) => {
                music.queued += 1;
                if let Some(track) = music.decoding_track.take() {
                    music.marks.insert(id, track);
                }
            }
            Err(e) => warn!("AUDIO_MUSIC {source:?} segment {index}: {e}"),
        }
    }
    for id in rt.music.take_started() {
        if let Some(track) = music.marks.remove(&id) {
            let (title, artist, custom) = match &track {
                Track::Bank { name, artist, .. } => (name.clone(), artist.clone(), true),
                Track::Retail { set, track } => (format!("{set} {track}"), String::new(), false),
            };
            info!("AUDIO_MUSIC playing {title:?} by {artist:?}");
            if music.events.len() >= 64 {
                music.events.remove(0);
            }
            music.events.push(serde_json::json!({"name": "music_track_started", "title": title, "artist": artist, "custom": custom}));
            let event = MusicTrackStarted { title, artist, custom };
            music.playing = Some(event.clone());
            started.write(event);
        }
    }
    let ahead = rt.music.queued_seconds();
    drop(rt);
    if music.decoding.is_none() && ahead < AHEAD && music.sequencer.is_some() {
        music.start_decode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_audio::music::pathfinder::format::{Action, Branch, Event, Kind, Node, Operand};

    /// A soundtrack graph of two tracks (entry -> two segments -> stop), its play event as the disc's.
    fn soundtrack() -> HashMap<String, Arc<Graph>> {
        let node = |kind, target| Node { kind, section: 1, loops: 0, random: false, owner: 0,
            branches: vec![Branch { lo: 0, hi: 127, target }] };
        let g = Graph {
            slot: 2,
            nodes: vec![node(Kind::Entry, Some(1)), node(Kind::Audio(1), Some(2)), node(Kind::Audio(2), None),
                node(Kind::Entry, Some(4)), node(Kind::Audio(3), Some(5)), node(Kind::Audio(4), None)],
            events: vec![Event { id: SOUNDTRACK_PLAY, actions: vec![
                Action::Goto { node: Operand::Var(0), section: Some(Operand::Const(1)), cut: false }] }],
            vars: vec![(SOUNDTRACK_VAR.into(), 0)],
            durations: vec![9500; 4],
        };
        HashMap::from([(SOUNDTRACK_SET.to_owned(), Arc::new(g))])
    }

    #[test]
    fn retail_and_bank_tracks_play_in_queue_order_and_the_queue_repeats() {
        let graphs = soundtrack();
        let tracks = vec![
            Track::Retail { set: SOUNDTRACK_SET.into(), track: 2 },
            Track::Bank { bank: "mine.bank".into(), first: 3, count: 2, name: "Mine".into(), artist: String::new() },
        ];
        let mut s = Sequencer::new(tracks, false, 1);
        let set = |seg| Some((Source::Set(SOUNDTRACK_SET.into()), seg));
        let file = |seg| Some((Source::File("mine.bank".into()), seg));
        let walk: Vec<_> = (0..6).map(|_| s.next(&graphs)).collect();
        assert_eq!(walk, [set(2), set(3), file(3), file(4), set(2), set(3)]);
    }

    #[test]
    fn requests_replace_the_queue_and_a_mods_queue_and_volume_end_with_it() {
        let mut m = Music { graphs: soundtrack(), loaded: true, ..Default::default() };
        m.sequencer = m.soundtrack();
        let dir = std::env::temp_dir().join(format!("skate-music-req-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tracks = [skate_audio::music::bank::Track { first: 0, count: 1, name: "Mine".into(), artist: "Me".into() }];
        let mut f = std::io::BufWriter::new(std::fs::File::create(dir.join("mine.bank")).unwrap());
        skate_audio::music::bank::write_bank(&mut f, 1, [Ok(skate_audio::music::bank::Segment { rate: 8000, channels: 1, samples: vec![0; 800] })], &tracks).unwrap();
        drop(f);
        let mut list = crate::music_playlist::from_banks(&dir.join("p.json"), "P", false, &[dir.join("mine.bank")]).unwrap();
        list.tracks.push(crate::music_playlist::Entry { bank: String::new(), soundtrack: Some(2), track: 0, name: String::new(), artist: String::new() });
        std::fs::write(dir.join("p.json"), serde_json::to_string(&list).unwrap()).unwrap();
        m.requests.push(MusicRequest::Playlist { owner: Some("mod_a".into()), path: dir.join("p.json"), queue: false });
        m.requests.push(MusicRequest::Volume { owner: Some("mod_a".into()), level: Some(0.5) });
        assert!(m.apply(), "a new queue restarts the stream");
        assert_eq!((m.owner.as_deref(), m.volume.clone()), (Some("mod_a"), Some(("mod_a".into(), 0.5))));
        let graphs = m.graphs.clone();
        assert_eq!(m.sequencer.as_ref().unwrap().tracks[1], Track::Retail { set: SOUNDTRACK_SET.into(), track: 2 }, "a soundtrack entry mixes in");
        assert!(matches!(m.sequencer.as_mut().unwrap().next(&graphs), Some((Source::File(_), 0))));
        m.requests.push(MusicRequest::ClearOwner("mod_b".into()));
        assert!(!m.apply() && m.owner.is_some(), "another mod's end changes nothing");
        m.requests.push(MusicRequest::ClearOwner("mod_a".into()));
        assert!(m.apply());
        assert_eq!((m.owner.clone(), m.volume.clone()), (None, None));
        assert!(matches!(m.sequencer.as_mut().unwrap().next(&graphs), Some((Source::Set(_), _))), "back to the soundtrack");
        m.requests.push(MusicRequest::Stop { owner: Some("mod_a".into()) });
        m.apply();
        assert!(m.sequencer.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_skip_goes_to_the_track_after_the_heard_one_not_after_the_one_decoded_ahead() {
        let graphs = soundtrack();
        let tracks: Vec<Track> = (0..3).map(|i| Track::Bank { bank: format!("{i}.bank").into(), first: 0, count: 1, name: String::new(), artist: String::new() }).collect();
        let mut m = Music { graphs: graphs.clone(), loaded: true, ..Default::default() };
        m.sequencer = Some(Sequencer::new(tracks, false, 1));
        let seq = m.sequencer.as_mut().unwrap();
        assert_eq!(seq.next(&graphs), Some((Source::File("0.bank".into()), 0)));
        assert_eq!(seq.next(&graphs), Some((Source::File("1.bank".into()), 0)), "track 1 decoded ahead");
        m.playing = Some(MusicTrackStarted { title: "0".into(), artist: String::new(), custom: true });
        m.marks.insert(7, m.sequencer.as_ref().unwrap().current.clone().unwrap());
        m.requests.push(MusicRequest::Skip);
        assert!(m.apply());
        assert_eq!(m.sequencer.as_mut().unwrap().next(&graphs), Some((Source::File("1.bank".into()), 0)));
    }

    #[test]
    fn shuffle_plays_every_track_once_per_pass_and_skip_moves_on() {
        let graphs = soundtrack();
        let tracks: Vec<Track> = (0..5).map(|i| Track::Bank { bank: format!("{i}.bank").into(), first: 0, count: 1, name: String::new(), artist: String::new() }).collect();
        let mut s = Sequencer::new(tracks, true, 7);
        let mut pass: Vec<_> = (0..5).filter_map(|_| s.next(&graphs)).map(|(src, _)| src).collect();
        pass.sort_by_key(|p| format!("{p:?}"));
        pass.dedup();
        assert_eq!(pass.len(), 5, "each track once");
        let mut s = Sequencer::new(vec![Track::Retail { set: SOUNDTRACK_SET.into(), track: 1 }], false, 1);
        assert_eq!(s.next(&graphs).map(|x| x.1), Some(0));
        s.skip();
        assert_eq!(s.next(&graphs).map(|x| x.1), Some(0), "restarts the only track");
        assert!(Sequencer::new(Vec::new(), true, 1).next(&graphs).is_none());
    }
}
