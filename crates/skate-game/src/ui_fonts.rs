//! Fonts from the operating system for the game's own UI text (user ask 2026-11-10, first user: the music caption),
//! on every OS: no fonts are bundled.
//! - The UI font: one regular system face (Segoe UI on Windows, San Francisco / Helvetica on macOS, Noto Sans /
//!   DejaVu Sans on Linux), read at startup into a Bevy `Font` so UI text does not fall back to Bevy's monospace.
//! - Script fallback: the text engine (cosmic-text inside Bevy) only knows the fonts it was given; its fallback looks
//!   for each script's system fonts by family name. The first time a text needs a script beyond Latin, the system's
//!   script fonts are read on a worker thread and added to the text engine's database; text that waits for them asks
//!   [`FallbackFonts::ready`].
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::text::CosmicFontSystem;

/// Windows: the files whose families cosmic-text's Windows fallback names, one per script group (2026-11-10: the full
/// list was 120 MB in memory): Latin / Greek / Cyrillic / Arabic / Hebrew (Segoe UI), symbols and emoji, Chinese with
/// Japanese kana (Microsoft YaHei), Japanese (Yu Gothic), Korean (Malgun Gothic), Indic (Nirmala UI), Thai and Lao
/// (Leelawadee UI), African scripts (Ebrima), Cherokee and others (Gadugi), Myanmar.
const WINDOWS_FILES: &[&str] = &[
    "segoeui.ttf", "seguisym.ttf", "seguiemj.ttf", "msyh.ttc", "YuGothR.ttc", "malgun.ttf", "Nirmala.ttc", "Nirmala.ttf",
    "LeelawUI.ttf", "ebrima.ttf", "gadugi.ttf", "mmrtext.ttf",
];

/// Linux / macOS: font files whose names hold one of these (the families cosmic-text's fallback names there: Noto for
/// every script and emoji, DejaVu, and the macOS script faces), under the system and user font folders.
const UNIX_NAMES: &[&str] = &[
    "NotoSans", "Noto Sans", "NotoSerifCJK", "NotoColorEmoji", "DejaVuSans", "Hiragino", "PingFang", "AppleSDGothic",
    "Apple Color Emoji", "AppleGothic", "Arial Unicode", "Thonburi", "Kohinoor", "Geeza", "STHeiti",
];
/// The script fallback reads at most this much (a full Noto install is several hundred MB).
const UNIX_BUDGET: u64 = 160 << 20;

fn windows_fonts() -> PathBuf {
    std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts")
}

fn unix_font_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["/usr/share/fonts", "/usr/local/share/fonts", "/System/Library/Fonts", "/Library/Fonts"]
        .iter().map(PathBuf::from).collect();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.extend([home.join(".local/share/fonts"), home.join(".fonts"), home.join("Library/Fonts")]);
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        dirs.push(data.join("fonts"));
    }
    dirs
}

/// The UI font candidates, best first.
fn ui_font_candidates() -> Vec<PathBuf> {
    if cfg!(windows) {
        return vec![windows_fonts().join("segoeui.ttf"), windows_fonts().join("arial.ttf")];
    }
    let mut out: Vec<PathBuf> = ["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf"]
        .iter().map(PathBuf::from).collect();
    for dir in ["/usr/share/fonts/truetype/noto", "/usr/share/fonts/noto", "/usr/share/fonts/google-noto", "/usr/share/fonts/google-noto-vf"] {
        out.push(Path::new(dir).join("NotoSans-Regular.ttf"));
    }
    for dir in ["/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/TTF", "/usr/share/fonts/dejavu", "/usr/share/fonts/dejavu-sans-fonts"] {
        out.push(Path::new(dir).join("DejaVuSans.ttf"));
    }
    out
}

/// Every font file under `dir` (depth-limited) whose name matches the Unix script list.
fn collect_unix(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                collect_unix(&path, depth - 1, out);
            }
            continue;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        let regular = !["Bold", "Italic", "Light", "Thin", "Black", "Medium", "Semi", "Extra", "Condensed"].iter().any(|w| name.contains(w));
        if matches!(ext.as_str(), "ttf" | "otf" | "ttc") && regular && UNIX_NAMES.iter().any(|n| name.contains(n)) {
            out.push(path);
        }
    }
}

