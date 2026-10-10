//! Frame-time diagnostics: an on-screen readout that shows stutter (frame time,
//! 1 % / 0.1 % lows, worst frame, a graph), the `SKATE_FRAME_LOG` per-frame log
//! and the read-only `frame` section of the mod snapshot.
//!
//! Replaces the old "FPS: N" counter, which averaged frames over 0.5 s: a
//! 200 ms hitch among 3 ms frames still read ~240 FPS, so stutter was invisible.
//!
//! A frame here is one iteration of the app's main loop, timed with its own
//! clock from the start of `First` to the start of the next `First`: what the
//! player sees, never the fixed 60 Hz physics step (which catches up after a
//! hitch and so always looks smooth, the trap of the audio state log, see
//! `.claude/todo/frame-timing.md`). Each frame carries its own main-thread CPU
//! split: `main_ms` (First..Last schedules) and `fixed_ms` / `fixed_steps` (the
//! physics loop inside them). A frame much longer than `main_ms` waited outside
//! the schedules: for the render thread / GPU / present, or the OS. The steps
//! that catch up after a long frame run in the following frame(s), because the
//! virtual clock advances from the render thread's timestamps.
//!
//! Observation only: these systems read clocks and write only this module's own
//! resource and its overlay entities (test `systems_touch_only_their_own_state`),
//! so gameplay is identical with the readout and the log on or off.
pub(crate) mod hitch;
pub(crate) mod log;
pub(crate) mod stats;

use bevy::app::{RunFixedMainLoop, RunFixedMainLoopSystems};
use bevy::prelude::*;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Seconds of frames the lows, the worst frame and the hitch count cover.
pub(crate) const WINDOW_S: f64 = 5.0;
/// Upper bound on stored frames (5 s at 4,000 fps); older frames drop first.
const MAX_FRAMES: usize = 20_000;
/// Readout and summary refresh period.
const REFRESH_S: f64 = 0.25;
/// Graph: bars of the worst frame per bucket.
const GRAPH_BARS: usize = 80;
const GRAPH_BUCKET_S: f64 = 0.05;
/// A bar of this frame time fills the graph height.
const GRAPH_FULL_MS: f32 = 50.0;
/// Simple mode: the latest bars only (2 s), drawn small.
const SIMPLE_BARS: usize = 40;

#[derive(Clone, Copy)]
struct Sample {
    end_s: f64,
    ms: f32,
    hitch: bool,
}

/// Frame statistics. Always collected (a push per frame); mods read them
/// through the snapshot's `frame` section.
#[derive(Resource)]
pub(crate) struct FrameTiming {
    frame: u64,
    last_ms: f32,
    last_steps: u32,
    last_hitch: bool,
    steps_pending: u32,
    last_main_ms: f32,
    last_fixed_ms: f32,
    last_median_ms: Option<f32>,
    last_now_s: f64,
    origin: Instant,
    /// Start of the current frame: monotonic and wall clock.
    frame_started: Option<(Instant, f64)>,
    fixed_started: Option<Instant>,
    fixed_pending: Duration,
    /// The current frame's CPU split, from `record` (Last).
    pending: Option<FrameCpu>,
    window: VecDeque<Sample>,
    history: VecDeque<f32>,
    summary: stats::Summary,
    hitches: usize,
    interval: Interval,
    shown: Interval,
    next_refresh_s: f64,
    scratch: Vec<f32>,
    legacy: Option<(u32, f64)>,
}

impl Default for FrameTiming {
    fn default() -> Self {
        Self {
            frame: 0,
            last_ms: 0.0,
            last_steps: 0,
            last_hitch: false,
            steps_pending: 0,
            last_main_ms: 0.0,
            last_fixed_ms: 0.0,
            last_median_ms: None,
            last_now_s: 0.0,
            origin: Instant::now(),
            frame_started: None,
            fixed_started: None,
            fixed_pending: Duration::ZERO,
            pending: None,
            window: VecDeque::new(),
            history: VecDeque::with_capacity(stats::HITCH_HISTORY + 1),
            summary: stats::Summary::default(),
            hitches: 0,
            interval: Interval::default(),
            shown: Interval::default(),
            next_refresh_s: 0.0,
            scratch: Vec::new(),
            // `SKATE_FPS_LOG` kept from the old counter: SKATE_FPS_SAMPLE lines.
            legacy: std::env::var_os("SKATE_FPS_LOG").map(|_| (0, 0.0)),
        }
    }
}

