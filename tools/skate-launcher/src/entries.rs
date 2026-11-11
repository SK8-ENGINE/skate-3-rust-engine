//! The play and tool entries and `entries` (9 tab columns per entry), as `launcher.ps1` lists them (doc 44a 1a, 1c).
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::{flag, setting, text, Launcher};

pub struct Entry {
    pub id: String,
    pub group: &'static str,
    pub name: String,
    pub hint: String,
    pub recomp: Option<Value>,
    pub rpcs3: Option<Value>,
}

const BUILTIN: &[(&str, &str, &str, &str)] = &[
    ("rust-play", "rust", "Play", "The version's PLAY.bat."),
    ("rust-traceall", "rust", "Play + trace all", "Every trace (frames, audio, performance, GPU, living world) into logs\\trace-all-<time>."),
    ("rust-statelog", "rust", "Play + audio state log", "Records the per-frame audio state (.local\\audio-state-logs)."),
    ("rust-trace", "rust", "Play + audio trace", "Logs every native sound start in logs\\game-*.log (SKATE_AUDIO_TRACE=1)."),
    ("rust-devmods", "rust", "Play with the dev test mods", "Turns on the version's dev test mod (mods_enable in versions.json)."),
    ("rust-perf", "rust", "Play + performance trace", "Bevy performance trace (PLAY.bat -trace)."),
    ("tool-refresh", "tools", "Refresh game files", "Checks the game files without playing; converts only what changed."),
    ("tool-build", "tools", "Update and build the selected version", "Fetches its branch and builds it now, so the next launch starts at once."),
    ("tool-self-update", "tools", "Update the launcher", "Brings the launcher to the newest hails-additions on the fork and rebuilds it (also at every start and before Update and build)."),
    ("tool-send", "tools", "Send the logs now", "Runs the after-session send (config.json after_session)."),
    ("tool-share", "tools", "Share the last session via GitHub", "Packs the new logs and opens a GitHub issue on the selected version's repository; you drop the zip in. Needs Share sessions turned on."),
    ("tool-recomp-update", "tools", "Recomp: get the newest build", "Copies the main PC's newest recomp build (play-recomp.ps1 -Update)."),
    ("tool-recomp-getsave", "tools", "Recomp: get the main PC save", "Copies the main PC's recomp save after backing up this PC's (play-recomp.ps1 -GetSave)."),
    ("tool-dlc", "tools", "Import DLC", "Copies your Skate 3 DLC packages (pick one or many files, or a folder) to the launcher and converts the new ones (Refresh game files)."),
    ("tool-setup", "tools", "First-time setup", "Checks git, Rust, the C++ build tools and Python, and installs what is missing."),
];

fn config_list(config: &Value, key: &str) -> Vec<Value> {
    match config.get(key) {
        Some(Value::Array(items)) => items.clone(),
        Some(v @ Value::Object(_)) => vec![v.clone()],
        _ => Vec::new(),
    }
}

/// The built-ins, then the recomp and RPCS3 entries of config.json (an id that exists keeps its place).
pub fn entries(l: &Launcher) -> Vec<Entry> {
    let mut list: Vec<Entry> = BUILTIN.iter().map(|(id, group, name, hint)| Entry {
        id: (*id).into(), group, name: (*name).into(), hint: (*hint).into(), recomp: None, rpcs3: None,
    }).collect();
    let mut put = |e: Entry| match list.iter_mut().find(|x| x.id == e.id) {
        Some(slot) => *slot = e,
        None => list.push(e),
    };
    for re in config_list(&l.config, "recomp_entries") {
        put(Entry { id: text(&re, "id"), group: "recomp", name: text(&re, "name"), hint: text(&re, "hint"), recomp: Some(re), rpcs3: None });
    }
    for pe in config_list(&l.config, "rpcs3_entries") {
        put(Entry { id: text(&pe, "id"), group: "retail", name: text(&pe, "name"), hint: text(&pe, "hint"), recomp: None, rpcs3: Some(pe) });
    }
    list
}

const SESSION_TRACES: &[(&str, &str)] = &[
    ("traceall", "SKATE_TRACE_ALL"), ("statelog", "SKATE_AUDIO_STATE_LOG"), ("audiotrace", "SKATE_AUDIO_TRACE"),
    ("frametimes", "SKATE_FRAME_LOG"), ("livingdbg", "SKATE_LIVING_WORLD_DEBUG"),
];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.file_name().is_some_and(|n| n.to_string_lossy().contains("tests")) {
            out.push(path);
        }
    }
}

