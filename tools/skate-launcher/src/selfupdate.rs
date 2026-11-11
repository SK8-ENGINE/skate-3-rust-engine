//! The launcher bringing its own files to the newest pushed branch on the fork, as `launcher.ps1` Self-Update (doc 44a
//! 1d): a shallow bare clone in the state folder (self.git) holds the branch; tools/steam-launcher and tools/second-pc
//! are copied from it, local files that differ are backed up first, nothing is committed.
use std::io::Read;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use sha2::Digest;

use crate::state::{setting, Launcher};

const PATHS: &[&str] = &["tools/steam-launcher", "tools/second-pc"];

fn now(fmt: &str) -> String {
    jiff::Zoned::now().strftime(fmt).to_string()
}

fn log(l: &Launcher, text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(l.state_dir.join("self-update.log")) {
        let _ = write!(f, "{}  {text}\r\n", now("%Y-%m-%d %H:%M:%S"));
    }
}

fn ui_line(text: &str) {
    if std::env::var("SKATE_LAUNCHER_UI").is_ok_and(|v| v == "2") {
        println!("{text}");
    }
}

fn short(sha: &str) -> String {
    sha.get(..8).unwrap_or(sha).to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// `Get-PrebuiltLauncher` + `Rebuild-LauncherExe` (Windows: the window is SkateLauncher.exe): the release's prebuilt exe
/// when its sidecar names this SkateLauncher.cs, else compiled with the .NET Framework's csc. True = rebuilt.
pub fn rebuild_launcher_exe(l: &Launcher) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let cs = l.here.join("SkateLauncher.cs");
    let (exe, old, new) = (l.here.join("SkateLauncher.exe"), l.here.join("SkateLauncher.old.exe"), l.here.join("SkateLauncher.new.exe"));
    let fork = setting(&l.config, "fork_url").and_then(Value::as_str).unwrap_or("https://github.com/Hailey-Ross/rusty-trucks.git");
    let repo = fork.trim_end_matches(".git").trim_start_matches("https://github.com/");
    let branch = setting(&l.config, "launcher_branch").and_then(Value::as_str).unwrap_or("hails-additions");
    let base = format!("https://github.com/{repo}/releases/download/{branch}");
    let side = l.here.join("SkateLauncher.new.exe.sha256");
    let get = |url: &str, out: &Path| Command::new("curl").args(["-fsSL", "--max-time", "900", "-o"]).arg(out).arg(url).status().is_ok_and(|s| s.success());
    let prebuilt = (|| {
        if !get(&format!("{base}/SkateLauncher.exe.source-sha256"), &side) {
            return false;
        }
        let fields: Vec<(String, String)> = std::fs::read_to_string(&side).unwrap_or_default().lines()
            .filter_map(|l| { let mut p = l.split_whitespace(); Some((p.next()?.to_owned(), p.next()?.to_ascii_lowercase())) }).collect();
        let want = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let text = std::fs::read_to_string(&cs).unwrap_or_default().replace('\r', "");
        if want("source") != sha256_hex(text.as_bytes()) {
            return false;
        }
        if !get(&format!("{base}/SkateLauncher.exe"), &new) {
            return false;
        }
        let mut bytes = Vec::new();
        let ok = std::fs::File::open(&new).and_then(|mut f| f.read_to_end(&mut bytes)).is_ok() && sha256_hex(&bytes) == want("exe");
        if !ok {
            let _ = std::fs::remove_file(&new);
        }
        ok
    })();
    let _ = std::fs::remove_file(&side);
    if !prebuilt {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        let csc = Path::new(&windir).join("Microsoft.NET").join("Framework64").join("v4.0.30319").join("csc.exe");
        let _ = Command::new(csc).args(["/nologo", "/target:winexe"]).arg(format!("/out:{}", new.display())).arg(&cs).output();
    }
    if !new.exists() {
        return false;
    }
    let moved = (|| -> std::io::Result<()> {
        if old.exists() {
            std::fs::remove_file(&old)?;
        }
        if exe.exists() {
            std::fs::rename(&exe, &old)?;
        }
        std::fs::rename(&new, &exe)
    })();
    if moved.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    moved.is_ok()
}

