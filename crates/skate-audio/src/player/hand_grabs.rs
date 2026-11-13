//! The HandGrabs component's skitch part (824EC3E0 -> 824EC488; b141).
//! Retail mechanism: docs/hails-additions/11-audio.md#handgrabs-skitch-audio

use super::AudioState;
use super::contacts::SpliceHost;
use crate::splice::SoundId;

/// `aud_splicer` `handgrab` and its bus (mod-overridable through the player tuning).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandGrabTuning {
    /// Skate_Collisions and Skate_Metal ids (stock 0x3C5, 0x1D0).
    pub collisions: u32,
    pub metal: u32,
    /// The eEQChain bus (`aud_randomeqtuning` `default` field `CCDFABBAEEA33EAE`): 2.
    pub eq: u8,
}

impl Default for HandGrabTuning {
    fn default() -> Self {
        Self { collisions: 0x3C5, metal: 0x1D0, eq: 2 }
    }
}

/// The skitch latch (`this+36`) and its two voices (`+40`, `+44`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandGrabs {
    pub latched: bool,
    pub voices: [Option<SoundId>; 2],
    /// Starts so far (diagnostics).
    pub starts: u64,
}

impl HandGrabs {
    /// `sub_824EC488` for this frame. Non-local skaters do nothing (the component's update gate).
    pub fn update(&mut self, s: &AudioState, t: &HandGrabTuning, host: &mut dyn SpliceHost) {
        if !s.local {
            return;
        }
        if s.skitching && !self.latched {
            self.latched = true;
            for v in self.voices.iter_mut() {
                if let Some(old) = v.take() {
                    host.release(old);
                }
            }
            for (i, (bank, id)) in [("Skate_Collisions", t.collisions), ("Skate_Metal", t.metal)].into_iter().enumerate() {
                host.set_route(crate::bus::Route { output: crate::bus::Output::Eq(t.eq), create: true, owner_env: 1.0, mono: true });
                self.voices[i] = host.start(bank, id, [0.0, 1.0, 0.0, s.dt, 1.0, 1.0]);
                self.starts += u64::from(self.voices[i].is_some());
            }
        } else if !s.skitching {
            self.latched = false;
        }
    }

    /// `HandGrabs.in0`.
    pub fn input(&self) -> i32 {
        if self.latched { 32767 } else { 0 }
    }

    /// The owner's release.
    pub fn release(&mut self, host: &mut dyn SpliceHost) {
        for v in self.voices.iter_mut() {
            if let Some(id) = v.take() {
                host.release(id);
            }
        }
        self.latched = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Host {
        started: Vec<(String, u32)>,
        routes: Vec<crate::bus::Route>,
        released: Vec<SoundId>,
    }
    impl SpliceHost for Host {
        fn set_route(&mut self, route: crate::bus::Route) {
            self.routes.push(route);
        }
        fn start(&mut self, bank: &str, id: u32, _: [f32; 6]) -> Option<SoundId> {
            self.started.push((bank.to_owned(), id));
            Some(self.started.len())
        }
        fn update(&mut self, _: SoundId, _: [f32; 6]) {}
        fn alive(&self, _: SoundId) -> bool {
            true
        }
        fn release(&mut self, sound: SoundId) {
            self.released.push(sound);
        }
    }

    #[test]
    fn a_skitch_starts_the_hand_grab_pair_once_and_holds_the_input() {
        let t = HandGrabTuning::default();
        let mut g = HandGrabs::default();
        let mut h = Host::default();
        let towed = AudioState { local: true, skitching: true, dt: 1.0 / 60.0, ..Default::default() };
        for _ in 0..5 {
            g.update(&towed, &t, &mut h);
        }
        assert_eq!(h.started, [("Skate_Collisions".to_owned(), 0x3C5), ("Skate_Metal".to_owned(), 0x1D0)]);
        assert!(h.routes.iter().all(|r| r.output == crate::bus::Output::Eq(2)));
        assert_eq!(g.input(), 32767);
        // Letting go only clears the latch; the next grab releases the old pair first.
        g.update(&AudioState { local: true, ..Default::default() }, &t, &mut h);
        assert_eq!((g.input(), h.released.len()), (0, 0));
        g.update(&towed, &t, &mut h);
        assert_eq!((h.started.len(), h.released, g.starts), (4, vec![1, 2], 4));
    }

    #[test]
    fn other_skaters_play_no_hand_grabs() {
        let mut g = HandGrabs::default();
        let mut h = Host::default();
        g.update(&AudioState { local: false, skitching: true, ..Default::default() }, &HandGrabTuning::default(), &mut h);
        assert!(h.started.is_empty() && g.input() == 0);
    }
}
