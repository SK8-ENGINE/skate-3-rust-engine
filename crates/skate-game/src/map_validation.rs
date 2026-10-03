//! `--validate-maps`: one process validates many converted maps.
//!
//! Setup streams one request per stdin line — a `.skate` path or `TEST_WORLD` —
//! and reads one `SKATE_MAP_CHECK {json}` stdout line back per request; EOF ends
//! the process. Stock data shared by every map (graphs, animation source,
//! controls, camera) loads once, so a map costs its own load instead of a whole
//! game startup (~7–8 s per map with `--check-assets`).
//!
//! `errors` are what `--check-assets` rejects (map load/validation, physics,
//! skater). `warnings` are physics::startup_check findings (spawn support, deck
//! pose, neutral startup); setup reports them without rejecting the map.
use crate::{
    camera::CameraRuntime,
    config::Config,
    graph_runtime::StockGraphs,
    physics::{startup_check, GamePhysics, PlayerControls, SkaterRuntime},
    skater_animation::AnimationSource,
};
use skate_data::skate_map::SkateMap;
use std::{
    io::{BufRead, Write},
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::Arc,
    time::Instant,
};

pub(crate) const TEST_WORLD: &str = "TEST_WORLD";
pub(crate) const READY: &str = "SKATE_VALIDATOR_READY";
pub(crate) const RESULT: &str = "SKATE_MAP_CHECK";
/// Collision and render floors may differ slightly; a gap above this means
/// the board stands on collision with nothing visible there.
const VISIBLE_FLOOR_TOLERANCE: f32 = 1.0;

/// Heights of every render triangle covering the XZ point.
pub(crate) fn visible_heights(map: &SkateMap, x: f32, z: f32) -> Vec<f32> {
    let vertices = &map.geometry.vertices;
    let mut heights = Vec::new();
    for tri in map.geometry.indices.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) = (
            vertices.get(tri[0] as usize),
            vertices.get(tri[1] as usize),
            vertices.get(tri[2] as usize),
        ) else {
            continue;
        };
        let [a, b, c] = [a.position, b.position, c.position];
        if x < a[0].min(b[0]).min(c[0]) || x > a[0].max(b[0]).max(c[0])
            || z < a[2].min(b[2]).min(c[2]) || z > a[2].max(b[2]).max(c[2])
        {
            continue;
        }
        let (abx, abz, acx, acz) = (b[0] - a[0], b[2] - a[2], c[0] - a[0], c[2] - a[2]);
        let det = abx * acz - acx * abz;
        if det.abs() < 1e-12 {
            continue;
        }
        let (dx, dz) = (x - a[0], z - a[2]);
        let u = (dx * acz - acx * dz) / det;
        let w = (abx * dz - dx * abz) / det;
        if u >= -1e-5 && w >= -1e-5 && u + w <= 1.0 + 1e-5 {
            heights.push(a[1] + u * (b[1] - a[1]) + w * (c[1] - a[1]));
        }
    }
    heights
}

#[derive(serde::Serialize, Default, Debug)]
pub(crate) struct MapCheck {
    pub path: String,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub collision_triangles: Option<usize>,
    pub spawn: Option<[f32; 3]>,
    pub support_drop: Option<f32>,
    pub grounded_ticks: Option<usize>,
    /// Smallest height difference between a render surface at the spawn's XZ
    /// and the collision floor under the spawn.
    pub visible_floor_gap: Option<f32>,
    pub seconds: f64,
    /// Seconds per step, in order (map, physics, skater, controls_camera, startup).
    pub timings: Vec<(&'static str, f64)>,
}

struct Shared<'a> {
    config: &'a Config,
    graphs: &'a StockGraphs,
    source: Arc<AnimationSource>,
}

