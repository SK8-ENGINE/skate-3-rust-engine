//! `status`: the main screen's facts, as `launcher.ps1 status` prints them (doc 44a section 1a).
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::cli::version_status;
use crate::state::{text, Launcher};

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim_start_matches('\u{feff}').to_owned())
}

fn git_line(dir: &Path, args: &[&str]) -> String {
    Command::new("git").arg("-C").arg(dir).args(args).output().ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(str::to_owned))
        .unwrap_or_default()
}

/// `Resolve-Local`.
fn resolve_local(l: &Launcher, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() { path } else { l.repo.join(p) }
}

/// `Send-Script` / `Send-Blocked`: why sending logs can't run ("" = it can), or None when no send script is named.
fn send(l: &Launcher) -> Option<String> {
    let script = l.config.get("after_session").and_then(Value::as_str).filter(|s| !s.is_empty())?;
    let path = resolve_local(l, script);
    if !path.exists() {
        return Some(format!("Send script not found: {}", path.display()));
    }
    let beside = path.parent().map(|d| d.join("send-sessions.txt"));
    if !beside.is_some_and(|f| f.exists()) && !l.repo.join("send-sessions.txt").exists() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Some(format!("Not set up: run {name} -Setup once."));
    }
    Some(String::new())
}

/// `Share-Status`.
pub fn share(l: &Launcher, root: &Path) -> String {
    let script = [l.here.join("share-session-issue.ps1"), l.repo.join("tools").join("second-pc").join("share-session-issue.ps1")];
    if !script.iter().any(|s| s.exists()) {
        return String::new();
    }
    let mut mode = String::new();
    let mut consent = String::new();
    let mut repo = String::new();
    for line in read_text(&l.repo.join("share-sessions.txt")).unwrap_or_default().lines() {
        let line = line.trim_start();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "mode" => mode = v.trim().into(),
                "consent" => consent = v.trim().into(),
                "repo" => repo = v.trim().into(),
                _ => {}
            }
        }
    }
    if mode != "github" || consent != "1" {
        return "off".into();
    }
    if repo.is_empty() {
        let url = git_line(root, &["remote", "get-url", "origin"]);
        if let Some(rest) = url.split("github.com").nth(1) {
            let parts: Vec<&str> = rest.trim_start_matches([':', '/']).trim_end_matches('/').splitn(2, '/').collect();
            if parts.len() == 2 {
                repo = format!("{}/{}", parts[0], parts[1].trim_end_matches(".git"));
            }
        }
    }
    if repo.is_empty() { "no repo".into() } else { format!("on {repo}") }
}

/// `Current-Assets`: the installation's assets (data/installation.json), else the checkout's assets link.
pub fn current_assets(l: &Launcher) -> PathBuf {
    let mut bases = vec![l.repo.join("data")];
    if l.standalone {
        bases.extend(l.versions.iter().map(|v| l.version_dir(v).join("data")));
    }
    for base in &bases {
        let marker = read_text(&base.join("installation.json")).and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(dir) = marker.as_ref().map(|m| text(m, "directory")).filter(|d| !d.is_empty()) {
            let assets = base.join(dir).join("assets");
            if assets.exists() {
                return assets;
            }
        }
    }
    let link = l.repo.join("assets");
    std::fs::read_link(&link).unwrap_or(link)
}

/// `Fixed-Executor`: the game's physics thread setting.
fn fixed_executor(l: &Launcher) -> &'static str {
    let settings = current_assets(l).parent().map(|p| p.join("settings").join("graphics.json"));
    let multi = settings.and_then(|p| read_text(&p)).and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("physics_threads_multi").and_then(Value::as_bool)).unwrap_or(false);
    if multi { "multi" } else { "single" }
}

/// `Send-Blocked` as the `tool-send` entry shows it.
pub fn send_blocked(l: &Launcher) -> String {
    send(l).unwrap_or_else(|| "Not set up: name a send script as \"after_session\" in config.json (see README.md).".into())
}

/// `Fixed-Executor` for the play environment (SKATE_FIXED_EXECUTOR).
pub fn fixed_executor_pub(l: &Launcher) -> &'static str {
    fixed_executor(l)
}

/// `Setup-Missing`: the required tools the first-time setup check reports missing, comma-separated. `release`: the
/// selected version is a prebuilt release, so the build tools are optional (`SKATE_LAUNCHER_RELEASES=1`).
pub fn setup_missing(l: &Launcher, release: bool) -> String {
    let mut command = if cfg!(windows) {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(l.here.join("setup.ps1"));
        c
    } else {
        let mut c = Command::new("sh");
        c.arg(l.here.join("setup.sh"));
        c
    };
    if release {
        command.env("SKATE_LAUNCHER_RELEASES", "1");
    }
    let output = command.output();
    let Ok(output) = output else { return String::new() };
    String::from_utf8_lossy(&output.stdout).lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() == 5 && f[2] == "0" && f[4] == "1").then(|| f[1].to_owned())
        })
        .collect::<Vec<_>>().join(", ")
}

fn short(sha: &str) -> String {
    sha.get(..8).unwrap_or(sha).to_owned()
}

/// `status`.
pub fn status(l: &Launcher) -> Vec<String> {
    let v = l.selected();
    let last = read_text(&l.state_dir.join("last_session.txt"))
        .map_or_else(|| "No session yet.".to_owned(), |t| t.lines().collect::<Vec<_>>().join(" / "));
    let send = match send(l) {
        None => "off".to_owned(),
        Some(blocked) if !blocked.is_empty() => blocked,
        Some(_) => "on".into(),
    };
    let first_setup = crate::state::setting(&l.config, "first_setup_done").and_then(Value::as_bool).unwrap_or(false);
    let launcher_commit = read_text(&l.state_dir.join("launcher_commit.txt")).map(|s| s.trim().to_owned())
        .unwrap_or_else(|| if l.standalone { String::new() } else { git_line(&l.repo, &["rev-parse", "HEAD"]) });
    let self_git = l.state_dir.join("self.git");
    let fork_commit = if self_git.join("HEAD").exists() { git_line(&self_git, &["rev-parse", "--verify", "-q", "refs/heads/self"]) } else { String::new() };
    let newer = if !fork_commit.is_empty() && !launcher_commit.is_empty() && fork_commit != launcher_commit { short(&fork_commit) } else { String::new() };
    vec![
        format!("version\t{}", text(v, "name")),
        format!("version_status\t{}", version_status(l, v)),
        format!("last\t{last}"),
        format!("send\t{send}"),
        format!("share\t{}", share(l, &l.version_dir(v))),
        format!("fixed_executor\t{}", fixed_executor(l)),
        format!("setup_missing\t{}", setup_missing(l, l.use_release(v))),
        format!("first_setup\t{}", if first_setup { "" } else { "1" }),
        format!("launcher\t{}", short(&launcher_commit)),
        format!("launcher_newer\t{newer}"),
    ]
}
