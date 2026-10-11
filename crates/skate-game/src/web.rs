//! The browser build's process-level differences (docs/web-port). A browser tab has no updater,
//! crash supervisor, setup tool modes or trace file, and no installation folder beside an exe:
//! the player chooses their own converted installation on the page (`web/installation.js`), and
//! `skate_fs` serves it.
//!
//! Startup reads synchronously, and a page can only read the player's disk asynchronously. So
//! startup first reads ahead what it needs ([`startup_files`], then the start map), then runs
//! the same startup as natively (`crate::run`). A read that finds a file was not read ahead
//! fails startup cleanly; it is read and startup starts again ([`failed`]).
use bevy::{
    asset::io::{AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader, VecReader},
    prelude::*,
    render::{
        RenderPlugin,
        renderer::RenderAdapterInfo,
        settings::{Backends, RenderCreation, WgpuSettings},
    },
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

/// The canvas the loader page provides (`web/index.html`).
pub(crate) const CANVAS: &str = "#skate3";

/// WebGPU only: the renderer's storage-buffer and compute use has no WebGL2 fallback.
pub(crate) const BACKENDS: Backends = Backends::BROWSER_WEBGPU;

/// Startup's exit code when a read wanted a file that was not read ahead: read it, start again.
const READ_AND_RETRY: u8 = 75;
/// More would mean startup keeps finding new files one at a time: a missing read-ahead rule.
const ATTEMPTS: usize = 12;

#[wasm_bindgen]
extern "C" {
    /// Before an app exists there is no logger (Bevy's `LogPlugin` installs it), so startup's own
    /// lines go to the console directly.
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn console_log(text: &str);
    /// A line of progress for the page's overlay.
    #[wasm_bindgen(catch, js_namespace = skatePage, js_name = status)]
    fn page_status(text: &str) -> Result<(), JsValue>;
    /// The game is drawing: the page hides its overlay.
    #[wasm_bindgen(catch, js_namespace = skatePage, js_name = running)]
    fn page_running() -> Result<(), JsValue>;
    /// Startup stopped, or the engine panicked: the page shows why.
    #[wasm_bindgen(catch, js_namespace = skatePage, js_name = stopped)]
    fn page_stopped(message: &str) -> Result<(), JsValue>;
}

fn status(text: &str) {
    console_log(&format!("SKATE_WEB_STATUS {text}"));
    let _ = page_status(text);
}

pub(crate) fn window(title: String) -> Window {
    Window {
        title,
        canvas: Some(CANVAS.into()),
        fit_canvas_to_parent: true,
        // Keys such as Tab and the arrows drive the menu, not the page.
        prevent_default_event_handling: true,
        ..default()
    }
}

/// `main` in the browser: mounts the chosen installation, reads ahead, then starts the game. The
/// work continues on the page's event loop after this returns.
pub(crate) fn start() -> AppExit {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = page_stopped(&format!("The engine stopped: {info}"));
        previous(info);
    }));
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = read_ahead().await {
            stopped(error);
            return;
        }
        status("Starting the engine…");
        for attempt in 1..=ATTEMPTS {
            let exit = crate::run();
            if exit != AppExit::from_code(READ_AND_RETRY) {
                return;
            }
            let missed = skate_fs::take_misses();
            console_log(&format!("SKATE_WEB_READ_AHEAD attempt={attempt} files={}", missed.len()));
            for path in &missed {
                console_log(&format!("SKATE_WEB_READ_AHEAD {}", path.display()));
            }
            if let Err(error) = skate_fs::prefetch(&missed).await {
                stopped(format!("Could not read your installation: {error}"));
                return;
            }
        }
        stopped("Startup kept asking for files it had not read ahead (see SKATE_WEB_READ_AHEAD in the console)".into());
    });
    AppExit::Success
}

/// A startup error (`crate::failed`): start again if a read wanted a file that can be read
/// ahead, else show it.
pub(crate) fn failed(error: String) -> AppExit {
    if skate_fs::missed() {
        console_log(&format!("SKATE_WEB_RETRY {error}"));
        return AppExit::from_code(READ_AND_RETRY);
    }
    stopped(error)
}

/// The installation `start` mounted (`setup::asset_root` in the browser).
pub(crate) fn asset_root() -> Result<PathBuf, String> {
    let assets = Path::new(skate_fs::ROOT).join("assets");
    if !skate_fs::is_file(assets.join("private/game.json")) {
        return Err("That folder is not a converted installation: it has no assets/private/game.json.".into());
    }
    Ok(assets)
}

async fn read_ahead() -> Result<(), String> {
    let root = skate_fs::mount()?;
    let assets = asset_root()?;
    skate_fs::prefetch(startup_files(&root)).await.map_err(|e| format!("Could not read your installation: {e}"))?;
    // The settings name the start map.
    let map = crate::map_library::default_map(&assets)?;
    if let Some(map) = &map {
        status(&format!("Reading {}…", map.file_name().unwrap_or_default().to_string_lossy()));
    }
    skate_fs::prefetch(map_files(&assets, map.as_deref())).await.map_err(|e| format!("Could not read your installation: {e}"))
}

