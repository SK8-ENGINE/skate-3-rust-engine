//! The browser build's file system: the player's installation, mounted at [`ROOT`] from the files
//! the loader page read (`web/installation.js`), plus writes kept in memory for the session and
//! handed to the page to keep. Mirrors the `std::fs` API the engine uses.
use crate::memory::{Content, Kind, Tree, normalise, not_found};
use std::{
    ffi::OsString,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    collections::BTreeSet,
    sync::{Arc, Mutex, MutexGuard},
};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};

/// Where the installation the player chose appears: its `assets/`, `maps/` and `settings/`.
pub const ROOT: &str = "/installation";

#[wasm_bindgen]
extern "C" {
    /// `[[path, size], ...]`: every file in the installation, `/`-separated and relative to it.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = files)]
    fn page_files() -> Result<js_sys::Array, JsValue>;
    /// The file's bytes if the page holds them (a `Uint8Array`), else `undefined`.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = read)]
    fn page_read(path: &str) -> Result<JsValue, JsValue>;
    /// Reads the file from the player's disk without keeping it: a promise of a `Uint8Array`.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = fetch)]
    fn page_fetch(path: &str) -> Result<js_sys::Promise, JsValue>;
    /// Reads these files from the player's disk and holds them for `read`: a promise.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = prefetch)]
    fn page_prefetch(paths: js_sys::Array) -> Result<js_sys::Promise, JsValue>;
    /// Stops holding the file.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = release)]
    fn page_release(path: &str) -> Result<(), JsValue>;
    /// The engine wrote this file (settings and the like); the page may keep it for next time.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = saved)]
    fn page_saved(path: &str, bytes: &[u8]) -> Result<(), JsValue>;
    /// The engine removed or renamed this file: the page forgets what it kept of it.
    #[wasm_bindgen(catch, js_namespace = skateInstallation, js_name = removed)]
    fn page_removed(path: &str) -> Result<(), JsValue>;
}

static TREE: Mutex<Option<Tree>> = Mutex::new(None);
/// Installation files a synchronous read wanted before they were read ahead.
static MISSES: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn tree() -> MutexGuard<'static, Option<Tree>> {
    TREE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_tree<T>(f: impl FnOnce(&mut Tree) -> io::Result<T>) -> io::Result<T> {
    let mut tree = tree();
    f(tree.get_or_insert_with(Tree::default))
}

fn js_error(context: &str, value: JsValue) -> io::Error {
    let text = value
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"));
    io::Error::other(format!("{context}: {text}"))
}

/// Mounts the installation the loader page read at [`ROOT`] and returns that root. Fails, with a
/// message for the player, if the page has none.
pub fn mount() -> Result<PathBuf, String> {
    let files = page_files().map_err(|_| {
        "No installation chosen. Choose your converted Skate 3 installation folder on this page.".to_string()
    })?;
    let root = PathBuf::from(ROOT);
    let mut mounted = Tree::default();
    for entry in files.iter() {
        let entry: js_sys::Array = entry.unchecked_into();
        let (Some(path), Some(len)) = (entry.get(0).as_string(), entry.get(1).as_f64()) else {
            return Err("The page listed an installation file without a path or size".into());
        };
        mounted.insert(&root.join(&path), Content::Installation { len: len as u64 }).map_err(|e| e.to_string())?;
    }
    mounted.create_dirs(&root).map_err(|e| e.to_string())?;
    *tree() = Some(mounted);
    Ok(root)
}

/// `path` relative to [`ROOT`], as the page names it; `None` outside the installation.
fn page_path(path: &Path) -> Option<String> {
    let relative = path.strip_prefix(ROOT).ok()?;
    let parts: Vec<_> = relative.iter().map(|part| part.to_str()).collect::<Option<_>>()?;
    Some(parts.join("/"))
}

fn uint8(value: JsValue) -> Option<Vec<u8>> {
    value.dyn_into::<js_sys::Uint8Array>().ok().map(|array| array.to_vec())
}

fn contents(path: &Path) -> io::Result<Vec<u8>> {
    let path = normalise(path);
    match with_tree(|tree| tree.content(&path))? {
        Content::Bytes(bytes) => Ok(bytes.to_vec()),
        Content::Installation { .. } => {
            let name = page_path(&path).ok_or_else(|| not_found(&path))?;
            let value = page_read(&name).map_err(|e| js_error(&name, e))?;
            uint8(value).ok_or_else(|| {
                MISSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(path.clone());
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("{name}: not read in advance; the browser build reads files ahead with skate_fs::prefetch"),
                )
            })
        }
    }
}

