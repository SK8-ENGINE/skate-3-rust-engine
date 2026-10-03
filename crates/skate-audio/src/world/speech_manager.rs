//! The game's speech manager for living-world peds (`Sk8::Audio::TheSpeechSystem`, recomp
//! `sub_824AB6C8` → `sub_824ABA18` → `sub_824ABD90`; `world-speech.md` "Speech manager").
//!
//! [`super::peds::PedSpeech`] hands the manager a speech value and the near / far flag whenever the
//! ped's state graph sends a new value. The manager then works in four steps:
//! 1. [`event_for_value`] maps the value to an `.evt` event. Some values pick between two events with
//!    `rand()`. For value 1 / 3 the speaker type decides: security guards say radio lines, bums their
//!    own lines.
//! 2. [`SpeechManager::gate`] applies the event's vault tuning ([`EventTuning`],
//!    `Sk8::Audio::tSpeechTuning`):
//!    - per-speaker timers: the same event again after `repeat` s, any line after `gap` s, and the
//!      not-follow list ("not within t s after event e");
//!    - a `rand() % 1000` probability draw;
//!    - the player's speed window;
//!    - excluded challenge types;
//!    - zombie mode.
//! 3. [`request_words`] builds the words the library matches records against:
//!    - word 1 = speaker type bit (audio state `+96`);
//!    - word 2 = voice variant bit (`+88`);
//!    - word 3 = the flag: 1 = far (`_f` / `_far` lines), 2 = near (`_n` / `_near`);
//!    - word 4 = the conversation partner's type (`+92`);
//!    - word 10 = 2 in zombie mode, else 1.
//!
//!    Which words an event reads is per event (`sub_824ABD90`).
//! 4. [`super::speech_rules::Library::start`] picks the record (the line) and the takes. On success
//!    the speaker's timers restart (`sub_824A90B8`, called when the line starts).
//!
//! The far / near meaning of the flag comes from the `.evt` data itself: the records with flag 1
//! name `101_51_GenPos_Grn1_far` / `104_51_grn1_Slam_far`, those with 2 `…_near`, and every
//! other `_f` / `_n` pair splits the same way (`.claude/notes/world-speech.md`). Which ped values
//! `+148` / `+156` compare is not traced.
//!
//! **Not ported:**
//! - the interrupt rules (`sub_824A73F0`: tuning `+13` / `+14` against the playing line's priority);
//! - the main-cast path for peds without a living-world speaker (`+116 == 0`: pros, `sub_824AC438`);
//! - the extra main-cast line of value 6 (`+71`);
//! - the speech level (`world-speech.md` "Level").
use std::collections::HashMap;

use super::Draw;
use super::speech::SpeechIndex;
use super::speech_rules::{EventTable, Library, NoLine, Pick};

/// Speaker type bits (audio state `+96`, request word 1). Named from the clip voice names of the
/// records that carry each bit. The code itself only tests 64 (radio) and 16384 (bum lines).
pub mod kind {
    pub const ADULT_MALE: u32 = 0x1;
    pub const ADULT_FEMALE: u32 = 0x2;
    pub const GRANNY: u32 = 0x4;
    pub const JOCK: u32 = 0x8;
    pub const TEEN_FEMALE: u32 = 0x10;
    pub const TEEN_MALE: u32 = 0x20;
    pub const SECURITY_GUARD: u32 = 0x40;
    pub const TOURIST_MALE: u32 = 0x80;
    pub const TOURIST_FEMALE: u32 = 0x100;
    pub const SKATER_MALE: u32 = 0x200;
    pub const SKATER_FEMALE: u32 = 0x400;
    pub const BUSINESS_MAN: u32 = 0x1000;
    pub const BUSINESS_WOMAN: u32 = 0x2000;
    pub const BUM: u32 = 0x4000;
}

/// The near / far flag (request word 3): `SFXObj_PedestrianSpeech` sends 1 when the ped's `+148`
/// exceeds `+156`.
pub const FAR: i32 = 1;
pub const NEAR: i32 = 2;

