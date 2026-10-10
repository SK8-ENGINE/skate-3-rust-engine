//! Opt-in cost readout of the game audio (`SKATE_AUDIO_TIMING=1`): once per second one
//! `AUDIO_TIMING` log line with the slowest and average time of each audio system on the game
//! thread, the time spent waiting for the native runtime's lock (game thread and audio thread),
//! the native render per 256-frame block, the load each block rendered (mixer voices, live AEMS
//! instances, grain voices: max / average per block), and the slowest frame. Off: one relaxed atomic load per
//! scope. For finding stutter in real play (`tools/audio-bench/`); measuring only, no behaviour.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bevy::prelude::*;

pub(crate) struct Stat {
    name: &'static str,
    max: AtomicU64,
    sum: AtomicU64,
    n: AtomicU64,
}

impl Stat {
    const fn new(name: &'static str) -> Self {
        Self { name, max: AtomicU64::new(0), sum: AtomicU64::new(0), n: AtomicU64::new(0) }
    }
    pub(crate) fn add(&self, us: u64) {
        self.max.fetch_max(us, Ordering::Relaxed);
        self.sum.fetch_add(us, Ordering::Relaxed);
        self.n.fetch_add(1, Ordering::Relaxed);
    }
    fn take(&self) -> (u64, u64, u64) {
        (self.max.swap(0, Ordering::Relaxed), self.sum.swap(0, Ordering::Relaxed), self.n.swap(0, Ordering::Relaxed))
    }
}

/// The render's load per block (counts, not µs): `name=max/avg×blocks` in the line, the average
/// with one decimal. Ties the render cost to what it rendered.
pub(crate) struct Gauge {
    name: &'static str,
    max: AtomicU64,
    sum: AtomicU64,
    n: AtomicU64,
}

impl Gauge {
    const fn new(name: &'static str) -> Self {
        Self { name, max: AtomicU64::new(0), sum: AtomicU64::new(0), n: AtomicU64::new(0) }
    }
    pub(crate) fn add(&self, v: u64) {
        self.max.fetch_max(v, Ordering::Relaxed);
        self.sum.fetch_add(v, Ordering::Relaxed);
        self.n.fetch_add(1, Ordering::Relaxed);
    }
    fn take(&self) -> (u64, u64, u64) {
        (self.max.swap(0, Ordering::Relaxed), self.sum.swap(0, Ordering::Relaxed), self.n.swap(0, Ordering::Relaxed))
    }
}

/// Mixer voices (AEMS, Splice, streams), live AEMS instances and grain voices per rendered block.
pub(crate) static BLOCK_VOICES: Gauge = Gauge::new("block_voices");
pub(crate) static BLOCK_INSTANCES: Gauge = Gauge::new("block_instances");
pub(crate) static BLOCK_GRAINS: Gauge = Gauge::new("block_grains");
static GAUGES: [&Gauge; 3] = [&BLOCK_VOICES, &BLOCK_INSTANCES, &BLOCK_GRAINS];

/// The mixer's running totals of opens refused at the voice cap and of finished voices freed to
/// make room (`Mixer::refused_cap` / `Mixer::evicted`), as of the last rendered block; the line
/// shows them per second (`voices_refused` / `voices_evicted`, only when not 0).
static MIXER_REFUSED_CAP: AtomicU64 = AtomicU64::new(0);
static MIXER_EVICTED: AtomicU64 = AtomicU64::new(0);

/// Record one rendered block's load (when the readout is on). Reads counters only: no allocation,
/// no state change.
pub(crate) fn block_load(rt: &skate_audio::runtime::Runtime) {
    // Always on (two relaxed stores per block): the `FRAME_HITCH` line's voice counts.
    LAST_BLOCK_VOICES.store(rt.mixer.voice_count() as u64, Ordering::Relaxed);
    LAST_BLOCK_INSTANCES.store(rt.eval.instance_count() as u64, Ordering::Relaxed);
    if !on() {
        return;
    }
    BLOCK_VOICES.add(rt.mixer.voice_count() as u64);
    MIXER_REFUSED_CAP.store(rt.mixer.refused_cap, Ordering::Relaxed);
    MIXER_EVICTED.store(rt.mixer.evicted, Ordering::Relaxed);
    BLOCK_INSTANCES.add(rt.eval.instance_count() as u64);
    let npc = rt.npc_grains.as_deref().map_or(0, skate_audio::grain::GrainBed::voices);
    BLOCK_GRAINS.add((rt.grains.voices() + npc) as u64);
}