/// What one frame did on the main thread.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FrameCpu {
    /// CPU time of the main world's schedules (First..Last).
    main_ms: f32,
    /// CPU time of the fixed (physics) loop inside them.
    fixed_ms: f32,
    /// Fixed steps that loop ran.
    steps: u32,
}

/// Sums over one readout refresh period; `mean` turns them into averages.
#[derive(Clone, Copy, Default)]
struct Interval {
    frames: u32,
    frame_ms: f64,
    main_ms: f64,
    fixed_ms: f64,
}

impl Interval {
    fn mean(self) -> (f32, f32, f32) {
        if self.frames == 0 {
            return (0.0, 0.0, 0.0);
        }
        let n = f64::from(self.frames);
        ((self.frame_ms / n) as f32, (self.main_ms / n) as f32, (self.fixed_ms / n) as f32)
    }
}

impl FrameTiming {
    /// Records one frame that took `ms` (start to next start) and ended at
    /// `now_s` (seconds on this resource's clock), with what it did on the main
    /// thread. Returns the log row values: (hitch, median of the history).
    fn push(&mut self, now_s: f64, ms: f32, cpu: FrameCpu) -> (bool, Option<f32>) {
        let FrameCpu { main_ms, fixed_ms, steps } = cpu;
        let median = stats::history_median(self.history.iter().copied(), &mut self.scratch);
        let hitch = stats::is_hitch(ms, median);
        self.frame += 1;
        self.last_ms = ms;
        self.last_hitch = hitch;
        self.last_steps = steps;
        self.last_main_ms = main_ms;
        self.last_fixed_ms = fixed_ms;
        self.last_median_ms = median;
        self.last_now_s = now_s;
        if self.history.len() == stats::HITCH_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(ms);
        self.window.push_back(Sample { end_s: now_s, ms, hitch });
        while self.window.len() > MAX_FRAMES
            || self.window.front().is_some_and(|s| s.end_s < now_s - WINDOW_S)
        {
            self.window.pop_front();
        }
        self.interval.frames += 1;
        self.interval.frame_ms += f64::from(ms);
        self.interval.main_ms += f64::from(main_ms);
        self.interval.fixed_ms += f64::from(fixed_ms);
        (hitch, median)
    }

    /// Recomputes the window summary; true when it did (every `REFRESH_S`).
    fn refresh(&mut self, now_s: f64) -> bool {
        if now_s < self.next_refresh_s {
            return false;
        }
        self.next_refresh_s = now_s + REFRESH_S;
        self.summary = stats::summarise(self.window.iter().map(|s| s.ms), &mut self.scratch);
        self.hitches = self.window.iter().filter(|s| s.hitch).count();
        self.shown = std::mem::take(&mut self.interval);
        true
    }

    /// The frame `begin_frame` closed last (the `FRAME_HITCH` line's numbers).
    pub(crate) fn closed(&self) -> hitch::ClosedFrame {
        hitch::ClosedFrame {
            frame: self.frame,
            ms: self.last_ms,
            median_ms: self.last_median_ms,
            main_ms: self.last_main_ms,
            fixed_ms: self.last_fixed_ms,
            steps: self.last_steps,
            now_s: self.last_now_s,
        }
    }

    fn now_s(&self) -> f64 {
        self.origin.elapsed().as_secs_f64()
    }

    #[cfg(test)]
    pub(crate) fn summary(&self) -> stats::Summary {
        self.summary
    }

