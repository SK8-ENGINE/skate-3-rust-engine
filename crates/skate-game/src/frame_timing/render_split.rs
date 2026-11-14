//! Render-thread side of the `FRAME_HITCH` line: per render frame CPU time (prep vs graph / submit / present),
//! pipeline compiles and asset uploads, summed until the main thread takes them. Observation only; the markers sit
//! on the edges of Bevy's chained render sets, so they add no ordering.
//! Details: docs/hails-additions/25-perf-diagnostics.md#render-side-split
use bevy::prelude::*;
use bevy::render::mesh::RenderMesh;
use bevy::render::render_asset::ExtractedAssets;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub(crate) const NAMES: [&str; 11] = [
    "frames", "frame_ms", "max_frame_ms", "prep_ms", "graph_ms", "pipelines_new", "pipelines_compiling",
    "pipelines_done", "meshes", "mesh_kb", "image_kb",
];
const FRAMES: usize = 0;
const FRAME_US: usize = 1;
const MAX_FRAME_US: usize = 2;
const PREP_US: usize = 3;
const GRAPH_US: usize = 4;
const PIPE_NEW: usize = 5;
const PIPE_COMPILING: usize = 6;
const PIPE_DONE: usize = 7;
const MESHES: usize = 8;
const MESH_BYTES: usize = 9;
const IMAGE_BYTES: usize = 10;
static SUMS: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];

/// The render frame in progress (render world only).
#[derive(Resource, Default)]
struct RenderFrame {
    begin: Option<Instant>,
    graph_begin: Option<Instant>,
    prep_us: u64,
    graph_us: u64,
    meshes: u64,
    mesh_bytes: u64,
    image_bytes: u64,
    pipelines: usize,
    pipelines_ok: usize,
}

pub(crate) fn register(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render.init_resource::<RenderFrame>().add_systems(
        Render,
        (
            begin.before(RenderSystems::ExtractCommands),
            uploads.after(RenderSystems::ExtractCommands).before(RenderSystems::PrepareAssets),
            graph_begin.after(RenderSystems::Prepare).before(RenderSystems::Render),
            graph_end.after(RenderSystems::Render).before(RenderSystems::Cleanup),
            end.after(RenderSystems::Cleanup),
        ),
    );
}

fn begin(mut frame: ResMut<RenderFrame>) {
    frame.begin = Some(Instant::now());
}

fn uploads(mut frame: ResMut<RenderFrame>, meshes: Option<Res<ExtractedAssets<RenderMesh>>>, images: Option<Res<ExtractedAssets<GpuImage>>>) {
    if let Some(meshes) = meshes {
        frame.meshes = meshes.extracted.len() as u64;
        frame.mesh_bytes = meshes.extracted.iter().map(|(_, m)| m.get_vertex_buffer_size() as u64).sum();
    }
    if let Some(images) = images {
        frame.image_bytes = images.extracted.iter().map(|(_, i)| i.data.as_ref().map_or(0, Vec::len) as u64).sum();
    }
}

fn graph_begin(mut frame: ResMut<RenderFrame>) {
    let now = Instant::now();
    frame.prep_us = frame.begin.map_or(0, |t| now.duration_since(t).as_micros() as u64);
    frame.graph_begin = Some(now);
}

fn graph_end(mut frame: ResMut<RenderFrame>) {
    frame.graph_us = frame.graph_begin.take().map_or(0, |t| t.elapsed().as_micros() as u64);
}

/// Publishes the frame; pipeline states are counted only while some are waiting (a few hundred entries).
fn end(mut frame: ResMut<RenderFrame>, cache: Option<Res<PipelineCache>>) {
    let Some(started) = frame.begin.take() else {
        return;
    };
    let frame_us = started.elapsed().as_micros() as u64;
    let (mut new, mut compiling, mut done) = (0, 0, 0);
    if let Some(cache) = cache {
        let total = cache.pipelines().count();
        if cache.waiting_pipelines().next().is_some() || total != frame.pipelines {
            let mut ok = 0usize;
            for p in cache.pipelines() {
                match p.state {
                    CachedPipelineState::Ok(_) => ok += 1,
                    CachedPipelineState::Creating(_) => compiling += 1,
                    _ => {}
                }
            }
            new = total.saturating_sub(frame.pipelines);
            done = ok.saturating_sub(frame.pipelines_ok);
            frame.pipelines_ok = ok;
        }
        frame.pipelines = total;
    }
    let add = |i: usize, v: u64| SUMS[i].fetch_add(v, Ordering::Relaxed);
    add(FRAMES, 1);
    add(FRAME_US, frame_us);
    SUMS[MAX_FRAME_US].fetch_max(frame_us, Ordering::Relaxed);
    add(PREP_US, frame.prep_us);
    add(GRAPH_US, frame.graph_us);
    add(PIPE_NEW, new as u64);
    SUMS[PIPE_COMPILING].fetch_max(compiling as u64, Ordering::Relaxed);
    add(PIPE_DONE, done as u64);
    add(MESHES, std::mem::take(&mut frame.meshes));
    add(MESH_BYTES, std::mem::take(&mut frame.mesh_bytes));
    add(IMAGE_BYTES, std::mem::take(&mut frame.image_bytes));
}

/// Takes (and resets) the sums of the render frames that finished since the last call.
pub(crate) fn take() -> [u64; 11] {
    std::array::from_fn(|i| SUMS[i].swap(0, Ordering::Relaxed))
}

/// The `render=[...]` part of the line: times in ms, sizes in KB.
pub(crate) fn format(v: &[u64; 11]) -> String {
    NAMES.iter().zip(v).enumerate().map(|(i, (n, &x))| match i {
        FRAME_US | MAX_FRAME_US | PREP_US | GRAPH_US => format!("{n}:{:.1}", x as f64 / 1000.0),
        MESH_BYTES | IMAGE_BYTES => format!("{n}:{}", x / 1024),
        _ => format!("{n}:{x}"),
    }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_units() {
        let mut v = [0u64; 11];
        v[FRAMES] = 2;
        v[FRAME_US] = 701_800;
        v[GRAPH_US] = 690_000;
        v[PIPE_DONE] = 3;
        v[IMAGE_BYTES] = 4096;
        let s = format(&v);
        assert!(s.starts_with("frames:2 frame_ms:701.8 "), "{s}");
        assert!(s.contains("graph_ms:690.0") && s.contains("pipelines_done:3") && s.ends_with("image_kb:4"), "{s}");
    }
}
