//! Retail front-end menu movies (APT) loaded and linked at startup (menus milestone 3c); their shapes
//! and bitmaps resolve for drawing through `MenuScene` (milestone 4); the screen manager draws them.
//!
//! Setup exports the movies from the owned disc (`tools/prepare_menu_movies.py`) into
//! `assets/private/menu-movies/`: `manifest.json`, `shared.json` (language table, fonts, native font
//! layout) and `movies/<key>.json` per movie in the HUD player format plus its import tables. Keys are
//! the retail import paths (`apt_imports::library_key`). Every movie is loaded with `Movie::load` and
//! its imports linked with `Movie::link_imports` (retail linker sub_82E76070, see apt_imports.rs).
//!
//! Mods supply or replace a movie by key (`sdk.menus.movie`); the mod's file is layered over the
//! retail one through `apt_imports::OverrideTable` (newest owner wins) and removed when the mod stops.
//!
//! Bitmaps (doc 31, Milestone 4): retail menu movies never place a bitmap character; every bitmap is
//! drawn as the texture of a GEO shape unit (render type 2 clamped / 3 wrapped), which names it by
//! character id. Its stable id is `<movie key>#<bitmap character id>` in the movie that defines the
//! shape (imported shapes keep the library's id). Mods replace one by that id (`sdk.menus.bitmap`).
use crate::{
    apt_imports::{library_key, resolve, MovieDecl, OverrideTable},
    apt_movie::Movie,
    apt_scene::{Shape, ShapeSource, Shapes},
};
use bevy::prelude::*;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const FORMAT: &str = "skate3-menu-movies";
/// Largest mod movie file accepted (the biggest retail menu movie is well below this).
pub const MAX_MOD_MOVIE_BYTES: u64 = 16 << 20;
/// Menu manifest version setup writes; history of what each version added: docs/hails-additions/31-retail-menus.md#menu-manifest-versions
pub const VERSION: u64 = 14;
/// Oldest manifest version this engine reads and what older sets lack: docs/hails-additions/31-retail-menus.md#oldest-menu-manifest
pub const OLDEST_VERSION: u64 = 5;
/// Texture key prefix of a named runtime image (`Game.ImageRenderer_Load`): `image/<image_key>`.
pub const NAMED_IMAGE_PREFIX: &str = "image/";

/// The table key of a runtime image request: retail builds `data\` + name + `.rx2` (b258 section 2); `\` -> `/`, no
/// leading `/` or `data/`, no `.rx2`, lower case (retail names are all lower case; this only widens what mods type).
pub fn image_key(request: &str) -> String {
    let path = request.replace('\\', "/").to_ascii_lowercase();
    let path = path.trim_start_matches('/');
    let path = path.strip_prefix("data/").unwrap_or(path);
    path.strip_suffix(".rx2").unwrap_or(path).to_owned()
}

/// Mod bitmap limits: per mod, and the largest side (same as mod mesh textures).
pub const MAX_MOD_BITMAPS: usize = 64;
pub const MAX_MOD_BITMAP_SIDE: u32 = 2048;

