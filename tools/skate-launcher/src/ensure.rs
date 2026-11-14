//! Getting a version ready to play, as `launcher.ps1` Ensure-Version does (doc 44a 1d): a worktree (or a clone when
//! standalone) that follows its pushed branch fast-forward only, the shared assets link, a build when the commit (or a
//! dev version's working tree) changed, or a prebuilt release download. Lines go to stdout as the PowerShell launcher
//! writes them; with `SKATE_LAUNCHER_UI=2` the `LAUNCHER_WAIT` / `LAUNCHER_BUILD` markers the window reads are added.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::Digest;

use crate::builds::{folder_bytes, target_dir};
use crate::cli::GAME_EXE;
use crate::state::{flag, setting, text, Launcher};

fn ui() -> bool {
    std::env::var("SKATE_LAUNCHER_UI").is_ok_and(|v| v == "2")
}

fn git(dir: &Path, args: &[&str]) -> (bool, String) {
    match crate::proc::cmd("git").arg("-C").arg(dir).args(args).output() {
        Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stdout).trim().to_owned()),
        Err(_) => (false, String::new()),
    }
}

fn now_local(fmt: &str) -> String {
    jiff::Zoned::now().strftime(fmt).to_string()
}

/// `Version-Source`: where the branch is fetched from and the ref it lands in.
fn source(l: &Launcher, v: &Value) -> (String, String) {
    let url = text(v, "url");
    let branch = text(v, "branch");
    if !url.is_empty() {
        return (url, format!("refs/remotes/launcher-{}/{branch}", text(v, "id")));
    }
    let remote = setting(&l.config, "remote").and_then(Value::as_str).unwrap_or("origin").to_owned();
    let r = format!("refs/remotes/{remote}/{branch}");
    (remote, r)
}

/// `Fetch-Version`: fetches the branch without a prompt and with a stall limit; returns the ref to follow.
fn fetch(l: &Launcher, v: &Value, dir: &Path) -> String {
    let (from, r) = source(l, v);
    let branch = text(v, "branch");
    println!("  Checking {branch} on {from}...");
    if ui() {
        println!("LAUNCHER_WAIT\tbegin\tfetch\t{branch}\t{from}");
    }
    let ok = crate::proc::cmd("git").arg("-C").arg(dir)
        .args(["-c", "http.lowSpeedLimit=1000", "-c", "http.lowSpeedTime=30", "fetch", "-q", &from, &format!("+refs/heads/{branch}:{r}")])
        .env("GIT_TERMINAL_PROMPT", "0").status().ok().and_then(|s| s.code()).unwrap_or(1);
    if ui() {
        println!("LAUNCHER_WAIT\tend\tfetch\t{ok}");
    }
    if ok != 0 {
        println!("  Could not reach {from} (git fetch failed); using the last fetched {branch}.");
    }
    r
}

/// The current installation's assets folder when it holds a converted game.
fn installed_assets(l: &Launcher) -> Option<PathBuf> {
    let a = crate::status::current_assets(l);
    a.join("private").join("game.json").is_file().then_some(a)
}

/// `Repair-AssetLink`: points a checkout's assets link at the current installation when it no longer resolves (a
/// real folder is never touched). A junction on Windows, a symbolic link elsewhere.
pub fn repair_asset_link(l: &Launcher, dir: &Path, name: &str) -> Option<String> {
    let link = dir.join("assets");
    let meta = std::fs::symlink_metadata(&link).ok();
    let is_link = meta.as_ref().is_some_and(|m| m.file_type().is_symlink() || is_junction(&link));
    if meta.is_some() && !is_link {
        return None;
    }
    if meta.is_some() && link.join("private").join("game.json").is_file() {
        return None;
    }
    let target = installed_assets(l)?;
    if meta.is_some() {
        remove_link(&link);
    } else if dir == l.repo {
        return None;
    }
    make_link(&link, &target).ok()?;
    Some(format!("Assets link of {name} pointed at the current installation."))
}

