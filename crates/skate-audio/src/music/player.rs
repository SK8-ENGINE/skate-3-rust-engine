//! The music stream: decoded segments queued back to back as one continuous stream, resampled to the mix rate with
//! the voices' resampler and added to the default bus's front left / right every block (gapless joins). The host
//! decodes segments from the music banks and keeps the queue topped up ([`MusicPlayer::queued_seconds`]); which
//! segment comes next is the host's (the PathFinder graph, research b1016). Docs: 11-audio.md#music-banks.
use std::collections::VecDeque;

use crate::dsp::resample::{ratio, Resampler};
use crate::mixer::Pcm;
use crate::BLOCK;

/// One queued segment: where it starts in the stream and its frames (stereo; a mono segment plays on both sides).
struct Queued {
    start: u64,
    id: u32,
    left: Vec<f32>,
    right: Vec<f32>,
}

#[derive(Default)]
pub struct MusicPlayer {
    queue: VecDeque<Queued>,
    /// Frames queued in the stream so far (the next segment's start).
    end: u64,
    rate: u32,
    resampler: Resampler,
    /// Linear gain on the stream (the host's music volume, ducking, zone volume).
    pub gain: f32,
    scratch: Vec<f32>,
    /// Segments that started playing since the host last took them (ids in order).
    started: Vec<u32>,
    announced: u64,
    /// Segments at another rate than the stream's: they start once the stream has played out (a new stream at their
    /// rate; at most one block between them).
    waiting: VecDeque<(u32, Pcm)>,
}

impl MusicPlayer {
    pub fn new() -> Self {
        Self { gain: 1.0, ..Self::default() }
    }

    /// Queues a decoded segment after the last one. A segment at another rate than the stream's (a player's own bank
    /// after the disc's) waits until the stream has played out and then starts a stream at its rate.
    pub fn push(&mut self, id: u32, pcm: Pcm) -> Result<(), String> {
        let frames = pcm.channels.first().map_or(0, Vec::len);
        if frames == 0 || pcm.rate == 0 {
            return Ok(());
        }
        let idle = self.queue.is_empty() && self.end <= self.resampler.position;
        if !self.waiting.is_empty() || (!idle && pcm.rate != self.rate) {
            self.waiting.push_back((id, pcm));
            return Ok(());
        }
        self.append(id, pcm, idle);
        Ok(())
    }

    fn append(&mut self, id: u32, pcm: Pcm, restart: bool) {
        let frames = pcm.channels.first().map_or(0, Vec::len);
        if restart {
            // Nothing playing: a new stream starts at this segment.
            self.queue.clear();
            self.end = 0;
            self.announced = 0;
            self.resampler = Resampler::default();
            self.rate = pcm.rate;
        }
        let mut channels = pcm.channels.into_iter();
        let left = channels.next().unwrap_or_default();
        let right = channels.next().unwrap_or_else(|| left.clone());
        self.queue.push_back(Queued { start: self.end, id, left, right });
        self.end += frames as u64;
    }