/// `Version-Has`: which of the quoted switches the version's game source contains (None: not set up).
fn version_has(l: &Launcher, v: &Value, needles: &[&str]) -> Option<Vec<String>> {
    let src = l.version_dir(v).join("crates").join("skate-game").join("src");
    if !src.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    let texts: Vec<String> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    Some(needles.iter().filter(|n| { let q = format!("\"{n}\""); texts.iter().any(|t| t.contains(&q)) }).map(|n| (*n).to_owned()).collect())
}

fn version_traces(l: &Launcher, v: &Value) -> Vec<&'static str> {
    let needles: Vec<&str> = SESSION_TRACES.iter().map(|(_, n)| *n).collect();
    let Some(has) = version_has(l, v, &needles) else { return Vec::new() };
    SESSION_TRACES.iter().filter(|(_, n)| has.iter().any(|h| h == n)).map(|(k, _)| *k).collect()
}

fn resolve_local(l: &Launcher, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() { path } else { l.repo.join(p) }
}

fn recomp_script(l: &Launcher) -> Option<PathBuf> {
    if let Some(s) = setting(&l.config, "recomp_script").and_then(Value::as_str) {
        return Some(resolve_local(l, s));
    }
    let d = l.repo.join("tools").join("second-pc").join("play-recomp.ps1");
    (d.exists() && l.repo.join("send-sessions.txt").exists()).then_some(d)
}

/// `Entry-Blocked`: why the entry can't run for version `v` ("" = it can). `missing`: the setup check's result.
fn blocked(l: &Launcher, e: &Entry, v: &Value, missing: &str, send_blocked: &str, share: &str) -> String {
    if e.id == "tool-setup" {
        return String::new();
    }
    if (e.group == "rust" || e.id == "tool-refresh" || e.id == "tool-build") && !missing.is_empty() {
        return format!("Needs {missing} first: run the first-time setup (Tools).");
    }
    let name = text(v, "name");
    match e.id.as_str() {
        "rust-traceall" => if l.version_dir(v).exists() && version_traces(l, v).is_empty() {
            return format!("{name} has no session traces: use Play.");
        },
        "rust-statelog" => if version_has(l, v, &["SKATE_AUDIO_STATE_LOG"]).is_some_and(|h| h.is_empty()) {
            return format!("{name} has no audio state log.");
        },
        "rust-trace" => if version_has(l, v, &["SKATE_AUDIO_TRACE"]).is_some_and(|h| h.is_empty()) {
            return format!("{name} has no audio trace.");
        },
        "rust-devmods" => if !flag(v, "mods_enable") {
            return format!("{name} has no dev test mod (mods_enable in versions.json).");
        },
        "rust-perf" => if !cfg!(windows) {
            return "The performance trace needs Windows for now (PLAY.bat -trace).".into();
        },
        "tool-refresh" => if version_has(l, v, &["--update-game-files"]).is_some_and(|h| h.is_empty()) {
            return format!("{name} can't refresh from here (built before the launcher refresh): use the game's Extras > Refresh Game Files.");
        },
        "tool-send" => return send_blocked.to_owned(),
        "tool-share" => if !share.starts_with("on ") {
            return "Turn on Share sessions via GitHub first (play screen or Tools).".into();
        },
        id if id.starts_with("tool-recomp-") => if recomp_script(l).is_none() {
            return "Not set up: this PC has no recomp share (tools\\second-pc, see docs\\hails-additions\\second-pc-testing.md), or set recomp_script in config.json.".into();
        },
        _ => {}
    }
    if let Some(re) = &e.recomp {
        let bat = text(re, "bat");
        if !bat.is_empty() {
            if !resolve_local(l, &bat).exists() {
                return format!("Not set up: {bat} not found.");
            }
        } else if !l.config.get("recomp").map(|r| text(r, "exe")).is_some_and(|x| !x.is_empty() && Path::new(&x).exists()) {
            return "Not set up: set recomp.exe in config.json (see README.md).".into();
        }
    }
    if e.rpcs3.is_some() {
        let r = l.config.get("rpcs3");
        if !r.map(|r| text(r, "exe")).is_some_and(|x| !x.is_empty() && Path::new(&x).exists()) {
            return "Not set up: set rpcs3.exe in config.json (see README.md).".into();
        }
        if !r.map(|r| text(r, "game")).is_some_and(|x| !x.is_empty() && Path::new(&x).exists()) {
            return "Not set up: set rpcs3.game in config.json (see README.md).".into();
        }
    }
    String::new()
}

const GAME_LOG_HEAD: &str = "#Game log: logs\\game-<time>.log";
const GAME_LOG_ROW: &str = "game-<time>.log: The game's own log (errors in .stderr.log)";

