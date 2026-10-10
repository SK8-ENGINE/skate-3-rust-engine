//! Ped conversations (doc 26 "Ped conversations"): the conversation object a ped spawns with
//! SpawnConversationArea. Retail (TU3, evidence only; re-implemented;
//! `.local/research/peds/b23-ped-plugins-conversations.md`, `b24-conversation-object.md`, main
//! checked the vtable, the completion state and the spawn constants):
//! - a conversation is a waypoint plugin entity (ctor `82E1CEC0`, interface vtable `0x8232B868`)
//!   with up to 3 member slots (participant, in-position bit) and a state (`D+1172`);
//! - gathering (vf4) while it has room and the state is 0 or 1; a ped may join (vf8) when its
//!   entity type fills a free slot of the chosen row, or of any candidate row before one is chosen;
//! - 3 waypoints (`82E1D620`) on a 1.5 m circle 120 deg apart from a random start in [0, pi);
//!   a ped locks the nearest free one (vfunc 144 `82E1CBC0`);
//! - when every member signalled "in position" (vf44) it starts (`82E1EBC0`): fewer than 2
//!   members ends it, else a candidate row is picked at random, the per-row value list gives one
//!   value, state 2, the first member speaks;
//! - each turn lasts the speaker's 3.0 s ConversationSpeak timer, then vf48 advances (`82E1ECB0`):
//!   state + 1, the next occupied member speaks; state 7 is complete (vf36): 5 turns.
//! Speech and lifetime (`b43-conversation-speech-slots.md`, main spot-checked `82E3DAD8`):
//! - start and every advance store the turn's line id (`82E1EA98`: 2 -> 0 or 1, 3 -> 3, 4 -> 5,
//!   5 -> 6, 6 -> 7; `D+1168`) and re-roll the variant (`D+1156`, uniform [0, 2)) once per turn;
//! - ConversationSpeak Begin (`826A66B8` -> `82E3DAD8`) gives the speaker the speech value
//!   line id + 33 (`ped+2468`) with the variant and the row's value (`ped+2472` / `+2476`), and
//!   every listener the value 41;
//! - the first join starts gathering (state 1) with a 30 s timer (`82E1D380`); when it runs out
//!   the conversation starts with whoever is there (`82E1EB10`);
//! - a member leaving (`82E1D470`) aborts a conversation that is not complete (vf52 `82E1E998`);
//!   the last one leaving resets it, and a spawned area is then removed (`82E1BD08`).

use super::super::Vec3;

/// The speaker's speech value is the line id plus this (`82E3DAD8`).
pub const SPEAKER_SPEECH_BASE: i32 = 33;
/// Every listener's speech value (`82E3DAD8`); no audio event uses it.
pub const LISTENER_SPEECH: i32 = 41;

/// Member slots (`D+1040`, 3 inline).
pub const MAX_MEMBERS: usize = 3;
/// The state at which a conversation is complete (vf36).
pub const COMPLETE: u8 = 7;

/// Retail values (data-driven; world tuning later).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConversationParams {
    /// Waypoint circle radius, m, and waypoint count (`82E1D620`).
    pub radius: f32,
    pub waypoints: usize,
    /// No new conversation within this distance of another, m (`0x8220E13C`).
    pub exclusion: f32,
    /// The area goes this far ahead of the starter, m (`0x821DBCEC`).
    pub ahead: f32,
    /// A speaker's turn, s (ConversationSpeak timer 30).
    pub turn_seconds: f32,
    /// The gather timer set by the first join, s (`D+1136`, `0x820D4924` 30.0).
    pub gather_seconds: f32,
}

impl Default for ConversationParams {
    fn default() -> Self {
        Self { radius: 1.5, waypoints: 3, exclusion: 50.0, ahead: 3.0, turn_seconds: 3.0, gather_seconds: 30.0 }
    }
}

/// One conversation row (`livingworld_conversations`): the entity types of its participants
/// (`7819`) and its value list (`36F1`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConversationRow {
    pub name: String,
    pub participants: Vec<String>,
    pub values: Vec<i32>,
}

/// A member slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Member {
    pub ped: u64,
    pub in_position: bool,
}

