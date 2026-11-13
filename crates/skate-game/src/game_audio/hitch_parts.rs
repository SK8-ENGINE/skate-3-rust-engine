//! Per-system wall time of the game audio's frame systems, read and reset by `FRAME_HITCH`.
//! Details: docs/hails-additions/25-perf-diagnostics.md#audio-hitch-parts
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Measured systems, in `FRAME_HITCH` order.
pub(crate) const NAMES: [&str; 15] = [
    "content", "map_audio", "ambience", "mixmap_frame", "world_sources", "npc_skaters", "mixmap_tick", "grain_bed",
    "reverb", "emitters", "random_sets", "speech", "bridge", "bank_load", "file_read",
];
pub(crate) const CONTENT: usize = 0;
pub(crate) const MAP_AUDIO: usize = 1;
pub(crate) const AMBIENCE: usize = 2;
pub(crate) const MIXMAP_FRAME: usize = 3;
pub(crate) const WORLD_SOURCES: usize = 4;
pub(crate) const NPC_SKATERS: usize = 5;
pub(crate) const MIXMAP_TICK: usize = 6;
pub(crate) const GRAIN_BED: usize = 7;
pub(crate) const REVERB: usize = 8;
pub(crate) const EMITTERS: usize = 9;
pub(crate) const RANDOM_SETS: usize = 10;
pub(crate) const SPEECH: usize = 11;
pub(crate) const BRIDGE: usize = 12;
pub(crate) const BANK_LOAD: usize = 13;
/// A sample file read on the game thread at first play (`Library::clip`, not read ahead).
pub(crate) const FILE_READ: usize = 14;

static US: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];

/// Adds the time until it is dropped to system `index`.
pub(crate) struct Span(usize, Instant);

pub(crate) fn span(index: usize) -> Span {
    Span(index, Instant::now())
}

impl Drop for Span {
    fn drop(&mut self) {
        US[self.0].fetch_add(self.1.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
}

/// This frame's sums in microseconds, reset to zero.
pub(crate) fn take() -> [u64; NAMES.len()] {
    std::array::from_fn(|i| US[i].swap(0, Ordering::Relaxed))
}
