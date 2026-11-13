//! The on-screen keyboard (ours) drawn while retail FE 46 is on top, in the retail menus' popup style.
//! Look and pieces: docs/hails-additions/31-retail-menus.md#on-screen-keyboard-look
use crate::apt_scene::{Draw, Fill, Vertex};
use crate::apt_text::{Font, TextAssets};
use crate::extras_host::TextEntryView;
use skate_core::text_entry::Key;

/// The keyboard state the stage writes and its render reads.
pub(crate) struct TextEntryUiPlugin;
impl bevy::prelude::Plugin for TextEntryUiPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.init_resource::<TextEntryView>();
    }
}

/// Retail panel colours (the Extras popup's `controls/panel` draws): the cream paper fills, the paper tint of the
/// body's inner corners and edges, the title band, the body, text on paper, the title.
const CREAM: [f32; 4] = [0.996, 0.918, 0.753, 1.0];
const PAPER_TINT: [f32; 4] = [0.749, 0.710, 0.616, 1.0];
const BAND: [f32; 4] = [0.106, 0.122, 0.133, 1.0];
const BODY: [f32; 4] = [0.145, 0.165, 0.180, 1.0];
const INK: [f32; 4] = [0.129, 0.129, 0.129, 1.0];
const TITLE_TAN: [f32; 4] = [0.753, 0.714, 0.620, 1.0];
const GRAIN_ALPHA: f32 = 0.325;
const PANEL: &str = "assets/data/fe/source/controls/panel/";
/// The selected row / value blue (`textColor` 0x37A5FF in the settings rows).
const SELECTED: [f32; 4] = [0.216, 0.647, 1.0, 1.0];
const FIELD: [f32; 4] = [0.07, 0.08, 0.09, 1.0];
const KEY: [f32; 4] = [0.21, 0.24, 0.27, 1.0];
const KEY_TEXT: [f32; 4] = [0.82, 0.85, 0.87, 1.0];

/// Layout in stage pixels (1280 x 720). The Extras panel: 8 px paper edge, 8 px inner paper, a 40 px title band and
/// a 32 px paper footer.
const WIDTH: f32 = 600.0;
const EDGE: f32 = 8.0;
const BAND_HEIGHT: f32 = 40.0;
const FOOTER: f32 = 32.0;
const PAD: f32 = 20.0;
const FIELD_HEIGHT: f32 = 44.0;
const KEY_HEIGHT: f32 = 38.0;
const GAP: f32 = 6.0;

/// The fonts the keyboard uses, by their names in the menu set (case-insensitive; missing = the set's fallback).
pub(crate) struct KeyboardFonts {
    title: Option<Font>,
    body: Option<Font>,
    /// futuraheavy (Futura Shadow's sharp pass): the prompt labels on the paper, as the movies draw them.
    heavy: Option<Font>,
}

impl KeyboardFonts {
    pub(crate) fn from(assets: &mut TextAssets) -> Self {
        let mut named = |name: &str| assets.font_named(name).and_then(|id| assets.fonts.get(&id).cloned());
        let shadow = named("Futura Shadow");
        Self {
            title: named("Futura Overprint"),
            body: named("Futura Std Medium"),
            heavy: shadow.and_then(|f| f.foreground.map(|f| *f)),
        }
    }
}

fn label(key: Key, shown: Option<char>) -> String {
    match key {
        Key::Char(_) => shown.map(String::from).unwrap_or_default(),
        Key::Space => "Space".into(),
        Key::Backspace => "Delete".into(),
        Key::Shift => "Shift".into(),
        Key::Done => "Done".into(),
        Key::Cancel => "Back".into(),
    }
}

/// A rectangle as two triangles with the UV at its four corners (top left, top right, bottom right, bottom left).
fn quad_uv(x0: f32, y0: f32, x1: f32, y1: f32, uv: [[f32; 2]; 4]) -> Vec<Vertex> {
    let v = |x: f32, y: f32, t: [f32; 2]| Vertex { position: [x, y], uv: t };
    let (a, b, c, d) = (v(x0, y0, uv[0]), v(x1, y0, uv[1]), v(x1, y1, uv[2]), v(x0, y1, uv[3]));
    vec![a.clone(), b, c.clone(), a, c, d]
}

