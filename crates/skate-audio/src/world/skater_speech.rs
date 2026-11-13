//! NPC skaters' own speech (`SFXObj_PlayerSpeech`'s non-local process, `sub_824DA1B0`; spec
//! `audio-specs/world-speech.md` "Main cast"): every skater with a speech record runs it, with no
//! distance gate. Its inputs are the skater record's reaction bytes, which the instance bridge
//! `sub_824B6E80` unpacks from the skater entry (`+392` / `+396` / `+388`); the AI sets those bits.
//!
//! - A **living-world voice** (`sub_824DAAC0`): a seen slam (`+102` or `+103`) says `104_spec_slam`
//!   in game mode 55, `903_pc_slam_race` in modes 19 / 20, else `404_pc_miss_trick` or `405_pc_slam`
//!   (`rand() & 1`, drawn every frame); a seen trick (`+104`) `101_pos`; the system's chase flag
//!   (`+1100` bit 6) `105_spec_chase` when nothing else. A request goes out only when the chosen
//!   event changes (`+44`).
//! - A **main-cast voice** (a pro, `sub_824DA768`): a seen slam (`+102`, by `+112`) says
//!   `104_slam` (event 1; `903_race_slam_pc` = 16 in modes 19 / 20) when the slammer is the player
//!   (0) and `151_pro_slam` (288) with the slammer's words when it is a pro (1–29); `+103` (by
//!   `+116`) the same without the race case; a seen trick (`+104`, by `+108`) `101_pos` (0) or
//!   `150_pro_pos` (287). Each request goes out every frame the byte holds (the manager's timers
//!   gate them).
//! - **Its own crash** (`sub_824DB688`, `+131` rising, once until it clears): the message
//!   [`super::speech_manager::main_cast::message::CRASH`] (`131_collide_object` / `906_aislm`),
//!   then [`Say::Announcer`] `480_slam_pro` (the host asks the announcer channel when the skater is
//!   within the crash distance of the camera and its model has an announcer pro id:
//!   [`super::announcer`]; no line plays without a challenge's announcer).
//! - **A bump** (`sub_824BDA90`, from the body poster): [`collide`], the collide line.
use super::Draw;
use super::speech_manager::main_cast::message;

/// The game modes the choices test (system `+892`); free skate is none of them.
pub const MODE_RACE_A: i32 = 19;
pub const MODE_RACE_B: i32 = 20;
pub const MODE_55: i32 = 55;

/// The skater record's reaction bytes for one frame (what the AI sets; `sub_824B6E80`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reactions {
    /// `+102` (entry `+392` bit 4): a slam was seen; `+112`: by which skater model (0 = the player).
    pub slam: bool,
    pub slam_by: u32,
    /// `+103` (entry `+392` bit 3): a second slam reaction; `+116`: by whom.
    pub slam_b: bool,
    pub slam_b_by: u32,
    /// `+104` (entry `+396` bit 10): a trick was seen; `+108`: by whom.
    pub trick: bool,
    pub trick_by: u32,
    /// `+131` (entry `+388` bit 5): the skater's own crash.
    pub crash: bool,
    /// The system's chase flag (`+1100` bit 6).
    pub chase: bool,
}

/// What the process asks the speech host for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Say {
    /// A living-world event (`sub_824ABA18`).
    Living(u16),
    /// A main-cast event (`sub_824AC560`), with the other skater's model for the pro-on-pro lines.
    MainCast { event: u16, other: Option<u32> },
    /// A skater speech message pair (`sub_824DAC00`): the speaker's kind picks the event.
    Message(u16, u16),
    /// An announcer event about this skater (`sub_824DB688`: the crash's `480_slam_pro`); the host
    /// applies the distance and pro-id conditions.
    Announcer(u16),
}

/// One skater's process state (`+44` the last living-world event, `+193` the crash latch, `+148`
/// the crash line's mute timer).
#[derive(Clone, Debug)]
pub struct SkaterSpeech {
    last: u16,
    crash_latch: bool,
    crash_mute: f32,
}

impl Default for SkaterSpeech {
    fn default() -> Self {
        Self { last: 8318, crash_latch: false, crash_mute: 0.0 }
    }
}

/// A non-local skater's collide line (824BDA90 -> 824A9A20 -> 824DAC00): 130_collide or the mode 19 / 20 pair.
pub fn collide(mode: i32, rng: &mut dyn Draw) -> Say {
    if mode == MODE_RACE_A || mode == MODE_RACE_B {
        Say::Message(if rng.draw() & 1 == 1 { 8240 } else { 8250 }, 17)
    } else {
        Say::Message(8228, 3)
    }
}

fn pro(model: u32) -> bool {
    (1..30).contains(&model)
}

impl SkaterSpeech {
    /// Nothing held: the process would do nothing on a frame without reactions.
    pub fn idle(&self) -> bool {
        self.last == 8318 && !self.crash_latch && !(self.crash_mute > 0.0)
    }

    /// A skater contact asked for this skater's collide line (`sub_824BDA90` at 0x824BDDD8): its
    /// crash line stays muted for `seconds` (retail 3.0, `ContactsTuning::collide_crash_mute`).
    pub fn mute_crash(&mut self, seconds: f32) {
        self.crash_mute = seconds;
    }