fn remove_stale_locks(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_dir() {
            remove_stale_locks(&p);
        } else if p.extension().is_some_and(|e| e == "lock")
            && entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e.as_secs() > 120)) {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// `Self-Update`: one line for the result; None = nothing to say (already at the newest commit, not forced).
pub fn self_update(l: &Launcher, force: bool) -> Option<String> {
    let branch = setting(&l.config, "launcher_branch").and_then(Value::as_str).unwrap_or("hails-additions").to_owned();
    let url = setting(&l.config, "fork_url").and_then(Value::as_str).unwrap_or("https://github.com/Hailey-Ross/rusty-trucks.git").to_owned();
    let self_git = l.state_dir.join("self.git");
    let commit_file = l.state_dir.join("launcher_commit.txt");
    println!("  Checking the launcher: {branch} on {url}...");
    if !self_git.join("HEAD").exists() {
        let _ = Command::new("git").args(["init", "-q", "--bare"]).arg(&self_git).output();
    }
    remove_stale_locks(&self_git);
    ui_line("LAUNCHER_STAGE\tChecking GitHub for a newer launcher");
    ui_line("LAUNCHER_SELF\tfetching");
    let ok = Command::new("git").arg("-C").arg(&self_git)
        .args(["-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=30", "fetch", "-q", "--depth", "1", &url, &format!("+refs/heads/{branch}:refs/heads/self")])
        .env("GIT_TERMINAL_PROMPT", "0").output().is_ok_and(|o| o.status.success());
    ui_line("LAUNCHER_SELF\tfetched");
    let sha = Command::new("git").arg("-C").arg(&self_git).args(["rev-parse", "--verify", "-q", "refs/heads/self"]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    if !ok || sha.is_empty() {
        let r = format!("Launcher update: could not reach {url} ({branch}); kept the current launcher.");
        log(l, &r);
        return Some(r);
    }
    if !force && std::fs::read_to_string(&commit_file).is_ok_and(|t| t.trim() == sha) {
        return None;
    }
    ui_line(&format!("LAUNCHER_STAGE\tInstalling the launcher update ({})", short(&sha)));
    let zip_path = std::env::temp_dir().join(format!("skate-launcher-update-{}.zip", std::process::id()));
    let autocrlf = if cfg!(windows) { "core.autocrlf=true" } else { "core.autocrlf=false" };
    let archived = Command::new("git").arg("-C").arg(&self_git).args(["-c", autocrlf, "archive", "--format=zip", "-o"]).arg(&zip_path)
        .arg("refs/heads/self").arg("--").args(PATHS).output().is_ok_and(|o| o.status.success());
    let archive = std::fs::File::open(&zip_path).ok().filter(|_| archived).and_then(|f| zip::ZipArchive::new(f).ok());
    let Some(mut archive) = archive else {
        let _ = std::fs::remove_file(&zip_path);
        let r = format!("Launcher update: could not unpack {}; kept the current launcher.", short(&sha));
        log(l, &r);
        return Some(r);
    };
    let stamp = now("%Y%m%d_%H%M%S");
    let (mut changed, mut backed_up) = (Vec::<String>::new(), Vec::<String>::new());
    let mut failure = None;
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let Some(p) = PATHS.iter().find(|p| name.starts_with(&format!("{p}/"))) else { continue };
        let rel = &name[p.len() + 1..];
        let dest = if *p == "tools/steam-launcher" { l.here.clone() } else { l.repo.join(p) };
        let target = rel.split('/').fold(dest, |d, part| d.join(part));
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        if let Ok(old) = std::fs::read(&target) {
            if String::from_utf8_lossy(&old).replace('\r', "") == String::from_utf8_lossy(&bytes).replace('\r', "") {
                continue;
            }
            let backup = rel.split('/').fold(l.state_dir.join("self-update-backup").join(&stamp).join(p), |d, part| d.join(part));
            let _ = std::fs::create_dir_all(backup.parent().unwrap_or(&l.state_dir));
            let _ = std::fs::copy(&target, &backup);
            backed_up.push(format!("{p}/{rel}"));
        }
        let _ = std::fs::create_dir_all(target.parent().unwrap_or(&l.here));
        if let Err(e) = std::fs::write(&target, &bytes) {
            failure = Some(e.to_string());
            break;
        }
        changed.push(format!("{p}/{rel}"));
    }
    drop(archive);
    let _ = std::fs::remove_file(&zip_path);
    if let Some(e) = failure {
        let r = format!("Launcher update: failed while copying ({e}); kept what was not copied yet.");
        log(l, &r);
        return Some(r);
    }
    let _ = std::fs::write(&commit_file, format!("{sha}\r\n"));
    let s = short(&sha);
    if changed.is_empty() {
        let r = format!("Launcher: up to date at {s}.");
        log(l, &r);
        return Some(r);
    }
    log(l, &format!("Updated to {s} from {url} {branch}: {}", changed.join(", ")));
    if !backed_up.is_empty() {
        log(l, &format!("Local versions backed up to self-update-backup\\{stamp}: {}", backed_up.join(", ")));
    }
    let mut r = format!("Launcher updated to {s} ({} files).", changed.len());
    if !backed_up.is_empty() {
        r.push_str(&format!(" Local changes to {} of them were backed up (self-update-backup\\{stamp}).", backed_up.len()));
    }
    if changed.iter().any(|c| c == "tools/steam-launcher/SkateLauncher.cs") && cfg!(windows) {
        if rebuild_launcher_exe(l) {
            let _ = std::fs::write(l.state_dir.join("restart.txt"), format!("{s}\r\n"));
            r.push_str(" The launcher restarts into the new version.");
            log(l, &format!("Rebuilt SkateLauncher.exe at {s}."));
        } else {
            r.push_str(" Could not rebuild SkateLauncher.exe: the old window stays until build.bat runs.");
            log(l, "SkateLauncher.exe rebuild failed; old exe kept.");
        }
    }
    Some(r)
}