/// Reads a whole file without blocking: what an asset loader awaits. Files not read in advance
/// come from the player's disk on demand.
pub async fn read_async(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = normalise(path.as_ref());
    let content = with_tree(|tree| tree.content(&path))?;
    let Content::Installation { .. } = content else { return contents(&path) };
    let name = page_path(&path).ok_or_else(|| not_found(&path))?;
    if let Some(bytes) = uint8(page_read(&name).map_err(|e| js_error(&name, e))?) {
        return Ok(bytes);
    }
    let promise = page_fetch(&name).map_err(|e| js_error(&name, e))?;
    let value = wasm_bindgen_futures::JsFuture::from(promise).await.map_err(|e| js_error(&name, e))?;
    uint8(value).ok_or_else(|| io::Error::other(format!("{name}: the page returned no bytes")))
}

/// Reads installation files from the player's disk ahead of the synchronous reads that need them,
/// which can then be served from the page. Paths outside the installation, and files already in
/// memory, are skipped.
pub async fn prefetch<P: AsRef<Path>>(paths: impl IntoIterator<Item = P>) -> io::Result<()> {
    let names = js_sys::Array::new();
    for path in paths {
        let path = normalise(path.as_ref());
        if let Ok(Content::Installation { .. }) = with_tree(|tree| tree.content(&path)) {
            names.push(&JsValue::from_str(&page_path(&path).ok_or_else(|| not_found(&path))?));
        }
    }
    if names.length() == 0 {
        return Ok(());
    }
    let promise = page_prefetch(names).map_err(|e| js_error("prefetch", e))?;
    wasm_bindgen_futures::JsFuture::from(promise).await.map_err(|e| js_error("prefetch", e))?;
    Ok(())
}

/// Whether a synchronous read has wanted an installation file that was not read ahead.
pub fn missed() -> bool {
    !MISSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty()
}

/// The installation files synchronous reads have wanted, and not had, since the last call: what
/// a retry should [`prefetch`].
pub fn take_misses() -> Vec<PathBuf> {
    std::mem::take(&mut *MISSES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())).into_iter().collect()
}

/// Lets the page drop a prefetched file (a map left behind). A later synchronous read needs
/// another [`prefetch`].
pub fn release(path: impl AsRef<Path>) {
    if let Some(name) = page_path(&normalise(path.as_ref())) {
        let _ = page_release(&name);
    }
}

/// Every installation file under `dir`, recursively, with its size: what a caller picks its
/// [`prefetch`] from.
pub fn files_under(dir: impl AsRef<Path>) -> Vec<(PathBuf, u64)> {
    with_tree(|tree| Ok(tree.files_under(dir.as_ref()))).unwrap_or_default()
}

fn store(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    let path = normalise(path);
    with_tree(|tree| tree.write(&path, Arc::from(bytes.as_slice())))?;
    if let Some(name) = page_path(&path) {
        // Keeping it is the page's choice; the session already has it.
        let _ = page_saved(&name, &bytes);
    }
    Ok(())
}

/// Scratch space for the session (`std::env::temp_dir` panics in a browser), in memory.
pub fn temp_dir() -> PathBuf {
    PathBuf::from("/tmp")
}

pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    contents(path.as_ref())
}

pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    String::from_utf8(contents(path.as_ref())?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    store(path.as_ref(), contents.as_ref().to_vec())
}

pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    let bytes = contents(from.as_ref())?;
    let len = bytes.len() as u64;
    store(to.as_ref(), bytes)?;
    Ok(len)
}

pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    with_tree(|tree| tree.create_dir(path.as_ref()))
}

pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    with_tree(|tree| tree.create_dirs(path.as_ref()))
}

fn forget(path: &Path) {
    if let Some(name) = page_path(&normalise(path)) {
        let _ = page_removed(&name);
    }
}

pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    with_tree(|tree| tree.remove_file(path.as_ref()))?;
    forget(path.as_ref());
    Ok(())
}

pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    with_tree(|tree| tree.remove_dir(path.as_ref(), false))
}

pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    with_tree(|tree| tree.remove_dir(path.as_ref(), true))
}

pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let (from, to) = (from.as_ref(), to.as_ref());
    // A written file (`settings.json.tmp` → `settings.json`) is handed to the page under its final name.
    let written = match with_tree(|tree| tree.content(from)) {
        Ok(Content::Bytes(bytes)) => Some(bytes),
        _ => None,
    };
    with_tree(|tree| tree.rename(from, to))?;
    if let (Some(bytes), Some(name)) = (written, page_path(&normalise(to))) {
        forget(from);
        let _ = page_saved(&name, &bytes);
    }
    Ok(())
}

pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    with_tree(|tree| tree.canonical(path.as_ref()))
}

pub fn exists(path: impl AsRef<Path>) -> bool {
    with_tree(|tree| Ok(tree.kind(path.as_ref()).is_some())).unwrap_or(false)
}

pub fn is_file(path: impl AsRef<Path>) -> bool {
    with_tree(|tree| Ok(tree.kind(path.as_ref()) == Some(Kind::File))).unwrap_or(false)
}

pub fn is_dir(path: impl AsRef<Path>) -> bool {
    with_tree(|tree| Ok(tree.kind(path.as_ref()) == Some(Kind::Dir))).unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileType(Kind);

impl FileType {
    pub fn is_file(&self) -> bool {
        self.0 == Kind::File
    }
    pub fn is_dir(&self) -> bool {
        self.0 == Kind::Dir
    }
    pub fn is_symlink(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    kind: Kind,
    len: u64,
}

// The same methods as `std::fs::Metadata`, which has no `is_empty` either.
#[allow(clippy::len_without_is_empty)]
impl Metadata {
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_file(&self) -> bool {
        self.kind == Kind::File
    }
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
    pub fn file_type(&self) -> FileType {
        FileType(self.kind)
    }
    /// Browsers do not expose it for these files.
    pub fn modified(&self) -> io::Result<std::time::SystemTime> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "modification times are not available in the browser"))
    }
}

pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    let path = path.as_ref();
    with_tree(|tree| match tree.kind(path) {
        Some(Kind::Dir) => Ok(Metadata { kind: Kind::Dir, len: 0 }),
        Some(Kind::File) => Ok(Metadata { kind: Kind::File, len: tree.content(path)?.len() }),
        None => Err(not_found(path)),
    })
}

pub fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    metadata(path)
}

pub struct DirEntry {
    path: PathBuf,
    kind: Kind,
}

impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> OsString {
        self.path.file_name().unwrap_or_default().to_owned()
    }
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(FileType(self.kind))
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        metadata(&self.path)
    }
}

pub struct ReadDir(std::vec::IntoIter<(PathBuf, Kind)>);

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(path, kind)| Ok(DirEntry { path, kind }))
    }
}

/// Entries come back with the path as given joined to each name, as `std::fs::read_dir` does.
pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
    let path = path.as_ref();
    let entries = with_tree(|tree| tree.list(path))?;
    Ok(ReadDir(
        entries
            .into_iter()
            .map(|(full, kind)| (path.join(full.file_name().unwrap_or_default()), kind))
            .collect::<Vec<_>>()
            .into_iter(),
    ))
}

/// An open file: its bytes in memory. Written files reach the tree (and the page) on flush or drop.
pub struct File {
    path: PathBuf,
    cursor: io::Cursor<Vec<u8>>,
    writable: bool,
    dirty: bool,
}

impl File {
    pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
        OpenOptions::new().read(true).open(path)
    }
    pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
        OpenOptions::new().write(true).create(true).truncate(true).open(path)
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        Ok(Metadata { kind: Kind::File, len: self.cursor.get_ref().len() as u64 })
    }
    pub fn sync_all(&mut self) -> io::Result<()> {
        self.flush()
    }
    pub fn set_len(&mut self, len: u64) -> io::Result<()> {
        self.cursor.get_mut().resize(len as usize, 0);
        self.dirty = true;
        Ok(())
    }
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.writable {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "file not opened for writing"));
        }
        self.dirty = true;
        self.cursor.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.dirty {
            store(&self.path, self.cursor.get_ref().clone())?;
            self.dirty = false;
        }
        Ok(())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }
    pub fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }
    pub fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }
    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.create_new = create_new;
        self
    }
    pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let path = normalise(path.as_ref());
        let writable = self.write || self.append;
        let found = exists(&path);
        if self.create_new && found {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: already exists", path.display())));
        }
        let bytes = if found && !(self.truncate && writable) {
            contents(&path)?
        } else if found || (writable && (self.create || self.create_new)) {
            Vec::new()
        } else {
            return Err(not_found(&path));
        };
        let created = !found;
        let mut cursor = io::Cursor::new(bytes);
        if self.append {
            cursor.seek(SeekFrom::End(0))?;
        }
        let mut file = File { path, cursor, writable, dirty: created || (self.truncate && writable) };
        // A created or truncated file exists from now on, as it would on disk.
        file.flush()?;
        Ok(file)
    }
}
