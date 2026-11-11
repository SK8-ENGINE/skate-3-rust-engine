//! Starting an entry, as `launcher.ps1` Launch / Run-Tool do (doc 44a 1c): one game at a time, PLAYING.txt while it
//! runs, the per-entry environment, the session check afterwards, the result line in last_session.txt.
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::entries::Entry;
use crate::state::{flag, setting, text, Launcher};

const GAME_NAMES: &[&str] = &["skate3", "skate3rust", "rpcs3"];

fn now(fmt: &str) -> String {
    jiff::Zoned::now().strftime(fmt).to_string()
}

/// `Running-Games`: the game processes that run, as `<name>.exe`, sorted.
pub fn running_games() -> Vec<String> {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut names: Vec<String> = sys.processes().values().filter_map(|p| {
        let n = p.name().to_string_lossy().to_lowercase();
        let base = n.strip_suffix(".exe").unwrap_or(&n).to_owned();
        GAME_NAMES.contains(&base.as_str()).then(|| format!("{base}.exe"))
    }).collect();
    names.sort();
    names.dedup();
    names
}

/// `Running-Holder`.
pub fn running_holder(l: &Launcher) -> String {
    let games = running_games();
    if let Ok(t) = std::fs::read_to_string(l.state_dir.join("REFRESHING.txt")) {
        if !games.is_empty() {
            return format!("Refresh game files ({})", t.trim_start_matches('\u{feff}').trim());
        }
    }
    games.join(", ")
}

/// `Wait-GameTree`: a game can outlive what started it (a refresh restarts it into setup and back).
fn wait_game_tree() {
    let mut said = false;
    while !running_games().is_empty() {
        if !said {
            println!("  The game is still running (it restarted itself, for example to refresh its files)...");
            said = true;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// `Save-Result`: last_session.txt (UTF-8 with a byte order mark and CRLF, as PowerShell 5.1 writes it).
pub fn save_result(l: &Launcher, text: &str) {
    let body = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let _ = std::fs::write(l.state_dir.join("last_session.txt"), format!("\u{feff}{body}\r\n"));
}

/// Runs a play script (PLAY.bat through cmd on Windows, PLAY.sh elsewhere) and waits for the game tree.
fn run_script(script: &Path, arg: &str, envs: &[(String, String)]) {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        let mut line = format!("\"\"{}\"", script.display());
        if !arg.is_empty() {
            line.push(' ');
            line.push_str(arg);
        }
        line.push('"');
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.arg("/c").raw_arg(&line);
        }
        c
    } else {
        let mut c = Command::new("bash");
        c.arg(script);
        if !arg.is_empty() {
            c.arg(arg);
        }
        c
    };
    if let Some(dir) = script.parent() {
        c.current_dir(dir);
    }
    let _ = c.envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str()))).status();
    wait_game_tree();
}

/// A PowerShell helper script (send, share, recomp): Windows PowerShell, else PowerShell 7 when installed.
pub fn powershell(script: &Path, args: &[&str], envs: &[(String, String)]) -> Option<(i32, Vec<String>)> {
    let exe = if cfg!(windows) { "powershell.exe" } else { "pwsh" };
    let out = Command::new(exe).args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).args(args)
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str()))).output().ok()?;
    let lines = String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).filter(|l| !l.trim().is_empty()).collect();
    Some((out.status.code().unwrap_or(1), lines))
}

fn resolve_local(l: &Launcher, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() { path } else { l.repo.join(p) }
}

fn recomp_sessions(l: &Launcher) -> PathBuf {
    let p = l.config.get("recomp").map(|r| text(r, "sessions")).filter(|s| !s.is_empty()).unwrap_or_else(|| ".local\\recomp\\sessions".into());
    resolve_local(l, &p)
}

fn newest(dir: &Path, keep: impl Fn(&std::fs::DirEntry) -> bool, since: std::time::SystemTime, created: bool) -> Option<std::fs::DirEntry> {
    std::fs::read_dir(dir).ok()?.flatten().filter(|e| keep(e))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let t = if created { m.created().ok()? } else { m.modified().ok()? };
            (t >= since).then_some((t, e))
        })
        .max_by_key(|(t, _)| *t).map(|(_, e)| e)
}

