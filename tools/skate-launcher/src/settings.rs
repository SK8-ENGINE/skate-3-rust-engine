//! The game settings the launcher shows and steps (`game-settings`, `game-setting <key>`), read from and written to
//! the installation's settings folder as `launcher.ps1` does (doc 44a 1a).
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::state::Launcher;

/// key, group, label, file, field, values (the first = the game's default), shown values, hint.
struct Row {
    key: &'static str,
    group: &'static str,
    label: &'static str,
    file: &'static str,
    field: &'static str,
    values: Vec<Value>,
    shown: Option<Vec<&'static str>>,
    hint: &'static str,
}

fn volumes(default: i64) -> Vec<Value> {
    std::iter::once(default).chain((0..=100).step_by(5).filter(move |v| *v != default)).map(Value::from).collect()
}

fn rows() -> Vec<Row> {
    let row = |key, group, label, file, field, values: Vec<Value>, shown: Option<Vec<&'static str>>, hint| Row { key, group, label, file, field, values, shown, hint };
    let flag = |default: bool| vec![json!(default), json!(!default)];
    vec![
        row("npc_draw_distance", "Opt-ins", "NPC Draw Distance", "graphics.json", "npc_draw_distance", vec![json!(1.0), json!(0.0), json!(1.5), json!(2.0), json!(3.0)],
            Some(vec!["Retail", "None (no NPCs, not retail)", "1.5x (not retail)", "2x (not retail)", "3x (not retail)"]),
            "How far out peds, NPC skaters and cars appear; farther costs frame time."),
        row("better_water", "Opt-ins", "Better Water", "water-look.json", "enabled", flag(false), Some(vec!["Retail", "On (not retail)"]), "A brighter, calmer water look on the canals, fountains and the sea."),
        row("more_audible", "Opt-ins", "More Audible World", "audio.json", "more_audible_world", flag(false), Some(vec!["Retail", "On (not retail)"]), "More cars, peds and NPC skaters heard at once. Applies at the next start."),
        row("ped_speech", "Opt-ins", "Ped Speech", "audio.json", "decode_speech", flag(true), Some(vec!["On (default)", "Off"]), "Peds, the main cast and the announcer talk; setup decodes the speech (about 3.4 GB) at the next Refresh Game Files."),
        row("extended_replays", "Opt-ins", "Extended Replays", "graphics.json", "extended_replays", flag(false), Some(vec!["Retail (1:30)", "On (not retail)"]), "The instant replay keeps more than retail's 90 seconds (about 85 MB a minute)."),
        row("replay_seconds", "Opt-ins", "Replay Length", "graphics.json", "replay_seconds", [120, 150, 180, 210, 240].map(Value::from).to_vec(), Some(vec!["2:00", "2:30", "3:00", "3:30", "4:00"]), "How much the replay keeps with Extended Replays on."),
        row("better_object_mover", "Opt-ins", "Better Object Mover", "graphics.json", "better_object_mover", flag(false), Some(vec!["Retail", "On (not retail)"]), "Every prop can be picked up and moved, not only those with grab handles."),
        row("debug_menus", "Opt-ins", "Debug Menus", "graphics.json", "debug_menus", flag(false), Some(vec!["Retail", "On (not retail)"]), "Shows the debug menus Skate 3 ships hidden; developer leftovers that may not all work."),
        row("physics_threads", "Opt-ins", "Physics Step Threads", "graphics.json", "physics_threads_multi", flag(false), Some(vec!["Single (default)", "Multi"]), "The physics step on one thread (faster on the test PCs) or on Bevy's multi-threaded scheduler."),
        row("zombie", "Opt-ins", "Zombie Mode", "graphics.json", "opt_ins.zombie", flag(false), Some(vec!["Off", "On"]), "The retail cheat: yellow screen, no traffic, every ped comes after you."),
        row("npc_physics", "Opt-ins", "NPC Skater Physics", "graphics.json", "opt_ins.npc_physics", flag(true), Some(vec!["On (default)", "Off"]), "NPC skaters near you run full skater physics and bail (still being tested)."),
        row("visualize_grabs", "Opt-ins", "Visualize Grabs", "graphics.json", "opt_ins.visualize_grabs", flag(false), Some(vec!["Off", "On"]), "Shows the props and car spots in grab range and logs grab availability."),
        row("vehicle_flee", "Opt-ins", "Ped Vehicle Flee", "graphics.json", "opt_ins.vehicle_flee", flag(false), Some(vec!["Off", "On"]), "Peds run from a car within 10 m that drives toward or away from them (still being tested)."),
        row("resolution", "Settings", "Resolution", "graphics.json", "resolution",
            ["1280x800", "1280x720", "1600x900", "1920x1080", "1920x1200", "2560x1440", "2560x1600", "3840x2160"].map(Value::from).to_vec(), None, "The window size."),
        row("scale", "Settings", "Render Scale", "graphics.json", "scale", [100, 25, 50, 67, 75, 85].map(Value::from).to_vec(), Some(vec!["100%", "25%", "50%", "67%", "75%", "85%"]), "The 3D render size against the window; lower is faster."),
        row("fps", "Settings", "Framerate Cap", "graphics.json", "fps", [0, 30, 60, 90, 120, 144, 165, 240].map(Value::from).to_vec(),
            Some(vec!["Unlimited", "30", "60", "90", "120", "144", "165", "240"]), "The highest frame rate the game runs at."),
        row("fps_counter", "Settings", "FPS Counter", "graphics.json", "fps_counter", ["off", "simple", "verbose"].map(Value::from).to_vec(), Some(vec!["Off", "Simple", "Verbose"]), "The on-screen frame-time readout."),
        row("master", "Settings", "Master Volume", "audio.json", "master", volumes(75), None, "Overall game volume."),
        row("ambience", "Settings", "Ambience Volume", "audio.json", "ambience", volumes(100), None, "The world's ambience beds."),
        row("effects", "Settings", "Effects Volume", "audio.json", "effects", volumes(100), None, "Skating and world sound effects."),
        row("difficulty", "Settings", "Difficulty", "gameplay.json", "difficulty", ["easy", "normal", "hardcore", "motorized"].map(Value::from).to_vec(),
            Some(vec!["Easy", "Normal", "Hardcore", "Motorized"]), "The skating difficulty (Custom is set in the game's Skater menu)."),
    ]
}