/// A mod-supplied bitmap: straight RGBA8 (sRGB), row-major, `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What a texture key from `MenuScene` draws: a retail payload in the exported set, or a mod image.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuImage<'a> {
    /// RGBA8 file relative to the menu movie set root, with its size.
    Retail { path: &'a str, width: u32, height: u32 },
    Mod(&'a ModBitmap),
}

/// Largest mod bitmap PNG file accepted.
pub const MAX_MOD_BITMAP_BYTES: u64 = 4 << 20;

/// Decodes a mod's PNG into straight RGBA8 (sRGB).
pub fn decode_mod_bitmap(png: &[u8]) -> Result<ModBitmap, String> {
    use bevy::{
        asset::RenderAssetUsages,
        image::{CompressedImageFormats, ImageSampler, ImageType},
        render::render_resource::TextureFormat,
    };
    let image = Image::from_buffer(
        png,
        ImageType::MimeType("image/png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| format!("menu bitmap PNG decode failed: {e}"))?;
    let image = if image.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb {
        image
    } else {
        image.convert(TextureFormat::Rgba8UnormSrgb).ok_or("menu bitmap pixel format unsupported")?
    };
    let (width, height) = (image.width(), image.height());
    let rgba = image.data.ok_or("menu bitmap has no pixels")?;
    Ok(ModBitmap { width, height, rgba })
}

/// Stable bitmap id: movie key plus bitmap character id.
pub fn bitmap_id(movie: &str, bitmap: i32) -> String {
    format!("{movie}#{bitmap}")
}
const MOD_TEXTURE_PREFIX: &str = "mod:";

/// Linked movies plus their sources; serialisable parts only in `snapshot`.
#[derive(Resource, Default)]
pub struct RetailMenuMovies {
    /// Retail movie JSON by key, with the shared fields merged in.
    base: BTreeMap<String, Value>,
    /// Shared language / font fields (merged into mod movies too).
    shared: serde_json::Map<String, Value>,
    /// Mod movie JSON by owner, then key.
    mods: BTreeMap<String, BTreeMap<String, Value>>,
    overrides: OverrideTable,
    /// GEO units by movie key, then shape character (from the winning source of each key).
    shapes: BTreeMap<String, Shapes>,
    /// Retail texture payloads (key = path in the set) with their size.
    retail_images: BTreeMap<String, [u32; 2]>,
    /// Mod bitmaps in submission order, newest last: (owner, movie key, bitmap id, image).
    mod_bitmaps: Vec<(String, String, i32, ModBitmap)>,
    /// Named runtime images (manifest `images`, version 7): image key -> payload path, size.
    named_images: BTreeMap<String, (String, [u32; 2])>,
    /// Mod named images, newest last: (owner, image key, image). A mod replaces a retail name or adds its own.
    mod_images: Vec<(String, String, ModBitmap)>,
    /// Linked movies by key (retail layered with mod movies).
    pub movies: BTreeMap<String, Movie>,
    pub imports_total: usize,
    pub imports_resolved: usize,
    /// Movies that failed to load or link, with the reason.
    pub failures: BTreeMap<String, String>,
}

fn merged(mut movie: Value, shared: &serde_json::Map<String, Value>) -> Value {
    if let Some(object) = movie.as_object_mut() {
        for (k, v) in shared {
            object.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    movie
}

fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}

impl RetailMenuMovies {
    /// Reads the exported set; an absent set gives an empty resource.
    pub fn read(root: &Path) -> Result<Self, String> {
        let manifest_path = root.join("manifest.json");
        if !manifest_path.is_file() {
            return Ok(Self::default());
        }
        let manifest = read_json(&manifest_path)?;
        let version = manifest["version"].as_u64().unwrap_or(0);
        if manifest["format"] != FORMAT || !(OLDEST_VERSION..=VERSION).contains(&version) {
            // 2 (Milestone 3d): placement clip actions; 3 (Milestone 4): unit bitmap ids. Setup re-exports.
            return Err("unsupported menu movie set".into());
        }
        let shared = read_json(&root.join("shared.json"))?
            .as_object()
            .cloned()
            .ok_or("menu movie shared.json is not an object")?;
        let mut out = Self { shared, ..Self::default() };
        for (key, file) in manifest["movies"].as_object().ok_or("menu movie list missing")? {
            let file = file.as_str().ok_or("menu movie file missing")?;
            if file.contains("..") {
                return Err(format!("menu movie path escapes its folder: {file}"));
            }
            out.base.insert(library_key(key), merged(read_json(&root.join(file))?, &out.shared));
        }
        for (key, image) in manifest["images"].as_object().into_iter().flatten() {
            let path = image["rgba"].as_str().ok_or("menu image path missing")?;
            if path.contains("..") {
                return Err(format!("menu image path escapes its folder: {path}"));
            }
            let size = [image["width"].as_u64().unwrap_or(0) as u32, image["height"].as_u64().unwrap_or(0) as u32];
            out.named_images.insert(image_key(key), (path.to_owned(), size));
        }
        out.relink();
        Ok(out)
    }

    /// The source JSON for a key: newest mod movie, else retail.
    fn source(&self, key: &str) -> Option<&Value> {
        // OverrideTable decides the winning owner; the JSON comes from that owner's entry.
        self.mods
            .iter()
            .filter_map(|(owner, movies)| movies.get(key).map(|m| (owner, m)))
            .max_by_key(|(owner, _)| self.order(owner, key))
            .map(|(_, m)| m)
            .or_else(|| self.base.get(key))
    }
    fn order(&self, owner: &str, key: &str) -> usize {
        self.overrides.position(owner, key).unwrap_or(0)
    }

    /// Loads every movie (retail plus mod) and links its imports.
    pub fn relink(&mut self) {
        let mut keys: Vec<String> = self.base.keys().cloned().collect();
        for movies in self.mods.values() {
            keys.extend(movies.keys().cloned());
        }
        keys.sort();
        keys.dedup();
        self.failures.clear();
        let mut all_shapes = BTreeMap::new();
        let mut images = BTreeMap::new();
        // Font atlases: text draws name them (apt_text), so they resolve like bitmaps.
        for font in self.shared.get("fonts").and_then(Value::as_object).into_iter().flat_map(|f| f.values()) {
            let size = &font["definition"]["textures"][0];
            if let (Some(path), Some(w), Some(h)) = (font["texture"].as_str(), size["width"].as_u64(), size["height"].as_u64()) {
                images.insert(path.to_string(), [w as u32, h as u32]);
            }
        }
        let mut decls = BTreeMap::new();
        let mut libraries = BTreeMap::new();
        for key in &keys {
            let json = self.source(key).expect("listed key");
            match shapes_of(json) {
                Ok(shapes) => {
                    for t in shapes.values().flatten().filter_map(|s| s.texture.as_ref()) {
                        images.insert(t.rgba.clone(), [t.width, t.height]);
                    }
                    all_shapes.insert(key.clone(), shapes);
                }
                Err(e) => {
                    self.failures.insert(key.clone(), e);
                    continue;
                }
            }
            // Import tables resolve even when the player cannot load the movie yet.
            match MovieDecl::from_json(json) {
                Ok(decl) => {
                    decls.insert(key.clone(), decl);
                }
                Err(e) => {
                    self.failures.insert(key.clone(), e);
                    continue;
                }
            }
            match Movie::load(json) {
                Ok(movie) => {
                    libraries.insert(key.clone(), movie);
                }
                Err(e) => {
                    self.failures.insert(key.clone(), e);
                }
            }
        }
        let (mut total, mut resolved) = (0, 0);
        let mut linked = BTreeMap::new();
        for (key, decl) in &decls {
            total += decl.imports.len();
            let resolution = resolve(key, decl.clone(), &mut decls.clone());
            resolved += resolution.bindings.iter().filter(|b| &b.movie == key).count();
            if self.failures.contains_key(key) {
                continue;
            }
            let json = self.source(key).expect("listed key");
            let result = Movie::load(json).and_then(|mut m| m.link_imports(key, &resolution, &libraries).map(|_| m));
            match result {
                Ok(m) => {
                    linked.insert(key.clone(), m);
                }
                Err(e) => {
                    self.failures.insert(key.clone(), e);
                }
            }
        }
        self.movies = linked;
        self.shapes = all_shapes;
        self.retail_images = images;
        self.imports_total = total;
        self.imports_resolved = resolved;
    }

    /// A mod supplies or replaces the movie `name` (a library key) with its own player-format JSON.
    pub fn set_mod_movie(&mut self, owner: &str, name: &str, movie: Value) -> Result<(), String> {
        let key = library_key(name);
        if !valid_movie_name(&key) {
            return Err(format!("invalid menu movie name {name}"));
        }
        let movie = merged(movie, &self.shared);
        let decl = MovieDecl::from_json(&movie)?;
        Movie::load(&movie)?;
        self.overrides.set(owner, &key, decl);
        self.mods.entry(owner.to_string()).or_default().insert(key, movie);
        self.relink();
        Ok(())
    }

    /// A mod replaces bitmap `bitmap` of movie `name` (stable id `bitmap_id`). It must be a bitmap
    /// character of the current source of that movie (retail or a mod movie).
    pub fn set_mod_bitmap(&mut self, owner: &str, name: &str, bitmap: i32, image: ModBitmap) -> Result<(), String> {
        let key = library_key(name);
        let json = self.source(&key).ok_or_else(|| format!("unknown menu movie {name}"))?;
        let is_bitmap = json["characters"].as_array().is_some_and(|cs| {
            cs.iter().any(|c| c["id"].as_i64() == Some(bitmap.into()) && c["type_name"] == "bitmap")
        });
        if !is_bitmap {
            return Err(format!("{} is not a bitmap", bitmap_id(&key, bitmap)));
        }
        let (w, h) = (image.width, image.height);
        if w == 0
            || h == 0
            || w > MAX_MOD_BITMAP_SIDE
            || h > MAX_MOD_BITMAP_SIDE
            || image.rgba.len() as u64 != u64::from(w) * u64::from(h) * 4
        {
            return Err("menu bitmap size out of range".into());
        }
        self.mod_bitmaps.retain(|(o, k, b, _)| !(o == owner && *k == key && *b == bitmap));
        if self.mod_bitmaps.iter().filter(|(o, ..)| o == owner).count() >= MAX_MOD_BITMAPS {
            return Err(format!("menu bitmap limit reached ({MAX_MOD_BITMAPS} per mod)"));
        }
        self.mod_bitmaps.push((owner.to_string(), key, bitmap, image));
        Ok(())
    }

    /// A mod supplies the runtime image `name` (a retail request path such as `fe\source\images\map\map1024`, or
    /// its own name a world record points at). Same limits as menu bitmaps.
    pub fn set_named_image(&mut self, owner: &str, name: &str, image: ModBitmap) -> Result<(), String> {
        let key = image_key(name);
        let (w, h) = (image.width, image.height);
        if key.is_empty()
            || w == 0
            || h == 0
            || w > MAX_MOD_BITMAP_SIDE
            || h > MAX_MOD_BITMAP_SIDE
            || image.rgba.len() as u64 != u64::from(w) * u64::from(h) * 4
        {
            return Err("menu image size out of range".into());
        }
        self.mod_images.retain(|(o, k, _)| !(o == owner && *k == key));
        if self.mod_images.iter().filter(|(o, ..)| o == owner).count() >= MAX_MOD_BITMAPS {
            return Err(format!("menu image limit reached ({MAX_MOD_BITMAPS} per mod)"));
        }
        self.mod_images.push((owner.to_string(), key, image));
        Ok(())
    }

    /// Whether a runtime image exists (a mod's or the set's).
    /// A movie by key is in the set (retail or a mod's).
    pub fn has_movie(&self, key: &str) -> bool {
        self.base.contains_key(key) || self.mods.values().any(|m| m.contains_key(key))
    }

    pub fn has_named_image(&self, name: &str) -> bool {
        let key = image_key(name);
        self.mod_images.iter().any(|(_, k, _)| *k == key) || self.named_images.contains_key(&key)
    }

    /// Owner of the newest mod bitmap for a stable id.
    fn mod_bitmap_owner(&self, key: &str, bitmap: i32) -> Option<&str> {
        self.mod_bitmaps
            .iter()
            .rev()
            .find(|(_, k, b, _)| k == key && *b == bitmap)
            .map(|(o, ..)| o.as_str())
    }

    /// Shapes and bitmaps for `apt_scene::draw` of a running instance (a clone of
    /// `movies[key]`, which carries the imported shapes' origins).
    pub fn scene<'a>(&'a self, key: &'a str, movie: &'a Movie) -> MenuScene<'a> {
        MenuScene { set: self, key, movie }
    }

    /// The image behind a texture key from `MenuScene`.
    /// The set's shared fonts and language without a movie (engine-drawn panels such as the on-screen keyboard).
    pub fn shared_text_assets(&self) -> Option<crate::apt_text::TextAssets> {
        let mut json = Value::Object(self.shared.clone());
        json["characters"] = Value::Array(Vec::new());
        crate::apt_text::TextAssets::load(&json).ok()
    }

    pub fn image(&self, texture: &str) -> Option<MenuImage<'_>> {
        if let Some(key) = texture.strip_prefix(NAMED_IMAGE_PREFIX) {
            if let Some((.., image)) = self.mod_images.iter().rev().find(|(_, k, _)| k == key) {
                return Some(MenuImage::Mod(image));
            }
            let (path, size) = self.named_images.get(key)?;
            return Some(MenuImage::Retail { path, width: size[0], height: size[1] });
        }
        if let Some(rest) = texture.strip_prefix(MOD_TEXTURE_PREFIX) {
            // mod:<owner>:<movie>#<bitmap>
            let (owner, id) = rest.split_once(':')?;
            let (key, bitmap) = id.rsplit_once('#')?;
            let bitmap: i32 = bitmap.parse().ok()?;
            return self
                .mod_bitmaps
                .iter()
                .rev()
                .find(|(o, k, b, _)| o == owner && k == key && *b == bitmap)
                .map(|(.., image)| MenuImage::Mod(image));
        }
        let (path, size) = self.retail_images.get_key_value(texture)?;
        Some(MenuImage::Retail { path, width: size[0], height: size[1] })
    }

    /// Removes everything `owner` supplied (mod disabled or stopped).
    pub fn clear_owner(&mut self, owner: &str) {
        self.mod_bitmaps.retain(|(o, ..)| o != owner);
        self.mod_images.retain(|(o, ..)| o != owner);
        if self.mods.remove(owner).is_some() {
            self.overrides.clear_owner(owner);
            self.relink();
        }
    }
    pub fn clear_mods(&mut self) {
        let owners: Vec<String> = self.mods.keys().cloned().collect();
        for owner in owners {
            self.overrides.clear_owner(&owner);
        }
        self.mod_bitmaps.clear();
        self.mod_images.clear();
        if !self.mods.is_empty() {
            self.mods.clear();
            self.relink();
        }
    }

    /// `sdk.snapshot.menus.movies`: loaded keys, import counts, failures, mod-supplied keys.
    pub fn snapshot(&self) -> Value {
        let mods: BTreeMap<&String, Vec<&String>> =
            self.mods.iter().map(|(o, m)| (o, m.keys().collect())).collect();
        let mut bitmap_ids = std::collections::BTreeSet::new();
        for (key, shapes) in &self.shapes {
            for b in shapes.values().flatten().filter_map(|s| s.bitmap) {
                bitmap_ids.insert(bitmap_id(key, b));
            }
        }
        let mut mod_bitmaps: BTreeMap<&String, Vec<String>> = BTreeMap::new();
        for (owner, key, b, _) in &self.mod_bitmaps {
            mod_bitmaps.entry(owner).or_default().push(bitmap_id(key, *b));
        }
        serde_json::json!({
            "bitmaps": {"ids": bitmap_ids, "mods": mod_bitmaps},
            "loaded": self.movies.keys().collect::<Vec<_>>(),
            "imports": {"resolved": self.imports_resolved, "total": self.imports_total},
            "failures": self.failures,
            "mods": mods,
        })
    }
}

/// GEO units of a movie JSON (`shapes`, absent = none), keyed by shape character id.
fn shapes_of(json: &Value) -> Result<Shapes, String> {
    if json["shapes"].is_null() {
        return Ok(Shapes::new());
    }
    let shapes: Shapes =
        serde_json::from_value(json["shapes"].clone()).map_err(|e| format!("menu shapes: {e}"))?;
    for unit in shapes.values().flatten() {
        if unit.fill.textured() && (unit.texture.is_none() || unit.bitmap.is_none()) {
            return Err("menu shape textured unit without its bitmap".into());
        }
        let finite = unit
            .triangles
            .iter()
            .flatten()
            .all(|v| v.position.iter().chain(&v.uv).all(|x| x.is_finite()));
        if !finite || !unit.color.iter().all(|x| x.is_finite()) {
            return Err("menu shape with a nonfinite value".into());
        }
    }
    Ok(shapes)
}

/// One linked movie's shapes for `apt_scene::draw`: imported shapes read their GEO units from the
/// library that defines them; textured units draw the newest mod bitmap for their stable id, else
/// the retail payload.
pub struct MenuScene<'a> {
    set: &'a RetailMenuMovies,
    key: &'a str,
    movie: &'a Movie,
}
impl MenuScene<'_> {
    fn origin(&self, character: i32) -> (&str, i32) {
        self.movie
            .shape_origins
            .get(&character)
            .map_or((self.key, character), |(k, id)| (k.as_str(), *id))
    }
}
impl ShapeSource for MenuScene<'_> {
    fn shape(&self, character: i32) -> Option<&[Shape]> {
        let (key, id) = self.origin(character);
        self.set.shapes.get(key)?.get(&id).map(Vec::as_slice)
    }
    fn texture(&self, character: i32, shape: &Shape) -> String {
        let (key, _) = self.origin(character);
        if let Some(bitmap) = shape.bitmap {
            if let Some(owner) = self.set.mod_bitmap_owner(key, bitmap) {
                return format!("{MOD_TEXTURE_PREFIX}{owner}:{}", bitmap_id(key, bitmap));
            }
        }
        shape.texture.as_ref().map_or_else(String::new, |t| t.rgba.clone())
    }
    fn retail_unit_colour(&self) -> bool {
        true
    }
}