    /// The `frame` section of the mod snapshot (read-only for mods).
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let s = self.summary;
        serde_json::json!({
            "frame": self.frame,
            "ms": self.last_ms,
            "fixed_steps": self.last_steps,
            "main_ms": self.last_main_ms,
            "fixed_ms": self.last_fixed_ms,
            "hitch": self.last_hitch,
            "window_s": WINDOW_S,
            "frames": s.frames,
            "fps": s.fps(),
            "mean_ms": s.mean_ms,
            "median_ms": s.median_ms,
            "low_1_ms": s.low_1_ms,
            "low_01_ms": s.low_01_ms,
            "worst_ms": s.worst_ms,
            "hitches": self.hitches,
        })
    }

    /// The simplified readout: the essentials on two short lines.
    fn readout_simple(&self) -> String {
        let s = self.summary;
        let (frame_ms, _, _) = self.shown.mean();
        let fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
        format!("{fps:4.0} fps {frame_ms:5.1} ms
1% {:5.1}  max {:5.0}", s.low_1_ms, s.worst_ms)
    }

    fn readout(&self) -> String {
        let s = self.summary;
        let (frame_ms, main_ms, fixed_ms) = self.shown.mean();
        let fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
        format!(
            "FRAME {frame_ms:6.2} ms  {fps:5.0} fps\n1% low {:6.2} ms  0.1% low {:6.2} ms\nworst {:.0} s {:7.1} ms  hitches {}\nCPU main {main_ms:5.2} ms  physics {fixed_ms:5.2} ms",
            s.low_1_ms,
            s.low_01_ms,
            WINDOW_S,
            s.worst_ms,
            self.hitches,
        )
    }
}

#[derive(Resource)]
struct FrameLogSink(log::FrameLog);

#[derive(Component)]
struct FrameOverlay;
#[derive(Component)]
struct FrameText;
#[derive(Component)]
struct FrameBar(usize);
#[derive(Component)]
struct FrameGraph;

pub(crate) struct FrameTimingPlugin;

impl Plugin for FrameTimingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameTiming>();
        if let Some(log) = log::FrameLog::from_env() {
            app.insert_resource(FrameLogSink(log));
        }
        app.add_systems(Startup, spawn_overlay)
            .add_systems(First, begin_frame.before(bevy::time::TimeSystems))
            .add_systems(RunFixedMainLoop, begin_fixed.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop))
            .add_systems(RunFixedMainLoop, end_fixed.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop))
            .add_systems(bevy::app::FixedFirst, count_fixed_step)
            .add_systems(Last, (record, show).chain());
        hitch::register(app);
    }
}

/// Start of a frame (first system of `First`): closes the previous frame.
fn begin_frame(mut timing: ResMut<FrameTiming>, sink: Option<Res<FrameLogSink>>) {
    let now = Instant::now();
    let wall_unix_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let Some((started, started_wall)) = timing.frame_started.replace((now, wall_unix_s)) else {
        return;
    };
    let cpu = timing.pending.take().unwrap_or_default();
    let ms = (now.duration_since(started).as_secs_f64() * 1000.0) as f32;
    let now_s = now.duration_since(timing.origin).as_secs_f64();
    let (hitch, median_ms) = timing.push(now_s, ms, cpu);
    if let Some(sink) = sink {
        sink.0.send(log::Row {
            wall_unix_s: started_wall,
            frame: timing.frame,
            frame_ms: ms,
            fixed_steps: cpu.steps,
            fixed_ms: cpu.fixed_ms,
            main_ms: cpu.main_ms,
            hitch,
            median_ms,
        });
    }
    timing.refresh(now_s);
}

fn begin_fixed(mut timing: ResMut<FrameTiming>) {
    timing.fixed_started = Some(Instant::now());
}

fn end_fixed(mut timing: ResMut<FrameTiming>) {
    if let Some(started) = timing.fixed_started.take() {
        timing.fixed_pending += started.elapsed();
    }
}

fn count_fixed_step(mut timing: ResMut<FrameTiming>) {
    timing.steps_pending += 1;
}

