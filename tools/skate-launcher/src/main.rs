//! The couch launcher for every OS (docs/hails-additions/44-every-os.md; spec of the PowerShell launcher it replaces:
//! 44a). The text commands and entries of `launcher.ps1`, with the same output; the window follows.
//!
//! `skate-launcher <command or entry> [label] [-Version <id>] [-Path <file or folder>]`
mod builds;
mod cli;
mod dlc;
mod ensure;
mod entries;
mod ojson;
mod play;
mod selfupdate;
mod settings;
mod state;
mod status;

use std::process::{Command, ExitCode};

/// `setup-check`: the OS's first-time setup check (setup.ps1 / setup.sh), its lines and exit code passed through.
fn setup_check(l: &state::Launcher) -> ExitCode {
    let status = if cfg!(windows) {
        Command::new("powershell.exe").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(l.here.join("setup.ps1")).status()
    } else {
        Command::new("sh").arg(l.here.join("setup.sh")).status()
    };
    exit(status.ok().and_then(|s| s.code()).unwrap_or(1))
}

fn exit(code: i32) -> ExitCode {
    if code == 0 { ExitCode::SUCCESS } else { ExitCode::from(u8::try_from(code).unwrap_or(1)) }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = Vec::new();
    let (mut version, mut path) = (String::new(), String::new());
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.to_ascii_lowercase().as_str() {
            "-version" | "--version" => version = it.next().unwrap_or_default(),
            "-path" | "--path" => path = it.next().unwrap_or_default(),
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
        "" => {
            eprintln!("The launcher window is not ported yet; the commands and entries work (see `list`).");
            return ExitCode::from(2);
        }
        key => {
            if entries::entries(&launcher).iter().any(|e| e.id == key) || key.starts_with("rust-") || key.starts_with("tool-") {
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
