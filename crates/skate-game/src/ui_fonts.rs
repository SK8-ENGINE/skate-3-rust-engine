//! Text in any script for the game's UI (user ask 2026-11-10, first user: the music caption): the text engine
//! (cosmic-text inside Bevy) only knows the fonts it was given, and its fallback on Windows looks for the system's
//! script fonts by family name (Yu Gothic, Microsoft YaHei UI, Malgun Gothic, Nirmala UI, Segoe UI ...). The first time
//! a text needs a script beyond Latin, these Windows font files are read on a worker thread and added to the text
//! engine's database; text that waits for them asks [`FallbackFonts::ready`]. Windows-only build: no fonts are bundled.
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::text::CosmicFontSystem;

/// The Windows font files whose families cosmic-text's Windows fallback names (cosmic-text `font/fallback/windows.rs`),
/// in `%WINDIR%\Fonts`; missing ones are skipped.
/// One font per script group (2026-11-10: the full list was 120 MB in memory): Latin / Greek / Cyrillic / Arabic /
/// Hebrew (Segoe UI), symbols and emoji, Chinese with Japanese kana (Microsoft YaHei), Japanese (Yu Gothic), Korean
/// (Malgun Gothic), Indic (Nirmala UI), Thai and Lao (Leelawadee UI), African scripts (Ebrima), Cherokee and others
/// (Gadugi), Myanmar.
const FILES: &[&str] = &[
    "segoeui.ttf", "seguisym.ttf", "seguiemj.ttf", "msyh.ttc", "YuGothR.ttc", "malgun.ttf", "Nirmala.ttc", "Nirmala.ttf",
    "LeelawUI.ttf", "ebrima.ttf", "gadugi.ttf", "mmrtext.ttf",
];

enum State {
    Idle,
    Loading(Arc<Mutex<Option<Vec<Vec<u8>>>>>),
    Ready,
}

#[derive(Resource)]
pub(crate) struct FallbackFonts {
    state: State,
}

impl Default for FallbackFonts {
    fn default() -> Self {
        Self { state: State::Idle }
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

    /// Starts reading the fonts (once).
    pub(crate) fn request(&mut self) {
        if !matches!(self.state, State::Idle) {
            return;
        }
        let cell = Arc::new(Mutex::new(None));
        let out = cell.clone();
        let dir = std::env::var_os("WINDIR").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from).join("Fonts");
        let spawned = std::thread::Builder::new().name("fallback-fonts".into()).spawn(move || {
            let fonts: Vec<Vec<u8>> = FILES.iter().filter_map(|f| std::fs::read(dir.join(f)).ok()).collect();
            if let Ok(mut o) = out.lock() {
                *o = Some(fonts);
            }
        });
        self.state = if spawned.is_ok() { State::Loading(cell) } else { State::Ready };
    }
}

/// Adds the read fonts to the text engine's database when they are in.
pub(crate) fn install(mut fallback: ResMut<FallbackFonts>, font_system: Option<ResMut<CosmicFontSystem>>) {
    let State::Loading(cell) = &fallback.state else { return };
    let Some(fonts) = cell.lock().ok().and_then(|mut c| c.take()) else { return };
    if let Some(mut system) = font_system {
        let (count, bytes) = (fonts.len(), fonts.iter().map(Vec::len).sum::<usize>());
        let db = system.0.db_mut();
        for data in fonts {
            db.load_font_data(data);
        }
        info!("UI_FONTS {count} Windows script fonts added ({} MB)", bytes / (1 << 20));
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
}
