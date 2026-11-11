//! The Skate launcher for every OS (docs/hails-additions/44-every-os.md; spec of the PowerShell launcher it replaces:
//! 44a). The text commands and entries of `launcher.ps1`, with the same output; the window follows.
//!
//! `skate-launcher <command or entry> [label] [-Version <id>] [-Path <file or folder>]`
// A windowed program on Windows: Steam starts it as SkateLauncher.exe after the switchover, without a console.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod builds;
mod camcorder;
mod cli;
mod dlc;
mod ensure;
mod entries;
mod ojson;
mod pick;
mod play;
mod proc;
mod selfupdate;
mod settings;
mod state;
mod status;
mod ui;
mod wear;
mod window;

use std::process::ExitCode;

/// `setup-check`: the OS's first-time setup check (setup.ps1 / setup.sh), its lines and exit code passed through.
fn setup_check(l: &state::Launcher) -> ExitCode {
    let status = if cfg!(windows) {
        crate::proc::cmd("powershell.exe").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(l.here.join("setup.ps1")).status()
    } else {
        crate::proc::cmd("sh").arg(l.here.join("setup.sh")).status()
    };
    exit(status.ok().and_then(|s| s.code()).unwrap_or(1))
}

fn exit(code: i32) -> ExitCode {
    if code == 0 { ExitCode::SUCCESS } else { ExitCode::from(u8::try_from(code).unwrap_or(1)) }
}