pub(crate) static MIXMAP_FRAME: Stat = Stat::new("mixmap_frame");
pub(crate) static GRAIN_BED: Stat = Stat::new("grain_bed");
pub(crate) static OBSERVE: Stat = Stat::new("observe");
pub(crate) static PLAY: Stat = Stat::new("cues");
pub(crate) static EMITTERS: Stat = Stat::new("emitters");
pub(crate) static VOICES: Stat = Stat::new("voices");
pub(crate) static GAME_LOCK: Stat = Stat::new("game_lock_wait");
pub(crate) static AUDIO_LOCK: Stat = Stat::new("audio_lock_wait");
pub(crate) static RENDER: Stat = Stat::new("render_block");
static FRAME: Stat = Stat::new("frame");
static ALL: [&Stat; 10] = [&FRAME, &MIXMAP_FRAME, &GRAIN_BED, &OBSERVE, &PLAY, &EMITTERS, &VOICES, &GAME_LOCK, &AUDIO_LOCK, &RENDER];

pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_AUDIO_TIMING").is_ok_and(|v| v == "1"))
}

/// Records the time from creation to drop into `stat` (when the readout is on).
pub(crate) struct Scope(Option<(Instant, &'static Stat)>);
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some((t, stat)) = self.0 {
            stat.add(t.elapsed().as_micros() as u64);
        }
    }
}
pub(crate) fn scope(stat: &'static Stat) -> Scope {
    Scope(on().then(|| (Instant::now(), stat)))
}

/// Lock `m`, recording the wait into `stat`.
pub(crate) fn lock<'a, T>(m: &'a std::sync::Mutex<T>, stat: &'static Stat) -> std::sync::LockResult<std::sync::MutexGuard<'a, T>> {
    if !on() && !std::ptr::eq(stat, &GAME_LOCK) {
        return m.lock();
    }
    let t = Instant::now();
    let g = m.lock();
    let us = t.elapsed().as_micros() as u64;
    if std::ptr::eq(stat, &GAME_LOCK) {
        // Always on for the game thread: the `FRAME_HITCH` line's wait for the audio render.
        FRAME_GAME_LOCK_US.fetch_add(us, Ordering::Relaxed);
    }
    if on() {
        stat.add(us);
    }
    g
}

/// Mixer voices and live AEMS instances of the last rendered block (always kept, `FRAME_HITCH`).
pub(crate) static LAST_BLOCK_VOICES: AtomicU64 = AtomicU64::new(0);
pub(crate) static LAST_BLOCK_INSTANCES: AtomicU64 = AtomicU64::new(0);
/// The game thread's wait for the native runtime's lock since the frame log last took it (µs).
pub(crate) static FRAME_GAME_LOCK_US: AtomicU64 = AtomicU64::new(0);

/// Once per second: the `AUDIO_TIMING` line (max / average µs and count per stat).
pub(crate) fn report(time: Res<Time<Real>>, mut clock: Local<f32>, mut last: Local<(u64, u64)>) {
    if !on() {
        return;
    }
    FRAME.add((time.delta_secs_f64() * 1e6) as u64);
    *clock += time.delta_secs();
    if *clock < 1.0 {
        return;
    }
    *clock = 0.0;
    let mut line = String::from("AUDIO_TIMING");
    for s in ALL {
        let (max, sum, n) = s.take();
        if n > 0 {
            line.push_str(&format!(" {}={}/{}us×{}", s.name, max, sum / n, n));
        }
    }
    for g in GAUGES {
        let (max, sum, n) = g.take();
        if n > 0 {
            line.push_str(&format!(" {}={}/{:.1}×{}", g.name, max, sum as f64 / n as f64, n));
        }
    }
    let totals = (MIXER_REFUSED_CAP.load(Ordering::Relaxed), MIXER_EVICTED.load(Ordering::Relaxed));
    let (refused, evicted) = (totals.0.saturating_sub(last.0), totals.1.saturating_sub(last.1));
    *last = totals;
    if refused > 0 {
        line.push_str(&format!(" voices_refused={refused}"));
    }
    if evicted > 0 {
        line.push_str(&format!(" voices_evicted={evicted}"));
    }
    let dropped = super::state_log::dropped();
    if dropped > 0 {
        line.push_str(&format!(" state_log_dropped={dropped}"));
    }
    info!("{line}");
}
