//! Pedestrians' sound objects (MixMap slot 5, `world-ped-audio.md`):
//! - [`PedSfx`] = `SFXObj_PedestrianSFX` (vtable `0x822FCCC8`, factory `sub_824D7E00`): process
//!   `sub_824D8078` (vfunc 9), update `sub_824D81A8` (vfunc 10). Its footsteps are two layers:
//!   - the held `livingword_footstep` packets (21 words, constructor `sub_824B77F0`, bank
//!     `fstep_livingworld`), one per foot, posted by `sub_824D81F8` and rewritten every frame by
//!     `sub_824D8658`: the program plays a step on the foot-down word;
//!   - a `sk8_foley` Splice one-shot per foot plant (`sub_824D8320`), walk / jog / run by the ped's
//!     speed (`sub_82494840`: Clothing thresholds 7.5 / 2.5 m/s → containers 64 / 63 / 62), kept
//!     up to date by `sub_824D84D0`. Retail's `sk8_foley` 62/63/64 starts by caller `824D8164` are
//!     these (recomp `all_20261002_164620`: 1017 events).
//! - [`PedSpeech`] = `SFXObj_PedestrianSpeech` (vtable `0x822FBF40`): process `sub_824D9908`
//!   hands the speech manager a request whenever the ped's speech value (`SendSpeechEvent`'s
//!   `speechvalue` in the state graphs) changes ([`super::speech`] resolves it to a clip).
//!
//! The ped side ([`PedState`]) is the record the objects read: the ped audio state `[object+32]`
//! (feet `+73`/`+74`, footsteps on `+68`, kind `+96`, class `+132`, speech value `+136`,
//! materials `+140`/`+144`, the speech distance pair `+148`/`+156`) and the owner `[object+28]`
//! (speed `+128`, weight `+144`). Which LW ped fields fill them is not traced; a ped system fills
//! [`PedState`] from its own animation (foot plants) and AI (speech values).
use super::{WorldCommand, WorldSlot};
use crate::player::contacts::SpliceHost;
use crate::player::footsteps::{Curve, footstep_surface};
use crate::player::tuning::PlayerTuning;
use crate::player::{Outputs, trunc_clamp};
use crate::splice::SoundId;

pub const FOOTSTEP_CLASS: &str = "livingword_footstep";
pub const FOOTSTEP_BANK: &str = "fstep_livingworld";
pub const FOOTSTEP_WORDS: usize = 21;
/// Retail's Splice bank table index 7 (`crate::player::footsteps::splice_bank`).
pub const STEP_BANK: &str = "sk8_foley";
/// The "no material" value of the ped's material words; the poster substitutes 3.
pub const NO_MATERIAL: u32 = 143;

const LEVEL: f32 = f32::from_bits(0x3800_0100); // 1/32767 (0x822F8898)
const PITCH: f32 = f32::from_bits(0x3980_0000); // 1/4096 (0x822F890C)
const DEGREES: f32 = f32::from_bits(0x3BB4_00B4); // 360/65535 (0x822F8C64)

/// The ped footstep values from the audio vault (setup export `world_tuning.peds`; the defaults are
/// the shipped records, as `player::footsteps::FootstepTuning`'s are).
#[derive(Clone, Debug, PartialEq)]
pub struct PedFootstepTuning {
    /// OffBoard tuning `90B47430C4ED2CCC` (`Sk8::PointNegGraphData16`): ped speed → `+416` → w13.
    pub speed_curve: Curve,
    /// Clothing `E12AF885D3C3A168` [run above, jog above] (m/s), shared with the player's walking
    /// voices.
    pub speeds: [f32; 2],
    /// `sk8_foley` ids [walk `6B61C043E53C44CB`, jog `EC3399A49055DD8D`, run `9D6D2863CFE908C4`].
    pub step_ids: [i32; 3],
    /// OffBoard tuning `62A2E64238934734` → packet w17..w19.
    pub tail: [i32; 3],
    /// eEQChain `A9023782094771B5` (holder `42AFE160E647167C`): w20 = this + 10.
    pub eq_chain: i32,
}