/// `sub_824AB6C8`: the event of a speech value on the living-world path, or `None` (the value says
/// nothing). `rand()` draws come from `rng` (retail's C runtime `rand`, `sub_82A8AF10`).
pub fn event_for_value(value: i32, speaker_kind: u32, rng: &mut dyn Draw) -> Option<u16> {
    let odd = |rng: &mut dyn Draw| rng.draw() & 1 == 1;
    Some(match value {
        2 => 8214,                                     // 1901_shout
        1 | 3 => match speaker_kind {
            kind::SECURITY_GUARD => 8277,              // 102_Radio
            kind::BUM => match rng.draw() % 3 {
                1 => 8247,                             // 206_pan_handle
                2 => 8248,                             // 207_random_bum
                _ => 8214,                             // 1901_shout
            },
            _ => return None,
        },
        4 | 5 => 8208,                                 // 204_gasp
        6 => 8207,                                     // 202_impact_react
        10 => if odd(rng) { 8220 } else { 8209 },      // 203_concern / 205_spec_collision
        11 | 16 | 60 => 8196,                          // 605_chase_terminate
        12 => 8201,                                    // 611_cool_down
        14 => if odd(rng) { 8194 } else { 8198 },      // 603_Chase_Start / 607_chase_join
        15 => 8198,                                    // 607_chase_join
        17 => 8199,                                    // 609_lunge
        18 => 8195,                                    // 604_hot_pursuit
        19 | 65 => 8197,                               // 606_catch
        20..=22 => if odd(rng) { 8224 } else { 8217 }, // 108_flee / 109_help
        23 | 24 => 8202,                               // 101_pos
        25 => 8204,                                    // 104_spec_slam
        33 => 8269,                                    // 550_Intro_Short
        34 => 8271,                                    // 551_Intro_Question
        35 => 8272,                                    // 552_Intro_Statement
        36 => 8273,                                    // 553_Opinion
        37 => 8274,                                    // 554_Statement
        38 => 8275,                                    // 555_Question
        39 => 8270,                                    // 556_Answer
        40 => 8276,                                    // 557_Outro
        48 => 8211,                                    // 805_conv_cell
        50 => 8216,                                    // 4405_cell_bye
        51 => 8206,                                    // 201_grunt
        53 | 54 => 8210,                               // 501_warn
        56 => 8212,                                    // 806_cls_interact
        63 => 8213,                                    // 807_cls_react
        64 => 8215,                                    // 4402_cell_greet
        66 => 8298,                                    // 330_TzerStart
        67 => 8299,                                    // 331_TzerDone
        _ => return None,
    })
}

/// What the ped's audio state holds for its speech.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Speaker {
    /// `+84`: the speaker slot the manager keeps timers for (per event and any event).
    pub index: u32,
    /// `+96`: one of [`kind`].
    pub kind: u32,
    /// `+88`: the voice variant bit within the type (1, 2, 4, 8 or 16).
    pub variant: u32,
    /// `+92`: the conversation partner's type bit (806 / 807 records).
    pub partner: u32,
    /// `+120` / `+124`: request words 5 / 6 (the conversation events 550–557 read word 5).
    pub word5: u32,
    pub word6: u32,
}

/// The request block (`sub_824D9908` builds words 1–6, `sub_824ABA18` sets word 10).
pub fn request_block(speaker: &Speaker, flag: i32, zombie: bool) -> [u32; 10] {
    [speaker.kind, speaker.variant, flag as u32, speaker.partner, speaker.word5, speaker.word6, 0, 0, 0, if zombie { 2 } else { 1 }]
}