/// One conversation (host-owned plain data).
#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub id: u64,
    pub center: Vec3,
    /// Waypoint positions and who holds each.
    pub waypoints: Vec<(Vec3, Option<u64>)>,
    pub members: Vec<Member>,
    /// Candidate rows (indices into the host's row list) and the chosen one.
    pub candidates: Vec<usize>,
    pub row: Option<usize>,
    pub value: Option<i32>,
    pub state: u8,
    pub speaker: usize,
    /// This turn's line id (`D+1168`) and variant (`D+1156`), rolled once per turn.
    pub line: Option<u8>,
    pub variant: u8,
    /// The gather timer (`D+1136`), s, and its start value.
    pub gather_timer: f32,
    pub gather_seconds: f32,
}

/// What a speaker says this turn (`82E3DAD8`): the speech value (line id + 33), the variant
/// (`ped+2472`) and the row's value (`ped+2476`). The last two have no known audio meaning yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnSpeech {
    pub value: i32,
    pub variant: u8,
    pub list_value: i32,
}

/// What `leave` did (`82E1D470`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// Not a member.
    NotMember,
    /// Members remain (an unfinished conversation was aborted).
    Remaining,
    /// The last member left: the owner resets a placed area, removes a spawned one.
    Empty,
}

/// Uniform [0, 1) from the host's seeded generator.
pub type Rand<'a> = &'a mut dyn FnMut() -> f32;

impl Conversation {
    /// `82E1D620`: the area at `center` with its waypoints.
    pub fn new(id: u64, center: Vec3, candidates: Vec<usize>, params: &ConversationParams, rand: Rand) -> Self {
        let start = rand() * std::f32::consts::PI;
        let step = std::f32::consts::TAU / params.waypoints.max(1) as f32;
        let waypoints = (0..params.waypoints)
            .map(|i| {
                let a = start + step * i as f32;
                ([center[0] + a.sin() * params.radius, center[1], center[2] + a.cos() * params.radius], None)
            })
            .collect();
        Self { id, center, waypoints, members: Vec::new(), candidates, row: None, value: None, state: 0, speaker: 0, line: None, variant: 0, gather_timer: 0.0, gather_seconds: params.gather_seconds }
    }

    /// vf4.
    pub fn is_gathering(&self) -> bool {
        self.members.len() < MAX_MEMBERS && self.state <= 1
    }

    /// vf8: `entity` fills a free slot of the chosen row, or of a candidate row before one is chosen.
    pub fn allowed_to_join(&self, entity: &str, member_types: &[String], rows: &[ConversationRow]) -> bool {
        let fits = |r: &ConversationRow| {
            let mut free = r.participants.clone();
            for t in member_types {
                if let Some(i) = free.iter().position(|p| p == t) {
                    free.remove(i);
                }
            }
            free.iter().any(|p| p == entity)
        };
        match self.row {
            Some(r) => rows.get(r).is_some_and(fits),
            None => self.candidates.iter().filter_map(|&r| rows.get(r)).any(fits),
        }
    }

    pub fn join(&mut self, ped: u64) -> bool {
        if !self.is_gathering() || self.members.iter().any(|m| m.ped == ped) {
            return false;
        }
        self.members.push(Member { ped, in_position: false });
        if self.state == 0 {
            self.state = 1;
            self.gather_timer = self.gather_seconds;
        }
        true
    }

    /// `82E1EB10`: the gather timer runs down; at 0 a gathering conversation starts with whoever
    /// is there (fewer than 2 members ends it). Returns true when it started or ended here.
    pub fn tick(&mut self, dt: f32, rows: &[ConversationRow], rand: Rand) -> bool {
        self.gather_timer = (self.gather_timer - dt).max(0.0);
        if self.state == 1 && self.gather_timer <= 0.0 {
            self.start(rows, rand);
            return true;
        }
        false
    }