impl Default for PedFootstepTuning {
    fn default() -> Self {
        Self {
            speed_curve: Curve::from_bits(
                [
                    0x0000_0000, 0x3F94_0358, 0x3F9E_6FBD, 0x4043_F5FD, 0x407D_4A38, 0x409C_5A0E, 0x40B3_488C, 0x40CC_D222, 0x40E0_A01B, 0x40F0_C821,
                    0x40FD_CFA2, 0x4104_E627, 0x410C_FA2A, 0x4113_7DEA, 0x4118_B41E, 0x411E_F529,
                ],
                [
                    0x0000_0000, 0x0000_0000, 0x42F7_EF0E, 0x4318_E481, 0x4339_F34D, 0x436B_8986, 0x438E_8FDF, 0x43A7_5AFB, 0x43BE_1529, 0x43DB_021D,
                    0x43FA_0000, 0x440F_9855, 0x4416_D392, 0x4421_2833, 0x4424_4196, 0x4424_4196,
                ],
            ),
            speeds: [7.5, 2.5],
            step_ids: [62, 63, 64],
            tail: [32767, 7000, 25000],
            eq_chain: 2,
        }
    }
}

/// What the ped objects read each frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PedState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Owner `+128`: the ped's speed (m/s).
    pub speed: f32,
    /// `+74` (foot A, packet w3 = 1000) and `+73` (foot B, w3 = 0): the foot is planted.
    pub feet: [bool; 2],
    /// `+68`: footsteps on (the ped is close / visible enough; who sets it is not traced).
    pub footsteps: bool,
    /// `+140` (foot A) / `+144` (foot B): the material under each foot (143 = none).
    pub materials: [u32; 2],
    /// `+136`: the speech value the ped's state graph last sent (`SendSpeechEvent`).
    pub speech_value: i32,
    /// `+132` → w14 (1..5); meaning not traced (ped class / shoe type).
    pub class: i32,
    /// Owner `+144` → w16 (1..5); meaning not traced (body weight).
    pub weight: i32,
    /// `+96 == 64`: the outputs read are the close-range variants (footstep level 2 / step level 4
    /// instead of 1 / 3; PedestrianSFX out4 rolls off over 3–8 m).
    pub close: bool,
    /// The speech manager's flag pair `+148` / `+156` (`flag = 1` when `+148 > +156`, else 2).
    pub speech_measure: f32,
    pub speech_limit: f32,
}

impl Default for PedState {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            velocity: [0.0; 3],
            speed: 0.0,
            feet: [false; 2],
            footsteps: true,
            materials: [NO_MATERIAL; 2],
            speech_value: 0,
            class: 1,
            weight: 1,
            close: false,
            speech_measure: 0.0,
            speech_limit: 0.0,
        }
    }
}

/// `SFXObj_PedestrianSFX`'s footstep state. Index 0 = foot A (`+40` packet, `+424` sound, flags
/// `+52`/`+54`, material `+56`), 1 = foot B (`+224`, `+420`, `+236`/`+238`, `+240`).
#[derive(Clone, Debug, Default)]
pub struct PedSfx {
    pub packets: [Option<[i32; FOOTSTEP_WORDS]>; 2],
    pub sounds: [Option<SoundId>; 2],
    /// `+416`: the speed curve's word.
    pub speed_word: i32,
    down: [bool; 2],
    prev: [bool; 2],
    materials: [u32; 2],
}

impl PedSfx {
    /// `sub_824B77F0` (foot 1000 = A, 0 = B).
    pub fn post_words(foot: i32, speed_word: i32, eq: i32) -> [i32; FOOTSTEP_WORDS] {
        let mut w = [0; FOOTSTEP_WORDS];
        w[0] = 32767;
        w[2] = 4096;
        w[3] = foot.clamp(0, 1000);
        w[4] = 25000;
        w[13] = speed_word.clamp(0, 1000);
        w[14] = 1;
        w[15] = 1;
        w[16] = 1;
        w[20] = eq.clamp(0, 32767);
        w
    }

    /// The Splice step for a speed (`sub_82494840`).
    pub fn step_id(t: &PedFootstepTuning, speed: f32) -> i32 {
        if speed > t.speeds[0] {
            t.step_ids[2]
        } else if speed > t.speeds[1] {
            t.step_ids[1]
        } else {
            t.step_ids[0]
        }
    }