fn count_lines(path: &Path, needle: &str) -> usize {
    std::fs::read_to_string(path).map(|t| t.lines().filter(|l| l.contains(needle)).count()).unwrap_or(0)
}

/// `Check-Session`: a one-line summary of the session written since `since`.
/// `root`: the checkout whose logs/ the session wrote (the version's, or the repository's for `pending`).
fn check_session(l: &Launcher, kind: &str, since: std::time::SystemTime, root: &Path) -> String {
    let root = root.to_path_buf();
    match kind {
        "rust" => {
            let Some(f) = newest(&root.join("logs"), |e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with("game-") && n.ends_with(".log") && !n.ends_with(".stderr.log") }, since, false) else {
                return "No new game log was written.".into();
            };
            let name = f.file_name().to_string_lossy().into_owned();
            let base = name.trim_end_matches(".log");
            let panics = count_lines(&f.path().with_file_name(format!("{base}.stderr.log")), "panicked");
            let frontend = count_lines(&f.path(), "AUDIO_NATIVE frontend");
            let mut line = format!("{name}: {}", if panics > 0 { format!("{panics} panic(s) - see {base}.stderr.log") } else { "no panics".into() });
            if frontend > 0 {
                line.push_str(&format!(", {frontend} menu / marker sounds logged"));
            }
            line
        }
        "statelog" => {
            let dir = l.repo.join(".local").join("audio-state-logs");
            let Some(f) = newest(&dir, |e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with("state_") && n.ends_with(".tsv") }, since, false) else {
                return "No new state log was written.".into();
            };
            println!("  Checking the state log...");
            let text = std::fs::read_to_string(f.path()).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            let cols = lines.first().map_or(0, |h| h.split('\t').count());
            let bad = lines.iter().skip(1).filter(|l| l.split('\t').count() != cols).count();
            let rows = lines.len().saturating_sub(1);
            let mins = (rows as f64 / 3600.0 * 10.0).round() / 10.0;
            format!("{}: {rows} rows (~{mins} min), {bad} malformed - {}", f.file_name().to_string_lossy(), if bad == 0 { "OK" } else { "MALFORMED - unusable" })
        }
        "traceall" => {
            let Some(d) = newest(&root.join("logs"), |e| e.file_name().to_string_lossy().starts_with("trace-all-") && e.path().is_dir(), since, false) else {
                return "No new trace-all folder was written.".into();
            };
            println!("  Checking the trace-all tables...");
            let mut parts = Vec::new();
            for name in ["audio_state.tsv", "frames.tsv"] {
                let Ok(text) = std::fs::read_to_string(d.path().join(name)) else { continue };
                let (mut cols, mut rows, mut bad) = (0, 0, 0);
                for line in text.lines().filter(|l| !l.starts_with('#')) {
                    let n = line.split('\t').count();
                    if cols == 0 { cols = n; continue; }
                    rows += 1;
                    if n != cols { bad += 1; }
                }
                parts.push(format!("{name} {rows} rows, {bad} malformed{}", if bad != 0 { " - MALFORMED - unusable" } else { "" }));
            }
            let mb = (crate::builds::folder_bytes(&d.path()) as f64 / 1048576.0).round();
            let dname = d.file_name().to_string_lossy().into_owned();
            if parts.is_empty() { format!("{dname}: {mb} MB, no tables") } else { format!("{dname} ({mb} MB): {}", parts.join("; ")) }
        }
        "recomp" => {
            let Some(d) = newest(&recomp_sessions(l), |e| e.path().is_dir(), since, true) else { return "No new recomp session folder.".into() };
            let dname = d.file_name().to_string_lossy().into_owned();
            let t = d.path().join("trace.tsv");
            let Ok(meta) = std::fs::metadata(&t) else { return format!("{dname}: no trace.tsv") };
            let mb = (meta.len() as f64 / 1048576.0).round();
            let check = l.config.get("recomp").map(|r| text(r, "trace_check")).filter(|s| !s.is_empty()).map(|s| resolve_local(l, &s));
            let Some(check) = check.filter(|c| c.exists()) else { return format!("{dname}: {mb} MB trace (no trace_check configured)") };
            println!("  Checking the trace (can take a minute for long sessions)...");
            let py = if cfg!(windows) { Command::new("py").arg("-3.13").arg(&check).arg(&t).output() } else { Command::new("python3").arg(&check).arg(&t).output() };
            let out = py.map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))).unwrap_or_default();
            let Some(last) = out.lines().filter(|l| l.contains("malformed")).last() else { return format!("{dname}: could not read the check result") };
            let counts: Vec<u64> = last.split(':').skip(1).filter_map(|p| p.trim_start().split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok())).collect();
            if counts.is_empty() {
                return format!("{dname}: could not read the check result");
            }
            let bad: u64 = counts.iter().sum();
            format!("{dname}: {mb} MB trace, {bad} malformed lines - {}", if bad == 0 { "OK" } else { "MALFORMED - unusable" })
        }
        _ => String::new(),
    }
}