/// What startup reads besides the map: the settings, and the configuration, stock, menu and HUD
/// files under `assets/private`. Large data is read when it is needed: maps, audio, the
/// customiser's models and textures (the asset server reads those without blocking).
pub(crate) fn startup_files(root: &Path) -> Vec<PathBuf> {
    let settings = root.join("settings");
    let private = root.join("assets/private");
    skate_fs::files_under(root)
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| path.starts_with(&settings) || path.strip_prefix(&private).is_ok_and(read_at_startup))
        .collect()
}

/// Stock files startup reads (`skate_data` loaders, the camera and gesture input).
const STOCK: [&str; 8] = [
    "stock/data/config",
    "stock/data/state",
    "stock/data/anim/OnBoard.abin",
    "stock/data/anim/OffBoard.abin",
    "stock/data/joystick",
    "stock/data/script/camera",
    "stock/data/camera",
    "stock/data/cacrecipes",
];

/// `path` is relative to `assets/private`.
fn read_at_startup(path: &Path) -> bool {
    let json = path.extension().is_some_and(|e| e == "json");
    match path.iter().next().and_then(|top| top.to_str()).unwrap_or_default() {
        "stock" => STOCK.iter().any(|stock| path.starts_with(stock)) || (json && path.parent() == Some(Path::new("stock"))),
        // The audio runtime's projects, banks, grains and wheels (about 120 MB) load at once at
        // its start; the ambience beds (about 550 MB) and decoded speech are read when they play.
        "audio" => !path.starts_with("audio/ambience") && !(path.starts_with("audio/speech") && path.extension().is_some_and(|e| e == "wav")),
        // Per map: read with the map (`map_files`), apart from the small descriptions.
        "native-skies" => json,
        "native-lighting" | "native-props" | "native-backdrops" => false,
        "menu-movies" | "hud" | "session-marker" | "particles" | "custom" => true,
        _ => json,
    }
}

/// A map and what is read with it: its sidecars and its sky and lighting under `assets`.
pub(crate) fn map_files(assets: &Path, map: Option<&Path>) -> Vec<PathBuf> {
    let Some(map) = map else { return Vec::new() };
    let stem = map.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    let mut files = vec![map.to_path_buf()];
    files.extend(["triggers", "irradiance", "audio.json"].map(|extension| map.with_extension(extension)));
    files.push(assets.join("private/native-lighting").join(format!("{stem}.irradiance")));
    // Skies are named by the map's own name, usually its file name.
    files.extend(
        skate_fs::files_under(assets.join("private/native-skies"))
            .into_iter()
            .map(|(path, _)| path)
            .filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().to_lowercase().starts_with(&format!("{stem}.")))),
    );
    files.retain(|file| skate_fs::is_file(file));
    files
}

/// After a map change: the page drops the files of the map left, unless the new one uses them.
pub(crate) fn release_map(assets: &Path, left: Option<&Path>, current: Option<&Path>) {
    let keep = map_files(assets, current);
    for file in map_files(assets, left).into_iter().filter(|file| !keep.contains(file)) {
        skate_fs::release(file);
    }
}

/// Files being read in the background for [`read_later`].
static READING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
/// Files [`read_later`] fetched, which the page can drop once they are taken: nothing reads one of
/// these big files twice.
static READ_LATER: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// A synchronous read found `path` was not read ahead (`WouldBlock`): read it in the background so
/// a later read finds it. What plays from disk (ambience beds, speech) waits for it a few frames.
pub(crate) fn read_later(path: &Path) {
    if !READING.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_owned()) {
        return;
    }
    READ_LATER.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_owned());
    let path = path.to_owned();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = skate_fs::prefetch([&path]).await {
            warn!("{}: {error}", path.display());
        }
        READING.lock().unwrap_or_else(|e| e.into_inner()).remove(&path);
    });
}

/// Whether `path` is still being read for [`read_later`].
pub(crate) fn reading(path: &Path) -> bool {
    READING.lock().unwrap_or_else(|e| e.into_inner()).contains(path)
}

/// The caller has `path`'s bytes: a file read for [`read_later`] need not stay in the page.
pub(crate) fn took(path: &Path) {
    if READ_LATER.lock().unwrap_or_else(|e| e.into_inner()).remove(path) {
        skate_fs::release(path);
    }
}

/// Work the native build runs on a thread. The browser has none: the job reads its files ahead,
/// then runs on the main thread between frames.
pub(crate) struct Job<T>(Arc<Mutex<Option<T>>>);

