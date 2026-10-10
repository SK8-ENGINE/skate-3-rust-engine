//! Living-world speech playback on the native runtime: the peds' PedestrianSpeech requests
//! (`world_sources`) and the NPC skaters' bail grunts (`npc_skaters`) go through the speech
//! manager (`skate_audio::world::speech_manager`: value → event, the vault tuning gate, the
//! request words), the speech library (`speech_rules`: the `.evt` record and its takes) and the
//! living world's two streams (`speech_player`: interrupts, the queue, the cut), and play as
//! stream voices whose level, reverb send, pitch, pan and filters follow the speaker's MixMap
//! owner every console frame: a ped's PedestrianSpeech instance, a skater's PlayerSpeech
//! instance (spec `audio-specs/world-audio-hookin-spec.md`, doc 15 "Speech").
//!
//! **Data.** The index and rules (`speech/livingworld.json`) are a normal setup export; the takes
//! are the opt-in decode (`SKATE_SETUP_SPEECH=1`, ~2.4 GB). Without the index or the decode the
//! speech stays silent and says so once in the log (`AUDIO_WORLD speech off: …`); requests are
//! still gated and logged. A take is read from disk when its line starts (as retail streams it)
//! and dropped after use.
//!
//! **The announcer** (`announcerspeech.big`, bank 3, channel 3; `skate_audio::world::announcer`):
//! an NPC pro's crash within 12 m of the camera asks for `480_slam_pro`, and engine systems / mods
//! ask through [`crate::world_audio::AnnouncerSpeechEvent`]. Lines need the running challenge's
//! announcer character ([`crate::world_audio::LivingWorldAudio::announcer`]); in free skate there is
//! none, so (as retail) the requests are gated, draw, and find no line. Its lines play at the Global
//! Announcer object's outputs and raise `Announcer.in0` (the mix ducks) while they play.
//!
//! **Inert** while nothing requests a line and nothing plays.
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use serde::Deserialize;
use skate_audio::bus::speech_echo::EchoParams;
use skate_audio::formats::SampleHeader;
use skate_audio::mixer::Mixer;
use skate_audio::player::objpos::{Listener, ObjPos};
use skate_audio::world::keys;
use skate_audio::world::owners::Pool;
use skate_audio::world::peds::SpeechRequest;
use skate_audio::world::skater_speech::{Reactions, Say, SkaterSpeech};
use skate_audio::world::announcer::{self, Context as AnnouncerContext};
use skate_audio::world::speech::{ANNOUNCER_BANK, Clip, Line, MAIN_CAST_BANK, SPEECH_BANK, SpeechIndex, SpeechSlots, Take};
use skate_audio::world::speech_manager::{GateInputs, Speaker, SpeechManager, kind, main_cast};
use skate_audio::world::speech_player::{self, Event, Outcome, PedLevelSelect, Request, SpeechPlayer, SpeechVoices, VoiceParams};
use skate_audio::world::speech_rules::{ClipHeader, ClipRef, EventTable, Library as SpeechLibrary, Record};
use skate_audio::world::traffic::OutputsSnapshot;
use skate_audio::world::Lcg;

use super::native::Native;

/// The NPC bail grunt's event (`201_grunt`, message 8206 / 115 of `sub_824BF5F8`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const BAIL_GRUNT_EVENT: u16 = main_cast::message::BAIL_GRUNT.0;
/// Decoded takes kept loaded after their line (the rest are read again when picked).
const KEEP_TAKES: usize = 24;

/// A ped's request with what the speech host needs of the ped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PedRequest {
    pub(crate) request: SpeechRequest,
    pub(crate) voice: u32,
    pub(crate) speaker: Speaker,
    pub(crate) level: PedLevelSelect,
}

/// A published NPC skater with a speech voice (its PlayerSpeech position).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SkaterSpeaker {
    pub(crate) id: u64,
    pub(crate) voice: u32,
    pub(crate) position: [f32; 3],
    pub(crate) velocity: [f32; 3],
    /// Its record's reaction bytes this frame (its own speech, `skater_speech`).
    pub(crate) reactions: Reactions,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Who {
    Ped(PedLevelSelect),
    Skater(u32),
    /// The announcer (the line's clip voice: 35 / 36).
    Announcer(u32),
}

/// Which channel a request went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Chan {
    Living,
    Cast,
    Announcer,
}

impl Chan {
    fn name(self) -> &'static str {
        match self {
            Self::Living => "living world",
            Self::Cast => "main cast",
            Self::Announcer => "announcer",
        }
    }
}

/// The announcer's speaker ids (one per announcer voice; no world object speaks them).
fn announcer_speaker(voice: u32) -> u64 {
    u64::MAX - 1024 + u64::from(voice)
}

/// An announcer request from an engine system or a mod ([`crate::world_audio::AnnouncerSpeechEvent`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnnouncerRequest {
    pub(crate) event: crate::world_audio::AnnouncerLine,
    pub(crate) pro: Option<u32>,
    pub(crate) words: Vec<u32>,
}

/// The event an [`crate::world_audio::AnnouncerLine`] names in the announcer's table.
fn announcer_event(table: &EventTable, line: &crate::world_audio::AnnouncerLine) -> Option<u16> {
    match line {
        crate::world_audio::AnnouncerLine::Id(id) => table.event(*id).map(|e| e.id),
        crate::world_audio::AnnouncerLine::Name(name) => {
            let name = name.to_ascii_lowercase();
            table.events.iter().find(|e| e.name.to_ascii_lowercase() == name || e.name.split('_').next() == Some(name.as_str())).map(|e| e.id)
        }
    }
}

/// A speech channel beside the living world's: the main cast (`maincastspeech.big`, speech manager
/// bank 0, channel 0: the pros' and the special cast's lines) or the announcer (bank 3, channel 3),
/// each with its own tuning, timers, streams and decoded takes.
struct MainCast {
    data: Data,
    manager: SpeechManager,
    player: SpeechPlayer,
    loaded: VecDeque<u16>,
}

/// The loaded speech export.
struct Data {
    index: SpeechIndex,
    table: EventTable,
    library: SpeechLibrary,
    slots: SpeechSlots,
    audio: Option<PathBuf>,
    /// Audio content overlays' takes by (clip, take): replacements and extra takes.
    mod_takes: HashMap<(usize, usize), PathBuf>,
}

/// An overlay take's rate and length from its WAV header (0 when unreadable: the take still plays
/// from its PCM when picked).
fn take_of(path: &std::path::Path) -> Take {
    let pcm = std::fs::read(path).ok().and_then(|b| super::library::wav_pcm(&b));
    let (rate, samples) = pcm.map_or((0, 0), |p| (p.rate, p.channels.first().map_or(0, Vec::len) as u32));
    Take { offset: 0, size: 0, rate, samples }
}

#[derive(Resource)]
pub(crate) struct WorldSpeech {
    /// This frame's requests (the hosts append).
    pub(crate) peds: Vec<PedRequest>,
    pub(crate) grunts: Vec<u64>,
    /// The published NPC skaters with a voice (the NPC host rewrites it).
    pub(crate) skaters: Vec<SkaterSpeaker>,
    tried: bool,
    data: Option<Data>,
    manager: SpeechManager,
    player: SpeechPlayer,
    /// The stream voice's PEAK curves (`world_tuning.speech_voice`).
    voice: skate_audio::world::speech_player::SpeechVoiceTuning,
    /// Each speaker's voice id (the clip names' voice: SFXObj_Speech's inputs read it).
    voices: HashMap<u64, u32>,
    /// SFXObj_Speech's inputs as last written (in0..in4).
    speech_inputs: [bool; 5],
    /// The speaking peds' positions (`world_sources` fills it; the echo delay reads it).
    pub(crate) ped_positions: HashMap<u64, [f32; 3]>,
    /// Each speaker's echo delay countdown and value (`sub_824D9370`: recomputed when the count
    /// reaches 0, then reloaded with `delay_frames`).
    delay_clock: HashMap<u64, (i32, f32)>,
    /// The delay last posted to each echo slot (retail posts Del0 only when it changed).
    echo_delay: [Option<f32>; skate_audio::bus::speech_echo::SLOTS],
    /// The main-cast channel (None: the install has no main-cast index).
    cast: Option<MainCast>,
    /// The announcer channel (None: the install has no announcer index).
    announcer: Option<MainCast>,
    /// This frame's announcer requests (engine systems and mods).
    pub(crate) announces: Vec<AnnouncerRequest>,
    /// The running challenge's announcer character (None: free skate) and the MixMap tick it was
    /// named at (the gate's challenge timer, retail system `+908`).
    pub(crate) announcer_character: Option<u32>,
    focus_speech: bool,
    announcer_since: u64,
    /// `Announcer.in0` as last written.
    announcer_input: bool,
    /// Announcer requests made (diagnostics; free skate refuses every one).
    pub(crate) announcer_asked: u64,
    /// The NPC skaters' own speech processes (dropped while idle).
    skater_speech: HashMap<u64, SkaterSpeech>,
    rng: Lcg,
    /// The manager clock (s): console time since the runtime started (`MixMap::ticks`).
    clock: f64,
    last_tick: u64,
    who: HashMap<u64, Who>,
    /// PlayerSpeech instances 1.. for speaking skaters (0 = the local player's record).
    skater_slots: Pool,
    skater_pos: HashMap<u64, ObjPos>,
    loaded: VecDeque<u16>,
    missing_logged: bool,
    epoch: Option<u64>,
    last_camera: Option<([f32; 3], u64)>,
    /// Lines started (the summary log).
    pub(crate) lines: u64,
    /// Audio event rows (line starts) while some mod subscribes (`mod_audio::events_frame`).
    pub(crate) events: super::mod_audio::EventBuf,
}