/// The version's play script for this OS.
fn play_script(l: &Launcher, v: &Value) -> PathBuf {
    l.version_dir(v).join(if cfg!(windows) { "PLAY.bat" } else { "PLAY.sh" })
}

/// `Send-Sessions`: the after-session send ("" when none is set up).
pub fn send_sessions(l: &Launcher, envs: &[(String, String)]) -> String {
    let Some(script) = l.config.get("after_session").and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| resolve_local(l, s)) else { return String::new() };
    let blocked = crate::status::send_blocked(l);
    if !blocked.is_empty() {
        return format!("Send: {blocked}");
    }
    println!("  Sending the logs...");
    let Some((_, out)) = powershell(&script, &["-SendOnly"], envs) else { return "Send: failed (PowerShell is not available).".into() };
    let Some(last) = out.last() else { return "Send: no output.".into() };
    let sent = out.iter().filter(|l| l.starts_with("Sent ") && !(l.contains(" file(s) to GitHub"))).count();
    if sent > 0 { format!("Send: {sent} file(s). {last}") } else { format!("Send: {last}") }
}

/// `Share-Sessions`.
pub fn share_sessions(l: &Launcher, root: &Path) -> String {
    if !crate::status::share(l, root).starts_with("on ") {
        return String::new();
    }
    println!("  Packing the session for GitHub...");
    let script = [l.here.join("share-session-issue.ps1"), l.repo.join("tools").join("second-pc").join("share-session-issue.ps1")].into_iter().find(|s| s.exists());
    let Some(script) = script else { return "Share: share: not available in this launcher".into() };
    let settings = l.repo.join("share-sessions.txt");
    let out = powershell(&script, &["-Root", &root.to_string_lossy(), "-Settings", &settings.to_string_lossy()], &[]).map(|(_, o)| o).unwrap_or_default();
    let said: Vec<&String> = out.iter().filter(|l| !l.starts_with("https://")).collect();
    format!("Share: {}", said.last().map_or("no output.", |s| s.as_str()))
}

/// The installation's assets a version plays with (SKATE3_ASSETS, its assets link, the data installations), as
/// Resolve-DevAssets.ps1 / PLAY.sh resolve them.
pub fn dev_assets(d: &Path) -> Option<PathBuf> {
    if let Some(a) = std::env::var_os("SKATE3_ASSETS").map(PathBuf::from) {
        return a.join("private").join("game.json").is_file().then_some(a);
    }
    if d.join("assets").join("private").join("game.json").is_file() {
        return Some(d.join("assets"));
    }
    for base in [d.join("data"), d.join("..").join("data")] {
        let dir = std::fs::read_to_string(base.join("installation.json")).ok()
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()).map(|m| text(&m, "directory"));
        if let Some(dir) = dir.filter(|d| d.starts_with("installations/")) {
            let a = base.join(dir).join("assets");
            if a.join("private").join("game.json").is_file() {
                return Some(a);
            }
        }
    }
    None
}

