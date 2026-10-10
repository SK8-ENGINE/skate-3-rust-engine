//! Frame timing harness. Active only when `SKATE_PERF_REPORT` names an output
//! path: warms up, samples a fixed window, writes JSON, then exits.
//!
//! With trace-all (`SKATE_TRACE_ALL=1`) it never exits: it rolls for the whole
//! session in windows of [`ROLLING_WINDOW`] seconds and a writer thread rewrites
//! the JSON after every window (every window's summary, plus the newest window's
//! render diagnostics; per-frame samples are in the frame log). The game thread
//! only pushes one sample per frame into a preallocated buffer and hands the full
//! buffer over once per window.
//!
//! The `Performance` resource is absent in normal play, which is why callers
//! take it as `Option<ResMut<_>>` — instrumentation must never cost anything in
//! a shipping session.
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Seconds discarded before sampling, so shader compilation, asset streaming and
/// the map publish settle do not pollute the window.
const WARMUP: f32 = 10.0;
/// Seconds of samples retained.
const SAMPLE: f32 = 15.0;
/// Seconds per window of the rolling (trace-all) report.
pub(crate) const ROLLING_WINDOW: f32 = 15.0;
/// A frame slower than this is counted separately: at 300 FPS the budget is
/// 3.33 ms, so 8 ms is an unambiguous hitch rather than jitter.
const HITCH_MS: f32 = 8.0;

/// Draw statistics for one frame, owned by the main world.
///
/// Populated from the render world (see `render_stats`), which is a frame behind
/// by construction; for a 15-second average that skew is irrelevant.
#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct DrawStats {
    pub world_draws: u32,
    pub world_triangles: u32,
    pub mod_draws: u32,
    pub mod_triangles: u32,
    pub other_draws: u32,
}
impl DrawStats {
    pub(crate) fn total_draws(&self) -> u32 {
        self.world_draws + self.mod_draws + self.other_draws
    }
}

/// CPU cost of the render schedule, split at phase boundaries.
///
/// Bevy pipelines the render sub-app against the next frame's main schedule, so
/// when `frame_ms` exceeds `main_schedule_ms` the render app is the critical
/// path and this is the only way to see which phase owns it. The main world and
/// the render world hold clones of the same `Arc`, which is why the counters are
/// atomics rather than a plain resource: they are written in the render world
/// and read in the main world with no ordering guarantee between the two.
#[derive(Resource, Clone, Default)]
pub(crate) struct RenderPhases(Arc<Phases>);

#[derive(Default)]
struct Phases {
    started: AtomicU64,
    extract_ns: AtomicU64,
    assets_ns: AtomicU64,
    views_ns: AtomicU64,
    queue_ns: AtomicU64,
    prepare_ns: AtomicU64,
    total_ns: AtomicU64,
    frames: AtomicU64,
}

/// Wall-clock nanoseconds since an arbitrary fixed origin.
///
/// `Instant` cannot live in an atomic, and the render world needs to hand a
/// timestamp to systems that run later in the same schedule.
fn now_ns() -> u64 {
    use std::sync::LazyLock;
    static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);
    ORIGIN.elapsed().as_nanos() as u64
}

impl RenderPhases {
    /// Mean milliseconds per frame for each phase, in report order.
    fn means(&self) -> [f32; 6] {
        let frames = self.0.frames.load(Ordering::Relaxed).max(1) as f32;
        let mean = |counter: &AtomicU64| {
            counter.load(Ordering::Relaxed) as f32 / frames / 1.0e6
        };
        [
            mean(&self.0.extract_ns),
            mean(&self.0.assets_ns),
            mean(&self.0.views_ns),
            mean(&self.0.queue_ns),
            mean(&self.0.prepare_ns),
            mean(&self.0.total_ns),
        ]
    }