/// `sub_824ABD90`: the words an event's library request carries (the library matches record field
/// `k` against word `k`).
pub fn request_words(event: u16, block: &[u32; 10]) -> Vec<u32> {
    let w = block;
    match event {
        8194 => vec![w[0], w[1], w[2], w[9]],
        8197 | 8199 | 8206 | 8214 => vec![w[0], w[1], w[9]],
        8202 | 8204 | 8207 | 8210 | 8220 => vec![w[0], w[1], w[2]],
        8212 | 8213 => vec![w[0], w[1], w[3]],
        8269..=8276 => vec![w[0], w[1], w[4]],
        8282 | 8288 => vec![w[0], w[1], w[6]],
        8291 => vec![w[0], w[1], w[7]],
        _ => vec![w[0], w[1]],
    }
}

/// One event's vault tuning (`Sk8::Audio::tSpeechTuning` + the not-follow list + the excluded
/// challenge types; `tools/asset_pipeline/world_audio.speech_tuning`).
#[derive(Clone, Debug, PartialEq)]
pub struct EventTuning {
    /// `+8`: seconds since this speaker's last line of any event.
    pub gap: f32,
    /// `+16`: priority (the interrupt rules, not ported).
    pub priority: i32,
    /// `+20`: percent.
    pub probability: f32,
    /// `+24`: seconds since this speaker last said this event.
    pub repeat: f32,
    /// `+32` / `+36`: the player's speed (km/h) must be at least / at most this (0 = no limit).
    pub min_player_kmh: f32,
    pub max_player_kmh: f32,
    /// `+40` / `+44`: two manager timers (meaning not traced; 0 for every living-world event).
    pub timer_40: f32,
    pub timer_44: f32,
    /// `+49..+51`: block the line while game flags 1..3 are set (0 for every living-world event).
    pub blocked_by: [bool; 3],
    /// `+60`: the line may play in zombie mode.
    pub zombie: bool,
    /// (event, s): not within `s` seconds after this speaker said `event` (at most 10 read).
    pub not_follow: Vec<(u16, f32)>,
    /// No line while one of these challenge types runs.
    pub challenges: Vec<i32>,
}

impl Default for EventTuning {
    /// The vault's `default` record (events without their own tuning inherit it).
    fn default() -> Self {
        Self {
            gap: 0.0,
            priority: 50,
            probability: 100.0,
            repeat: 0.0,
            min_player_kmh: 0.0,
            max_player_kmh: 0.0,
            timer_40: 0.0,
            timer_44: 0.0,
            blocked_by: [false; 3],
            zombie: false,
            not_follow: Vec::new(),
            challenges: Vec::new(),
        }
    }
}

/// The game state the gate reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateInputs {
    /// The manager's clock (s). Retail's timers start at 0, so a speaker's first line of an event
    /// needs `now >= repeat`.
    pub now: f64,
    /// The local player's speed (m/s; the tuning compares km/h).
    pub player_speed: f32,
    /// The running challenge type, `None` in free skate.
    pub challenge: Option<i32>,
    pub zombie: bool,
    /// The three game flags `+49..+51` test, and the two manager timers `+40` / `+44` compare.
    pub game_flags: [bool; 3],
    pub timer_a: f32,
    pub timer_b: f32,
}

impl Default for GateInputs {
    fn default() -> Self {
        Self { now: 0.0, player_speed: 0.0, challenge: None, zombie: false, game_flags: [false; 3], timer_a: f32::MAX, timer_b: f32::MAX }
    }
}

/// Why a request played nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The value maps to no event for this speaker.
    NoEvent,
    Repeat,
    Gap,
    NotFollow(u16),
    Probability,
    PlayerSpeed,
    Challenge,
    Timer,
    GameFlag,
    Zombie,
    Library(NoLine),
}

/// A line to play: the event and its clips in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub event: u16,
    pub picks: Vec<Pick>,
}

/// Not-follow ids the gate skips (`sub_824A8C78`).
const NOT_FOLLOW_SKIP: [u16; 5] = [0, 294, 8318, 33245, 24752];