/// Runs the request loop. Errors only for stock inputs every map needs.
pub(crate) fn run(config: &Config, graphs: &StockGraphs) -> Result<(), String> {
    let start = Instant::now();
    let source = AnimationSource::load(&config.asset_root)?;
    // Fail fast on stock inputs, exactly as --check-assets does.
    PlayerControls::load(&config.asset_root)?;
    CameraRuntime::load(&config.asset_root)?;
    let shared = Shared { config, graphs, source };
    let mut out = std::io::stdout().lock();
    writeln!(out, "{READY} seconds={:.3}", start.elapsed().as_secs_f64()).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("stdin: {e}"))?;
        let request = line.trim();
        if request.is_empty() {
            continue;
        }
        let result = check(&shared, request);
        let json = serde_json::to_string(&result).map_err(|e| e.to_string())?;
        writeln!(out, "{RESULT} {json}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn check(shared: &Shared, request: &str) -> MapCheck {
    let start = Instant::now();
    let mut result = MapCheck { path: request.to_owned(), ..MapCheck::default() };
    // A defect in one map must become that map's result, not end the process.
    match catch_unwind(AssertUnwindSafe(|| check_map(shared, request, &mut result))) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => result.errors.push(error),
        Err(panic) => result.errors.push(format!(
            "panic: {}",
            panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic payload")
        )),
    }
    result.ok = result.errors.is_empty();
    result.seconds = start.elapsed().as_secs_f64();
    result
}

fn check_map(shared: &Shared, request: &str, result: &mut MapCheck) -> Result<(), String> {
    let root = &shared.config.asset_root;
    let difficulty = shared.config.difficulty;
    let mut step = Instant::now();
    let mut lap = |result: &mut MapCheck, name: &'static str| {
        result.timings.push((name, step.elapsed().as_secs_f64()));
        step = Instant::now();
    };
    let map = if request == TEST_WORLD {
        None
    } else {
        let map = SkateMap::load(Path::new(request))?;
        crate::skate_world::validate_runtime(&map)?;
        Some(map)
    };
    lap(result, "map");
    let mut physics = GamePhysics::load_with_difficulty(root, map.as_ref(), difficulty)?;
    result.collision_triangles = Some(startup_check::collision_triangles(&physics));
    lap(result, "physics");
    let mut skater = SkaterRuntime::load_for_world(
        root,
        shared.graphs,
        &physics,
        difficulty.profile_key(),
        Some(shared.source.clone()),
    )?;
    lap(result, "skater");
    let Some(map) = map else { return Ok(()) };
    result.spawn = Some(map.spawn);
    let mut controls = PlayerControls::load(root)?;
    let mut camera = CameraRuntime::load(root)?;
    lap(result, "controls_camera");
    match startup_check::check(
        &mut physics,
        &mut skater,
        &mut controls,
        &mut camera,
        shared.graphs,
        map.spawn,
        map.heading,
    ) {
        Ok(report) => {
            result.support_drop = report.support_drop;
            result.grounded_ticks = Some(report.grounded_ticks);
            result.warnings = report.warnings;
            if let Some(drop) = report.support_drop {
                // The floor the board lands on must be visible: collision-only
                // floors (e.g. Industrial's harbour bed) or volumes look like
                // an invisible map with working collision.
                let floor = map.spawn[1] - drop;
                let gap = visible_heights(&map, map.spawn[0], map.spawn[2])
                    .into_iter()
                    .map(|height| (height - floor).abs())
                    .reduce(f32::min);
                result.visible_floor_gap = gap;
                if gap.is_none_or(|gap| gap > VISIBLE_FLOOR_TOLERANCE) {
                    result.warnings.push(format!(
                        "no visible surface within {VISIBLE_FLOOR_TOLERANCE} m of the collision floor under the spawn (invisible collision)"
                    ));
                }
            }
        }
        // New checks report but never reject a map.
        Err(error) => result.warnings.push(format!("startup: {error}")),
    }
    lap(result, "startup");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> SkateMap {
        SkateMap::parse(include_bytes!("../../../maps/format-demo.skate")).unwrap()
    }

    #[test]
    fn visible_heights_find_render_triangles_under_a_point_and_nothing_outside() {
        let map = demo();
        let tri = &map.geometry.indices[..3];
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| map.geometry.vertices[i as usize].position);
        let centre = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
        let heights = visible_heights(&map, centre[0], centre[2]);
        assert!(heights.iter().any(|h| (h - centre[1]).abs() < 1e-3), "{heights:?} vs {centre:?}");
        assert!(visible_heights(&map, 1.0e7, -1.0e7).is_empty());
    }

    #[test]
    fn result_lines_are_single_line_json_with_the_documented_fields() {
        let result = MapCheck { path: "a\nb".into(), ok: true, timings: vec![("map", 0.5)], ..MapCheck::default() };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains('\n'));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in ["path", "ok", "errors", "warnings", "collision_triangles", "spawn", "support_drop",
            "grounded_ticks", "visible_floor_gap", "seconds", "timings"] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
    }
}