    fn reset(&self) {
        for counter in [
            &self.0.extract_ns,
            &self.0.assets_ns,
            &self.0.views_ns,
            &self.0.queue_ns,
            &self.0.prepare_ns,
            &self.0.total_ns,
            &self.0.frames,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

/// Accumulates the span from the recorded phase start to now, then re-arms the
/// start for the next phase.
fn mark(phases: &RenderPhases, counter: &AtomicU64) {
    let now = now_ns();
    let start = phases.0.started.swap(now, Ordering::Relaxed);
    counter.fetch_add(now.saturating_sub(start), Ordering::Relaxed);
}

#[derive(Default, Clone, Copy)]
struct Frame {
    total_ms: f32,
    main_ms: f32,
    physics_ms: f32,
    draws: u32,
    triangles: u32,
}

/// One finished window handed to the report writer thread.
struct Window {
    index: u64,
    started_s: f32,
    window_s: f32,
    samples: Vec<Frame>,
    draws: DrawStats,
    render_phase_ms: [f32; 6],
    gpu: Vec<(String, f64)>,
}

/// The rolling report (trace-all): window bookkeeping and the writer thread's queue.
struct Rolling {
    warmup: f32,
    window: f32,
    index: u64,
    window_started: f32,
    sender: std::sync::mpsc::SyncSender<Window>,
}

#[derive(Resource)]
pub(crate) struct Performance {
    output: PathBuf,
    rolling: Option<Rolling>,
    started: Instant,
    frame_started: Option<Instant>,
    main_elapsed: Duration,
    physics_elapsed: Duration,
    samples: Vec<Frame>,
    sampling: bool,
    finished: bool,
}

impl Performance {
    fn new(output: PathBuf) -> Self {
        Self {
            output,
            rolling: None,
            started: Instant::now(),
            frame_started: None,
            main_elapsed: Duration::ZERO,
            physics_elapsed: Duration::ZERO,
            // 15 s at an optimistic 600 FPS; growth beyond this is harmless.
            samples: Vec::with_capacity(9_000),
            sampling: false,
            finished: false,
        }
    }

    /// Physics tick cost, reported by `physics::advance`. FixedUpdate may run
    /// zero or several times per frame, so this accumulates until the frame ends.
    pub(crate) fn physics(&mut self, elapsed: Duration) {
        self.physics_elapsed += elapsed;
    }

    /// The rolling report: windows of `window` seconds after the warmup, summarised and written
    /// by the `perf-report` thread.
    pub(crate) fn rolling(output: PathBuf, window: f32) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Window>(4);
        let path = output.clone();
        let _ = std::thread::Builder::new().name("perf-report".into()).spawn(move || {
            let mut windows: Vec<serde_json::Value> = Vec::new();
            while let Ok(window) = receiver.recv() {
                let mut report = summarise(&window.samples, &window.draws);
                report.render_phase_ms = window.render_phase_ms;
                report.gpu = window.gpu;
                report.samples.clear();
                info!(
                    "SKATE_PERF window={} t={:.0}s fps={:.1} low1={:.1} frame_ms_mean={:.3} p99={:.3} max={:.1} main={:.3} physics={:.3} hitches={} draws={}",
                    window.index, window.started_s, report.fps, report.fps_1_percent_low, report.frame_ms_mean,
                    report.frame_ms_p99, report.frame_ms_max, report.main_schedule_ms_mean, report.physics_ms_mean,
                    report.frames_over_hitch, report.draw_calls
                );
                windows.push(window_summary(&report, window.index, window.started_s));
                if let Err(error) = write_rolling(&path, &report, &windows, window.window_s) {
                    error!("SKATE_PERF_REPORT could not be written: {error}");
                }
            }
        });
        let mut performance = Self::new(output);
        // One window at a generous 600 FPS; a full buffer is handed over once per window.
        performance.samples = Vec::with_capacity((window * 600.0) as usize);
        performance.rolling = Some(Rolling { warmup: WARMUP, window, index: 0, window_started: 0.0, sender });
        performance
    }

    /// The rolling report with another warmup (tests).
    #[cfg(test)]
    pub(crate) fn with_warmup(mut self, warmup: f32) -> Self {
        if let Some(rolling) = self.rolling.as_mut() {
            rolling.warmup = warmup;
        }
        self
    }
}

pub(crate) struct PerformancePlugin;
impl Plugin for PerformancePlugin {
    fn build(&self, app: &mut App) {
        let Some(output) = std::env::var_os("SKATE_PERF_REPORT").map(PathBuf::from) else {
            return;
        };
        let performance = if crate::trace_all::on() {
            info!("SKATE_PERF starting (rolling, trace-all): warmup={WARMUP}s window={ROLLING_WINDOW}s output={output:?}");
            Performance::rolling(output, ROLLING_WINDOW)
        } else {
            info!("SKATE_PERF starting: warmup={WARMUP}s sample={SAMPLE}s output={output:?}");
            Performance::new(output)
        };
        install(app, performance);
    }
}

/// The report's systems and, on request, the GPU diagnostics and the render phase split.
pub(crate) fn install(app: &mut App, performance: Performance) {
        let phases = RenderPhases::default();
        app.insert_resource(performance)
            .init_resource::<DrawStats>()
            .insert_resource(phases.clone())
            .add_systems(First, frame_begin)
            .add_systems(Last, frame_end);
        // GPU timestamp/statistics queries add work of their own. Ordinary CPU
        // frame comparisons must not enable them implicitly.
        if std::env::var("SKATE_PERF_GPU").as_deref() == Ok("1") {
            let every = gpu_sample_every();
            info!("SKATE_PERF_GPU sample_every={every} frames ({ENV_GPU_EVERY}; 1 = every frame, 0 = off)");
            if every > 0 {
                app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
                if every > 1 {
                    install_gpu_sampling(app, every, recorder_type(bevy::render::renderer::RenderContext::new));
                }
            }
        }

        // Phase-boundary systems serialize otherwise overlapping render work.
        // Only enable that detailed attribution when explicitly requested.
        if std::env::var("SKATE_PERF_RENDER").as_deref() != Ok("1") {
            return;
        }

        use bevy::render::{Render, RenderApp, RenderSystems};
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render.insert_resource(phases);
        render.add_systems(
            Render,
            (
                begin_render_frame.before(RenderSystems::ExtractCommands),
                after_extract
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::PrepareAssets),
                after_assets
                    .after(RenderSystems::PrepareMeshes)
                    .after(RenderSystems::PrepareAssets)
                    .before(RenderSystems::ManageViews),
                after_views
                    .after(RenderSystems::ManageViews)
                    .before(RenderSystems::Queue),
                after_queue
                    .after(RenderSystems::QueueSweep)
                    .before(RenderSystems::PhaseSort),
                after_prepare
                    .after(RenderSystems::PrepareBindGroups)
                    .before(RenderSystems::Render),
                end_render_frame.after(RenderSystems::Cleanup),
            ),
        );
}

/// How often the GPU queries run, in rendered frames (`SKATE_PERF_GPU_EVERY`).
pub(crate) const ENV_GPU_EVERY: &str = "SKATE_PERF_GPU_EVERY";
/// Trace-all default: one frame in 30 (about one per second at 30 fps, two at 60).
const TRACE_ALL_GPU_EVERY: u32 = 30;

/// The GPU query interval: `SKATE_PERF_GPU_EVERY` when set (1 = every frame, the
/// single report's behaviour; 0 = no GPU queries), otherwise 30 in trace-all and 1
/// for the one-shot `SKATE_PERF_REPORT` benchmark.
fn gpu_sample_every() -> u32 {
    parse_gpu_sample_every(std::env::var(ENV_GPU_EVERY).ok().as_deref(), crate::trace_all::on())
}

fn parse_gpu_sample_every(value: Option<&str>, trace_all: bool) -> u32 {
    let default = if trace_all { TRACE_ALL_GPU_EVERY } else { 1 };
    match value.map(str::trim) {
        Some(v) if !v.is_empty() => v.parse().unwrap_or_else(|_| {
            warn!("{ENV_GPU_EVERY}={v:?} is not a frame count, using {default}");
            default
        }),
        _ => default,
    }
}

/// Whether render frame `frame` (counting from 0) records GPU queries.
fn gpu_frame_sampled(frame: u64, every: u32) -> bool {
    every > 0 && frame % u64::from(every) == 0
}

/// Names Bevy's diagnostics recorder type: the render world resource
/// `RenderDiagnosticsPlugin` inserts is crate-private in bevy_render, but
/// `RenderContext::new` takes it, so its type is inferred from that signature.
fn recorder_type<R>(_new: fn(bevy::render::renderer::RenderDevice, Option<R>) -> bevy::render::renderer::RenderContext<'static>) -> std::marker::PhantomData<R> {
    std::marker::PhantomData
}

/// The recorder while it is parked (frames without GPU queries).
#[derive(Resource)]
struct GpuSampleGate<R: Resource> {
    every: u32,
    frame: u64,
    parked: Option<R>,
}

/// Bevy's render system records GPU queries only while the recorder resource is in the render
/// world (it removes it, runs the graph, and puts it back). On frames that are not sampled the
/// gate moves it aside, so those frames carry no timestamp or pipeline statistics queries.
/// Readbacks still in flight complete on the GPU and are collected on the next sampled frame.
fn install_gpu_sampling<R: Resource>(app: &mut App, every: u32, _recorder: std::marker::PhantomData<R>) {
    use bevy::render::{Render, RenderApp, RenderSystems};
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render.insert_resource(GpuSampleGate::<R> { every, frame: 0, parked: None });
    render.add_systems(
        Render,
        gate_gpu_queries::<R>
            .after(RenderSystems::PrepareBindGroups)
            .before(RenderSystems::Render),
    );
}

fn gate_gpu_queries<R: Resource>(world: &mut World) {
    let Some(mut gate) = world.get_resource_mut::<GpuSampleGate<R>>() else {
        return;
    };
    let sampled = gpu_frame_sampled(gate.frame, gate.every);
    gate.frame += 1;
    if sampled {
        if let Some(recorder) = gate.parked.take() {
            world.insert_resource(recorder);
        }
    } else if let Some(recorder) = world.remove_resource::<R>() {
        world.resource_mut::<GpuSampleGate<R>>().parked = Some(recorder);
    }
}

fn begin_render_frame(phases: Res<RenderPhases>) {
    phases.0.started.store(now_ns(), Ordering::Relaxed);
}

fn after_extract(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.extract_ns);
}