impl WorldSpeech {
    /// A runtime tuning write changed the world tuning (`tuning.rs`): the managers' event tuning and
    /// the stream voice's curves follow; the managers' timers (who spoke when) are kept.
    pub(crate) fn retune(&mut self, library: &super::Library) {
        if !self.tried {
            return;
        }
        self.manager.tuning = library.world_tuning().speech_tuning();
        self.voice = library.world_tuning().speech_voice();
        if let Some(cast) = &mut self.cast {
            cast.manager.tuning = library.world_tuning().speech_tuning_bank(0);
        }
    }
}

impl WorldSpeech {
    /// The speech generator's state (`seed.rs`, doc 16 L5).
    pub(crate) fn rng_state(&self) -> u32 {
        self.rng.0
    }
    pub(crate) fn set_rng_state(&mut self, state: u32) {
        self.rng.0 = state;
    }

    /// An audio content hot swap changed the speech (an overlay's takes, `swap.rs`): the lines
    /// speaking stop and the index and takes are read again at the next request (the managers'
    /// "who spoke when" timers start again with them).
    pub(crate) fn reload_content(&mut self, rt: &mut skate_audio::runtime::Runtime) {
        let mut missing = self.missing_logged;
        if let Some(data) = self.data.as_ref() {
            let mut v = Voices { mixer: &mut rt.mixer, bank: SPEECH_BANK, data, loaded: &mut self.loaded, missing: &mut missing, echo_delay: &mut self.echo_delay };
            self.player.clear(&mut v);
        }
        if let Some(c) = self.cast.as_mut() {
            let mut v = Voices { mixer: &mut rt.mixer, bank: MAIN_CAST_BANK, data: &c.data, loaded: &mut c.loaded, missing: &mut missing, echo_delay: &mut self.echo_delay };
            c.player.clear(&mut v);
        }
        self.missing_logged = missing;
        self.loaded.clear();
        rt.mixer.remove_bank(SPEECH_BANK);
        rt.mixer.remove_bank(MAIN_CAST_BANK);
        self.data = None;
        self.cast = None;
        self.tried = false;
    }
}

impl Default for WorldSpeech {
    fn default() -> Self {
        Self {
            peds: Vec::new(),
            grunts: Vec::new(),
            skaters: Vec::new(),
            tried: false,
            data: None,
            manager: SpeechManager::default(),
            // The living world's speech channel is the second (streams 2 / 3; the main cast's first).
            player: SpeechPlayer::on_channel(1),
            voice: Default::default(),
            voices: HashMap::new(),
            speech_inputs: [false; 5],
            ped_positions: HashMap::new(),
            delay_clock: HashMap::new(),
            echo_delay: [None; skate_audio::bus::speech_echo::SLOTS],
            cast: None,
            announcer: None,
            announces: Vec::new(),
            announcer_character: None,
            focus_speech: false,
            announcer_since: 0,
            announcer_input: false,
            announcer_asked: 0,
            skater_speech: HashMap::new(),
            rng: Lcg(0x5EEC),
            clock: 0.0,
            last_tick: 0,
            who: HashMap::new(),
            skater_slots: Pool::new(keys::PLAYER_SPEECH_INSTANCES - 1),
            skater_pos: HashMap::new(),
            loaded: VecDeque::new(),
            missing_logged: false,
            epoch: None,
            last_camera: None,
            lines: 0,
            events: None,
        }
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<WorldSpeech>()
        .add_systems(Update, frame.after(super::world_sources::frame).after(super::npc_skaters::frame));
}

// ---- the export (speech/livingworld.json) ----

#[derive(Deserialize)]
struct IndexJson {
    clips: Vec<ClipJson>,
    rules: RulesJson,
}

#[derive(Deserialize)]
struct ClipJson {
    name: String,
    #[serde(default)]
    id: Option<u16>,
    #[serde(default)]
    history: u8,
    takes: Vec<TakeJson>,
}

#[derive(Deserialize)]
struct TakeJson {
    offset: u32,
    size: u32,
    rate: u32,
    samples: u32,
}

#[derive(Deserialize)]
struct RulesJson {
    bank: u8,
    sub_bank: u8,
    events: Vec<EventJson>,
}

#[derive(Deserialize)]
struct EventJson {
    id: u16,
    name: String,
    queue_timeout: u16,
    priority: u16,
    conditions: u8,
    flags: u8,
    probability: u8,
    flags2: u8,
    fields: Vec<u8>,
    records: Vec<RecordJson>,
}

#[derive(Deserialize)]
struct RecordJson {
    weight: u8,
    probability: u8,
    mode: u8,
    locals: u8,
    values: Vec<u32>,
    clips: Vec<u16>,
}

/// `mods`: the overlays' replaced takes by (clip name without `.dat`, take) and extra takes by
/// clip (`Library::speech_mods`; empty without overlays).
fn load(index_path: &std::path::Path, audio: Option<PathBuf>, mods: &(HashMap<(String, u32), PathBuf>, HashMap<String, Vec<PathBuf>>)) -> Result<Data, String> {
    let text = std::fs::read_to_string(index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let json: IndexJson = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", index_path.display()))?;
    let mut clips = Vec::with_capacity(json.clips.len());
    let mut ids = Vec::with_capacity(json.clips.len());
    let mut headers = Vec::new();
    let mut mod_takes = HashMap::new();
    let (replaced, extra) = mods;
    let mut known = std::collections::HashSet::new();
    for c in json.clips {
        let Some((event, voice, voice_name, line)) = skate_audio::world::speech::parse_name(&c.name) else { continue };
        let stem = c.name.strip_suffix(".dat").unwrap_or(&c.name).to_owned();
        let mut takes: Vec<Take> = c.takes.iter().map(|t| Take { offset: t.offset, size: t.size, rate: t.rate, samples: t.samples }).collect();
        if !replaced.is_empty() || !extra.is_empty() {
            let index = clips.len();
            for (t, take) in takes.iter_mut().enumerate() {
                if let Some(path) = replaced.get(&(stem.clone(), t as u32)) {
                    *take = take_of(path);
                    mod_takes.insert((index, t), path.clone());
                }
            }
            for path in extra.get(&stem).into_iter().flatten() {
                if takes.len() < 255 {
                    mod_takes.insert((index, takes.len()), path.clone());
                    takes.push(take_of(path));
                }
            }
            known.insert(stem);
        }
        if let Some(id) = c.id {
            headers.push(ClipHeader { id, takes: takes.len().min(255) as u8, history: c.history, flags: 0 });
        }
        ids.push(c.id);
        clips.push(Clip { name: c.name, event, voice, voice_name, line, takes });
    }
    for (clip, take) in replaced.keys() {
        if !known.contains(clip) {
            warn!("AUDIO_WORLD speech: an audio mod replaces take {take} of {clip}, which the speech index does not have");
        }
    }
    for clip in extra.keys().filter(|c| !known.contains(*c)) {
        warn!("AUDIO_WORLD speech: an audio mod adds takes to {clip}, which the speech index does not have");
    }
    let mut index = SpeechIndex::new(clips);
    index.set_ids(&ids);
    let events = json
        .rules
        .events
        .into_iter()
        .map(|e| skate_audio::world::speech_rules::Event {
            id: e.id,
            name: e.name,
            queue_timeout: e.queue_timeout,
            priority: e.priority,
            conditions: e.conditions,
            flags: e.flags,
            probability: e.probability,
            flags2: e.flags2,
            fields: e.fields,
            records: e
                .records
                .into_iter()
                .map(|r| Record {
                    weight_code: r.weight,
                    probability: r.probability,
                    mode: r.mode,
                    locals: r.locals,
                    values: r.values,
                    clips: r.clips.into_iter().map(|id| ClipRef { id, lookup: 0, params: 0 }).collect(),
                })
                .collect(),
        })
        .collect();
    let table = EventTable { bank: json.rules.bank, sub_bank: json.rules.sub_bank, events };
    let slots = SpeechSlots::new(&index);
    Ok(Data { index, table, library: SpeechLibrary::new(headers), slots, audio, mod_takes })
}

// ---- the stream voices ----

struct Voices<'a> {
    mixer: &'a mut Mixer,
    /// The mixer bank of this channel's takes.
    bank: usize,
    data: &'a Data,
    loaded: &'a mut VecDeque<u16>,
    missing: &'a mut bool,
    echo_delay: &'a mut [Option<f32>; skate_audio::bus::speech_echo::SLOTS],
}

impl Voices<'_> {
    fn ensure(&mut self, line: Line) -> Option<u16> {
        let slot = self.data.slots.slot(line)?;
        if self.loaded.contains(&slot) {
            return Some(slot);
        }
        let path = match self.data.mod_takes.get(&(line.clip, line.take)) {
            Some(p) => p.clone(),
            None => {
                let audio = self.data.audio.as_ref()?;
                let clip = self.data.index.clips.get(line.clip)?;
                let stem = clip.name.strip_suffix(".dat").unwrap_or(&clip.name);
                audio.join(stem).join(format!("{:02}.wav", line.take))
            }
        };
        let pcm = match std::fs::read(&path).ok().and_then(|b| super::library::wav_pcm(&b)) {
            Some(p) => p,
            None => {
                if !*self.missing {
                    *self.missing = true;
                    warn!("AUDIO_WORLD speech: {} is not decoded (the speech decode covers the free-roam events: SKATE_SETUP_SPEECH=1)", path.display());
                }
                return None;
            }
        };
        let frames = pcm.channels.first().map_or(0, Vec::len) as u32;
        let header = SampleHeader { codec: 0, channels: pcm.channels.len().clamp(1, 8) as u8, rate: pcm.rate, frames, loop_start: None };
        self.mixer.set_bank_sample(self.bank, slot, header, Arc::new(pcm));
        self.loaded.push_back(slot);
        while self.loaded.len() > KEEP_TAKES {
            if let Some(old) = self.loaded.pop_front() {
                // A playing voice keeps its own reference to the PCM.
                self.mixer.clear_bank_sample(self.bank, old);
            }
        }
        Some(slot)
    }