    /// The mute timer's countdown (`sub_824F9FC8`: `+148` -= dt).
    pub fn tick(&mut self, dt: f32) {
        if self.crash_mute > 0.0 {
            self.crash_mute -= dt;
        }
    }

    /// One frame. `main_cast` = the speaker has no living-world voice (record `+100 == 0`).
    pub fn process(&mut self, main_cast: bool, r: &Reactions, mode: i32, rng: &mut dyn Draw) -> Vec<Say> {
        let mut out = Vec::new();
        if main_cast {
            if r.slam {
                let event = if pro(r.slam_by) {
                    288
                } else if r.slam_by == 0 && (mode == MODE_RACE_A || mode == MODE_RACE_B) {
                    16
                } else {
                    1
                };
                out.push(Say::MainCast { event, other: pro(r.slam_by).then_some(r.slam_by) });
            }
            if r.slam_b {
                let event = if pro(r.slam_b_by) { 288 } else { 1 };
                out.push(Say::MainCast { event, other: pro(r.slam_b_by).then_some(r.slam_b_by) });
            }
            if r.trick {
                let event = if pro(r.trick_by) { 287 } else { 0 };
                out.push(Say::MainCast { event, other: pro(r.trick_by).then_some(r.trick_by) });
            }
        } else {
            let mut event = 8318u16;
            if r.slam || r.slam_b {
                event = match mode {
                    MODE_55 => 8204,
                    MODE_RACE_A | MODE_RACE_B => 8239,
                    _ => 8234 | (rng.draw() & 1) as u16,
                };
            }
            if r.trick {
                event = 8202;
            }
            if r.chase && event == 8318 {
                event = 8205;
            }
            if event != self.last {
                self.last = event;
                if event != 8318 {
                    out.push(Say::Living(event));
                }
            }
        }
        // `sub_824DB688`: the crash message only with the mute timer run out (`+148 <= 0`); the
        // announcer request and the latch either way.
        if r.crash {
            if !self.crash_latch {
                if !(self.crash_mute > 0.0) {
                    out.push(Say::Message(message::CRASH.0, message::CRASH.1));
                }
                out.push(Say::Announcer(super::announcer::SLAM_PRO));
                self.crash_latch = true;
            }
        } else {
            self.crash_latch = false;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pro_reacts_to_the_player_and_to_pros_and_crashes_once() {
        let mut s = SkaterSpeech::default();
        let mut rng = || 0u32;
        let r = Reactions { slam: true, ..Default::default() };
        assert_eq!(s.process(true, &r, 0, &mut rng), vec![Say::MainCast { event: 1, other: None }]);
        assert_eq!(s.process(true, &r, MODE_RACE_A, &mut rng), vec![Say::MainCast { event: 16, other: None }]);
        let r = Reactions { trick: true, trick_by: 12, crash: true, ..Default::default() };
        assert_eq!(s.process(true, &r, 0, &mut rng), vec![Say::MainCast { event: 287, other: Some(12) }, Say::Message(8229, 125), Say::Announcer(24708)]);
        assert_eq!(s.process(true, &r, 0, &mut rng), vec![Say::MainCast { event: 287, other: Some(12) }], "the crash latch holds");
    }

    #[test]
    fn a_living_world_skater_requests_on_changes() {
        let mut s = SkaterSpeech::default();
        let mut one = || 1u32;
        let r = Reactions { slam: true, ..Default::default() };
        assert_eq!(s.process(false, &r, 0, &mut one), vec![Say::Living(8235)]);
        assert!(s.process(false, &r, 0, &mut one).is_empty(), "unchanged");
        assert_eq!(s.process(false, &Reactions { trick: true, ..r }, 0, &mut one), vec![Say::Living(8202)]);
        assert!(s.process(false, &Reactions::default(), 0, &mut one).is_empty());
        assert_eq!(s.process(false, &Reactions { chase: true, ..Default::default() }, 0, &mut one), vec![Say::Living(8205)]);
    }

    #[test]
    fn a_bumped_skater_says_its_collide_line() {
        let (mut zero, mut one) = (|| 0u32, || 1u32);
        assert_eq!(collide(0, &mut zero), Say::Message(8228, 3));
        assert_eq!(collide(MODE_RACE_A, &mut zero), Say::Message(8250, 17));
        assert_eq!(collide(MODE_RACE_B, &mut one), Say::Message(8240, 17));
    }

    #[test]
    fn a_skater_collide_mutes_the_crash_message_for_3_s_but_not_the_announcer() {
        let mut s = SkaterSpeech::default();
        let mut rng = || 0u32;
        let (crash, calm) = (Reactions { crash: true, ..Default::default() }, Reactions::default());
        s.mute_crash(3.0);
        assert!(!s.idle(), "a running mute keeps the process");
        s.tick(1.0);
        assert_eq!(s.process(true, &crash, 0, &mut rng), vec![Say::Announcer(24708)], "muted: only the announcer request");
        assert_eq!(s.process(true, &crash, 0, &mut rng), vec![], "the latch is set while muted too");
        s.process(true, &calm, 0, &mut rng);
        s.tick(2.0);
        assert!(s.idle());
        assert_eq!(s.process(true, &crash, 0, &mut rng), vec![Say::Message(8229, 125), Say::Announcer(24708)], "the mute ran out");
    }
}
