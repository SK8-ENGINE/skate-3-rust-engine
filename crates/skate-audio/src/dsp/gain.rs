//! Gain (Gai0, spec §4.6): a 64-sample linear de-click on every target change, then flat.
//! On the graph's first block the applied gain jumps to the target (no ramp).

#[derive(Clone, Debug)]
pub struct Gain {
    /// Parameter 0: target gain (linear amplitude).
    pub target: f32,
    applied: f32,
    started: bool,
}

impl Default for Gain {
    fn default() -> Self {
        Self { target: 1.0, applied: 1.0, started: false }
    }
}

impl Gain {
    pub fn applied(&self) -> f32 {
        self.applied
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]]) {
        if !self.started {
            self.applied = self.target;
            self.started = true;
        }
        let start = self.applied;
        // Two roundings, never fused.
        let step = (self.target - start) * (1.0 / 64.0);
        if step == 0.0 {
            if start != 1.0 {
                for ch in channels.iter_mut() {
                    for s in ch.iter_mut() {
                        *s *= start;
                    }
                }
            }
        } else {
            for ch in channels.iter_mut() {
                ramp(ch, start, step);
            }
        }
        self.applied = self.target;
    }
}

/// The de-click kernel: samples 0..63 × a gain ramping from `start` by `step`, the rest ×
/// (start + 64·step). Lanes are built per group of 8 as (start + 8g·step) + m·step with plain
/// (unfused) single-precision operations. Against the PoC's replay-verified kernel
/// (tests/dsp_oracle.rs): the flat part and power-of-two steps are bit-exact; with an irregular
/// step 11 of 64 ramp samples differ by 1 ulp (the exact lane arithmetic is not recovered).
pub fn ramp(samples: &mut [f32], start: f32, step: f32) {
    let flat = start + 64.0 * step;
    for (k, s) in samples.iter_mut().enumerate() {
        *s *= if k < 64 { (start + (8 * (k / 8)) as f32 * step) + (k % 8) as f32 * step } else { flat };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_block_jumps_then_changes_ramp_over_64_samples() {
        let mut g = Gain { target: 0.5, ..Gain::default() };
        let mut x = vec![1.0f32; 256];
        g.process(&mut [&mut x[..]]);
        assert!(x.iter().all(|&v| v == 0.5));
        g.target = 0.0;
        let mut x = vec![1.0f32; 256];
        g.process(&mut [&mut x[..]]);
        assert_eq!(x[0], 0.5);
        assert_eq!(x[32], 0.5 + 32.0 * (-0.5 / 64.0));
        assert_eq!(x[63], (0.5 + 56.0 * (-0.5 / 64.0)) + 7.0 * (-0.5 / 64.0));
        assert!(x[64..].iter().all(|&v| v == 0.0));
        assert_eq!(g.applied(), 0.0);
    }
}
