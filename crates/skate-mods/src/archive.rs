use crate::schema::{valid_id, Manifest};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Cache {
    entries: HashMap<PathBuf, (u64, PathBuf)>,
    session: Option<PathBuf>,
    next: u64,
}

impl Cache {
    pub fn materialize(&mut self, root: &Path, archive: &Path) -> Result<PathBuf, String> {
        let bytes = fs::read(archive).map_err(|e| e.to_string())?;
        let hash = {
            let mut h = 0u64;
            for b in blake3::hash(&bytes).as_bytes() {
                h = h.wrapping_mul(31).wrapping_add(*b as u64);
            }
            h
        };
        if let Some((prev, path)) = self.entries.get(archive) {
            if *prev == hash && path.is_dir() {
                return Ok(path.clone());
            }
        }
        if self.session.is_none() {
            let cache = root.join(".cache");
            if cache.exists()
                && fs::symlink_metadata(&cache)
                    .map_err(|e| e.to_string())?
                    .file_type()
                    .is_symlink()
            {
                return Err("ZIP cache must not be a link".into());
            }
            fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
            if !cache
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
            {
                return Err("ZIP cache escapes mods folder".into());
            }
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let session = cache.join(format!("{}-{nonce}", std::process::id()));
            fs::create_dir(&session).map_err(|e| e.to_string())?;
            self.session = Some(session);
        }
        self.next += 1;
        let destination = self
            .session
            .as_ref()
            .unwrap()
            .join(self.next.to_string());
        fs::create_dir(&destination).map_err(|e| e.to_string())?;
        let file = fs::File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = file
                .enclosed_name()
                .ok_or_else(|| "zip entry escapes archive".to_string())?
                .to_path_buf();
            if name
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err("zip path contains ..".into());
            }
            let out = destination.join(&name);
            if file.is_dir() {
                fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                continue;
            }
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut dest = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&out)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut dest).map_err(|e| e.to_string())?;
        }
        self.entries
            .insert(archive.to_owned(), (hash, destination.clone()));
        Ok(destination)
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        if let Some(path) = &self.session {
            if let (Ok(actual), Ok(parent)) = (
                path.canonicalize(),
                path.parent().unwrap().canonicalize(),
            ) {
                if actual.parent() == Some(parent.as_path())
                    && actual.file_name() == path.file_name()
                {
                    let _ = fs::remove_dir_all(path);
                }
            }
        }
    }
}

pub fn validate_package(path: &Path) -> Result<Manifest, String> {
    let source = path.canonicalize().map_err(|e| e.to_string())?;
    let mut cache = Cache::default();
    let root = if source.is_dir() {
        source.clone()
    } else {
        let parent = source.parent().ok_or("Missing package parent")?;
        cache.materialize(parent, &source)?
    };
    let bytes = read_bounded(&root, "mod.json", 64 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest.validate()?;
    let _ = valid_id(&manifest.id);
    let entry = read_bounded(&root, &manifest.entry, 256 * 1024)?;
    let source = std::str::from_utf8(&entry).map_err(|e| e.to_string())?;
    if source.starts_with('\u{1b}') {
        return Err("Lua bytecode is unsupported".into());
    }
    mlua::Lua::new()
        .load(source)
        .set_mode(mlua::chunk::ChunkMode::Text)
        .into_function()
        .map_err(|e| e.to_string())?;
    Ok(manifest)
}

pub fn read_bounded(root: &Path, relative: &str, limit: u64) -> Result<Vec<u8>, String> {
    let path = Path::new(relative);
    if path
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || relative.contains(':')
    {
        return Err("Use a relative path without traversal".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(&root) {
        return Err("Asset escapes mod root".into());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Asset too large".into());
    }
    Ok(bytes)
}

pub fn fingerprint(root: &Path) -> Result<u64, String> {
    let mut paths = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                walk(&path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    walk(root, &mut paths)?;
    paths.sort();
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy();
        hasher.update(rel.as_bytes());
        hasher.update(&[0]);
        hasher.update(&fs::read(&path).map_err(|e| e.to_string())?);
        hasher.update(&[0]);
    }
    let hash = hasher.finalize();
    let mut out = 0u64;
    for b in hash.as_bytes() {
        out = out.wrapping_mul(31).wrapping_add(*b as u64);
    }
    Ok(out)
}

#[allow(dead_code)]
pub fn write_temp(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}