/// The script fallback files for this OS.
fn fallback_files() -> Vec<PathBuf> {
    if cfg!(windows) {
        let dir = windows_fonts();
        return WINDOWS_FILES.iter().map(|f| dir.join(f)).collect();
    }
    let mut files = Vec::new();
    for dir in unix_font_dirs() {
        collect_unix(&dir, 4, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

enum State {
    Idle,
    Loading(Arc<Mutex<Option<Vec<Vec<u8>>>>>),
    Ready,
}

#[derive(Resource)]
pub(crate) struct FallbackFonts {
    state: State,
    /// The system UI font (None: none found, Bevy's default is used).
    pub ui: Option<Handle<Font>>,
}

impl Default for FallbackFonts {
    fn default() -> Self {
        Self { state: State::Idle, ui: None }
    }
}

/// Whether `text` uses characters past Latin Extended (other scripts, symbols, emoji).
pub(crate) fn needs_fallback(text: &str) -> bool {
    text.chars().any(|c| c as u32 > 0x024F && !c.is_whitespace())
}

impl FallbackFonts {
    pub(crate) fn ready(&self) -> bool {
        matches!(self.state, State::Ready)
    }

    /// The UI font for a `TextFont` (Bevy's default when none was found).
    pub(crate) fn ui_font(&self) -> Handle<Font> {
        self.ui.clone().unwrap_or_default()
    }

    /// Starts reading the script fonts (once).
    pub(crate) fn request(&mut self) {
        if !matches!(self.state, State::Idle) {
            return;
        }
        let cell = Arc::new(Mutex::new(None));
        let out = cell.clone();
        let spawned = std::thread::Builder::new().name("fallback-fonts".into()).spawn(move || {
            let mut total = 0u64;
            let fonts: Vec<Vec<u8>> = fallback_files().into_iter().filter_map(|f| {
                let size = std::fs::metadata(&f).ok()?.len();
                if !cfg!(windows) && total + size > UNIX_BUDGET {
                    return None;
                }
                total += size;
                std::fs::read(&f).ok()
            }).collect();
            if let Ok(mut o) = out.lock() {
                *o = Some(fonts);
            }
        });
        self.state = if spawned.is_ok() { State::Loading(cell) } else { State::Ready };
    }
}

/// Startup: the system UI font (one small file).
pub(crate) fn load_ui_font(mut fallback: ResMut<FallbackFonts>, fonts: Option<ResMut<Assets<Font>>>) {
    let Some(mut fonts) = fonts else { return };
    for path in ui_font_candidates() {
        if let Some(font) = std::fs::read(&path).ok().and_then(|data| Font::try_from_bytes(data).ok()) {
            info!("UI_FONTS ui font {}", path.display());
            fallback.ui = Some(fonts.add(font));
            return;
        }
    }
    info!("UI_FONTS no system ui font found; Bevy's default is used");
}

/// Adds the read script fonts to the text engine's database when they are in.
pub(crate) fn install(mut fallback: ResMut<FallbackFonts>, font_system: Option<ResMut<CosmicFontSystem>>) {
    let State::Loading(cell) = &fallback.state else { return };
    let Some(fonts) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
    if let Some(mut system) = font_system {
        let (count, bytes) = (fonts.len(), fonts.iter().map(Vec::len).sum::<usize>());
        let db = system.0.db_mut();
        for data in fonts {
            db.load_font_data(data);
        }
        info!("UI_FONTS {count} system script fonts added ({} MB)", bytes / (1 << 20));
    }
    fallback.state = State::Ready;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_beyond_latin_needs_the_fallback_fonts() {
        assert!(!needs_fallback("Now That's Fancy - Jaeden Camstra"));
        assert!(!needs_fallback("Beyoncé, Mötley Crüe, Ångström"));
        assert!(needs_fallback("酒田市 [feat. THIAGO]") && needs_fallback("Кино") && needs_fallback("🔥"));
    }

    #[test]
    fn the_unix_scan_keeps_regular_script_faces_only() {
        let dir = std::env::temp_dir().join(format!("skate-ui-fonts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("noto")).unwrap();
        for f in ["noto/NotoSansCJK-Regular.ttc", "noto/NotoSans-Bold.ttf", "DejaVuSans.ttf", "Comic.ttf", "NotoSans-Regular.txt"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let mut out = Vec::new();
        collect_unix(&dir, 4, &mut out);
        let mut names: Vec<String> = out.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["DejaVuSans.ttf", "NotoSansCJK-Regular.ttc"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_os_has_ui_font_candidates() {
        assert!(!ui_font_candidates().is_empty());
    }
}