fn after_assets(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.assets_ns);
}

fn after_views(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.views_ns);
}

fn after_queue(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.queue_ns);
}

fn after_prepare(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.prepare_ns);
}

/// Closes the frame: the residual span is the render graph plus cleanup, and the
/// frame counter is what turns every accumulator into a per-frame mean.
fn end_render_frame(phases: Res<RenderPhases>) {
    mark(&phases, &phases.0.total_ns);
    phases.0.frames.fetch_add(1, Ordering::Relaxed);
}

fn frame_begin(mut performance: ResMut<Performance>) {
    performance.frame_started = Some(Instant::now());
    performance.main_elapsed = Duration::ZERO;
    performance.physics_elapsed = Duration::ZERO;
}

fn frame_end(
    mut performance: ResMut<Performance>,
    time: Res<Time<Real>>,
    draws: Res<DrawStats>,
    phases: Res<RenderPhases>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    if performance.finished {
        return;
    }
    let elapsed = performance.started.elapsed().as_secs_f32();
    if !performance.sampling {
        if elapsed < performance.rolling.as_ref().map_or(WARMUP, |r| r.warmup) {
            return;
        }
        performance.sampling = true;
        // Warmup includes shader compilation and asset streaming, whose cost
        // would otherwise dominate the phase accumulators.
        phases.reset();
        info!("SKATE_PERF warmup complete, sampling {SAMPLE}s");
    }

    // Wall-clock delta rather than the frame_begin instant: it includes the
    // presentation wait, which is what the player actually experiences.
    let total_ms = time.delta().as_secs_f32() * 1000.0;
    let main_ms = performance
        .frame_started
        .map_or(0.0, |start| start.elapsed().as_secs_f32() * 1000.0);
    let physics_ms = performance.physics_elapsed.as_secs_f32() * 1000.0;
    performance.samples.push(Frame {
        total_ms,
        main_ms,
        physics_ms,
        draws: draws.total_draws(),
        triangles: draws.world_triangles + draws.mod_triangles,
    });

    if let Some(rolling) = performance.rolling.as_mut() {
        if elapsed < rolling.warmup + rolling.window_started + rolling.window {
            return;
        }
        let window_s = rolling.window;
        let started_s = rolling.warmup + rolling.window_started;
        rolling.window_started += rolling.window;
        rolling.index += 1;
        let index = rolling.index;
        let render_phase_ms = phases.means();
        phases.reset();
        // Once per window, not per frame: the full buffer goes to the writer thread (which sorts
        // and formats) and a fresh one of the same size takes its place.
        let capacity = performance.samples.capacity();
        let samples = std::mem::replace(&mut performance.samples, Vec::with_capacity(capacity));
        let window = Window { index, started_s, window_s, samples, draws: *draws, render_phase_ms, gpu: gpu_diagnostics(&diagnostics) };
        if let Some(rolling) = performance.rolling.as_ref() {
            if rolling.sender.try_send(window).is_err() {
                warn!("SKATE_PERF window {index} dropped (report writer busy)");
            }
        }
        return;
    }

    if elapsed < WARMUP + SAMPLE {
        return;
    }
    performance.finished = true;
    let mut report = summarise(&performance.samples, &draws);
    report.render_phase_ms = phases.means();
    // Every render diagnostic, so GPU pass costs land in the report without
    // this module needing to know the pass names the render graph happens to use.
    report.gpu = gpu_diagnostics(&diagnostics);
    match write_report(&performance.output, &report) {
        Ok(()) => info!("SKATE_PERF_REPORT written to {:?}", performance.output),
        Err(error) => error!("SKATE_PERF_REPORT could not be written: {error}"),
    }
    let [extract, assets, views, queue, prepare, graph] = report.render_phase_ms;
    eprintln!(
        "SKATE_PERF fps={:.1} frame_ms_mean={:.3} p99={:.3} main={:.3} draws={} triangles={}",
        report.fps,
        report.frame_ms_mean,
        report.frame_ms_p99,
        report.main_schedule_ms_mean,
        report.draw_calls,
        report.triangles
    );
    eprintln!(
        "SKATE_PERF_RENDER extract={extract:.3} assets={assets:.3} views={views:.3} \
         queue={queue:.3} prepare={prepare:.3} graph={graph:.3}"
    );
    // Times first and in full: sorting the whole set by value buries
    // sub-millisecond durations under invocation counts in the millions.
    for (name, value) in report.gpu.iter().filter(|(n, _)| n.ends_with("elapsed_gpu")) {
        eprintln!("SKATE_PERF_GPU_MS {name} {value:.4}");
    }
    for (name, value) in report
        .gpu
        .iter()
        .filter(|(n, _)| n.ends_with("invocations"))
        .take(8)
    {
        eprintln!("SKATE_PERF_GPU {name} {value:.0}");
    }
    exit.write(AppExit::Success);
}

