//! Patch updates for the release download (doc 44a "Patch updates"): an install whose revision is a direct ancestor
//! of the release downloads one small patch per release in between instead of the whole package.
//!
//! A patch (format `skate-patch/1`) is a zip, stored, holding `manifest.json`, the new `release.json` and one payload
//! per changed file: a zstd frame of the new file made against the old file as its reference ("patch from"), or of
//! the new file alone for a new file. The release's `skate3rust-<target>.patches.json` lists the links (from / to
//! revision, file, sha256, size) and the full package's size. Applying checks every installed file against its old
//! sha256, builds the new files in a staging folder, checks every one against the new `release.json`, and only then
//! swaps them in (rolled back on any error). Anything that does not fit falls back to the full download.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::Digest;

pub const FORMAT: &str = "skate-patch/1";
pub const INDEX_FORMAT: &str = "skate-patches/1";
/// zstd level for payloads (made once in CI, applied everywhere).
const LEVEL: i32 = 19;
/// A chain of patches is used while it is smaller than this share of the full package.
const CHAIN_SHARE: f64 = 0.8;

pub fn sha256_bytes(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
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

/// Top-level folders that hold the player's data: a release never owns anything in them (as tools/release_meta.py).
const PROTECTED: &[&str] = &["data", "assets", "maps", "logs", "saves", "settings", "mods", "custom-models", "screenshots"];

/// A release-owned file name: relative, `/`-separated, no `..`, no drive or root, no Windows device name, nothing in
/// the player's data folders or a dot folder.
fn safe(name: &str) -> Result<&str, String> {
    let device = |p: &str| {
        let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
        matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit())
    };
    let first = name.split('/').next().unwrap_or("").to_ascii_lowercase();
    let bad = name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':')
        || name.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with([' ', '.']) || p.chars().any(char::is_control) || device(p))
        || PROTECTED.contains(&first.as_str()) || first.starts_with('.');
    if bad { Err(format!("unsafe file name in a patch: {name:?}")) } else { Ok(name) }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The `files` map of a release.json: name -> sha256.
fn files_of(meta: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = meta.get("files").and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_ascii_lowercase()))).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn read_meta(dir: &Path) -> Result<Value, String> {
    let p = dir.join("release.json");
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", p.display()))
}

/// The window a frame needs to reach back over its reference and itself (zstd windowLog, 10..=31).
fn window_log(len: usize) -> u32 {
    let mut w = 10;
    while w < 31 && (1usize << w) < len.max(1) {
        w += 1;
    }
    w
}

fn encode(new: &[u8], reference: Option<&[u8]>) -> Result<(Vec<u8>, u32), String> {
    let wlog = window_log(new.len().max(reference.map_or(0, <[u8]>::len)));
    let mut out = Vec::new();
    let mut enc = match reference {
        Some(r) => zstd::stream::write::Encoder::with_ref_prefix(&mut out, LEVEL, r),
        None => zstd::stream::write::Encoder::new(&mut out, LEVEL),
    }.map_err(|e| e.to_string())?;
    enc.long_distance_matching(true).map_err(|e| e.to_string())?;
    enc.window_log(wlog).map_err(|e| e.to_string())?;
    enc.set_pledged_src_size(Some(new.len() as u64)).map_err(|e| e.to_string())?;
    enc.write_all(new).map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())?;
    Ok((out, wlog))
}

fn decode(payload: &[u8], reference: Option<&[u8]>, wlog: u32) -> Result<Vec<u8>, String> {
    let mut dec = match reference {
        Some(r) => zstd::stream::read::Decoder::with_ref_prefix(std::io::BufReader::new(payload), r),
        None => zstd::stream::read::Decoder::with_buffer(std::io::BufReader::new(payload)),
    }.map_err(|e| e.to_string())?;
    dec.window_log_max(wlog.max(10)).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out).map_err(|e| format!("patch payload: {e}"))?;
    Ok(out)
}

