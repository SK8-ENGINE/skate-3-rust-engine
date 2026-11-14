//! `FRAME_HITCH` log line per hitch frame (rate-limited); observation only, thresholds via `SKATE_FRAME_HITCH`.
//! Details: docs/hails-additions/25-perf-diagnostics.md#frame-hitch-line
use bevy::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// When a frame counts as a hitch and how often the line may be written.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct HitchConfig {
    pub enabled: bool,
    /// Hitch when the frame is longer than this times its expected time.
    pub factor: f32,
    /// Hitch when the frame is longer than this, whatever the median (ms).
    pub floor_ms: f32,
    /// At most one line per this many seconds; the ones in between are counted (`suppressed=`).
    pub interval_s: f32,
}

impl Default for HitchConfig {
    fn default() -> Self {
        Self { enabled: true, factor: super::stats::HITCH_FACTOR, floor_ms: 50.0, interval_s: 0.5 }
    }
}

impl HitchConfig {
    /// The defaults with `SKATE_FRAME_HITCH` applied (unknown keys and bad values are ignored).
    pub(crate) fn parse(spec: Option<&str>) -> Self {
        let mut config = Self::default();
        let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
            return config;
        };
        if matches!(spec, "0" | "off" | "false") {
            config.enabled = false;
            return config;
        }
        for (key, value) in spec.split(',').filter_map(|kv| kv.split_once('=')) {
            let Ok(v) = value.trim().parse::<f32>() else { continue };
            if !v.is_finite() || v < 0.0 {
                continue;
            }
            match key.trim() {
                "factor" => config.factor = v,
                "floor_ms" => config.floor_ms = v,
                "interval_s" => config.interval_s = v,
                _ => {}
            }
        }
        config
    }

    pub(crate) fn is_hitch(&self, ms: f32, median_ms: Option<f32>) -> bool {
        self.enabled
            && ((self.floor_ms > 0.0 && ms > self.floor_ms)
                || median_ms.is_some_and(|m| m > 0.0 && self.factor > 0.0 && ms > self.factor * m))
    }
}

/// Wall time of the frame's stages between marker systems (`pre_update`, `post_render` span the frame edges).
pub(crate) const STAGES: [&str; 9] = ["fixed_input", "fixed_controls", "fixed_physics", "assets", "present", "animation", "audio", "pre_update", "post_render"];
const F_INPUT: usize = 0;
const F_CONTROLS: usize = 1;
const F_PHYSICS: usize = 2;
const U_ASSETS: usize = 3;
const U_PRESENT: usize = 4;
const U_ANIMATION: usize = 5;
const U_AUDIO: usize = 6;
const U_PRE: usize = 7;
const U_POST: usize = 8;
const NONE: usize = usize::MAX;

/// Phases inside the physics tick, summed over the frame's ticks; `solve_*` split `solve`, `solve_npc` = sim NPCs.
pub(crate) const PHASES: [&str; 11] = ["anim_graphs", "solve", "finish_skater", "scoring", "hud_advance", "hud_render", "solve_queries", "solve_contacts", "solve_solver", "solve_rest", "solve_npc"];
pub(crate) const PHASE_SOLVE_QUERIES: usize = 6;
pub(crate) const PHASE_SOLVE_CONTACTS: usize = 7;
pub(crate) const PHASE_SOLVE_SOLVER: usize = 8;
pub(crate) const PHASE_SOLVE_REST: usize = 9;
pub(crate) const PHASE_SOLVE_NPC: usize = 10;
pub(crate) const PHASE_ANIM_GRAPHS: usize = 0;
pub(crate) const PHASE_SOLVE: usize = 1;
pub(crate) const PHASE_FINISH: usize = 2;
pub(crate) const PHASE_SCORING: usize = 3;
pub(crate) const PHASE_HUD_ADVANCE: usize = 4;
pub(crate) const PHASE_HUD_RENDER: usize = 5;
static PHASE_US: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];
/// The same sums since the last `PHYSICS_PHASES` line (`system_timing.rs`).
static PHASE_SECOND_US: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];

fn add_phase_us(phase: usize, us: u64) {
    PHASE_US[phase].fetch_add(us, Ordering::Relaxed);
    PHASE_SECOND_US[phase].fetch_add(us, Ordering::Relaxed);
}