/// Every render diagnostic, highest first.
fn gpu_diagnostics(diagnostics: &bevy::diagnostic::DiagnosticsStore) -> Vec<(String, f64)> {
    let mut gpu: Vec<(String, f64)> = diagnostics
        .iter()
        .filter(|d| d.path().as_str().starts_with("render/"))
        .filter_map(|d| d.smoothed().map(|v| (d.path().to_string(), v)))
        .filter(|(_, v)| *v > 0.0)
        .collect();
    gpu.sort_by(|a, b| b.1.total_cmp(&a.1));
    gpu
}

/// One window's entry in the rolling report.
fn window_summary(report: &Report, index: u64, started_s: f32) -> serde_json::Value {
    serde_json::json!({
        "window": index,
        "start_s": started_s,
        "frames": report.frames,
        "fps": report.fps,
        "fps_1_percent_low": report.fps_1_percent_low,
        "frame_ms_mean": report.frame_ms_mean,
        "frame_ms_median": report.frame_ms_median,
        "frame_ms_p95": report.frame_ms_p95,
        "frame_ms_p99": report.frame_ms_p99,
        "frame_ms_max": report.frame_ms_max,
        "frames_over_8ms": report.frames_over_hitch,
        "main_schedule_ms_mean": report.main_schedule_ms_mean,
        "physics_ms_mean": report.physics_ms_mean,
        "draw_calls": report.draw_calls,
        "triangles": report.triangles,
        "render_phase_ms": report.render_phase_ms,
    })
}