    /// vfunc 9 (`sub_824D8078`), before the tick. `dt` is the frame time the Splice block carries.
    pub fn process(&mut self, owner: u64, s: &PedState, t: &PedFootstepTuning, host: &mut dyn SpliceHost, dt: f32) -> Vec<WorldCommand> {
        let mut out = Vec::new();
        self.speed_word = trunc_clamp(t.speed_curve.eval(s.speed), i32::MIN, i32::MAX);
        self.down = s.feet;
        if s.footsteps {
            // Poster (`sub_824D81F8`): materials (none → 3), then the held packets, B first.
            for i in 0..2 {
                self.materials[i] = if s.materials[i] == NO_MATERIAL { 3 } else { s.materials[i] };
            }
            let eq = t.eq_chain + 10;
            for (i, foot) in [(1usize, 0), (0usize, 1000)] {
                if self.packets[i].is_none() {
                    let words = Self::post_words(foot, self.speed_word, eq);
                    self.packets[i] = Some(words);
                    out.push(WorldCommand::Post { owner, slot: WorldSlot::PedFootstep(i as u8), class: FOOTSTEP_CLASS, words: words.to_vec() });
                }
            }
            // Splice steps on each plant (`sub_824D8320`): foot A, then B.
            for i in 0..2 {
                if self.down[i] && !self.prev[i] {
                    if let Some(old) = self.sounds[i].take() {
                        host.release(old);
                    }
                    let id = Self::step_id(t, s.speed);
                    self.sounds[i] = u32::try_from(id).ok().and_then(|id| host.start(STEP_BANK, id, [0.0, 1.0, 0.0, dt, 0.0, 1.0]));
                }
            }
            // `sub_824D8E60` (a one-shot flag `+412` by camera distance and a vault key) is not
            // ported: its result feeds nothing the footsteps read.
        }
        self.prev = self.down;
        out
    }

    /// vfunc 10 (`sub_824D81A8`), after the tick.
    pub fn update(&mut self, owner: u64, s: &PedState, t: &PedFootstepTuning, tuning: &PlayerTuning, out: &dyn Outputs, host: &mut dyn SpliceHost, dt: f32) -> Vec<WorldCommand> {
        let mut cmds = Vec::new();
        if s.footsteps {
            // `sub_824D8658`: both packets, A then B.
            let collision = matches!(s.speech_value, 6 | 7);
            let jump = matches!(s.speech_value, 4 | 5);
            let level = out.level(if s.close { 2 } else { 1 }).clamp(0, 32767);
            for i in 0..2 {
                let Some(mut w) = self.packets[i] else { continue };
                w[0] = 32767;
                w[1] = out.raw(0).clamp(0, 65535);
                w[2] = out.pitch(5).clamp(0, 8192);
                w[4] = out.level(7).clamp(0, 25001);
                w[5] = out.level(8).clamp(0, 25001);
                w[6] = out.level(9).clamp(0, 32767);
                w[7] = level;
                w[8] = i32::from(self.down[i]);
                w[9] = 0;
                w[10] = i32::from(jump);
                w[11] = 0;
                w[12] = i32::from(collision);
                w[13] = self.speed_word.clamp(0, 1000);
                w[14] = s.class.clamp(1, 5);
                w[15] = footstep_surface(tuning, self.materials[i]).clamp(1, 7);
                w[16] = s.weight.clamp(1, 5);
                for k in 0..3 {
                    w[17 + k] = t.tail[k].clamp(0, 32767);
                }
                self.packets[i] = Some(w);
                cmds.push(WorldCommand::Redeliver { owner, slot: WorldSlot::PedFootstep(i as u8), words: w.to_vec() });
            }
        }
        // `sub_824D84D0`: the Splice steps follow the owner (level, pitch, azimuth in degrees).
        let block = [
            out.level(if s.close { 4 } else { 3 }) as f32 * LEVEL,
            out.pitch(5) as f32 * PITCH,
            out.raw(0) as f32 * DEGREES,
            dt,
            0.0,
            1.0,
        ];
        for i in 0..2 {
            if let Some(sound) = self.sounds[i] {
                if host.alive(sound) {
                    host.update(sound, block);
                } else {
                    host.release(sound);
                    self.sounds[i] = None;
                }
            }
        }
        cmds
    }

    pub fn release(&mut self, owner: u64, host: &mut dyn SpliceHost) -> Vec<WorldCommand> {
        let mut out = Vec::new();
        for i in 0..2 {
            if self.packets[i].take().is_some() {
                out.push(WorldCommand::Release { owner, slot: WorldSlot::PedFootstep(i as u8) });
            }
            if let Some(sound) = self.sounds[i].take() {
                host.release(sound);
            }
        }
        out
    }
}

/// A request `SFXObj_PedestrianSpeech` hands the speech manager (`sub_824AB6C8` / `sub_824AC438`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeechRequest {
    pub owner: u64,
    /// The speech value after the object's remap (7 / 8 → 30 after 29, else 51).
    pub value: i32,
    /// 1 when the ped's `+148` exceeds `+156`, else 2.
    pub flag: i32,
}

/// `SFXObj_PedestrianSpeech`'s state.
#[derive(Clone, Debug, Default)]
pub struct PedSpeech {
    /// `+36`: the speech value last handed on.
    pub last: i32,
}