/// The play detail panel: (info, records) per play entry.
fn details(id: &str) -> Option<(&'static str, Vec<&'static str>)> {
    Some(match id {
        "rust-play" => ("Starts the selected version with no extra traces. For playing and for checking how a change feels.", vec![GAME_LOG_HEAD, GAME_LOG_ROW]),
        "rust-traceall" => ("Records every trace the version has in one folder, which the after-session send picks up. The game plays exactly as with Play.", vec![]),
        "rust-statelog" => ("Records what the audio saw every frame (speed, wheels, grinds, landings, bails), for tuning sounds against retail.",
            vec!["#Files: .local\\audio-state-logs\\", "state_<time>.tsv: Audio state, one row per audio frame", GAME_LOG_HEAD, "AUDIO_TIMING: Audio cost each second"]),
        "rust-trace" => ("Writes every native sound start and stop and every gameplay sound event to the game log.",
            vec![GAME_LOG_HEAD, "AUDIO_NATIVE: Every native sound start and stop", "AUDIO_EVENT: Brake, push, grind and landing sound events"]),
        "rust-devmods" => ("Turns on the version's dev test mod (mods_enable in versions.json) to try a mod feature before it ships.", vec![GAME_LOG_HEAD, GAME_LOG_ROW]),
        "rust-perf" => ("Records a 10 s performance trace of every system and render pass, for finding stutter.",
            vec!["#Files: the version folder", "trace-downtown-lag.json: Chrome trace, open in Perfetto or chrome://tracing", GAME_LOG_HEAD, GAME_LOG_ROW]),
        _ => return None,
    })
}

/// `TraceAll-Records`.
fn traceall_records(l: &Launcher, v: &Value) -> Vec<&'static str> {
    let t = version_traces(l, v);
    if t.is_empty() {
        return vec!["#Not known yet: the version is not set up", "Its source decides: Set it up once and this lists its traces"];
    }
    let has = |k: &str| t.contains(&k);
    let all = has("traceall");
    let mut r = vec!["#Files: logs\\trace-all-<time>\\"];
    if all || has("frametimes") { r.push("frames.tsv: Frame times, one row per frame"); }
    if all || has("statelog") { r.push("audio_state.tsv: Audio state, one row per audio frame"); }
    if all { r.push("perf.json: Performance report with GPU and render timings"); }
    r.push(GAME_LOG_HEAD);
    if all || has("audiotrace") { r.push("AUDIO_NATIVE / EVENT: Every sound start, stop and sound event"); }
    if all { r.push("*_TIMING: Audio, system and fixed-step costs each second"); }
    if all || has("livingdbg") { r.push("LIVING_WORLD: Population, NPC skaters and traffic every 5 s"); }
    if all { r.push("SKATE_FPS_SAMPLE: Frame rate"); }
    r
}

/// `Entry-Blocked` for one entry and version ("" = it can run).
pub fn blocked_for(l: &Launcher, e: &Entry, v: &Value) -> String {
    let missing = crate::status::setup_missing(l, l.use_release(v));
    blocked(l, e, v, &missing, &crate::status::send_blocked(l), &crate::status::share(l, &l.version_dir(v)))
}

/// `Version-Traces`: the session traces the version's source has.
pub fn traces(l: &Launcher, v: &Value) -> Vec<&'static str> {
    version_traces(l, v)
}

/// `entries`: id, name, hint, ask, group, on, why off, info, records (`|`-joined), tab-separated.
pub fn list(l: &Launcher, version_id: &str) -> Vec<String> {
    let v = if version_id.is_empty() { l.selected() } else { l.versions.iter().find(|x| text(x, "id") == version_id).unwrap_or(&l.versions[0]) };
    let missing = crate::status::setup_missing(l, l.use_release(v));
    let send_blocked = crate::status::send_blocked(l);
    let share = crate::status::share(l, &l.version_dir(v));
    entries(l).iter().map(|e| {
        let ask = e.recomp.as_ref().is_some_and(|r| flag(r, "ask_label"));
        let why = blocked(l, e, v, &missing, &send_blocked, &share);
        let (info, records) = details(&e.id).unwrap_or(("", Vec::new()));
        let records = if e.id == "rust-traceall" { traceall_records(l, v) } else { records };
        let records: Vec<String> = records.iter().map(|r| r.replace(['\t', '|'], " ")).collect();
        format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", e.id, e.name, e.hint.replace('\t', " "), u8::from(ask), e.group,
            u8::from(why.is_empty()), why, info, records.join("|"))
    }).collect()
}