/// `source/...` style keys: ASCII letters, digits, `_`, `-`, `/`; no empty or dot segments.
pub fn valid_movie_name(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key.split('/').all(|s| {
            !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// The exported set: `SKATE3_MENU_MOVIES` (a dev export) or the installed `private/menu-movies`.
/// The world records' own map picture names (manifest `world_maps`, version 7; empty for older sets).
pub(crate) fn world_maps(root: &Path) -> BTreeMap<String, String> {
    read_json(&root.join("manifest.json"))
        .ok()
        .and_then(|m| serde_json::from_value(m["world_maps"].clone()).ok())
        .unwrap_or_default()
}

pub(crate) fn set_root(config: &crate::config::Config) -> std::path::PathBuf {
    std::env::var("SKATE3_MENU_MOVIES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| config.asset_root.join("private/menu-movies"))
}

type EarlyRead = (std::path::PathBuf, std::thread::JoinHandle<Result<RetailMenuMovies, String>>);
static EARLY: std::sync::Mutex<Option<EarlyRead>> = std::sync::Mutex::new(None);

/// Begin [`RetailMenuMovies::read`] on its own thread at the top of `main`; [`load`] takes the result at its old place
/// (docs/hails-additions/08-setup-performance.md#menu-movies-read-begins-in-main). `SKATE_EARLY_MENU_MOVIES=0` off.
pub(crate) fn begin_early(config: &crate::config::Config) {
    if std::env::var("SKATE_EARLY_MENU_MOVIES").is_ok_and(|v| v == "0") {
        return;
    }
    let root = set_root(config);
    let read_root = root.clone();
    if let Ok(handle) = std::thread::Builder::new().name("menu-movies-read".into()).spawn(move || RetailMenuMovies::read(&read_root)) {
        if let Ok(mut early) = EARLY.lock() {
            *early = Some((root, handle));
        }
    }
}

pub(crate) fn load(mut commands: Commands, config: Res<crate::config::Config>) {
    let root = set_root(&config);
    let early = EARLY.lock().ok().and_then(|mut e| e.take()).filter(|(r, _)| *r == root);
    let read = early.and_then(|(_, handle)| handle.join().ok()).unwrap_or_else(|| RetailMenuMovies::read(&root));
    let movies = read.unwrap_or_else(|e| {
        warn!("RETAIL_MENUS menu movies unavailable: {e}");
        RetailMenuMovies::default()
    });
    info!(
        "RETAIL_MENUS movies={} imports={}/{}",
        movies.movies.len(),
        movies.imports_resolved,
        movies.imports_total
    );
    for (key, e) in &movies.failures {
        warn!("RETAIL_MENUS movie {key} failed: {e}");
    }
    commands.insert_resource(movies);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apt_vm::{Host, Instruction, Value as AptValue, Vm};

    struct Stub;
    impl Host for Stub {
        fn call(&mut self, _: &mut Vm, _: usize, _: &str, _: Vec<AptValue>) -> Result<AptValue, String> {
            Ok(AptValue::Undefined)
        }
    }

    fn movie(chars: &[i32], imports: Value, exports: Value) -> Value {
        let characters: Vec<Value> = chars
            .iter()
            .map(|id| serde_json::json!({"id": id, "type_name": if *id == 0 {"movie"} else {"sprite"}, "frames": []}))
            .collect();
        serde_json::json!({"characters": characters, "actions": {}, "language": {},
            "imports": imports, "exports": exports})
    }

    fn set() -> RetailMenuMovies {
        let mut out = RetailMenuMovies::default();
        out.base.insert(
            "source/screens/main/menu".into(),
            movie(&[0], serde_json::json!([{"file": "source/controls/lib", "name": "item", "character_id": 5}]), serde_json::json!([])),
        );
        out.base.insert(
            "source/controls/lib".into(),
            movie(&[0, 2], serde_json::json!([]), serde_json::json!([{"name": "item", "character_id": 2}])),
        );
        out.relink();
        out
    }

    fn place(depth: i32, character: i32) -> Value {
        serde_json::json!({"type_name": "place_object2", "flags": 6, "depth": depth, "character_id": character,
            "matrix": [1.0, 0.0, 0.0, 1.0, 10.0, 0.0], "color_transform": [255, 255, 255, 255, 0, 0, 0, 0],
            "clip_depth": -1})
    }

    /// A menu importing a library sprite that places one shape with a solid and a textured unit.
    fn textured_set() -> RetailMenuMovies {
        let v = |x: f32, y: f32| serde_json::json!({"position": [x, y], "uv": [x / 4., y / 4.]});
        let tri = serde_json::json!([[v(0., 0.), v(4., 0.), v(4., 4.)]]);
        let lib = serde_json::json!({
            "characters": [
                {"id": 0, "type_name": "movie", "frames": []},
                {"id": 2, "type_name": "sprite", "frames": [{"controls": [place(1, 3)]}]},
                {"id": 3, "type_name": "shape"},
                {"id": 4, "type_name": "bitmap", "bitmap": {"texture_id": 4}},
            ],
            "shapes": {"3": [
                {"render_type": "solid", "color": [1.0, 0.0, 0.0, 1.0], "texture": null,
                 "triangles": [[{"position": [0.0, 0.0]}, {"position": [1.0, 0.0]}, {"position": [1.0, 1.0]}]]},
                {"render_type": "texture_clamped", "color": [1.0, 1.0, 1.0, 0.5], "bitmap": 4,
                 "texture": {"width": 4, "height": 4, "rgba": "lib/4.rgba"}, "triangles": tri},
            ]},
            "actions": {}, "language": {}, "imports": [], "exports": [{"name": "item", "character_id": 2}]});
        let menu = serde_json::json!({
            "characters": [{"id": 0, "type_name": "movie", "frames": [{"controls": [place(1, 5)]}]}],
            "actions": {}, "language": {}, "exports": [],
            "imports": [{"file": "source/controls/lib", "name": "item", "character_id": 5}]});
        let mut out = RetailMenuMovies::default();
        out.base.insert("source/screens/main/menu".into(), menu);
        out.base.insert("source/controls/lib".into(), lib);
        out.relink();
        out
    }

    fn draw(m: &RetailMenuMovies, key: &str) -> Vec<crate::apt_scene::Draw> {
        let mut movie = m.movies[key].clone();
        let mut vm = Vm::new();
        movie.initialize(&mut vm).unwrap();
        crate::apt_scene::draw(&movie, &vm, &m.scene(key, &movie)).unwrap()
    }

    #[test]
    fn menu_bitmaps_draw_through_imports_and_mods_replace_then_revert() {
        use crate::apt_scene::Fill;
        let mut m = textured_set();
        assert!(m.failures.is_empty(), "{:?}", m.failures);
        let key = "source/screens/main/menu";
        let draws = draw(&m, key);
        assert_eq!(draws.len(), 2);
        // Solid unit: colour only. Textured unit: the library's payload, its UVs, unit colour.
        assert_eq!((draws[0].fill, draws[0].texture.as_str()), (Fill::Solid, ""));
        assert_eq!(draws[0].multiply, [1., 0., 0., 1.]);
        assert_eq!((draws[1].fill, draws[1].texture.as_str()), (Fill::TextureClamped, "lib/4.rgba"));
        assert_eq!(draws[1].vertices[1].uv, [1., 0.]);
        assert_eq!(draws[1].vertices[1].position, [24., 0.]); // two placements at x 10
        assert_eq!(draws[1].multiply[3], 0.5);
        assert_eq!(
            m.image("lib/4.rgba"),
            Some(MenuImage::Retail { path: "lib/4.rgba", width: 4, height: 4 })
        );
        assert_eq!(m.snapshot()["bitmaps"]["ids"][0], "source/controls/lib#4");
        // A mod replaces the bitmap by its stable id (the library's, also when drawn via the menu).
        let image = ModBitmap { width: 1, height: 1, rgba: vec![1, 2, 3, 4] };
        assert!(m.set_mod_bitmap("a", "source/controls/lib", 3, image.clone()).is_err()); // a shape
        assert!(m.set_mod_bitmap("a", "source/controls/nope", 4, image.clone()).is_err());
        let bad = ModBitmap { width: 2, height: 1, rgba: vec![0; 4] };
        assert!(m.set_mod_bitmap("a", "source/controls/lib", 4, bad).is_err());
        m.set_mod_bitmap("a", "data/fe/source/controls/lib.apt", 4, image.clone()).unwrap();
        let texture = draw(&m, key)[1].texture.clone();
        assert_eq!(texture, "mod:a:source/controls/lib#4");
        assert_eq!(m.image(&texture), Some(MenuImage::Mod(&image)));
        assert_eq!(m.snapshot()["bitmaps"]["mods"]["a"][0], "source/controls/lib#4");
        // Newest owner wins; stopping it falls back to the older mod, then retail.
        m.set_mod_bitmap("b", "source/controls/lib", 4, image.clone()).unwrap();
        assert_eq!(draw(&m, key)[1].texture, "mod:b:source/controls/lib#4");
        m.clear_owner("b");
        assert_eq!(draw(&m, key)[1].texture, "mod:a:source/controls/lib#4");
        m.clear_mods();
        assert_eq!(draw(&m, key)[1].texture, "lib/4.rgba");
        assert!(m.image("mod:a:source/controls/lib#4").is_none());
    }

    #[test]
    fn menu_units_follow_the_retail_colour_rule() {
        let mut m = textured_set();
        let lib = m.base.get_mut("source/controls/lib").unwrap();
        // A line unit (type 0) is never drawn; a unit whose colour alpha ends <= 0 is skipped.
        let line = serde_json::json!({"render_type": "line", "color": [1.0, 1.0, 1.0, 1.0], "texture": null,
            "triangles": [[{"position": [0.0, 0.0]}, {"position": [1.0, 0.0]}, {"position": [1.0, 1.0]}]]});
        lib["shapes"]["3"].as_array_mut().unwrap().push(line);
        lib["shapes"]["3"][0]["color"] = serde_json::json!([1.0, 0.0, 0.0, 0.0]);
        // The library sprite's placement adds 0.25 red and multiplies alpha by 0.5.
        lib["characters"][1]["frames"][0]["controls"][0]["flags"] = serde_json::json!(14); // + colour
        lib["characters"][1]["frames"][0]["controls"][0]["color_transform"] =
            serde_json::json!([128, 255, 255, 255, 0, 64, 0, 0]); // [a, r, g, b] multiply, then add
        m.relink();
        let draws = draw(&m, "source/screens/main/menu");
        assert_eq!(draws.len(), 1);
        // colour = unit colour * multiply + add, folded into one colour (no separate add).
        let d = &draws[0];
        assert_eq!(d.add, [0.; 4]);
        assert!((d.multiply[0] - (1. + 64. / 255.)).abs() < 1e-6, "{:?}", d.multiply);
        assert!((d.multiply[3] - 0.5 * 128. / 255.).abs() < 1e-6);
    }

    #[test]
    fn menu_shapes_reject_textured_units_without_bitmap() {
        let mut m = textured_set();
        let lib = m.base.get_mut("source/controls/lib").unwrap();
        lib["shapes"]["3"][1]["bitmap"] = Value::Null;
        m.relink();
        assert!(m.failures.contains_key("source/controls/lib"));
    }

    #[test]
    fn menu_movies_link_and_mods_replace_then_revert() {
        let mut m = set();
        assert_eq!((m.movies.len(), m.imports_resolved, m.imports_total), (2, 1, 1));
        assert!(m.failures.is_empty(), "{:?}", m.failures);
        assert!(m.movies["source/screens/main/menu"].characters.contains_key(&5));
        // A mod library without the export: the import goes unresolved (retail: slot null).
        let empty = movie(&[0], serde_json::json!([]), serde_json::json!([]));
        m.set_mod_movie("a", "data/fe/source/controls/lib.apt", empty).unwrap();
        assert_eq!(m.imports_resolved, 0);
        assert!(!m.movies["source/screens/main/menu"].characters.contains_key(&5));
        assert_eq!(m.snapshot()["mods"]["a"][0], "source/controls/lib");
        m.clear_owner("a");
        assert_eq!(m.imports_resolved, 1);
        assert!(m.set_mod_movie("a", "../x", movie(&[0], serde_json::json!([]), serde_json::json!([]))).is_err());
        assert!(m.mods.is_empty());
    }

    /// Version 7's named runtime images: the manifest table by request key, mods replace or add names and revert.
    #[test]
    fn named_images_resolve_by_request_key_and_mods_replace_then_revert() {
        assert_eq!(image_key("fe\\source\\images\\map\\MAP1024"), "fe/source/images/map/map1024");
        assert_eq!(image_key("data/fe/source/images/map/map1024.rx2"), "fe/source/images/map/map1024");
        let root = std::env::temp_dir().join(format!("skate3-named-images-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let manifest = |path: &str| {
            serde_json::json!({"format": FORMAT, "version": 7, "movies": {}, "shared": "shared.json",
                "images": {"fe/source/images/map/map1024": {"rgba": path, "width": 4, "height": 2}}})
        };
        std::fs::write(root.join("shared.json"), "{}").unwrap();
        std::fs::write(root.join("manifest.json"), manifest("a/0000_map1024.Texture.rgba").to_string()).unwrap();
        let mut m = RetailMenuMovies::read(&root).unwrap();
        let key = format!("{NAMED_IMAGE_PREFIX}fe/source/images/map/map1024");
        assert!(matches!(m.image(&key), Some(MenuImage::Retail { path: "a/0000_map1024.Texture.rgba", width: 4, height: 2 })));
        assert!(m.image("image/fe/source/images/map/nothing").is_none());
        assert!(m.has_named_image("fe\\source\\images\\map\\map1024"));
        let image = ModBitmap { width: 1, height: 1, rgba: vec![255; 4] };
        assert!(m.set_named_image("mod", "fe/source/images/map/map1024", ModBitmap { width: 2, ..image.clone() }).is_err(), "size check");
        m.set_named_image("mod", "fe\\source\\images\\map\\map1024", image.clone()).unwrap();
        m.set_named_image("mod", "mymod/maps/park", image).unwrap();
        assert!(matches!(m.image(&key), Some(MenuImage::Mod(b)) if b.width == 1));
        assert!(m.image("image/mymod/maps/park").is_some());
        m.clear_owner("mod");
        assert!(matches!(m.image(&key), Some(MenuImage::Retail { .. })));
        assert!(m.image("image/mymod/maps/park").is_none());
        std::fs::write(root.join("manifest.json"), manifest("../escape.rgba").to_string()).unwrap();
        assert!(RetailMenuMovies::read(&root).is_err(), "a path out of the set is refused");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_set_is_empty() {
        let m = RetailMenuMovies::read(Path::new("does/not/exist")).unwrap();
        assert!(m.movies.is_empty());
    }

    /// Loads the set setup exported (SKATE3_MENU_MOVIES, else the main checkout's installed
    /// `assets/private/menu-movies`, found from a worktree too). Skips when absent.
    #[test]
    fn retail_menu_movie_set_loads_links_and_runs() {
        let Some(root) = crate::apt_imports::test_menu_set() else {
            return;
        };
        let m = RetailMenuMovies::read(&root).unwrap();
        // After M3e (doc 31 "Milestone 3e") every movie loads; any failure is a regression.
        for (key, e) in &m.failures {
            eprintln!("open player gap: {key}: {e}");
            panic!("{key}: {e}");
        }
        assert!(m.base.len() >= 20);
        assert!(!m.movies.is_empty());
        assert_eq!(m.imports_resolved, m.imports_total);
        assert!(m.imports_total > 0);
        fn walk(code: &[Instruction], out: &mut std::collections::BTreeSet<u8>) {
            for i in code {
                out.insert(i.opcode);
                walk(&i.body, out);
            }
        }
        let mut ops = std::collections::BTreeSet::new();
        // Every exported movie, loadable by the player yet or not.
        for json in m.base.values() {
            let streams: BTreeMap<String, Vec<Instruction>> =
                serde_json::from_value(json["actions"].clone()).unwrap();
            for code in streams.values() {
                walk(code, &mut ops);
            }
        }
        for op in ops {
            let code: Vec<Instruction> =
                vec![serde_json::from_value(serde_json::json!({"offset": 0, "opcode": op})).unwrap()];
            let mut vm = Vm::new();
            vm.begin_update();
            if let Err(e) = vm.run(&code, &mut Stub) {
                assert!(!e.contains("Unsupported APT opcode"), "{op:02x}: {e}");
            }
        }
        // Doc 31, Milestone 3f: no retail menu placement carries a clip depth (all -1, flag
        // 0x40 never set), so masks never matter for these movies.
        fn placements(v: &serde_json::Value, count: &mut usize, clip: &mut usize) {
            match v {
                serde_json::Value::Object(o) => {
                    if o.get("type_name")
                        .and_then(|t| t.as_str())
                        .is_some_and(|t| t.starts_with("place_object"))
                    {
                        *count += 1;
                        let depth = o.get("clip_depth").and_then(|d| d.as_i64()).unwrap_or(-1);
                        let flags = o.get("flags").and_then(|f| f.as_u64()).unwrap_or(0);
                        if depth as i16 >= 0 || flags & 0x40 != 0 {
                            *clip += 1;
                        }
                    }
                    o.values().for_each(|v| placements(v, count, clip));
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| placements(v, count, clip)),
                _ => {}
            }
        }
        let (mut count, mut clip) = (0, 0);
        m.base.values().for_each(|json| placements(json, &mut count, &mut clip));
        assert!(count > 0);
        assert_eq!(clip, 0, "a retail menu movie uses clip depth: see doc 31 Milestone 3f");
        // Doc 31, Milestone 4: every shape (local or imported) of every linked movie resolves its
        // GEO units; every textured unit names a bitmap character and an exported payload of the
        // right size. Then each movie's first frame draws through the scene.
        let (mut units, mut textured, mut solid, mut drawn, mut drawn_bitmaps) = (0, 0, 0, 0, 0);
        let mut bitmaps = std::collections::BTreeSet::new();
        for (key, movie) in &m.movies {
            let scene = m.scene(key, movie);
            for c in movie.characters.values().filter(|c| c.type_name == "shape") {
                let shape = scene.shape(c.id).unwrap_or_else(|| panic!("{key} shape {}", c.id));
                for unit in shape {
                    units += 1;
                    if unit.fill.textured() {
                        textured += 1;
                        let texture = scene.texture(c.id, unit);
                        let Some(MenuImage::Retail { path, width, height }) = m.image(&texture) else {
                            panic!("{key} shape {}: no image {texture}", c.id)
                        };
                        let bytes = std::fs::metadata(root.join(path)).unwrap().len();
                        assert_eq!(bytes, u64::from(width) * u64::from(height) * 4, "{path}");
                        let origin = movie.shape_origins.get(&c.id).map_or(key.as_str(), |(k, _)| k);
                        bitmaps.insert(bitmap_id(origin, unit.bitmap.unwrap()));
                    } else {
                        solid += 1;
                    }
                }
            }
            let mut running = movie.clone();
            let mut vm = Vm::new();
            if running.initialize(&mut vm).is_ok() {
                let draws = crate::apt_scene::draw(&running, &vm, &m.scene(key, &running))
                    .unwrap_or_else(|e| panic!("{key}: {e}"));
                drawn += draws.len();
                drawn_bitmaps += draws.iter().filter(|d| d.texture.contains(".Texture")).count();
            }
        }
        let ids = m.snapshot()["bitmaps"]["ids"].as_array().unwrap().len();
        assert!(textured > 0 && ids >= bitmaps.len());
        eprintln!(
            "RETAIL_MENUS movies={} of {} imports={}/{} placements={count} clip_depth={clip} \
             shape_units={units} textured={textured} solid={solid} bitmaps_used={} bitmap_ids={ids} \
             frame0_draws={drawn} frame0_bitmap_draws={drawn_bitmaps}",
            m.movies.len(),
            m.base.len(),
            m.imports_resolved,
            m.imports_total,
            bitmaps.len()
        );
    }
}