/// `Refresh-GameFiles`: the game's own check without playing; setup runs only for what is out of date.
fn refresh_game_files(l: &Launcher, v: &Value) -> String {
    let mut log = String::new();
    let name = text(v, "name");
    if !crate::ensure::ensure_version(l, v, &mut log) {
        return format!("Refresh game files: {name} could not be built. Build log: {log}");
    }
    let d = l.version_dir(v);
    if l.use_release(v) {
        let code = Command::new(d.join(crate::cli::GAME_EXE)).arg("--update-game-files").status().ok().and_then(|s| s.code()).unwrap_or(1);
        return format!("Refresh game files: exit code {code}.");
    }
    let Some(assets) = dev_assets(&d) else { return "Refresh game files: no game files yet. Convert them first (tools\\setup-dev-assets.ps1).".into() };
    println!("  Checking the game files ({})...", assets.display());
    let marker = l.state_dir.join("REFRESHING.txt");
    let _ = std::fs::write(&marker, format!("\u{feff}{name}\r\n"));
    let out = Command::new(d.join("bin").join(crate::cli::GAME_EXE)).arg("--update-game-files").arg("--assets").arg(&assets).output();
    let _ = std::fs::remove_file(&marker);
    let Ok(out) = out else { return "Refresh game files: failed (exit code 1).".into() };
    let mut said = String::new();
    for line in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) {
        if let Some(rest) = line.strip_prefix("GAME_FILES ") {
            said = rest.to_owned();
        }
        println!("  {line}");
    }
    match out.status.code().unwrap_or(1) {
        0 => format!("Refresh game files: {said}."),
        2 => "Refresh game files: no game files yet. Convert them first (tools\\setup-dev-assets.ps1).".into(),
        code => format!("Refresh game files: {} (exit code {code}).", if said.is_empty() { "failed".to_owned() } else { said }),
    }
}

fn recomp_script(l: &Launcher) -> Option<PathBuf> {
    if let Some(s) = setting(&l.config, "recomp_script").and_then(Value::as_str) {
        return Some(resolve_local(l, s));
    }
    let d = l.repo.join("tools").join("second-pc").join("play-recomp.ps1");
    (d.exists() && l.repo.join("send-sessions.txt").exists()).then_some(d)
}

/// `Run-Tool`. Returns false when a build or refresh failed (the command line run then exits with 1).
fn run_tool(l: &Launcher, key: &str, v: &Value, path_arg: &str) -> bool {
    let stamp = now("%Y-%m-%d %H:%M:%S");
    let name = text(v, "name");
    let r = match key {
        "tool-setup" => {
            let ok = if cfg!(windows) {
                Command::new("powershell.exe").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(l.here.join("setup.ps1")).arg("-Install").status()
            } else {
                Command::new("sh").arg(l.here.join("setup.sh")).arg("--install").status()
            }.is_ok_and(|s| s.success());
            if ok { "First-time setup: everything is installed.".to_owned() } else { "First-time setup: not finished (see the steps above).".to_owned() }
        }
        "tool-self-update" => crate::selfupdate::self_update(l, true).unwrap_or_else(|| "Launcher: up to date.".into()),
        "tool-build" => {
            let auto = setting(&l.config, "self_update").and_then(Value::as_bool) != Some(false);
            let me = if auto { crate::selfupdate::self_update(l, false).unwrap_or_default() } else { String::new() };
            if !me.is_empty() {
                println!("  {me}");
            }
            let mut log = String::new();
            let r = if crate::ensure::ensure_version(l, v, &mut log) {
                format!("Ready: {name} ({})", crate::cli::version_status(l, v))
            } else {
                format!("Build failed: {name}. Build log: {log}")
            };
            if !me.is_empty() && !me.starts_with("Launcher: up to date") { format!("{me}\r\n{r}") } else { r }
        }
        "tool-send" => send_sessions(l, &[]),
        "tool-share" => { let r = share_sessions(l, &l.version_dir(v)); if r.is_empty() { "Share: off.".into() } else { r } }
        "tool-dlc" => {
            let (mut r, added) = crate::dlc::import(l, path_arg);
            if added > 0 {
                println!("  Converting the new DLC (Refresh game files)...");
                r = format!("{r}\r\n{}", refresh_game_files(l, v));
            }
            r
        }
        "tool-recomp-update" | "tool-recomp-getsave" => {
            let (flag_arg, what) = if key == "tool-recomp-update" { ("-Update", "Recomp update") } else { ("-GetSave", "Recomp save") };
            let code = recomp_script(l).and_then(|s| powershell_passthrough(&s, &[flag_arg])).unwrap_or(1);
            format!("{what}: exit code {code}{}", if code == 0 { " (done)" } else { " - see above" })
        }
        "tool-refresh" => refresh_game_files(l, v),
        _ => String::new(),
    };
    let failed = matches!(key, "tool-build" | "tool-refresh") && (r.starts_with("Build failed") || r.contains("could not be built")
        || r.starts_with("Refresh game files: exit code ") && !r.starts_with("Refresh game files: exit code 0")
        || (r.ends_with(").") && r.contains("(exit code ") && !r.contains("(exit code 0)")));
    save_result(l, &format!("{stamp}  {key} [{}]\r\n{r}", text(v, "id")));
    println!();
    println!("  {r}");
    !failed
}