    /// Stops at once and forgets the queue.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.waiting.clear();
        self.end = 0;
        self.announced = 0;
        self.resampler = Resampler::default();
    }

    /// Seconds of music queued ahead of the play position (waiting segments included).
    pub fn queued_seconds(&self) -> f32 {
        let stream = if self.rate == 0 { 0.0 } else { self.end.saturating_sub(self.resampler.position) as f32 / self.rate as f32 };
        stream + self.waiting.iter().map(|(_, p)| p.channels.first().map_or(0, Vec::len) as f32 / p.rate as f32).sum::<f32>()
    }

    pub fn playing(&self) -> bool {
        (!self.queue.is_empty() && self.resampler.position < self.end) || !self.waiting.is_empty()
    }

    /// Ids of the segments that started since the last call.
    pub fn take_started(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.started)
    }

    fn frame(queue: &VecDeque<Queued>, index: u64, right: bool) -> f32 {
        queue.iter().find(|q| index >= q.start && index < q.start + q.left.len() as u64)
            .map_or(0.0, |q| {
                let i = (index - q.start) as usize;
                if right { q.right[i] } else { q.left[i] }
            })
    }

    /// Adds one block into the 6-channel bus (front left 0, front right 2).
    pub fn add_to(&mut self, bus: &mut [[f32; BLOCK]; 6]) {
        if self.resampler.position >= self.end && !self.waiting.is_empty() {
            // The stream played out: the waiting segments of the next rate start a new one.
            let rate = self.waiting.front().map_or(0, |(_, p)| p.rate);
            let mut restart = true;
            while self.waiting.front().is_some_and(|(_, p)| p.rate == rate) {
                let (id, pcm) = self.waiting.pop_front().expect("checked");
                self.append(id, pcm, restart);
                restart = false;
            }
        }
        if !self.playing() {
            return;
        }
        for q in &self.queue {
            if q.start >= self.announced && q.start <= self.resampler.position {
                self.started.push(q.id);
                self.announced = q.start + 1;
            }
        }
        self.resampler.set_ratio(ratio(self.rate, 1.0));
        self.scratch.resize(BLOCK, 0.0);
        for (side, out) in [(false, 0usize), (true, 2usize)] {
            let queue = &self.queue;
            self.resampler.render(&mut self.scratch, |i| Self::frame(queue, i, side));
            for (b, s) in bus[out].iter_mut().zip(&self.scratch) {
                *b += s * self.gain;
            }
        }
        self.resampler.advance(BLOCK);
        // Segments wholly before the interpolation point leave.
        while self.queue.front().is_some_and(|q| q.start + q.left.len() as u64 + 1 < self.resampler.position) {
            self.queue.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(rate: u32, frames: usize, value: f32) -> Pcm {
        Pcm { rate, channels: vec![vec![value; frames], vec![-value; frames]] }
    }

    #[test]
    fn segments_play_back_to_back_without_a_gap() {
        let mut m = MusicPlayer::new();
        m.push(1, pcm(48_000, 300, 0.5)).unwrap();
        m.push(2, pcm(48_000, 300, 0.25)).unwrap();
        let mut seen = Vec::new();
        for _ in 0..3 {
            let mut bus = [[0.0; BLOCK]; 6];
            m.add_to(&mut bus);
            seen.extend_from_slice(&bus[0]);
            assert!(bus[2].iter().zip(&bus[0]).all(|(r, l)| (r + l).abs() < 1e-6), "right is the right channel");
        }
        assert!(seen[..300].iter().all(|&s| (s - 0.5).abs() < 1e-3), "first segment");
        assert!(seen[301..600].iter().all(|&s| (s - 0.25).abs() < 1e-3), "second follows at once");
        assert!(seen[601..].iter().all(|&s| s == 0.0), "then silence");
        assert_eq!(m.take_started(), [1, 2]);
        assert!(!m.playing());
    }

    #[test]
    fn a_segment_at_another_rate_waits_for_the_stream_and_starts_its_own() {
        let mut m = MusicPlayer::new();
        m.push(1, pcm(48_000, 300, 0.5)).unwrap();
        m.push(2, pcm(44_100, 300, 0.25)).unwrap();
        m.push(3, pcm(48_000, 10, 0.75)).unwrap();
        let mut seen = Vec::new();
        // Each rate change costs up to one block before the next stream starts.
        for _ in 0..6 {
            let mut bus = [[0.0; BLOCK]; 6];
            m.add_to(&mut bus);
            seen.extend_from_slice(&bus[0]);
        }
        assert_eq!(m.take_started(), [1, 2, 3], "in order");
        assert!(seen.iter().any(|&s| (s - 0.25).abs() < 1e-3) && seen.iter().any(|&s| (s - 0.75).abs() < 1e-3));
    }

    #[test]
    fn a_44100_stream_is_resampled_and_reports_what_is_queued() {
        let mut m = MusicPlayer::new();
        m.push(7, pcm(44_100, 44_100, 0.1)).unwrap();
        assert!((m.queued_seconds() - 1.0).abs() < 1e-6);
        let mut bus = [[0.0; BLOCK]; 6];
        m.add_to(&mut bus);
        // 256 output frames at 48 kHz consume about 235 source frames.
        assert!((m.queued_seconds() - (1.0 - 235.2 / 44_100.0)).abs() < 1e-3);
        m.gain = 0.0;
        let mut quiet = [[0.0; BLOCK]; 6];
        m.add_to(&mut quiet);
        assert!(quiet[0].iter().all(|&s| s == 0.0));
    }
}