fn settings_dir(l: &Launcher) -> PathBuf {
    let assets = crate::status::current_assets(l);
    assets.parent().map_or_else(|| PathBuf::from("settings"), |p| p.join("settings"))
}

fn get<'a>(o: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(o, |o, part| o.get(part))
}

fn set(o: &mut Value, path: &str, value: Value) {
    let mut cur = o;
    let parts: Vec<&str> = path.split('.').collect();
    for part in &parts[..parts.len() - 1] {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        cur = cur.as_object_mut().expect("object").entry(*part).or_insert_with(|| Value::Object(Map::new()));
    }
    if !cur.is_object() {
        *cur = Value::Object(Map::new());
    }
    cur.as_object_mut().expect("object").insert(parts[parts.len() - 1].to_owned(), value);
}

/// `Read-GameJson`: the file, with the defaults a file from before `defaults_version` 1 takes once (as the game).
fn read(l: &Launcher, file: &str) -> Value {
    let path = settings_dir(l).join(file);
    let mut o = std::fs::read_to_string(&path).ok()
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        .filter(Value::is_object).unwrap_or_else(|| Value::Object(Map::new()));
    let version = get(&o, "defaults_version").and_then(Value::as_i64).unwrap_or(0);
    if path.exists() && (file == "graphics.json" || file == "audio.json") && version < 1 {
        if file == "graphics.json" { set(&mut o, "opt_ins.npc_physics", json!(true)) } else { set(&mut o, "decode_speech", json!(true)) }
        set(&mut o, "defaults_version", json!(1));
    }
    o
}