/// The folder holding release.json in an extracted package (the root or its one sub folder).
pub fn package_root(dir: &Path) -> Option<PathBuf> {
    if dir.join("release.json").is_file() {
        return Some(dir.to_path_buf());
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.join("release.json").is_file())
}

/// A package given as a folder, a zip or a tar.gz: its root (an archive is extracted into `scratch`).
fn open_package(path: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
        if path.to_string_lossy().ends_with(".tar.gz") {
            let ok = crate::proc::cmd("tar").arg("-xzf").arg(path).arg("-C").arg(scratch).status().map_err(|e| format!("tar: {e}"))?;
            if !ok.success() {
                return Err(format!("{}: tar could not unpack it", path.display()));
            }
        } else {
            let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            zip::ZipArchive::new(f).and_then(|mut a| a.extract(scratch)).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        scratch.to_path_buf()
    };
    package_root(&dir).ok_or_else(|| format!("{}: no release.json", path.display()))
}

/// Files a launcher writes into a release folder itself (its play scripts): never release-owned.
const LAUNCHER_OWNED: &[&str] = &["release.json", "PLAY.sh", "PLAY.bat"];

/// Whether a file is executable (Unix mode bits; never on Windows).
#[cfg(unix)]
fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn is_exec(_: &Path) -> bool {
    false
}

#[cfg(unix)]
fn set_exec(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
fn set_exec(_: &Path) {}

/// The release.json for the package in `dir` (the Linux and macOS packages; Windows writes its own in
/// Build-Release.ps1): every file but the launcher's own and the player's data folders, by sha256.
pub fn manifest(dir: &Path, target: &str, build: u64, tag: &str, revision: &str, repository: &str) -> Result<Value, String> {
    fn walk(root: &Path, at: &Path, out: &mut serde_json::Map<String, Value>) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(at).map_err(|e| e.to_string())?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = p.strip_prefix(root).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                walk(root, &p, out)?;
            } else if !LAUNCHER_OWNED.contains(&name.as_str()) && safe(&name).is_ok() {
                out.insert(name, json!(sha256_file(&p)?));
            }
        }
        Ok(())
    }
    let mut files = serde_json::Map::new();
    walk(dir, dir, &mut files)?;
    Ok(json!({"schema": 1, "repository": repository, "target": target, "build": build, "tag": tag, "revision": revision, "files": files}))
}

/// `skate-launcher write-manifest <package folder> --target T --build N --revision SHA [--tag T]`: writes the
/// package's release.json (CI, Linux and macOS packages). The repository is $SKATE3RUST_REPOSITORY or
/// $GITHUB_REPOSITORY.
pub fn cli_manifest(args: &[String]) -> Result<(), String> {
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).map(String::as_str);
    let dir = Path::new(args.first().ok_or("write-manifest <package folder> ...")?);
    let target = get("--target").ok_or("--target missing")?;
    let build: u64 = get("--build").and_then(|s| s.parse().ok()).ok_or("--build missing")?;
    let revision = get("--revision").ok_or("--revision missing")?;
    let repository = std::env::var("SKATE3RUST_REPOSITORY").or_else(|_| std::env::var("GITHUB_REPOSITORY")).unwrap_or_default();
    let meta = manifest(dir, target, build, get("--tag").unwrap_or("rolling"), revision, &repository)?;
    std::fs::write(dir.join("release.json"), serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("MANIFEST {} files", meta["files"].as_object().map_or(0, |m| m.len()));
    Ok(())
}