fn powershell_passthrough(script: &Path, args: &[&str]) -> Option<i32> {
    let exe = if cfg!(windows) { "powershell.exe" } else { "pwsh" };
    Command::new(exe).args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(script).args(args).status().ok()?.code()
}

/// `Launch`: runs entry `key` (label `label`) for version `version_id` ("" = the selected one). False = it failed in
/// a way the command line reports with exit code 1 (a tool's build or refresh).
pub fn launch(l: &Launcher, key: &str, label: &str, version_id: &str, path_arg: &str) -> bool {
    let all = crate::entries::entries(l);
    let Some(e) = all.iter().find(|e| e.id == key) else {
        save_result(l, &format!("Unknown entry: {key}"));
        return true;
    };
    if !running_games().is_empty() {
        save_result(l, &format!("Not started: {} is running.", running_holder(l)));
        return true;
    }
    let v = if version_id.is_empty() { l.selected() } else { l.versions.iter().find(|x| text(x, "id") == version_id).unwrap_or(&l.versions[0]) };
    let blocked = crate::entries::blocked_for(l, e, v);
    if !blocked.is_empty() {
        save_result(l, &format!("Not started: {blocked}"));
        println!("  {blocked}");
        return true;
    }
    if e.group == "tools" {
        return run_tool(l, key, v, path_arg);
    }
    let mut envs: Vec<(String, String)> = Vec::new();
    let mut ver_text = String::new();
    let mut label = label.to_owned();
    if e.group == "rust" {
        ver_text = format!(" [{}]", text(v, "id"));
        println!("  Version: {}", text(v, "name"));
        let mut log = String::new();
        if !crate::ensure::ensure_version(l, v, &mut log) {
            save_result(l, &format!("Not started: {} could not be set up or built.{}", text(v, "name"), if log.is_empty() { String::new() } else { format!(" Build log: {log}") }));
            return true;
        }
        let d = l.version_dir(v);
        let disc = l.setting_path("disc");
        let stock = d.join("assets").join("private").join("stock");
        if l.use_release(v) {
            if let Some(disc) = &disc {
                envs.push(("SKATE3_SETUP_DISC".into(), disc.clone()));
            }
        } else if let Some(disc) = disc.as_ref().filter(|p| Path::new(p).exists()).filter(|_| !stock.exists()) {
            println!("  Converting the game files from {disc} (once; this takes a while)...");
            if cfg!(windows) {
                let _ = powershell_passthrough(&d.join("tools").join("setup-dev-assets.ps1"), &["-Disc", disc]);
            } else {
                let _ = Command::new("bash").arg(d.join("SETUP.sh")).arg(disc).current_dir(&d).status();
            }
        }
        if !l.use_release(v) && !stock.exists() {
            let how = if cfg!(windows) {
                format!("powershell -ExecutionPolicy Bypass -File \"{}\" -Disc <your Skate 3 ISO or default.xex>", d.join("tools").join("setup-dev-assets.ps1").display())
            } else {
                format!("{} <your Skate 3 ISO or default.xex>", d.join("SETUP.sh").display())
            };
            let msg = format!("Not started: {} has no game files (assets\\private). Convert them from the disc once: {how}.", text(v, "name"));
            save_result(l, &msg);
            println!("  {msg}");
            return true;
        }
        if let Some(Value::Object(vars)) = v.get("env") {
            for (k, val) in vars {
                envs.push((k.clone(), match val { Value::String(s) => s.clone(), other => other.to_string() }));
            }
        }
        envs.push(("SKATE_FIXED_EXECUTOR".into(), crate::status::fixed_executor_pub(l).into()));
    }
    let since = std::time::SystemTime::now();
    let playing = l.state_dir.join("PLAYING.txt");
    let _ = std::fs::write(&playing, format!("entry={key} label={label} version={} started={}\r\n", text(v, "id"), now("%Y-%m-%d %H:%M:%S")));
    println!("  Starting: {}{ver_text}", e.name);
    let state_logs = l.repo.join(".local").join("audio-state-logs");
    let play = play_script(l, v);
    match key {
        "rust-play" => run_script(&play, "", &envs),
        "rust-statelog" => {
            let _ = std::fs::create_dir_all(&state_logs);
            envs.push(("SKATE_AUDIO_STATE_LOG".into(), state_logs.join(format!("state_{}.tsv", now("%Y%m%d_%H%M%S"))).display().to_string()));
            envs.push(("SKATE_AEMS".into(), "1".into()));
            envs.push(("SKATE_AUDIO_TIMING".into(), "1".into()));
            run_script(&play, "", &envs);
        }
        "rust-trace" => { envs.push(("SKATE_AUDIO_TRACE".into(), "1".into())); run_script(&play, "", &envs) }
        "rust-devmods" => { envs.push(("SKATE3_MODS_ENABLE".into(), text(v, "mods_enable"))); run_script(&play, "", &envs) }
        "rust-perf" => run_script(&play, "-trace", &envs),
        "rust-traceall" => {
            let traces = crate::entries::traces(l, v);
            let dir = l.version_dir(v).join("logs").join(format!("trace-all-{}", now("%Y%m%d_%H%M%S")));
            let _ = std::fs::create_dir_all(&dir);
            let has = |k: &str| traces.contains(&k);
            if has("statelog") {
                envs.push(("SKATE_AUDIO_STATE_LOG".into(), dir.join("audio_state.tsv").display().to_string()));
                envs.push(("SKATE_AEMS".into(), "1".into()));
                envs.push(("SKATE_AUDIO_TIMING".into(), "1".into()));
            }
            if has("audiotrace") { envs.push(("SKATE_AUDIO_TRACE".into(), "1".into())); }
            if has("frametimes") { envs.push(("SKATE_FRAME_LOG".into(), dir.join("frames.tsv").display().to_string())); }
            if has("livingdbg") { envs.push(("SKATE_LIVING_WORLD_DEBUG".into(), "1".into())); }
            if has("traceall") {
                envs.push(("SKATE_TRACE_ALL".into(), "1".into()));
                envs.push(("SKATE_PERF_REPORT".into(), dir.join("perf.json").display().to_string()));
            }
            println!("  Traces: {} -> {}", traces.join(", "), dir.display());
            run_script(&play, "", &envs);
        }
        _ => {
            if let Some(pe) = &e.rpcs3 {
                run_rpcs3(l, pe);
                wait_game_tree();
            } else if let Some(re) = &e.recomp {
                let own = text(re, "label");
                if !own.is_empty() {
                    label = own;
                }
                let bat = text(re, "bat");
                if !bat.is_empty() {
                    let arg = if !label.is_empty() && label != "session" { label.clone() } else { text(re, "bat_args") };
                    run_script(&resolve_local(l, &bat), &arg, &[]);
                } else {
                    run_recomp(l, re, if label == "session" { "" } else { &label });
                    wait_game_tree();
                }
            }
        }
    }
    let _ = std::fs::remove_file(&playing);
    println!("  Session ended.");
    let kind = entry_kind(e);
    let mut summary = check_session(l, kind, since, &l.version_dir(v));
    let repaired = crate::ensure::repair_all_asset_links(l);
    if !repaired.is_empty() {
        summary = std::iter::once(summary).chain(repaired).collect::<Vec<_>>().join("\r\n");
    }
    let send_env = vec![("SKATE_SEND_LOGS_DIR".to_owned(), l.version_dir(v).display().to_string())];
    let sent = send_sessions(l, &send_env);
    if !sent.is_empty() {
        summary = [summary, sent].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\r\n");
    }
    let shared = share_sessions(l, &l.version_dir(v));
    if !shared.is_empty() {
        summary = [summary, shared].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\r\n");
    }
    save_result(l, &format!("{}  {key} {label}{ver_text}\r\n{summary}", now("%Y-%m-%d %H:%M:%S")));
    true
}

