//! `SKATE_TRACE_ALL=1`: one play session records every session trace and diagnostic the engine
//! has, whatever the launcher remembered to set.
//!
//! [`apply`] runs first thing in the game process (before any thread, before the log subscriber)
//! and switches on every diagnostic below by setting its own environment switch, so every existing
//! reader (including the ones cached in `OnceLock`s deep in audio and physics) sees it unchanged.
//! Output files the launcher did not name go next to the ones it did (or `<exe dir>/logs/trace-all-
//! <unix seconds>`). Diagnostics only: nothing here changes the game (see
//! `frame_timing::tests::game_is_identical_with_trace_all_on_or_off`).
//!
//! What trace-all does differently from the single switches, so a whole session stays cheap:
//! the performance report rolls for the whole session instead of exiting after 25 s
//! (`performance.rs`), GPU timestamp queries are used only when the adapter has them (`app.rs`)
//! and only on one frame in `SKATE_PERF_GPU_EVERY` (30 by default, `performance.rs`),
//! the render phase split is sampled in windows (`performance.rs`), every log line goes through a
//! bounded writer thread instead of a blocking stderr write (`profiling.rs`), and a few
//! per-session lines log more often (`HELD_PROP`).
use std::path::PathBuf;
use std::sync::OnceLock;

/// The environment switch.
pub(crate) const ENV: &str = "SKATE_TRACE_ALL";

/// What a switch needs to be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wants {
    /// This exact value (the reader compares against it).
    Value(&'static str),
    /// An output path; the file name used when the launcher did not set one.
    Path(&'static str),
}

/// One diagnostic switch trace-all turns on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Switch {
    pub name: &'static str,
    pub wants: Wants,
    /// Where its output goes, for the startup line.
    pub output: &'static str,
}

/// Every session diagnostic (test-only and tool-mode switches such as `SKATE_BUDGET_MAP`,
/// `SKATE_VERIFY_*` or the `SKATE_RETAIL_*_VECTORS` data roots are not session traces, and
/// gameplay switches such as `SKATE_AUDIO_MORE_AUDIBLE` or `SKATE_FIXED_EXPOSURE` change the game,
/// so neither belongs here). `SKATE_AEMS`, which the launcher still sets, has no reader any more
/// (the native audio is always on).
pub(crate) const SWITCHES: &[Switch] = &[
    Switch { name: "SKATE_FRAME_LOG", wants: Wants::Path("frames.tsv"), output: "file (one row per frame, writer thread)" },
    Switch { name: "SKATE_AUDIO_STATE_LOG", wants: Wants::Path("audio_state.tsv"), output: "file (one row per audio frame, writer thread)" },
    Switch { name: "SKATE_PERF_REPORT", wants: Wants::Path("perf.json"), output: "file (rolling report, rewritten every window, writer thread)" },
    Switch { name: "SKATE_PERF_GPU", wants: Wants::Value("1"), output: "perf report render_diagnostics + SKATE_PERF_GPU_MS lines" },
    Switch { name: "SKATE_PERF_RENDER", wants: Wants::Value("1"), output: "perf report render phases + SKATE_PERF_RENDER lines (sampled windows)" },
    Switch { name: "SKATE_GPU_TIMING", wants: Wants::Value("1"), output: "GPU pass times in the perf report (only when the adapter supports timestamps)" },
    Switch { name: "SKATE_AUDIO_TRACE", wants: Wants::Value("1"), output: "log: AUDIO_NATIVE post/release, AUDIO_EVENT brake/push/grind, audio frontend trace" },
    Switch { name: "SKATE_AUDIO_TIMING", wants: Wants::Value("1"), output: "log: audio cost line once per second" },
    Switch { name: "SKATE_LIVING_WORLD_DEBUG", wants: Wants::Value("1"), output: "log: population, NPC skater and traffic readout every 5 s" },
    Switch { name: "SKATE_FPS_LOG", wants: Wants::Value("1"), output: "log: SKATE_FPS_SAMPLE lines" },
];

/// Whether trace-all is on (`SKATE_TRACE_ALL=1`), read once.
pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os(ENV).is_some_and(|v| v == "1"))
}

/// Whether `value` (the switch's current value) already turns `wants` on.
fn satisfied(wants: Wants, value: Option<&std::ffi::OsStr>) -> bool {
    match (wants, value) {
        (Wants::Value(on), Some(v)) => v == on,
        (Wants::Path(_), Some(v)) => !v.is_empty(),
        (_, None) => false,
    }
}

