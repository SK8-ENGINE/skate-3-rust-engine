//! The presentation fade object (owner +0x214; start 82D5BEB0, update 82D5BB80, cancel 82D5C010, reset 82D5C080;
//! research b900 §1, b902). It only produces an alpha and finish messages; who draws the alpha is open (b904).
//! Details: docs/hails-additions/42-challenges.md#presentation-fade

/// Retail values as defaults a mod can replace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeParams {
    /// Seconds taken off `remaining` per update (0x820849C8 = 1/60), a fixed step per 60 Hz tick.
    pub step: f32,
    /// Updates between a non-held mode reaching 0 and its cancel (5).
    pub end_ticks: u32,
    /// Where the flash turns from rising to falling, as a share of its time (0x8209975C = 0.5).
    pub flash_split: f32,
}

impl Default for FadeParams {
    fn default() -> Self {
        Self { step: 1.0 / 60.0, end_ticks: 5, flash_split: 0.5 }
    }
}

/// The four slots (mode 0 is not used by the presentation; 3 is the flash).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FadeMode {
    Unnamed = 0,
    In = 1,
    Out = 2,
    Flash = 3,
}

const MODES: [FadeMode; 4] = [FadeMode::Unnamed, FadeMode::In, FadeMode::Out, FadeMode::Flash];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Slot {
    active: bool,
    cancel: bool,
    remaining: f32,
    total: f32,
    held: bool,
    countdown: u32,
}

/// What an update or start posts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FadeMsg {
    /// The overlay alpha (0x859AECE8); mode 0 posts its own value (0xA5BE2E3B), reported as `Other`.
    Alpha(f32),
    Other(f32),
    /// cMsgEffectFinished with the slot's mode.
    Finished(FadeMode),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fade {
    pub params: FadeParams,
    slots: [Slot; 4],
    /// The overlay bit (fade +0x6C +0x18 bit 8).
    pub overlay: bool,
}

impl Fade {
    pub fn new(params: FadeParams) -> Self {
        Self { params, ..Self::default() }
    }

    /// 82D5BEB0. A FadeOut while already held finishes at once and restarts held; a FadeIn clears a held FadeOut
    /// without a finish message.
    pub fn start(&mut self, mode: FadeMode, duration: f32) -> Vec<FadeMsg> {
        let mut out = Vec::new();
        if mode != FadeMode::Unnamed {
            self.overlay = true;
        }
        let held = self.slots[FadeMode::Out as usize].held;
        if held && mode == FadeMode::Out {
            out.push(FadeMsg::Finished(FadeMode::Out));
        }
        if held && mode == FadeMode::In {
            self.slots[FadeMode::Out as usize] = Slot::default();
        }
        let slot = &mut self.slots[mode as usize];
        slot.active = true;
        slot.cancel = false;
        slot.remaining = duration;
        slot.total = duration;
        slot.countdown = 0;
        out
    }