fn main() -> ExitCode {
    proc::attach_parent_console();
    let mut args: Vec<String> = Vec::new();
    let (mut version, mut path, mut snapshot, mut snapshot_page, mut snapshot_seed) = (String::new(), String::new(), None, None, None);
    let (mut same_monitor, mut after_update) = (false, false);
    let mut snapshot_size = None;
    let mut bench = None;
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.to_ascii_lowercase().as_str() {
            "-version" | "--version" => version = it.next().unwrap_or_default(),
            "-path" | "--path" => path = it.next().unwrap_or_default(),
            // --snapshot <out.png> [<screen>[@t][#seed]] [WxH] (SkateLauncher.exe's form), or with the flags below.
            "--snapshot" => {
                snapshot = it.next().map(std::path::PathBuf::from);
                for _ in 0..2 {
                    let Some(r) = it.next_if(|r| !r.starts_with('-')) else { break };
                    if let Some((w, h)) = r.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))) {
                        snapshot_size = Some((w, h));
                    } else {
                        snapshot_page = Some(r);
                    }
                }
            }
            "--snapshot-page" => snapshot_page = it.next(),
            "--snapshot-seed" => snapshot_seed = it.next().and_then(|s| s.parse().ok()),
            "--same-monitor" => same_monitor = true,
            // --bench <out.txt> [WxH]: render times of the window (SkateLauncher.exe's form).
            "--bench" => {
                bench = it.next().map(std::path::PathBuf::from);
                if let Some(r) = it.next_if(|r| r.contains('x') && !r.starts_with('-')) {
                    snapshot_size = r.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)));
                }
            }
            "--after-update" => after_update = true,
            "--sticker-sheet" => {
                let out = it.next().unwrap_or_else(|| "stickers.png".into());
                let only = it.next().and_then(|d| d.parse().ok());
                return match wear::sticker_sheet(std::path::Path::new(&out), only) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("{e}");
                        ExitCode::FAILURE
                    }
                };
            }
            _ => args.push(a),
        }
    }
    let Some(here) = state::find_here() else {
        eprintln!("Could not find the launcher folder (tools/steam-launcher). Run this from the game's checkout, or set SKATE_LAUNCHER_DIR.");
        return ExitCode::from(2);
    };
    let launcher = state::Launcher::load(here);
    let dlc = dlc::dlc_dir(&launcher);
    if dlc.exists() {
        // SAFETY: still one thread; every game, setup and refresh started from here inherits it (SKATE3_DLC_DIR).
        unsafe { std::env::set_var("SKATE3_DLC_DIR", &dlc) };
    }
    let arg = |n: usize| args.get(n).map_or("", String::as_str);
    let lines = match arg(0) {
        "versions" => cli::versions(&launcher),
        "notes" => cli::notes(&launcher, if arg(1).is_empty() { &version } else { arg(1) }),
        "version-set" => cli::version_step(&launcher, 0, arg(1)),
        "version-next" => cli::version_step(&launcher, 1, ""),
        "version-prev" => cli::version_step(&launcher, -1, ""),
        "config-get" => cli::config_get(&launcher),
        "status" => status::status(&launcher),
        "entries" => entries::list(&launcher, &version),
        "game-settings" => settings::list(&launcher),
        "game-setting" => match settings::step(&launcher, arg(1)) {
            Ok(()) => Vec::new(),
            Err(e) => {
                println!("{e}");
                return ExitCode::FAILURE;
            }
        },
        "sizes" => builds::sizes(&launcher),
        "config-set" => match cli::config_set(&launcher, arg(1)) {
            Ok(lines) => lines,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        },
        "build" => {
            let id = if arg(1).is_empty() { version.as_str() } else { arg(1) };
            let v = if id.is_empty() { launcher.selected() } else { launcher.versions.iter().find(|x| state::text(x, "id") == id).unwrap_or(&launcher.versions[0]) };
            let mut log = String::new();
            if ensure::ensure_version(&launcher, v, &mut log) {
                println!("Ready: {} ({})", state::text(v, "name"), cli::version_status(&launcher, v));
                return ExitCode::SUCCESS;
            }
            println!("Build failed: {}. Build log: {log}", state::text(v, "name"));
            return ExitCode::FAILURE;
        }
        "list" => entries::entries(&launcher).iter().map(|e| format!("{:<20} {}", e.id, e.name)).collect(),
        "setup-check" => return setup_check(&launcher),
        "clear-cache" | "clear-build" => match builds::clear(&launcher, arg(0) == "clear-build", arg(1)) {
            Ok(line) => vec![line],
            Err(e) => {
                println!("{e}");
                return ExitCode::FAILURE;
            }
        },
        "room" => {
            let id = if arg(1).is_empty() { version.as_str() } else { arg(1) };
            let v = if id.is_empty() { launcher.selected() } else { launcher.versions.iter().find(|x| state::text(x, "id") == id).unwrap_or(&launcher.versions[0]) };
            ensure::make_room(&launcher, v);
            Vec::new()
        }
        "send" => {
            let r = play::send_sessions(&launcher, &[]);
            vec![if r.is_empty() { "Send: no after_session in config.json.".into() } else { r }]
        }
        "fixed-executor-single" | "fixed-executor-multi" => {
            let _ = settings::set_physics_threads(&launcher, arg(0) == "fixed-executor-multi");
            Vec::new()
        }
        "pending" => return if play::pending(&launcher) { ExitCode::from(10) } else { ExitCode::SUCCESS },
        "launcher-exe" => {
            if selfupdate::rebuild_launcher_exe(&launcher) {
                println!("ok");
                return ExitCode::SUCCESS;
            }
            println!("failed");
            return ExitCode::FAILURE;
        }
        "share-on" | "share-off" => {
            let flag = if arg(0) == "share-on" { "-Consent" } else { "-Revoke" };
            let root = launcher.version_dir(launcher.selected());
            let script = [launcher.here.join("share-session-issue.ps1"), launcher.repo.join("tools").join("second-pc").join("share-session-issue.ps1")]
                .into_iter().find(|s| s.exists());
            match script {
                Some(s) => play::powershell(&s, &["-Root", &root.to_string_lossy(), "-Settings", &launcher.repo.join("share-sessions.txt").to_string_lossy(), flag], &[])
                    .map(|(_, out)| out).unwrap_or_else(|| vec!["share: PowerShell is not available".into()]),
                None => vec!["share: not available in this launcher".into()],
            }
        }
        "" | "--window" => {
            let direct: Vec<String> = args.iter().skip(1).cloned().collect();
            // One launcher per folder: a second start brings the first to the front instead (snapshots run alongside).
            if snapshot.is_none() && bench.is_none() && !window::single_instance(&launcher.state_dir) {
                return ExitCode::SUCCESS;
            }
            let same_monitor = same_monitor || window::same_monitor_file(&launcher.state_dir).exists();
            return match ui::run(launcher.state_dir.clone(), direct, snapshot, snapshot_page, snapshot_seed, same_monitor, after_update, snapshot_size, bench) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("The launcher window could not open: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        key => {
            let is_entry = entries::entries(&launcher).iter().any(|e| e.id == key) || key.starts_with("rust-") || key.starts_with("tool-");
            // A launch option (Steam: `skate-launcher rust-play [label] [@version]`) opens the window and runs it there, as
            // SkateLauncher.exe does; the window's own jobs come with SKATE_LAUNCHER_UI=2 and run here headless.
            if is_entry && std::env::var_os("SKATE_LAUNCHER_UI").is_none() {
                if !window::single_instance(&launcher.state_dir) { return ExitCode::SUCCESS; }
                let same_monitor = same_monitor || window::same_monitor_file(&launcher.state_dir).exists();
                return match ui::run(launcher.state_dir.clone(), args.clone(), None, None, None, same_monitor, after_update, None, None) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("The launcher window could not open: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
            if is_entry {
                return if play::launch(&launcher, key, arg(1), &version, &path) { ExitCode::SUCCESS } else { ExitCode::FAILURE };
            }
            eprintln!("Not ported yet: {key} (doc 44a parity checklist)");
            return ExitCode::from(2);
        }
    };
    for line in lines {
        println!("{line}");
    }
    ExitCode::SUCCESS
}