/// The manager's per-speaker timers and the living world's tuning.
#[derive(Clone, Debug, Default)]
pub struct SpeechManager {
    pub tuning: HashMap<u16, EventTuning>,
    /// (speaker, event) → when that speaker last started a line of it (retail keeps f32).
    last_event: HashMap<(u32, u16), f32>,
    /// speaker → when it last started any line.
    last_any: HashMap<u32, f32>,
}

impl SpeechManager {
    pub fn new(tuning: HashMap<u16, EventTuning>) -> Self {
        Self { tuning, ..Default::default() }
    }

    fn since(&self, now: f64, last: Option<&f32>) -> f64 {
        now - f64::from(last.copied().unwrap_or(0.0))
    }

    /// `sub_824A8C78` + `sub_824A75F0`: may `speaker` say `event` now?
    pub fn gate(&self, speaker: u32, event: u16, t: &EventTuning, inputs: &GateInputs, rng: &mut dyn Draw) -> Result<(), Refusal> {
        let now = inputs.now;
        if self.since(now, self.last_event.get(&(speaker, event))) < f64::from(t.repeat) {
            return Err(Refusal::Repeat);
        }
        if t.gap > 0.0 && self.since(now, self.last_any.get(&speaker)) < f64::from(t.gap) {
            return Err(Refusal::Gap);
        }
        for (other, seconds) in t.not_follow.iter().take(10) {
            if NOT_FOLLOW_SKIP.contains(other) {
                continue;
            }
            if self.since(now, self.last_event.get(&(speaker, *other))) <= f64::from(*seconds) {
                return Err(Refusal::NotFollow(*other));
            }
        }
        // `rand() % 1000 × 0.1` in f32; 99.9 always passes.
        let roll = (rng.draw() % 1000) as f32 * 0.1f32;
        if !(roll < t.probability || roll >= 99.9f32) {
            return Err(Refusal::Probability);
        }
        let kmh = inputs.player_speed * 3.6f32;
        if (t.min_player_kmh > 0.0 && kmh < t.min_player_kmh) || (t.max_player_kmh > 0.0 && kmh > t.max_player_kmh) {
            return Err(Refusal::PlayerSpeed);
        }
        if inputs.challenge.is_some_and(|c| t.challenges.contains(&c)) {
            return Err(Refusal::Challenge);
        }
        if (t.timer_40 > 0.0 && inputs.timer_a < t.timer_40) || (t.timer_44 > 0.0 && inputs.timer_b < t.timer_44) {
            return Err(Refusal::Timer);
        }
        if t.blocked_by.iter().zip(inputs.game_flags).any(|(b, f)| *b && f) {
            return Err(Refusal::GameFlag);
        }
        if inputs.zombie && !t.zombie {
            return Err(Refusal::Zombie);
        }
        Ok(())
    }

    /// `sub_824A90B8`: the speaker started a line of `event`.
    pub fn started(&mut self, speaker: u32, event: u16, now: f64) {
        self.last_event.insert((speaker, event), now as f32);
        self.last_any.insert(speaker, now as f32);
    }

    /// A ped's request end to end: value → event, gate, request words, line and takes.
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &mut self,
        library: &mut Library,
        table: &EventTable,
        value: i32,
        flag: i32,
        speaker: &Speaker,
        inputs: &GateInputs,
        rng: &mut dyn Draw,
    ) -> Result<Line, Refusal> {
        let event = event_for_value(value, speaker.kind, rng).ok_or(Refusal::NoEvent)?;
        let default = EventTuning::default();
        let tuning = self.tuning.get(&event).unwrap_or(&default);
        self.gate(speaker.index, event, tuning, inputs, rng)?;
        let words = request_words(event, &request_block(speaker, flag, inputs.zombie));
        let picks = library.start(table, event, &words).map_err(Refusal::Library)?;
        self.started(speaker.index, event, inputs.now);
        Ok(Line { event, picks })
    }
}