/// The rolling report: every window so far, and the newest window's render diagnostics.
fn write_rolling(path: &std::path::Path, latest: &Report, windows: &[serde_json::Value], window_s: f32) -> std::io::Result<()> {
    let json = serde_json::json!({
        "mode": "rolling",
        "warmup_seconds": WARMUP,
        "window_seconds": window_s,
        "gpu_queries_enabled": std::env::var("SKATE_PERF_GPU").as_deref() == Ok("1"),
        "gpu_sample_every_frames": gpu_sample_every(),
        "render_phase_instrumentation": std::env::var("SKATE_PERF_RENDER").as_deref() == Ok("1"),
        "note":"per-frame samples are in the SKATE_FRAME_LOG file",
        "latest_render_diagnostics": latest.gpu.iter()
            .map(|(name, value)| serde_json::json!({ "name": name, "ms": value }))
            .collect::<Vec<_>>(),
        "windows": windows,
    });
    // Write then rename, so a reader (or a crash mid-write) never sees half a file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&json)?)?;
    std::fs::rename(&tmp, path)
}

struct Report {
    frames: usize,
    fps: f32,
    fps_1_percent_low: f32,
    slowest_1_percent_ms_mean: f32,
    frame_ms_mean: f32,
    frame_ms_median: f32,
    frame_ms_p95: f32,
    frame_ms_p99: f32,
    frame_ms_max: f32,
    frames_over_hitch: usize,
    main_schedule_ms_mean: f32,
    physics_ms_mean: f32,
    draw_calls: u32,
    triangles: u32,
    /// extract, assets, views, queue, prepare, graph — see `RenderPhases`.
    render_phase_ms: [f32; 6],
    /// Render diagnostics, highest first. Includes GPU pass timings when the
    /// adapter supports timestamp queries.
    gpu: Vec<(String, f64)>,
    samples: Vec<Frame>,
}