#[cfg(windows)]
fn is_junction(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & 0x400 != 0)
}
#[cfg(not(windows))]
fn is_junction(_: &Path) -> bool {
    false
}

#[cfg(windows)]
fn remove_link(p: &Path) {
    let _ = std::fs::remove_dir(p);
}
#[cfg(not(windows))]
fn remove_link(p: &Path) {
    let _ = std::fs::remove_file(p);
}

#[cfg(windows)]
fn make_link(link: &Path, target: &Path) -> std::io::Result<()> {
    let ok = crate::proc::cmd("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output()?.status.success();
    if ok { Ok(()) } else { Err(std::io::Error::other("mklink failed")) }
}
#[cfg(not(windows))]
fn make_link(link: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// `Repair-AllAssetLinks`.
pub fn repair_all_asset_links(l: &Launcher) -> Vec<String> {
    let mut lines: Vec<String> = repair_asset_link(l, &l.repo, "this checkout").into_iter().collect();
    for v in &l.versions {
        let d = l.version_dir(v);
        if d != l.repo && d.join(".git").exists() {
            lines.extend(repair_asset_link(l, &d, &text(v, "name")));
        }
    }
    lines
}

/// The release package's name for this OS.
fn package_target() -> &'static str {
    if cfg!(windows) { "windows-x64" } else if cfg!(target_os = "macos") { "macos-arm64" } else { "linux-x64" }
}

fn download(url: &str, out: &Path) -> Result<(), String> {
    let status = crate::proc::cmd("curl").args(["-fsSL", "--retry", "2", "--max-time", "900", "-o"]).arg(out).arg(url).status()
        .map_err(|e| format!("curl: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("download failed ({url})")) }
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The release package's file type: the Windows package is a zip, the Linux and macOS ones are tar.gz (doc 44).
fn package_ext() -> &'static str {
    if cfg!(windows) { "zip" } else { "tar.gz" }
}

/// The game in a release folder: at the root in the Windows package, in bin/ in the Linux and macOS packages (laid
/// out like a checkout).
pub fn release_exe(d: &Path) -> PathBuf {
    let root = d.join(GAME_EXE);
    if cfg!(windows) || root.is_file() { root } else { d.join("bin").join(GAME_EXE) }
}

/// `Write-ReleasePlay`: the release folder's play script (the game with its session log in logs/).
fn write_release_play(dir: &Path) {
    if cfg!(windows) {
        let bat = ["@echo off", "cd /d \"%~dp0\"", "if not exist \"logs\" mkdir \"logs\"",
            "for /f %%t in ('powershell -NoProfile -Command \"Get-Date -Format yyyyMMdd-HHmmss\"') do set \"STAMP=%%t\"",
            "\"%~dp0skate3rust.exe\" > \"logs\\game-%STAMP%.log\" 2> \"logs\\game-%STAMP%.stderr.log\""].join("\r\n");
        let _ = std::fs::write(dir.join("PLAY.bat"), bat + "\r\n");
    } else {
        let exe = if dir.join(GAME_EXE).is_file() { format!("./{GAME_EXE}") } else { format!("./bin/{GAME_EXE}") };
        let sh = format!("#!/bin/sh\ncd \"$(dirname \"$0\")\"\nmkdir -p logs\nstamp=$(date +%Y%m%d-%H%M%S)\n{exe} > \"logs/game-$stamp.log\" 2> \"logs/game-$stamp.stderr.log\"\n");
        let path = dir.join("PLAY.sh");
        let _ = std::fs::write(&path, sh);
        set_executable(&path);
    }
}

/// The release's metadata for this OS: `<pkg>.release.json` (every OS), else the Windows-era `release.json`.
fn release_meta(base: &str, pkg: &str, tmp: &Path) -> Option<Value> {
    let p = tmp.join("release.json");
    [format!("{base}/{pkg}.release.json"), format!("{base}/release.json")].iter()
        .find(|url| download(url, &p).is_ok()).and_then(|_| read_json(&p))
}

/// Unpacks a downloaded package into `x` (zip with the zip crate; tar.gz with the system tar, present on Linux and
/// macOS).
fn unpack(file: &Path, x: &Path) -> Result<(), String> {
    std::fs::create_dir_all(x).map_err(|e| e.to_string())?;
    if package_ext() == "zip" {
        let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        zip::ZipArchive::new(f).and_then(|mut a| a.extract(x)).map_err(|e| e.to_string())
    } else {
        let ok = crate::proc::cmd("tar").arg("-xzf").arg(file).arg("-C").arg(x).status().map_err(|e| format!("tar: {e}"))?;
        if ok.success() { Ok(()) } else { Err("tar could not unpack it".into()) }
    }
}

/// `skate-launcher release-patch <release folder> <release base url> <package name> <revision>`: brings the release
/// folder to `revision` with the release's patches (the classic PowerShell launcher calls this). Exit 0 = updated,
/// 3 = no usable patch (take the full download).
pub fn cli_release_patch(args: &[String]) -> std::process::ExitCode {
    let [d, base, pkg, revision, ..] = args else {
        eprintln!("usage: release-patch <release folder> <release base url> <package name> <revision>");
        return std::process::ExitCode::from(2);
    };
    let d = Path::new(d);
    let tmp = std::env::temp_dir().join(format!("skate-release-patch-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let done = read_json(&d.join("release.json"))
        .is_some_and(|cur| try_patches(base, pkg, d, &tmp, &cur, &serde_json::json!({"revision": revision})));
    let _ = std::fs::remove_dir_all(&tmp);
    if done {
        set_executable(&release_exe(d));
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(3)
    }
}

#[cfg(unix)]
fn set_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(p) {
        let mut perm = m.permissions();
        perm.set_mode(perm.mode() | 0o111);
        let _ = std::fs::set_permissions(p, perm);
    }
}
#[cfg(not(unix))]
fn set_executable(_: &Path) {}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(std::fs::read_to_string(p).ok()?.trim_start_matches('\u{feff}')).ok()
}

/// Copies `src` over `dst` (files replaced, others kept), leaving out `skip` at the top level.
fn copy_tree(src: &Path, dst: &Path, skip: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == skip {
            continue;
        }
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to, "")?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Patch updates (patch.rs): when the release lists a chain of patches from the installed revision that is smaller
/// than the package, download and apply them link by link. False (nothing changed, or put back) = take the full
/// download.
fn try_patches(base: &str, pkg: &str, d: &Path, tmp: &Path, cur: &Value, meta: &Value) -> bool {
    let index_path = tmp.join("patches.json");
    if download(&format!("{base}/{pkg}.patches.json"), &index_path).is_err() {
        return false;
    }
    let Some(index) = read_json(&index_path) else { return false };
    let Some((links, bytes)) = crate::patch::chain(&index, &text(cur, "revision"), &text(meta, "revision")) else { return false };
    println!("  Updating with {} patch{} ({:.1} MB instead of the full download)...", links.len(),
        if links.len() == 1 { "" } else { "es" }, bytes as f64 / 1_048_576.0);
    for (i, link) in links.iter().enumerate() {
        let file = text(link, "file");
        let path = tmp.join(&file);
        if file.contains(['/', '\\']) || download(&format!("{base}/{file}"), &path).is_err() {
            println!("  The patch download failed; downloading the full build instead.");
            return false;
        }
        if crate::patch::sha256_file(&path).ok().as_deref() != Some(text(link, "sha256").as_str()) {
            println!("  A patch does not match its checksum; downloading the full build instead.");
            return false;
        }
        let staged = crate::patch::stage(d, &path, &tmp.join(format!("stage-{i}")))
            .and_then(|s| crate::patch::commit(d, &s));
        if let Err(e) = staged {
            println!("  The patch could not be applied ({e}); downloading the full build instead.");
            return false;
        }
    }
    true
}

/// `Ensure-Release`: downloads the release when it moved (checksum checked) and installs it over the old build.
fn ensure_release(l: &Launcher, v: &Value) -> bool {
    let d = l.version_dir(v);
    let have = release_exe(&d).is_file();
    let url = { let u = text(v, "url"); if u.is_empty() { setting(&l.config, "fork_url").and_then(Value::as_str).unwrap_or("https://github.com/Hailey-Ross/rusty-trucks.git").to_owned() } else { u } };
    let repo = url.trim_end_matches(".git").trim_start_matches("https://github.com/").to_owned();
    let base = format!("https://github.com/{repo}/releases/download/{}", text(v, "release"));
    let tmp = std::env::temp_dir().join(format!("skate-release-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let result = (|| {
        println!("  Checking the {} release...", text(v, "release"));
        let pkg = format!("skate3rust-{}", package_target());
        let Some(meta) = release_meta(&base, &pkg, &tmp) else {
            if have {
                println!("  Could not reach the release; playing the installed build.");
                return true;
            }
            println!("  Could not download {} (the release is not reachable).", text(v, "name"));
            return false;
        };
        let cur = read_json(&d.join("release.json"));
        if have && cur.as_ref().is_some_and(|c| text(c, "revision") == text(&meta, "revision")) {
            write_release_play(&d);
            return true;
        }
        if have {
            if let Some(c) = cur.as_ref() {
                if try_patches(&base, &pkg, &d, &tmp, c, &meta) {
                    set_executable(&release_exe(&d));
                    write_release_play(&d);
                    println!("  Installed {} build {}.", text(v, "name"), text(&meta, "build"));
                    return true;
                }
            }
        }
        println!("  Downloading {} build {}...", text(v, "name"), text(&meta, "build"));
        let file = format!("{pkg}.{}", package_ext());
        let package = tmp.join(&file);
        let sum = tmp.join(format!("{file}.sha256"));
        if let Err(e) = download(&format!("{base}/{file}"), &package).and_then(|_| download(&format!("{base}/{file}.sha256"), &sum)) {
            println!("  The download failed ({e}).");
            return have;
        }
        let want = std::fs::read_to_string(&sum).unwrap_or_default().split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
        if sha256(&package).ok().as_deref() != Some(want.as_str()) {
            println!("  The download does not match its checksum; not installed.");
            return have;
        }
        let x = tmp.join("x");
        if let Err(e) = unpack(&package, &x) {
            println!("  Could not unpack the download ({e}).");
            return have;
        }
        let src = if x.join(&pkg).is_dir() { x.join(&pkg) } else { x.clone() };
        if copy_tree(&src, &d, "release.json").is_err() {
            println!("  Could not copy the new build into place (is the game still running?).");
            return have;
        }
        // a package without its own release.json (an older Linux / macOS one) keeps the release's
        if std::fs::copy(src.join("release.json"), d.join("release.json")).is_err() {
            let _ = std::fs::write(d.join("release.json"), meta.to_string());
        }
        set_executable(&release_exe(&d));
        write_release_play(&d);
        println!("  Installed {} build {}.", text(v, "name"), text(&meta, "build"));
        true
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn processes() -> sysinfo::System {
    let mut s = sysinfo::System::new();
    s.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, sysinfo::ProcessRefreshKind::everything());
    s
}

/// `Build-Holder`: who else is building this version ("" = nobody).
fn build_holder(l: &Launcher, v: &Value, target: &Path) -> String {
    let mark = l.state_dir.join(format!("BUILDING-{}.txt", text(v, "id")));
    let sys = processes();
    if let Ok(t) = std::fs::read_to_string(&mark) {
        let f: Vec<&str> = t.trim().split('\t').collect();
        let alive = f.first().and_then(|p| p.parse::<usize>().ok())
            .is_some_and(|pid| pid != std::process::id() as usize && sys.process(sysinfo::Pid::from(pid)).is_some());
        if f.len() >= 4 && alive {
            let since = f[2].get(11..16).unwrap_or(f[2]);
            return format!("{} is being built by the {} since {since} (pid {})", f[1], f[3], f[0]);
        }
        let _ = std::fs::remove_file(&mark);
    }
    let needle = target.to_string_lossy().to_lowercase();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_lowercase();
        if name == "cargo" || name == "cargo.exe" {
            let cmd: String = p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ").to_lowercase();
            if cmd.contains(&needle) {
                return format!("another cargo is using this build folder (pid {pid})");
            }
        }
    }
    String::new()
}

/// `Make-Room`: keeps the build folders under the size cap, deleting the oldest-built versions' build caches.
pub fn make_room(l: &Launcher, v: &Value) {
    let num = |k: &str, d: f64| setting(&l.config, k).and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.parse().ok()))).unwrap_or(d);
    let cap = num("build_cap_gb", 0.0);
    if cap <= 0.0 {
        return;
    }
    let reserve = num("build_reserve_gb", 30.0);
    let cap_dir = setting(&l.config, "build_cap_dir").and_then(Value::as_str).map(PathBuf::from);
    let mine = target_dir(l, v);
    let gb = 1024.0 * 1024.0 * 1024.0;
    let total = || -> f64 {
        match &cap_dir {
            Some(d) => folder_bytes(d) as f64 / gb,
            None => {
                let mut dirs: Vec<PathBuf> = l.versions.iter().map(|x| target_dir(l, x)).collect();
                dirs.sort();
                dirs.dedup();
                dirs.iter().map(|d| folder_bytes(d) as f64).sum::<f64>() / gb
            }
        }
    };
    println!("  Checking the build folders' size...");
    let mut used = total();
    println!("  Build folders: {used:.1} GB of {cap:.0} GB (a build needs about {reserve:.0} GB).");
    let mut others: Vec<(&Value, PathBuf, std::time::SystemTime)> = l.versions.iter()
        .filter(|o| text(o, "id") != text(v, "id"))
        .map(|o| (o, target_dir(l, o)))
        .filter(|(_, d)| d.exists() && *d != mine)
        .map(|(o, d)| {
            let built = std::fs::metadata(l.version_dir(o).join("bin").join(GAME_EXE)).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (o, d, built)
        }).collect();
    others.sort_by_key(|(_, _, t)| *t);
    for (o, dir, _) in others {
        if used + reserve <= cap {
            break;
        }
        println!("  Over the cap: deleting the build cache of {} ({:.1} GB, {})...", text(o, "name"), folder_bytes(&dir) as f64 / gb, dir.display());
        let _ = std::fs::remove_dir_all(&dir);
        used = total();
    }
    if used + reserve > cap {
        println!("  Still {used:.1} GB of {cap:.0} GB: nothing more of the launcher's to delete; building anyway.");
    }
}

/// A dev version's build stamp: the commit plus a hash of the uncommitted changes to the exe's inputs.
fn head_stamp(d: &Path, dev: bool) -> String {
    let head = git(d, &["rev-parse", "--short", "HEAD"]).1;
    if !dev {
        return head;
    }
    let inputs = ["--", "crates", "vendor", "Cargo.toml", "Cargo.lock", ".cargo", "docs/images", "maps"];
    let changed = crate::proc::cmd("git").arg("-C").arg(d).args(["diff", "--quiet", "HEAD"]).args(inputs).status().ok().and_then(|s| s.code()) == Some(1);
    if !changed {
        return head;
    }
    let tmp = std::env::temp_dir().join(format!("skate-launcher-diff-{}", std::process::id()));
    let _ = crate::proc::cmd("git").arg("-C").arg(d).args(["diff", "--binary"]).arg(format!("--output={}", tmp.display())).arg("HEAD").args(inputs).status();
    let hash = git(d, &["hash-object", &tmp.to_string_lossy()]).1;
    let _ = std::fs::remove_file(&tmp);
    if hash.len() >= 12 { format!("{head}+{}", &hash[..12]) } else { head }
}

/// The window reads cargo's own "Building [..] n/total" bar: plain text, always on (as launcher.ps1 sets them).
fn cargo_env(c: &mut Command) -> &mut Command {
    c.env("SKATE_BUILD_STAMP", "inputs");
    if ui() {
        c.envs([("CARGO_TERM_COLOR", "never"), ("CARGO_TERM_PROGRESS_WHEN", "always"), ("CARGO_TERM_PROGRESS_WIDTH", "120"), ("CARGO_TERM_PROGRESS_TERM_INTEGRATION", "false")]);
    }
    c
}

/// The build itself: Build.ps1 on Windows (as the PowerShell launcher); elsewhere one static binary into bin/.
fn run_build(d: &Path, target: &Path) -> i32 {
    if cfg!(windows) {
        return cargo_env(crate::proc::cmd("powershell.exe").args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(d.join("scripts").join("Build.ps1"))
            .arg("-TargetDirectory").arg(target)).status().ok().and_then(|s| s.code()).unwrap_or(1);
    }
    let code = cargo_env(crate::proc::cmd("cargo").current_dir(d)
        .args(["build", "--release", "--locked", "-p", "skate-game", "--bin", "skate3rust", "--no-default-features", "--target-dir"]).arg(target))
        .status().ok().and_then(|s| s.code()).unwrap_or(1);
    if code != 0 {
        return code;
    }
    let bin = d.join("bin");
    let _ = std::fs::create_dir_all(&bin);
    if std::fs::copy(target.join("release").join(GAME_EXE), bin.join(GAME_EXE)).is_err() {
        return 1;
    }
    set_executable(&bin.join(GAME_EXE));
    let lib = if cfg!(target_os = "macos") { "librefpack.dylib" } else { "librefpack.so" };
    let native = d.join("target").join("native");
    let _ = std::fs::create_dir_all(&native);
    let _ = crate::proc::cmd("rustc").args(["--edition", "2024", "--crate-type", "cdylib", "-C", "opt-level=3", "-C", "panic=abort"])
        .arg(d.join("tools").join("asset_pipeline").join("refpack_native.rs")).arg("-o").arg(native.join(lib)).status();
    0
}

/// `Ensure-Version`: true = ready to play. `build_log` gets the build's log path.
pub fn ensure_version(l: &Launcher, v: &Value, build_log: &mut String) -> bool {
    if l.use_release(v) {
        return ensure_release(l, v);
    }
    let d = l.version_dir(v);
    let name = text(v, "name");
    let branch = text(v, "branch");
    if !d.join(".git").exists() {
        if let Some(parent) = d.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if l.standalone {
            let url = { let u = text(v, "url"); if u.is_empty() { setting(&l.config, "fork_url").and_then(Value::as_str).unwrap_or("https://github.com/Hailey-Ross/rusty-trucks.git").to_owned() } else { u } };
            println!("  Setting up {name}: downloading {branch} from {url}...");
            let ok = crate::proc::cmd("git").args(["clone", "-q", "--single-branch", "--branch", &branch, &url]).arg(&d).status().is_ok_and(|s| s.success());
            if !ok {
                println!("  Could not download it (git clone failed).");
                return false;
            }
        }
    }
    if !d.join(".git").exists() {
        println!("  Setting up {name}: new worktree for {branch}...");
        let r = fetch(l, v, &l.repo);
        let mut c = crate::proc::cmd("git");
        c.arg("-C").arg(&l.repo).args(["worktree", "add"]);
        if flag(v, "detached") || !text(v, "url").is_empty() { c.arg("--detach").arg(&d).arg(&r); } else { c.arg(&d).arg(&branch); }
        if !c.status().is_ok_and(|s| s.success()) {
            println!("  Could not create the worktree.");
            return false;
        }
    }
    if let Some(fixed) = repair_asset_link(l, &d, &name) {
        println!("  {fixed}");
    }
    let dev = flag(v, "dev");
    let dirty = !git(&d, &["status", "--porcelain", "--untracked-files=no"]).1.is_empty();
    if !dev && dirty && !branch.is_empty() {
        println!("  NOT UPDATED: {name} has local changes, so it stays at its current commit.");
    }
    if !dev && !dirty && !branch.is_empty() {
        let r = fetch(l, v, &d);
        let behind: u64 = git(&d, &["rev-list", "--count", &format!("HEAD..{r}")]).1.parse().unwrap_or(0);
        if behind > 0 {
            println!("  Updating {branch} ({behind} new commits)...");
            let _ = git(&d, &["merge", "-q", "--ff-only", &r]);
        }
    }
    let exe = d.join("bin").join(GAME_EXE);
    let marker = d.join("bin").join("built_commit.txt");
    let head = head_stamp(&d, dev);
    let built = std::fs::read_to_string(&marker).map(|s| s.trim().to_owned()).unwrap_or_default();
    if exe.is_file() && built == head {
        return true;
    }
    println!("  Building {name} at {head}. The first build of a version takes several minutes...");
    let target = target_dir(l, v);
    make_room(l, v);
    let log_dir = d.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let id = text(v, "id");
    *build_log = log_dir.join(format!("launcher-build-{id}-{}.log", now_local("%Y%m%d_%H%M%S"))).display().to_string();
    let prefix = format!("launcher-build-{id}-");
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&log_dir).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix) && e.file_name().to_string_lossy().ends_with(".log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in logs.into_iter().skip(9) {
        let _ = std::fs::remove_file(old);
    }
    if ui() {
        println!("LAUNCHER_BUILD\tbegin\t{name}\t{head}\t{build_log}");
    }
    let holder = build_holder(l, v, &target);
    if !holder.is_empty() {
        println!("  Waiting: {holder}.");
        if ui() {
            println!("LAUNCHER_BUILD\twait\tbuild\t{holder}");
        }
    }
    let exe_held = || processes().processes().values().any(|p| p.exe().is_some_and(|e| e == exe));
    if exe_held() {
        let refreshing = std::fs::read_to_string(l.state_dir.join("REFRESHING.txt")).is_ok_and(|t| t.trim() == name);
        let what = if refreshing { format!("the game files refresh of {name} is using its exe") } else { format!("{name} is running from its build folder") };
        println!("  Waiting: {what}.");
        if ui() {
            println!("LAUNCHER_BUILD\twait\tbuild\t{what}\thold");
        }
        while exe_held() {
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        if ui() {
            println!("LAUNCHER_BUILD\twaitend");
        }
    }
    let mark = l.state_dir.join(format!("BUILDING-{id}.txt"));
    let who = if ui() { "window" } else { "shell" };
    let _ = std::fs::write(&mark, format!("{}\t{name}\t{}\t{who}\r\n", std::process::id(), now_local("%Y-%m-%dT%H:%M:%S")));
    let code = run_build(&d, &target);
    if std::fs::read_to_string(&mark).is_ok_and(|t| t.split('\t').next() == Some(&std::process::id().to_string())) {
        let _ = std::fs::remove_file(&mark);
    }
    if ui() {
        println!("LAUNCHER_BUILD\tend\t{code}");
    }
    if code != 0 || !exe.is_file() {
        println!("  Build failed. Build log: {build_log}");
        return false;
    }
    let _ = std::fs::write(&marker, format!("{head}\r\n"));
    true
}