/// The (type bit, variant bit) the records give a clip voice id (the number in the clip names),
/// for hosts that know a ped's voice but not its bits: the pair most records with that voice's
/// clips carry in their first two fields.
pub fn speaker_bits(table: &EventTable, index: &SpeechIndex, voice: u32) -> Option<(u32, u32)> {
    let mut count: HashMap<(u32, u32), usize> = HashMap::new();
    for ev in &table.events {
        if ev.fields.len() < 2 || ev.fields[0] != 1 || ev.fields[1] != 2 {
            continue;
        }
        for r in &ev.records {
            let (Some(&kind), Some(&variant)) = (r.values.first(), r.values.get(1)) else { continue };
            if kind == 0 || variant == 0 {
                continue;
            }
            let voiced = r.clips.iter().filter_map(|c| index.clip_by_id(c.id)).any(|i| index.clips[i].voice == voice && index.clips[i].event != 497);
            if voiced {
                *count.entry((kind, variant)).or_default() += 1;
            }
        }
    }
    count.into_iter().max_by_key(|(bits, n)| (*n, std::cmp::Reverse(*bits))).map(|(bits, _)| bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::speech_rules::{ClipHeader, ClipRef, Event, Record};

    fn seq(values: Vec<u32>) -> impl FnMut() -> u32 {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    #[test]
    fn values_map_to_events_with_retails_coin_flips() {
        let mut odd = seq(vec![1]);
        let mut even = seq(vec![2]);
        assert_eq!(event_for_value(10, kind::BUSINESS_MAN, &mut odd), Some(8220));
        assert_eq!(event_for_value(10, kind::BUSINESS_MAN, &mut even), Some(8209));
        assert_eq!(event_for_value(54, kind::JOCK, &mut even), Some(8210));
        assert_eq!(event_for_value(1, kind::SECURITY_GUARD, &mut even), Some(8277));
        assert_eq!(event_for_value(1, kind::JOCK, &mut even), None);
        let mut three = seq(vec![3, 4, 5]);
        let bum: Vec<_> = (0..3).map(|_| event_for_value(3, kind::BUM, &mut three)).collect();
        assert_eq!(bum, vec![Some(8214), Some(8247), Some(8248)]);
        assert_eq!(event_for_value(29, kind::JOCK, &mut even), None, "photographer: nothing on this path");
    }

    #[test]
    fn events_read_their_own_request_words() {
        let s = Speaker { index: 3, kind: kind::JOCK, variant: 2, partner: kind::BUM, word5: 0x40, word6: 0 };
        let b = request_block(&s, FAR, true);
        assert_eq!(request_words(8210, &b), vec![kind::JOCK, 2, 1]);
        assert_eq!(request_words(8214, &b), vec![kind::JOCK, 2, 2], "1901 reads the zombie word");
        assert_eq!(request_words(8212, &b), vec![kind::JOCK, 2, kind::BUM]);
        assert_eq!(request_words(8194, &b), vec![kind::JOCK, 2, 1, 2]);
        assert_eq!(request_words(8270, &b), vec![kind::JOCK, 2, 0x40]);
        assert_eq!(request_words(8209, &b), vec![kind::JOCK, 2]);
    }

    #[test]
    fn the_gate_applies_per_speaker_timers_and_the_probability() {
        let mut m = SpeechManager::default();
        let t = EventTuning { repeat: 15.0, gap: 5.0, not_follow: vec![(8194, 30.0), (294, 99.0)], ..Default::default() };
        let mut rng = seq(vec![0]);
        let at = |now| GateInputs { now, ..Default::default() };
        assert_eq!(m.gate(1, 8210, &t, &at(10.0), &mut rng), Err(Refusal::Repeat), "timers start at 0");
        assert_eq!(m.gate(1, 8210, &t, &at(40.0), &mut rng), Ok(()));
        m.started(1, 8210, 40.0);
        assert_eq!(m.gate(1, 8210, &t, &at(50.0), &mut rng), Err(Refusal::Repeat));
        assert_eq!(m.gate(2, 8210, &t, &at(50.0), &mut rng), Ok(()), "another speaker");
        m.started(1, 8194, 52.0);
        assert_eq!(m.gate(1, 8210, &t, &at(56.0), &mut rng), Err(Refusal::Gap));
        assert_eq!(m.gate(1, 8210, &t, &at(70.0), &mut rng), Err(Refusal::NotFollow(8194)));
        assert_eq!(m.gate(1, 8210, &t, &at(82.5), &mut rng), Ok(()));
        let half = EventTuning { probability: 50.0, ..Default::default() };
        assert_eq!(m.gate(1, 8214, &half, &at(100.0), &mut seq(vec![499])), Ok(()));
        assert_eq!(m.gate(1, 8214, &half, &at(100.0), &mut seq(vec![500])), Err(Refusal::Probability));
        assert_eq!(m.gate(1, 8214, &EventTuning { probability: 0.0, ..Default::default() }, &at(100.0), &mut seq(vec![999])), Ok(()), "999 always passes");
        let zombie = GateInputs { zombie: true, ..at(100.0) };
        assert_eq!(m.gate(1, 8210, &t, &zombie, &mut rng), Err(Refusal::Zombie));
        let slow = EventTuning { min_player_kmh: 7.5, ..Default::default() };
        assert_eq!(m.gate(1, 8292, &slow, &GateInputs { player_speed: 2.0, ..at(100.0) }, &mut rng), Err(Refusal::PlayerSpeed));
        assert_eq!(m.gate(1, 8292, &slow, &GateInputs { player_speed: 3.0, ..at(100.0) }, &mut rng), Ok(()));
    }

    #[test]
    fn a_warn_picks_the_far_or_near_line_of_the_speakers_voice() {
        let rec = |kind, variant, flag, clip| Record { weight_code: 0x39, probability: 100, mode: 0, locals: 0, values: vec![kind, variant, flag], clips: vec![ClipRef { id: clip, lookup: 0, params: 0 }] };
        let ev = Event {
            id: 8210,
            name: "501_warn".into(),
            queue_timeout: 60,
            priority: 500,
            conditions: 0,
            flags: 0x30,
            probability: 100,
            flags2: 0,
            fields: vec![1, 2, 3],
            records: vec![rec(0x1000, 1, 1, 0x20), rec(0x1000, 1, 2, 0x21), rec(0x1000, 2, 2, 0x22)],
        };
        let table = EventTable { bank: 1, sub_bank: 0, events: vec![ev] };
        let headers = (0x20..=0x22).map(|id| ClipHeader { id, takes: 4, history: 4, flags: 0 }).collect();
        let mut lib = Library::new(headers);
        let mut m = SpeechManager::default();
        let busm1 = Speaker { index: 7, kind: kind::BUSINESS_MAN, variant: 1, ..Default::default() };
        let inputs = GateInputs { now: 1000.0, ..Default::default() };
        let mut rng = seq(vec![0]);
        let far = m.request(&mut lib, &table, 53, FAR, &busm1, &inputs, &mut rng).unwrap();
        assert_eq!((far.event, far.picks[0].clip), (8210, 0x20));
        let near = m.request(&mut lib, &table, 53, NEAR, &Speaker { index: 8, ..busm1 }, &inputs, &mut rng).unwrap();
        assert_eq!(near.picks[0].clip, 0x21);
        assert!(m.request(&mut lib, &table, 53, NEAR, &busm1, &inputs, &mut rng).is_ok(), "the default tuning has no repeat time");
        m.tuning.insert(8210, EventTuning { repeat: 15.0, ..Default::default() });
        assert_eq!(m.request(&mut lib, &table, 53, NEAR, &busm1, &inputs, &mut rng), Err(Refusal::Repeat));
        let ghost = Speaker { index: 9, kind: kind::GRANNY, variant: 1, ..Default::default() };
        assert_eq!(m.request(&mut lib, &table, 53, NEAR, &ghost, &inputs, &mut rng), Err(Refusal::Library(NoLine::NoRecord)));
    }
}