fn newest_time(dir: &Path, keep: impl Fn(&str, bool) -> bool, created: bool) -> Option<std::time::SystemTime> {
    std::fs::read_dir(dir).ok()?.flatten()
        .filter(|e| keep(&e.file_name().to_string_lossy(), e.path().is_dir()))
        .filter_map(|e| { let m = e.metadata().ok()?; if created { m.created().ok() } else { m.modified().ok() } })
        .max()
}

/// `pending`, run by the window at every start: the launcher's own update, the assets links, and the newest session
/// when it closed before its check (Steam's Exit game closes the launcher with the game). True = a result was written
/// (exit code 10).
pub fn pending(l: &Launcher) -> bool {
    let ui = |t: &str| if std::env::var("SKATE_LAUNCHER_UI").is_ok_and(|v| v == "2") { println!("LAUNCHER_STAGE\t{t}") };
    let _ = std::fs::remove_file(l.here.join("SkateLauncher.old.exe"));
    let auto = setting(&l.config, "self_update").and_then(Value::as_bool) != Some(false);
    let no_self = std::env::var("SKATE_LAUNCHER_NO_SELF").is_ok_and(|v| v == "1");
    let mut self_said = if auto && !no_self && running_games().is_empty() { crate::selfupdate::self_update(l, false).unwrap_or_default() } else { String::new() };
    if cfg!(windows) {
        let (cs, exe, restart) = (l.here.join("SkateLauncher.cs"), l.here.join("SkateLauncher.exe"), l.state_dir.join("restart.txt"));
        let newer = std::fs::metadata(&cs).and_then(|m| m.modified()).ok().zip(std::fs::metadata(&exe).and_then(|m| m.modified()).ok()).is_some_and(|(c, e)| c > e);
        if !restart.exists() && newer {
            ui("Building the updated launcher");
            if crate::selfupdate::rebuild_launcher_exe(l) {
                let _ = std::fs::write(&restart, "local\r\n");
            }
        }
    }
    if self_said.starts_with("Launcher: up to date") {
        self_said.clear();
    }
    if running_games().is_empty() {
        ui("Checking the game file links");
        for line in crate::ensure::repair_all_asset_links(l) {
            println!("  {line}");
        }
    }
    ui("Looking for your last session");
    let last = l.state_dir.join("last_session.txt");
    let since = std::fs::metadata(&last).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    let second = std::time::Duration::from_secs(1);
    let state_log = newest_time(&l.repo.join(".local").join("audio-state-logs"), |n, d| !d && n.starts_with("state_") && n.ends_with(".tsv"), false);
    let recomp = if l.config.is_null() { None } else { newest_time(&recomp_sessions(l), |_, d| d, true) };
    let (mut kind, mut t0) = ("", since);
    if let Some(t) = state_log.filter(|t| *t > since) {
        kind = "statelog";
        t0 = t - second;
    }
    if let Some(t) = recomp.filter(|t| *t > since && state_log.is_none_or(|s| *t > s)) {
        kind = "recomp";
        t0 = t - second;
    }
    let logs_root = if l.standalone { l.version_dir(l.selected()).join("logs") } else { l.repo.join("logs") };
    let trace_all = std::fs::read_dir(&logs_root).ok().and_then(|r| r.flatten()
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("trace-all-"))
        .filter_map(|e| { let m = e.metadata().ok()?; Some((m.modified().ok()?, m.created().ok()?)) })
        .max_by_key(|(m, _)| *m));
    if let Some((_, created)) = trace_all.filter(|(m, _)| *m > since && *m > t0) {
        kind = "traceall";
        t0 = created - second;
    }
    let playing = l.state_dir.join("PLAYING.txt");
    if playing.exists() && running_games().is_empty() {
        let _ = std::fs::remove_file(&playing);
    }
    if playing.exists() {
        return false;
    }
    let game = std::fs::read_dir(&logs_root).ok().and_then(|r| r.flatten()
        .filter(|e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with("game-") && n.ends_with(".log") })
        .filter_map(|e| { let m = e.metadata().ok()?; Some((m.modified().ok()?, m.created().ok()?)) })
        .max_by_key(|(m, _)| *m));
    let has_send = l.config.get("after_session").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let missed_send = has_send && (game.is_some_and(|(m, _)| m > since) || trace_all.is_some_and(|(m, _)| m > since));
    if kind.is_empty() {
        if let Some((_, created)) = game.filter(|(m, _)| *m > since) {
            kind = "rust";
            t0 = created - second;
        }
    }
    if kind.is_empty() && !missed_send {
        if !self_said.is_empty() {
            save_result(l, &format!("{}  launcher update\r\n{self_said}", now("%Y-%m-%d %H:%M:%S")));
            return true;
        }
        return false;
    }
    let mut summary = String::new();
    if !kind.is_empty() {
        println!("  Checking the last session (it closed before its check)...");
        ui("Checking your last session");
        summary = check_session(l, kind, t0, &l.repo);
    }
    if missed_send {
        ui("Sending your last session");
        summary = [summary, send_sessions(l, &[])].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\r\n");
    }
    if !self_said.is_empty() {
        summary = format!("{self_said}\r\n{summary}");
    }
    save_result(l, &format!("{}  (checked at next start)\r\n{summary}", now("%Y-%m-%d %H:%M:%S")));
    true
}