fn summarise(samples: &[Frame], draws: &DrawStats) -> Report {
    let frames = samples.len();
    let mut sorted: Vec<f32> = samples.iter().map(|f| f.total_ms).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mean = |values: &[f32]| -> f32 {
        if values.is_empty() { 0.0 } else { values.iter().sum::<f32>() / values.len() as f32 }
    };
    // Nearest-rank percentile. Exact interpolation is not worth it here: the
    // sample count is in the thousands and the budget is stated in whole ms.
    let percentile = |q: f32| -> f32 {
        if sorted.is_empty() {
            return 0.0;
        }
        // Nearest-rank: the smallest sample at or below which `q` of the frames
        // fall, i.e. rank `ceil(q*N)` counting from one. Reporting "95% of frames
        // were at or under this" only means that under this definition.
        let rank = (q * sorted.len() as f32).ceil() as usize;
        sorted[rank.clamp(1, sorted.len()) - 1]
    };
    let frame_ms_mean = mean(&sorted);
    // A 1% low averages the slowest ceil(N / 100) frame times, then takes
    // their reciprocal. It is distinct from the reciprocal of the p99 cutoff.
    let slowest_1_percent_ms_mean = mean(&sorted[frames - frames.div_ceil(100)..]);
    Report {
        frames,
        fps: if frame_ms_mean > 0.0 { 1000.0 / frame_ms_mean } else { 0.0 },
        fps_1_percent_low: if slowest_1_percent_ms_mean > 0.0 {
            1000.0 / slowest_1_percent_ms_mean
        } else { 0.0 },
        slowest_1_percent_ms_mean,
        frame_ms_mean,
        frame_ms_median: percentile(0.50),
        frame_ms_p95: percentile(0.95),
        frame_ms_p99: percentile(0.99),
        frame_ms_max: sorted.last().copied().unwrap_or(0.0),
        frames_over_hitch: samples.iter().filter(|f| f.total_ms > HITCH_MS).count(),
        main_schedule_ms_mean: mean(&samples.iter().map(|f| f.main_ms).collect::<Vec<_>>()),
        physics_ms_mean: mean(&samples.iter().map(|f| f.physics_ms).collect::<Vec<_>>()),
        draw_calls: draws.total_draws(),
        triangles: draws.world_triangles + draws.mod_triangles,
        render_phase_ms: [0.0; 6],
        gpu: Vec::new(),
        samples: samples.to_vec(),
    }
}

