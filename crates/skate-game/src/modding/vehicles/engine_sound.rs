//! Original continuous exhaust synthesizer.
use bevy::{audio::Source, prelude::*};
use std::time::Duration;
#[derive(Asset, TypePath)]
pub struct Engine;
pub struct Exhaust { phase: f32, noise: u32, filtered: f32 }
impl Iterator for Exhaust {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.phase = (self.phase + 80. / 48000.) % 1.;
        self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = self.noise as f32 / u32::MAX as f32 * 2. - 1.;
        self.filtered += 0.12 * (noise - self.filtered);
        let p = self.phase * std::f32::consts::TAU;
        let pulse = p.sin() * 0.48 + (p * 2.).sin() * 0.24
            + (p * 3.).sin() * 0.12 + (p * 5.).sin() * 0.06;
        Some((pulse + self.filtered * 0.18) * 0.65)
    }
}
impl Source for Exhaust {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 1 }
    fn sample_rate(&self) -> u32 { 48000 }
    fn total_duration(&self) -> Option<Duration> { None }
}
impl Decodable for Engine {
    type DecoderItem = f32;
    type Decoder = Exhaust;
    fn decoder(&self) -> Exhaust { Exhaust { phase: 0., noise: 17, filtered: 0. } }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhaust_is_bounded_continuous_and_not_silent() {
        let mut source = Engine.decoder();
        let mut last = 0.;
        let mut energy = 0.;
        for _ in 0..48000 {
            let sample = source.next().unwrap();
            assert!(sample.is_finite() && sample.abs() < 0.8);
            assert!((sample - last).abs() < 0.1);
            energy += sample * sample;
            last = sample;
        }
        assert!(energy > 100.);
    }
}