/// The entry's session kind (what Check-Session looks at).
fn entry_kind(e: &Entry) -> &'static str {
    match e.id.as_str() {
        "rust-traceall" => "traceall",
        "rust-statelog" => "statelog",
        id if id.starts_with("rust-") => "rust",
        _ => if e.recomp.as_ref().is_some_and(|r| !text(r, "trace").is_empty() || !text(r, "bat").is_empty()) { "recomp" } else { "none" },
    }
}

/// `Run-Recomp`: the recomp with research-hooks tracing into a session folder.
fn run_recomp(l: &Launcher, re: &Value, label: &str) {
    let r = l.config.get("recomp").cloned().unwrap_or(Value::Null);
    let exe = text(&r, "exe");
    let args = { let a = text(&r, "args"); if a.is_empty() { "--skate3_demo_path --skate3_demo_path_signed_in".to_owned() } else { a } };
    let cwd = { let w = text(&r, "working_dir"); if w.is_empty() { Path::new(&exe).parent().map(Path::to_path_buf).unwrap_or_default() } else { PathBuf::from(w) } };
    let mut c = Command::new(&exe);
    c.args(args.split_whitespace()).current_dir(&cwd);
    let trace = text(re, "trace");
    if trace.is_empty() {
        let _ = c.status();
        return;
    }
    let prefix = { let f = text(re, "folder"); if f.is_empty() { text(re, "id").trim_start_matches("recomp-").to_owned() } else { f } };
    let name = if label.is_empty() { format!("{prefix}_{}", now("%Y%m%d_%H%M%S")) } else { format!("{prefix}_{label}_{}", now("%Y%m%d_%H%M%S")) };
    let out = recomp_sessions(l).join(name);
    let _ = std::fs::create_dir_all(&out);
    c.env("SKATE3_AUDIO_TRACE_FILE", out.join("trace.tsv")).env("SKATE3_TRACE", &trace).env("SKATE3_INPUT_RECORD", out.join("pad.txt"));
    if flag(re, "capture") {
        c.env("SKATE3_AUDIO_CAPTURE", out.join("audio.f32"));
    }
    println!("  Session folder: {}", out.display());
    let stdout = std::fs::File::create(out.join("game.out")).ok();
    let stderr = std::fs::File::create(out.join("game.err")).ok();
    if let (Some(o), Some(e)) = (stdout, stderr) {
        c.stdout(o).stderr(e);
    }
    let _ = c.status();
}

/// `Run-Rpcs3`: RPCS3 straight into the game.
fn run_rpcs3(l: &Launcher, pe: &Value) {
    let r = l.config.get("rpcs3").cloned().unwrap_or(Value::Null);
    let exe = text(&r, "exe");
    let mut args = text(&r, "args");
    if args.is_empty() {
        args = "--no-gui --fullscreen".into();
    }
    let own = text(pe, "args");
    if !own.is_empty() {
        args = own;
    }
    let _ = Command::new(&exe).args(args.split_whitespace()).arg(text(&r, "game"))
        .current_dir(Path::new(&exe).parent().unwrap_or(Path::new("."))).status();
}