/// End of a frame's schedules (`Last`): the CPU split, closed by the next
/// `begin_frame`.
fn record(time: Res<Time<Real>>, mut timing: ResMut<FrameTiming>) {
    let main_ms = timing.frame_started.map_or(0.0, |(t, _)| t.elapsed().as_secs_f32() * 1000.0);
    let fixed_ms = std::mem::take(&mut timing.fixed_pending).as_secs_f32() * 1000.0;
    let steps = std::mem::take(&mut timing.steps_pending);
    timing.pending = Some(FrameCpu { main_ms, fixed_ms, steps });
    let seconds = time.delta_secs_f64();
    if let Some((frames, total)) = timing.legacy.as_mut().filter(|_| seconds > 0.0) {
        *frames += 1;
        *total += seconds;
        if *total >= 0.5 {
            eprintln!("SKATE_FPS_SAMPLE frames={frames} seconds={total:.9} fps={:.3}", f64::from(*frames) / *total);
            timing.legacy = Some((0, 0.0));
        }
    }
}

fn show(
    timing: Res<FrameTiming>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    mut overlay: Query<&mut Node, (With<FrameOverlay>, Without<FrameBar>)>,
    mut text: Query<(&mut Text, &mut TextFont), With<FrameText>>,
    mut bars: Query<(&FrameBar, &mut Node, &mut BackgroundColor), (Without<FrameOverlay>, Without<FrameGraph>)>,
    mut graph_node: Query<&mut Node, (With<FrameGraph>, Without<FrameOverlay>, Without<FrameBar>)>,
    mut graph: Local<Vec<f32>>,
    mut shown_at: Local<Option<f64>>,
    mut layout_simple: Local<Option<bool>>,
) {
    let visible = menu.as_ref().is_some_and(|m| m.frame_stats_visible());
    let simple = menu.as_ref().is_some_and(|m| m.frame_stats_simple());
    for mut node in &mut overlay {
        let display = if visible { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    if !visible {
        *shown_at = None;
        return;
    }
    // Resize for the mode once per change (smaller font, a tiny graph of the latest bars).
    if *layout_simple != Some(simple) {
        *layout_simple = Some(simple);
        *shown_at = None;
        for (_, mut font) in &mut text {
            font.font_size = if simple { 11.0 } else { 14.0 };
        }
        for mut node in &mut graph_node {
            node.width = px(if simple { SIMPLE_BARS as f32 * 2.0 } else { GRAPH_BARS as f32 * 3.0 });
            node.height = px(if simple { 14 } else { 40 });
        }
        for (bar, mut node, _) in &mut bars {
            node.width = px(if simple { 1 } else { 2 });
            node.display = if simple && bar.0 >= SIMPLE_BARS { Display::None } else { Display::Flex };
        }
    }
    // Update the text with each summary refresh and the graph at 10 Hz; layout
    // changes every frame would cost more than the readout is worth.
    let now_s = timing.now_s();
    if shown_at.is_some_and(|t| now_s - t < 0.1) {
        return;
    }
    *shown_at = Some(now_s);
    for (mut label, _) in &mut text {
        label.0 = if simple { timing.readout_simple() } else { timing.readout() };
    }
    graph.resize(if simple { SIMPLE_BARS } else { GRAPH_BARS }, 0.0);
    stats::bucket_worst(timing.window.iter().map(|s| (s.end_s, s.ms)), now_s, GRAPH_BUCKET_S, graph.as_mut_slice());
    let median = timing.summary.median_ms;
    for (bar, mut node, mut color) in &mut bars {
        let Some(&ms) = graph.get(bar.0) else { continue };
        node.height = percent((ms / GRAPH_FULL_MS).clamp(0.0, 1.0) * 100.0);
        color.0 = if median > 0.0 && ms > stats::HITCH_FACTOR * median {
            Color::srgb(0.95, 0.25, 0.2)
        } else if ms > 1000.0 / 60.0 {
            Color::srgb(0.95, 0.75, 0.2)
        } else {
            Color::srgb(0.55, 0.85, 0.3)
        };
    }
}

fn spawn_overlay(mut commands: Commands) {
    // Uses the existing UI camera; no separate render loop. Hidden until the
    // graphics menu's "Frame-time counter" row turns it on.
    commands
        .spawn((
            Name::new("Frame-time overlay"),
            FrameOverlay,
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: px(12),
                right: px(12),
                padding: UiRect::axes(px(10), px(6)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            GlobalZIndex(100),
        ))
        .with_children(|parent| {
            parent.spawn((
                FrameText,
                Text::new("FRAME --"),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::WHITE),
            ));
            parent
                .spawn((FrameGraph, Node {
                    width: px(GRAPH_BARS as f32 * 3.0),
                    height: px(40),
                    align_items: AlignItems::FlexEnd,
                    column_gap: px(1),
                    ..default()
                }))
                .with_children(|graph| {
                    for i in 0..GRAPH_BARS {
                        graph.spawn((
                            FrameBar(i),
                            Node { width: px(2), height: percent(0), ..default() },
                            BackgroundColor(Color::srgb(0.55, 0.85, 0.3)),
                        ));
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_drops_old_frames_and_counts_hitches() {
        let mut timing = FrameTiming { legacy: None, ..default() };
        let mut now = 0.0;
        for i in 0..2000 {
            let ms = if i == 1500 { 200.0 } else { 5.0 };
            now += f64::from(ms) / 1000.0;
            timing.push(now, ms, FrameCpu { main_ms: 1.0, fixed_ms: 0.5, steps: 1 });
        }
        assert!(timing.refresh(now));
        assert!(!timing.refresh(now + 0.1), "refreshes at 4 Hz");
        let s = timing.summary();
        // 5 s window: frames 1001..=1999 (~4.995 s) plus the 200 ms hitch.
        assert!(timing.window.front().unwrap().end_s >= now - WINDOW_S);
        assert_eq!(s.worst_ms, 200.0);
        assert_eq!(timing.hitches, 1);
        assert_eq!(s.median_ms, 5.0);
        assert!(timing.readout().contains("CPU main  1.00 ms  physics  0.50 ms"), "{}", timing.readout());
        let snap = timing.snapshot();
        assert_eq!(snap["frame"], 2000);
        assert_eq!(snap["worst_ms"], 200.0);
        assert_eq!(snap["hitches"], 1);
        assert!(timing.readout().contains("worst 5 s   200.0 ms  hitches 1"), "{}", timing.readout());
    }

    #[test]
    fn fixed_steps_are_attributed_to_the_frame_that_ran_them() {
        let mut timing = FrameTiming { legacy: None, ..default() };
        timing.push(0.05, 50.0, FrameCpu { main_ms: 2.0, fixed_ms: 1.0, steps: 3 });
        assert_eq!(timing.last_steps, 3);
        timing.push(0.055, 5.0, FrameCpu::default());
        assert_eq!(timing.last_steps, 0);
    }

    #[test]
    fn window_is_bounded_by_count() {
        let mut timing = FrameTiming { legacy: None, ..default() };
        for i in 0..(MAX_FRAMES + 10) {
            timing.push(i as f64 * 1e-6, 0.001, FrameCpu::default());
        }
        assert_eq!(timing.window.len(), MAX_FRAMES);
        assert_eq!(timing.history.len(), stats::HITCH_HISTORY);
    }

    #[derive(Resource, Default, Debug, Clone, PartialEq)]
    struct Sim {
        steps: u64,
        state: f64,
        deltas: Vec<u128>,
    }

    fn sim(time: Res<Time>, mut sim: ResMut<Sim>) {
        sim.steps += 1;
        sim.state = sim.state * 0.999 + time.delta_secs_f64() * (sim.steps % 7) as f64;
        sim.deltas.push(time.delta().as_nanos());
    }

    /// Runs a fixed-update "game" through 600 uneven frames (hitches included),
    /// optionally with the plugin and the frame log.
    fn run_app(plugin: bool, log: Option<std::path::PathBuf>) -> (Sim, Option<(u64, u64)>) {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Sim>().add_systems(FixedUpdate, sim);
        if plugin {
            app.add_plugins(FrameTimingPlugin);
            if let Some(path) = log {
                app.insert_resource(FrameLogSink(log::FrameLog::open(path).unwrap()));
            }
        }
        let mut steps_seen = 0;
        for i in 0..600u64 {
            let ms = if i % 97 == 0 { 120 } else { 3 + i % 5 * 4 };
            app.insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(ms)));
            app.update();
            if let Some(timing) = app.world().get_resource::<FrameTiming>() {
                steps_seen += u64::from(timing.last_steps);
            }
        }
        // The last frame's split is still waiting for the next frame.
        let frames = app.world().get_resource::<FrameTiming>()
            .map(|t| (t.frame, steps_seen + u64::from(t.pending.map_or(0, |c| c.steps))));
        let sim = app.world().resource::<Sim>().clone();
        app.world_mut().remove_resource::<FrameLogSink>();
        (sim, frames)
    }

    /// Readout and log on or off: the simulated game sees the same fixed steps
    /// and deltas, bit for bit; every fixed step is attributed to a frame.
    #[test]
    fn game_is_identical_with_diagnostics_on_or_off() {
        let (plain, none) = run_app(false, None);
        assert!(none.is_none());
        assert!(plain.steps > 100, "{}", plain.steps);
        let (with_plugin, stats) = run_app(true, None);
        let path = std::env::temp_dir().join(format!("skate-frame-log-app-{}.tsv", std::process::id()));
        let (with_log, logged) = run_app(true, Some(path.clone()));
        assert_eq!(plain, with_plugin);
        assert_eq!(plain, with_log);
        assert_eq!(plain.state.to_bits(), with_log.state.to_bits());
        let (frames, steps) = stats.unwrap();
        assert_eq!(stats, logged);
        assert!(frames >= 599, "{frames}");
        assert_eq!(steps, plain.steps);
        let text = std::fs::read_to_string(&path).unwrap();
        let rows: Vec<log::Row> = text.lines().skip(2).map(|l| log::parse_row(l).expect(l)).collect();
        assert_eq!(rows.len() as u64, frames);
        let last_steps = steps - rows.iter().map(|r| u64::from(r.fixed_steps)).sum::<u64>();
        assert!(last_steps <= 8, "only the final frame's steps are unlogged: {last_steps}");
        assert!(rows.iter().all(|r| r.main_ms <= r.frame_ms + 0.01), "a frame contains its schedules");
        let _ = std::fs::remove_file(path);
    }

    /// The same game with every trace-all diagnostic that runs per frame on the game thread: the
    /// frame timing plugin, the frame log and the rolling performance report (short windows, so
    /// several are handed to the writer thread during the run).
    fn run_trace_all(dir: &std::path::Path, frames: u64) -> (Sim, std::time::Duration) {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Sim>().add_systems(FixedUpdate, sim);
        app.add_plugins(FrameTimingPlugin);
        app.insert_resource(FrameLogSink(log::FrameLog::open(dir.join("frames.tsv")).unwrap()));
        app.init_resource::<bevy::diagnostic::DiagnosticsStore>();
        crate::performance::install(
            &mut app,
            crate::performance::Performance::rolling(dir.join("perf.json"), 0.02).with_warmup(0.0),
        );
        let started = std::time::Instant::now();
        for i in 0..frames {
            let ms = if i % 97 == 0 { 120 } else { 3 + i % 5 * 4 };
            app.insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(ms)));
            app.update();
        }
        let elapsed = started.elapsed();
        let sim = app.world().resource::<Sim>().clone();
        app.world_mut().remove_resource::<FrameLogSink>();
        (sim, elapsed)
    }

    /// Trace-all on or off: the simulated game sees the same fixed steps and deltas, bit for bit,
    /// and the rolling report keeps writing windows instead of exiting the app.
    #[test]
    fn game_is_identical_with_trace_all_on_or_off() {
        let (plain, _) = run_app(false, None);
        let dir = std::env::temp_dir().join(format!("skate-trace-all-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (traced, _) = run_trace_all(&dir, 600);
        assert_eq!(plain, traced);
        assert_eq!(plain.state.to_bits(), traced.state.to_bits());
        // The writer thread rewrites the report after each window; give it a moment.
        let report = dir.join("perf.json");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !report.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
        assert_eq!(json["mode"], "rolling");
        assert!(!json["windows"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Per-frame game-thread cost of the trace-all diagnostics (run with `--ignored --nocapture`):
    /// the same 3000-frame app as normal play has it (the frame timing plugin is always on) and with
    /// the trace-all additions (frame log, rolling perf report), interleaved, best of 5 each.
    #[test]
    #[ignore = "timing measurement, run by hand"]
    fn trace_all_overhead() {
        use bevy::time::TimeUpdateStrategy;
        let frames = 3000u64;
        let plain = || {
            let mut app = App::new();
            app.add_plugins(MinimalPlugins).init_resource::<Sim>().add_systems(FixedUpdate, sim);
            app.add_plugins(FrameTimingPlugin);
            let started = std::time::Instant::now();
            for i in 0..frames {
                let ms = if i % 97 == 0 { 120 } else { 3 + i % 5 * 4 };
                app.insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(ms)));
                app.update();
            }
            started.elapsed()
        };
        let dir = std::env::temp_dir().join(format!("skate-trace-all-cost-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut off, mut on) = (Vec::new(), Vec::new());
        for _ in 0..7 {
            off.push(plain());
            on.push(run_trace_all(&dir, frames).1);
        }
        let per_frame = |v: &[std::time::Duration]| v.iter().min().unwrap().as_secs_f64() * 1e6 / frames as f64;
        let spread = |v: &[std::time::Duration]| {
            let us: Vec<f64> = v.iter().map(|d| d.as_secs_f64() * 1e6 / frames as f64).collect();
            us.iter().cloned().fold(f64::MIN, f64::max) - us.iter().cloned().fold(f64::MAX, f64::min)
        };
        eprintln!(
            "TRACE_ALL_OVERHEAD frames={frames} off_us_per_frame={:.2} (spread {:.2}) on_us_per_frame={:.2} (spread {:.2}) delta_us={:.2}",
            per_frame(&off), spread(&off), per_frame(&on), spread(&on), per_frame(&on) - per_frame(&off)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Diagnostics never change the game: no system here writes any resource
    /// but `FrameTiming`, and only `show` writes components (UI nodes, text and
    /// colours, filtered to this module's overlay markers).
    #[test]
    fn systems_touch_only_their_own_state() {
        use bevy::ecs::system::{IntoSystem, System};
        let mut world = World::new();
        world.init_resource::<FrameTiming>();
        world.init_resource::<Time<Real>>();
        let own = world.resource_id::<FrameTiming>().unwrap();
        let mut systems: Vec<(Box<dyn System<In = (), Out = ()>>, bool)> = vec![
            (Box::new(IntoSystem::into_system(count_fixed_step)), false),
            (Box::new(IntoSystem::into_system(begin_frame)), false),
            (Box::new(IntoSystem::into_system(begin_fixed)), false),
            (Box::new(IntoSystem::into_system(end_fixed)), false),
            (Box::new(IntoSystem::into_system(record)), false),
            (Box::new(IntoSystem::into_system(show)), true),
        ];
        for (system, ui) in &mut systems {
            let access = system.initialize(&mut world);
            let combined = access.combined_access();
            assert!(!combined.has_write_all(), "{:?}", system.name());
            for write in combined.resource_writes() {
                assert_eq!(write, own, "{:?} writes a foreign resource", system.name());
            }
            if !*ui {
                assert!(!combined.has_any_component_write(), "{:?} writes components", system.name());
            }
        }
    }
}