/// `Game-Value`.
fn value(r: &Row, o: &Value) -> Option<Value> {
    match r.field {
        "resolution" => {
            let w = get(o, "width")?;
            Some(Value::from(format!("{}x{}", shown_scalar(w), get(o, "height").map(shown_scalar).unwrap_or_default())))
        }
        "fps_counter" => {
            let stats = get(o, "frame_stats")?;
            if !truthy(stats) {
                return Some(Value::from("off"));
            }
            Some(Value::from(if get(o, "frame_stats_simple").is_some_and(truthy) { "simple" } else { "verbose" }))
        }
        field => get(o, field).filter(|v| !v.is_null()).cloned(),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// A value as PowerShell writes it in a string ("True", 1.0 as "1").
fn shown_scalar(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `Same-Value`: booleans only equal booleans; numbers compare as numbers; the rest as text.
fn same(a: &Value, b: &Value) -> bool {
    if a.is_boolean() || b.is_boolean() {
        return a.is_boolean() && b.is_boolean() && a == b;
    }
    let num = |v: &Value| shown_scalar(v).parse::<f64>().ok();
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        return (x - y).abs() < 1e-6;
    }
    shown_scalar(a) == shown_scalar(b)
}

fn index(r: &Row, o: &Value) -> i64 {
    let Some(cur) = value(r, o) else { return 0 };
    r.values.iter().position(|v| same(v, &cur)).map_or(-1, |i| i as i64)
}

fn shown(r: &Row, i: i64, o: &Value) -> String {
    if i < 0 {
        return format!("{} (custom)", value(r, o).map(|v| shown_scalar(&v)).unwrap_or_default());
    }
    match &r.shown {
        Some(s) => s[i as usize].to_owned(),
        None => shown_scalar(&r.values[i as usize]),
    }
}

/// `game-settings`: key, group, label, shown value, hint per row.
pub fn list(l: &Launcher) -> Vec<String> {
    let mut files: Vec<(&str, Value)> = Vec::new();
    rows().iter().map(|r| {
        if !files.iter().any(|(f, _)| *f == r.file) {
            files.push((r.file, read(l, r.file)));
        }
        let o = &files.iter().find(|(f, _)| *f == r.file).expect("read").1;
        format!("{}\t{}\t{}\t{}\t{}", r.key, r.group, r.label, shown(r, index(r, o), o), r.hint)
    }).collect()
}

/// `fixed-executor-single` / `-multi`: the game's physics thread setting.
pub fn set_physics_threads(l: &Launcher, multi: bool) -> Result<(), String> {
    let mut o = read(l, "graphics.json");
    set(&mut o, "physics_threads_multi", json!(multi));
    let dir = settings_dir(l);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("graphics.json"), serde_json::to_string_pretty(&o).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// `game-setting <key>`: steps the row to its next value and writes the file (not while a game runs).
pub fn step(l: &Launcher, key: &str) -> Result<(), String> {
    let all = rows();
    let r = all.iter().find(|r| r.key == key).ok_or_else(|| format!("Unknown setting: {key}"))?;
    if l.state_dir.join("PLAYING.txt").exists() {
        return Err("The game is running: change it in the game's overlay (Guide / F1) instead.".into());
    }
    let mut o = read(l, r.file);
    let next = r.values[((index(r, &o) + 1).rem_euclid(r.values.len() as i64)) as usize].clone();
    match r.field {
        "resolution" => {
            let text = next.as_str().unwrap_or_default().to_owned();
            let (w, h) = text.split_once('x').unwrap_or(("0", "0"));
            set(&mut o, "width", json!(w.parse::<i64>().unwrap_or(0)));
            set(&mut o, "height", json!(h.parse::<i64>().unwrap_or(0)));
        }
        "fps_counter" => {
            let v = next.as_str().unwrap_or("off");
            set(&mut o, "frame_stats", json!(v != "off"));
            set(&mut o, "frame_stats_simple", json!(v == "simple"));
        }
        field => set(&mut o, field, next),
    }
    let dir = settings_dir(l);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&o).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(r.file), text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_compare_and_show_like_the_powershell_launcher() {
        assert!(same(&json!(1.0), &json!(1)) && !same(&json!(true), &json!(1)) && same(&json!("normal"), &json!("normal")));
        assert_eq!((shown_scalar(&json!(1.0)), shown_scalar(&json!(0.75)), shown_scalar(&json!(true))), ("1".into(), "0.75".into(), "True".into()));
        let o = json!({"width": 1920, "height": 1080, "frame_stats": true, "frame_stats_simple": false});
        let all = rows();
        let res = all.iter().find(|r| r.key == "resolution").unwrap();
        assert_eq!(shown(res, index(res, &o), &o), "1920x1080");
        let fps = all.iter().find(|r| r.key == "fps_counter").unwrap();
        assert_eq!(shown(fps, index(fps, &o), &o), "Verbose");
        let draw = all.iter().find(|r| r.key == "npc_draw_distance").unwrap();
        let custom = json!({"npc_draw_distance": 0.75});
        assert_eq!(shown(draw, index(draw, &custom), &custom), "0.75 (custom)");
        assert_eq!(volumes(75).len(), 21);
    }
}