    /// The echo send and its slot's parameters; the delay goes out only when it changed.
    fn echo(&mut self, voice: u32, p: &VoiceParams) {
        let cached = self.echo_delay.get_mut(usize::from(p.slot));
        let delay = match (p.delay, cached) {
            (Some(d), Some(c)) if *c != Some(d) => {
                *c = Some(d);
                Some(d)
            }
            _ => None,
        };
        self.mixer.set_stream_echo(voice, p.slot, p.echo, &EchoParams { high_pass: p.echo_hpf, low_pass: p.echo_lpf, delay });
    }
}

impl SpeechVoices for Voices<'_> {
    fn open(&mut self, line: Line, p: &VoiceParams) -> Option<u32> {
        let slot = self.ensure(line)?;
        let v = self.mixer.open_direct(self.bank, slot, 0.0, p.pitch, p.gain, Some(p.azimuth))?;
        self.mixer.set_stream_dsp(v, p.hpf, p.lpf, p.send, p.peak);
        self.echo(v, p);
        Some(v)
    }
    fn set(&mut self, voice: u32, p: &VoiceParams) {
        self.mixer.set_direct(voice, p.pitch, p.gain, Some(p.azimuth));
        self.mixer.set_stream_dsp(voice, p.hpf, p.lpf, p.send, p.peak);
        self.echo(voice, p);
    }
    fn alive(&self, voice: u32) -> bool {
        self.mixer.direct_alive(voice)
    }
    fn stop(&mut self, voice: u32) {
        skate_audio::eval::VoiceHost::release(self.mixer, voice);
    }
}

/// The speaker words of a skater's voice (the `aud_characteristics` model of the AI skaters):
/// type / variant bits from the export, else from the speech rules.
fn skater_speaker(library: &super::Library, data: &Data, voice: u32) -> Speaker {
    let tuning = library.world_tuning();
    match tuning.ped_model(voice) {
        Some(m) => Speaker { index: voice, kind: m.kind, variant: m.variant, partner: 0, word5: 0, word6: m.gender },
        None => {
            let (kind, variant) = skate_audio::world::speech_manager::speaker_bits(&data.table, &data.index, voice).unwrap_or((kind::SKATER_MALE, 1));
            Speaker { index: voice, kind, variant, partner: 0, word5: 0, word6: 0 }
        }
    }
}

/// A main-cast request block (`sub_824AC560`): the speaker's cast bit / word, the near / far flag
/// (the main cast's sense: 1 near, 2 far) and, for the pro-on-pro lines, the other skater's cast bit
/// and other-skater word.
fn cast_block(cast: (u32, u32, u32), far: bool, other: Option<(u32, u32, u32)>) -> main_cast::Block {
    let mut b = [0u32; 14];
    b[0] = cast.0;
    b[1] = cast.1;
    b[2] = if far { main_cast::FAR } else { main_cast::NEAR };
    if let Some(o) = other {
        b[4] = o.0;
        b[5] = o.2;
    }
    b
}

/// A speaker's main-cast words when its model speaks on the main cast.
fn cast_of(library: &super::Library, voice: u32) -> Option<(u32, u32, u32)> {
    library.world_tuning().ped_model(voice).and_then(|m| m.main_cast())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    native: Option<ResMut<Native>>,
    mut speech: ResMut<WorldSpeech>,
    mut held: ResMut<super::world_sources::WorldHeld>,
    library: Option<Res<super::Library>>,
    cues: Res<super::skate_events::Cues>,
    listener: Query<&GlobalTransform, With<super::GameAudioListener>>,
    (living, mut announce): (Option<Res<crate::world_audio::LivingWorldAudio>>, MessageReader<crate::world_audio::AnnouncerSpeechEvent>),
) {
    let speech = &mut *speech;
    // The announcer: the running challenge's character, and this frame's requests.
    speech.focus_speech = living.as_deref().is_some_and(|l| l.focus_speech);
    let character = living.as_deref().and_then(crate::world_audio::LivingWorldAudio::announcer_character);
    if character != speech.announcer_character {
        speech.announcer_character = character;
        speech.announcer_since = native.as_deref().and_then(|n| n.mixmap.as_ref()).map_or(0, |m| m.ticks);
    }
    for e in announce.read() {
        speech.announces.push(AnnouncerRequest { event: e.event.clone(), pro: e.pro, words: e.words.clone() });
    }
    let announcer_idle = speech.announces.is_empty() && !speech.announcer_input && speech.announcer.as_ref().is_none_or(|c| c.player.busy() == 0 && c.player.queued() == 0);
    let cast_idle = speech.cast.as_ref().is_none_or(|c| c.player.busy() == 0 && c.player.queued() == 0) && announcer_idle;
    let reacting = !speech.skater_speech.is_empty() || speech.skaters.iter().any(|s| s.reactions != Reactions::default());
    if speech.peds.is_empty() && speech.grunts.is_empty() && speech.player.busy() == 0 && speech.player.queued() == 0 && cast_idle && !reacting && speech.skater_slots.holders().next().is_none() && !speech.speech_inputs.contains(&true) {
        return;
    }
    let (Some(mut native), Some(library)) = (native, library) else {
        speech.peds.clear();
        speech.grunts.clear();
        speech.announces.clear();
        return;
    };
    let camera = listener.single().ok().map(|t| (t.translation().to_array(), t.forward().as_vec3().to_array()));
    run(speech, &held.peds, &mut native, &library, camera, &cues.riding.audio);
    if held.speech_lines != speech.lines {
        held.speech_lines = speech.lines;
    }
}