/// The phase sums since the last call (µs), reset to zero.
pub(crate) fn take_phase_second() -> [u64; 11] {
    std::array::from_fn(|i| PHASE_SECOND_US[i].swap(0, Ordering::Relaxed))
}

/// Runs `f`, adding its wall time to physics phase `phase` (always on: one clock read each side).
pub(crate) fn phase<R>(phase: usize, f: impl FnOnce() -> R) -> R {
    let t = Instant::now();
    let r = f();
    add_phase_us(phase, t.elapsed().as_micros() as u64);
    r
}

/// Adds the time since `started` to physics phase `phase`.
pub(crate) fn add_phase(phase: usize, started: Instant) {
    add_phase_us(phase, started.elapsed().as_micros() as u64);
}

/// Takes (and resets) the physics phase sums (µs), as the log line does each frame.
#[cfg(test)]
pub(crate) fn take_phases() -> [u64; 11] {
    std::array::from_fn(|i| PHASE_US[i].swap(0, Ordering::Relaxed))
}

#[derive(Resource, Default)]
pub(crate) struct HitchSpans {
    open: [Option<Instant>; 9],
    us: [u64; 9],
}

fn boundary<const END: usize, const BEGIN: usize>(mut spans: ResMut<HitchSpans>) {
    let now = Instant::now();
    if END != NONE {
        if let Some(t) = spans.open[END].take() {
            spans.us[END] += now.duration_since(t).as_micros() as u64;
        }
    }
    if BEGIN != NONE {
        spans.open[BEGIN] = Some(now);
    }
}

#[derive(Default)]
pub(crate) struct LogState {
    last_line_s: Option<f64>,
    suppressed: u32,
}

/// What the line needs from the frame timing (the closed frame).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClosedFrame {
    pub frame: u64,
    pub ms: f32,
    pub median_ms: Option<f32>,
    pub main_ms: f32,
    pub fixed_ms: f32,
    pub steps: u32,
    pub now_s: f64,
}

pub(crate) fn register(app: &mut App) {
    use crate::app::{FrameSet, SimulationSet};
    let config = HitchConfig::parse(std::env::var("SKATE_FRAME_HITCH").ok().as_deref());
    app.insert_resource(config)
        .init_resource::<HitchSpans>()
        .add_systems(First, log_hitch.after(super::begin_frame))
        .add_systems(FixedUpdate, (
            boundary::<NONE, F_INPUT>.before(SimulationSet::Input),
            boundary::<F_INPUT, F_CONTROLS>.after(SimulationSet::Input).before(SimulationSet::Controls),
            boundary::<F_CONTROLS, F_PHYSICS>.after(SimulationSet::Controls).before(SimulationSet::Physics),
            boundary::<F_PHYSICS, NONE>.after(SimulationSet::Physics),
        ))
        .add_systems(Update, (
            boundary::<U_PRE, U_ASSETS>.before(FrameSet::Assets),
            boundary::<U_ASSETS, U_PRESENT>.after(FrameSet::Assets).before(FrameSet::Physics),
            boundary::<U_PRESENT, U_ANIMATION>.after(FrameSet::Physics).before(FrameSet::Animation),
            boundary::<U_ANIMATION, U_AUDIO>.after(FrameSet::Animation),
            boundary::<U_AUDIO, U_POST>.after(crate::game_audio::CueSet),
        ));
}