impl PedSpeech {
    /// vfunc 9 (`sub_824D9908`): a request when the value changes. Value 49 goes to a separate
    /// handler (`sub_824D9C70`, not ported) and 29 (the photographer, `PictureTaking`) has a repeat
    /// timer (`+160`) gated by a global flag, not ported.
    pub fn process(&mut self, owner: u64, s: &PedState) -> Option<SpeechRequest> {
        let value = s.speech_value;
        if value == self.last {
            return None;
        }
        let previous = self.last;
        self.last = value;
        if value == 49 {
            return None;
        }
        let value = if matches!(value, 7 | 8) { if previous == 29 { 30 } else { 51 } } else { value };
        let flag = if s.speech_measure > s.speech_limit { 1 } else { 2 };
        Some(SpeechRequest { owner, value, flag })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Route;

    #[derive(Default)]
    struct Host {
        starts: Vec<(String, u32, [f32; 6])>,
        released: Vec<SoundId>,
        next: u32,
    }

    impl SpliceHost for Host {
        fn set_route(&mut self, _: Route) {}
        fn start(&mut self, bank: &str, id: u32, block: [f32; 6]) -> Option<SoundId> {
            self.starts.push((bank.into(), id, block));
            self.next += 1;
            Some(self.next as SoundId)
        }
        fn update(&mut self, _: SoundId, _: [f32; 6]) {}
        fn alive(&self, _: SoundId) -> bool {
            true
        }
        fn release(&mut self, s: SoundId) {
            self.released.push(s);
        }
    }

    struct Flat;
    impl Outputs for Flat {
        fn level(&self, id: usize) -> i32 {
            [0, 1000, 2000, 3000, 4000, 0, 0, 25000, 0, 900][id.min(9)]
        }
        fn raw(&self, _: usize) -> i32 {
            100
        }
        fn pitch(&self, _: usize) -> i32 {
            4096
        }
    }

    #[test]
    fn footsteps_post_both_feet_and_step_on_plants() {
        let t = PedFootstepTuning::default();
        let mut sfx = PedSfx::default();
        let mut host = Host::default();
        let mut s = PedState { speed: 1.3, ..Default::default() };
        let c = sfx.process(5, &s, &t, &mut host, 1.0 / 30.0);
        assert_eq!(c.len(), 2);
        let WorldCommand::Post { slot, words, .. } = &c[0] else { panic!() };
        assert_eq!(*slot, WorldSlot::PedFootstep(1));
        assert_eq!(words[3], 0);
        assert_eq!(words[20], 12);
        assert!(host.starts.is_empty());
        // Foot A plants: one walk step (62) from sk8_foley.
        s.feet = [true, false];
        sfx.process(5, &s, &t, &mut host, 1.0 / 30.0);
        assert_eq!(host.starts.len(), 1);
        assert_eq!((host.starts[0].0.as_str(), host.starts[0].1), ("sk8_foley", 62));
        // Held: no new step. Running speed on foot B's plant: 64.
        s.speed = 8.0;
        s.feet = [true, true];
        sfx.process(5, &s, &t, &mut host, 1.0 / 30.0);
        assert_eq!(host.starts.len(), 2);
        assert_eq!(host.starts[1].1, 64);
        // The update rewrites both packets with the foot flags.
        let tuning = PlayerTuning::default();
        let c = sfx.update(5, &s, &t, &tuning, &Flat, &mut host, 1.0 / 30.0);
        let WorldCommand::Redeliver { words, .. } = &c[0] else { panic!() };
        assert_eq!(words[8], 1);
        assert_eq!(words[7], 1000);
        assert_eq!(words[6], 900);
        assert_eq!(&words[17..20], &[32767, 7000, 25000]);
    }

    #[test]
    fn speech_requests_on_value_changes_with_the_remap() {
        let mut sp = PedSpeech::default();
        let mut s = PedState { speech_value: 10, speech_measure: 5.0, speech_limit: 3.0, ..Default::default() };
        assert_eq!(sp.process(1, &s), Some(SpeechRequest { owner: 1, value: 10, flag: 1 }));
        assert_eq!(sp.process(1, &s), None);
        s.speech_value = 29;
        sp.process(1, &s);
        s.speech_value = 7;
        s.speech_measure = 1.0;
        assert_eq!(sp.process(1, &s), Some(SpeechRequest { owner: 1, value: 30, flag: 2 }));
        s.speech_value = 8;
        assert_eq!(sp.process(1, &s).map(|r| r.value), Some(51));
    }
}