    /// vfunc 144: the nearest free waypoint, locked for `ped`.
    pub fn lock_closest_waypoint(&mut self, ped: u64, at: Vec3) -> Option<Vec3> {
        if let Some(w) = self.waypoints.iter().find(|w| w.1 == Some(ped)) {
            return Some(w.0);
        }
        let d2 = |p: Vec3| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2);
        let i = self.waypoints.iter().enumerate().filter(|w| w.1 .1.is_none()).min_by(|a, b| d2(a.1 .0).total_cmp(&d2(b.1 .0))).map(|w| w.0)?;
        self.waypoints[i].1 = Some(ped);
        Some(self.waypoints[i].0)
    }

    pub fn unlock_waypoint(&mut self, ped: u64) {
        for w in &mut self.waypoints {
            if w.1 == Some(ped) {
                w.1 = None;
            }
        }
    }

    pub fn waypoint_of(&self, ped: u64) -> Option<Vec3> {
        self.waypoints.iter().find(|w| w.1 == Some(ped)).map(|w| w.0)
    }

    /// vf44, then `82E1EBC0` when every member is in position.
    pub fn signal_in_position(&mut self, ped: u64, rows: &[ConversationRow], rand: Rand) {
        if let Some(m) = self.members.iter_mut().find(|m| m.ped == ped) {
            m.in_position = true;
        }
        if self.state <= 1 && self.members.iter().all(|m| m.in_position) {
            self.start(rows, rand);
        }
    }

    fn start(&mut self, rows: &[ConversationRow], rand: Rand) {
        if self.members.len() < 2 {
            self.abort();
            return;
        }
        if self.row.is_none() && !self.candidates.is_empty() {
            let i = ((rand() * self.candidates.len() as f32) as usize).min(self.candidates.len() - 1);
            self.row = Some(self.candidates[i]);
        }
        if self.value.is_none() {
            let list = self.row.and_then(|r| rows.get(r)).map(|r| r.values.as_slice()).unwrap_or(&[]);
            self.value = Some(if list.is_empty() { 6 } else { list[((rand() * list.len() as f32) as usize).min(list.len() - 1)] });
        }
        self.state = 2;
        self.speaker = 0;
        self.roll_turn(rand);
    }

    /// vf52 `82E1E998`: complete, no speaker.
    fn abort(&mut self) {
        self.state = COMPLETE;
        self.line = None;
    }

    /// Start / advance: store the line id and re-roll the variant (uniform [0, 2), `82E17478`).
    fn roll_turn(&mut self, rand: Rand) {
        self.line = Self::line_for(self.state, rand);
        self.variant = ((rand() * 2.0) as u8).min(1);
    }

    /// vf12: the speaking member.
    pub fn speaker(&self) -> Option<u64> {
        (self.state >= 2 && self.state < COMPLETE).then(|| self.members.get(self.speaker).map(|m| m.ped)).flatten()
    }

    /// vf48 / `82E1ECB0`: the turn passes.
    pub fn pass_turn(&mut self, rand: Rand) {
        if self.state < 2 || self.state >= COMPLETE {
            return;
        }
        self.state += 1;
        if !self.members.is_empty() {
            self.speaker = (self.speaker + 1) % self.members.len();
        }
        self.roll_turn(rand);
    }

    /// `82E3DAD8`: the speaker's speech this turn.
    pub fn turn_speech(&self) -> Option<TurnSpeech> {
        self.speaker()?;
        Some(TurnSpeech { value: i32::from(self.line?) + SPEAKER_SPEECH_BASE, variant: self.variant, list_value: self.value.unwrap_or(0) })
    }

    /// vf32 `82E1E800`: the members other than the speaker.
    pub fn listeners(&self) -> impl Iterator<Item = u64> + '_ {
        let speaker = self.speaker();
        self.members.iter().map(|m| m.ped).filter(move |&p| Some(p) != speaker)
    }

    /// vf36.
    pub fn is_complete(&self) -> bool {
        self.state >= COMPLETE
    }

    /// `82E1EA98`: the line id for a state (`rand` [0, 2) for state 2's 0 / 1).
    fn line_for(state: u8, rand: Rand) -> Option<u8> {
        Some(match state {
            2 => ((rand() * 2.0) as u8).min(1),
            3 => 3,
            4 => 5,
            5 => 6,
            6 => 7,
            _ => return None,
        })
    }

    /// `82E1D470`. Retail keeps the leaver's slot key (a re-join by the same entity type refills
    /// it); ours drops the member.
    pub fn leave(&mut self, ped: u64) -> Left {
        self.unlock_waypoint(ped);
        let Some(i) = self.members.iter().position(|m| m.ped == ped) else {
            return Left::NotMember;
        };
        self.members.remove(i);
        if self.speaker >= self.members.len() {
            self.speaker = 0;
        }
        if self.members.is_empty() {
            return Left::Empty;
        }
        if !self.is_complete() {
            self.abort();
        }
        Left::Remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conversation_gathers_starts_and_runs_five_turns() {
        let rows = vec![ConversationRow { name: "r".into(), participants: vec!["adult_male".into(), "teen_male".into()], values: vec![4] }];
        let mut seq = [0.0f32, 0.0, 0.0, 0.0].into_iter().cycle();
        let mut rand = || seq.next().unwrap();
        let mut c = Conversation::new(1, [0.0; 3], vec![0], &ConversationParams::default(), &mut rand);
        assert_eq!(c.waypoints.len(), 3);
        assert!((c.waypoints[0].0[2] - 1.5).abs() < 1e-5, "start angle 0 -> +z");
        assert!(c.allowed_to_join("teen_male", &["adult_male".into()], &rows));
        assert!(!c.allowed_to_join("adult_male", &["adult_male".into()], &rows), "no free adult_male slot");
        assert!(c.join(10) && c.join(11));
        let a = c.lock_closest_waypoint(10, [0.0, 0.0, 5.0]).unwrap();
        assert_eq!(a, c.waypoints[0].0);
        assert_ne!(c.lock_closest_waypoint(11, [0.0, 0.0, 5.0]).unwrap(), a, "a locked waypoint is skipped");
        c.signal_in_position(10, &rows, &mut rand);
        assert_eq!(c.state, 1, "gathering: waits for every member");
        c.signal_in_position(11, &rows, &mut rand);
        assert_eq!((c.state, c.row, c.value, c.speaker()), (2, Some(0), Some(4), Some(10)));
        let mut speakers = Vec::new();
        let mut values = Vec::new();
        while !c.is_complete() {
            speakers.push(c.speaker().unwrap());
            let t = c.turn_speech().unwrap();
            assert_eq!(c.listeners().collect::<Vec<_>>(), vec![if speakers.last() == Some(&10) { 11 } else { 10 }]);
            assert_eq!((t.variant, t.list_value), (0, 4));
            values.push(t.value);
            c.pass_turn(&mut rand);
        }
        assert_eq!(speakers, vec![10, 11, 10, 11, 10]);
        assert_eq!(values, vec![33, 36, 38, 39, 40], "line id + 33 per state (rand 0 -> intro short)");
        assert_eq!(c.turn_speech(), None);
        assert!(!c.is_gathering());
    }

    #[test]
    fn the_line_and_variant_are_rolled_once_per_turn() {
        let rows = vec![ConversationRow { name: "r".into(), participants: vec!["a".into(), "b".into()], values: vec![8] }];
        let mut seq = [0.0f32, 0.0, 0.0, 0.9, 0.9].into_iter().chain(std::iter::repeat(0.0));
        let mut rand = || seq.next().unwrap();
        let mut c = Conversation::new(1, [0.0; 3], vec![0], &ConversationParams::default(), &mut rand);
        c.join(1);
        c.join(2);
        c.signal_in_position(1, &rows, &mut rand);
        c.signal_in_position(2, &rows, &mut rand);
        let t = c.turn_speech().unwrap();
        assert_eq!((t.value, t.variant, t.list_value), (34, 1, 8), "intro question, variant 1");
        assert_eq!(c.turn_speech(), Some(t), "reading it again does not re-roll");
    }

    #[test]
    fn the_gather_timer_starts_with_whoever_is_there() {
        let rows = vec![ConversationRow { name: "r".into(), participants: vec!["a".into(), "b".into()], values: vec![] }];
        let mut rand = || 0.0f32;
        let mut c = Conversation::new(1, [0.0; 3], vec![0], &ConversationParams::default(), &mut rand);
        c.join(1);
        c.join(2);
        assert_eq!((c.state, c.gather_timer), (1, 30.0));
        assert!(!c.tick(29.9, &rows, &mut rand));
        assert!(c.tick(0.2, &rows, &mut rand), "nobody signalled in position: starts at 0");
        assert_eq!((c.state, c.speaker(), c.value), (2, Some(1), Some(6)));
    }

    #[test]
    fn a_member_leaving_aborts_and_the_last_one_empties_it() {
        let rows = vec![ConversationRow { name: "r".into(), participants: vec!["a".into(), "b".into()], values: vec![1] }];
        let mut rand = || 0.0f32;
        let mut c = Conversation::new(1, [0.0; 3], vec![0], &ConversationParams::default(), &mut rand);
        c.join(1);
        c.join(2);
        c.signal_in_position(1, &rows, &mut rand);
        c.signal_in_position(2, &rows, &mut rand);
        assert_eq!(c.leave(9), Left::NotMember);
        assert_eq!(c.leave(2), Left::Remaining);
        assert!(c.is_complete() && c.speaker().is_none(), "mid-talk leave aborts");
        assert_eq!(c.leave(1), Left::Empty);
    }

    #[test]
    fn a_lone_member_ends_the_conversation() {
        let mut rand = || 0.5f32;
        let mut c = Conversation::new(1, [0.0; 3], vec![], &ConversationParams::default(), &mut rand);
        c.join(10);
        c.signal_in_position(10, &[], &mut rand);
        assert!(c.is_complete());
    }
}