/// After `begin_frame` closed the previous frame: write the line if it was a hitch. Every frame it
/// only takes (resets) the stage sums; the player / entity reads happen on hitch frames only.
#[allow(clippy::too_many_arguments)]
fn log_hitch(
    timing: Res<super::FrameTiming>,
    config: Res<HitchConfig>,
    mut spans: ResMut<HitchSpans>,
    physics: Option<Res<crate::physics::GamePhysics>>,
    skater: Option<Res<crate::physics::SkaterRuntime>>,
    entities: Query<Entity>,
    map: Option<Res<crate::map_transition::CurrentMap>>,
    mut state: Local<LogState>,
) {
    // `post_render` runs from the end of the last frame's Update to here; `pre_update` from here to this Update.
    let now = Instant::now();
    if let Some(t) = spans.open[U_POST].take() {
        spans.us[U_POST] += now.duration_since(t).as_micros() as u64;
    }
    let stages = std::mem::take(&mut spans.us);
    spans.open = [None; 9];
    spans.open[U_PRE] = Some(now);
    let phases: [u64; 11] = std::array::from_fn(|i| PHASE_US[i].swap(0, Ordering::Relaxed));
    let lock_us = crate::game_audio::timing::FRAME_GAME_LOCK_US.swap(0, Ordering::Relaxed);
    let living = crate::living_world::timing::take();
    let audio_parts = crate::game_audio::hitch_parts::take();
    let render = super::render_split::take();
    let closed = timing.closed();
    if closed.frame == 0 || !config.is_hitch(closed.ms, closed.median_ms) {
        return;
    }
    if state.last_line_s.is_some_and(|t| closed.now_s - t < f64::from(config.interval_s)) {
        state.suppressed += 1;
        return;
    }
    let suppressed = std::mem::take(&mut state.suppressed);
    state.last_line_s = Some(closed.now_s);
    let player = match (physics.as_deref(), skater.as_deref()) {
        (Some(physics), Some(skater)) => {
            let wheels = (0..4).filter(|&i| physics.riding.ground.parts[i].in_contact).count();
            let st = skater.player_state.current() as u32;
            let p = physics.board.bodies().first().map(|b| b.rates.position);
            let trick = skater.scoring.trick_name();
            format!(
                " state={st} airborne={} wheels={wheels} body_flip={} trick=\"{}\" trick_seq={} pos=[{:.1}, {:.1}, {:.1}] tick={}",
                (200..300).contains(&st) && wheels == 0,
                skater.player_input.physical.air.flag_441 != 0,
                trick.escape_debug(),
                skater.scoring.trick_seq(),
                p.map_or(0.0, |p| p.x), p.map_or(0.0, |p| p.y), p.map_or(0.0, |p| p.z),
                physics.ticks,
            )
        }
        _ => String::new(),
    };
    let fmt = |names: &[&str], us: &[u64]| {
        names.iter().zip(us).map(|(n, u)| format!("{n}:{:.1}", *u as f64 / 1000.0)).collect::<Vec<_>>().join(" ")
    };
    // The living world's fixed-step systems (outside the measured stages), largest first, those over 1 ms.
    let mut lw: Vec<(&str, u64)> = crate::living_world::timing::NAMES.iter().copied().zip(living).filter(|(_, u)| *u >= 1000).collect();
    lw.sort_by(|a, b| b.1.cmp(&a.1));
    let living_line = lw.iter().map(|(n, u)| format!("{n}:{:.1}", *u as f64 / 1000.0)).collect::<Vec<_>>().join(" ");
    // The game audio's frame systems (the `audio` stage and the Native bank loads), largest first, those over 1 ms.
    let mut au: Vec<(&str, u64)> = crate::game_audio::hitch_parts::NAMES.iter().copied().zip(audio_parts).filter(|(_, u)| *u >= 1000).collect();
    au.sort_by(|a, b| b.1.cmp(&a.1));
    let audio_line = au.iter().map(|(n, u)| format!("{n}:{:.1}", *u as f64 / 1000.0)).collect::<Vec<_>>().join(" ");
    // Time no stage covers (research b552: most big hitches had ~33 ms there, so `slowest` named a small stage):
    // fixed steps outside the three fixed stages (e.g. `modding::fixed`), and main-thread time outside the stages.
    // `post_render` also holds the render / present wait, so it stays out of that sum: `update_other` is then the
    // CPU part of it plus anything between the other stages.
    let us = |ms: f32| (f64::from(ms) * 1000.0).max(0.0) as u64;
    let fixed_other = us(closed.fixed_ms).saturating_sub(stages[F_INPUT] + stages[F_CONTROLS] + stages[F_PHYSICS]);
    let update_other = us(closed.main_ms).saturating_sub(stages[..U_POST].iter().sum());
    let others = [("fixed_other", fixed_other), ("update_other", update_other)];
    let slowest = STAGES.iter().copied().zip(stages.iter().copied()).chain(PHASES.iter().copied().zip(phases.iter().copied())).chain(others)
        .max_by_key(|(_, u)| *u).map_or("", |(n, _)| n);
    info!(
        "FRAME_HITCH frame={} ms={:.1} median_ms={} main_ms={:.1} fixed_ms={:.1} steps={} wait_ms={:.1} audio_lock_ms={:.1} slowest={slowest} map={} stages=[{}] fixed_other:{:.1} update_other:{:.1} phases=[{}] living_world=[{living_line}] audio_parts=[{audio_line}] render=[{}]{player} entities={} voices={} aems_instances={} suppressed={suppressed}",
        closed.frame,
        closed.ms,
        closed.median_ms.map_or("-".to_owned(), |m| format!("{m:.1}")),
        closed.main_ms,
        closed.fixed_ms,
        closed.steps,
        (closed.ms - closed.main_ms).max(0.0),
        lock_us as f64 / 1000.0,
        map.as_deref().map_or("-", |m| m.name.as_str()),
        fmt(&STAGES, &stages),
        fixed_other as f64 / 1000.0,
        update_other as f64 / 1000.0,
        fmt(&PHASES, &phases),
        super::render_split::format(&render),
        entities.iter().count(),
        crate::game_audio::timing::LAST_BLOCK_VOICES.load(Ordering::Relaxed),
        crate::game_audio::timing::LAST_BLOCK_INSTANCES.load(Ordering::Relaxed),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_overrides() {
        let d = HitchConfig::parse(None);
        assert_eq!(d, HitchConfig::default());
        assert!(d.enabled && d.factor == 2.0 && d.floor_ms == 50.0);
        assert!(!HitchConfig::parse(Some("off")).enabled);
        let c = HitchConfig::parse(Some("factor=3, floor_ms=40,interval_s=0,bogus=1,factor_x=nan"));
        assert_eq!((c.factor, c.floor_ms, c.interval_s), (3.0, 40.0, 0.0));
        let bad = HitchConfig::parse(Some("factor=-1,floor_ms=abc"));
        assert_eq!(bad, HitchConfig::default());
    }

    #[test]
    fn hitch_rule_median_or_floor() {
        let c = HitchConfig::default();
        assert!(!c.is_hitch(9.0, Some(5.0)));
        assert!(c.is_hitch(11.0, Some(5.0)));
        assert!(c.is_hitch(51.0, None), "the floor needs no history");
        assert!(!c.is_hitch(40.0, None));
        assert!(!HitchConfig { enabled: false, ..c }.is_hitch(500.0, Some(5.0)));
    }

    /// Marker and log systems write only their own resource (gameplay identical with the log).
    #[test]
    fn systems_write_only_their_own_state() {
        use bevy::ecs::system::{IntoSystem, System};
        let mut world = World::new();
        world.init_resource::<HitchSpans>();
        world.init_resource::<HitchConfig>();
        world.init_resource::<super::super::FrameTiming>();
        let own = world.resource_id::<HitchSpans>().unwrap();
        let mut systems: Vec<Box<dyn System<In = (), Out = ()>>> = vec![
            Box::new(IntoSystem::into_system(boundary::<NONE, F_INPUT>)),
            Box::new(IntoSystem::into_system(boundary::<F_INPUT, NONE>)),
            Box::new(IntoSystem::into_system(log_hitch)),
        ];
        for system in &mut systems {
            let access = system.initialize(&mut world);
            let combined = access.combined_access();
            assert!(!combined.has_write_all(), "{:?}", system.name());
            assert!(!combined.has_any_component_write(), "{:?}", system.name());
            for write in combined.resource_writes() {
                assert_eq!(write, own, "{:?} writes a foreign resource", system.name());
            }
        }
    }

    #[test]
    fn boundary_sums_spans_across_fixed_steps() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<HitchSpans>();
        for _ in 0..3 {
            world.run_system_once(boundary::<NONE, F_PHYSICS>).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
            world.run_system_once(boundary::<F_PHYSICS, NONE>).unwrap();
        }
        let spans = world.resource::<HitchSpans>();
        assert!(spans.us[F_PHYSICS] >= 6_000, "{}", spans.us[F_PHYSICS]);
        assert!(spans.open.iter().all(Option::is_none));
    }
}