fn write_report(path: &std::path::Path, report: &Report) -> std::io::Result<()> {
    let json = serde_json::json!({
        "frames": report.frames,
        "gpu_queries_enabled": std::env::var("SKATE_PERF_GPU").as_deref() == Ok("1"),
        "gpu_sample_every_frames": gpu_sample_every(),
        "render_phase_instrumentation": std::env::var("SKATE_PERF_RENDER").as_deref() == Ok("1"),
        "fps":report.fps,
        "fps_1_percent_low": report.fps_1_percent_low,
        "slowest_1_percent_ms_mean": report.slowest_1_percent_ms_mean,
        "frame_ms_mean": report.frame_ms_mean,
        "frame_ms_median": report.frame_ms_median,
        "frame_ms_p95": report.frame_ms_p95,
        "frame_ms_p99": report.frame_ms_p99,
        "frame_ms_max": report.frame_ms_max,
        "frames_over_8ms": report.frames_over_hitch,
        "main_schedule_ms_mean": report.main_schedule_ms_mean,
        "physics_ms_mean": report.physics_ms_mean,
        "draw_calls": report.draw_calls,
        "triangles": report.triangles,
        "render_extract_ms_mean": report.render_phase_ms[0],
        "render_assets_ms_mean": report.render_phase_ms[1],
        "render_views_ms_mean": report.render_phase_ms[2],
        "render_queue_ms_mean": report.render_phase_ms[3],
        "render_prepare_ms_mean": report.render_phase_ms[4],
        "render_graph_ms_mean": report.render_phase_ms[5],
        "render_diagnostics": report.gpu.iter()
            .map(|(name, value)| serde_json::json!({ "name": name, "ms": value }))
            .collect::<Vec<_>>(),
        "warmup_seconds": WARMUP,
        "sample_seconds": SAMPLE,
        "samples": report.samples.iter().map(|f| serde_json::json!({
            "frame_ms": f.total_ms,
            "main_ms": f.main_ms,
            "physics_ms": f.physics_ms,
            "draws": f.draws,
            "triangles": f.triangles,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(total_ms: f32) -> Frame {
        Frame { total_ms, ..default() }
    }

    #[test]
    fn gpu_sample_interval_defaults_and_overrides() {
        assert_eq!(parse_gpu_sample_every(None, true), 30);
        assert_eq!(parse_gpu_sample_every(None, false), 1);
        assert_eq!(parse_gpu_sample_every(Some("1"), true), 1);
        assert_eq!(parse_gpu_sample_every(Some(" 0 "), true), 0);
        assert_eq!(parse_gpu_sample_every(Some("120"), false), 120);
        assert_eq!(parse_gpu_sample_every(Some("x"), true), 30);
        assert_eq!(parse_gpu_sample_every(Some(""), false), 1);
        let picked: Vec<u64> = (0..65).filter(|&f| gpu_frame_sampled(f, 30)).collect();
        assert_eq!(picked, vec![0, 30, 60]);
        assert!((0..10).all(|f| gpu_frame_sampled(f, 1)));
        assert!(!(0..10).any(|f| gpu_frame_sampled(f, 0)));
    }

    /// The gate moves the recorder out on unsampled frames and back on sampled ones, and
    /// never loses or duplicates it.
    #[test]
    fn gpu_gate_parks_and_restores_the_recorder() {
        #[derive(Resource)]
        struct Recorder(u32);
        let mut world = World::new();
        world.insert_resource(Recorder(7));
        world.insert_resource(GpuSampleGate::<Recorder> { every: 3, frame: 0, parked: None });
        let mut present = Vec::new();
        for _ in 0..7 {
            gate_gpu_queries::<Recorder>(&mut world);
            let here = world.get_resource::<Recorder>().is_some();
            let parked = world.resource::<GpuSampleGate<Recorder>>().parked.is_some();
            assert!(here != parked, "exactly one copy of the recorder");
            present.push(here);
        }
        assert_eq!(present, vec![true, false, false, true, false, false, true]);
        assert_eq!(world.resource::<Recorder>().0, 7);
    }

    #[test]
    fn percentiles_follow_sample_order() {
        // Values 1..=100, so the Nth smallest sample is exactly N ms and every
        // rank is readable directly.
        let samples: Vec<Frame> = (1..=100).map(|ms| frame(ms as f32)).collect();
        let report = summarise(&samples, &DrawStats::default());
        assert_eq!(report.frames, 100);
        assert_eq!(report.frame_ms_median, 50.0);
        assert_eq!(report.frame_ms_p95, 95.0);
        assert_eq!(report.frame_ms_p99, 99.0);
        assert_eq!(report.frame_ms_max, 100.0);
        // A single sample is every percentile of itself.
        let one = summarise(&[frame(7.0)], &DrawStats::default());
        assert_eq!(
            (one.frame_ms_median, one.frame_ms_p95, one.frame_ms_p99),
            (7.0, 7.0, 7.0)
        );
    }

    #[test]
    fn fps_is_the_reciprocal_of_mean_frame_time() {
        // 300 FPS is the contract; 3.333 ms per frame must read back as ~300.
        let samples = vec![frame(10.0 / 3.0); 64];
        let report = summarise(&samples, &DrawStats::default());
        assert!((report.fps - 300.0).abs() < 1.0, "fps={}", report.fps);
    }

    #[test]
    fn hitches_are_counted_against_the_8ms_threshold() {
        let samples = vec![frame(3.0), frame(9.0), frame(8.0), frame(20.0)];
        let report = summarise(&samples, &DrawStats::default());
        assert_eq!(report.frames_over_hitch, 2);
    }

    #[test]
    fn empty_sample_set_does_not_panic() {
        let report = summarise(&[], &DrawStats::default());
        assert_eq!(report.frames, 0);
        assert_eq!(report.fps, 0.0);
        assert_eq!(report.fps_1_percent_low, 0.0);
    }

    #[test]
    fn one_percent_low_averages_the_tail_including_fractional_sample_counts() {
        let mut samples = vec![frame(2.0); 199];
        samples.extend([frame(10.0), frame(18.0)]);
        let report = summarise(&samples, &DrawStats::default());
        assert_eq!(report.frame_ms_p99, 2.0);
        assert_eq!(report.slowest_1_percent_ms_mean, 10.0);
        assert_eq!(report.fps_1_percent_low, 100.0);
        let one = summarise(&[frame(8.0)], &DrawStats::default());
        assert_eq!(one.fps_1_percent_low, 125.0);
    }
}