    /// 82D5BB80, once per 60 Hz tick.
    pub fn tick(&mut self) -> Vec<FadeMsg> {
        let p = self.params;
        let mut out = Vec::new();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            let mode = MODES[i];
            if !slot.active {
                continue;
            }
            if slot.cancel {
                *slot = Slot::default();
                out.push(FadeMsg::Finished(mode));
                continue;
            }
            if slot.held {
                out.push(FadeMsg::Alpha(1.0));
                continue;
            }
            slot.remaining -= p.step;
            let t = slot.total;
            let r = slot.remaining.max(0.0);
            let share = |v: f32| if t > 0.0 { (v / t).clamp(0.0, 1.0) } else { 0.0 };
            out.push(match mode {
                FadeMode::In => FadeMsg::Alpha(share(r)),
                FadeMode::Out => FadeMsg::Alpha(if t > 0.0 { share(t - r) } else { 1.0 }),
                FadeMode::Flash => {
                    let half = p.flash_split * t;
                    let a = if half <= 0.0 {
                        0.0
                    } else if r <= half {
                        r / half
                    } else {
                        (t - r) / half
                    };
                    FadeMsg::Alpha(a.clamp(0.0, 1.0))
                }
                FadeMode::Unnamed => FadeMsg::Other(share(t - r)),
            });
            if slot.remaining > 0.0 {
                continue;
            }
            if mode == FadeMode::Out {
                slot.held = true;
                out.push(FadeMsg::Finished(FadeMode::Out));
            } else if slot.countdown == 0 {
                slot.countdown = p.end_ticks.max(1);
            } else {
                slot.countdown -= 1;
                if slot.countdown == 0 {
                    slot.cancel = true;
                    if mode != FadeMode::Unnamed {
                        self.overlay = false;
                    }
                }
            }
        }
        out
    }

    /// 82D5C010 (a fade event's undo): every active slot is cancelled (finished on the next tick); overlay off.
    pub fn cancel_all(&mut self) {
        for s in self.slots.iter_mut().filter(|s| s.active) {
            s.cancel = true;
        }
        self.overlay = false;
    }

    /// 82D5C080: cancel everything and post alpha 0 at once.
    pub fn reset(&mut self) -> Vec<FadeMsg> {
        self.cancel_all();
        vec![FadeMsg::Alpha(0.0)]
    }

    pub fn held(&self) -> bool {
        self.slots[FadeMode::Out as usize].held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_until_finished(f: &mut Fade, mode: FadeMode) -> (usize, Vec<f32>) {
        let mut alphas = Vec::new();
        for n in 1..1000 {
            for m in f.tick() {
                match m {
                    FadeMsg::Alpha(a) => alphas.push(a),
                    FadeMsg::Finished(x) if x == mode => return (n, alphas),
                    _ => {}
                }
            }
        }
        panic!("never finished");
    }

    #[test]
    fn fade_out_is_linear_and_holds_black_until_a_fade_in() {
        let mut f = Fade::default();
        assert!(f.start(FadeMode::Out, 0.5).is_empty());
        let (n, alphas) = run_until_finished(&mut f, FadeMode::Out);
        assert!((30..=31).contains(&n), "{n}");
        assert!((alphas[0] - 1.0 / 30.0).abs() < 1e-4 && (alphas.last().unwrap() - 1.0).abs() < 1e-4);
        assert!(f.held() && f.overlay);
        assert_eq!(f.tick(), vec![FadeMsg::Alpha(1.0)]);
        // already black: a second FadeOut finishes at once
        assert_eq!(f.start(FadeMode::Out, 0.5), vec![FadeMsg::Finished(FadeMode::Out)]);
        // a FadeIn clears the held FadeOut without a finish message for it
        assert!(f.start(FadeMode::In, 0.25).is_empty());
        assert!(!f.held());
    }

    #[test]
    fn fade_in_finishes_six_ticks_after_its_curve_reaches_zero() {
        let mut f = Fade::default();
        f.start(FadeMode::In, 0.25);
        let (n, alphas) = run_until_finished(&mut f, FadeMode::In);
        // the curve reaches 0 on tick 15 (16 with f32 rounding), the 5-tick countdown cancels 5 later, the finish
        // posts on the tick after
        assert!((21..=22).contains(&n), "{n}");
        assert!((alphas[0] - 14.0 / 15.0).abs() < 1e-4 && alphas[n - 7] < 1e-3);
        assert!(!f.overlay);
    }

    #[test]
    fn flash_rises_then_falls() {
        let mut f = Fade::default();
        f.start(FadeMode::Flash, 0.2);
        let (_, a) = run_until_finished(&mut f, FadeMode::Flash);
        let peak = a.iter().cloned().fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-3 && a[0] < 0.2 && *a.last().unwrap() < 1e-3);
    }

    #[test]
    fn cancel_finishes_every_active_slot_on_the_next_tick_and_reset_posts_zero() {
        let mut f = Fade::default();
        f.start(FadeMode::Out, 1.0);
        f.tick();
        f.cancel_all();
        assert!(!f.overlay);
        assert_eq!(f.tick(), vec![FadeMsg::Finished(FadeMode::Out)]);
        assert!(f.tick().is_empty());
        assert_eq!(f.reset(), vec![FadeMsg::Alpha(0.0)]);
    }
}