/// One frame of the speech host (see the module docs).
pub(crate) fn run(speech: &mut WorldSpeech, peds: &[(u64, u32)], native: &mut Native, library: &super::Library, camera: Option<([f32; 3], [f32; 3])>, local: &skate_audio::player::AudioState) {
    // A map change unloaded nothing of ours, but the speakers are gone: stop and forget.
    if speech.epoch != Some(native.map_epoch) {
        speech.epoch = Some(native.map_epoch);
        if let Ok(mut rt) = super::timing::lock(&native.shared, &super::timing::GAME_LOCK)
            && let Some(data) = speech.data.as_ref()
        {
            let mut missing = speech.missing_logged;
            let mut v = Voices { mixer: &mut rt.mixer, bank: SPEECH_BANK, data, loaded: &mut speech.loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
            speech.player.clear(&mut v);
            if let Some(c) = speech.cast.as_mut() {
                let mut v = Voices { mixer: &mut rt.mixer, bank: MAIN_CAST_BANK, data: &c.data, loaded: &mut c.loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
                c.player.clear(&mut v);
            }
            if let Some(c) = speech.announcer.as_mut() {
                let mut v = Voices { mixer: &mut rt.mixer, bank: ANNOUNCER_BANK, data: &c.data, loaded: &mut c.loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
                c.player.clear(&mut v);
            }
            speech.missing_logged = missing;
        }
        speech.skater_speech.clear();
        speech.who.clear();
        speech.skater_slots.clear();
        speech.skater_pos.clear();
        speech.last_camera = None;
    }
    if !speech.tried {
        speech.tried = true;
        speech.manager = SpeechManager::new(library.world_tuning().speech_tuning());
        speech.voice = library.world_tuning().speech_voice();
        match library.speech("livingworld") {
            None => info!("AUDIO_WORLD speech off: the install has no speech index (rerun setup)"),
            Some((index, audio)) => match load(&index, audio.clone(), &library.speech_mods("livingworld")) {
                Ok(data) => {
                    if audio.is_none() {
                        info!("AUDIO_WORLD speech: lines are chosen and logged but silent: the takes are not decoded (setup with SKATE_SETUP_SPEECH=1)");
                    } else {
                        info!("AUDIO_WORLD speech on: {} clips, {} events", data.index.clips.len(), data.table.events.len());
                    }
                    speech.data = Some(data);
                }
                Err(e) => warn!("AUDIO_WORLD speech off: {e}"),
            },
        }
        // The main cast's channel (the pros and the special cast; opt-in decode as the living world's).
        if let Some((index, audio)) = library.speech("maincast") {
            match load(&index, audio.clone(), &library.speech_mods("maincast")) {
                Ok(data) => {
                    info!("AUDIO_WORLD main-cast speech {}: {} clips, {} events", if audio.is_some() { "on" } else { "chosen but silent (not decoded)" }, data.index.clips.len(), data.table.events.len());
                    speech.cast = Some(MainCast { data, manager: SpeechManager::new(library.world_tuning().speech_tuning_bank(0)), player: SpeechPlayer::with_queue_of(0, &speech.player), loaded: VecDeque::new() });
                }
                Err(e) => warn!("AUDIO_WORLD main-cast speech off: {e}"),
            }
        }
        // The announcer's channel (no cut: the announcer object has none).
        if let Some((index, audio)) = library.speech("announcer") {
            match load(&index, audio.clone(), &library.speech_mods("announcer")) {
                Ok(data) => {
                    info!("AUDIO_WORLD announcer speech {}: {} clips, {} events", if audio.is_some() { "on" } else { "chosen but silent (not decoded)" }, data.index.clips.len(), data.table.events.len());
                    let mut player = SpeechPlayer::with_queue_of(announcer::CHANNEL, &speech.player);
                    player.no_cut = true;
                    speech.announcer = Some(MainCast { data, manager: SpeechManager::new(library.world_tuning().speech_tuning_bank(announcer::BANK)), player, loaded: VecDeque::new() });
                }
                Err(e) => warn!("AUDIO_WORLD announcer speech off: {e}"),
            }
        }
    }
    let Some(data) = speech.data.as_mut() else {
        speech.peds.clear();
        speech.grunts.clear();
        speech.announces.clear();
        return;
    };
    let Native { mixmap, shared, cuts, .. } = native;
    let Some(m) = mixmap.as_mut() else { return };
    let Ok(mut runtime) = super::timing::lock(shared, &super::timing::GAME_LOCK) else { return };
    let rt = &mut *runtime;
    // The manager clock: console time since the runtime started (every MixMap evaluation), so the
    // per-speaker timers run while nothing speaks, as retail's (they start at 0 at boot).
    speech.clock = m.ticks as f64 * f64::from(super::world_sources::evaluation_dt());
    let inputs = GateInputs { now: speech.clock, player_speed: local.ground_speed.abs(), ..Default::default() };
    let mut missing = speech.missing_logged;
    // The requests.
    // (speaker, who, result, value, channel)
    type Res = Result<skate_audio::world::speech_manager::Line, skate_audio::world::speech_manager::Refusal>;
    let mut new: Vec<(u64, Who, Res, i32, Chan)> = Vec::new();
    for r in std::mem::take(&mut speech.peds) {
        if r.voice == 0 {
            continue;
        }
        speech.voices.insert(r.request.owner, r.voice);
        // `sub_824AC438`: a ped without a living-world type (a pro) takes the main-cast path.
        if let (Some(c), Some(words)) = (speech.cast.as_mut(), cast_of(library, r.voice)) {
            // 30 / 51 stop the speaker's playing line first, whatever the request then does.
            if main_cast::stops_line(r.request.value) {
                let mut v = Voices { mixer: &mut rt.mixer, bank: MAIN_CAST_BANK, data: &c.data, loaded: &mut c.loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
                if let Some(k) = c.player.stop_speaker(r.request.owner, &mut v) {
                    info!("AUDIO_WORLD speech (main cast) owner={} value={} stops its line on stream {k}", r.request.owner, r.request.value);
                }
            }
            let block = cast_block(words, r.request.flag == skate_audio::world::speech_manager::FAR, None);
            // 29 asks twice (141, then 136).
            for event in main_cast::events_for_value(r.request.value, false, &mut speech.rng) {
                let res = c.manager.request_main_cast(&mut c.data.library, &c.data.table, event, r.voice, &block, &inputs, &mut speech.rng);
                new.push((r.request.owner, Who::Ped(r.level), res, r.request.value, Chan::Cast));
            }
            continue;
        }
        let res = speech.manager.request(&mut data.library, &data.table, r.request.value, r.request.flag, &r.speaker, &inputs, &mut speech.rng);
        new.push((r.request.owner, Who::Ped(r.level), res, r.request.value, Chan::Living));
    }
    let camera_pos = camera.map(|c| c.0);
    // Skater messages and reactions: (skater, living-world event, main-cast event, the other skater,
    // an announcer event about the skater).
    let mut says: Vec<(SkaterSpeaker, Option<u16>, Option<u16>, Option<u32>, Option<u16>)> = Vec::new();
    for id in std::mem::take(&mut speech.grunts) {
        let Some(sk) = speech.skaters.iter().find(|s| s.id == id).copied() else { continue };
        let (lw, mc) = main_cast::message::BAIL_GRUNT;
        says.push((sk, Some(lw), Some(mc), None, None));
    }
    // `SFXObj_PlayerSpeech`'s non-local process, once per console frame (free skate: game mode 0).
    if m.ticks != speech.last_tick {
        for sk in speech.skaters.clone() {
            let main = speech.cast.is_some() && cast_of(library, sk.voice).is_some();
            let process = speech.skater_speech.entry(sk.id).or_default();
            for say in process.process(main, &sk.reactions, 0, &mut speech.rng) {
                says.push(match say {
                    Say::Living(e) => (sk, Some(e), None, None, None),
                    Say::MainCast { event, other } => (sk, None, Some(event), other, None),
                    Say::Message(lw, mc) => (sk, Some(lw), Some(mc), None, None),
                    Say::Announcer(e) => (sk, None, None, None, Some(e)),
                });
            }
        }
        let skaters = &speech.skaters;
        speech.skater_speech.retain(|id, p| !p.idle() && skaters.iter().any(|s| s.id == *id));
    }
    // The announcer's game state: the challenge's character (its announcer word from the export),
    // the challenge timer since it was named (free skate: no character, the timers never matter).
    let announcer_context = AnnouncerContext {
        character: speech.announcer_character,
        character_word: speech.announcer_character.and_then(|c| library.world_tuning().ped_model(c)).map_or(0, |m| m.announcer_id),
        flag_1089: false,
        challenge_flag: false,
    };
    let announcer_inputs = GateInputs {
        timer_a: if speech.announcer_character.is_some() { m.ticks.saturating_sub(speech.announcer_since) as f32 * super::world_sources::evaluation_dt() } else { f32::MAX },
        ..inputs
    };
    let announcer_level = library.world_tuning().announcer_level();
    for (sk, lw, mc, other, announce) in says {
        let far_m = library.world_tuning().ped_model(sk.voice).map_or(20.0, |m| m.far);
        let distance = camera_pos.map_or(0.0, |c| ((sk.position[0] - c[0]).powi(2) + (sk.position[1] - c[1]).powi(2) + (sk.position[2] - c[2]).powi(2)).sqrt());
        // `sub_824DB688`: a crash near the camera asks the announcer about a pro (word 2 = its
        // announcer pro id; none: no request).
        if let Some(event) = announce {
            let pro = library.world_tuning().ped_model(sk.voice).map_or(0, |m| m.announcer_pro);
            if let (Some(a), true, Some(block)) = (speech.announcer.as_mut(), distance < announcer_level.crash_distance, announcer::crash_block(pro)) {
                let res = a.manager.request_announcer(&mut a.data.library, &a.data.table, event, &announcer_context, &block, &announcer_inputs, &mut speech.rng);
                speech.announcer_asked += 1;
                new.push((sk.id, Who::Announcer(0), res, -1, Chan::Announcer));
            }
            continue;
        }
        // `sub_824DAC00`: far when the skater's distance exceeds the model's far threshold.
        let far = distance > far_m;
        speech.voices.insert(sk.id, sk.voice);
        if let (Some(c), Some(words), Some(event)) = (speech.cast.as_mut(), cast_of(library, sk.voice), mc) {
            let other = other.and_then(|o| cast_of(library, o));
            let block = cast_block(words, far, other);
            let res = c.manager.request_main_cast(&mut c.data.library, &c.data.table, event, sk.voice, &block, &inputs, &mut speech.rng);
            new.push((sk.id, Who::Skater(sk.voice), res, -1, Chan::Cast));
        } else if let Some(event) = lw {
            let words = skater_speaker(library, &*data, sk.voice);
            let flag = if far { skate_audio::world::speech_manager::FAR } else { skate_audio::world::speech_manager::NEAR };
            let res = speech.manager.request_event(&mut data.library, &data.table, event, flag, &words, &inputs, &mut speech.rng);
            new.push((sk.id, Who::Skater(sk.voice), res, -1, Chan::Living));
        }
    }
    // Engine systems' and mods' announcer requests.
    for r in std::mem::take(&mut speech.announces) {
        let Some(a) = speech.announcer.as_mut() else { continue };
        let Some(event) = announcer_event(&a.data.table, &r.event) else {
            warn!("AUDIO_WORLD announcer: no event {:?}", r.event);
            continue;
        };
        let mut block = [0u32; announcer::BLOCK];
        for (w, v) in block.iter_mut().zip(&r.words) {
            *w = *v;
        }
        if block[2] == 0 {
            block[2] = r.pro.and_then(|p| library.world_tuning().ped_model(p)).map_or(0, |m| m.announcer_pro);
        }
        let res = a.manager.request_announcer(&mut a.data.library, &a.data.table, event, &announcer_context, &block, &announcer_inputs, &mut speech.rng);
        speech.announcer_asked += 1;
        new.push((0, Who::Announcer(0), res, -1, Chan::Announcer));
    }
    for (speaker, who, res, value, chan) in new {
        match res {
            Ok(line) => {
                //824ABA18 preflight uses main-cast player0 with living-world records1.
                let control_available = matches!(chan, Chan::Living)
                    .then(|| speech.cast.as_ref().is_none_or(|c| c.player.available()));
                let (d, manager, player, loaded, bank) = match (chan, speech.cast.as_mut(), speech.announcer.as_mut()) {
                    (Chan::Cast, Some(c), _) => (&c.data, &c.manager, &mut c.player, &mut c.loaded, MAIN_CAST_BANK),
                    (Chan::Announcer, _, Some(c)) => (&c.data, &c.manager, &mut c.player, &mut c.loaded, ANNOUNCER_BANK),
                    _ => (&*data, &speech.manager, &mut speech.player, &mut speech.loaded, SPEECH_BANK),
                };
                let Some(lines) = d.index.picks_to_lines(&line.picks, u32::from(line.event)) else { continue };
                // The announcer speaks as the line's voice (the stream block's speaker, `sub_824A89E8`).
                let (speaker, who) = match who {
                    Who::Announcer(_) => {
                        let voice = lines.first().and_then(|l| d.index.clips.get(l.clip)).map_or(announcer::DEFAULT_SLOT, |c| c.voice);
                        (announcer_speaker(voice), Who::Announcer(voice))
                    }
                    other => (speaker, other),
                };
                let tuning = manager.tuning.get(&line.event).cloned().unwrap_or_default();
                let timeout = d.table.event(line.event).map_or(60, |e| u32::from(e.queue_timeout));
                let names: Vec<&str> = lines.iter().filter_map(|l| d.index.clips.get(l.clip).map(|c| c.name.as_str())).collect();
                let req = Request { speaker, event: line.event, priority: tuning.priority, queue_priority: d.table.event(line.event).map_or(0, |e| e.priority), retain_queued: d.table.event(line.event).is_some_and(|e| e.flags2 & 4 != 0), interrupt: tuning.interrupt, interrupt_when_full: tuning.interrupt_when_full, lines, timeout };
                let mut v = Voices { mixer: &mut rt.mixer, bank, data: d, loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
                let (outcome, external_stop) = player.request_controlled(req, &mut v, control_available);
                let channel = chan.name();
                info!("AUDIO_WORLD speech ({channel}) owner={speaker} value={value} event={} line={} -> {outcome:?}", line.event, names.join("+"));
                #[cfg(test)]
                eprintln!("AUDIO_WORLD speech ({channel}) owner={speaker} value={value} event={} line={} -> {outcome:?}", line.event, names.join("+"));
                if let Some(k) = external_stop && let Some(c) = speech.cast.as_mut() {
                    let mut control = Voices { mixer: &mut rt.mixer, bank: MAIN_CAST_BANK,
                        data: &c.data, loaded: &mut c.loaded, missing: &mut missing,
                        echo_delay: &mut speech.echo_delay };
                    c.player.stop_stream(k, &mut control);
                }
                if !matches!(outcome, Outcome::Dropped) {
                    speech.who.insert(speaker, who);
                }
            }
            Err(refusal) => {
                #[cfg(test)]
                eprintln!("AUDIO_WORLD speech ({}) owner={speaker} value={value} refused: {refusal:?}", chan.name());
                debug!("AUDIO_WORLD speech ({}) owner={speaker} value={value} refused: {refusal:?}", chan.name());
            }
        }
    }
    // Per console evaluation: the skaters' PlayerSpeech instances and positions, then the streams.
    if m.ticks == speech.last_tick {
        return;
    }
    let evaluations = m.ticks.saturating_sub(speech.last_tick).max(1);
    speech.last_tick = m.ticks;
    let dt = super::world_sources::evaluation_dt() * evaluations.min(4) as f32;
    let skater_ids: Vec<(u64, f32)> = speech.who.iter().filter(|(_, w)| matches!(w, Who::Skater(_))).map(|(id, _)| (*id, 0.0)).collect();
    let assignment = speech.skater_slots.assign(&skater_ids);
    let Some((cam, view)) = camera else { return };
    let cam_velocity = speech.last_camera.filter(|l| l.1 == *cuts).map_or([0.0; 3], |(last, _)| std::array::from_fn(|i| (cam[i] - last[i]) / dt));
    speech.last_camera = Some((cam, *cuts));
    let l = Listener { camera: cam, view, camera_velocity: cam_velocity, followed: local.com_position, facing: local.com_velocity, followed_velocity: local.com_velocity };
    for (id, g) in assignment.released {
        if let Some(mut pos) = speech.skater_pos.remove(&id) {
            pos.write(m, keys::player_speech_pos(g as u32 + 1), &l, None);
        }
    }
    for (g, id) in speech.skater_slots.holders().collect::<Vec<_>>() {
        let point = speech.skaters.iter().find(|s| s.id == id).map(|s| (s.position, s.velocity));
        speech.skater_pos.entry(id).or_default().write(m, keys::player_speech_pos(g as u32 + 1), &l, point);
    }
    let who = &speech.who;
    let skater_slots = &speech.skater_slots;
    let voice_tuning = speech.voice;
    let voices_of = &speech.voices;
    let ped_positions = &speech.ped_positions;
    let skaters = &speech.skaters;
    let delay_clock = &mut speech.delay_clock;
    let world = library.world_tuning();
    let m_ref: &skate_audio::mixmap::MixMap = m;
    let challenge_flag = announcer_context.challenge_flag;
    let focus = if speech.focus_speech { speech.announcer_character } else { None };
    let mut params = |speaker: u64, far: bool, event: u16| -> Option<VoiceParams> {
        // The announcer: the Global Announcer object's outputs, no position, no echo.
        if let Some(Who::Announcer(voice)) = who.get(&speaker) {
            let out = OutputsSnapshot::take(m_ref, skate_audio::mixmap::keys::ANNOUNCER, &speech_player::ANNOUNCER_SNAPSHOT_FILTERS);
            return Some(speech_player::announcer_outputs(&out, announcer_level.scale(*voice, challenge_flag)));
        }
        // `S+152`, the per-voice float (0 = absent: 1.0).
        let scale = voices_of.get(&speaker).and_then(|v| world.ped_model(*v)).map_or(1.0, |m| if m.pitch > 0.0 { m.pitch } else { 1.0 });
        let mut p = match *who.get(&speaker)? {
            Who::Ped(sel) => {
                let g = peds.iter().find(|(o, _)| *o == speaker)?.1;
                let out = OutputsSnapshot::take(m_ref, keys::ped_speech(g), &speech_player::PED_SNAPSHOT_FILTERS);
                speech_player::ped_outputs_focused(&out, sel, event, far, focus.is_some_and(|id| voices_of.get(&speaker) == Some(&id)), &voice_tuning, scale)
            }
            Who::Skater(voice) => {
                let g = skater_slots.instance(speaker)? as u32 + 1;
                let out = OutputsSnapshot::take(m_ref, keys::player_speech(g), &speech_player::SKATER_SNAPSHOT_FILTERS);
                speech_player::skater_outputs_focused(&out, voice, far, focus == Some(voice), &voice_tuning, scale)
            }
            Who::Announcer(_) => return None,
        };
        // The echo delay: every `delay_frames` console frames from the camera distance.
        let pos = ped_positions.get(&speaker).copied().or_else(|| skaters.iter().find(|s| s.id == speaker).map(|s| s.position));
        let clock = delay_clock.entry(speaker).or_insert((0, 0.0));
        clock.0 -= 1;
        if clock.0 <= 0 {
            clock.0 = voice_tuning.delay_frames.max(1) as i32;
            if let Some(pos) = pos {
                let d = ((pos[0] - cam[0]).powi(2) + (pos[1] - cam[1]).powi(2) + (pos[2] - cam[2]).powi(2)).sqrt();
                clock.1 = voice_tuning.delay(d);
            }
        }
        p.delay = Some(clock.1);
        Some(p)
    };
    let data = &*data;
    let mut channels: Vec<(&Data, &mut SpeechPlayer, &mut VecDeque<u16>, usize)> = Vec::with_capacity(3);
    if let Some(c) = speech.cast.as_mut() {
        channels.push((&c.data, &mut c.player, &mut c.loaded, MAIN_CAST_BANK));
    }
    if let Some(c) = speech.announcer.as_mut() {
        channels.push((&c.data, &mut c.player, &mut c.loaded, ANNOUNCER_BANK));
    }
    channels.push((data, &mut speech.player, &mut speech.loaded, SPEECH_BANK));
    for (d, player, loaded, bank) in channels {
        let mut v = Voices { mixer: &mut rt.mixer, bank, data: d, loaded, missing: &mut missing, echo_delay: &mut speech.echo_delay };
        let events = player.frame_at(&d.index, &mut params, &mut v, m_ref.ticks as u32);
        for e in &events {
            match *e {
                Event::Started { speaker, line, .. } => {
                    speech.lines += 1;
                    // Observe-only row for mods: class "speech" (living world) or "maincast".
                    let class = if bank == MAIN_CAST_BANK { "maincast" } else { "speech" };
                    super::mod_audio::record(&mut speech.events, super::mod_audio::EventRow { kind: super::mod_audio::EventKind::Speech, source: super::mod_audio::Source::Speech, class, slot: "", id: line.event as i32, owner: speaker, hit: None });
                    debug!("AUDIO_WORLD speech start owner={speaker} {} take {}", d.index.clips.get(line.clip).map_or("?", |c| c.name.as_str()), line.take);
                }
                Event::Cut { speaker, .. } => debug!("AUDIO_WORLD speech cut owner={speaker} (level at or below 200 for 2 s)"),
                _ => {}
            }
        }
    }
    speech.missing_logged = missing;
    // SFXObj_Speech (process `sub_824E2050`, every frame): its inputs 0 / 1 / 4 are 32767 while a
    // line on one of the stream system's first two streams (`+1120` / `+1124`: channel 0, the
    // main cast's) has a speaker (the voice id of its clip name) in 37–38 (the special cast) /
    // 75–77 (main-cast voice 77) / 1–29 (the pros), else 0. The living world's streams (2 / 3)
    // are not looked at: a living-world guard (voices 75 / 76) raises nothing. The Global ducks
    // F18 / F19 / F23 / F42 / F43 (→ the music, Global C3) and F45 (+200 mB on ped speech) read
    // them. Input 2 is the speech system's scripted-dialogue state (`+0x1BD28` == 2; requests
    // with ids ≥ 5000, `sub_824A4EE8`), input 3 a main-cast stream whose block has `+60` == 0 and
    // `+93` set: neither happens in free roam (left 0).
    let mut active: Vec<u64> = speech.player.speakers().collect();
    let cast_speakers: Vec<u64> = speech.cast.as_ref().map(|c| c.player.speakers().collect()).unwrap_or_default();
    active.extend(cast_speakers.iter().copied());
    // SFXObj_Announcer (`sub_824CF218`): `Announcer.in0` = 32767 while an announcer line plays (the
    // stream system's playing channel is 3, `sub_824A61C0`); written when it changes.
    let announcer_on = speech.announcer.as_ref().is_some_and(|c| c.player.busy() > 0);
    if announcer_on != speech.announcer_input {
        m.set_input(skate_audio::mixmap::keys::ANNOUNCER, 0, if announcer_on { 32767 } else { 0 });
        speech.announcer_input = announcer_on;
    }
    active.extend(speech.announcer.as_ref().map(|c| c.player.speakers().collect::<Vec<_>>()).unwrap_or_default());
    let mut inputs = [false; 5];
    for v in cast_speakers.iter().filter_map(|s| speech.voices.get(s)) {
        inputs[0] |= (37..=38).contains(v);
        inputs[1] |= (75..=77).contains(v);
        inputs[4] |= (1..=29).contains(v);
    }
    if inputs != speech.speech_inputs {
        for (i, on) in inputs.iter().enumerate() {
            m.set_input(skate_audio::mixmap::keys::SPEECH, i, if *on { 32767 } else { 0 });
        }
        speech.speech_inputs = inputs;
    }
    // Forget speakers that no longer play or wait.
    let waiting = speech.player.queued() > 0 || speech.cast.as_ref().is_some_and(|c| c.player.queued() > 0) || speech.announcer.as_ref().is_some_and(|c| c.player.queued() > 0);
    if !waiting {
        speech.who.retain(|id, _| active.contains(id));
        speech.voices.retain(|id, _| active.contains(id));
        speech.delay_clock.retain(|id, _| active.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_audio::world_sources::{WorldHost, WorldOwners};
    use skate_audio::mixmap::cadence::CONSOLE_DT;
    use skate_audio::world::peds::PedState;

    /// Audio content overlays on speech: a replaced take plays from the mod's WAV (with its rate
    /// and length), extra takes join the clip (the rules' take count grows, so the manager can
    /// pick them), the other clips and takes stay the export's. Without overlays the index loads
    /// as before (no mod takes).
    #[test]
    fn mod_takes_replace_and_extend_a_clip() {
        let dir = std::env::temp_dir().join(format!("skate-speech-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("livingworld.json");
        let take = serde_json::json!({"offset": 0, "size": 10, "rate": 32000, "samples": 100});
        std::fs::write(&index, serde_json::json!({
            "clips": [
                {"name": "501_59_busm1_Warn_n.dat", "id": 7, "takes": [take, take]},
                {"name": "502_59_busm1_Other_n.dat", "id": 8, "takes": [take]}
            ],
            "rules": {"bank": 1, "sub_bank": 2, "events": []}
        }).to_string()).unwrap();
        let wav = crate::game_audio::library::tests::test_wav(2205, 22050, 99);
        std::fs::write(dir.join("r.wav"), &wav).unwrap();
        std::fs::write(dir.join("e.wav"), &wav).unwrap();
        let plain = load(&index, Some(dir.clone()), &Default::default()).unwrap();
        assert!(plain.mod_takes.is_empty());
        assert_eq!(plain.index.clips[0].takes.len(), 2);
        let mut replaced = HashMap::new();
        replaced.insert(("501_59_busm1_Warn_n".to_owned(), 1u32), dir.join("r.wav"));
        let mut extra = HashMap::new();
        extra.insert("501_59_busm1_Warn_n".to_owned(), vec![dir.join("e.wav")]);
        let data = load(&index, Some(dir.clone()), &(replaced, extra)).unwrap();
        let clip = &data.index.clips[0];
        assert_eq!(clip.takes.len(), 3, "one extra take");
        assert_eq!((clip.takes[1].rate, clip.takes[1].samples), (22050, 2205), "the replacement's own header");
        assert_eq!(clip.takes[0].rate, 32000, "other takes stay");
        assert_eq!(data.mod_takes.get(&(0, 1)), Some(&dir.join("r.wav")));
        assert_eq!(data.mod_takes.get(&(0, 2)), Some(&dir.join("e.wav")));
        assert_eq!(data.index.clips[1].takes.len(), 1, "other clips stay");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// As `native::mixmap_frame`: the category gains.
    fn globals(m: &mut skate_audio::mixmap::MixMap) {
        for id in 1..=4 {
            m.set_input(skate_audio::mixmap::keys::MASTER, id, 32767);
        }
        for id in [1, 2, 5] {
            m.set_input(skate_audio::mixmap::keys::MUSIC, id, 32767);
        }
        m.set_input(skate_audio::mixmap::keys::REVERB, 5, 32767);
    }

    /// The category gains, then the tick.
    fn tick(m: &mut skate_audio::mixmap::MixMap) {
        globals(m);
        m.tick(CONSOLE_DT);
    }

    /// Ped speech end to end through the install (data-gated): a business man (voice 59) warns
    /// (value 53) 3.6 m from the camera: the manager picks a `501_59_busm1_Warn_n` take, a stream
    /// voice plays it at PedestrianSpeech out2 / 32767 with out15 as its reverb send and out13 / out14
    /// as its filters (the recomp's 24956 / 77 Hz near a speaker); 30 m away the next warn is a
    /// `_f` line at out3. Without the decode the line is chosen and nothing plays.
    #[test]
    #[ignore = "needs the private install data"]
    fn a_ped_warns_through_a_stream_at_its_owner_levels() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        if library.speech("livingworld").and_then(|s| s.1).is_none() {
            panic!("missing private data: the speech takes are not decoded (SKATE_SETUP_SPEECH=1)");
        }
        let model = library.world_tuning().ped_model(59).expect("ped models export");
        let mut host = WorldHost::default();
        let mut owners = WorldOwners::default();
        let mut speech = WorldSpeech::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let ped = 9u64;
        let speaker = Speaker { index: 59, kind: model.kind, variant: model.variant, partner: 0, word5: 0, word6: model.gender };
        let state = |z: f32, value: i32| PedState {
            position: [0.0, 1.5, z],
            speed: 0.0,
            class: 5,
            voice: 59,
            speaker,
            speech_measure: z,
            speech_limit: model.far,
            speech_value: value,
            ..Default::default()
        };
        // Past the warn's repeat (15 s) and not-follow (30 s) times: the timers start at 0.
        native.mixmap.as_mut().unwrap().ticks = 40 * 30;
        let mut seen: Vec<(u32, f32, f32, f32)> = Vec::new();
        let run_frames = |native: &mut Native, host: &mut WorldHost, owners: &mut WorldOwners, speech: &mut WorldSpeech, z: f32, frames: usize, seen: &mut Vec<(u32, f32, f32, f32)>| {
            for f in 0..frames {
                owners.peds.insert(ped, state(z, if f >= 3 { 53 } else { 0 }));
                globals(native.mixmap.as_mut().unwrap());
                crate::game_audio::world_sources::run(host, owners, native, &library, camera, &local);
                speech.peds.append(&mut host.speech_requests);
                let held = host.held().1;
                run(speech, &held, native, &library, camera, &local);
                let bank = SPEECH_BANK;
                let m = native.mixmap.as_ref().unwrap();
                let (out2, out3) = (m.level(keys::ped_speech(0), 2), m.level(keys::ped_speech(0), 3));
                let mut rt = native.shared.lock().unwrap();
                for _ in 0..7 {
                    rt.render_block();
                }
                for v in rt.mixer.snapshot().iter().filter(|v| v.bank == bank) {
                    seen.push((v.id, v.gain, out2 as f32 / 32767.0, out3 as f32 / 32767.0));
                }
            }
        };
        run_frames(&mut native, &mut host, &mut owners, &mut speech, 3.6, 40, &mut seen);
        assert!(speech.lines >= 1, "a line started");
        assert!(!seen.is_empty(), "a speech voice sounded");
        let (_, gain, out2, _) = seen[seen.len() / 2];
        assert!((gain - out2).abs() < 1e-3, "near: the stream follows out2 ({gain} vs {out2})");
        // Far: a new warn (another value first), 30 m away → a `_f` line at out3.
        owners.peds.insert(ped, state(30.0, 0));
        seen.clear();
        // Past the repeat time again.
        native.mixmap.as_mut().unwrap().ticks += 40 * 30;
        run_frames(&mut native, &mut host, &mut owners, &mut speech, 30.0, 40, &mut seen);
        let far_line = speech.data.as_ref().unwrap().index.clips.iter().any(|c| c.name.ends_with("Warn_f.dat"));
        assert!(far_line);
        assert!(!seen.is_empty(), "the far line sounded");
        // The newest voice (the near line may still be playing on the other stream).
        let newest = seen.iter().map(|s| s.0).max().unwrap();
        let far: Vec<_> = seen.iter().filter(|s| s.0 == newest).collect();
        let (_, gain, out2, out3) = *far[far.len() / 2];
        assert!((gain - out3).abs() < 1e-3 && (gain - out2).abs() > 1e-3, "far: the stream follows out3 ({gain} vs out3 {out3}, out2 {out2})");
        eprintln!("speech: {} lines; near / far gains match out2 / out3", speech.lines);
    }

    #[derive(Deserialize)]
    struct LevelRow {
        ms: f64,
        clip: Option<String>,
        geometry: Option<Geometry>,
        first: First,
        #[serde(default)]
        sends_by_module: std::collections::HashMap<String, f32>,
    }
    #[derive(Deserialize)]
    struct Geometry {
        ped: [f32; 3],
        camera: [f32; 3],
        player: [f32; 3],
    }
    #[derive(Deserialize)]
    struct First {
        gain: Option<f32>,
        send: Option<f32>,
        lpf: Option<f32>,
        hpf: Option<f32>,
        #[serde(default)]
        peak: Option<[f32; 3]>,
    }

    /// Our PedestrianSpeech values against the recomp's speech voices (data-gated: the install's
    /// MixMap and `$SKATE_SPEECH_LEVELS/levels_*.json` from the local tool `speech_levels.py`
    /// on sessions 163809 / 164620 / 180430). Per line with a joined speaker: the ped, the camera
    /// (WPPOS) and the player (PEDSEE's target) at its start drive one ped's 3DObjPos (the camera
    /// looking at the player); the stream values our port derives from the outputs (`_f` clips
    /// out3, else out2, × the voice float; the echo send out21, the env send out15; filters out13 / out14; the PEAK from the
    /// azimuth curves) are compared with the recomp's first GAIN / SEND (per send module: `+0x570`
    /// before the gain, `+0x7D0` after the filters) / LPF / HPF / PEAK targets of the voice. The
    /// PEAK is also checked without our geometry: the recomp's (centre, gain) must lie on the two
    /// curves at one azimuth. Prints every row and the agreement.
    #[test]
    #[ignore = "needs the private install data and the recomp level export"]
    fn speech_levels_follow_the_recomp() {
        let base = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let Ok(bytes) = std::fs::read(base.join("assets/private/audio/aems/MixMapSK8.mxb")) else { panic!("missing private data: no MixMap") };
        let mut rows = Vec::new();
        let levels = std::env::var_os("SKATE_SPEECH_LEVELS").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        for s in ["163809", "164620", "180430"] {
            let Some(Ok(text)) = levels.as_ref().map(|d| std::fs::read_to_string(d.join(format!("levels_{s}.json")))) else { continue };
            let list: Vec<LevelRow> = serde_json::from_str(&text).unwrap();
            rows.extend(list.into_iter().filter(|r| r.geometry.is_some() && r.first.gain.is_some()).map(|r| (s, r)));
        }
        if rows.is_empty() {
            panic!("missing private data: no speech level export (SKATE_SPEECH_LEVELS, speech_levels.py --json)");
        }
        let (mut filt_ok, mut filt_n, mut send_ok, mut send_n) = (0, 0, 0, 0);
        let (mut echo_ok, mut echo_n, mut old_ok, mut peak_on, mut peak_ours, mut peak_n) = (0, 0, 0, 0, 0, 0);
        let voice = skate_audio::world::speech_player::SpeechVoiceTuning::default();
        let world = crate::game_audio::Library::load(&base.join("assets")).ok();
        let mut ratios = Vec::new();
        for (session, r) in &rows {
            let g = r.geometry.as_ref().unwrap();
            let mut m = skate_audio::mixmap::MixMap::from_bytes(&bytes).unwrap();
            let view = {
                let d = [g.player[0] - g.camera[0], 0.0, g.player[2] - g.camera[2]];
                let n = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-3);
                [d[0] / n, 0.0, d[2] / n]
            };
            let l = Listener { camera: g.camera, view, camera_velocity: [0.0; 3], followed: g.player, facing: view, followed_velocity: [0.0; 3] };
            let mut pos = ObjPos::default();
            for _ in 0..40 {
                pos.write(&mut m, keys::ped_pos(0), &l, Some((g.ped, [0.0; 3])));
                tick(&mut m);
            }
            let far = r.clip.as_deref().is_some_and(speech_player::far_clip);
            let out = OutputsSnapshot::take(&m, keys::ped_speech(0), &speech_player::PED_SNAPSHOT_FILTERS);
            // The speaker's per-voice float (`S+152`) from the clip name's voice.
            let scale = r
                .clip
                .as_deref()
                .and_then(skate_audio::world::speech::parse_name)
                .and_then(|(_, v, _, _)| world.as_ref().and_then(|l| l.world_tuning().ped_model(v)))
                .map_or(1.0, |m| if m.pitch > 0.0 { m.pitch } else { 1.0 });
            let p = speech_player::ped_outputs(&out, PedLevelSelect::default(), 0, far, &voice, scale);
            let gain = r.first.gain.unwrap();
            if p.gain > 1e-3 && gain > 1e-3 {
                ratios.push(gain / p.gain);
            }
            if let (Some(lpf), Some(hpf)) = (r.first.lpf, r.first.hpf) {
                filt_n += 1;
                filt_ok += usize::from((lpf - p.lpf).abs() <= 0.1 * lpf.max(1.0) && (hpf - p.hpf).abs() <= 0.1 * hpf.max(10.0));
            }
            let near = |a: f32, b: f32| (a - b).abs() <= 0.01_f32.max(0.25 * a.abs());
            if let Some(&send) = r.sends_by_module.get("570") {
                send_n += 1;
                send_ok += usize::from(near(send, p.echo));
                // The first port's reading (out15 as this send).
                old_ok += usize::from(near(send, p.send));
            }
            if let Some(&env) = r.sends_by_module.get("7D0") {
                echo_n += 1;
                echo_ok += usize::from(near(env, p.send));
            }
            if let Some([freq, gain, _]) = r.first.peak {
                peak_n += 1;
                // The azimuth the recomp's gain says (the gain curve falls from 0.4 to 0.1), then
                // the centre the other curve gives there.
                let (mut lo, mut hi) = (0.0f32, 32767.0f32);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if voice.peak_gain.eval(mid) > gain { lo = mid } else { hi = mid }
                }
                let at = voice.peak_freq.eval(0.5 * (lo + hi));
                peak_on += usize::from((at - freq).abs() <= 0.02 * freq);
                peak_ours += usize::from((p.peak[0] - freq).abs() <= 0.1 * freq && (p.peak[1] - gain).abs() <= 0.1 * gain);
                eprintln!("    PEAK recomp {freq:.0} Hz x {gain:.4} (on the curves: {at:.0} Hz at that gain) ours {:.0} Hz x {:.4}", p.peak[0], p.peak[1]);
            }
            if std::env::var("EXPLORE").is_ok() {
                use skate_audio::player::Outputs as _;
                let lv: Vec<String> = (0..26).map(|i| format!("{i}:{:.4}", out.level(i) as f32 / 32767.0)).collect();
                eprintln!("  sends {:?} raw0 {} peak {:?} | {}", r.sends_by_module, out.raw(0), r.first.peak, lv.join(" "));
            }
            let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
            eprintln!(
                "{session} {:7.1}s cam {:5.1} m skater {:5.1} m {:>32}: gain recomp {gain:.3} ours {:.3} | echo send (+570) {:?} {:.3} env send (+7D0) {:?} {:.3} | lpf {:?} {:.0} hpf {:?} {:.0}",
                r.ms / 1000.0,
                d(g.ped, g.camera),
                d(g.ped, g.player),
                r.clip.as_deref().unwrap_or("?"),
                p.gain,
                r.sends_by_module.get("570"),
                p.echo,
                r.sends_by_module.get("7D0"),
                p.send,
                r.first.lpf,
                p.lpf,
                r.first.hpf,
                p.hpf
            );
        }
        ratios.sort_by(f32::total_cmp);
        let median = ratios.get(ratios.len() / 2).copied().unwrap_or(0.0);
        eprintln!("{} lines: filters within 10 % in {filt_ok} of {filt_n}; the echo send (+0x570) = out21 within 25 % (or 0.01) in {send_ok} of {send_n} (as out15: {old_ok}); the env send (+0x7D0) = out15 in {echo_ok} of {echo_n}; PEAK on the curves in {peak_on} of {peak_n}, ours within 10 % in {peak_ours}; gain recomp / ours median {median:.3} (p10 {:.3} p90 {:.3}, n {})", rows.len(), ratios.get(ratios.len() / 10).copied().unwrap_or(0.0), ratios.get(ratios.len() * 9 / 10).copied().unwrap_or(0.0), ratios.len());
        assert!(filt_n == 0 || filt_ok * 10 >= filt_n * 6, "the filters follow out13 / out14");
        assert!(send_ok > old_ok && echo_ok * 2 >= echo_n, "the two sends follow out21 / out15");
        assert_eq!(peak_on, peak_n, "every recomp PEAK lies on the curves");
    }

    #[derive(Deserialize)]
    struct CastRow {
        session: String,
        ms: f64,
        clip: String,
    }

    /// Every main-cast line the recomp streamed (`maincast_reads.py --json`:
    /// `$SKATE_MAINCAST_LINES/maincast_lines.json`, 82 lines in 11 sessions) is reachable through
    /// our port: the speaker's model speaks on the main cast (its cast bit / word from
    /// `world_tuning.ped_models`), and a record of the clip's event matches the words
    /// `main_cast::request_words` builds for it (near or far; the pro-on-pro lines with some other
    /// main-cast model) with the clip among its clips. Lines whose event the port sends are counted
    /// apart (130 `_col`, the skater-collision line, has no ported sender).
    #[test]
    #[ignore = "needs the private install data and the recomp main-cast export"]
    fn main_cast_lines_are_reachable() {
        use skate_audio::world::speech_rules::matches;
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Some((index_path, _)) = library.speech("maincast") else { panic!("missing private data: no main-cast index (stage_world_audio.py --maincast)") };
        let dir = std::env::var_os("SKATE_MAINCAST_LINES").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let Some(Ok(text)) = dir.map(|d| std::fs::read_to_string(d.join("maincast_lines.json"))) else { panic!("missing private data: SKATE_MAINCAST_LINES (maincast_reads.py --json)") };
        let rows: Vec<CastRow> = serde_json::from_str(&text).unwrap();
        let json: IndexJson = serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
        let ids: HashMap<String, u16> = json.clips.iter().filter_map(|c| Some((c.name.clone(), c.id?))).collect();
        let data = load(&index_path, None, &Default::default()).unwrap();
        let world = library.world_tuning();
        let cast: Vec<(u32, (u32, u32, u32))> = (1..=96).filter_map(|v| Some((v, world.ped_model(v)?.main_cast()?))).collect();
        // The events the port sends on the main cast (skater_speech, the messages, the ped values).
        let sent = [0u16, 1, 16, 287, 288, 115, 125, 6, 141, 11, 77, 253, 250, 247];
        let (mut ok, mut ported, mut missing) = (0, 0, Vec::new());
        for r in &rows {
            let Some((number, voice, _, _)) = skate_audio::world::speech::parse_name(&r.clip) else { continue };
            let event = data.table.events.iter().find(|e| e.name.split('_').next() == Some(&number.to_string())).map(|e| e.id);
            let words_of = cast.iter().find(|(v, _)| *v == voice).map(|c| c.1);
            let (Some(event), Some(words), Some(&id)) = (event, words_of, ids.get(&r.clip)) else {
                missing.push(format!("{} {:.1}s {} (event {event:?}, cast {words_of:?})", r.session, r.ms / 1000.0, r.clip));
                continue;
            };
            let ev = data.table.event(event).unwrap();
            let others: Vec<Option<(u32, u32, u32)>> = if matches!(event, 254 | 287 | 288) { cast.iter().map(|c| Some(c.1)).collect() } else { vec![None] };
            let hit = [false, true].iter().any(|far| {
                others.iter().any(|o| {
                    let w = main_cast::request_words(event, &cast_block(words, *far, *o));
                    ev.records.iter().any(|rec| rec.clips.iter().any(|c| c.id == id) && matches(ev, rec, data.table.packed(event), &w))
                })
            });
            if hit {
                ok += 1;
                ported += usize::from(sent.contains(&event));
            } else {
                missing.push(format!("{} {:.1}s {} (event {event})", r.session, r.ms / 1000.0, r.clip));
            }
        }
        for m in &missing {
            eprintln!("  not reachable: {m}");
        }
        eprintln!("{} recorded main-cast lines: {ok} reachable through our words ({ported} of an event the port sends)", rows.len());
        assert!(!rows.is_empty());
        assert!(missing.is_empty(), "{} lines not reachable", missing.len());
    }

    /// A pro (Ryan Smith, model 24) as an NPC skater sees the player's trick: its speech process
    /// sends main-cast event 0 (`101_pos`) and the main-cast channel streams a `101_24_Smit_pos`
    /// take at its PlayerSpeech owner's level (data-gated: the install with the main-cast decode).
    #[test]
    #[ignore = "needs the private install data"]
    fn a_pro_skater_says_a_main_cast_line() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        if library.speech("maincast").and_then(|s| s.1).is_none() {
            panic!("missing private data: no main-cast decode (stage_world_audio.py --maincast decode)");
        }
        let mut speech = WorldSpeech::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let id = 77u64;
        let mut seen = Vec::new();
        // Past the events' repeat times (the manager's timers start at 0 at boot).
        native.mixmap.as_mut().unwrap().ticks += 30 * 120;
        for frame in 0..60 {
            let reactions = Reactions { trick: frame == 2, ..Default::default() };
            speech.skaters = vec![SkaterSpeaker { id, voice: 24, position: [0.0, 0.0, 4.0], velocity: [0.0; 3], reactions }];
            {
                let m = native.mixmap.as_mut().unwrap();
                tick(m);
            }
            run(&mut speech, &[], &mut native, &library, camera, &local);
            let mut rt = native.shared.lock().unwrap();
            for _ in 0..3 {
                rt.render_block();
            }
            for v in rt.mixer.snapshot().iter().filter(|v| v.bank == MAIN_CAST_BANK) {
                seen.push((v.id, v.gain));
            }
        }
        let cast = speech.cast.as_ref().expect("the main-cast channel loaded");
        assert!(speech.lines >= 1, "a line started");
        assert!(!seen.is_empty(), "a main-cast voice sounded");
        assert!(seen.iter().any(|s| s.1 > 0.0), "audible");
        eprintln!("main cast: {} lines, {} voice frames, started {}", speech.lines, seen.len(), cast.player.started);
    }

    /// A pro ped's values 30 / 51 stop its playing main-cast line before the new request
    /// (`sub_824AC438`), and SFXObj_Speech's input 4 (a pro on the main cast's streams) is raised
    /// while the line plays, input 1 never (data-gated: the install with the main-cast decode).
    #[test]
    #[ignore = "needs the private install data with the main-cast decode"]
    fn a_pro_ped_s_impact_value_stops_its_line_first() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        if library.speech("maincast").and_then(|s| s.1).is_none() {
            panic!("missing private data: no main-cast decode (stage_world_audio.py --maincast decode)");
        }
        let mut speech = WorldSpeech::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let owner = 501u64;
        native.mixmap.as_mut().unwrap().ticks += 30 * 120;
        let (mut in4_frames, mut in1_frames) = (0, 0);
        for frame in 0..12 {
            // 51 (a knock: `202_impact_react` / `201_Light_Impact_Grunt`), then 30 while it plays.
            let value = match frame {
                0 => Some(51),
                6 => Some(30),
                _ => None,
            };
            if let Some(value) = value {
                speech.peds.push(PedRequest { request: SpeechRequest { owner, value, flag: skate_audio::world::speech_manager::NEAR }, voice: 24, speaker: Speaker::default(), level: PedLevelSelect::default() });
            }
            if frame == 6 {
                assert!(speech.cast.as_ref().is_some_and(|c| c.player.speaking(owner)), "the first line plays when the second value comes");
            }
            tick(native.mixmap.as_mut().unwrap());
            run(&mut speech, &[(owner, 0)], &mut native, &library, camera, &local);
            let m = native.mixmap.as_ref().unwrap();
            in4_frames += usize::from(m.input(skate_audio::mixmap::keys::SPEECH, 4) == 32767);
            in1_frames += usize::from(m.input(skate_audio::mixmap::keys::SPEECH, 1) == 32767);
        }
        let cast = speech.cast.as_ref().expect("the main-cast channel loaded");
        eprintln!("pro ped: {} lines, stopped {}, started {}, in4 on {in4_frames} frames", speech.lines, cast.player.stopped, cast.player.started);
        assert_eq!(cast.player.stopped, 1, "value 30 stopped the playing line");
        assert!(speech.lines >= 2 && cast.player.speaking(owner), "and its own line followed");
        assert!(in4_frames >= 10 && in1_frames == 0, "input 4 follows the pro's main-cast line; input 1 stays off");
    }

    /// An NPC pro (Ryan Smith, model 24: announcer pro word 0x400000, `480_36_slam_pro_Smit`) crashes
    /// 4 m from the camera (data-gated: the install with the announcer decode). Free skate: the
    /// announcer is asked (`480_slam_pro`) and no announcer voice plays. With announcer 36 named: the
    /// `480_36_slam_pro_Smit` line streams on the announcer bank at the Announcer object's out2 × 1.25,
    /// with `Announcer.in0` up while it plays. 20 m away a crash does not ask. A mod-style request by
    /// name (`475`, `475_BigAir_A`) reaches the gate (refused: it wants the player at 10 km/h or more).
    #[test]
    #[ignore = "needs the private install data with the announcer decode"]
    fn a_pro_crash_near_the_camera_asks_the_announcer() {
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let Ok(library) = crate::game_audio::Library::load(root) else { panic!("missing private data: no audio install") };
        let Ok(mut native) = Native::start(&library) else { panic!("missing private data: no AEMS install") };
        if library.speech("announcer").and_then(|s| s.1).is_none() {
            panic!("missing private data: no announcer decode (stage_world_audio.py --announcer decode)");
        }
        assert_eq!(library.world_tuning().ped_model(24).map(|m| m.announcer_pro), Some(0x40_0000), "the export's announcer pro word");
        let mut speech = WorldSpeech::default();
        let local = skate_audio::player::AudioState::default();
        let camera = Some(([0.0, 1.5, 0.0], [0.0, 0.0, 1.0]));
        let id = 78u64;
        native.mixmap.as_mut().unwrap().ticks += 30 * 120;
        // (announcer voices seen, gains, frames with Announcer.in0 up, expected gain from out2)
        let crash = |speech: &mut WorldSpeech, native: &mut Native, z: f32, frames: usize| {
            let (mut voices, mut in0, mut gains) = (0, 0, Vec::new());
            for frame in 0..frames {
                let reactions = Reactions { crash: (2..6).contains(&frame), ..Default::default() };
                speech.skaters = vec![SkaterSpeaker { id, voice: 24, position: [0.0, 1.5, z], velocity: [0.0; 3], reactions }];
                tick(native.mixmap.as_mut().unwrap());
                run(speech, &[], native, &library, camera, &local);
                let m = native.mixmap.as_ref().unwrap();
                in0 += usize::from(m.input(skate_audio::mixmap::keys::ANNOUNCER, 0) == 32767);
                let expected = ((m.level(skate_audio::mixmap::keys::ANNOUNCER, 2) as f32 * 1.25) as i32).clamp(0, 32767) as f32 / 32767.0;
                let mut rt = native.shared.lock().unwrap();
                for _ in 0..3 {
                    rt.render_block();
                }
                for v in rt.mixer.snapshot().iter().filter(|v| v.bank == ANNOUNCER_BANK) {
                    voices += 1;
                    gains.push((v.gain, expected));
                }
            }
            (voices, in0, gains)
        };
        // Free skate.
        let (voices, in0, _) = crash(&mut speech, &mut native, 4.0, 30);
        let a = speech.announcer.as_ref().expect("the announcer channel loaded");
        assert_eq!((speech.announcer_asked, a.player.started, voices, in0), (1, 0, 0, 0), "free skate: asked once, nothing plays");
        // A challenge with announcer 36.
        speech.announcer_character = Some(36);
        speech.announcer_since = 0;
        let (voices, in0, gains) = crash(&mut speech, &mut native, 4.0, 90);
        let a = speech.announcer.as_ref().unwrap();
        eprintln!("announcer 36: asked {}, started {}, voice frames {voices}, in0 frames {in0}, gains {:?}", speech.announcer_asked, a.player.started, gains.get(gains.len() / 2));
        assert_eq!((speech.announcer_asked, a.player.started), (2, 1));
        assert!(voices > 0 && in0 > 0, "the line streams and ducks the mix");
        assert!(gains.iter().all(|(g, e)| (g - e).abs() < 1e-3), "out2 × 1.25");
        // Far: no request.
        native.mixmap.as_mut().unwrap().ticks += 30 * 60;
        crash(&mut speech, &mut native, 20.0, 10);
        assert_eq!(speech.announcer_asked, 2, "20 m: beyond the crash distance");
        // A request by name (announcer 35's `475_BigAir_A`): asked, and gated by the player's speed (min 10 km/h).
        speech.announcer_character = Some(35);
        speech.announces.push(AnnouncerRequest { event: crate::world_audio::AnnouncerLine::Name("475".into()), pro: None, words: Vec::new() });
        crash(&mut speech, &mut native, 20.0, 2);
        assert_eq!(speech.announcer_asked, 3);
    }
}
