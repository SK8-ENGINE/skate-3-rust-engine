//! Opt-in repeatable frame/physics timing: SKATE_PERF_REPORT=path.json.
use bevy::prelude::*;
use std::{path::PathBuf, time::Instant};
use std::sync::{Arc, Mutex};
use bevy::render::{Render, RenderApp, RenderSystems};

#[derive(Resource)]
pub(crate) struct Performance {
    path: PathBuf,
    start: Option<Instant>,
    frame_start: Instant,
    previous: Instant,
    physics_ms: f64,
    ticks: u32,
    samples: Vec<[f64; 4]>,
    render: Arc<Mutex<Vec<[f64; 2]>>>,
}
impl Performance {
    pub(crate) fn physics(&mut self, elapsed: std::time::Duration) {
        self.physics_ms += elapsed.as_secs_f64() * 1000.;
        self.ticks += 1;
    }
}

pub(crate) struct PerformancePlugin;
impl Plugin for PerformancePlugin {
    fn build(&self, app: &mut App) {
        let Some(path) = std::env::var_os("SKATE_PERF_REPORT") else { return; };
        let now = Instant::now();
        let render = Arc::new(Mutex::new(Vec::new()));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(RenderPerformance { start: None, frame: now, prepared: now, samples: render.clone() })
                .add_systems(Render, render_begin.before(RenderSystems::ExtractCommands))
                .add_systems(Render, render_prepared.after(RenderSystems::Prepare).before(RenderSystems::Render))
                .add_systems(Render, render_finish.after(RenderSystems::PostCleanup));
        }
        // Benchmark measurements must not depend on whether the window has focus.
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.insert_resource(Performance {
            path: path.into(), start: None, frame_start: now, previous: now,
            physics_ms: 0., ticks: 0, samples: Vec::new(), render,
        }).add_systems(First, begin).add_systems(Last, finish);
    }
}
fn begin(mut p: ResMut<Performance>) {
    p.frame_start = Instant::now();
    p.physics_ms = 0.;
    p.ticks = 0;
}
fn finish(mut p: ResMut<Performance>, mut exit: MessageWriter<AppExit>) {
    let now = Instant::now();
    let elapsed = now.duration_since(*p.start.get_or_insert(now)).as_secs_f64();
    let frame = now.duration_since(p.previous).as_secs_f64() * 1000.;
    p.previous = now;
    // Exclude initialization and shader warmup. Frame interval includes render
    // synchronization; CPU schedule time is First..Last of the main world.
    if elapsed > 10. {
        let sample = [frame, now.duration_since(p.frame_start).as_secs_f64() * 1000., p.physics_ms, f64::from(p.ticks)];
        p.samples.push(sample);
    }
    if elapsed > 25. && !p.samples.is_empty() {
        let mean = |column: usize| p.samples.iter().map(|s| s[column]).sum::<f64>() / p.samples.len() as f64;
        let mut frames: Vec<_> = p.samples.iter().map(|s| s[0]).collect();
        frames.sort_by(f64::total_cmp);
        let render = p.render.lock().unwrap();
        let render_mean = |column: usize| render.iter().map(|s| s[column]).sum::<f64>() / render.len().max(1) as f64;
        let report = serde_json::json!({
            "frames": frames.len(), "fps": 1000. / mean(0),
            "frame_ms_mean": mean(0), "frame_ms_median": frames[frames.len()/2],
            "frame_ms_p95": frames[(frames.len()-1)*95/100],
            "main_schedule_ms_mean": mean(1), "physics_ms_per_frame": mean(2),
            "physics_ticks_per_frame": mean(3),
            "physics_ms_per_tick": mean(2) / mean(3).max(f64::EPSILON),
            "render_prepare_ms_mean": render_mean(0), "render_submit_ms_mean": render_mean(1),
            "samples": p.samples,
        });
        match std::fs::write(&p.path, serde_json::to_vec_pretty(&report).unwrap()) {
            Ok(()) => { eprintln!("SKATE_PERF_REPORT {}", p.path.display()); exit.write(AppExit::Success); }
            Err(e) => { eprintln!("Performance report: {e}"); exit.write(AppExit::error()); }
        }
    }
}

#[derive(Resource)]
struct RenderPerformance {
    start: Option<Instant>, frame: Instant, prepared: Instant,
    samples: Arc<Mutex<Vec<[f64; 2]>>>,
}
fn render_begin(mut p: ResMut<RenderPerformance>) {
    p.frame = Instant::now();
}
fn render_prepared(mut p: ResMut<RenderPerformance>) {
    p.prepared = Instant::now();
}
fn render_finish(mut p: ResMut<RenderPerformance>) {
    let now = Instant::now();
    if now.duration_since(*p.start.get_or_insert(now)).as_secs_f64() > 10. {
        p.samples.lock().unwrap().push([
            p.prepared.duration_since(p.frame).as_secs_f64() * 1000.,
            now.duration_since(p.prepared).as_secs_f64() * 1000.,
        ]);
    }
}
