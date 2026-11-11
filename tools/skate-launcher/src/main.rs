//! The couch launcher for every OS (docs/hails-additions/44-every-os.md; spec of the PowerShell launcher it replaces:
//! 44a). Step 2: the state folder and the text commands; the window follows.
mod cli;
mod state;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(here) = state::find_here() else {
        eprintln!("Could not find the launcher folder (tools/steam-launcher). Run this from the game's checkout, or set SKATE_LAUNCHER_DIR.");
        return ExitCode::from(2);
    };
    let launcher = state::Launcher::load(here);
    match args.first().map(String::as_str) {
        Some("versions") => {
            for line in cli::versions(&launcher) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Some("version-set") => match args.get(1).map(|id| cli::version_set(&launcher, id)) {
            Some(Ok(())) => ExitCode::SUCCESS,
            Some(Err(e)) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
            None => {
                eprintln!("Usage: skate-launcher version-set <id>");
                ExitCode::from(2)
            }
        },
        Some(other) => {
            eprintln!("Not ported yet: {other} (doc 44a parity checklist)");
            ExitCode::from(2)
        }
        None => {
            eprintln!("The launcher window is not ported yet; the text commands work (versions, version-set).");
            ExitCode::from(2)
        }
    }
}