/// Makes the patch from the package in `old` to the one in `new` (folders with release.json at the root). Returns the
/// manifest.
pub fn make(old: &Path, new: &Path, out: &Path) -> Result<Value, String> {
    let (om, nm) = (read_meta(old)?, read_meta(new)?);
    if str_of(&om, "target") != str_of(&nm, "target") {
        return Err(format!("targets differ: {} -> {}", str_of(&om, "target"), str_of(&nm, "target")));
    }
    let old_files: std::collections::BTreeMap<String, String> = files_of(&om).into_iter().collect();
    let new_files = files_of(&nm);
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
    let mut entries = Vec::new();
    for (i, (name, sha)) in new_files.iter().enumerate() {
        safe(name)?;
        let base = old_files.get(name);
        if base == Some(sha) {
            entries.push(json!({"name": name, "op": "same", "sha": sha}));
            continue;
        }
        let bytes = std::fs::read(new.join(name)).map_err(|e| format!("{name}: {e}"))?;
        if sha256_bytes(&bytes) != *sha {
            return Err(format!("{name}: the new package does not match its release.json"));
        }
        let old_bytes = match base {
            Some(b) => {
                let o = std::fs::read(old.join(name)).map_err(|e| format!("{name}: {e}"))?;
                if sha256_bytes(&o) != *b {
                    return Err(format!("{name}: the old package does not match its release.json"));
                }
                Some(o)
            }
            None => None,
        };
        let (mut payload, mut wlog) = encode(&bytes, old_bytes.as_deref())?;
        let mut op = if old_bytes.is_some() { "patch" } else { "add" };
        if old_bytes.is_some() {
            // a file that changed past recognition is smaller on its own
            let (alone, w) = encode(&bytes, None)?;
            if alone.len() < payload.len() {
                (payload, wlog, op) = (alone, w, "add");
            }
        }
        let entry = format!("p/{i:06}");
        zip.start_file(entry.as_str(), opts).map_err(|e| e.to_string())?;
        zip.write_all(&payload).map_err(|e| e.to_string())?;
        let mut e = json!({"name": name, "op": op, "sha": sha, "size": bytes.len(), "payload": entry, "window_log": wlog,
            "exec": is_exec(&new.join(name))});
        if op == "patch" {
            e["base_sha"] = json!(base);
        }
        entries.push(e);
    }
    let new_names: std::collections::BTreeSet<&String> = new_files.iter().map(|(n, _)| n).collect();
    for (name, sha) in &old_files {
        if !new_names.contains(name) {
            safe(name)?;
            entries.push(json!({"name": name, "op": "delete", "base_sha": sha}));
        }
    }
    let release = std::fs::read(new.join("release.json")).map_err(|e| e.to_string())?;
    let manifest = json!({
        "format": FORMAT, "target": str_of(&nm, "target"),
        "from_revision": str_of(&om, "revision"), "to_revision": str_of(&nm, "revision"),
        "from_build": om.get("build").cloned().unwrap_or(Value::Null), "to_build": nm.get("build").cloned().unwrap_or(Value::Null),
        "release_json_sha": sha256_bytes(&release), "files": entries,
    });
    zip.start_file("release.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(&release).map_err(|e| e.to_string())?;
    zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(manifest)
}

/// What staging a patch produced: the new files under `staging`, the files to delete, the new release.json.
pub struct Staged {
    staging: PathBuf,
    write: Vec<String>,
    delete: Vec<String>,
    release: Vec<u8>,
    pub to_revision: String,
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = zip.by_name(name).map_err(|e| format!("patch entry {name}: {e}"))?;
    let mut out = Vec::new();
    f.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Checks the install in `dir` against the patch's old side and builds every changed file in `staging`, each checked
/// against its new sha256. Nothing in `dir` changes.
pub fn stage(dir: &Path, patch: &Path, staging: &Path) -> Result<Staged, String> {
    let f = std::fs::File::open(patch).map_err(|e| format!("{}: {e}", patch.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", patch.display()))?;
    let manifest: Value = serde_json::from_slice(&read_entry(&mut zip, "manifest.json")?).map_err(|e| e.to_string())?;
    if str_of(&manifest, "format") != FORMAT {
        return Err(format!("unknown patch format {:?}", str_of(&manifest, "format")));
    }
    let installed = read_meta(dir)?;
    if str_of(&installed, "revision") != str_of(&manifest, "from_revision") {
        return Err(format!("the patch starts at {} but {} is installed", str_of(&manifest, "from_revision"), str_of(&installed, "revision")));
    }
    let release = read_entry(&mut zip, "release.json")?;
    if sha256_bytes(&release) != str_of(&manifest, "release_json_sha") {
        return Err("the patch's release.json does not match its manifest".into());
    }
    let new_meta: Value = serde_json::from_slice(&release).map_err(|e| e.to_string())?;
    let new_files: std::collections::BTreeMap<String, String> = files_of(&new_meta).into_iter().collect();
    std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let (mut write, mut delete) = (Vec::new(), Vec::new());
    for e in manifest.get("files").and_then(Value::as_array).into_iter().flatten() {
        let name = safe(str_of(e, "name"))?;
        let have = |sha: &str| -> Result<(), String> {
            match sha256_file(&dir.join(name)) {
                Ok(s) if s == sha => Ok(()),
                Ok(_) => Err(format!("{name} was changed since it was installed")),
                Err(_) => Err(format!("{name} is missing from the install")),
            }
        };
        match str_of(e, "op") {
            "same" => have(str_of(e, "sha"))?,
            "delete" => delete.push(name.to_owned()),
            op @ ("patch" | "add") => {
                let reference = if op == "patch" {
                    have(str_of(e, "base_sha"))?;
                    Some(std::fs::read(dir.join(name)).map_err(|e| e.to_string())?)
                } else {
                    None
                };
                let wlog = e.get("window_log").and_then(Value::as_u64).unwrap_or(27) as u32;
                let bytes = decode(&read_entry(&mut zip, str_of(e, "payload"))?, reference.as_deref(), wlog)?;
                let sha = sha256_bytes(&bytes);
                if sha != str_of(e, "sha") || new_files.get(name) != Some(&sha) {
                    return Err(format!("{name}: the patched file does not match the release"));
                }
                let to = staging.join(name);
                if let Some(p) = to.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                std::fs::write(&to, &bytes).map_err(|e| format!("{name}: {e}"))?;
                if e.get("exec").and_then(Value::as_bool).unwrap_or(false) {
                    set_exec(&to);
                }
                write.push(name.to_owned());
            }
            other => return Err(format!("{name}: unknown patch step {other:?}")),
        }
    }
    // every file the new release owns is accounted for
    let covered: std::collections::BTreeSet<&str> = manifest.get("files").and_then(Value::as_array).into_iter().flatten()
        .filter(|e| str_of(e, "op") != "delete").map(|e| str_of(e, "name")).collect();
    if let Some(missing) = new_files.keys().find(|n| !covered.contains(n.as_str())) {
        return Err(format!("the patch leaves out {missing}"));
    }
    Ok(Staged { staging: staging.to_path_buf(), write, delete, release, to_revision: str_of(&manifest, "to_revision").to_owned() })
}

/// Swaps the staged files into `dir`: each old file is moved aside first, everything is put back if a step fails, the
/// new release.json goes in last.
pub fn commit(dir: &Path, s: &Staged) -> Result<(), String> {
    let aside = |n: &str| dir.join(format!("{n}.patch-old"));
    let mut moved: Vec<&str> = Vec::new();
    let mut placed: Vec<&str> = Vec::new();
    let result = (|| -> Result<(), String> {
        for n in s.write.iter().chain(&s.delete) {
            if dir.join(n).exists() {
                std::fs::rename(dir.join(n), aside(n)).map_err(|e| format!("{n}: {e} (is the game still running?)"))?;
                moved.push(n);
            }
        }
        for n in &s.write {
            let to = dir.join(n);
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::rename(s.staging.join(n), &to).or_else(|_| std::fs::copy(s.staging.join(n), &to).map(|_| ()))
                .map_err(|e| format!("{n}: {e}"))?;
            placed.push(n);
        }
        std::fs::write(dir.join("release.json"), &s.release).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => {
            for n in moved {
                let _ = std::fs::remove_file(aside(n));
            }
            Ok(())
        }
        Err(e) => {
            for n in placed {
                let _ = std::fs::remove_file(dir.join(n));
            }
            for n in moved {
                let _ = std::fs::rename(aside(n), dir.join(n));
            }
            Err(e)
        }
    }
}

/// The links to apply from `from` to `to` in `index` (each link's `from` = the previous one's `to`), and their bytes;
/// None when there is no such chain or it is not worth it next to the full package.
pub fn chain(index: &Value, from: &str, to: &str) -> Option<(Vec<Value>, u64)> {
    if str_of(index, "format") != INDEX_FORMAT || from.is_empty() || from == to {
        return None;
    }
    let links = index.get("links")?.as_array()?;
    let (mut at, mut out, mut bytes) = (from.to_owned(), Vec::new(), 0u64);
    while at != to {
        let next = links.iter().find(|l| str_of(l, "from") == at)?;
        bytes += next.get("size").and_then(Value::as_u64)?;
        at = str_of(next, "to").to_owned();
        out.push(next.clone());
        if out.len() > links.len() {
            return None;
        }
    }
    let full = index.get("package_size").and_then(Value::as_u64)?;
    ((bytes as f64) < full as f64 * CHAIN_SHARE).then_some((out, bytes))
}

/// The asset name of the link from `from` to `to`.
pub fn link_file(target: &str, from: &str, to: &str) -> String {
    let short = |s: &str| s.chars().take(12).collect::<String>();
    format!("skate3rust-{target}.{}-{}.patch", short(from), short(to))
}

/// `skate-launcher make-patch --old <pkg> --new <pkg> --out <dir> [--index <previous patches.json>] [--keep N]
/// [--package-size N]`: writes the new link (checked by staging it against the old package) and the new index (the
/// new link plus the newest N-1 links of the previous index that chain into it). Prints `PATCH <file>` and
/// `CARRY <file>` for each link file the release needs, and `INDEX <file>`.
pub fn cli_make(args: &[String]) -> Result<(), String> {
    let get = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).map(String::as_str);
    let (old, new, out) = (get("--old").ok_or("--old missing")?, get("--new").ok_or("--new missing")?, get("--out").ok_or("--out missing")?);
    let keep: usize = get("--keep").and_then(|s| s.parse().ok()).unwrap_or(5).max(1);
    let out = Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let scratch = std::env::temp_dir().join(format!("skate-make-patch-{}", std::process::id()));
    let result = (|| {
        let old_dir = open_package(Path::new(old), &scratch.join("old"))?;
        let new_dir = open_package(Path::new(new), &scratch.join("new"))?;
        let (om, nm) = (read_meta(&old_dir)?, read_meta(&new_dir)?);
        let target = str_of(&nm, "target").to_owned();
        let (from, to) = (str_of(&om, "revision").to_owned(), str_of(&nm, "revision").to_owned());
        if from == to || from.is_empty() {
            println!("SAME the old package is already at {to}: no patch");
            return Ok(());
        }
        let file = link_file(&target, &from, &to);
        let path = out.join(&file);
        let manifest = make(&old_dir, &new_dir, &path)?;
        // the link must turn the old package into the new one
        let staged = stage(&old_dir, &path, &scratch.join("check"))?;
        if staged.to_revision != to {
            return Err("the made patch does not end at the new revision".into());
        }
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        let counts = |op: &str| manifest["files"].as_array().map_or(0, |a| a.iter().filter(|e| str_of(e, "op") == op).count());
        println!("PATCH {file} {size} bytes: {} patched, {} added, {} deleted, {} unchanged", counts("patch"), counts("add"),
            counts("delete"), counts("same"));
        let mut links = vec![json!({"from": from, "to": to, "from_build": om.get("build"), "to_build": nm.get("build"),
            "file": file, "sha256": sha256_file(&path)?, "size": size})];
        if let Some(prev) = get("--index").and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
        {
            // older links, newest first, as long as they still chain into the new one
            let mut at = from.clone();
            let old_links = prev.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
            while links.len() < keep {
                let Some(l) = old_links.iter().find(|l| str_of(l, "to") == at) else { break };
                println!("CARRY {}", str_of(l, "file"));
                at = str_of(l, "from").to_owned();
                links.push(l.clone());
            }
        }
        let package_size: u64 = get("--package-size").and_then(|s| s.parse().ok()).unwrap_or(0);
        let index = json!({"format": INDEX_FORMAT, "target": target, "revision": to, "package_size": package_size, "links": links});
        let index_file = format!("skate3rust-{target}.patches.json");
        std::fs::write(out.join(&index_file), serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("INDEX {index_file}");
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// `skate-launcher apply-patch <install folder> <patch>`: applies one patch to an installed release (tests, support).
pub fn cli_apply(args: &[String]) -> Result<(), String> {
    let (dir, patch) = match args {
        [d, p, ..] => (Path::new(d), Path::new(p)),
        _ => return Err("usage: apply-patch <install folder> <patch>".into()),
    };
    let staging = std::env::temp_dir().join(format!("skate-apply-patch-{}", std::process::id()));
    let result = stage(dir, patch, &staging).and_then(|s| {
        commit(dir, &s)?;
        println!("APPLIED {} files changed, {} deleted, now at {}", s.write.len(), s.delete.len(), s.to_revision);
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(dir: &Path, revision: &str, build: u64, files: &[(&str, &[u8])]) {
        let _ = std::fs::remove_dir_all(dir);
        let mut map = serde_json::Map::new();
        for (n, b) in files {
            let p = dir.join(n);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b).unwrap();
            map.insert((*n).into(), json!(sha256_bytes(b)));
        }
        let meta = json!({"schema": 1, "target": "windows-x64", "revision": revision, "build": build, "files": map});
        std::fs::write(dir.join("release.json"), meta.to_string()).unwrap();
    }

    fn big(seed: u8, len: usize) -> Vec<u8> {
        (0..len).map(|i| (i as u32).wrapping_mul(2_654_435_761).rotate_left(seed as u32) as u8).collect()
    }

    #[test]
    fn a_patch_turns_the_old_install_into_the_new_release_and_is_small() {
        let root = std::env::temp_dir().join(format!("skate-patch-test-{}", std::process::id()));
        let (old, new, install) = (root.join("old"), root.join("new"), root.join("install"));
        let exe = big(3, 400_000);
        let mut exe2 = exe.clone();
        exe2[1000..1100].copy_from_slice(&[7u8; 100]);
        package(&old, "aaaa", 20, &[("skate3rust.exe", &exe), ("docs/a.md", b"same"), ("support/gone.txt", b"old only")]);
        package(&new, "bbbb", 21, &[("skate3rust.exe", &exe2), ("docs/a.md", b"same"), ("docs/new.md", b"brand new")]);
        package(&install, "aaaa", 20, &[("skate3rust.exe", &exe), ("docs/a.md", b"same"), ("support/gone.txt", b"old only")]);
        std::fs::create_dir_all(install.join("saves")).unwrap();
        std::fs::write(install.join("saves/mine.sav"), b"user data").unwrap();
        let patch = root.join("p.patch");
        let m = make(&old, &new, &patch).unwrap();
        let ops: Vec<(String, String)> = m["files"].as_array().unwrap().iter().map(|e| (str_of(e, "name").into(), str_of(e, "op").into())).collect();
        assert!(ops.contains(&("skate3rust.exe".into(), "patch".into())) && ops.contains(&("docs/new.md".into(), "add".into()))
            && ops.contains(&("support/gone.txt".into(), "delete".into())) && ops.contains(&("docs/a.md".into(), "same".into())), "{ops:?}");
        assert!(std::fs::metadata(&patch).unwrap().len() < 20_000, "a 100-byte change in 400 KB stays small");
        let s = stage(&install, &patch, &root.join("staging")).unwrap();
        commit(&install, &s).unwrap();
        assert_eq!(std::fs::read(install.join("skate3rust.exe")).unwrap(), exe2);
        assert_eq!(std::fs::read(install.join("docs/new.md")).unwrap(), b"brand new");
        assert!(!install.join("support/gone.txt").exists());
        assert_eq!(std::fs::read(install.join("saves/mine.sav")).unwrap(), b"user data", "user files are never touched");
        assert_eq!(str_of(&read_meta(&install).unwrap(), "revision"), "bbbb");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_changed_install_or_another_revision_is_refused_and_nothing_changes() {
        let root = std::env::temp_dir().join(format!("skate-patch-refuse-{}", std::process::id()));
        let (old, new, install) = (root.join("old"), root.join("new"), root.join("install"));
        package(&old, "aaaa", 20, &[("a.bin", b"one"), ("b.bin", b"two")]);
        package(&new, "bbbb", 21, &[("a.bin", b"ONE"), ("b.bin", b"two")]);
        package(&install, "aaaa", 20, &[("a.bin", b"one"), ("b.bin", b"tw0")]);
        let patch = root.join("p.patch");
        make(&old, &new, &patch).unwrap();
        let err = stage(&install, &patch, &root.join("s1")).err().unwrap();
        assert!(err.contains("b.bin"), "{err}");
        assert_eq!(std::fs::read(install.join("a.bin")).unwrap(), b"one");
        package(&install, "cccc", 19, &[("a.bin", b"one"), ("b.bin", b"two")]);
        assert!(stage(&install, &patch, &root.join("s2")).err().unwrap().contains("cccc"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_unix_manifest_owns_the_program_files_only() {
        let root = std::env::temp_dir().join(format!("skate-manifest-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (n, b) in [("bin/skate3rust", "game"), ("SETUP.sh", "setup"), ("PLAY.sh", "play"), ("tools/a.py", "tool"),
            ("mods/Skyline_Drive_Mod/mod.lua", "mod"), ("saves/s.sav", "save"), (".git/HEAD", "ref")] {
            let p = root.join(n);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        }
        let m = manifest(&root, "linux-x64", 140, "rolling", &"c".repeat(40), "Hailey-Ross/rusty-trucks").unwrap();
        let mut names: Vec<&String> = m["files"].as_object().unwrap().keys().collect();
        names.sort();
        assert_eq!(names, ["SETUP.sh", "bin/skate3rust", "tools/a.py"], "no PLAY.sh (the launcher writes it), mods, saves or dot folders");
        assert_eq!(m["files"]["tools/a.py"], json!(sha256_bytes(b"tool")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patches_never_name_player_data_or_paths_outside_the_install() {
        for bad in ["../x.exe", "/abs", "a\\b", "C:x", "saves/slot.sav", "Settings/a.json", "mods/m.lua", ".git/config", "a/CON.txt", "x/", "a/./b"] {
            assert!(safe(bad).is_err(), "{bad}");
        }
        for good in ["skate3rust.exe", "support/skate3setup.exe", "docs/images/skating-crab.png", "steam-relay/steam_api64.dll", "console.txt"] {
            assert!(safe(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn the_chain_follows_links_from_the_installed_revision_and_gives_up_when_not_worth_it() {
        let link = |f: &str, t: &str, size: u64| json!({"from": f, "to": t, "file": link_file("windows-x64", f, t), "size": size});
        let index = json!({"format": INDEX_FORMAT, "package_size": 1000, "links": [link("c", "d", 100), link("b", "c", 200), link("a", "b", 300)]});
        let (links, bytes) = chain(&index, "b", "d").unwrap();
        assert_eq!((links.len(), bytes), (2, 300));
        assert_eq!(chain(&index, "a", "d").map(|c| c.1), Some(600));
        assert!(chain(&index, "z", "d").is_none(), "an older install takes the full download");
        let heavy = json!({"format": INDEX_FORMAT, "package_size": 500, "links": [link("a", "b", 450)]});
        assert!(chain(&heavy, "a", "b").is_none(), "a patch near the package's size is not worth it");
        assert!(chain(&index, "d", "d").is_none());
    }
}
