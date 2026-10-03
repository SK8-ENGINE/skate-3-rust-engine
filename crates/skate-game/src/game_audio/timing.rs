//! Opt-in cost readout of the game audio (`SKATE_AUDIO_TIMING=1`): once per second one
//! `AUDIO_TIMING` log line with the slowest and average time of each audio system on the game
//! thread, the time spent waiting for the native runtime's lock (game thread and audio thread),
//! the native render per 256-frame block, and the slowest frame. Off: one relaxed atomic load per
//! scope. For finding stutter in real play (skill `optimisation`); measuring only, no behaviour.
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
    if !on() {
        return m.lock();
    }
    let t = Instant::now();
    let g = m.lock();
    stat.add(t.elapsed().as_micros() as u64);
    g
}

/// Once per second: the `AUDIO_TIMING` line (max / average µs and count per stat).
pub(crate) fn report(time: Res<Time<Real>>, mut clock: Local<f32>) {
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
    let dropped = super::state_log::dropped();
    if dropped > 0 {
        line.push_str(&format!(" state_log_dropped={dropped}"));
    }
    info!("{line}");
}
