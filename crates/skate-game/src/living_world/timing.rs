//! Per-system wall time of the living world's fixed-step systems, summed per frame and read (and reset) by the
//! `FRAME_HITCH` line (`frame_timing/hitch.rs`), so a hitch names the living-world system that took the time
//! (2026-10-13: 0.2 to 0.9 s hitches inside the fixed step that no measured stage covered).
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Measured systems, in `FRAME_HITCH` order.
pub(crate) const NAMES: [&str; 10] =
    ["population", "dmo_stream", "ped_obstacles", "think_peds", "advance_peds", "npc_avoid", "npc_advance", "npc_sim", "traffic", "vehicle_contacts"];
pub(crate) const POPULATION: usize = 0;
pub(crate) const DMO_STREAM: usize = 1;
pub(crate) const PED_OBSTACLES: usize = 2;
pub(crate) const THINK_PEDS: usize = 3;
pub(crate) const ADVANCE_PEDS: usize = 4;
pub(crate) const NPC_AVOID: usize = 5;
pub(crate) const NPC_ADVANCE: usize = 6;
pub(crate) const NPC_SIM: usize = 7;
pub(crate) const TRAFFIC: usize = 8;
pub(crate) const VEHICLE_CONTACTS: usize = 9;

static US: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];
/// The same sums since the last `LIVING_TIMING` line (`system_timing.rs`).
static SECOND_US: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];

/// Adds the time until it is dropped to system `index`.
pub(crate) struct Span(usize, Instant);

pub(crate) fn span(index: usize) -> Span {
    Span(index, Instant::now())
}

impl Drop for Span {
    fn drop(&mut self) {
        let us = self.1.elapsed().as_micros() as u64;
        US[self.0].fetch_add(us, Ordering::Relaxed);
        SECOND_US[self.0].fetch_add(us, Ordering::Relaxed);
    }
}

/// This frame's sums in microseconds, reset to zero.
pub(crate) fn take() -> [u64; NAMES.len()] {
    std::array::from_fn(|i| US[i].swap(0, Ordering::Relaxed))
}

/// The sums since the last call in microseconds, reset to zero.
pub(crate) fn take_second() -> [u64; NAMES.len()] {
    std::array::from_fn(|i| SECOND_US[i].swap(0, Ordering::Relaxed))
}

/// Per-event costs of in-play loading work (`LOAD_EVENTS` line, `system_timing.rs`): count, total and longest.
pub(crate) const EVENT_NAMES: [&str; 6] = ["ped_tint_miss", "ped_bind", "npc_bind", "fade_copy", "dmo_pass", "ped_tint_hit"];
pub(crate) const EV_PED_TINT_MISS: usize = 0;
pub(crate) const EV_PED_BIND: usize = 1;
pub(crate) const EV_NPC_BIND: usize = 2;
pub(crate) const EV_FADE_COPY: usize = 3;
pub(crate) const EV_DMO_PASS: usize = 4;
pub(crate) const EV_PED_TINT_HIT: usize = 5;
static EV_COUNT: [AtomicU64; EVENT_NAMES.len()] = [const { AtomicU64::new(0) }; EVENT_NAMES.len()];
static EV_US: [AtomicU64; EVENT_NAMES.len()] = [const { AtomicU64::new(0) }; EVENT_NAMES.len()];
static EV_MAX_US: [AtomicU64; EVENT_NAMES.len()] = [const { AtomicU64::new(0) }; EVENT_NAMES.len()];

/// Adds one event of `kind` lasting until it is dropped.
pub(crate) struct Event(usize, Instant);

pub(crate) fn event(kind: usize) -> Event {
    Event(kind, Instant::now())
}

impl Drop for Event {
    fn drop(&mut self) {
        let us = self.1.elapsed().as_micros() as u64;
        EV_COUNT[self.0].fetch_add(1, Ordering::Relaxed);
        EV_US[self.0].fetch_add(us, Ordering::Relaxed);
        EV_MAX_US[self.0].fetch_max(us, Ordering::Relaxed);
    }
}

/// (count, total us, longest us) per event kind since the last call, reset to zero.
pub(crate) fn take_events() -> [(u64, u64, u64); EVENT_NAMES.len()] {
    std::array::from_fn(|i| (EV_COUNT[i].swap(0, Ordering::Relaxed), EV_US[i].swap(0, Ordering::Relaxed), EV_MAX_US[i].swap(0, Ordering::Relaxed)))
}