impl<T: 'static> Job<T> {
    pub(crate) fn spawn(files: Vec<PathBuf>, work: impl FnOnce() -> T + 'static) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let done = slot.clone();
        wasm_bindgen_futures::spawn_local(async move {
            // A file that could not be read fails the work itself, with its name.
            if let Err(error) = skate_fs::prefetch(&files).await {
                warn!("{error}");
            }
            *done.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(work());
        });
        Self(slot)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some()
    }

    /// The result, once [`Job::is_finished`].
    pub(crate) fn join(self) -> std::thread::Result<T> {
        let result = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        result.ok_or_else(|| Box::new("joined an unfinished job") as Box<dyn std::any::Any + Send>)
    }
}

/// The asset server's default source: the installation's `assets`, read from the player's disk
/// without blocking. Installations carry no `.meta` files.
struct InstallationReader(PathBuf);

impl AssetReader for InstallationReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full = self.0.join(path);
        if !skate_fs::is_file(&full) {
            return Err(AssetReaderError::NotFound(path.to_owned()));
        }
        let bytes = skate_fs::read_async(&full).await.map_err(|e| AssetReaderError::Io(Arc::new(e)))?;
        Ok(VecReader::new(bytes))
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_owned()))
    }

    async fn read_directory<'a>(&'a self, path: &'a Path) -> Result<Box<PathStream>, AssetReaderError> {
        let entries = skate_fs::read_dir(self.0.join(path)).map_err(|_| AssetReaderError::NotFound(path.to_owned()))?;
        let names: Vec<_> = entries.flatten().filter_map(|entry| Some(path.join(entry.path().file_name()?))).collect();
        Ok(Box::new(bevy::tasks::futures_lite::stream::iter(names)))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(skate_fs::is_dir(self.0.join(path)))
    }
}

/// Before `DefaultPlugins`: serves the default asset source from the installation, in place of
/// Bevy's HTTP reader.
pub(crate) fn register_installation_source(app: &mut App, assets: &Path) {
    let assets = assets.to_owned();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(InstallationReader(assets.clone()))),
    );
}

#[derive(Resource)]
pub(crate) struct Stopped(String);

/// Shows why startup stopped, on the page and the canvas (logged as `SKATE_WEB_STOPPED`), then
/// idles. Logs `SKATE_WEB_READY` once a WebGPU adapter is up; `tools/web-port/smoke.mjs` waits
/// for both.
pub(crate) fn stopped(message: String) -> AppExit {
    let _ = page_stopped(&message);
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin { primary_window: Some(window("Skate 3 Rust Engine".into())), ..default() })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings { backends: Some(BACKENDS), ..default() }),
                    ..default()
                })
                .disable::<bevy::gilrs::GilrsPlugin>(),
        )
        .insert_resource(Stopped(message))
        .add_systems(Startup, show)
        .add_systems(Update, (announce, list_controllers))
        .run()
}

/// The stop screen's controller line: the pads the browser exposes, so a player can check one
/// before the game itself starts.
#[derive(Component)]
struct ControllerList;

fn list_controllers(mut text: Query<&mut Text, With<ControllerList>>) {
    let pads: Vec<String> = (0..4)
        .filter_map(|slot| crate::input::platform::poll(slot).ok().and_then(|packet| Some(format!("{slot}: {}", packet.kind?.summary()))))
        .collect();
    let line = if pads.is_empty() {
        "No controller yet: press a button on one.".to_string()
    } else {
        format!("Controllers {}", pads.join("; "))
    };
    for mut text in &mut text {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}

fn show(mut commands: Commands, stopped: Res<Stopped>) {
    info!("SKATE_WEB_STOPPED {}", stopped.0);
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(16),
            ..default()
        })
        .with_children(|page| {
            page.spawn((Text::new("SKATE / 3"), TextFont { font_size: 48., ..default() }, TextColor(Color::srgb(0.78, 0.96, 0.3))));
            page.spawn((Text::new(stopped.0.clone()), TextFont { font_size: 18., ..default() }, TextColor(Color::WHITE)));
            page.spawn((ControllerList, Text::default(), TextFont { font_size: 14., ..default() }, TextColor(Color::srgb(0.55, 0.62, 0.62))));
        });
}

/// Logs `SKATE_WEB_READY` once the WebGPU adapter is up, and lets the page show the canvas.
pub(crate) fn announce(info: Option<Res<RenderAdapterInfo>>, stopped: Option<Res<Stopped>>, mut done: Local<bool>) {
    if let Some(info) = info.filter(|_| !*done) {
        info!("SKATE_WEB_READY adapter={:?} backend={:?}", info.name, info.backend);
        if stopped.is_none() {
            let _ = page_running();
            let missed = skate_fs::take_misses();
            if !missed.is_empty() {
                warn!("SKATE_WEB_READ_AHEAD startup read past what was read ahead: {missed:?}");
            }
        }
        *done = true;
    }
}