/// The folder for outputs the launcher did not name: next to the first one it did name.
fn output_dir() -> PathBuf {
    SWITCHES
        .iter()
        .filter(|s| matches!(s.wants, Wants::Path(_)))
        .filter_map(|s| std::env::var_os(s.name).filter(|v| !v.is_empty()))
        .find_map(|p| PathBuf::from(p).parent().filter(|d| !d.as_os_str().is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| {
            let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_default();
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            exe_dir.join("logs").join(format!("trace-all-{stamp}"))
        })
}

/// The values trace-all sets: `(switch, value)` for every switch not already on.
pub(crate) fn plan(get: impl Fn(&str) -> Option<std::ffi::OsString>, dir: impl Fn() -> PathBuf) -> Vec<(&'static str, std::ffi::OsString)> {
    let mut dir_cache: Option<PathBuf> = None;
    let mut out = Vec::new();
    for switch in SWITCHES {
        if satisfied(switch.wants, get(switch.name).as_deref()) {
            continue;
        }
        let value = match switch.wants {
            Wants::Value(v) => v.into(),
            Wants::Path(file) => dir_cache.get_or_insert_with(&dir).join(file).into_os_string(),
        };
        out.push((switch.name, value));
    }
    out
}

/// Turn every diagnostic on when trace-all is requested. Call before any other thread starts
/// (first thing in `main` after the crash supervisor) and before the log subscriber.
pub(crate) fn apply() {
    if !on() {
        return;
    }
    let dir = output_dir();
    for (name, value) in plan(|n| std::env::var_os(n), || dir.clone()) {
        // SAFETY: called from `main` before the game starts any thread (the crash supervisor child
        // runs this before the log subscriber, Bevy, audio or any worker exists).
        unsafe { std::env::set_var(name, value) };
    }
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("SKATE_TRACE_ALL could not create {}: {error}", dir.display());
    }
}

/// The one startup line: every active trace and where it writes. Logged after the subscriber is
/// installed (whether trace-all is on or not, so a session log always says what it recorded).
pub(crate) fn announce() {
    let active: Vec<String> = SWITCHES
        .iter()
        .filter_map(|s| {
            let value = std::env::var_os(s.name)?;
            if !satisfied(s.wants, Some(&value)) {
                return None;
            }
            Some(match s.wants {
                Wants::Path(_) => format!("{}={}", s.name, PathBuf::from(value).display()),
                Wants::Value(_) => format!("{} -> {}", s.name, s.output),
            })
        })
        .collect();
    if !on() && active.is_empty() {
        return;
    }
    bevy::log::info!(
        "TRACE_ACTIVE trace_all={} log_writer={} chrome_trace={} traces=[{}] always_on=[FRAME_HITCH, HELD_PROP, PROP_BELOW_GROUND, PROP_HELD, AUDIO_LANDING, AUDIO_EVENT body impact, MANUAL_LANDING, NPC_SKATER_BACKWARDS, PED_*, RETAIL_MATERIAL_FAMILIES, WORLD_SHADOW_FLOOR, BOARD_POSSESSION, GPU_TIMING]",
        on(),
        if crate::profiling::log_writer_threaded() { "thread (bounded, non-blocking)" } else { "stderr (direct)" },
        crate::profiling::chrome_trace_state(),
        active.join("; ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    #[test]
    fn trace_all_turns_on_every_switch_and_keeps_named_paths() {
        let mut env: HashMap<&str, OsString> = HashMap::new();
        env.insert("SKATE_FRAME_LOG", r"C:\logs\s1\frames.tsv".into());
        env.insert("SKATE_AUDIO_TIMING", "0".into());
        let plan = plan(|n| env.get(n).cloned(), || PathBuf::from(r"C:\logs\s1"));
        let set: HashMap<_, _> = plan.into_iter().collect();
        // The named path is kept; every other switch is turned on.
        assert!(!set.contains_key("SKATE_FRAME_LOG"));
        assert_eq!(set["SKATE_AUDIO_TIMING"], OsString::from("1"));
        assert_eq!(set["SKATE_AUDIO_STATE_LOG"], PathBuf::from(r"C:\logs\s1").join("audio_state.tsv").into_os_string());
        for switch in SWITCHES.iter().filter(|s| s.name != "SKATE_FRAME_LOG") {
            assert!(set.contains_key(switch.name), "{} left off", switch.name);
        }
    }

    #[test]
    fn already_on_switches_are_left_alone() {
        let env: HashMap<&str, OsString> = SWITCHES
            .iter()
            .map(|s| (s.name, match s.wants { Wants::Value(v) => v.into(), Wants::Path(f) => f.into() }))
            .collect();
        assert!(plan(|n| env.get(n).cloned(), || unreachable!()).is_empty());
    }
}