/// UV turns of the panel pieces, as the Extras movie draws them (top left, top right, bottom right, bottom left).
const UPRIGHT: [[f32; 2]; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
const TURN_90: [[f32; 2]; 4] = [[0., 1.], [0., 0.], [1., 0.], [1., 1.]];
const TURN_180: [[f32; 2]; 4] = [[1., 1.], [0., 1.], [0., 0.], [1., 0.]];
const TURN_270: [[f32; 2]; 4] = [[1., 0.], [1., 1.], [0., 1.], [0., 0.]];
const MIRROR_DIAGONAL: [[f32; 2]; 4] = [[0., 0.], [0., 1.], [1., 1.], [1., 0.]];
const MIRROR_ANTI: [[f32; 2]; 4] = [[1., 1.], [1., 0.], [0., 0.], [0., 1.]];

fn piece(out: &mut Vec<Draw>, texture: &str, x0: f32, y0: f32, x1: f32, y1: f32, uv: [[f32; 2]; 4], colour: [f32; 4]) {
    out.push(Draw {
        texture: format!("{PANEL}{texture}.Texture.rgba"),
        vertices: quad_uv(x0, y0, x1, y1, uv),
        multiply: colour,
        add: [0.0; 4],
        fill: Fill::TextureClamped,
    });
}

fn rect(out: &mut Vec<Draw>, x0: f32, y0: f32, x1: f32, y1: f32, colour: [f32; 4]) {
    out.push(Draw { texture: String::new(), vertices: quad_uv(x0, y0, x1, y1, [[0.0; 2]; 4]), multiply: colour, add: [0.0; 4], fill: Fill::Solid });
}

/// The Extras popup frame (`controls/panel`) over the outer rectangle: paper corners and edges, cream inner paper,
/// the title band, the body, the cream footer, the body's tinted inner corners and edges, then the paper grain.
fn panel(out: &mut Vec<Draw>, x0: f32, y0: f32, x1: f32, y1: f32) {
    const WHITE: [f32; 4] = [1.0; 4];
    let (c, e) = (2.0 * EDGE, EDGE);
    // Outer paper corners and edges.
    piece(out, "0012_68", x0, y0, x0 + c, y0 + c, UPRIGHT, WHITE);
    piece(out, "0012_68", x1 - c, y0, x1, y0 + c, TURN_90, WHITE);
    piece(out, "0012_68", x1 - c, y1 - c, x1, y1, TURN_180, WHITE);
    piece(out, "0012_68", x0, y1 - c, x0 + c, y1, TURN_270, WHITE);
    piece(out, "0014_71", x0 + c, y0, x1 - c, y0 + e, UPRIGHT, WHITE);
    piece(out, "0014_71", x0, y0 + c, x0 + e, y1 - c, TURN_270, WHITE);
    piece(out, "0014_71", x1 - e, y0 + c, x1, y1 - c, TURN_90, WHITE);
    piece(out, "0014_71", x0 + c, y1 - c, x1 - c, y1 - e, UPRIGHT, WHITE);
    // Cream inner paper strips and their 8 px corners.
    rect(out, x0 + c, y0 + e, x1 - c, y0 + c, CREAM);
    rect(out, x0 + c, y1 - c, x1 - c, y1 - e, CREAM);
    piece(out, "0015_79", x0 + e, y0 + e, x0 + c, y0 + c, UPRIGHT, WHITE);
    piece(out, "0015_79", x1 - c, y0 + e, x1 - e, y0 + c, TURN_90, WHITE);
    piece(out, "0015_79", x1 - c, y1 - c, x1 - e, y1 - e, TURN_180, WHITE);
    piece(out, "0015_79", x0 + e, y1 - c, x0 + c, y1 - e, TURN_270, WHITE);
    // Title band, body, footer.
    let (bx0, bx1) = (x0 + e, x1 - e);
    let (band_top, band_bottom, footer_top, footer_bottom) = (y0 + c, y0 + c + BAND_HEIGHT, y1 - c - FOOTER, y1 - c);
    rect(out, bx0, band_bottom, bx1, footer_top, BODY);
    rect(out, bx0, band_top, bx1, band_bottom, BAND);
    rect(out, bx0, footer_top, bx1, footer_bottom, CREAM);
    // The body's rounded inner corners and side edges in the paper tint.
    piece(out, "0004_29", bx0, band_top, bx0 + c, band_top + c, MIRROR_DIAGONAL, PAPER_TINT);
    piece(out, "0005_32", bx1 - c, band_top, bx1, band_top + c, [[1., 0.], [0., 0.], [0., 1.], [1., 1.]], PAPER_TINT);
    piece(out, "0005_32", bx0, footer_top - c, bx0 + c, footer_top, [[0., 1.], [1., 1.], [1., 0.], [0., 0.]], PAPER_TINT);
    piece(out, "0004_29", bx1 - c, footer_top - c, bx1, footer_top, MIRROR_ANTI, PAPER_TINT);
    let side = |out: &mut Vec<Draw>, x: f32, left: bool| {
        // Three 128 px strips per side like the movie (the last overlaps to reach the footer).
        let uv = if left { [[1., 1.], [0., 1.], [0., 0.], [1., 0.]] } else { UPRIGHT };
        let (top, bottom) = (band_top, footer_bottom);
        let span = ((bottom - top) / 3.0).max(1.0);
        for (i, texture) in ["0007_4", if left { "0013_7" } else { "0001_10" }, if left { "0001_10" } else { "0013_7" }].iter().enumerate() {
            let s = top + i as f32 * span;
            piece(out, texture, x, s, x + e, (s + span).min(bottom), uv, PAPER_TINT);
        }
    };
    side(out, bx0, true);
    side(out, bx1 - e, false);
    // Paper grain over the whole inner panel, four quadrants.
    let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let grain = [1.0, 1.0, 1.0, GRAIN_ALPHA];
    for (a, b, cx, cy) in [(bx0, y0 + e, mx, my), (bx0, my, mx, y1 - e), (mx, y0 + e, bx1, my), (mx, my, bx1, y1 - e)] {
        piece(out, "0000_1", a, b, cx, cy, UPRIGHT, grain);
    }
}

/// Text in a retail font with its top-left at (x, y) (centred on x when asked); glyphs as `apt_scene` lays a text
/// field's line out (shadow fonts draw their black pass, then the foreground one pixel right).
fn text(out: &mut Vec<Draw>, font: Option<&Font>, s: &str, height: f32, x: f32, y: f32, centred: bool, colour: [f32; 4]) {
    let Some(font) = font else { return };
    let passes: Vec<(&Font, bool, f32)> = match &font.foreground {
        Some(fg) => vec![(font, true, 0.0), (&**fg, false, 1.0)],
        None => vec![(font, false, 0.0)],
    };
    for (font, shadow, advance_x) in passes {
        let (sx, sy) = (font.scale[0] * height, font.scale[1] * height);
        let width = font.width(s, height);
        let mut pen = x + advance_x + font.offset[0] * sx - if centred { width * 0.5 } else { 0.0 };
        let top = y + font.offset[1] * sy;
        let mut vertices = Vec::new();
        for c in s.chars() {
            let Some(g) = font.glyph(c) else { continue };
            let x0 = pen + g.x_offset * sx;
            let y0 = top + (font.ascent - g.y_offset) * sy;
            let [u0, v0, u1, v1] = g.atlas_bounds;
            let (w, h) = (font.size[0] as f32, font.size[1] as f32);
            vertices.extend(quad_uv(x0, y0, x0 + g.width * sx, y0 + g.height * sy, [[u0 / w, v0 / h], [u1 / w, v0 / h], [u1 / w, v1 / h], [u0 / w, v1 / h]]));
            pen = g.x_advance.mul_add(sx, pen);
        }
        if !vertices.is_empty() {
            let multiply = if shadow { [0.0, 0.0, 0.0, colour[3]] } else { colour };
            out.push(Draw { texture: font.texture.clone(), vertices, multiply, add: [0.0; 4], fill: Fill::TextureClamped });
        }
    }
}

fn text_width(font: Option<&Font>, s: &str, height: f32) -> f32 {
    font.map_or(0.0, |f| f.width(s, height))
}

/// The prompts right-aligned on the paper footer like the Extras popup's (button glyph, then its label in ink), the
/// last one ending at `right`.
fn prompts(out: &mut Vec<Draw>, fonts: &KeyboardFonts, items: &[(&str, String)], right: f32, y: f32) {
    let (size, h, after_glyph, between) = (32.0, 19.0, 3.0, 14.0);
    let total: f32 = items.iter().map(|(_, l)| size + after_glyph + text_width(fonts.heavy.as_ref(), l, h)).sum::<f32>()
        + between * (items.len().saturating_sub(1)) as f32;
    let mut x = right - total;
    for (button, label) in items {
        out.push(Draw {
            texture: format!("prompt/{button}"),
            vertices: quad_uv(x, y, x + size, y + size, UPRIGHT),
            multiply: [1.0; 4],
            add: [0.0; 4],
            fill: Fill::TextureClamped,
        });
        x += size + after_glyph;
        text(out, fonts.heavy.as_ref(), label, h, x, y + 6.0, false, INK);
        x += text_width(fonts.heavy.as_ref(), label, h) + between;
    }
}

/// The keyboard's draws, centred on the stage; nothing while it is closed or hidden under a popup.
pub(crate) fn draws(view: &TextEntryView, fonts: &KeyboardFonts, language: &std::collections::BTreeMap<String, String>) -> Vec<Draw> {
    let mut out = Vec::new();
    let Some(keyboard) = view.keyboard.as_ref().filter(|_| !view.hidden) else { return out };
    let rows = &view.layout.rows;
    let grid = rows.len() as f32 * (KEY_HEIGHT + GAP) - GAP;
    let content = PAD + FIELD_HEIGHT + PAD + grid + PAD;
    let height = 2.0 * (2.0 * EDGE) + BAND_HEIGHT + content + FOOTER;
    let (x0, y0) = ((1280.0 - WIDTH) * 0.5, (720.0 - height) * 0.5);
    let (x1, y1) = (x0 + WIDTH, y0 + height);
    panel(&mut out, x0, y0, x1, y1);
    let (left, right) = (x0 + 3.0 * EDGE, x1 - 3.0 * EDGE);
    let band_top = y0 + 2.0 * EDGE;
    text(&mut out, fonts.title.as_ref(), &view.title, 30.0, left, band_top + 6.0, false, TITLE_TAN);
    // The text field with a caret.
    let fy = band_top + BAND_HEIGHT + PAD;
    rect(&mut out, left, fy, right, fy + FIELD_HEIGHT, FIELD);
    rect(&mut out, left, fy + FIELD_HEIGHT - 2.0, right, fy + FIELD_HEIGHT, SELECTED);
    let typed = format!("{}_", keyboard.text);
    // Long entries show their end (the field scrolls with the caret).
    let room = right - left - 2.0 * PAD;
    let mut shown: &str = &typed;
    while text_width(fonts.body.as_ref(), shown, 28.0) > room && !shown.is_empty() {
        shown = &shown[shown.char_indices().nth(1).map_or(shown.len(), |(i, _)| i)..];
    }
    text(&mut out, fonts.body.as_ref(), shown, 28.0, left + PAD, fy + 8.0, false, [1.0; 4]);
    // The key grid: every row spans the body width, its keys share it.
    let lit = keyboard.key(&view.layout);
    let gy = fy + FIELD_HEIGHT + PAD;
    for (r, row) in rows.iter().enumerate() {
        let n = row.len().max(1) as f32;
        let kw = (right - left - GAP * (n - 1.0)) / n;
        let ky = gy + r as f32 * (KEY_HEIGHT + GAP);
        for (c, &key) in row.iter().enumerate() {
            let kx = left + c as f32 * (kw + GAP);
            let highlighted = keyboard.row == r && keyboard.col.min(row.len().saturating_sub(1)) == c && lit == Some(key);
            let on = key == Key::Shift && keyboard.shift;
            rect(&mut out, kx, ky, kx + kw, ky + KEY_HEIGHT, if highlighted { SELECTED } else { KEY });
            if on && !highlighted {
                rect(&mut out, kx, ky + KEY_HEIGHT - 3.0, kx + kw, ky + KEY_HEIGHT, SELECTED);
            }
            let s = label(key, keyboard.shown(key));
            let colour = if highlighted { [1.0; 4] } else if on { SELECTED } else { KEY_TEXT };
            text(&mut out, fonts.body.as_ref(), &s, 22.0, kx + kw * 0.5, ky + 8.0, true, colour);
        }
    }
    // Button help on the paper footer, the retail menus' words where they have them.
    let word = |id: &str, fallback: &str| language.get(id).cloned().unwrap_or_else(|| fallback.to_owned());
    let footer_top = y1 - 2.0 * EDGE - FOOTER;
    prompts(
        &mut out,
        fonts,
        &[
            ("A", word("ID_COMMON_SELECT", "Select")),
            ("X", word("ID_COMMON_DELETE", "Delete")),
            ("Y", "Shift".into()),
            ("Start", "Done".into()),
            ("B", word("ID_COMMON_BACK", "Back")),
        ],
        right,
        footer_top + 4.0,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::text_entry::{Keyboard, KeyboardLayout, TextEntryRequest};

    fn view(text: &str) -> TextEntryView {
        let mut keyboard = Keyboard::open(TextEntryRequest::cheat_code("ID_EXTRA_CHEAT_CODE"));
        keyboard.text = text.into();
        TextEntryView { keyboard: Some(keyboard), layout: KeyboardLayout::default(), title: "Enter Cheat Code".into(), hidden: false }
    }

    /// Without fonts (no menu set) the Extras frame, the field, every key and the five prompts still draw, inside
    /// the stage; exactly one key is lit; hidden (a popup up) or closed draws nothing.
    #[test]
    fn keyboard_draws_frame_keys_and_prompts_on_the_stage() {
        let fonts = KeyboardFonts { title: None, body: None, heavy: None };
        let draws = draws(&view("zombie"), &fonts, &Default::default());
        let keys: usize = KeyboardLayout::default().rows.iter().map(Vec::len).sum();
        let frame = draws.iter().filter(|d| d.texture.starts_with(PANEL)).count();
        assert_eq!(frame, 8 + 4 + 4 + 6 + 4, "corners, edges, inner corners, body corners and sides, grain");
        let keys_drawn = draws.iter().filter(|d| d.fill == Fill::Solid && (d.multiply == KEY || d.multiply == SELECTED)).count();
        assert_eq!(keys_drawn, keys + 1, "every key and the field's line");
        let prompts: Vec<_> = draws.iter().filter_map(|d| d.texture.strip_prefix("prompt/")).collect();
        assert_eq!(prompts, ["A", "X", "Y", "Start", "B"]);
        for d in &draws {
            for v in &d.vertices {
                assert!((0.0..=1280.0).contains(&v.position[0]) && (0.0..=720.0).contains(&v.position[1]), "{:?}", v.position);
            }
        }
        let lit = draws.iter().filter(|d| d.fill == Fill::Solid && d.multiply == SELECTED).count();
        assert_eq!(lit, 2, "the field line and the highlighted key");
        let mut hidden = view("");
        hidden.hidden = true;
        assert!(super::draws(&hidden, &fonts, &Default::default()).is_empty());
        assert!(super::draws(&TextEntryView::default(), &fonts, &Default::default()).is_empty());
    }
}
